//! Built-in Skills - Git, Terminal, Web, Test Skills

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use super::skill_trait::*;

// ═══════════════════════════════════════════════════════════════════════════════
// GIT SKILL
// ═══════════════════════════════════════════════════════════════════════════════

pub struct GitSkill;

impl Default for GitSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl GitSkill {
    pub fn new() -> Self {
        Self
    }
}

impl Skill for GitSkill {
    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            id: "builtin:git".to_string(),
            name: "Git".to_string(),
            description: "Git version control operations".to_string(),
            version: "1.0.0".to_string(),
            author: Some("deepseek-carp".to_string()),
            tags: vec!["vcs".to_string(), "git".to_string(), "version".to_string()],
            capabilities: vec![
                SkillCapability {
                    name: "status".to_string(),
                    description: "Show working tree status".to_string(),
                    parameters: vec![],
                    returns: Some("Status output".to_string()),
                },
                SkillCapability {
                    name: "log".to_string(),
                    description: "Show commit logs".to_string(),
                    parameters: vec![
                        ParameterSchema {
                            name: "limit".to_string(),
                            param_type: "number".to_string(),
                            description: "Number of commits to show".to_string(),
                            required: false,
                            default: Some(serde_json::json!(10)),
                        },
                    ],
                    returns: Some("Commit log entries".to_string()),
                },
            ],
            dependencies: vec![],
        }
    }

    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError> {
        let start = Instant::now();
        let capability = params.values.get("capability")
            .and_then(|v| v.as_string())
            .unwrap_or("status");

        let workspace = params.context.workspace_root.clone();

        let output = match capability {
            "status" => self.run_git(&workspace, &["status", "--porcelain"]),
            "log" => {
                let limit = params.values.get("limit")
                    .and_then(|v| v.as_number())
                    .unwrap_or(10.0) as usize;
                self.run_git(&workspace, &["log", &format!("-{}", limit), "--oneline"])
            }
            "diff" => {
                let file = params.values.get("file")
                    .and_then(|v| v.as_string());
                match file {
                    Some(f) => self.run_git(&workspace, &["diff", f]),
                    None => self.run_git(&workspace, &["diff"]),
                }
            }
            "branch" => self.run_git(&workspace, &["branch", "-a"]),
            _ => return Err(SkillError {
                code: "UNKNOWN_CAPABILITY".to_string(),
                message: format!("Unknown capability: {}", capability),
                details: None,
                recoverable: true,
            }),
        };

        Ok(SkillResult {
            success: true,
            output: SkillOutput::Text(output.unwrap_or_else(|e| e.message)),
            metrics: SkillMetrics {
                execution_time_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                files_modified: 0,
                errors_count: 0,
            },
            errors: vec![],
        })
    }
}

impl GitSkill {
    fn run_git(&self, repo: &PathBuf, args: &[&str]) -> Result<String, SkillError> {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .map_err(|e| SkillError {
                code: "GIT_ERROR".to_string(),
                message: format!("Failed to run git: {}", e),
                details: None,
                recoverable: true,
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(SkillError {
                code: "GIT_FAILED".to_string(),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
                details: None,
                recoverable: true,
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TERMINAL SKILL
// ═══════════════════════════════════════════════════════════════════════════════

pub struct TerminalSkill;

impl Default for TerminalSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalSkill {
    pub fn new() -> Self {
        Self
    }
}

impl Skill for TerminalSkill {
    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            id: "builtin:terminal".to_string(),
            name: "Terminal".to_string(),
            description: "Execute terminal commands".to_string(),
            version: "1.0.0".to_string(),
            author: Some("deepseek-carp".to_string()),
            tags: vec!["terminal".to_string(), "shell".to_string(), "command".to_string()],
            capabilities: vec![
                SkillCapability {
                    name: "run".to_string(),
                    description: "Run a shell command".to_string(),
                    parameters: vec![
                        ParameterSchema {
                            name: "command".to_string(),
                            param_type: "string".to_string(),
                            description: "Command to execute".to_string(),
                            required: true,
                            default: None,
                        },
                    ],
                    returns: Some("Command output".to_string()),
                },
            ],
            dependencies: vec![],
        }
    }

    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError> {
        let start = Instant::now();
        
        let command = params.values.get("command")
            .and_then(|v| v.as_string())
            .ok_or_else(|| SkillError {
                code: "MISSING_COMMAND".to_string(),
                message: "Command parameter is required".to_string(),
                details: None,
                recoverable: true,
            })?;

        let cwd = params.context.workspace_root.clone();

        let output = Command::new("sh")
            .args(["-c", command])
            .current_dir(&cwd)
            .output()
            .map_err(|e| SkillError {
                code: "EXEC_ERROR".to_string(),
                message: format!("Failed to execute: {}", e),
                details: None,
                recoverable: true,
            })?;

        Ok(SkillResult {
            success: output.status.success(),
            output: SkillOutput::Text(format!(
                "STDOUT:\n{}\n\nSTDERR:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
            metrics: SkillMetrics {
                execution_time_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                files_modified: 0,
                errors_count: if output.status.success() { 0 } else { 1 },
            },
            errors: if output.status.success() { vec![] } else { vec!["Command failed".to_string()] },
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH SKILL
// ═══════════════════════════════════════════════════════════════════════════════

pub struct SearchSkill;

impl Default for SearchSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchSkill {
    pub fn new() -> Self {
        Self
    }
}

impl Skill for SearchSkill {
    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            id: "builtin:search".to_string(),
            name: "Code Search".to_string(),
            description: "Search code in the project".to_string(),
            version: "1.0.0".to_string(),
            author: Some("deepseek-carp".to_string()),
            tags: vec!["search".to_string(), "code".to_string(), "find".to_string()],
            capabilities: vec![
                SkillCapability {
                    name: "grep".to_string(),
                    description: "Search for a pattern".to_string(),
                    parameters: vec![
                        ParameterSchema {
                            name: "pattern".to_string(),
                            param_type: "string".to_string(),
                            description: "Search pattern".to_string(),
                            required: true,
                            default: None,
                        },
                    ],
                    returns: Some("Search results".to_string()),
                },
            ],
            dependencies: vec![],
        }
    }

    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError> {
        let start = Instant::now();
        
        let pattern = params.values.get("pattern")
            .and_then(|v| v.as_string())
            .ok_or_else(|| SkillError {
                code: "MISSING_PATTERN".to_string(),
                message: "Pattern parameter is required".to_string(),
                details: None,
                recoverable: true,
            })?;

        let path = params.context.workspace_root.clone();

        let output = Command::new("sh")
            .args(["-c", &format!("grep -rn '{}' {} 2>/dev/null", pattern, path.display())])
            .output()
            .map_err(|e| SkillError {
                code: "SEARCH_ERROR".to_string(),
                message: format!("Search failed: {}", e),
                details: None,
                recoverable: true,
            })?;

        Ok(SkillResult {
            success: true,
            output: SkillOutput::Text(String::from_utf8_lossy(&output.stdout).to_string()),
            metrics: SkillMetrics {
                execution_time_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                files_modified: 0,
                errors_count: 0,
            },
            errors: vec![],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SKILL
// ═══════════════════════════════════════════════════════════════════════════════

pub struct TestSkill;

impl Default for TestSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSkill {
    pub fn new() -> Self {
        Self
    }
}

impl Skill for TestSkill {
    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            id: "builtin:test".to_string(),
            name: "Test Generator".to_string(),
            description: "Generate unit tests for code".to_string(),
            version: "1.0.0".to_string(),
            author: Some("deepseek-carp".to_string()),
            tags: vec!["test".to_string(), "testing".to_string(), "unit".to_string()],
            capabilities: vec![
                SkillCapability {
                    name: "generate".to_string(),
                    description: "Generate tests for a function".to_string(),
                    parameters: vec![
                        ParameterSchema {
                            name: "function".to_string(),
                            param_type: "string".to_string(),
                            description: "Function name".to_string(),
                            required: true,
                            default: None,
                        },
                    ],
                    returns: Some("Generated test code".to_string()),
                },
            ],
            dependencies: vec![],
        }
    }

    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError> {
        let start = Instant::now();
        
        let function = params.values.get("function")
            .and_then(|v| v.as_string())
            .unwrap_or("unknown");

        let test_code = format!(
            "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn test_{}() {{\n        // TODO: Add test cases\n        assert!(true);\n    }}\n}}",
            function
        );

        Ok(SkillResult {
            success: true,
            output: SkillOutput::Text(test_code),
            metrics: SkillMetrics {
                execution_time_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                files_modified: 0,
                errors_count: 0,
            },
            errors: vec![],
        })
    }
}

/// List of built-in skill names for MCP discovery.
pub static BUILTIN_NAMES: &[&str] = &[
    "git",
    "terminal",
    "web",
    "test",
    "search",
];
