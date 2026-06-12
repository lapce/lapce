//! Lightweight event hook system for extensibility.
//!
//! Allows external plugins or modules to register callbacks for key events
//! like provider responses, tool executions, and agent turn completions.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Event types that can be hooked.
#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    /// Fired after each agent turn completes.
    AgentTurnCompleted {
        provider: String,
        total_tokens: u32,
        tools_used: Vec<String>,
    },
    /// Fired after each provider response.
    ProviderResponse {
        provider: String,
        latency_ms: u64,
        tokens: u32,
    },
    /// Fired after each tool execution.
    ToolExecuted {
        tool_name: String,
        success: bool,
    },
    // ── LoopEngine events ──

    /// Fired after each loop round completes.
    LoopRoundCompleted {
        target: String,
        mode: String,
        round: u32,
        verdict: String,
        total_time_ms: u64,
    },
    /// Fired after the entire loop run completes.
    LoopRunCompleted {
        target: String,
        mode: String,
        total_rounds: u32,
        passed: bool,
        total_time_ms: u64,
    },
    /// Fired when a file edit is applied during the Act phase.
    LoopFileEdited {
        target: String,
        file_path: String,
        description: String,
        status: String,
    },
}

/// A hook callback — async function that receives an event.
pub type HookCallback = Arc<dyn Fn(HookEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Registry of hook callbacks.
pub struct HookRegistry {
    callbacks: Arc<RwLock<Vec<HookCallback>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { callbacks: Arc::new(RwLock::new(Vec::new())) }
    }

    /// Register a callback that fires on every event.
    pub async fn register<F, Fut>(&self, callback: F)
    where
        F: Fn(HookEvent) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let cb: HookCallback = Arc::new(move |event| Box::pin(callback(event)));
        self.callbacks.write().await.push(cb);
    }

    /// Fire an event to all registered callbacks.
    pub async fn fire(&self, event: HookEvent) {
        let callbacks = self.callbacks.read().await;
        for cb in callbacks.iter() {
            let cb = cb.clone();
            let event = event.clone();
            tokio::spawn(async move { cb(event).await });
        }
    }
}

impl Default for HookRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hook_fire() {
        let hooks = HookRegistry::new();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = counter.clone();

        hooks.register(move |_event| {
            let c = c.clone();
            async move { c.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
        }).await;

        hooks.fire(HookEvent::AgentTurnCompleted {
            provider: "test".into(),
            total_tokens: 10,
            tools_used: vec![],
        }).await;

        // Allow spawned tasks to complete
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(counter.load(std::sync::atomic::Ordering::SeqCst) > 0);
    }
}
