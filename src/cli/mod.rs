//! CLI argument parsing and command dispatch.
//!
//! ## Commands
//!
//! ```text
//! deepseek-carp chat          — Interactive TUI chat mode
//! deepseek-carp ask "..."     — One-shot question
//! deepseek-carp complete      — Code completion
//! deepseek-carp config        — Manage configuration
//! deepseek-carp serve         — Start as a server
//! deepseek-carp enterprise    — Enterprise compute node commands
//! deepseek-carp version       — Print version info
//! ```

pub mod args;
pub mod dispatch;
pub mod json_output;

pub use args::{Cli, Commands};
pub use dispatch::run;
