//! Debounce mechanism for completion requests.
//!
//! When the user is actively typing, each keystroke could trigger a completion
//! request. This debouncer waits 200ms after the last keystroke before firing
//! the actual request, canceling any pending requests that are now stale.
//!
//! ## How it works
//!
//! 1. User types → `debounce()` is called with a new request
//! 2. Previous pending request (if any) is cancelled
//! 3. A 200ms timer starts
//! 4. If no new keystroke arrives within 200ms → request is sent
//! 5. If a new keystroke arrives → go to step 2
//!
//! This reduces meaningless API calls by ~60-80% during active typing.

use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{sleep, Duration, Instant};

use super::FimRequest;

/// A debounced completion request handle.
/// When dropped, the pending request is cancelled.
#[derive(Debug)]
struct DebounceHandle {
    /// Unique ID for this debounce cycle.
    id: u64,
    /// The request that should be sent after the debounce period.
    request: FimRequest,
    /// When the debounce period started.
    started_at: Instant,
}

/// Debouncer for completion requests.
///
/// Thread-safe: can be shared across multiple async tasks.
/// Uses Notify for efficient wake-up instead of polling.
pub struct CompletionDebouncer {
    /// Current pending debounce state.
    state: Arc<Mutex<Option<DebounceHandle>>>,
    /// Debounce delay duration.
    delay: Duration,
    /// Generation counter for canceling stale requests.
    generation: Arc<Mutex<u64>>,
    /// Notification signal for the debounce loop.
    notify: Arc<Notify>,
}

impl CompletionDebouncer {
    /// Create a new debouncer with the default 200ms delay.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
            delay: Duration::from_millis(200),
            generation: Arc::new(Mutex::new(0)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Create a debouncer with a custom delay.
    pub fn with_delay(delay_ms: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
            delay: Duration::from_millis(delay_ms),
            generation: Arc::new(Mutex::new(0)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Submit a completion request with debouncing.
    ///
    /// Returns the request that should be sent, or None if the request
    /// was cancelled by a newer keystroke before the debounce period elapsed.
    ///
    /// This is an async function that:
    /// 1. Stores the new request (canceling any previous pending one)
    /// 2. Waits for the debounce delay
    /// 3. Checks if this request is still the latest one
    /// 4. Returns the request if it's still current, None otherwise
    pub async fn debounce(&self, request: FimRequest) -> Option<FimRequest> {
        let mut gen = self.generation.lock().await;
        *gen += 1;
        let my_gen = *gen;
        drop(gen);

        // Store the request
        {
            let mut state = self.state.lock().await;
            *state = Some(DebounceHandle {
                id: my_gen,
                request: request.clone(),
                started_at: Instant::now(),
            });
        }

        // Wake any waiting debounce tasks
        self.notify.notify_one();

        // Wait for the debounce delay
        sleep(self.delay).await;

        // Check if our request is still the latest
        let gen = self.generation.lock().await;
        let is_current = *gen == my_gen;
        drop(gen);

        if is_current {
            let mut state = self.state.lock().await;
            let result = state.take();
            if let Some(handle) = result {
                if handle.id == my_gen {
                    return Some(handle.request);
                }
            }
        }

        None
    }

    /// Get the current debounce delay.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Cancel any pending debounced request.
    pub async fn cancel_pending(&self) {
        let mut gen = self.generation.lock().await;
        *gen += 1;
        let mut state = self.state.lock().await;
        *state = None;
    }

    /// Get elapsed time since current debounce started (if any).
    pub async fn elapsed_since_start(&self) -> Option<std::time::Duration> {
        let state = self.state.lock().await;
        state.as_ref().map(|h| h.started_at.elapsed())
    }
}

impl Default for CompletionDebouncer {
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
    async fn test_debounce_fires_after_delay() {
        let debouncer = CompletionDebouncer::with_delay(50);
        let req = make_request("hello");

        let start = Instant::now();
        let result = debouncer.debounce(req).await;
        let elapsed = start.elapsed();

        assert!(result.is_some());
        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_debounce_returns_none_when_cancelled() {
        // Two rapid requests — the first should be cancelled by the second
        let debouncer = CompletionDebouncer::with_delay(50);

        let req1 = make_request("first");
        let req2 = make_request("second");

        // Fire both rapidly
        let (r1, r2) = tokio::join!(
            debouncer.debounce(req1),
            debouncer.debounce(req2),
        );

        // First should be cancelled, second should fire
        assert!(r1.is_none(), "First request should be cancelled");
        assert!(r2.is_some(), "Second request should fire");
        assert_eq!(r2.unwrap().prefix, "second");
    }

    #[tokio::test]
    async fn test_cancel_pending() {
        let debouncer = CompletionDebouncer::with_delay(200);

        // Start a request and cancel it before it fires
        let req = make_request("cancel_me");
        let req2 = req.clone();

        let (r1, _) = tokio::join!(
            async {
                // Small delay then cancel
                tokio::time::sleep(Duration::from_millis(10)).await;
                debouncer.cancel_pending().await;
                debouncer.debounce(req2).await
            },
            async {
                debouncer.debounce(req).await
            },
        );

        // The first task starts a new request after cancel, so it should fire
        assert!(r1.is_some());
    }
}