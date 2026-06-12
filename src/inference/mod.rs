//! Inference engine abstraction layer.
//!
//! Decouples prompt generation from model execution so deepseek-carp can
//! swap backends (local candle, llama.cpp, cloud API, enterprise cluster)
//! without touching agent/tool/skill code.
//!
//! ```text
//! Agent/Tool/Skill
//!       │
//!       ▼
//! InferenceEngine (trait)  ←── unified API
//!   ├── CloudEngine        ←── HTTP to DeepSeek/GLM/Kimi
//!   ├── LlamaCppEngine     ←── llama-server localhost (low-latency local)
//!   ├── CandleEngine       ←── pure-Rust candle (no external deps)
//!   └── EnterpriseEngine   ←── gRPC to CarpAI cluster
//! ```

pub mod engine;
pub mod cloud;
pub mod llama_cpp;
pub mod candle;
pub mod router;
pub mod complexity;

pub use engine::{InferenceEngine, InferenceRequest, InferenceResponse, InferenceError, EngineStats};
pub use cloud::CloudEngine;
pub use llama_cpp::LlamaCppEngine;
pub use router::EngineRouter;
pub use complexity::{ComplexityScore, EngineChoice, estimate_complexity};
