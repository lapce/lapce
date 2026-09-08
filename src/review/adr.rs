//! Architecture Decision Records (ADR) — inspired by study8677/awesome-architecture.
//!
//! ADRs document key architectural decisions with context, consequences, and status.
//! This module provides:
//! - ADR template generation (YAML frontmatter + Markdown body)
//! - ADR CRUD: create, list, view, update status
//! - ADR indexing and search
//! - Integration with the skill system as a composable skill
//!
//! ## ADR Format
//!
//! ```markdown
//! ---
//! id: 0001
//! title: Use ADRs for architecture decisions
//! status: accepted  # proposed | accepted | deprecated | superseded
//! date: 2026-06-07
//! deciders: [user]
//! tags: [architecture, documentation]
//! superseded_by: ~
//! ---
//!
//! # ADR 0001: Use ADRs for architecture decisions
//!
//! ## Context
//!
//! We need a way to record architectural decisions...
//!
//! ## Decision
//!
//! We will use Architecture Decision Records...
//!
//! ## Consequences
//!
//! - Positive: Clear decision history
//! - Negative: Maintenance overhead
//! - Neutral: Team needs to learn ADR format
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

/// ADR status lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AdrStatus {
    /// Initial proposal — under discussion.
    #[default]
    Proposed,
    /// Accepted and implemented.
    Accepted,
    /// No longer relevant.
    Deprecated,
    /// Replaced by another ADR.
    Superseded,
}


impl std::fmt::Display for AdrStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Proposed => write!(f, "proposed"),
            Self::Accepted => write!(f, "accepted"),
            Self::Deprecated => write!(f, "deprecated"),
            Self::Superseded => write!(f, "superseded"),
        }
    }
}

/// YAML frontmatter for an ADR file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdrFrontmatter {
    /// Sequential ID (e.g., 0001).
    pub id: u32,
    /// Short title.
    pub title: String,
    /// Current status.
    #[serde(default)]
    pub status: AdrStatus,
    /// Creation date (ISO 8601).
    pub date: String,
    /// People who made this decision.
    #[serde(default)]
    pub deciders: Vec<String>,
    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
    /// If superseded, the ID of the ADR that replaces this one.
    pub superseded_by: Option<u32>,
}

/// A complete Architecture Decision Record.
#[derive(Debug, Clone)]
pub struct AdrRecord {
    pub frontmatter: AdrFrontmatter,
    /// Context section body.
    pub context: String,
    /// Decision section body.
    pub decision: String,
    /// Consequences section body.
    pub consequences: String,
    /// Path to the ADR file on disk.
    pub source_path: PathBuf,
}

impl AdrRecord {
    /// Generate the standard ADR filename.
    pub fn filename(&self) -> String {
        let slug = self.frontmatter.title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        format!("{:04}-{}.md", self.frontmatter.id, slug)
    }

    /// Render the ADR as a Markdown string.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // YAML frontmatter
        md.push_str("---\n");
        md.push_str(&format!("id: {:04}\n", self.frontmatter.id));
        md.push_str(&format!("title: {}\n", self.frontmatter.title));
        md.push_str(&format!("status: {}\n", self.frontmatter.status));
        md.push_str(&format!("date: {}\n", self.frontmatter.date));
        if !self.frontmatter.deciders.is_empty() {
            md.push_str(&format!("deciders: [{}]\n", self.frontmatter.deciders.join(", ")));
        }
        if !self.frontmatter.tags.is_empty() {
            md.push_str(&format!("tags: [{}]\n", self.frontmatter.tags.join(", ")));
        }
        if let Some(sup) = self.frontmatter.superseded_by {
            md.push_str(&format!("superseded_by: {:04}\n", sup));
        }
        md.push_str("---\n\n");

        // Title
        md.push_str(&format!("# ADR {:04}: {}\n\n", self.frontmatter.id, self.frontmatter.title));

        // Context
        md.push_str("## Context\n\n");
        md.push_str(&self.context);
        md.push_str("\n\n");

        // Decision
        md.push_str("## Decision\n\n");
        md.push_str(&self.decision);
        md.push_str("\n\n");

        // Consequences
        md.push_str("## Consequences\n\n");
        md.push_str(&self.consequences);
        md.push('\n');

        md
    }

    /// Parse an ADR from a Markdown file.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content, path.to_path_buf())
    }

    /// Parse an ADR from a Markdown string.
    pub fn parse(content: &str, source_path: PathBuf) -> anyhow::Result<Self> {
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            anyhow::bail!("ADR must start with YAML frontmatter (---)");
        }

        let after_first = &trimmed[3..];
        let end_idx = after_first.find("\n---")
            .ok_or_else(|| anyhow::anyhow!("Missing closing --- in ADR frontmatter"))?;

        let yaml_str = &after_first[..end_idx];
        let body_start = end_idx + 4; // skip \n---

        let frontmatter: AdrFrontmatter = serde_yaml::from_str(yaml_str)?;
        let body = after_first[body_start..].trim();

        // Extract sections
        let context = extract_section(body, "Context").unwrap_or_default();
        let decision = extract_section(body, "Decision").unwrap_or_default();
        let consequences = extract_section(body, "Consequences").unwrap_or_default();

        Ok(Self {
            frontmatter,
            context,
            decision,
            consequences,
            source_path,
        })
    }

    /// Write the ADR to disk.
    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        let path = dir.join(self.filename());
        let content = self.to_markdown();
        std::fs::write(&path, &content)?;
        Ok(path)
    }
}

/// Extract a Markdown section by heading name.
fn extract_section(body: &str, heading: &str) -> Option<String> {
    let heading_pattern = format!("## {}", heading);
    let body_lower = body.to_lowercase();
    let heading_lower = heading_pattern.to_lowercase();
    let start = body_lower.find(&heading_lower)?;
    let after_heading = &body[start + heading_pattern.len()..];

    // Find next heading or end
    let end = after_heading.find("\n## ")
        .unwrap_or(after_heading.len());

    Some(after_heading[..end].trim().to_string())
}

/// ADR manager — manages the ADR directory on disk.
pub struct AdrManager {
    /// Directory where ADRs are stored (e.g., `docs/adr/`).
    adr_dir: PathBuf,
}

impl AdrManager {
    /// Create a new ADR manager.
    ///
    /// Default location: `<project_root>/docs/adr/`
    pub fn new(project_root: Option<&Path>) -> Self {
        let adr_dir = project_root
            .map(|p| p.join("docs").join("adr"))
            .or_else(|| {
                std::env::current_dir().ok().map(|p| p.join("docs").join("adr"))
            })
            .unwrap_or_else(|| PathBuf::from("docs/adr"));

        Self { adr_dir }
    }

    /// Get the ADR directory path.
    pub fn adr_dir(&self) -> &Path {
        &self.adr_dir
    }

    /// Ensure the ADR directory exists.
    pub fn ensure_dir(&self) -> anyhow::Result<()> {
        if !self.adr_dir.exists() {
            std::fs::create_dir_all(&self.adr_dir)?;
        }
        Ok(())
    }

    /// Get the next available ADR ID.
    pub fn next_id(&self) -> anyhow::Result<u32> {
        let records = self.list()?;
        let max_id = records.iter()
            .map(|r| r.frontmatter.id)
            .max()
            .unwrap_or(0);
        Ok(max_id + 1)
    }

    /// Create a new ADR from a template.
    pub fn create(
        &self,
        title: &str,
        context: &str,
        decision: &str,
        consequences: &str,
        tags: Vec<String>,
    ) -> anyhow::Result<AdrRecord> {
        self.ensure_dir()?;
        let id = self.next_id()?;

        let record = AdrRecord {
            frontmatter: AdrFrontmatter {
                id,
                title: title.to_string(),
                status: AdrStatus::Proposed,
                date: chrono_now(),
                deciders: vec![],
                tags,
                superseded_by: None,
            },
            context: context.to_string(),
            decision: decision.to_string(),
            consequences: consequences.to_string(),
            source_path: self.adr_dir.clone(),
        };

        let path = record.save(&self.adr_dir)?;
        tracing::info!(adr_id = id, path = %path.display(), "Created ADR");
        Ok(record)
    }

    /// Generate an ADR template for user editing.
    pub fn generate_template(title: &str, id: u32) -> String {
        format!(
            r#"---
id: {:04}
title: {}
status: proposed
date: {}
deciders: []
tags: []
superseded_by: ~
---

# ADR {:04}: {}

## Context

What is the context behind this decision?
What forces are at play?
Why is this decision needed?

## Decision

What is the decision?
What alternatives were considered?

## Consequences

What becomes easier or harder?
What trade-offs are being made?
"#,
            id, title, chrono_now(), id, title
        )
    }

    /// List all ADRs in the directory.
    pub fn list(&self) -> anyhow::Result<Vec<AdrRecord>> {
        let mut records = Vec::new();

        if !self.adr_dir.exists() {
            return Ok(records);
        }

        let mut entries: Vec<_> = std::fs::read_dir(&self.adr_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map(|ext| ext == "md").unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            match AdrRecord::from_file(&path) {
                Ok(record) => records.push(record),
                Err(e) => {
                    tracing::warn!("Failed to parse ADR at {}: {}", path.display(), e);
                }
            }
        }

        Ok(records)
    }

    /// Search ADRs by title, context, or tags.
    pub fn search(&self, query: &str) -> anyhow::Result<Vec<AdrRecord>> {
        let all = self.list()?;
        let lower = query.to_lowercase();

        Ok(all.into_iter().filter(|r| {
            r.frontmatter.title.to_lowercase().contains(&lower)
                || r.context.to_lowercase().contains(&lower)
                || r.decision.to_lowercase().contains(&lower)
                || r.frontmatter.tags.iter().any(|t| t.to_lowercase().contains(&lower))
        }).collect())
    }

    /// Get ADR by ID.
    pub fn get_by_id(&self, id: u32) -> anyhow::Result<Option<AdrRecord>> {
        let all = self.list()?;
        Ok(all.into_iter().find(|r| r.frontmatter.id == id))
    }

    /// Update the status of an ADR.
    pub fn update_status(&self, id: u32, status: AdrStatus) -> anyhow::Result<bool> {
        let all = self.list()?;
        let record = match all.into_iter().find(|r| r.frontmatter.id == id) {
            Some(r) => r,
            None => return Ok(false),
        };

        // Read the original file, replace the status line
        let content = std::fs::read_to_string(&record.source_path)?;
        let new_content = content.replace(
            &format!("status: {}", record.frontmatter.status),
            &format!("status: {}", status),
        );
        std::fs::write(&record.source_path, &new_content)?;

        tracing::info!(adr_id = id, new_status = %status, "Updated ADR status");
        Ok(true)
    }

    /// Mark an ADR as superseded by another.
    pub fn supersede(&self, old_id: u32, new_id: u32) -> anyhow::Result<bool> {
        let all = self.list()?;
        let record = match all.into_iter().find(|r| r.frontmatter.id == old_id) {
            Some(r) => r,
            None => return Ok(false),
        };

        let content = std::fs::read_to_string(&record.source_path)?;
        let new_content = content
            .replace("status: proposed", "status: superseded")
            .replace("status: accepted", "status: superseded")
            .replace("superseded_by: ~", &format!("superseded_by: {:04}", new_id))
            .replace("superseded_by:", &format!("superseded_by: {:04}", new_id));

        // Handle case where superseded_by doesn't exist yet
        let new_content = if !new_content.contains("superseded_by:") {
            // Insert after tags line
            let tags_line = format!("tags: [{}]", record.frontmatter.tags.join(", "));
            new_content.replace(&tags_line, &format!("{}\nsuperseded_by: {:04}", tags_line, new_id))
        } else {
            new_content
        };

        std::fs::write(&record.source_path, &new_content)?;

        tracing::info!(old_id, new_id, "ADR superseded");
        Ok(true)
    }

    /// Get ADR stats.
    pub fn stats(&self) -> anyhow::Result<AdrStats> {
        let all = self.list()?;
        let mut stats = AdrStats {
            total: all.len(),
            proposed: 0,
            accepted: 0,
            deprecated: 0,
            superseded: 0,
            by_tag: HashMap::new(),
        };

        for r in &all {
            match r.frontmatter.status {
                AdrStatus::Proposed => stats.proposed += 1,
                AdrStatus::Accepted => stats.accepted += 1,
                AdrStatus::Deprecated => stats.deprecated += 1,
                AdrStatus::Superseded => stats.superseded += 1,
            }
            for tag in &r.frontmatter.tags {
                *stats.by_tag.entry(tag.clone()).or_insert(0) += 1;
            }
        }

        Ok(stats)
    }
}

/// ADR statistics.
#[derive(Debug, Clone, Serialize)]
pub struct AdrStats {
    pub total: usize,
    pub proposed: usize,
    pub accepted: usize,
    pub deprecated: usize,
    pub superseded: usize,
    pub by_tag: HashMap<String, usize>,
}

/// Get current date in ISO 8601 format (YYYY-MM-DD).
fn chrono_now() -> String {
    // Use a simple approach without extra dependencies
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Simple leap-year-aware date calculation
    let days = secs / 86400;
    let remaining = secs % 86400;
    let _hours = remaining / 3600;
    let _minutes = (remaining % 3600) / 60;
    let _seconds = remaining % 60;

    // Days since epoch to year/month/day
    let mut y = 1970i64;
    let mut d = days as i64;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    let months_days = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in months_days.iter().enumerate() {
        if d < md as i64 {
            m = i + 1;
            break;
        }
        d -= md as i64;
    }
    if m == 0 {
        m = 12;
    }

    format!("{:04}-{:02}-{:02}", y, m, d + 1)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ============================================================================
// ADR as a Skill — integrates with the composable skill system
// ============================================================================

/// Generate an ADR skill template (SKILL.md format) that users can install.
pub fn adr_skill_template() -> String {
    r#"---
name: arch-decision-record
description: Create Architecture Decision Records (ADR) for project decisions
version: 1.0.0
tags: [architecture, documentation, adr]
triggers:
  - adr
  - architecture decision
  - decision record
  - record architecture
dependencies: []
---

## Instructions

When the user asks to create an Architecture Decision Record (ADR):

1. Ask about the context: what forces are at play, why is this decision needed?
2. Ask about the decision: what is being decided, what alternatives were considered?
3. Ask about consequences: what trade-offs, what becomes easier/harder?
4. Generate the ADR using the template format with YAML frontmatter.
5. Save it to `docs/adr/` with the correct sequential ID.

## Examples

- "create an ADR for using PostgreSQL" → walks through context/decision/consequences
- "record the decision to use microservices" → generates ADR 0001
"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adr_parse() {
        let content = r#"---
id: 0001
title: Use PostgreSQL for primary storage
status: accepted
date: 2026-06-07
deciders: [architect, team-lead]
tags: [database, storage]
superseded_by: ~
---

# ADR 0001: Use PostgreSQL for primary storage

## Context

We need a primary database for our application.
Requirements: ACID compliance, JSON support, high availability.

## Decision

We will use PostgreSQL 16 with Patroni for HA.

## Consequences

- Positive: Mature ecosystem, strong consistency
- Negative: Need DBA expertise
- Neutral: Team needs to learn PostgreSQL
"#;
        let record = AdrRecord::parse(content, PathBuf::from("test.md")).unwrap();
        assert_eq!(record.frontmatter.id, 1);
        assert_eq!(record.frontmatter.title, "Use PostgreSQL for primary storage");
        assert_eq!(record.frontmatter.status, AdrStatus::Accepted);
        assert!(record.context.contains("ACID compliance"));
        assert!(record.decision.contains("PostgreSQL 16"));
        assert!(record.consequences.contains("Mature ecosystem"));
    }

    #[test]
    fn test_adr_roundtrip() {
        let record = AdrRecord {
            frontmatter: AdrFrontmatter {
                id: 42,
                title: "Test ADR".to_string(),
                status: AdrStatus::Proposed,
                date: "2026-06-07".to_string(),
                deciders: vec!["tester".to_string()],
                tags: vec!["test".to_string()],
                superseded_by: None,
            },
            context: "Testing context.".to_string(),
            decision: "Testing decision.".to_string(),
            consequences: "Testing consequences.".to_string(),
            source_path: PathBuf::from("."),
        };

        let md = record.to_markdown();
        assert!(md.contains("id: 0042"));
        assert!(md.contains("status: proposed"));
        assert!(md.contains("Testing context."));

        // Parse it back
        let parsed = AdrRecord::parse(&md, PathBuf::from("test.md")).unwrap();
        assert_eq!(parsed.frontmatter.id, 42);
        assert_eq!(parsed.frontmatter.status, AdrStatus::Proposed);
        assert_eq!(parsed.context, "Testing context.");
    }

    #[test]
    fn test_adr_filename() {
        let record = AdrRecord {
            frontmatter: AdrFrontmatter {
                id: 1,
                title: "Use PostgreSQL".to_string(),
                status: AdrStatus::Accepted,
                date: "2026-06-07".to_string(),
                deciders: vec![],
                tags: vec![],
                superseded_by: None,
            },
            context: String::new(),
            decision: String::new(),
            consequences: String::new(),
            source_path: PathBuf::from("."),
        };

        let filename = record.filename();
        assert_eq!(filename, "0001-use-postgresql.md");
    }

    #[test]
    fn test_adr_manager_list() {
        let tmp = std::env::temp_dir().join(format!("adr_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let manager = AdrManager::new(Some(&tmp));

        // Should be empty initially
        let records = manager.list().unwrap();
        assert!(records.is_empty());

        // Create one
        manager.create("Test ADR", "Context", "Decision", "Consequences", vec![]).unwrap();
        let records = manager.list().unwrap();
        assert_eq!(records.len(), 1);

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adr_next_id() {
        let tmp = std::env::temp_dir().join(format!("adr_id_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let manager = AdrManager::new(Some(&tmp));

        // First ID should be 1
        assert_eq!(manager.next_id().unwrap(), 1);

        // Create one and check next
        manager.create("First", "C", "D", "Conseq", vec![]).unwrap();
        assert_eq!(manager.next_id().unwrap(), 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adr_search() {
        let tmp = std::env::temp_dir().join(format!("adr_search_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let manager = AdrManager::new(Some(&tmp));
        manager.create("PostgreSQL Database", "Use Postgres", "Use it", "Good", vec!["database".into()]).unwrap();
        manager.create("Redis Caching", "Use Redis", "Use it", "Fast", vec!["cache".into()]).unwrap();

        let results = manager.search("database").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].frontmatter.title.contains("PostgreSQL"));

        let results = manager.search("cache").unwrap();
        assert_eq!(results.len(), 1);

        let results = manager.search("nonexistent").unwrap();
        assert_eq!(results.len(), 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adr_status_update() {
        let tmp = std::env::temp_dir().join(format!("adr_status_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let manager = AdrManager::new(Some(&tmp));
        manager.create("Test", "Ctx", "Dec", "Cons", vec![]).unwrap();

        // Update to accepted
        let updated = manager.update_status(1, AdrStatus::Accepted).unwrap();
        assert!(updated);

        let record = manager.get_by_id(1).unwrap().unwrap();
        assert_eq!(record.frontmatter.status, AdrStatus::Accepted);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adr_stats() {
        let tmp = std::env::temp_dir().join(format!("adr_stats_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let manager = AdrManager::new(Some(&tmp));
        manager.create("ADR 1", "Ctx", "Dec", "Cons", vec!["a".into()]).unwrap();
        manager.create("ADR 2", "Ctx", "Dec", "Cons", vec!["b".into()]).unwrap();

        let stats = manager.stats().unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.proposed, 2);
        assert!(stats.by_tag.contains_key("a"));
        assert!(stats.by_tag.contains_key("b"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adr_skill_template() {
        let template = adr_skill_template();
        assert!(template.contains("name: arch-decision-record"));
        assert!(template.contains("## Instructions"));
        assert!(template.contains("## Examples"));
    }

    #[test]
    fn test_adr_manager_custom_path() {
        let tmp = std::env::temp_dir().join(format!("adr_custom_{}", std::process::id()));
        let custom_dir = tmp.join("custom-adrs");
        let _ = std::fs::create_dir_all(&custom_dir);

        let manager = AdrManager::new(Some(&custom_dir));
        manager.create("Custom Path", "Ctx", "Dec", "Cons", vec![]).unwrap();

        assert!(custom_dir.join("0001-custom-path.md").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}