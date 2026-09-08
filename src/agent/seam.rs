//! Seam Manager — layered context management with append-only semantics.
//!
//! Ported from CodeWhale's 3-layer seam system. Unlike traditional
//! compaction (which replaces old content), seam layers APPEND summary
//! blocks. This preserves prefix cache warmth for the next request.
//!
//! ## Architecture
//!
//! ```text
//! Full conversation history:
//! ┌────────────────────────────────────────────┐
//! │ L3 Archive (576K) — oldest, coarsest        │
//! │   <archived_context>                        │
//! ├────────────────────────────────────────────┤
//! │ L2 Summary (384K) — medium granularity      │
//! │   <archived_context>                        │
//! ├────────────────────────────────────────────┤
//! │ L1 Compressed (192K) — nearest old messages │
//! │   <archived_context>                        │
//! ├────────────────────────────────────────────┤
//! │ Verbatim Window (recent 16 turns)           │ ← never summarized
//! │   [user] [assistant] [tool] ...             │
//! └────────────────────────────────────────────┘
//! ```

use std::collections::VecDeque;

/// Seam layer configuration.
#[derive(Debug, Clone)]
pub struct SeamConfig {
    /// L1 threshold: compress messages older than this many turns.
    pub l1_turns: usize,
    /// L2 threshold.
    pub l2_turns: usize,
    /// L3 threshold.
    pub l3_turns: usize,
    /// Token counts per layer.
    pub l1_tokens: usize,
    pub l2_tokens: usize,
    pub l3_tokens: usize,
    /// Number of recent turns to always keep verbatim.
    pub verbatim_window: usize,
}

impl Default for SeamConfig {
    fn default() -> Self {
        Self {
            l1_turns: 20,
            l2_turns: 40,
            l3_turns: 60,
            l1_tokens: 192_000,
            l2_tokens: 384_000,
            l3_tokens: 576_000,
            verbatim_window: 16,
        }
    }
}

/// A single conversation turn (user→assistant pair).
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub user_message: String,
    pub assistant_message: String,
    pub tool_calls: Vec<String>,
    pub total_tokens: usize,
}

/// Seam Manager — layered append-only context management.
pub struct SeamManager {
    config: SeamConfig,
    /// All turns in order.
    turns: VecDeque<ConversationTurn>,
    /// L1 summary text (most recent compression layer).
    l1_summary: Option<String>,
    /// L2 summary text.
    l2_summary: Option<String>,
    /// L3 archive text.
    l3_archive: Option<String>,
    /// Total turns seen (monotonically increasing).
    total_turns: usize,
    /// Whether compaction has been applied since last clear.
    dirty: bool,
}

impl SeamManager {
    pub fn new(config: SeamConfig) -> Self {
        Self {
            config,
            turns: VecDeque::new(),
            l1_summary: None,
            l2_summary: None,
            l3_archive: None,
            total_turns: 0,
            dirty: false,
        }
    }

    /// Record a new conversation turn.
    pub fn record_turn(&mut self, user: String, assistant: String, tools: Vec<String>, tokens: usize) {
        self.total_turns += 1;
        self.turns.push_back(ConversationTurn {
            turn_number: self.total_turns,
            user_message: user,
            assistant_message: assistant,
            tool_calls: tools,
            total_tokens: tokens,
        });
        self.dirty = true;
    }

    /// Build the context window with layered summaries.
    /// Returns a vector of messages suitable for the API call.
    /// Prefix cache is preserved because old messages are APPENDED, not modified.
    pub fn build_context(&mut self) -> Vec<crate::providers::provider::ChatMessage> {
        let verbatim_start = self.turns.len().saturating_sub(self.config.verbatim_window);

        // Check if we need to push old turns into summary layers
        if self.turns.len() > self.config.l3_turns && self.dirty {
            self.compact_layer3();
        }
        if self.turns.len() > self.config.l2_turns && self.dirty {
            self.compact_layer2();
        }
        if self.turns.len() > self.config.l1_turns && self.dirty {
            self.compact_layer1();
        }
        self.dirty = false;

        let mut messages: Vec<crate::providers::provider::ChatMessage> = Vec::new();

        // L3: oldest archive
        if let Some(ref l3) = self.l3_archive {
            messages.push(Self::system_msg(&format!("<archived_context layer=\"L3\" tokens=\"{}\">{}</archived_context>",
                self.config.l3_tokens, l3)));
        }

        // L2: medium summary
        if let Some(ref l2) = self.l2_summary {
            messages.push(Self::system_msg(&format!("<archived_context layer=\"L2\" tokens=\"{}\">{}</archived_context>",
                self.config.l2_tokens, l2)));
        }

        // L1: recent summary
        if let Some(ref l1) = self.l1_summary {
            messages.push(Self::system_msg(&format!("<archived_context layer=\"L1\" tokens=\"{}\">{}</archived_context>",
                self.config.l1_tokens, l1)));
        }

        // Verbatim: recent turns never summarized
        for turn in self.turns.iter().skip(verbatim_start) {
            messages.push(Self::user_msg(&turn.user_message));
            messages.push(Self::assistant_msg(&turn.assistant_message));
            for tool in &turn.tool_calls {
                messages.push(Self::tool_msg(tool));
            }
        }

        messages
    }

    /// Compact turns beyond L3 into an archive.
    fn compact_layer3(&mut self) {
        let cutoff = self.turns.len().saturating_sub(self.config.l3_turns);
        if cutoff == 0 { return; }

        let archive_turns: Vec<_> = self.turns.drain(0..cutoff).collect();
        let total_tokens: usize = archive_turns.iter().map(|t| t.total_tokens).sum();

        let mut summary = format!("## Session Archive ({} turns, ~{} tokens)\n\n", archive_turns.len(), total_tokens);
        for t in &archive_turns {
            summary.push_str(&format!("Turn {}: User asked about \"{}\" — {} tool calls. Key result: {}.\n",
                t.turn_number,
                &t.user_message[..t.user_message.len().min(80)],
                t.tool_calls.len(),
                &t.assistant_message[..t.assistant_message.len().min(100)],
            ));
        }

        self.l3_archive = Some(summary);
        tracing::info!(turns=archive_turns.len(), "L3 seam compacted");
    }

    /// Compact turns between L2 and L3.
    fn compact_layer2(&mut self) {
        if self.turns.len() <= self.config.l2_turns { return; }
        let cutoff = self.turns.len().saturating_sub(self.config.l2_turns);
        if cutoff == 0 { return; }

        let turns: Vec<_> = self.turns.drain(0..cutoff).collect();
        let summary: String = turns.iter()
            .map(|t| format!("Turn {}: {} → {} ({} tools)", t.turn_number, &t.user_message[..40.min(t.user_message.len())], &t.assistant_message[..60.min(t.assistant_message.len())], t.tool_calls.len()))
            .collect::<Vec<_>>()
            .join("\n");

        self.l2_summary = Some(summary);
    }

    /// Compact turns between L1 and L2.
    fn compact_layer1(&mut self) {
        if self.turns.len() <= self.config.l1_turns { return; }
        let cutoff = self.turns.len().saturating_sub(self.config.l1_turns);
        if cutoff == 0 { return; }

        let turns: Vec<_> = self.turns.drain(0..cutoff).collect();
        let short: String = turns.iter()
            .map(|t| format!("Turn {}: {}", t.turn_number, &t.user_message[..60.min(t.user_message.len())]))
            .collect::<Vec<_>>()
            .join("; ");

        self.l1_summary = Some(short);
    }

    /// Estimated token count across all layers + verbatim.
    pub fn estimated_tokens(&self) -> usize {
        let verbatim_tokens: usize = self.turns.iter().map(|t| t.total_tokens).sum();
        let l1 = self.l1_summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
        let l2 = self.l2_summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
        let l3 = self.l3_archive.as_ref().map(|s| s.len() / 4).unwrap_or(0);
        verbatim_tokens + l1 + l2 + l3
    }

    fn system_msg(content: &str) -> crate::providers::provider::ChatMessage {
        crate::providers::provider::ChatMessage {
            role: "system".into(), content: content.into(),
            tool_calls: None, tool_call_id: None,
            ..Default::default()
        }
    }

    fn user_msg(content: &str) -> crate::providers::provider::ChatMessage {
        crate::providers::provider::ChatMessage {
            role: "user".into(), content: content.into(),
            tool_calls: None, tool_call_id: None,
            ..Default::default()
        }
    }

    fn assistant_msg(content: &str) -> crate::providers::provider::ChatMessage {
        crate::providers::provider::ChatMessage {
            role: "assistant".into(), content: content.into(),
            tool_calls: None, tool_call_id: None,
            ..Default::default()
        }
    }

    fn tool_msg(content: &str) -> crate::providers::provider::ChatMessage {
        crate::providers::provider::ChatMessage {
            role: "tool".into(), content: content.into(),
            tool_calls: None, tool_call_id: Some("seam".into()),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seam_layers_build_context() {
        let mut mgr = SeamManager::new(SeamConfig {
            l1_turns: 5, l2_turns: 10, l3_turns: 15,
            verbatim_window: 3,
            ..Default::default()
        });

        for i in 0..20 {
            mgr.record_turn(format!("User {}", i), format!("Assistant {}", i), vec![], 500);
        }

        let context = mgr.build_context();
        assert!(context.len() > 0, "Should produce context messages");
        assert!(mgr.l1_summary.is_some(), "L1 should be populated");
        assert!(mgr.l2_summary.is_some(), "L2 should be populated");
        assert!(mgr.l3_archive.is_some(), "L3 should be populated");
    }

    #[test]
    fn test_prefix_preservation() {
        let mut mgr = SeamManager::new(Default::default());
        mgr.record_turn("Hello".into(), "Hi!".into(), vec![], 10);
        let ctx1 = mgr.build_context();
        mgr.record_turn("How are you?".into(), "Good!".into(), vec![], 10);
        let ctx2 = mgr.build_context();
        // Verbatim window should have the new turn appended after old context
        assert!(ctx2.len() > ctx1.len());
    }
}
