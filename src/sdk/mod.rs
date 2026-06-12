//! DeepSeek Carp SDK — headless programmatic API for embedding.
//!
//! Inspired by Claude Code's `QueryEngine`. Provides a clean, non-CLI
//! interface for other applications (e.g., Lapce editor) to use deepseek-carp
//! as an embedded AI engine without spawning a subprocess.
//!
//! ## Usage
//!
//! ```no_run
//! use deepseek_carp::config::DeepSeekConfig;
//! use deepseek_carp::sdk::QueryEngine;
//!
//! # async {
//! let config = DeepSeekConfig::load().unwrap_or_default();
//! let engine = QueryEngine::new(config).expect("unwrap failed: mod.rs:15");
//!
//! // Single-shot query
//! let response = engine.submit("Write a hello world in Rust").await.expect("unwrap failed: mod.rs:18");
//! println!("{}", response.content);
//!
//! // Streaming query — receive chunks as they arrive
//! let mut rx = engine.stream_submit("Explain async/await").await.expect("unwrap failed: mod.rs:22");
//! while let Some(chunk) = rx.recv().await {
//!     print!("{}", chunk.content);
//!     if chunk.is_done { break; }
//! }
//! # };
//! ```

pub mod engine;
pub mod multi_model;

pub use engine::QueryEngine;
pub use multi_model::{RlmExecutor, ModelTier, RoutedTask};
