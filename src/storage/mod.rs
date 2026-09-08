//! Persistent session storage — inspired by Crush's SQLite-backed persistence.
//!
//! Crush uses `sqlc`-generated queries + `ncruces/go-sqlite3` (CGO-free).
//! This module provides an equivalent using `rusqlite` with the `bundled` feature
//! for zero-dependency cross-platform SQLite.
//!
//! ## Feature flag
//!
//! This module is gated behind `#[cfg(feature = "sqlite-storage")]`.
//! When disabled (default), sessions fall back to JSON file storage via `MemoryManager`.
//!
//! ## Tables
//!
//! - `sessions`: conversation sessions (id, title, working_dir, timestamps)
//! - `messages`: chat messages (id, session_id, role, content, seq)
//! - `api_usage`: API call tracking (provider, model, tokens, latency)

#[cfg(feature = "sqlite-storage")]
mod db;

#[cfg(feature = "sqlite-storage")]
mod session_store;

#[cfg(feature = "sqlite-storage")]
mod loop_store;

#[cfg(feature = "sqlite-storage")]
pub use db::Database;

#[cfg(feature = "sqlite-storage")]
pub use session_store::SessionStore;

#[cfg(feature = "sqlite-storage")]
pub use loop_store::{LoopStore, LoopRunRecord, LoopRoundRecord, DiffRecord, LoopRunHistory};

/// File-based archive — always available (no SQLite dependency).
pub mod archive;
