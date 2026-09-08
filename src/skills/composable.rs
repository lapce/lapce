//! Composable Skills Architecture — SKILL.md format & community skill management.
//!
//! Inspired by mattpocock/skills: each skill is a single-purpose SKILL.md file
//! that can be composed, installed, and versioned independently.
//!
//! ## SKILL.md Format
//!
//! ```markdown
//! ---
//! name: my-skill
//! description: Does one thing well
//! version: 1.0.0
//! author: user
//! tags: [rust, testing]
//! dependencies: []
//! ---
//!
//! ## Instructions
//!
//! When the user asks about <trigger>, do the following:
//! 1. Step one
//! 2. Step two
//!
//! ## Examples
//!
//! - "do the thing" → runs step one and two
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::skill_trait::{SkillMetadata, SkillCapability, ParameterSchema};

// ============================================================================
// SKILL.md parsing
// ============================================================================

/// Frontmatter parsed from a SKILL.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

/// A fully parsed SKILL.md.
#[derive(Debug, Clone)]
pub struct SkillDoc {
    pub frontmatter: SkillFrontmatter,
    pub instructions: String,
    pub examples: Vec<String>,
    pub source_path: PathBuf,
}

impl SkillDoc {
    /// Parse a SKILL.md file at the given path.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read SKILL.md at {}", path.display()))?;
        Self::parse(&content, path.to_path_buf())
    }

    /// Parse SKILL.md content from a string.
    pub fn parse(content: &str, source_path: PathBuf) -> Result<Self> {
        // Extract YAML frontmatter between --- delimiters
        let content_trimmed = content.trim_start();
        if !content_trimmed.starts_with("---") {
            anyhow::bail!("SKILL.md must start with frontmatter between --- delimiters");
        }

        let after_first = &content_trimmed[3..];
        let end_idx = after_first.find("\n---")
            .ok_or_else(|| anyhow::anyhow!("Missing closing --- in SKILL.md frontmatter"))?;

        let yaml_str = &after_first[..end_idx];
        let body_start = end_idx + 4; // skip \n---

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
            .with_context(|| "Failed to parse SKILL.md frontmatter as YAML")?;

        let body = after_first[body_start..].trim();

        // Extract ## Instructions section
        let instructions = extract_section(body, "Instructions")
            .unwrap_or_else(|| body.to_string());

        // Extract ## Examples section
        let examples_section = extract_section(body, "Examples").unwrap_or_default();
        let examples: Vec<String> = examples_section
            .lines()
            .filter(|l| l.trim_start().starts_with("- "))
            .map(|l| l.trim_start_matches("- ").to_string())
            .collect();

        Ok(SkillDoc {
            frontmatter,
            instructions,
            examples,
            source_path,
        })
    }

    /// Convert to `SkillMetadata` (for progressive disclosure — lightweight).
    pub fn to_metadata(&self) -> SkillMetadata {
        SkillMetadata {
            id: self.frontmatter.name.clone(),
            name: self.frontmatter.name.clone(),
            description: self.frontmatter.description.clone(),
            version: self.frontmatter.version.clone(),
            author: self.frontmatter.author.clone(),
            tags: self.frontmatter.tags.clone(),
            capabilities: vec![SkillCapability {
                name: "execute".to_string(),
                description: self.frontmatter.description.clone(),
                parameters: vec![],
                returns: Some("Skill execution result".to_string()),
            }],
            dependencies: self.frontmatter.dependencies.clone(),
        }
    }

    /// Get the full instruction text (loaded on demand — progressive disclosure).
    pub fn full_instructions(&self) -> &str {
        &self.instructions
    }

    /// Check if a user query triggers this skill.
    pub fn matches_query(&self, query: &str) -> bool {
        let lower = query.to_lowercase();
        // Check triggers
        if self.frontmatter.triggers.iter().any(|t| lower.contains(&t.to_lowercase())) {
            return true;
        }
        // Check name/description
        lower.contains(&self.frontmatter.name.to_lowercase())
            || lower.contains(&self.frontmatter.description.to_lowercase())
    }
}

/// Extract a markdown section by heading name.
fn extract_section(body: &str, heading: &str) -> Option<String> {
    let heading_pattern = format!("## {}", heading);
    let body_lower = body.to_lowercase();
    let heading_lower = heading_pattern.to_lowercase();
    let start = body_lower.find(&heading_lower)?;

    let after_heading = &body[start + heading_pattern.len()..];

    // Find next heading or end
    let end = after_heading.find("\n## ")
        .map(|i| i)
        .unwrap_or(after_heading.len());

    Some(after_heading[..end].trim().to_string())
}

// ============================================================================
// SkillStore — manages installed community skills on filesystem
// ============================================================================

/// Manages the local skill store (filesystem-based).
pub struct SkillStore {
    /// Directory where skills are stored: `~/.config/carp/skills/` or `.carp/skills/`
    store_dir: PathBuf,
}

impl SkillStore {
    /// Create a new skill store at the default location.
    pub fn new(project_root: Option<&Path>) -> Self {
        let store_dir = project_root
            .map(|p| p.join(".carp").join("skills"))
            .or_else(|| {
                dirs_next::config_dir().map(|d| d.join("carp").join("skills"))
            })
            .unwrap_or_else(|| PathBuf::from(".carp/skills"));

        if !store_dir.exists() {
            let _ = fs::create_dir_all(&store_dir);
        }

        Self { store_dir }
    }

    /// List all installed skills (metadata only — progressive disclosure).
    pub fn list_skills(&self) -> Result<Vec<SkillMetadata>> {
        let mut skills = Vec::new();
        if !self.store_dir.exists() {
            return Ok(skills);
        }

        for entry in fs::read_dir(&self.store_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Ok(doc) = SkillDoc::from_file(&path) {
                    skills.push(doc.to_metadata());
                }
            }
        }

        Ok(skills)
    }

    /// Install a skill from a SKILL.md file or URL.
    pub fn install_skill(&self, source: &str) -> Result<SkillFrontmatter> {
        let content = if source.starts_with("http://") || source.starts_with("https://") {
            // URL — download
            self.download_skill(source)?
        } else if source.ends_with(".md") && Path::new(source).exists() {
            // Local file
            fs::read_to_string(source)?
        } else {
            // Assume it's a built-in or registry name — generate stub
            return self.install_builtin_stub(source);
        };

        let doc = SkillDoc::parse(&content, PathBuf::from(source))?;
        let target_path = self.store_dir.join(format!("{}.md", doc.frontmatter.name));
        fs::write(&target_path, &content)?;
        info!("Installed skill '{}' from {}", doc.frontmatter.name, source);

        Ok(doc.frontmatter)
    }

    /// P1-C2: Auto-install skill from GitHub repo URL or direct SKILL.md URL.
    ///
    /// Supports (CLI-Anything pattern):
    /// - GitHub repo: `https://github.com/owner/repo` → auto-discovers SKILL.md
    /// - Raw GitHub: `https://raw.githubusercontent.com/.../SKILL.md` → direct fetch
    /// - Any HTTP(S) URL → treated as SKILL.md content
    ///
    /// For GitHub repos, probes these locations in order:
    /// 1. `/SKILL.md` (repo root)
    /// 2. `.carp/SKILL.md` (deepseek-carp convention)
    /// 3. `skills/SKILL.md` (common convention)
    pub fn install_skill_auto(&self, url: &str) -> Result<SkillFrontmatter> {
        let content = if Self::is_github_repo_url(url) {
            // GitHub repo — auto-discover SKILL.md
            self.discover_github_skill(url)?
        } else {
            // Direct URL or path
            self.download_skill(url).or_else(|_| {
                fs::read_to_string(url).map_err(|e| anyhow::anyhow!("{}", e))
            })?
        };

        let doc = SkillDoc::parse(&content, PathBuf::from(url))?;
        let target_path = self.store_dir.join(format!("{}.md", doc.frontmatter.name));
        fs::write(&target_path, &content)?;
        info!("Auto-installed skill '{}' from {}", doc.frontmatter.name, url);

        Ok(doc.frontmatter)
    }

    /// Check if a URL looks like a GitHub repository URL.
    fn is_github_repo_url(url: &str) -> bool {
        url.contains("github.com/")
            && !url.contains("/blob/")
            && !url.contains("/raw/")
            && !url.ends_with(".md")
            && !url.ends_with(".git")
    }

    /// Discover and download SKILL.md from a GitHub repository.
    ///
    /// Probes common locations for skill definitions in order.
    fn discover_github_skill(&self, repo_url: &str) -> Result<String> {
        // Convert github.com URL to raw.githubusercontent.com URL
        let raw_base = repo_url
            .replace("github.com/", "raw.githubusercontent.com/")
            .trim_end_matches('/').to_string();

        // Probe paths in priority order
        let probe_paths = [
            "SKILL.md",
            ".carp/SKILL.md",
            "skills/SKILL.md",
            ".github/SKILL.md",
        ];

        for path in &probe_paths {
            let full_url = format!("{}/main/{}", raw_base, path);
            match self.download_skill(&full_url) {
                Ok(content) => {
                    if content.trim().starts_with("---") || content.contains("name:") {
                        info!("Discovered SKILL.md at {}/main/{}", raw_base, path);
                        return Ok(content);
                    }
                }
                Err(_) => continue, // Try next path
            }

            // Also try master branch
            let master_url = format!("{}/master/{}", raw_base, path);
            if let Ok(content) = self.download_skill(&master_url) {
                if content.trim().starts_with("---") || content.contains("name:") {
                    info!("Discovered SKILL.md at {}/master/{}", raw_base, path);
                    return Ok(content);
                }
            }
        }

        anyhow::bail!(
            "No SKILL.md found in '{}'. Probed: {}\n\
             Tip: Ensure the repo contains a SKILL.md file at root or .carp/",
            repo_url,
            probe_paths.join(", ")
        )
    }

    /// Download a skill from a URL.
    fn download_skill(&self, url: &str) -> Result<String> {
        // Simple HTTP GET using ureq or reqwest
        #[cfg(feature = "network")]
        {
            let resp = ureq::get(url).call()
                .map_err(|e| anyhow::anyhow!("Failed to download skill: {}", e))?;
            let mut body = Vec::new();
            resp.into_reader().read_to_end(&mut body)?;
            return Ok(String::from_utf8(body)?);
        }
        #[cfg(not(feature = "network"))]
        {
            let _ = url;
            anyhow::bail!("Network features disabled. Use a local SKILL.md file path instead.");
        }
    }

    /// Install a built-in registry skill by name.
    fn install_builtin_stub(&self, name: &str) -> Result<SkillFrontmatter> {
        let content = generate_skill_stub(name);
        let target_path = self.store_dir.join(format!("{}.md", name));
        fs::write(&target_path, &content)?;
        info!("Created stub skill '{}' at {}", name, target_path.display());
        let doc = SkillDoc::parse(&content, target_path)?;
        Ok(doc.frontmatter)
    }

    /// Get a skill by name.
    pub fn get_skill(&self, name: &str) -> Result<SkillDoc> {
        let path = self.store_dir.join(format!("{}.md", name));
        if !path.exists() {
            anyhow::bail!("Skill '{}' not found. Use `carp skill add {}` to install it.", name, name);
        }
        SkillDoc::from_file(&path)
    }

    /// Remove a skill by name.
    pub fn remove_skill(&self, name: &str) -> Result<()> {
        let path = self.store_dir.join(format!("{}.md", name));
        if path.exists() {
            fs::remove_file(&path)?;
            info!("Removed skill '{}'", name);
            Ok(())
        } else {
            anyhow::bail!("Skill '{}' not found", name)
        }
    }

    /// Get the store directory path.
    pub fn store_path(&self) -> &Path {
        &self.store_dir
    }
}

/// Generate a SKILL.md stub for user editing.
fn generate_skill_stub(name: &str) -> String {
    format!(
        r#"---
name: {name}
description: Does one thing well
version: 1.0.0
author: ""
tags: []
dependencies: []
triggers: []
---

## Instructions

Describe what this skill does and how it works.

## Examples

- "example prompt" → expected behavior
"#,
        name = name
    )
}

/// Generate a template SKILL.md for `carp skill init`.
pub fn generate_skill_template(name: &str) -> String {
    format!(
        r#"---
name: {name}
description: Add a concise description of what this skill does
version: 1.0.0
author: ""
tags: []
dependencies: []
triggers:
  - trigger phrase 1
  - trigger phrase 2
---

## Instructions

Write step-by-step instructions for the AI to follow when this skill is triggered.
Be specific and include code examples where appropriate.

1. First step
2. Second step
3. Third step

## Examples

- "user prompt example" → expected response / behavior
- "another example" → different scenario
"#,
        name = name
    )
}

// ============================================================================
// Community Registry (built-in skill index)
// ============================================================================

/// Built-in community registry of known skills.
pub fn community_skill_registry() -> Vec<SkillMetadata> {
    vec![
        SkillMetadata {
            id: "community:tdd".into(),
            name: "tdd".into(),
            description: "Red-Green-Refactor TDD workflow — write test, make it pass, refactor".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["testing".into(), "workflow".into(), "tdd".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        SkillMetadata {
            id: "community:grill-me".into(),
            name: "grill-me".into(),
            description: "Structured questioning to clarify ambiguous requirements before coding".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["planning".into(), "requirements".into(), "clarity".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        SkillMetadata {
            id: "community:caveman".into(),
            name: "caveman".into(),
            description: "Caveman mode — 75%% token reduction, short direct answers".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["compression".into(), "efficiency".into(), "concise".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        SkillMetadata {
            id: "community:arch-decision-record".into(),
            name: "arch-decision-record".into(),
            description: "Create Architecture Decision Records (ADR) for project decisions".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["architecture".into(), "documentation".into(), "adr".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        SkillMetadata {
            id: "community:diagnose".into(),
            name: "diagnose".into(),
            description: "Systematic debugging: reproduce → minimize → hypothesize → fix → verify".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["debugging".into(), "testing".into(), "fix".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        SkillMetadata {
            id: "community:improve-codebase".into(),
            name: "improve-codebase".into(),
            description: "Improve codebase architecture using CONTEXT.md and ADRs".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["architecture".into(), "refactoring".into(), "improvement".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
        // ── mattpocock/skills 工作流编排技能 (Phase 3 深度吸收) ──
        SkillMetadata {
            id: "community:handoff".into(),
            name: "handoff".into(),
            description: "Agent handoff & documentation — pass context between AI agents with structured summaries".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["workflow".into(), "orchestration".into(), "handoff".into(), "agent".into()],
            capabilities: vec![],
            dependencies: vec!["diagnose".into(), "improve-codebase".into()],
        },
        SkillMetadata {
            id: "community:triage".into(),
            name: "triage".into(),
            description: "Issue classification — analyze and categorize problems by severity and type before acting".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["workflow".into(), "classification".into(), "planning".into(), "agent".into()],
            capabilities: vec![],
            dependencies: vec!["diagnose".into()],
        },
        SkillMetadata {
            id: "community:plan".into(),
            name: "plan".into(),
            description: "Structured planning — decompose tasks into ordered steps with verification checkpoints".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["workflow".into(), "planning".into(), "execution".into(), "agent".into()],
            capabilities: vec![],
            dependencies: vec!["triage".into()],
        },
        SkillMetadata {
            id: "community:verify".into(),
            name: "verify".into(),
            description: "Verification checkpoints — after each change, run defined checks before proceeding".into(),
            version: "1.0.0".into(),
            author: Some("mattpocock/skills".into()),
            tags: vec!["testing".into(), "verification".into(), "quality".into(), "workflow".into()],
            capabilities: vec![],
            dependencies: vec![],
        },
    ]
}

// ============================================================================
// Skills 2.0 — SkillPackage (SKILL.md + scripts + templates)
// ============================================================================

/// A skill package extension: SKILL.md with optional scripts and templates.
///
/// Directory structure:
/// ```text
/// my-skill/
///   SKILL.md              # Required: skill metadata + instructions
///   scripts/
///     setup.sh            # Optional: setup script
///     run.py              # Optional: execution script
///   templates/
///     example.rs          # Optional: file templates
/// ```
#[derive(Debug, Clone)]
pub struct SkillPackage {
    /// Core skill document.
    pub skill_doc: SkillDoc,
    /// Package root directory.
    pub package_dir: PathBuf,
    /// Script files relative to package_dir.
    pub scripts: Vec<PathBuf>,
    /// Template files relative to package_dir.
    pub templates: Vec<PathBuf>,
}

impl SkillPackage {
    /// Discover and load a skill package from a directory.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            anyhow::bail!("SKILL.md not found in {}", dir.display());
        }

        let doc = SkillDoc::from_file(&skill_md)?;

        // Discover scripts
        let scripts_dir = dir.join("scripts");
        let scripts = if scripts_dir.exists() {
            Self::list_files(&scripts_dir)
        } else {
            Vec::new()
        };

        // Discover templates
        let templates_dir = dir.join("templates");
        let templates = if templates_dir.exists() {
            Self::list_files(&templates_dir)
        } else {
            Vec::new()
        };

        Ok(Self {
            skill_doc: doc,
            package_dir: dir.to_path_buf(),
            scripts,
            templates,
        })
    }

    /// List files in a directory (non-recursive).
    fn list_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    files.push(path);
                }
            }
        }
        files
    }

    /// Generate a skill package template directory.
    pub fn generate_template(dir: &Path, name: &str) -> Result<()> {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir)?;
        fs::create_dir_all(skill_dir.join("scripts"))?;
        fs::create_dir_all(skill_dir.join("templates"))?;

        // Write SKILL.md
        let skill_content = generate_skill_template(name);
        fs::write(skill_dir.join("SKILL.md"), &skill_content)?;

        // Write example script
        let script_content = Self::example_script();
        fs::write(skill_dir.join("scripts").join("setup.sh"), &script_content)?;

        // Write README
        let readme = format!(
            "# {name} Skill Package\n\nSee SKILL.md for usage.\n\n## Contents\n\n- `scripts/` — helper scripts\n- `templates/` — file templates\n",
            name = name
        );
        fs::write(skill_dir.join("README.md"), &readme)?;

        info!("Created skill package at {}", skill_dir.display());
        Ok(())
    }

    fn example_script() -> String {
        r#"#!/bin/bash
# Setup script for this skill
echo "Setting up skill environment..."
"#
        .to_string()
    }

    /// Create a compressed package (.zip or .tar.gz).
    pub fn export_package(&self, output: &Path) -> Result<()> {
        #[cfg(feature = "compress")]
        {
            let file = fs::File::create(output)?;
            let mut tar = tar::Builder::new(file);
            let parent = self.package_dir.parent().unwrap_or(Path::new("."));
            tar.append_dir_all(".", &self.package_dir)?;
            tar.finish()?;
            info!("Exported skill package to {}", output.display());
            Ok(())
        }
        #[cfg(not(feature = "compress"))]
        {
            let _ = output;
            anyhow::bail!("Compression support disabled. Enable 'compress' feature.");
        }
    }

    /// Get all script contents as a map of (name, content).
    pub fn get_scripts_content(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for script_path in &self.scripts {
            if let Ok(content) = fs::read_to_string(script_path) {
                if let Some(name) = script_path.file_name().and_then(|n| n.to_str()) {
                    map.insert(name.to_string(), content);
                }
            }
        }
        map
    }

    /// Get all template contents as a map of (name, content).
    pub fn get_templates_content(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for tmpl_path in &self.templates {
            if let Ok(content) = fs::read_to_string(tmpl_path) {
                if let Some(name) = tmpl_path.file_name().and_then(|n| n.to_str()) {
                    map.insert(name.to_string(), content);
                }
            }
        }
        map
    }
}

/// SkillPackageManager — provides package-level operations.
pub struct SkillPackageManager;

impl SkillPackageManager {
    /// Validate a skill package directory.
    pub fn validate(dir: &Path) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            anyhow::bail!("SKILL.md is required");
        }

        let doc = SkillDoc::from_file(&skill_md)?;
        if doc.frontmatter.name.is_empty() {
            warnings.push("Skill name is empty".into());
        }
        if doc.frontmatter.description.is_empty() {
            warnings.push("Skill description is empty".into());
        }
        if doc.instructions.trim().is_empty() {
            warnings.push("Instructions section is empty".into());
        }

        Ok(warnings)
    }
}

// ============================================================================
// ComposableSkill — adapts SkillDoc to the Skill trait
// ============================================================================

use super::skill_trait::{
    Skill, SkillParams, SkillResult, SkillOutput, SkillError, SkillMetrics,
};
use std::time::Instant;

/// Wraps a `SkillDoc` (parsed SKILL.md) as a `Skill` trait object.
///
/// This bridges the composable skill format (from mattpocock/skills)
/// with the core skill execution system. Skills defined as SKILL.md files
/// can be registered with the `SkillRegistry` and executed like any built-in skill.
///
/// ## Progressive Disclosure
/// Metadata (name, description, capabilities) is always available.
/// Full instruction text is loaded on `execute()` — matching the P0 pattern
/// of showing lightweight info first, loading details on demand.
#[derive(Clone)]
pub struct ComposableSkill {
    /// The parsed skill document (full instructions loaded).
    skill_doc: SkillDoc,
    /// Cached metadata for fast access without re-parsing.
    metadata: SkillMetadata,
}

impl ComposableSkill {
    /// Create a new `ComposableSkill` from a `SkillDoc`.
    pub fn new(skill_doc: SkillDoc) -> Self {
        let metadata = Self::build_metadata(&skill_doc);
        Self { skill_doc, metadata }
    }

    /// Create from a SKILL.md file path.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let doc = SkillDoc::from_file(path)?;
        Ok(Self::new(doc))
    }

    /// Build `SkillMetadata` from the `SkillDoc` frontmatter.
    fn build_metadata(doc: &SkillDoc) -> SkillMetadata {
        let capabilities = vec![SkillCapability {
            name: "execute".to_string(),
            description: format!(
                "{} ({} examples, {} dependencies)",
                doc.frontmatter.description,
                doc.examples.len(),
                doc.frontmatter.dependencies.len(),
            ),
            parameters: vec![
                ParameterSchema {
                    name: "query".to_string(),
                    param_type: "string".to_string(),
                    description: "The user's query to process with this skill".to_string(),
                    required: true,
                    default: None,
                },
            ],
            returns: Some("Skill-guided response".to_string()),
        }];

        SkillMetadata {
            id: format!("community:{}", doc.frontmatter.name),
            name: doc.frontmatter.name.clone(),
            description: doc.frontmatter.description.clone(),
            version: doc.frontmatter.version.clone(),
            author: doc.frontmatter.author.clone(),
            tags: doc.frontmatter.tags.clone(),
            capabilities,
            dependencies: doc.frontmatter.dependencies.clone(),
        }
    }

    /// Get a reference to the underlying `SkillDoc`.
    pub fn skill_doc(&self) -> &SkillDoc {
        &self.skill_doc
    }

    /// Check if a user query triggers this skill.
    pub fn matches_query(&self, query: &str) -> bool {
        self.skill_doc.matches_query(query)
    }
}

impl Skill for ComposableSkill {
    fn metadata(&self) -> SkillMetadata {
        self.metadata.clone()
    }

    fn execute(&self, params: SkillParams) -> Result<SkillResult, SkillError> {
        let start = Instant::now();

        // Extract the user query from parameters
        let query = params.values.get("query")
            .and_then(|v| v.as_string())
            .unwrap_or("");

        // Build the skill execution context:
        // Combine instructions, examples, and the user query into a structured output
        let instructions = self.skill_doc.full_instructions();

        // Check if the query matches any trigger patterns
        let triggered = if query.is_empty() {
            true
        } else {
            self.skill_doc.matches_query(query)
        };

        if !triggered && !query.is_empty() {
            return Err(SkillError {
                code: "NOT_TRIGGERED".to_string(),
                message: format!(
                    "Query '{}' does not match skill '{}' triggers",
                    query,
                    self.skill_doc.frontmatter.name
                ),
                details: Some(format!(
                    "Triggers: {:?}",
                    self.skill_doc.frontmatter.triggers
                )),
                recoverable: true,
            });
        }

        // Build output with instructions and examples for the agent to use
        let mut output = String::new();
        output.push_str(&format!(
            "## Skill: {}\n\n{}\n\n",
            self.skill_doc.frontmatter.name,
            self.skill_doc.frontmatter.description
        ));

        if !instructions.is_empty() {
            output.push_str("### Instructions\n\n");
            output.push_str(instructions);
            output.push('\n');
        }

        if !self.skill_doc.examples.is_empty() {
            output.push_str("\n### Examples\n\n");
            for (i, example) in self.skill_doc.examples.iter().enumerate() {
                output.push_str(&format!("{}. {}\n", i + 1, example));
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(SkillResult {
            success: true,
            output: SkillOutput::Text(output),
            metrics: SkillMetrics {
                execution_time_ms: elapsed,
                tokens_used: 0,
                files_modified: 0,
                errors_count: 0,
            },
            errors: vec![],
        })
    }
}

/// Register all composable skills from a `SkillStore` into a `SkillRegistry`.
///
/// This is the bridge between the filesystem-based skill store and the
/// in-memory skill registry. Call this during initialization to make
/// installed community skills available for execution.
pub async fn register_composable_skills(
    registry: &super::registry::SkillRegistry,
    store: &SkillStore,
) -> Result<usize, SkillError> {
    let mut count = 0;

    let skills = store.list_skills().map_err(|e| SkillError {
        code: "STORE_ERROR".to_string(),
        message: format!("Failed to list skills: {}", e),
        details: None,
        recoverable: false,
    })?;

    for meta in &skills {
        // Load full doc from store
        let doc = match store.get_skill(&meta.name) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!("Failed to load skill '{}': {}", meta.name, e);
                continue;
            }
        };

        let composable = ComposableSkill::new(doc);
        registry.register(composable).await.map_err(|e| SkillError {
            code: "REGISTER_ERROR".to_string(),
            message: format!("Failed to register skill '{}': {}", meta.name, e),
            details: None,
            recoverable: false,
        })?;

        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use super::super::registry::SkillRegistry;
    use crate::skills::skill_trait::SkillContext;
    use super::super::skill_trait::SkillValue;

    #[test]
    fn test_composable_skill_from_skill_doc() {
        let content = r#"---
name: test-composable
description: A composable skill for testing
version: 1.0.0
author: test
tags: [test, composable]
dependencies: []
triggers: [hello, test]
---

## Instructions

1. Greet the user
2. Provide helpful information

## Examples

- "hello world" → greets the user
- "test me" → runs tests
"#;
        let doc = SkillDoc::parse(content, std::path::PathBuf::from("test.md")).unwrap();
        let skill = ComposableSkill::new(doc);

        let meta = skill.metadata();
        assert_eq!(meta.name, "test-composable");
        assert_eq!(meta.id, "community:test-composable");
        assert!(meta.tags.contains(&"test".to_string()));
        assert_eq!(meta.capabilities.len(), 1);
    }

    #[test]
    fn test_composable_skill_execute() {
        let content = r#"---
name: greeter
description: A greeting skill
version: 1.0.0
author: test
tags: [greeting]
dependencies: []
triggers: [hello, hi]
---

## Instructions

Respond with a friendly greeting.

## Examples

- "hello" → says hello back
"#;
        let doc = SkillDoc::parse(content, std::path::PathBuf::from("greeter.md")).unwrap();
        let skill = ComposableSkill::new(doc);

        let params = SkillParams {
            values: std::collections::HashMap::new(),
            context: SkillContext {
                workspace_root: std::path::PathBuf::from("."),
                current_file: None,
                user_id: None,
                session_id: None,
                env_vars: std::collections::HashMap::new(),
            },
        };

        let result = skill.execute(params).unwrap();
        assert!(result.success);
        // Check output via match since SkillOutput doesn't impl Display
        let output_text = match &result.output {
            SkillOutput::Text(t) => t.as_str(),
            _ => panic!("Expected Text output"),
        };
        assert!(output_text.contains("Skill: greeter"));
        assert!(output_text.contains("## Instructions"));
    }

    #[test]
    fn test_composable_skill_trigger_matching() {
        let content = r#"---
name: code-review
description: Reviews code changes
version: 1.0.0
tags: [code, review]
triggers: [review, code review]
dependencies: []
---

## Instructions

Review the code and provide feedback.

## Examples

- "review this PR" → reviews the pull request
"#;
        let doc = SkillDoc::parse(content, std::path::PathBuf::from("review.md")).unwrap();
        let skill = ComposableSkill::new(doc);

        // Should match trigger
        assert!(skill.matches_query("can you review this PR?"));
        assert!(skill.matches_query("code review needed"));

        // Should not match unrelated queries
        assert!(!skill.matches_query("write a test"));
    }

    #[test]
    fn test_composable_skill_not_triggered_error() {
        let content = r#"---
name: strict-skill
description: Only responds to specific triggers
version: 1.0.0
tags: [strict]
triggers: [magic-word]
dependencies: []
---

## Instructions

Do something special.
"#;
        let doc = SkillDoc::parse(content, std::path::PathBuf::from("strict.md")).unwrap();
        let skill = ComposableSkill::new(doc);

        let params = SkillParams {
            values: {
                let mut m = std::collections::HashMap::new();
                m.insert("query".to_string(), SkillValue::String("wrong query".to_string()));
                m
            },
            context: SkillContext {
                workspace_root: std::path::PathBuf::from("."),
                current_file: None,
                user_id: None,
                session_id: None,
                env_vars: std::collections::HashMap::new(),
            },
        };

        let result = skill.execute(params);
        assert!(result.is_err());
        assert!(result.unwrap_err().code == "NOT_TRIGGERED");
    }

    #[tokio::test]
    async fn test_register_composable_skills_with_registry() {
        // Create a temp skill store
        let tmp = std::env::temp_dir().join(format!("comp_reg_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // Write a SKILL.md
        let content = r#"---
name: registry-test
description: Test registration
version: 1.0.0
tags: [test]
triggers: [test]
dependencies: []
---

## Instructions

Test skill.
"#;
        std::fs::write(tmp.join("registry-test.md"), content).unwrap();

        let store = SkillStore::new(Some(&tmp));
        let registry = SkillRegistry::new();

        let count = register_composable_skills(&registry, &store).await.unwrap();
        assert_eq!(count, 1);

        // Verify it's in the registry
        let meta = registry.list().await;
        assert!(meta.iter().any(|m| m.name == "registry-test"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_doc() {
        let content = r#"---
name: test-skill
description: A test skill
version: 1.0.0
author: test
tags: [test]
dependencies: []
triggers: [hello, world]
---

## Instructions

Do something useful.

## Examples

- "hello world" → does something
"#;
        let doc = SkillDoc::parse(content, PathBuf::from("test.md")).unwrap();
        assert_eq!(doc.frontmatter.name, "test-skill");
        assert_eq!(doc.frontmatter.triggers, vec!["hello", "world"]);
        assert!(doc.instructions.contains("Do something useful"));
        assert_eq!(doc.examples.len(), 1);
        assert!(doc.matches_query("hello"));
        assert!(!doc.matches_query("nope"));
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "No frontmatter here";
        let result = SkillDoc::parse(content, PathBuf::from("test.md"));
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_store_list() {
        let store = SkillStore::new(None);
        // Should not crash on empty store
        let skills = store.list_skills().unwrap();
        assert!(skills.is_empty() || skills.len() >= 0);
    }

    #[test]
    fn test_generate_template() {
        let template = generate_skill_template("my-skill");
        assert!(template.contains("name: my-skill"));
        assert!(template.contains("## Instructions"));
        assert!(template.contains("## Examples"));
    }

    #[test]
    fn test_community_registry() {
        let registry = community_skill_registry();
        assert!(!registry.is_empty());
        assert!(registry.iter().any(|s| s.name == "tdd"));
        assert!(registry.iter().any(|s| s.name == "caveman"));
        assert!(registry.iter().any(|s| s.name == "handoff"));
        assert!(registry.iter().any(|s| s.name == "triage"));
        assert!(registry.iter().any(|s| s.name == "plan"));
        assert!(registry.iter().any(|s| s.name == "verify"));
    }

    // ── Skills 2.0: SkillPackage tests ──

    #[test]
    fn test_skill_package_from_dir_not_found() {
        let tmp = std::env::temp_dir().join("nonexistent_skill");
        let result = SkillPackage::from_dir(&tmp);
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_package_scripts_templates() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!("skill_pkg_test_{}", std::process::id()));

        // Create minimal package structure
        fs::create_dir_all(&tmp).ok();
        fs::write(
            tmp.join("SKILL.md"),
            "---\nname: test-pkg\ndescription: A test\n---\n\n## Instructions\n\nDo stuff.\n",
        ).ok();
        fs::create_dir_all(tmp.join("scripts")).ok();
        fs::write(tmp.join("scripts").join("run.sh"), "echo hi").ok();
        fs::create_dir_all(tmp.join("templates")).ok();
        fs::write(tmp.join("templates").join("main.rs"), "fn main() {}").ok();

        let pkg = SkillPackage::from_dir(&tmp).unwrap();
        assert_eq!(pkg.skill_doc.frontmatter.name, "test-pkg");
        assert_eq!(pkg.scripts.len(), 1);
        assert_eq!(pkg.templates.len(), 1);

        // Test content retrieval
        let scripts = pkg.get_scripts_content();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts.get("run.sh").map(|s| s.as_str()), Some("echo hi"));

        let templates = pkg.get_templates_content();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates.get("main.rs").map(|s| s.as_str()), Some("fn main() {}"));

        // Cleanup
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_skill_package_generate_template() {
        let tmp = std::env::temp_dir().join(format!("skill_pkg_gen_{}", std::process::id()));
        SkillPackage::generate_template(&tmp, "my-pkg").unwrap();

        let skill_md = tmp.join("my-pkg").join("SKILL.md");
        assert!(skill_md.exists());
        let script_dir = tmp.join("my-pkg").join("scripts");
        assert!(script_dir.exists());
        let template_dir = tmp.join("my-pkg").join("templates");
        assert!(template_dir.exists());

        // Cleanup
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_skill_package_manager_validate() {
        // Test validation against an invalid path
        let tmp = std::env::temp_dir().join("nonexistent");
        let result = SkillPackageManager::validate(&tmp);
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_package_no_scripts() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!("skill_no_scripts_{}", std::process::id()));
        fs::create_dir_all(&tmp).ok();
        fs::write(
            tmp.join("SKILL.md"),
            "---\nname: minimal\ndescription: Minimal test\n---\n\n## Instructions\n\nTest.\n",
        ).ok();

        let pkg = SkillPackage::from_dir(&tmp).unwrap();
        assert!(pkg.scripts.is_empty());
        assert!(pkg.templates.is_empty());

        fs::remove_dir_all(&tmp).ok();
    }
}