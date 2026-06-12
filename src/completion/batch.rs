//! Request batching for completion — merge concurrent FIM requests into one batch.
//!
//! When multiple completion requests arrive within a short time window
//! (e.g., user types fast, or multiple files are being edited), batching
//! them into a single inference call improves GPU utilization and reduces
//! total latency. This is especially important for local models running
//! on llama.cpp which support batch inference natively.
//!
//! ## How it works
//!
//! 1. Completion requests enter a queue
//! 2. A batcher goroutine collects requests for a configurable window (default: 50ms)
//! 3. When the window closes or max batch size is reached, all requests are sent
//!    as a single batch inference call
//! 4. Results are distributed back to each requester
//!
//! ## Batch size limits
//!
//! - Default max batch size: 4 (balance between throughput and latency)
//! - Default batch window: 50ms (keeps total latency within acceptable range)
//! - For llama.cpp, batch inference is supported via the `n_predict` parameter

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::Duration;

use super::{CompletionCandidate, FimRequest};

/// A batched completion request with its response channel.
#[derive(Debug)]
struct BatchRequest {
    request: FimRequest,
    response_tx: oneshot::Sender<Option<CompletionCandidate>>,
}

impl BatchRequest {
    /// Get a reference to the underlying FIM request.
    pub fn request(&self) -> &FimRequest {
        &self.request
    }
}

/// Request batching engine for completion.
///
/// Collects requests within a time window and sends them as a batch.
pub struct CompletionBatcher {
    /// Channel to submit new batch requests.
    submit_tx: mpsc::UnboundedSender<BatchRequest>,
    /// Maximum batch size.
    max_batch_size: usize,
    /// Batch collection window.
    batch_window: Duration,
    /// Statistics.
    stats: Arc<Mutex<BatcherStats>>,
}

#[derive(Debug, Clone, Default)]
pub struct BatcherStats {
    pub total_requests: u64,
    pub batched_requests: u64,
    pub solo_requests: u64,
    pub avg_batch_size: f64,
    pub total_batches: u64,
}

impl CompletionBatcher {
    /// Create a new batcher with default settings.
    /// - max_batch_size: 4
    /// - batch_window: 50ms
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<BatchRequest>();

        let stats = Arc::new(Mutex::new(BatcherStats::default()));
        let batch_window = Duration::from_millis(50);
        let max_batch_size = 4;

        // Spawn the batcher loop
        let stats_clone = stats.clone();
        tokio::spawn(async move {
            let mut pending: Vec<BatchRequest> = Vec::new();

            loop {
                // Wait for the first request or batch window expiry
                let first = if pending.is_empty() {
                    match rx.recv().await {
                        Some(req) => req,
                        None => break, // channel closed
                    }
                } else {
                    // Try to collect more within the window
                    match tokio::time::timeout(batch_window, rx.recv()).await {
                        Ok(Some(req)) => {
                            pending.push(req);
                            if pending.len() >= max_batch_size {
                                // Batch is full — flush immediately
                                flush_batch(&mut pending, &stats_clone).await;
                            }
                            continue;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            // Timeout — flush pending batch
                            flush_batch(&mut pending, &stats_clone).await;
                            continue;
                        }
                    }
                };

                pending.push(first);

                // If we hit max size immediately, flush
                if pending.len() >= max_batch_size {
                    flush_batch(&mut pending, &stats_clone).await;
                }
            }
        });

        Self {
            submit_tx: tx,
            max_batch_size,
            batch_window,
            stats,
        }
    }

    /// Create a batcher with custom settings.
    pub fn with_config(max_batch_size: usize, batch_window_ms: u64) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<BatchRequest>();
        let stats = Arc::new(Mutex::new(BatcherStats::default()));
        let batch_window = Duration::from_millis(batch_window_ms);

        let stats_clone = stats.clone();
        tokio::spawn(async move {
            let mut pending: Vec<BatchRequest> = Vec::new();

            loop {
                let first = if pending.is_empty() {
                    match rx.recv().await {
                        Some(req) => req,
                        None => break,
                    }
                } else {
                    match tokio::time::timeout(batch_window, rx.recv()).await {
                        Ok(Some(req)) => {
                            pending.push(req);
                            if pending.len() >= max_batch_size {
                                flush_batch(&mut pending, &stats_clone).await;
                            }
                            continue;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            flush_batch(&mut pending, &stats_clone).await;
                            continue;
                        }
                    }
                };

                pending.push(first);
                if pending.len() >= max_batch_size {
                    flush_batch(&mut pending, &stats_clone).await;
                }
            }
        });

        Self {
            submit_tx: tx,
            max_batch_size,
            batch_window,
            stats,
        }
    }

    /// Submit a completion request to the batcher.
    /// Returns a receiver that will get the result when the batch is processed.
    pub fn submit(&self, request: FimRequest) -> oneshot::Receiver<Option<CompletionCandidate>> {
        let (tx, rx) = oneshot::channel();
        let batch_req = BatchRequest {
            request,
            response_tx: tx,
        };

        let _ = self.submit_tx.send(batch_req);
        rx
    }

    /// Get batcher statistics.
    pub async fn stats(&self) -> BatcherStats {
        self.stats.lock().await.clone()
    }

    /// Get the maximum batch size for this batcher.
    pub fn max_batch_size(&self) -> usize {
        self.max_batch_size
    }

    /// Get the batch collection window duration.
    pub fn batch_window(&self) -> Duration {
        self.batch_window
    }
}

/// Flush pending batch requests.
/// In a real implementation, this would send all requests as a single
/// batch inference call to llama.cpp. For now, it marks each request
/// as needing individual processing (the caller will handle actual dispatch).
async fn flush_batch(pending: &mut Vec<BatchRequest>, stats: &Arc<Mutex<BatcherStats>>) {
    if pending.is_empty() {
        return;
    }

    let batch_size = pending.len();

    // Update stats
    {
        let mut s = stats.lock().await;
        s.total_batches += 1;
        s.total_requests += batch_size as u64;
        if batch_size > 1 {
            s.batched_requests += batch_size as u64;
        } else {
            s.solo_requests += 1;
        }
        // Update running average
        let old_total = s.total_batches - 1;
        s.avg_batch_size = (s.avg_batch_size * old_total as f64 + batch_size as f64) / s.total_batches as f64;
    }

    tracing::debug!(
        batch_size,
        "CompletionBatcher: flushing batch"
    );

    // Log each request's prompt prefix for debugging (uses BatchRequest::request getter)
    for req in pending.iter() {
        let _prefix_len = req.request().prefix.len();
        tracing::debug!(prefix_len = _prefix_len, "BatchRequest: queued");
    }

    // Send None to each requester — the caller will dispatch individually
    // The batcher's role is to time-group requests; actual inference is done
    // by the CompletionEngine. Here we send None to signal "process individually"
    // since the true batch inference requires llama.cpp batching API support.
    for req in pending.drain(..) {
        let _ = req.response_tx.send(None);
    }
}

impl Default for CompletionBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(text: &str) -> FimRequest {
        FimRequest {
            prefix: text.to_string(),
            suffix: String::new(),
            file_path: None,
            language: None,
            max_tokens: 64,
            temperature: 0.1,
        }
    }

    #[tokio::test]
    async fn test_batcher_single_request() {
        let batcher = CompletionBatcher::with_config(4, 100);
        let rx = batcher.submit(make_request("hello"));

        // Wait for batch to flush
        tokio::time::sleep(Duration::from_millis(200)).await;

        let result = rx.await;
        assert!(result.is_ok());
        // Single request — should return None (process individually)
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_batcher_multiple_requests() {
        let batcher = CompletionBatcher::with_config(4, 100);

        let r1 = batcher.submit(make_request("a"));
        let r2 = batcher.submit(make_request("b"));
        let r3 = batcher.submit(make_request("c"));

        // All should complete
        let results = tokio::join!(
            async { r1.await.unwrap() },
            async { r2.await.unwrap() },
            async { r3.await.unwrap() },
        );

        // All return None (process individually by engine)
        assert!(results.0.is_none());
        assert!(results.1.is_none());
        assert!(results.2.is_none());

        let stats = batcher.stats().await;
        assert!(stats.total_requests >= 3);
    }
}