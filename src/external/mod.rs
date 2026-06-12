//! Direct API clients for overseas providers (Claude, OpenAI, Copilot)
//! and Chinese providers (GLM, Kimi, Minimax).
//!
//! These clients connect directly to each provider's native API, bypassing
//! the OpenAI-compatible abstraction layer. Use them when you need
//! provider-specific features or optimized request paths.
//!
//! ## Overseas entry points (海外用户基本入口)
//! - `claude` — Anthropic Claude API
//! - `openai` — OpenAI API (GPT-4o, etc.)
//! - `copilot` — GitHub Copilot API
//!
//! ## Domestic providers (国内服务商)
//! - `glm` — Zhipu GLM-4
//! - `kimi` — Moonshot Kimi
//! - `minimax` — Minimax ABAB

pub mod claude;
pub mod openai;
pub mod copilot;
pub mod glm;
pub mod kimi;
pub mod minimax;
pub mod carpai_grpc;
