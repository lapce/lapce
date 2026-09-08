//! Program.md driver — Markdown-based loop configuration (autoresearch pattern).
//!
//! Inspired by Karpathy's autoresearch: instead of hardcoding CLI flags,
//! the human writes a `program.md` file that serves as "research org code"
//! — a living document that tells the AI agent what to do, how to behave,
//! and what constraints to follow.
//!
//! ## File Location
//!
//! `{project_root}/.carp/program.md`
//!
//! ## Format
//!
//! ```markdown
//! # Program: Code Quality Gate
//!
//! ## Goal
//! Every PR must pass review with zero HIGH severity findings.
//!
//! ## Role
//! reviewer
//!
//! ## Constraints
//! - NO production code without tests (TDD Iron Law)
//! - All changes must compile with `cargo check --lib`
//! - Security findings are blocking
//!
//! ## Evaluation Criteria
//! - Pass: verdict == Passed AND zero security findings
//! - Fail: any HIGH or CRITICAL finding
//!
//! ## Budget
//! max_rounds: 5
//! round_timeout_secs: 120
//!
//! ## History
//! - 2026-06-07: Initial program. Focus on correctness.
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Parsed content from `program.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramConfig {
    /// Human-readable name for this program.
    pub name: String,
    /// What this program is trying to achieve.
    pub goal: String,
    /// The cognitive role to adopt.
    pub role: Option<String>,
    /// Constraints / Iron Laws to enforce.
    pub constraints: Vec<String>,
    /// Red Flags — excuses the AI might use to skip rules.
    pub red_flags: Vec<RedFlagEntry>,
    /// How success/failure is determined.
    pub evaluation_criteria: Option<String>,
    /// Maximum loop rounds.
    pub max_rounds: Option<u32>,
    /// Round timeout in seconds.
    pub round_timeout_secs: Option<u64>,
    /// Whether to enforce the ReviewGate.
    pub enforce_review_gate: Option<bool>,
    /// Whether to use Iron Laws.
    pub use_iron_laws: Option<bool>,
    /// Changelog / history of changes to this program.
    pub history: Vec<String>,
}

/// A single Red Flag entry from program.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedFlagEntry {
    /// The excuse the AI might make.
    pub excuse: String,
    /// Why it's invalid.
    pub rebuttal: String,
}

impl Default for ProgramConfig {
    fn default() -> Self {
        Self {
            name: "default".into(),
            goal: "Improve code quality".into(),
            role: None,
            constraints: vec![
                "Follow project coding conventions.".into(),
                "All changes must compile successfully.".into(),
            ],
            red_flags: vec![],
            evaluation_criteria: None,
            max_rounds: None,
            round_timeout_secs: None,
            enforce_review_gate: None,
            use_iron_laws: Some(true),
            history: vec![],
        }
    }
}

impl ProgramConfig {
    /// Load program.md from the given project root.
    ///
    /// Returns `None` if no program.md exists (use defaults).
    pub fn load(project_root: &Path) -> anyhow::Result<Option<Self>> {
        let path = Self::path_for(project_root);
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)?;
        let parsed = Self::parse(&content)?;
        Ok(Some(parsed))
    }

    /// Save this program config to disk as program.md.
    pub fn save(&self, project_root: &Path) -> anyhow::Result<PathBuf> {
        let path = Self::path_for(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let md = self.to_markdown();
        std::fs::write(&path, md)?;
        Ok(path)
    }

    /// Generate a default program.md template.
    pub fn generate_template() -> &'static str {
        r#"# Program: DeepSeek Carp Loop

## Goal
[Describe what this loop should achieve]

## Role
developer

## Constraints
- Follow project coding conventions
- All changes must compile (`cargo check --lib`)
- No production code without tests (TDD Iron Law)
- Security findings block merge

## Evaluation Criteria
Pass when: verdict == Passed AND zero blocking findings

## Budget
max_rounds: 5
round_timeout_secs: 120

## History
- [Date]: Initial program created by deepseek-carp init
"#
    }

    /// Convert this config back to markdown format.
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# Program: {}\n\n", self.name);
        md.push_str(&format!("## Goal\n{}\n\n", self.goal));

        if let Some(ref role) = self.role {
            md.push_str(&format!("## Role\n{}\n\n", role));
        }

        if !self.constraints.is_empty() {
            md.push_str("## Constraints\n");
            for c in &self.constraints {
                md.push_str(&format!("- {}\n", c));
            }
            md.push('\n');
        }

        if !self.red_flags.is_empty() {
            md.push_str("## Red Flags\n");
            for rf in &self.red_flags {
                md.push_str(&format!("- \"{}\" → {}\n", rf.excuse, rf.rebuttal));
            }
            md.push('\n');
        }

        if let Some(ref ec) = self.evaluation_criteria {
            md.push_str(&format!("## Evaluation Criteria\n{}\n\n", ec));
        }

        md.push_str("## Budget\n");
        md.push_str(&format!(
            "max_rounds: {}\nround_timeout_secs: {}s\n\n",
            self.max_rounds.unwrap_or(5),
            self.round_timeout_secs.unwrap_or(120),
        ));

        if !self.history.is_empty() {
            md.push_str("## History\n");
            for h in &self.history {
                md.push_str(&format!("- {}\n", h));
            }
            md.push('\n');
        }

        md
    }

    /// Convert to system-prompt snippet for injection into Clarify phase.
    pub fn to_system_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("## Program: {}\n", self.name));
        out.push_str(&format!("**Goal**: {}\n\n", self.goal));

        if !self.constraints.is_empty() {
            out.push_str("### Constraints (NON-NEGOTIABLE)\n");
            for (i, c) in self.constraints.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, c));
            }
            out.push('\n');
        }

        if !self.red_flags.is_empty() {
            out.push_str("### Red Flags\n");
            out.push_str("| Excuse | Why It's Wrong |\n");
            out.push_str("|--------|----------------|\n");
            for rf in &self.red_flags {
                out.push_str(&format!(
                    "| \"{}\" | {} |\n",
                    rf.excuse, rf.rebuttal
                ));
            }
            out.push('\n');
        }

        if let Some(ref ec) = self.evaluation_criteria {
            out.push_str(&format!("**Success Criteria**: {}\n", ec));
        }

        out
    }

    /// Parse raw markdown text into a ProgramConfig.
    fn parse(content: &str) -> anyhow::Result<Self> {
        // Simple section-based parser for program.md
        let mut config = Self::default();
        let mut current_section = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect headers
            if trimmed.starts_with('#') && !trimmed.starts_with("##") {
                if let Some(name) = trimmed.strip_prefix("# ").or(trimmed.trim_start_matches('#').strip_prefix(" ")) {
                    config.name = name.trim().to_string();
                }
                continue;
            }

            if let Some(section) = trimmed.strip_prefix("## ") {
                current_section = section.to_lowercase().replace([' ', '-'], "_");
                continue;
            }

            match current_section.as_str() {
                "goal" => {
                    if !trimmed.is_empty() { config.goal = trimmed.to_string(); }
                }
                "role" => {
                    if !trimmed.is_empty() { config.role = Some(trimmed.to_string()); }
                }
                "constraints" => {
                    if let Some(c) = trimmed.strip_prefix('-').or(trimmed.strip_prefix('*')) {
                        config.constraints.push(c.trim().to_string());
                    } else if !trimmed.is_empty() {
                        config.constraints.push(trimmed.to_string());
                    }
                }
                "red_flags" | "redflags" => {
                    // Parse "excuse → rebuttal" format
                    if let Some(rest) = trimmed.strip_prefix('-').or(trimmed.strip_prefix('*')) {
                        let rest = rest.trim();
                        if let Some((excuse, rebuttal)) = rest.split_once("→") {
                            config.red_flags.push(RedFlagEntry {
                                excuse: excuse.trim().trim_matches('"').to_string(),
                                rebuttal: rebuttal.trim().trim_matches('"').to_string(),
                            });
                        }
                    }
                }
                "evaluation_criteria" | "evaluationcriteria" => {
                    if !trimmed.is_empty() {
                        config.evaluation_criteria = Some(
                            config.evaluation_criteria.unwrap_or_default() + "\n" + trimmed
                        );
                    }
                }
                "budget" => {
                    if let Some(v) = trimmed.strip_prefix("max_rounds:") {
                        if let Ok(n) = v.trim().parse::<u32>() {
                            config.max_rounds = Some(n);
                        }
                    }
                    if let Some(v) = trimmed.strip_prefix("round_timeout_secs:") {
                        if let Ok(n) = v.trim().parse::<u64>() {
                            config.round_timeout_secs = Some(n);
                        }
                    }
                }
                "history" => {
                    if let Some(h) = trimmed.strip_prefix('-').or(trimmed.strip_prefix('*')) {
                        config.history.push(h.trim().to_string());
                    }
                }
                _ => {}
            }
        }

        Ok(config)
    }

    /// Get the path to program.md for a given project root.
    fn path_for(project_root: &Path) -> PathBuf {
        project_root.join(".carp").join("program.md")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_program_md() {
        let md = r#"
# Program: Test Program

## Goal
Make code better.

## Role
reviewer

## Constraints
- Must compile
- Must have tests

## Budget
max_rounds: 3
round_timeout_secs: 60
"#;
        let config = ProgramConfig::parse(md).unwrap();
        assert_eq!(config.name, "Test Program");
        assert_eq!(config.goal, "Make code better.");
        assert_eq!(config.role, Some("reviewer".into()));
        assert_eq!(config.constraints.len(), 2);
        assert_eq!(config.max_rounds, Some(3));
        assert_eq!(config.round_timeout_secs, Some(60));
    }

    #[test]
    fn test_to_system_prompt() {
        let mut config = ProgramConfig::default();
        config.name = "Test".into();
        config.goal = "Fix bugs".into();
        config.constraints = vec!["No bugs".into()];
        let prompt = config.to_system_prompt();
        assert!(prompt.contains("Test"));
        assert!(prompt.contains("Fix bugs"));
        assert!(prompt.contains("No bugs"));
    }

    #[test]
    fn test_red_flag_parsing() {
        let md = r#"
# Program: RF Test

## Goal
Test.

## Red Flags
- "It's simple enough" → Simple code breaks in production.
"#;
        let config = ProgramConfig::parse(md).unwrap();
        assert_eq!(config.red_flags.len(), 1);
        assert_eq!(config.red_flags[0].excuse, "It's simple enough");
    }

    #[test]
    fn test_roundtrip_markdown() {
        let config = ProgramConfig {
            name: "RoundTrip".into(),
            goal: "Test RT".into(),
            role: Some("architect".into()),
            constraints: vec!["C1".into(), "C2".into()],
            ..Default::default()
        };
        let md = config.to_markdown();
        let reparsed = ProgramConfig::parse(&md).unwrap();
        assert_eq!(reparsed.name, config.name);
        assert_eq!(reparsed.role, config.role);
        assert_eq!(reparsed.constraints.len(), 2);
    }

    #[test]
    fn test_path_for() {
        let path = ProgramConfig::path_for(Path::new("/project"));
        assert_eq!(path, Path::new("/project/.carp/program.md"));
    }

    #[test]
    fn test_load_nonexistent() {
        let result = ProgramConfig::load(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}