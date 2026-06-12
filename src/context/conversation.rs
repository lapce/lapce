//! Multi-turn Conversation Manager - Maintains context across turns.
//!
//! This module provides:
//! - Conversation history management
//! - Context summarization for long conversations
//! - Topic tracking and segmentation
//! - Coherence scoring

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A conversation turn.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub turn_id: u64,
    pub role: MessageRole,
    pub content: String,
    pub timestamp: u64,
    pub token_count: usize,
    pub references: Vec<String>,
    pub topic: Option<String>,
    pub summary: Option<String>,
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// A conversation session.
#[derive(Debug, Clone)]
pub struct ConversationSession {
    pub id: String,
    pub turns: Vec<ConversationTurn>,
    pub current_topic: Option<String>,
    pub coherence_score: f32,
    pub total_tokens: usize,
    pub last_turn_at: u64,
}

/// Configuration for conversation management.
#[derive(Debug, Clone)]
pub struct ConversationConfig {
    /// Max turns to keep in memory.
    pub max_turns: usize,
    /// Max tokens before summarization.
    pub max_tokens_before_summary: usize,
    /// Summary frequency (every N turns).
    pub summary_frequency: usize,
    /// Topic stability threshold.
    pub topic_stability_threshold: f32,
    /// Min turns before topic switch.
    pub min_turns_before_topic_switch: usize,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_tokens_before_summary: 8000,
            summary_frequency: 10,
            topic_stability_threshold: 0.7,
            min_turns_before_topic_switch: 3,
        }
    }
}

/// Multi-turn conversation manager.
pub struct ConversationManager {
    config: ConversationConfig,
    sessions: Arc<RwLock<HashMap<String, ConversationSession>>>,
    summaries: Arc<RwLock<HashMap<String, Vec<ConversationSummary>>>>,
}

impl ConversationManager {
    pub fn new(config: ConversationConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            summaries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new conversation session.
    pub async fn create_session(&self, id: String) -> ConversationSession {
        let session = ConversationSession {
            id: id.clone(),
            turns: Vec::new(),
            current_topic: None,
            coherence_score: 1.0,
            total_tokens: 0,
            last_turn_at: current_timestamp(),
        };

        self.sessions.write().await.insert(id, session.clone());
        session
    }

    /// Add a turn to the conversation.
    pub async fn add_turn(&self, session_id: &str, mut turn: ConversationTurn) -> Option<AddTurnResult> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)?;

        let turn_id = session.turns.len() as u64 + 1;
        turn.turn_id = turn_id;

        // Detect topic from content
        let detected_topic = self.detect_topic(&turn.content);
        let topic_changed;

        if let Some(new_topic) = detected_topic {
            if session.current_topic.is_none() {
                session.current_topic = Some(new_topic.clone());
                turn.topic = Some(new_topic);
                topic_changed = false;
            } else {
                let current = session.current_topic.as_ref().expect("unwrap failed: conversation.rs:121");
                let similarity = self.topic_similarity(current, &new_topic);

                if similarity < self.config.topic_stability_threshold {
                    let turns_since_change = session.turns.iter()
                        .filter(|t| t.topic.as_ref() == Some(current))
                        .count();

                    if turns_since_change >= self.config.min_turns_before_topic_switch {
                        session.current_topic = Some(new_topic.clone());
                        turn.topic = Some(new_topic);
                        topic_changed = true;
                    } else {
                        turn.topic = session.current_topic.clone();
                        topic_changed = false;
                    }
                } else {
                    turn.topic = session.current_topic.clone();
                    topic_changed = false;
                }
            }
        } else {
            turn.topic = session.current_topic.clone();
            topic_changed = false;
        }

        // Add turn
        session.turns.push(turn.clone());
        session.total_tokens += turn.token_count;
        session.last_turn_at = turn.timestamp;

        // Evict old turns if necessary
        if session.turns.len() > self.config.max_turns {
            let removed = session.turns.remove(0);
            session.total_tokens = session.total_tokens.saturating_sub(removed.token_count);
        }

        // Compute new coherence score
        session.coherence_score = self.compute_coherence(session);

        // Check if summarization is needed
        let needs_summary = session.total_tokens > self.config.max_tokens_before_summary ||
            (session.turns.len() % self.config.summary_frequency == 0);

        let result = AddTurnResult {
            turn_id,
            topic_changed,
            needs_summary,
            coherence_score: session.coherence_score,
        };

        Some(result)
    }

    /// Maybe update topic based on new turn.
    pub fn maybe_update_topic(&self, turn: &mut ConversationTurn, session: &mut ConversationSession) -> bool {
        // Detect topic from content
        let detected_topic = self.detect_topic(&turn.content);

        if let Some(new_topic) = detected_topic {
            if session.current_topic.is_none() {
                session.current_topic = Some(new_topic.clone());
                turn.topic = Some(new_topic);
                return false;
            }

            let current = session.current_topic.as_ref().expect("unwrap failed: conversation.rs:187");
            let similarity = self.topic_similarity(current, &new_topic);

            if similarity < self.config.topic_stability_threshold {
                // Check if enough turns since last topic change
                let turns_since_change = session.turns.iter()
                    .filter(|t| t.topic.as_ref() == Some(current))
                    .count();

                if turns_since_change >= self.config.min_turns_before_topic_switch {
                    session.current_topic = Some(new_topic.clone());
                    turn.topic = Some(new_topic);
                    return true;
                }
            }
        }

        turn.topic = session.current_topic.clone();
        false
    }

    /// Detect topic from content.
    fn detect_topic(&self, content: &str) -> Option<String> {
        // Simple keyword-based topic detection
        let content_lower = content.to_lowercase();

        let topic_keywords = [
            ("refactoring", vec!["refactor", "extract", "rename", "inline"]),
            ("debugging", vec!["bug", "error", "fix", "debug", "crash"]),
            ("testing", vec!["test", "unit", "integration", "coverage"]),
            ("performance", vec!["slow", "performance", "optimize", "speed"]),
            ("security", vec!["security", "vulnerability", "exploit", "hack"]),
            ("documentation", vec!["doc", "comment", "readme", "spec"]),
            ("database", vec!["sql", "query", "database", "table", "index"]),
            ("api", vec!["api", "endpoint", "route", "request", "response"]),
            ("web", vec!["html", "css", "frontend", "backend", "server"]),
            ("mobile", vec!["ios", "android", "mobile", "app"]),
        ];

        for (topic, keywords) in topic_keywords {
            let matches = keywords.iter()
                .filter(|k| content_lower.contains(*k))
                .count();

            if matches >= 2 {
                return Some(topic.to_string());
            }
        }

        None
    }

    /// Compute topic similarity.
    fn topic_similarity(&self, a: &str, b: &str) -> f32 {
        if a == b {
            return 1.0;
        }

        let words_a: std::collections::HashSet<_> = a.split_whitespace().collect();
        let words_b: std::collections::HashSet<_> = b.split_whitespace().collect();

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Compute coherence score.
    fn compute_coherence(&self, session: &ConversationSession) -> f32 {
        if session.turns.len() < 2 {
            return 1.0;
        }

        let mut coherence_sum = 0.0;
        let mut count = 0;

        for i in 1..session.turns.len() {
            let prev = &session.turns[i - 1];
            let curr = &session.turns[i];

            // Check topic continuity
            let topic_match = if let (Some(t1), Some(t2)) = (&prev.topic, &curr.topic) {
                self.topic_similarity(t1, t2)
            } else {
                1.0
            };

            // Check time proximity (normalized)
            let time_diff = curr.timestamp.saturating_sub(prev.timestamp);
            let time_factor: f32 = if time_diff < 300 { 1.0 } else { 0.8_f32.max(1.0_f32 - time_diff as f32 / 3600.0) };

            coherence_sum += topic_match * time_factor;
            count += 1;
        }

        coherence_sum / count.max(1) as f32
    }

    /// Get conversation context for AI.
    pub async fn get_context(&self, session_id: &str, max_tokens: usize) -> Option<ConversationContext> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)?;

        let mut context_lines = Vec::new();
        let mut total_tokens = 0;

        // Build context from recent turns (newest first)
        for turn in session.turns.iter().rev() {
            let turn_text = format!("[{}]: {}", format!("{:?}", turn.role), turn.content);
            let turn_tokens = turn_text.len() / 4; // Rough estimate

            if total_tokens + turn_tokens > max_tokens {
                break;
            }

            context_lines.push(turn_text);
            total_tokens += turn_tokens;
        }

        context_lines.reverse();

        Some(ConversationContext {
            session_id: session_id.to_string(),
            turns: context_lines,
            current_topic: session.current_topic.clone(),
            coherence_score: session.coherence_score,
            total_turns: session.turns.len(),
            total_tokens,
        })
    }

    /// Summarize old conversation.
    pub async fn summarize(&self, session_id: &str) -> Option<String> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)?;

        if session.turns.is_empty() {
            return None;
        }

        // Generate summary from first N turns
        let summary_turns = session.turns.iter().take(5).collect::<Vec<_>>();
        let summary_text = summary_turns.iter()
            .map(|t| format!("{}: {}", format!("{:?}", t.role), t.content.chars().take(100).collect::<String>()))
            .collect::<Vec<_>>()
            .join(" | ");

        let summary = format!("[Summary of first {} turns]: {}", summary_turns.len(), summary_text);

        // Store summary
        let summaries = self.summaries.read().await;
        let entry = ConversationSummary {
            turn_range: (1usize, session.turns.len()),
            summary: summary.clone(),
            created_at: current_timestamp(),
        };

        drop(summaries);
        self.summaries.write().await
            .entry(session_id.to_string())
            .or_insert_with(Vec::new)
            .push(entry);

        Some(summary)
    }

    /// Get session statistics.
    pub async fn stats(&self, session_id: &str) -> Option<ConversationStats> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)?;

        Some(ConversationStats {
            total_turns: session.turns.len(),
            total_tokens: session.total_tokens,
            current_topic: session.current_topic.clone(),
            coherence_score: session.coherence_score,
            last_turn_at: session.last_turn_at,
        })
    }

    /// Clear session.
    pub async fn clear_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }
}

impl Default for ConversationManager {
    fn default() -> Self {
        Self::new(ConversationConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct AddTurnResult {
    pub turn_id: u64,
    pub topic_changed: bool,
    pub needs_summary: bool,
    pub coherence_score: f32,
}

#[derive(Debug, Clone)]
pub struct ConversationContext {
    pub session_id: String,
    pub turns: Vec<String>,
    pub current_topic: Option<String>,
    pub coherence_score: f32,
    pub total_turns: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub turn_range: (usize, usize),
    pub summary: String,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct ConversationStats {
    pub total_turns: usize,
    pub total_tokens: usize,
    pub current_topic: Option<String>,
    pub coherence_score: f32,
    pub last_turn_at: u64,
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: conversation.rs:421")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let manager = ConversationManager::default();
        let session = manager.create_session("s1".to_string()).await;

        assert_eq!(session.id, "s1");
        assert_eq!(session.turns.len(), 0);
    }

    #[tokio::test]
    async fn test_add_turn() {
        let manager = ConversationManager::default();
        manager.create_session("s1".to_string()).await;

        let turn = ConversationTurn {
            turn_id: 0,
            role: MessageRole::User,
            content: "Help me refactor this function".to_string(),
            timestamp: current_timestamp(),
            token_count: 10,
            references: Vec::new(),
            topic: None,
            summary: None,
        };

        let result = manager.add_turn("s1", turn).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().turn_id, 1);
    }

    #[tokio::test]
    async fn test_topic_detection() {
        let manager = ConversationManager::default();

        let topic = manager.detect_topic("I need to refactor this code and extract a method");
        assert_eq!(topic, Some("refactoring".to_string()));

        let topic = manager.detect_topic("There's a bug in my code that causes it to crash");
        assert_eq!(topic, Some("debugging".to_string()));
    }

    #[tokio::test]
    async fn test_get_context() {
        let manager = ConversationManager::default();
        manager.create_session("s1".to_string()).await;

        let turn = ConversationTurn {
            turn_id: 0,
            role: MessageRole::User,
            content: "Hello".to_string(),
            timestamp: current_timestamp(),
            token_count: 5,
            references: Vec::new(),
            topic: None,
            summary: None,
        };

        manager.add_turn("s1", turn).await;

        let context = manager.get_context("s1", 1000).await;
        assert!(context.is_some());
        assert!(!context.unwrap().turns.is_empty());
    }
}
