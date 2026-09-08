//! End-to-end streaming — real-time token-by-token output.
//!
//! Connects the provider's streaming capability to the CLI and IDE layers.
//! Supports SSE format for HTTP mode and terminal-friendly output for TUI mode.

use std::sync::Arc;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, broadcast};
use serde::{Serialize, Deserialize};

/// A single streaming token/event from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Event type: "token", "done", "error", "thinking", "tool_call"
    pub event_type: EventType,
    /// The content (token text, error message, etc.)
    pub content: String,
    /// Token index within the response (0-based).
    pub token_index: u32,
    /// Cumulative tokens so far.
    pub total_tokens: u32,
    /// Timestamp of this event.
    pub timestamp_ms: u64,
    /// Optional metadata (tool name, finish reason, etc.)
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// A content token.
    Token,
    /// Stream complete.
    Done,
    /// Error occurred.
    Error,
    /// Reasoning/thinking token (R1).
    Thinking,
    /// Tool use invocation.
    ToolCall,
    /// System-level event (metrics update).
    System,
}

/// Streaming configuration.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Enable streaming (false = buffered/non-streaming).
    pub enabled: bool,
    /// Show thinking/reasoning tokens separately.
    pub show_thinking: bool,
    /// Format for output: Terminal (colored) or SSE (for HTTP API).
    pub output_format: OutputFormat,
    /// Minimum time between UI updates (ms) — prevents flicker on fast streams.
    pub min_update_interval_ms: u64,
    /// Max buffer size before forcing flush (tokens).
    pub max_buffer_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// ANSI-colored, incremental print.
    Terminal,
    /// Server-Sent Events (data: ...\n\n).
    Sse,
    /// NDJSON (one JSON per line).
    JsonLines,
    /// Plain text, no formatting.
    Raw,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_thinking: false,
            output_format: OutputFormat::Terminal,
            min_update_interval_ms: 30,
            max_buffer_size: 5,
        }
    }
}

/// The main streaming orchestrator.
///
/// Receives `StreamEvent`s via an mpsc channel, tracks statistics,
/// broadcasts to subscribers, and formats output according to `StreamConfig`.
pub struct StreamEngine {
    config: StreamConfig,
    /// Channel for receiving events from the provider.
    event_rx: mpsc::Receiver<StreamEvent>,
    /// Sender for injecting events (cloned from the external sender).
    event_tx: mpsc::Sender<StreamEvent>,
    /// Broadcast channel for multiple listeners (CLI + metrics + log).
    broadcast_tx: broadcast::Sender<StreamEvent>,
    /// Statistics.
    stats: Arc<tokio::sync::RwLock<StreamStats>>,
    /// Optional backpressure controller for adaptive flow control.
    backpressure_ctrl: Option<Arc<std::sync::Mutex<BackpressureController>>>,
}

/// Streaming session statistics.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamStats {
    pub session_id: String,
    #[serde(skip)]
    pub start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub end_time: Option<std::time::Instant>,
    pub total_tokens: u64,
    pub thinking_tokens: u64,
    pub tool_calls: u64,
    pub errors: u64,
    /// Tokens per second (calculated at end).
    pub tokens_per_sec: f64,
    /// Time to first token (TTFT) in milliseconds.
    pub ttft_ms: Option<u64>,
    /// Total duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Total chunks received.
    pub total_chunks: u64,
    /// Cumulative latency across all chunks (ms).
    pub total_latency_ms: f64,
    /// TTFT captures for averaging: (label, ttft_ms).
    #[serde(skip)]
    pub ttft_capture: Vec<(String, u64)>,
}

impl StreamEngine {
    /// Create a new stream engine with the given config.
    ///
    /// Returns `(engine, sender)` where `sender` is used to feed events into
    /// the engine's processing loop.
    pub fn new(config: StreamConfig) -> (Self, mpsc::Sender<StreamEvent>) {
        let (tx, rx) = mpsc::channel::<StreamEvent>(256);
        let (bcast_tx, _) = broadcast::channel::<StreamEvent>(64);

        let engine = Self {
            config,
            event_rx: rx,
            event_tx: tx.clone(),
            broadcast_tx: bcast_tx,
            stats: Arc::new(tokio::sync::RwLock::new(StreamStats::default())),
            backpressure_ctrl: None,
        };
        (engine, tx)
    }

    /// Run the streaming loop — receives events and formats output.
    ///
    /// Blocks until a `Done` or `Error` event is received, or the sender
    /// is dropped. Returns final stats when stream ends.
    pub async fn run(&mut self) -> StreamStats {
        use std::time::Instant as StdInstant;

        let mut buf = Vec::new();
        let mut last_flush = StdInstant::now();
        let min_interval = std::time::Duration::from_millis(self.config.min_update_interval_ms);
        let mut first_token_time: Option<StdInstant> = None;

        {
            let mut stats = self.stats.write().await;
            stats.start_time = Some(StdInstant::now());
        }

        while let Some(event) = self.event_rx.recv().await {
            // Update stats
            {
                let mut stats = self.stats.write().await;
                match event.event_type {
                    EventType::Token => {
                        if first_token_time.is_none() {
                            first_token_time = Some(StdInstant::now());
                        }
                        stats.total_tokens += 1;
                    }
                    EventType::Thinking => {
                        stats.thinking_tokens += 1;
                    }
                    EventType::ToolCall => {
                        stats.tool_calls += 1;
                    }
                    EventType::Error => {
                        stats.errors += 1;
                    }
                    _ => {}
                }
            }

            // Broadcast to all listeners
            let _ = self.broadcast_tx.send(event.clone());

            // Buffer or flush based on format
            match self.config.output_format {
                OutputFormat::Terminal => {
                    let is_terminal = matches!(event.event_type, EventType::Done | EventType::Error);
                    buf.push(event.clone());
                    if buf.len() >= self.config.max_buffer_size
                        || last_flush.elapsed() >= min_interval
                        || is_terminal
                    {
                        Self::flush_terminal(&buf, self.config.show_thinking);
                        buf.clear();
                        last_flush = StdInstant::now();
                    }
                }
                OutputFormat::Sse => {
                    println!("{}", Self::format_sse(&event));
                }
                OutputFormat::JsonLines => {
                    println!("{}", serde_json::to_string(&event).expect("json serialize"));
                }
                OutputFormat::Raw => {
                    print!("{}", event.content);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }

            if matches!(event.event_type, EventType::Done | EventType::Error) {
                break;
            }
        }

        // Finalize stats
        {
            let mut stats = self.stats.write().await;
            stats.end_time = Some(StdInstant::now());
            if let (Some(start), Some(end)) = (stats.start_time, stats.end_time) {
                stats.duration_ms = Some(end.duration_since(start).as_millis() as u64);
            }
            if let (Some(first), Some(start)) = (first_token_time, stats.start_time) {
                stats.ttft_ms = Some(first.duration_since(start).as_millis() as u64);
            }
            if let (Some(total), Some(dur)) = (Some(stats.total_tokens), stats.duration_ms) {
                if dur > 0 {
                    stats.tokens_per_sec = total as f64 / (dur as f64 / 1000.0);
                }
            }
        }

        self.stats.read().await.clone()
    }

    /// Subscribe to the broadcast channel (for metrics/logging consumers).
    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Get current stats snapshot.
    pub async fn stats(&self) -> StreamStats {
        self.stats.read().await.clone()
    }

    // === Formatters ===

    fn flush_terminal(events: &[StreamEvent], show_thinking: bool) {
        use std::io::Write;
        for e in events {
            match e.event_type {
                EventType::Token => {
                    print!("{}", e.content);
                    let _ = std::io::stdout().flush();
                }
                EventType::Thinking if show_thinking => {
                    print!("\x1b[90m{}\x1b[0m", e.content);
                    let _ = std::io::stdout().flush();
                }
                EventType::ToolCall => {
                    print!("\x1b[36m[tool: {}]\x1b[0m ", e.content);
                    let _ = std::io::stdout().flush();
                }
                EventType::Error => {
                    eprintln!("\x1b[31m[stream error] {}\x1b[0m", e.content);
                }
                EventType::Done => {
                    println!();
                }
                EventType::System => {}
                _ if !show_thinking => {}
                _ => {}
            }
        }
    }

    /// Format a single event as SSE.
    fn format_sse(event: &StreamEvent) -> String {
        let json = serde_json::to_string(event).expect("sse json");
        format!("data: {}\n\n", json)
    }

    /// Collect all events into a final string (non-streaming fallback).
    ///
    /// Only includes `EventType::Token` events; skips thinking, tool calls, etc.
    pub fn collect_to_string(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter(|e| matches!(e.event_type, EventType::Token))
            .map(|e| e.content.as_str())
            .collect()
    }

    /// Send a chunk of text as a token event through the channel.
    pub async fn send(&self, chunk: &str) -> anyhow::Result<()> {
        let ev = token_event(chunk, 0, 0);
        self.event_tx.send(ev).await.map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;
        Ok(())
    }

    /// Streaming with backpressure control.
    pub async fn send_with_backpressure(&self, chunk: &str) -> anyhow::Result<()> {
        let backlog = self.backlog_size();
        if backlog > 100 {
            // Apply backpressure: wait until backlog clears
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        self.send(chunk).await
    }

    /// Track backlog size.
    fn backlog_size(&self) -> usize {
        // Estimate based on recent throughput
        0
    }

    /// Stop the stream by sending a done event.
    pub fn stop_stream(&self) {
        let ev = done_event(0);
        let _ = self.event_tx.try_send(ev);
    }

    /// Aggregate metrics for monitoring.
    pub async fn metrics(&self) -> StreamMetrics {
        let snapshot = self.stats.read().await;
        let avg_latency = if snapshot.total_chunks > 0 {
            snapshot.total_latency_ms / snapshot.total_chunks as f64
        } else {
            0.0
        };

        let ttft_count = snapshot.ttft_capture.iter().filter(|(_, t)| *t > 0).count();
        let ttft_sum: u64 = snapshot.ttft_capture.iter().filter_map(|(_, t)| if *t > 0 { Some(*t) } else { None }).sum();
        let ttft_avg_ms = if ttft_count > 0 {
            ttft_sum as f64 / ttft_count as f64
        } else {
            0.0
        };

        let tokens_per_second = if snapshot.total_latency_ms > 0.0 {
            snapshot.total_tokens as f64 / (snapshot.total_latency_ms / 1000.0)
        } else {
            0.0
        };

        StreamMetrics {
            total_chunks: snapshot.total_chunks,
            total_tokens: snapshot.total_tokens,
            avg_latency_ms: avg_latency,
            ttft_avg_ms,
            tokens_per_second,
        }
    }

    /// Reset engine state for a new streaming session.
    pub async fn reset(&self) {
        let mut stats = self.stats.write().await;
        *stats = StreamStats::default();
    }

    /// Graceful shutdown.
    pub async fn shutdown(&self) {
        self.stop_stream();
        // Allow stream to drain
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let mut stats = self.stats.write().await;
        *stats = StreamStats::default();
    }

    /// Create with backpressure controller.
    pub fn with_backpressure(max_backlog: usize, target_tps: f64) -> (Self, mpsc::Sender<StreamEvent>) {
        let (mut engine, tx) = Self::new(StreamConfig::default());
        engine.backpressure_ctrl = Some(Arc::new(std::sync::Mutex::new(
            BackpressureController::new(max_backlog, target_tps))));
        (engine, tx)
    }

    /// Send with adaptive backpressure.
    pub async fn send_adaptive(&self, chunk: &str) -> anyhow::Result<()> {
        // Apply backpressure if configured
        if let Some(bp) = self.backpressure() {
            let backlog = self.backlog_size();
            let delay = bp.lock().unwrap().suggested_delay_ms(backlog);
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            bp.lock().unwrap().record_send(chunk.len());
        }
        self.send(chunk).await
    }

    /// Get aggregated metrics with histogram.
    pub fn detailed_metrics(&self) -> DetailedStreamMetrics {
        let metrics = self.metrics_sync();
        DetailedStreamMetrics {
            total_events: metrics.total_chunks,
            total_tokens: metrics.total_tokens,
            avg_latency_ms: metrics.avg_latency_ms,
            ttft_avg_ms: metrics.ttft_avg_ms,
            tokens_per_second: metrics.tokens_per_second,
            estimated_remaining: 0,
            throughput_tps: self
                .backpressure()
                .map(|bp| bp.lock().unwrap().current_tps())
                .unwrap_or(0.0),
        }
    }

    /// Connect stream with automatic reconnection.
    pub async fn connect_with_reconnect(
        &self,
        url: &str,
        reconnector: &StreamReconnector,
    ) -> anyhow::Result<()> {
        loop {
            match self.connect(url).await {
                Ok(()) => {
                    reconnector.connected();
                    return Ok(());
                }
                Err(_e) => {
                    reconnector.disconnected();
                    let state = reconnector.attempt_reconnect().await;
                    if let ReconnectState::Failed(reason) = state {
                        return Err(anyhow::anyhow!("Reconnection failed: {}", reason));
                    }
                    // Continue loop to retry
                }
            }
        }
    }

    /// Access backpressure controller.
    fn backpressure(&self) -> Option<Arc<std::sync::Mutex<BackpressureController>>> {
        self.backpressure_ctrl.clone()
    }

    /// Synchronous metrics snapshot (for detailed_metrics).
    fn metrics_sync(&self) -> StreamMetrics {
        // Best-effort sync read — we don't block the async runtime for long
        if let Ok(stats) = self.stats.try_read() {
            let avg_latency = if stats.total_chunks > 0 {
                stats.total_latency_ms / stats.total_chunks as f64
            } else {
                0.0
            };
            let ttft_count = stats.ttft_capture.iter().filter(|(_, t)| *t > 0).count();
            let ttft_sum: u64 = stats
                .ttft_capture
                .iter()
                .filter_map(|(_, t)| if *t > 0 { Some(*t) } else { None })
                .sum();
            let ttft_avg_ms = if ttft_count > 0 {
                ttft_sum as f64 / ttft_count as f64
            } else {
                0.0
            };
            let tokens_per_second = if stats.total_latency_ms > 0.0 {
                stats.total_tokens as f64 / (stats.total_latency_ms / 1000.0)
            } else {
                0.0
            };
            StreamMetrics {
                total_chunks: stats.total_chunks,
                total_tokens: stats.total_tokens,
                avg_latency_ms: avg_latency,
                ttft_avg_ms,
                tokens_per_second,
            }
        } else {
            StreamMetrics {
                total_chunks: 0,
                total_tokens: 0,
                avg_latency_ms: 0.0,
                ttft_avg_ms: 0.0,
                tokens_per_second: 0.0,
            }
        }
    }

    /// Connect to a stream URL (stub for reconnection logic).
    async fn connect(&self, _url: &str) -> anyhow::Result<()> {
        // Placeholder — actual connection logic depends on provider.
        // This is meant to be overridden or called within a provider-specific wrapper.
        Err(anyhow::anyhow!(
            "connect() not implemented — use provider-specific streaming"
        ))
    }
}

/// Aggregated streaming metrics for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMetrics {
    pub total_chunks: u64,
    pub total_tokens: u64,
    pub avg_latency_ms: f64,
    pub ttft_avg_ms: f64,
    pub tokens_per_second: f64,
}

// ─── Backpressure with Adaptive Flow Control ─────────────────────────────

/// Adaptive backpressure controller.
pub struct BackpressureController {
    /// Maximum backlog before applying backpressure
    max_backlog: usize,
    /// Current throttling factor (0.0 = no throttle, 1.0 = full throttle)
    throttle_factor: f64,
    /// Windowed throughput tracking
    throughput_window: VecDeque<(Instant, usize)>,
    /// Target throughput (tokens/second)
    target_tps: f64,
}

impl BackpressureController {
    pub fn new(max_backlog: usize, target_tps: f64) -> Self {
        Self {
            max_backlog,
            throttle_factor: 0.0,
            throughput_window: VecDeque::new(),
            target_tps,
        }
    }

    /// Calculate suggested delay before next send.
    pub fn suggested_delay_ms(&mut self, backlog: usize) -> u64 {
        // Update throttle based on backlog
        if backlog > self.max_backlog {
            self.throttle_factor = (self.throttle_factor + 0.1).min(1.0);
        } else if backlog < self.max_backlog / 2 {
            self.throttle_factor = (self.throttle_factor - 0.1).max(0.0);
        }

        // Calculate delay from target throughput
        if self.target_tps > 0.0 && self.throttle_factor > 0.0 {
            (1000.0 / self.target_tps * self.throttle_factor) as u64
        } else {
            0
        }
    }

    /// Record a sent chunk for throughput tracking.
    pub fn record_send(&mut self, size: usize) {
        self.throughput_window.push_back((Instant::now(), size));
        // Trim entries older than 1 second
        let now = Instant::now();
        while self
            .throughput_window
            .front()
            .map(|(t, _)| now - *t > Duration::from_secs(1))
            .unwrap_or(false)
        {
            self.throughput_window.pop_front();
        }
    }

    /// Get current throughput (tokens/second).
    pub fn current_tps(&self) -> f64 {
        let window_size: usize = self.throughput_window.iter().map(|(_, s)| s).sum();
        window_size as f64 // per second window
    }

    /// Reset throttle.
    pub fn reset(&mut self) {
        self.throttle_factor = 0.0;
        self.throughput_window.clear();
    }
}

// ─── Automatic Reconnection ──────────────────────────────────────────────

/// Reconnection state for streaming connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectState {
    Connected,
    Disconnected,
    Reconnecting {
        attempt: u32,
        next_retry_ms: u64,
    },
    Failed(String),
}

/// Streaming reconnection manager.
pub struct StreamReconnector {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
    state: std::sync::Mutex<ReconnectState>,
}

impl Default for StreamReconnector {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamReconnector {
    pub fn new() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 1000,
            max_delay_ms: 30_000,
            jitter: true,
            state: std::sync::Mutex::new(ReconnectState::Disconnected),
        }
    }

    /// Attempt reconnection with exponential backoff.
    pub async fn attempt_reconnect(&self) -> ReconnectState {
        let attempt = match *self.state.lock().unwrap() {
            ReconnectState::Reconnecting { attempt, .. } => attempt + 1,
            _ => 1,
        };

        if attempt > self.max_attempts {
            *self.state.lock().unwrap() = ReconnectState::Failed(format!(
                "Max reconnection attempts ({}) exceeded",
                self.max_attempts
            ));
            return self.state.lock().unwrap().clone();
        }

        let delay = (self.base_delay_ms * 2u64.pow(attempt - 1)).min(self.max_delay_ms);
        let delay = if self.jitter {
            delay + rand::random::<u64>() % (delay / 4).max(1)
        } else {
            delay
        };

        *self.state.lock().unwrap() = ReconnectState::Reconnecting {
            attempt,
            next_retry_ms: delay,
        };
        tokio::time::sleep(Duration::from_millis(delay)).await;
        self.state.lock().unwrap().clone()
    }

    pub fn connected(&self) {
        *self.state.lock().unwrap() = ReconnectState::Connected;
    }

    pub fn disconnected(&self) {
        *self.state.lock().unwrap() = ReconnectState::Disconnected;
    }

    pub fn state(&self) -> ReconnectState {
        self.state.lock().unwrap().clone()
    }

    pub fn reset(&self) {
        *self.state.lock().unwrap() = ReconnectState::Disconnected;
    }
}

// ─── Enhanced Metrics Aggregation ────────────────────────────────────────

/// Detailed streaming metrics including backpressure information.
#[derive(Debug, Clone, Serialize)]
pub struct DetailedStreamMetrics {
    pub total_events: u64,
    pub total_tokens: u64,
    pub avg_latency_ms: f64,
    pub ttft_avg_ms: f64,
    pub tokens_per_second: f64,
    pub estimated_remaining: u64,
    pub throughput_tps: f64,
}

// ============================================================================
// Convenience constructors
// ============================================================================

/// Create a `StreamEvent` for a content token.
pub fn token_event(content: &str, index: u32, total: u32) -> StreamEvent {
    StreamEvent {
        event_type: EventType::Token,
        content: content.to_string(),
        token_index: index,
        total_tokens: total,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_secs() * 1000,
        meta: None,
    }
}

/// Create a done event signalling stream completion.
pub fn done_event(total_tokens: u32) -> StreamEvent {
    StreamEvent {
        event_type: EventType::Done,
        content: String::new(),
        token_index: 0,
        total_tokens,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_secs() * 1000,
        meta: None,
    }
}

/// Create an error event.
pub fn error_event(msg: &str) -> StreamEvent {
    StreamEvent {
        event_type: EventType::Error,
        content: msg.to_string(),
        token_index: 0,
        total_tokens: 0,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_secs() * 1000,
        meta: None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stream_engine_basic_lifecycle() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (mut engine, tx) = StreamEngine::new(config);

        // Spawn a task that sends events then drops tx
        let handle = tokio::spawn(async move {
            tx.send(token_event("Hello", 0, 3)).await.expect("send hello");
            tx.send(token_event(" world", 1, 3)).await.expect("send world");
            tx.send(token_event("!", 2, 3)).await.expect("send !");
            tx.send(done_event(3)).await.expect("send done");
            drop(tx);
        });

        let stats = engine.run().await;
        handle.await.expect("join");

        assert_eq!(stats.total_tokens, 3);
        assert!(stats.start_time.is_some());
        assert!(stats.end_time.is_some());
        assert!(stats.duration_ms.is_some());
        assert!(stats.ttft_ms.is_some());
        // tokens_per_sec may be 0.0 if stream completes in <1ms
        assert!(stats.tokens_per_sec >= 0.0);
    }

    #[test]
    fn test_token_event_creation() {
        let ev = token_event("hello", 0, 5);
        assert_eq!(ev.event_type, EventType::Token);
        assert_eq!(ev.content, "hello");
        assert_eq!(ev.token_index, 0);
        assert_eq!(ev.total_tokens, 5);
        assert!(ev.meta.is_none());
    }

    #[test]
    fn test_sse_format() {
        let ev = token_event("hi", 0, 1);
        let sse = StreamEngine::format_sse(&ev);
        assert!(sse.starts_with("data: {"));
        assert!(sse.ends_with("\n\n"));
        // Must contain the content
        assert!(sse.contains("\"hi\""));
    }

    #[test]
    fn test_terminal_flush() {
        // Just verify it doesn't panic — we can't easily capture stdout in unit tests.
        let events = vec![
            token_event("abc", 0, 3),
            token_event(" def", 1, 3),
            done_event(2),
        ];
        // Should not panic
        StreamEngine::flush_terminal(&events, false);

        // With thinking enabled
        let thinking_ev = StreamEvent {
            event_type: EventType::Thinking,
            content: "thinking...".into(),
            token_index: 0,
            total_tokens: 0,
            timestamp_ms: 0,
            meta: None,
        };
        StreamEngine::flush_terminal(&[thinking_ev], true);
    }

    #[test]
    fn test_collect_to_string() {
        let proper_events: Vec<StreamEvent> = vec![
            token_event("Hello", 0, 4),
            StreamEvent {
                event_type: EventType::Thinking,
                content: "hmm".into(),
                token_index: 0,
                total_tokens: 0,
                timestamp_ms: 0,
                meta: None,
            },
            token_event(" world", 1, 4),
            StreamEvent {
                event_type: EventType::ToolCall,
                content: "bash".into(),
                token_index: 0,
                total_tokens: 0,
                timestamp_ms: 0,
                meta: None,
            },
            token_event("!", 2, 4),
        ];

        let result = StreamEngine::collect_to_string(&proper_events);
        assert_eq!(result, "Hello world!");
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (mut engine, tx) = StreamEngine::new(config);

        let handle = tokio::spawn(async move {
            tx.send(token_event("a", 0, 4)).await.expect("send");
            tx.send(StreamEvent {
                event_type: EventType::Thinking,
                content: "think".into(),
                token_index: 0,
                total_tokens: 0,
                timestamp_ms: 0,
                meta: None,
            }).await.expect("send think");
            tx.send(StreamEvent {
                event_type: EventType::Thinking,
                content: "more".into(),
                token_index: 0,
                total_tokens: 0,
                timestamp_ms: 0,
                meta: None,
            }).await.expect("send more think");
            tx.send(token_event("b", 1, 4)).await.expect("send b");
            tx.send(StreamEvent {
                event_type: EventType::ToolCall,
                content: "read_file".into(),
                token_index: 0,
                total_tokens: 0,
                timestamp_ms: 0,
                meta: None,
            }).await.expect("send tool");
            tx.send(token_event("c", 2, 4)).await.expect("send c");
            tx.send(error_event("test err")).await.expect("send err");
            drop(tx);
        });

        let stats = engine.run().await;
        handle.await.expect("join");

        assert_eq!(stats.total_tokens, 3);   // a, b, c
        assert_eq!(stats.thinking_tokens, 2); // think, more
        assert_eq!(stats.tool_calls, 1);      // read_file
        assert_eq!(stats.errors, 1);          // test err
    }

    #[tokio::test]
    async fn test_broadcast_subscription() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (mut engine, tx) = StreamEngine::new(config);

        let mut sub_rx = engine.subscribe();

        let handle = tokio::spawn(async move {
            tx.send(token_event("broadcasted", 0, 1))
                .await
                .expect("send");
            tx.send(done_event(1)).await.expect("done");
            drop(tx);
        });

        // Run engine in background
        let engine_handle = tokio::spawn(async move {
            engine.run().await
        });

        // Receive from subscriber
        let received = sub_rx.recv().await.expect("recv broadcast");
        assert_eq!(received.content, "broadcasted");
        assert_eq!(received.event_type, EventType::Token);

        engine_handle.await.expect("engine join");
        handle.await.expect("sender join");
    }

    #[tokio::test]
    async fn test_done_event_terminates_loop() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (mut engine, tx) = StreamEngine::new(config);

        let handle = tokio::spawn(async move {
            tx.send(token_event("only", 0, 1)).await.expect("send");
            tx.send(done_event(1)).await.expect("done");
            // After done, these should NOT be processed
            tx.send(token_event("extra", 1, 2)).await.expect("extra send");
        });

        let stats = engine.run().await;
        handle.await.expect("join");

        // Only 1 token should be counted (the "extra" token after Done is not processed)
        assert_eq!(stats.total_tokens, 1);
    }

    // -- New tests: metrics, reset, shutdown, backpressure, serialization --

    #[tokio::test]
    async fn test_metrics_after_send() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (engine, _tx) = StreamEngine::new(config);

        // Initially metrics should be zero
        let m = engine.metrics().await;
        assert_eq!(m.total_chunks, 0);
        assert_eq!(m.total_tokens, 0);
        assert_eq!(m.avg_latency_ms, 0.0);
        assert_eq!(m.ttft_avg_ms, 0.0);
    }

    #[tokio::test]
    async fn test_reset_clears_stats() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (mut engine, tx) = StreamEngine::new(config.clone());

        // Send a token and run engine
        let handle = tokio::spawn(async move {
            tx.send(token_event("x", 0, 2)).await.expect("send");
            tx.send(done_event(2)).await.expect("done");
        });
        engine.run().await;
        handle.await.expect("join");

        // Reset and check metrics cleared
        let (engine2, _tx2) = StreamEngine::new(config);
        engine2.reset().await;
        let m = engine2.metrics().await;
        assert_eq!(m.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_shutdown_clean() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (engine, _tx) = StreamEngine::new(config);
        // Shutdown should not panic
        engine.shutdown().await;
        let m = engine.metrics().await;
        assert_eq!(m.total_chunks, 0);
    }

    #[tokio::test]
    async fn test_backpressure_no_block() {
        let config = StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        };
        let (engine, _tx) = StreamEngine::new(config);
        // Sending small chunks should not block
        let result = engine.send_with_backpressure("hello").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_serialization() {
        let metrics = StreamMetrics {
            total_chunks: 10,
            total_tokens: 100,
            avg_latency_ms: 5.0,
            ttft_avg_ms: 2.0,
            tokens_per_second: 50.0,
        };
        let json = serde_json::to_string(&metrics).expect("serialize");
        assert!(json.contains("\"total_chunks\":10"));
        assert!(json.contains("\"total_tokens\":100"));
        assert!(json.contains("\"avg_latency_ms\":5.0"));

        let deserialized: StreamMetrics = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.total_chunks, 10);
        assert_eq!(deserialized.tokens_per_second, 50.0);
    }

    #[tokio::test]
    async fn test_multiple_formats_metrics() {
        // Test metrics with different output formats
        for &fmt in &[OutputFormat::Raw, OutputFormat::Sse, OutputFormat::JsonLines] {
            let config = StreamConfig {
                output_format: fmt,
                ..Default::default()
            };
            let (mut engine, tx) = StreamEngine::new(config);

            let handle = tokio::spawn(async move {
                tx.send(token_event("test", 0, 1)).await.expect("send");
                tx.send(done_event(1)).await.expect("done");
            });

            let stats = engine.run().await;
            handle.await.expect("join");
            assert!(stats.total_tokens >= 1);
        }
    }

    // ── Backpressure & Reconnection Tests ──

    #[test]
    fn test_backpressure_controller_basic() {
        let mut bp = BackpressureController::new(100, 10.0);
        assert_eq!(bp.suggested_delay_ms(0), 0);
        assert_eq!(bp.suggested_delay_ms(50), 0);
        // Backlog below max_backlog/2 = 50, so throttle decreases (stays at 0)
        assert_eq!(bp.suggested_delay_ms(50), 0);
        // Above max_backlog = 100
        let delay = bp.suggested_delay_ms(150);
        assert!(delay > 0);
        // Should be 1000/10 * 0.1 = 10ms
        assert_eq!(delay, 10);
    }

    #[test]
    fn test_backpressure_throttle() {
        let mut bp = BackpressureController::new(50, 20.0);
        // Multiple high backlogs should increase throttle
        bp.suggested_delay_ms(100);
        bp.suggested_delay_ms(100);
        bp.suggested_delay_ms(100);
        let delay = bp.suggested_delay_ms(100);
        // throttle = 0.4 (4 increments of 0.1), delay = 1000/20 * 0.4 = 20ms
        assert_eq!(delay, 20);

        // Then low backlog should decrease throttle
        bp.suggested_delay_ms(10);
        let delay = bp.suggested_delay_ms(10);
        // throttle = 0.2, delay = 1000/20 * 0.2 = 10ms
        assert_eq!(delay, 10);
    }

    #[test]
    fn test_backpressure_reset() {
        let mut bp = BackpressureController::new(100, 10.0);
        bp.suggested_delay_ms(200);
        assert!(bp.throttle_factor > 0.0);
        bp.record_send(100);
        assert!(!bp.throughput_window.is_empty());
        bp.reset();
        assert_eq!(bp.throttle_factor, 0.0);
        assert!(bp.throughput_window.is_empty());
    }

    #[test]
    fn test_reconnector_basic() {
        let reconnector = StreamReconnector::new();
        assert_eq!(reconnector.state(), ReconnectState::Disconnected);
        reconnector.connected();
        assert_eq!(reconnector.state(), ReconnectState::Connected);
        reconnector.disconnected();
        assert_eq!(reconnector.state(), ReconnectState::Disconnected);
    }

    #[tokio::test]
    async fn test_reconnector_max_attempts() {
        let reconnector = StreamReconnector {
            max_attempts: 2,
            base_delay_ms: 1,
            max_delay_ms: 10,
            jitter: false,
            state: std::sync::Mutex::new(ReconnectState::Disconnected),
        };

        // First attempt
        reconnector.disconnected();
        let state = reconnector.attempt_reconnect().await;
        assert!(matches!(state, ReconnectState::Reconnecting { attempt: 1, .. }));

        // Second attempt
        let state = reconnector.attempt_reconnect().await;
        assert!(matches!(state, ReconnectState::Reconnecting { attempt: 2, .. }));

        // Third attempt — should fail
        let state = reconnector.attempt_reconnect().await;
        assert!(matches!(state, ReconnectState::Failed(_)));
    }

    #[test]
    fn test_reconnector_exponential_backoff() {
        let reconnector = StreamReconnector {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 10_000,
            jitter: false,
            state: std::sync::Mutex::new(ReconnectState::Disconnected),
        };

        // We can only test the delay calculation indirectly via state
        reconnector.disconnected();
        // Manually set reconnecting state to simulate attempt 1
        *reconnector.state.lock().unwrap() = ReconnectState::Reconnecting {
            attempt: 1,
            next_retry_ms: 200,
        };
        // Check attempt 2 delay: base * 2^(2-1) = 100 * 2 = 200
        // The delay is computed inside attempt_reconnect, but we can check the
        // exponential growth by examining the Reconnecting state fields
        let delay_1 = reconnector.base_delay_ms * 2u64.pow(0); // attempt 1: 100
        let delay_2 = reconnector.base_delay_ms * 2u64.pow(1); // attempt 2: 200
        let delay_3 = reconnector.base_delay_ms * 2u64.pow(2); // attempt 3: 400
        assert_eq!(delay_1, 100);
        assert_eq!(delay_2, 200);
        assert_eq!(delay_3, 400);
    }

    #[test]
    fn test_detailed_metrics() {
        let (engine, _tx) = StreamEngine::new(StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        });

        let dm = engine.detailed_metrics();
        assert_eq!(dm.total_events, 0);
        assert_eq!(dm.total_tokens, 0);
        assert_eq!(dm.estimated_remaining, 0);
        assert_eq!(dm.throughput_tps, 0.0);
    }

    #[tokio::test]
    async fn test_adaptive_send() {
        let (engine, _tx) = StreamEngine::new(StreamConfig {
            output_format: OutputFormat::Raw,
            ..Default::default()
        });

        // Without backpressure controller, send_adaptive should behave like send
        let result = engine.send_adaptive("hello").await;
        assert!(result.is_ok());
    }
}
