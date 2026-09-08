//! Multi-provider orchestration for DeepSeek Carp.
//!
//! This module implements intelligent routing across multiple AI providers:
//! - **Official APIs**: DeepSeek, GLM (Zhipu), Kimi (Moonshot), Minimax
//! - **Local models**: Qwen2.5, DeepSeek-R1 via Ollama/llama.cpp
//! - **Enterprise**: CarpAI Enterprise cluster (optional)
//!
//! ## Orchestration Strategies
//!
//! | Strategy | Description | API Cost |
//! |----------|-------------|----------|
//! | `SmartUpgrade` ⭐ | Local Qwen first → smart cloud routing for complex tasks | Low (60%+ local) |
//! | `Cascade` | Sequential fallback A→B→C→D | Medium |
//! | `ParallelRace` | All at once, first wins | High (all called) |
//! | `TaskBasedRouting` | Route by task type (code→DeepSeek, chat→GLM) | Medium |
//! | `AdaptiveWeighted` | ML-based weight learning | Medium |
//! | `CostOptimized` | Cheapest acceptable response | Lowest API cost |
//!
//! ## Example
//!
//! ```no_run
//! use deepseek_carp::config::DeepSeekConfig;
//! use deepseek_carp::providers::{ProviderOrchestrator, ProviderRequest, ChatMessage};
//!
//! # async {
//! let config = DeepSeekConfig::default();
//! let orchestrator = ProviderOrchestrator::new(&config).expect("unwrap failed: mod.rs:27");
//!
//! let request = ProviderRequest {
//!     system: Some("You are a helpful coding assistant.".into()),
//!     messages: vec![ChatMessage {
//!         role: "user".into(),
//!         content: "Write a hello world in Rust".into(),
//!         tool_calls: None,
//!         tool_call_id: None,
//!     }],
//!     max_tokens: Some(1024),
//!     temperature: Some(0.7),
//!     stop: None,
//!     tools: None,
//!     stream: false,
//! };
//!
//! let response = orchestrator.orchestrate(&request).await.expect("unwrap failed: mod.rs:44");
//! println!("[{}] {}", response.provider, response.content);
//! # };
//! ```

pub mod provider;
pub mod orchestrator;
pub mod sync;
pub mod api_keys;
pub mod cache;
pub mod auto_router;
pub mod parallel;
pub mod recovery;
pub mod semantic_cache;
pub mod prefix_cache;
pub mod reasonix_cache;
pub mod reasonix_benchmark;
pub mod ast_cache;
pub mod cross_session_cache;

pub use provider::{
    AiProvider, ChatMessage, OpenAiCompatibleProvider, ProviderError,
    ProviderRequest, ProviderResponse, StreamChunk, TokenUsage, ToolDef, FunctionDef,
};
pub use orchestrator::{
    ProviderOrchestrator, TaskCategory, ProviderHealth, ProviderStats,
};
pub use sync::{CsyncValue, CsyncSlice, CsyncMap, CsyncVersionedMap};
pub use api_keys::{ApiKeyPool, ApiKeyPoolStats, ProtocolAdapter, OpenAiAdapter, SecureKey, SecureKeyStore, KeyInfo};
pub use cache::{PromptCache, CacheManager, CacheStats, hash_prefix};
pub use auto_router::{AutoRouter, TaskComplexity, ModelRecommendation};
pub use parallel::{
    HybridParallelConfig, HybridParallelResult, HybridSource, hybrid_parallel,
    StreamingRaceConfig, StreamingRace,
    DedupConfig, RequestDedup, DedupResult, DedupCompletion,
};
pub use recovery::{
    CircuitState, CircuitBreaker, RetryConfig,
    FallbackChain, FallbackError, ProviderFallbackError, RecoveryManager,
};
pub use semantic_cache::{SemanticCache, CacheKey, CacheEntry, SemanticCacheManager};
pub use reasonix_cache::{
    ReasonixCache, ReasonixConfig, CacheMetrics, ContextZone, ApiUsage,
    PrefixFingerprint, CacheError,
};
