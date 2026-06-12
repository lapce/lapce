//! Message compression & context window management.
//!
//! Inspired by Claude Code's three-layer compaction system:
//!   1. Micro-compact: replace old tool outputs with stubs (zero API calls)
//!   2. Session-memory compact: replace old messages with summary file
//!   3. LLM compact: ask a model to summarize (expensive)
//!
//! This module implements layers 1-2 (layer 3 requires model calls, TBD).
//!
//! Also provides context threshold tracking (warning/error/block levels).

use crate::providers::provider::ChatMessage;
use crate::agent::context_optimizer::SmartSummarizer;
use std::time::{Duration, Instant};

// ============================================================================
// Context Thresholds
// ============================================================================

/// Context window thresholds (in estimated token count).
#[derive(Debug, Clone)]
pub struct ContextThresholds {
    /// Approximate total context window size for the active model.
    pub context_window: usize,
    /// Trigger auto-compact at this threshold.
    pub auto_compact_at: usize,
    /// Show warning to user at this threshold.
    pub warning_at: usize,
    /// Block further messages at this threshold.
    pub error_at: usize,
}

impl Default for ContextThresholds {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            auto_compact_at: 100_000,   // ~78% of window
            warning_at: 108_000,         // ~84% of window
            error_at: 120_000,           // ~94% of window
        }
    }
}

impl ContextThresholds {
    /// Create thresholds for a given context window size.
    pub fn for_window(window: usize) -> Self {
        Self {
            context_window: window,
            auto_compact_at: (window as f64 * 0.78) as usize,
            warning_at: (window as f64 * 0.84) as usize,
            error_at: (window as f64 * 0.94) as usize,
        }
    }

    /// Check current token count against thresholds.
    pub fn check(&self, current_tokens: usize) -> ContextLevel {
        if current_tokens >= self.error_at {
            ContextLevel::Error
        } else if current_tokens >= self.warning_at {
            ContextLevel::Warning
        } else if current_tokens >= self.auto_compact_at {
            ContextLevel::Compact
        } else {
            ContextLevel::Normal
        }
    }
}

/// Current context level based on token usage.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextLevel {
    Normal,
    Compact,
    Warning,
    Error,
}

// ============================================================================
// Micro-Compact
// ============================================================================

/// Replace old tool outputs with stubs to save tokens.
/// Zero API calls — pure text manipulation.
///
/// Claude Code's micro-compact replaces tool outputs older than
/// the last assistant message with `[Old tool result content cleared]`.
///
/// The tools whose outputs can be stubbed:
///   FileRead, Bash, Grep, Glob, WebSearch, WebFetch, FileEdit, FileWrite
pub struct MicroCompactor {
    /// Messages to keep intact (most recent N).
    keep_tail: usize,
    /// Minimum age of tool outputs to stub (since last assistant msg).
    min_age_since_last_assistant: Duration,
    /// Last compaction timestamp.
    last_compaction: Option<Instant>,
    /// Minimum interval between compactions.
    min_interval: Duration,
    /// Smart summarizer for tool outputs.
    summarizer: SmartSummarizer,
}

impl Default for MicroCompactor {
    fn default() -> Self {
        Self {
            keep_tail: 20,
            min_age_since_last_assistant: Duration::from_secs(30),
            last_compaction: None,
            min_interval: Duration::from_secs(60),
            summarizer: SmartSummarizer::default(),
        }
    }
}

impl MicroCompactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of recent messages to keep intact during compaction.
    /// Useful for testing with small message sets.
    pub fn with_keep_tail(mut self, n: usize) -> Self {
        self.keep_tail = n;
        self
    }

    /// Attempt to micro-compact the message list.
    /// Returns the compacted list and the number of messages stubbed.
    ///
    /// Strategy (Claude Code-style):
    ///   1. Find the index of the last assistant message
    ///   2. Stub tool outputs (role="tool") before that index
    ///   3. Keep the most recent `keep_tail` messages intact
    pub fn compact(&mut self, messages: &[ChatMessage], current_tokens: usize, thresholds: &ContextThresholds) -> (Vec<ChatMessage>, usize) {
        let now = Instant::now();

        // Rate-limit compactions
        if let Some(last) = self.last_compaction {
            if now.duration_since(last) < self.min_interval {
                return (messages.to_vec(), 0);
            }
        }

        // Only compact if at or above auto-compact threshold
        if current_tokens < thresholds.auto_compact_at {
            return (messages.to_vec(), 0);
        }

        self.last_compaction = Some(now);

        if messages.len() <= self.keep_tail {
            return (messages.to_vec(), 0);
        }

        let mut result = messages.to_vec();
        let mut stubbed = 0usize;

        // Find the last assistant message (cutoff point)
        let last_assistant_idx = messages.iter()
            .rposition(|m| m.role == "assistant")
            .unwrap_or(0);

        // Stub tool outputs before the last assistant
        let stub_limit = messages.len().saturating_sub(self.keep_tail).min(last_assistant_idx);
        for i in 0..stub_limit {
            if result[i].role == "tool" {
                result[i].content = "[Old tool result cleared by micro-compact]".to_string();
                stubbed += 1;
            }
        }

        if stubbed > 0 {
            tracing::info!(
                stubbed,
                kept = self.keep_tail,
                total = messages.len(),
                "Micro-compact: stubbed old tool outputs"
            );
        }

        (result, stubbed)
    }

    /// Smart compact: use summarizer to extract key info from old tool outputs
    /// instead of just stubbing them. This preserves more useful context within
    /// the same token budget compared to the basic micro-compact.
    ///
    /// Returns (compacted_messages, stubbed_count, tokens_saved_estimate).
    pub fn smart_compact(
        &mut self,
        messages: &[ChatMessage],
        current_tokens: usize,
        thresholds: &ContextThresholds,
    ) -> (Vec<ChatMessage>, usize, usize) {
        let now = Instant::now();

        if let Some(last) = self.last_compaction {
            if now.duration_since(last) < self.min_interval {
                return (messages.to_vec(), 0, 0);
            }
        }

        if current_tokens < thresholds.auto_compact_at {
            return (messages.to_vec(), 0, 0);
        }

        self.last_compaction = Some(now);

        if messages.len() <= self.keep_tail {
            return (messages.to_vec(), 0, 0);
        }

        let mut result = messages.to_vec();
        let mut stubbed = 0usize;
        let mut tokens_saved = 0usize;

        let last_assistant_idx = messages.iter()
            .rposition(|m| m.role == "assistant")
            .unwrap_or(0);

        let stub_limit = messages.len().saturating_sub(self.keep_tail).min(last_assistant_idx);

        for i in 0..stub_limit {
            if result[i].role == "tool" {
                let original_len = result[i].content.len();
                // Use smart summarizer to extract key info
                let summary = self.summarizer.summarize(&result[i].content, "generic");
                let saved = original_len.saturating_sub(summary.summary.len());
                tokens_saved += saved / 4; // ~4 chars per token

                if summary.compression_ratio < 0.5 {
                    // Significant compression — use summary
                    result[i].content = summary.summary;
                } else {
                    // Minimal compression — just stub
                    result[i].content = "[Old tool result cleared by smart-compact]".to_string();
                }
                stubbed += 1;
            }
        }

        if stubbed > 0 {
            tracing::info!(
                stubbed,
                tokens_saved,
                total = messages.len(),
                "Smart-compact: summarized old tool outputs"
            );
        }

        (result, stubbed, tokens_saved)
    }

    /// Estimate token count from messages (rough approximation).
    /// Real implementation would use tiktoken or similar.
    pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
        messages.iter().map(|m| {
            // ~4 characters per token for English text
            let text_tokens = m.content.len() / 4;
            // Overhead per message
            text_tokens + 8
        }).sum()
    }

    /// Get the minimum age of tool outputs before they can be stubbed.
    pub fn min_age_since_last_assistant(&self) -> Duration {
        self.min_age_since_last_assistant
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: "tool".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some("call_1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn test_micro_compact_stubs_old_tools() {
        let messages = vec![
            ChatMessage { role: "system".into(), content: "System".into(), tool_calls: None, tool_call_id: None, ..Default::default() },
            ChatMessage { role: "user".into(), content: "Hi".into(), tool_calls: None, tool_call_id: None, ..Default::default() },
            tool_msg("Very long tool output that uses many tokens and should be stubbed to save context space because it's an old tool result from many turns ago"),
            ChatMessage { role: "assistant".into(), content: "Got it!".into(), tool_calls: None, tool_call_id: None, ..Default::default() },
            ChatMessage { role: "user".into(), content: "Thanks".into(), tool_calls: None, tool_call_id: None, ..Default::default() },
        ];

        let mut compactor = MicroCompactor::new().with_keep_tail(2);
        let thresholds = ContextThresholds::default();

        let (result, stubbed) = compactor.compact(&messages, 150000, &thresholds);
        assert!(stubbed > 0, "Should stub at least one old tool output");
        assert_eq!(result[2].content, "[Old tool result cleared by micro-compact]");
    }

    #[test]
    fn test_context_thresholds() {
        let t = ContextThresholds::for_window(128_000);
        assert_eq!(t.check(50_000), ContextLevel::Normal);
        assert_eq!(t.check(105_000), ContextLevel::Compact);
        assert_eq!(t.check(110_000), ContextLevel::Warning);
        assert_eq!(t.check(125_000), ContextLevel::Error);
    }
}
