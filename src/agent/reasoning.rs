//! Chain-of-Thought reasoning visualization — DeepSeek-TUI inspired.
//!
//! Parses and displays the model's reasoning process in real-time.
//! Supports DeepSeek R1/V3 reasoning_content field and general think blocks.
//!
//! ## Architecture
//!
//! ```text
//! SSE stream → parse chunks → if reasoning_content: emit ReasoningToken
//!                            → if content: emit ContentToken
//! TUI: render ReasoningToken in grey italic, ContentToken normally
//! ```

use tokio::sync::mpsc;

/// A token from the reasoning stream — either thinking or content.
#[derive(Debug, Clone)]
pub enum ReasoningToken {
    /// Model's internal reasoning (DeepSeek R1/V3 reasoning_content field).
    Think { text: String },
    /// Model's visible output (normal content).
    Content { text: String },
    /// End of reasoning section.
    ThinkEnd,
    /// Error during reasoning.
    Error { message: String },
}

/// Message role for reasoning display.
#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningRole {
    /// Normal assistant message.
    Assistant,
    /// Model is thinking (deep reasoning mode).
    Thinking,
}

/// Extract reasoning tokens from a streaming SSE response.
///
/// This wraps an mpsc::Receiver<StreamChunk> and splits out
/// reasoning_content (thinking) from regular content.
pub struct ReasoningParser {
    /// Buffer for accumulating partial JSON lines.
    buffer: String,
    /// Whether we're currently in a thinking block.
    in_think_block: bool,
    /// Accumulated thinking content for logging.
    think_content: String,
    /// Accumulated regular content.
    output_content: String,
}

impl Default for ReasoningParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ReasoningParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_think_block: false,
            think_content: String::new(),
            output_content: String::new(),
        }
    }

    /// Parse an SSE line into reasoning tokens.
    /// Returns None if no token emitted (empty line, comment, etc.).
    pub fn parse_line(&mut self, line: &str) -> Option<ReasoningToken> {
        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        let data = line.strip_prefix("data: ")?;

        if data == "[DONE]" {
            if self.in_think_block {
                self.in_think_block = false;
                return Some(ReasoningToken::ThinkEnd);
            }
            return None;
        }

        let json: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => {
                // Partial JSON — accumulate
                self.buffer.push_str(data);
                return None;
            }
        };

        // Check for reasoning_content (DeepSeek R1/V3 specific)
        let delta = &json["choices"][0]["delta"];
        let reasoning = delta["reasoning_content"].as_str();
        let content = delta["content"].as_str();

        if let Some(reasoning_text) = reasoning {
            if !reasoning_text.is_empty() {
                self.in_think_block = true;
                self.think_content.push_str(reasoning_text);
                return Some(ReasoningToken::Think {
                    text: reasoning_text.to_string(),
                });
            }
        }

        if let Some(content_text) = content {
            if !content_text.is_empty() {
                // Transition from thinking to content
                if self.in_think_block {
                    self.in_think_block = false;
                    let _end_token = ReasoningToken::ThinkEnd;
                    // Also emit content
                    self.output_content.push_str(content_text);
                    // Return content (think end already handled)
                }
                self.output_content.push_str(content_text);
                return Some(ReasoningToken::Content {
                    text: content_text.to_string(),
                });
            }
        }

        None
    }

    /// Get accumulated thinking content.
    pub fn think_summary(&self) -> &str {
        &self.think_content
    }

    /// Get accumulated output content.
    pub fn output_summary(&self) -> &str {
        &self.output_content
    }

    /// Clear accumulated state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.in_think_block = false;
        self.think_content.clear();
        self.output_content.clear();
    }
}

/// Wrapper that processes a StreamChunk receiver into ReasoningTokens.
///
/// Usage:
/// ```no_run
/// let mut parser = ReasoningParser::new();
/// while let Some(chunk) = stream_rx.recv().await {
///     if let Some(token) = parser.parse_line(&chunk.content) {
///         match token {
///             ReasoningToken::Think { text } => /* render thinking UI */,
///             ReasoningToken::Content { text } => /* render output */,
///             _ => {}
///         }
///     }
/// }
/// ```
pub fn reasoning_stream(
    mut raw_rx: mpsc::Receiver<crate::providers::provider::StreamChunk>,
) -> (mpsc::Receiver<ReasoningToken>, mpsc::Sender<ReasoningToken>) {
    let (tx, rx) = mpsc::channel(128);
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        let mut parser = ReasoningParser::new();
        while let Some(chunk) = raw_rx.recv().await {
            if chunk.is_done {
                if parser.in_think_block {
                    let _ = tx_clone.send(ReasoningToken::ThinkEnd).await;
                }
                break;
            }
            if let Some(token) = parser.parse_line(&chunk.content) {
                let _ = tx_clone.send(token).await;
            }
        }
    });

    (rx, tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reasoning_content() {
        let mut parser = ReasoningParser::new();
        let line = r#"data: {"choices":[{"delta":{"reasoning_content":"Let me think about this..."}}]}"#;
        let token = parser.parse_line(line).unwrap();
        match token {
            ReasoningToken::Think { text } => assert!(text.contains("think")),
            _ => panic!("Expected Think token"),
        }
    }

    #[test]
    fn test_parse_content() {
        let mut parser = ReasoningParser::new();
        let line = r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#;
        let token = parser.parse_line(line).unwrap();
        match token {
            ReasoningToken::Content { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Content token"),
        }
    }

    #[test]
    fn test_parse_done() {
        let mut parser = ReasoningParser::new();
        // In think mode
        parser.in_think_block = true;
        let line = "data: [DONE]";
        let token = parser.parse_line(line).unwrap();
        match token {
            ReasoningToken::ThinkEnd => {}
            _ => panic!("Expected ThinkEnd"),
        }
        assert!(!parser.in_think_block);
    }
}
