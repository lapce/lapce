//! Adaptive streaming chunking — CodeWhale-style two-gear rendering.
//!
//! ## Architecture
//!
//! ```text
//! raw delta → LineBuffer → take_committable → StreamChunker → AdaptiveChunkingPolicy → commit_tick
//! ```
//!
//! ## Two Gear Modes
//!
//! - **Smooth mode**: Drips one chunk per commit tick — calm, readable output.
//! - **CatchUp mode**: Drains all queued chunks per tick — prevents display lag during bursts.
//!
//! ## Hysteresis (prevents gear hunting)
//!
//! ```
//! Enter CatchUp:  queued >= 160 lines OR oldest chunk age >= 1200ms
//! Exit CatchUp:   queued <= 32 lines AND age <= 300ms for >= 250ms
//! Re-entry hold:  250ms cooldown after exit (bypassed for severe: >= 640 lines or >= 4000ms)
//! ```
//!
//! Inspired by CodeWhale's `tui/streaming/` module.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

// ── LineBuffer ───────────────────────────────────────────────────

/// Accumulates incoming text bytes, splitting on newlines.
/// Supports a bypass gate for raw micro-chunk streaming of assistant text.
pub struct LineBuffer {
    buf: String,
    /// When true, all pushes bypass the line buffer and are immediately available.
    bypass_gate: bool,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineBuffer {
    pub fn new() -> Self {
        Self { buf: String::new(), bypass_gate: false }
    }

    /// Push raw text delta. Newline-terminated lines become available.
    pub fn push(&mut self, text: &str) {
        self.buf.push_str(text);
    }

    /// Enable bypass mode — all text is immediately committable (no newline waiting).
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass_gate = bypass;
    }

    /// Take all committable content (complete lines + remainder in bypass mode).
    /// Returns None if nothing is ready.
    pub fn take_committable(&mut self) -> Option<String> {
        if self.bypass_gate {
            if self.buf.is_empty() {
                return None;
            }
            let result = std::mem::take(&mut self.buf);
            return Some(result);
        }

        // Find last newline
        if let Some(last_nl) = self.buf.rfind('\n') {
            let ready: String = self.buf[..=last_nl].to_string();
            self.buf = self.buf[last_nl + 1..].to_string();
            if ready.is_empty() { None } else { Some(ready) }
        } else {
            None
        }
    }

    /// Flush all remaining content (used on stream completion).
    pub fn flush(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.buf))
    }
}

// ── Chunk ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub text: String,
    pub timestamp: Instant,
}

// ── AdaptiveChunkingPolicy ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkingMode {
    /// Drip one chunk per tick.
    Smooth,
    /// Drain all available chunks per tick.
    CatchUp,
}

/// Manages the adaptive two-gear chunking policy with hysteresis.
pub struct AdaptiveChunkingPolicy {
    mode: ChunkingMode,
    /// When we last exited CatchUp mode (for re-entry cooldown).
    exit_catchup_at: Option<Instant>,
    /// How long we've been in "safe" state (for exit hysteresis).
    safe_since: Option<Instant>,
}

impl Default for AdaptiveChunkingPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveChunkingPolicy {
    pub fn new() -> Self {
        Self {
            mode: ChunkingMode::Smooth,
            exit_catchup_at: None,
            safe_since: None,
        }
    }

    pub fn mode(&self) -> ChunkingMode { self.mode }

    /// Re-evaluate the gear based on current queue state.
    pub fn evaluate(&mut self, queued_lines: usize, oldest_age_ms: u64, now: Instant) {
        // Severe backlog: force CatchUp immediately
        if queued_lines >= 640 || oldest_age_ms >= 4000 {
            self.mode = ChunkingMode::CatchUp;
            self.safe_since = None;
            return;
        }

        // Re-entry cooldown: don't re-enter CatchUp within 250ms of exit
        let in_cooldown = self.exit_catchup_at
            .map(|t| now.duration_since(t) < Duration::from_millis(250))
            .unwrap_or(false);

        match self.mode {
            ChunkingMode::Smooth => {
                // Enter CatchUp: backlog pressure
                if queued_lines >= 160 || oldest_age_ms >= 1200 {
                    self.mode = ChunkingMode::CatchUp;
                    self.safe_since = None;
                }
            }
            ChunkingMode::CatchUp => {
                // Check if safe to exit
                let safe = queued_lines <= 32 && oldest_age_ms <= 300;
                if safe {
                    if self.safe_since.is_none() {
                        self.safe_since = Some(now);
                    } else if now.duration_since(self.safe_since.expect("unwrap failed: streaming_chunks.rs:160")) >= Duration::from_millis(250)
                        && !in_cooldown {
                            self.mode = ChunkingMode::Smooth;
                            self.exit_catchup_at = Some(now);
                        }
                } else {
                    self.safe_since = None;
                }
            }
        }
    }
}

// ── StreamChunker ────────────────────────────────────────────────

/// Chunks text into renderable pieces, honoring the adaptive policy.
pub struct StreamChunker {
    queue: VecDeque<TextChunk>,
    policy: AdaptiveChunkingPolicy,
    /// Force permanent Smooth mode (e.g., for low-motion scenarios).
    low_motion: bool,
    /// Total lines accumulated since last tick.
    queued_lines: usize,
    /// Timestamp of the oldest chunk in queue.
    oldest_at: Option<Instant>,
}

impl Default for StreamChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamChunker {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            policy: AdaptiveChunkingPolicy::new(),
            low_motion: false,
            queued_lines: 0,
            oldest_at: None,
        }
    }

    /// Feed new text into the chunker.
    pub fn feed(&mut self, text: &str) {
        let now = Instant::now();
        for line in text.lines() {
            self.queue.push_back(TextChunk {
                text: line.to_string(),
                timestamp: now,
            });
            self.queued_lines += 1;
        }
        if self.oldest_at.is_none() && !self.queue.is_empty() {
            self.oldest_at = Some(now);
        }
    }

    /// Force permanent smooth mode (useful when user is scrolling/reading).
    pub fn set_low_motion(&mut self, low: bool) {
        self.low_motion = low;
    }

    /// Get the number of queued lines waiting to be drained.
    pub fn pending(&self) -> usize { self.queued_lines }

    /// Drain chunks according to the current policy.
    /// Returns the chunks to render this tick.
    pub fn drain(&mut self) -> Vec<TextChunk> {
        if self.queue.is_empty() {
            return Vec::new();
        }

        let now = Instant::now();
        let oldest_age = self.oldest_at
            .map(|t| now.duration_since(t).as_millis() as u64)
            .unwrap_or(0);

        if !self.low_motion {
            self.policy.evaluate(self.queued_lines, oldest_age, now);
        }

        let drained: Vec<TextChunk> = match self.policy.mode() {
            ChunkingMode::Smooth => {
                // Drip one chunk
                self.queue.pop_front().into_iter().collect()
            }
            ChunkingMode::CatchUp => {
                // Drain all available
                std::mem::take(&mut self.queue).into_iter().collect()
            }
        };

        self.queued_lines = self.queued_lines.saturating_sub(drained.len());
        self.oldest_at = self.queue.front().map(|c| c.timestamp);

        drained
    }

    /// Get current gear mode name for debugging.
    pub fn gear_name(&self) -> &'static str {
        match self.policy.mode() {
            ChunkingMode::Smooth => "smooth",
            ChunkingMode::CatchUp => "catchup",
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_line_buffer_basic() {
        let mut lb = LineBuffer::new();
        lb.push("hello\nworld\npar");
        assert_eq!(lb.take_committable(), Some("hello\nworld\n".into()));
        assert_eq!(lb.take_committable(), None);
        lb.push("tial\n");
        assert_eq!(lb.take_committable(), Some("partial\n".into()));
    }

    #[test]
    fn test_line_buffer_bypass() {
        let mut lb = LineBuffer::new();
        lb.set_bypass(true);
        lb.push("stream");
        assert_eq!(lb.take_committable(), Some("stream".into()));
    }

    #[test]
    fn test_line_buffer_flush() {
        let mut lb = LineBuffer::new();
        lb.push("incomplete");
        assert_eq!(lb.flush(), Some("incomplete".into()));
        assert_eq!(lb.flush(), None);
    }

    #[test]
    fn test_chunker_smooth_mode() {
        let mut chunker = StreamChunker::new();
        chunker.feed("line1\nline2\nline3\n");
        let drained = chunker.drain();
        assert_eq!(drained.len(), 1); // Smooth: one per tick
        assert_eq!(drained[0].text, "line1");
    }

    #[test]
    fn test_chunker_catchup_on_backlog() {
        let mut chunker = StreamChunker::new();
        // Feed 200 lines to trigger CatchUp (threshold: 160)
        let text = (0..200).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n");
        chunker.feed(&text);

        // Force age > 1200ms so policy enters CatchUp
        sleep(Duration::from_millis(5));
        let drained = chunker.drain();
        // With 200 lines and CatchUp mode, should drain many
        assert!(drained.len() > 1);
    }

    #[test]
    fn test_adaptive_policy_enter_catchup() {
        let mut policy = AdaptiveChunkingPolicy::new();
        assert_eq!(policy.mode(), ChunkingMode::Smooth);

        policy.evaluate(200, 50, Instant::now());
        assert_eq!(policy.mode(), ChunkingMode::CatchUp);
    }

    #[test]
    fn test_adaptive_policy_severe_force() {
        let mut policy = AdaptiveChunkingPolicy::new();
        policy.evaluate(640, 100, Instant::now());
        assert_eq!(policy.mode(), ChunkingMode::CatchUp);
    }
}
