//! Skills Framework - Plugin-based Skill System
//!
//! Based on Claude Code's skill system, this module provides:
//! - Plugin-based architecture
//! - Dynamic skill loading
//! - Built-in skills (Git, Terminal, Web, Test)
//! - Skill registry and discovery

use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Skill metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub capabilities: Vec<SkillCapability>,
    pub dependencies: Vec<String>,
}

/// Skill capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCapability {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParameterSchema>,
    pub returns: Option<String>,
}

/// Parameter schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

/// Skill parameters
#[derive(Debug, Clone)]
pub struct SkillParams {
    pub values: HashMap<String, SkillValue>,
    pub context: SkillContext,
}

/// Skill value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SkillValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Array(Vec<SkillValue>),
    Object(HashMap<String, SkillValue>),
    Null,
}

impl SkillValue {
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SkillValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SkillValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            SkillValue::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// Skill execution context
#[derive(Debug, Clone)]
pub struct SkillContext {
    pub workspace_root: PathBuf,
    pub current_file: Option<PathBuf>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub env_vars: HashMap<String, String>,
}

/// Skill result
#[derive(Debug, Clone)]
pub struct SkillResult {
    pub success: bool,
    pub output: SkillOutput,
    pub metrics: SkillMetrics,
    pub errors: Vec<String>,
}

/// Skill output
#[derive(Debug, Clone)]
pub enum SkillOutput {
    Text(String),
    Structured(serde_json::Value),
    Files(Vec<FileOutput>),
    Error(String),
}

/// File output
#[derive(Debug, Clone)]
pub struct FileOutput {
    pub path: PathBuf,
    pub content: String,
    pub action: FileAction,
}

#[derive(Debug, Clone, Copy)]
pub enum FileAction {
    Create,
    Modify,
    Delete,
}

/// Skill metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetrics {
    pub execution_time_ms: u64,
    pub tokens_used: usize,
    pub files_modified: usize,
    pub errors_count: usize,
}

/// Skill error
#[derive(Debug, Clone)]
pub struct SkillError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
    pub recoverable: bool,
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for SkillError {}

/// Skill trait - interface for all skills
pub trait Skill: Send + Sync {
    /// Get skill metadata (owned value)
    fn metadata(&self) -> SkillMetadata;
    
    /// Validate parameters before execution
    fn validate(&self, params: &SkillParams) -> Result<(), SkillError> {
        for cap in &self.metadata().capabilities {
            for param in &cap.parameters {
                if param.required && !params.values.contains_key(&param.name) {
                    return Err(SkillError {
                        code: "MISSING_PARAM".to_string(),
                        message: format!("Required parameter '{}' is missing", param.name),
                        details: None,
                        recoverable: true,
                    });
                }
            }
        }
        Ok(())
    }
    
    /// Execute the skill
    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError>;
    
    /// Get help information
    fn help(&self) -> String {
        let meta = self.metadata();
        let mut help = format!("# {}\n\n{}\n\n", meta.name, meta.description);
        
        for cap in &meta.capabilities {
            help.push_str(&format!("## {}\n{}\n\n", cap.name, cap.description));
            
            if !cap.parameters.is_empty() {
                help.push_str("### Parameters\n\n");
                for param in &cap.parameters {
                    let required = if param.required { " (required)" } else { "" };
                    help.push_str(&format!(
                        "- `{}`{}{}: {}\n",
                        param.name, required, param.param_type, param.description
                    ));
                }
                help.push('\n');
            }
        }
        
        help
    }
}

/// Skill state
#[derive(Debug, Clone)]
pub enum SkillState {
    Idle,
    Initializing,
    Running,
    Completed,
    Failed(String),
}

/// Skill instance
#[derive(Debug, Clone)]
pub struct SkillInstance {
    pub metadata: SkillMetadata,
    pub state: SkillState,
    pub state_since: std::time::Instant,
}

impl SkillInstance {
    pub fn new(metadata: SkillMetadata) -> Self {
        Self {
            metadata,
            state: SkillState::Idle,
            state_since: std::time::Instant::now(),
        }
    }
}

/// Built-in skill IDs
pub mod builtin {
    pub const GIT_SKILL: &str = "builtin:git";
    pub const TERMINAL_SKILL: &str = "builtin:terminal";
    pub const WEB_SKILL: &str = "builtin:web";
    pub const TEST_SKILL: &str = "builtin:test";
    pub const SEARCH_SKILL: &str = "builtin:search";
}
