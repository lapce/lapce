//! Skills Framework - Plugin-based Skill System
//!
//! This module provides a plugin-based skill system inspired by Claude Code's skills.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Skill Orchestrator                     │
//! ├─────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
//! │  │ Skill       │  │ Skill       │  │ Skill       │   │
//! │  │ Registry    │  │ Loader      │  │ Executor    │   │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘   │
//! │         │                │                │           │
//! │         └────────────────┼────────────────┘           │
//! │                          ▼                            │
//! │  ┌─────────────────────────────────────────────┐     │
//! │  │              Skill Registry                  │     │
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐       │     │
//! │  │  │ Git     │ │Terminal │ │  Web    │ ...  │     │
//! │  │  │ Skill   │ │ Skill   │ │ Skill   │       │     │
//! │  │  └─────────┘ └─────────┘ └─────────┘       │     │
//! │  └─────────────────────────────────────────────┘     │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use deepseek_carp::skills::{SkillRegistry, SkillLoader, SkillParams};
//!
//! // Create registry
//! let registry = Arc::new(SkillRegistry::new());
//!
//! // Load skills
//! let loader = SkillLoader::new(registry.clone());
//! loader.load_all().await?;
//!
//! // Execute a skill
//! let result = registry.execute("builtin:git", params).await?;
//! ```

pub mod skill_trait;
pub mod registry;
pub mod builtin;
pub mod composable;
pub mod progressive;
pub mod ide_sync;

pub use skill_trait::*;
pub use registry::{SkillRegistry, SkillLoader};
pub use composable::{
    ComposableSkill, SkillDoc, SkillStore, SkillPackage,
    SkillFrontmatter, register_composable_skills, community_skill_registry,
};

use std::sync::Arc;

/// Initialize the skills system
///
/// Loads in order:
/// 1. Built-in skills (Git, Terminal, Search, Test)
/// 2. Community registry skills (tdd, grill-me, caveman, etc.)
/// 3. Filesystem composable skills (from `~/.config/carp/skills/` or `.carp/skills/`)
pub async fn init_skills() -> Arc<SkillRegistry> {
    let registry = Arc::new(SkillRegistry::new());
    let loader = SkillLoader::new(registry.clone());
    
    // 1. Load built-in skills (同步)
    if let Err(e) = loader.load_builtin_skills().await {
        eprintln!("Warning: Failed to load some built-in skills: {}", e);
    }
    
    // 2. Register community registry skills (同步)
    let store = SkillStore::new(None);
    if let Err(e) = register_composable_skills(&registry, &store).await {
        eprintln!("Warning: Failed to register composable skills: {}", e);
    }
    
    registry
}
