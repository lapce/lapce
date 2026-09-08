use serde::{Deserialize, Serialize};
use std::path::Path;

/// Project constitution — structural constraints for the LoopEngine.
///
/// Loaded from `.carp/constitution.toml` and injected into the Planner
/// and Evaluator system prompts to guide AI-generated code toward
/// project-specific standards.
///
/// # Example (`.carp/constitution.toml`)
///
/// ```toml
/// [architecture]
/// style = "modular monolith"
/// principles = [
///   "domain logic MUST NOT depend on infrastructure",
///   "all services use dependency injection",
/// ]
///
/// [coding_standards]
/// error_handling = "anyhow::Result"
/// naming = "snake_case for functions, CamelCase for types"
///
/// [security]
/// rules = [
///   "unwrap() MUST be documented with safety justification",
///   "no hardcoded secrets in source code",
/// ]
///
/// [testing]
/// require_unit_tests = true
/// require_lib_tests = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Constitution {
    /// Architecture-level constraints.
    pub architecture: ArchitectureSection,
    /// Coding standards and style rules.
    pub coding_standards: CodingStandardsSection,
    /// Security policies.
    pub security: SecuritySection,
    /// Testing requirements.
    pub testing: TestingSection,
}

/// Architecture constraints section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ArchitectureSection {
    /// High-level architecture style (e.g. "modular monolith", "hexagonal").
    pub style: String,
    /// List of architecture principles the planner must follow.
    pub principles: Vec<String>,
}


/// Coding standards section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct CodingStandardsSection {
    /// Error handling convention (e.g. "anyhow::Result").
    pub error_handling: String,
    /// Naming conventions.
    pub naming: String,
}


/// Security policies section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct SecuritySection {
    /// List of security rules the evaluator checks against.
    pub rules: Vec<String>,
}


/// Testing requirements section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TestingSection {
    /// Whether unit tests are required for all public APIs.
    pub require_unit_tests: bool,
    /// Whether lib tests (`cargo test --lib`) must pass.
    pub require_lib_tests: bool,
}

impl Default for TestingSection {
    fn default() -> Self {
        Self {
            require_unit_tests: true,
            require_lib_tests: true,
        }
    }
}

impl Constitution {
    /// Load constitution from a TOML file path.
    ///
    /// Returns `Ok(None)` if the file doesn't exist (no constitution configured),
    /// which is not an error — the loop runs without constitution constraints.
    pub fn from_file(path: &Path) -> anyhow::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let constitution: Constitution = toml::from_str(&content)?;
        Ok(Some(constitution))
    }

    /// Load constitution from the default project path (`.carp/constitution.toml`).
    ///
    /// Searches upward from `project_root` for the `.carp` directory.
    pub fn from_project_root(project_root: &Path) -> anyhow::Result<Option<Self>> {
        let carp_dir = project_root.join(".carp");
        if !carp_dir.is_dir() {
            return Ok(None);
        }
        Self::from_file(&carp_dir.join("constitution.toml"))
    }

    /// Format the constitution as a system prompt snippet for LLM injection.
    ///
    /// Returns an empty string if the constitution has no meaningful content.
    pub fn to_system_prompt(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !self.architecture.style.is_empty() || !self.architecture.principles.is_empty() {
            let mut arch = "## Architecture Constraints\n".to_string();
            if !self.architecture.style.is_empty() {
                arch.push_str(&format!("- Style: {}\n", self.architecture.style));
            }
            for p in &self.architecture.principles {
                arch.push_str(&format!("- {}\n", p));
            }
            parts.push(arch);
        }

        if !self.coding_standards.error_handling.is_empty()
            || !self.coding_standards.naming.is_empty()
        {
            let mut cs = "## Coding Standards\n".to_string();
            if !self.coding_standards.error_handling.is_empty() {
                cs.push_str(&format!(
                    "- Error handling: {}\n",
                    self.coding_standards.error_handling
                ));
            }
            if !self.coding_standards.naming.is_empty() {
                cs.push_str(&format!("- Naming: {}\n", self.coding_standards.naming));
            }
            parts.push(cs);
        }

        if !self.security.rules.is_empty() {
            let mut sec = "## Security Rules\n".to_string();
            for r in &self.security.rules {
                sec.push_str(&format!("- {}\n", r));
            }
            parts.push(sec);
        }

        if self.testing.require_unit_tests || self.testing.require_lib_tests {
            let mut tst = "## Testing Requirements\n".to_string();
            if self.testing.require_unit_tests {
                tst.push_str("- Unit tests required for public APIs\n");
            }
            if self.testing.require_lib_tests {
                tst.push_str("- `cargo test --lib` must pass\n");
            }
            parts.push(tst);
        }

        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_constitution_load_from_toml() {
        let toml_content = r#"
[architecture]
style = "modular monolith"
principles = [
    "domain logic MUST NOT depend on infrastructure",
]

[coding_standards]
error_handling = "anyhow::Result"
naming = "snake_case for functions, CamelCase for types"

[security]
rules = ["no hardcoded secrets"]

[testing]
require_unit_tests = true
require_lib_tests = true
"#;

        let constitution: Constitution = toml::from_str(toml_content).unwrap();
        assert_eq!(constitution.architecture.style, "modular monolith");
        assert_eq!(constitution.architecture.principles.len(), 1);
        assert_eq!(constitution.coding_standards.error_handling, "anyhow::Result");
        assert!(constitution.testing.require_unit_tests);
    }

    #[test]
    fn test_constitution_default() {
        let constitution = Constitution::default();
        assert!(constitution.architecture.style.is_empty());
        assert!(constitution.architecture.principles.is_empty());
        assert!(constitution.testing.require_unit_tests);
    }

    #[test]
    fn test_constitution_from_file_not_found() {
        let constitution = Constitution::from_file(Path::new("/nonexistent/path.toml")).unwrap();
        assert!(constitution.is_none());
    }

    #[test]
    fn test_constitution_from_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("constitution.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(
            br#"
[architecture]
style = "hexagonal"
principles = ["ports and adapters"]

[coding_standards]
error_handling = "thiserror"
naming = "snake_case"

[security]
rules = ["no unsafe blocks"]

[testing]
require_unit_tests = true
require_lib_tests = false
"#,
        )
        .unwrap();

        let constitution = Constitution::from_file(&path).unwrap().unwrap();
        assert_eq!(constitution.architecture.style, "hexagonal");
        assert!(!constitution.testing.require_lib_tests);
    }

    #[test]
    fn test_to_system_prompt() {
        let constitution = Constitution {
            architecture: ArchitectureSection {
                style: "hexagonal".into(),
                principles: vec!["ports and adapters".into()],
            },
            coding_standards: CodingStandardsSection {
                error_handling: "anyhow".into(),
                naming: "snake_case".into(),
            },
            security: SecuritySection {
                rules: vec!["no unwrap".into()],
            },
            testing: TestingSection {
                require_unit_tests: true,
                require_lib_tests: false,
            },
        };

        let prompt = constitution.to_system_prompt();
        assert!(prompt.contains("Architecture Constraints"));
        assert!(prompt.contains("hexagonal"));
        assert!(prompt.contains("Coding Standards"));
        assert!(prompt.contains("Security Rules"));
        assert!(prompt.contains("Unit tests required"));
    }

    #[test]
    fn test_to_system_prompt_empty() {
        let constitution = Constitution::default();
        let prompt = constitution.to_system_prompt();
        assert!(prompt.is_empty());
    }
}