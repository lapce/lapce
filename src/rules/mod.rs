//! Rules & specification module — Constitution + SpecDelta
//!
//! Provides structured project constraints (constitution) and
//! requirement-level change tracking (spec deltas) for the LoopEngine.
//!
//! ## Constitution
//!
//! A TOML file (`.carp/constitution.toml`) that defines the project's
//! architectural rules, coding standards, security policies, and testing
//! requirements. Read by the Planner/Evaluator to validate plans against
//! project-level constraints.
//!
//! ## SpecDelta
//!
//! Records requirement-level changes (ADDED/MODIFIED/REMOVED) made during
//! each loop round, enabling the Markdown report to show what changed
//! from a specification perspective.

pub mod claude_md;
pub mod constitution;
pub mod iron_laws;
pub mod program;
pub mod spec;