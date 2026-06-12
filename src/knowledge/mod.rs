//! Shared Domain Language — CONTEXT.md + ADR (Architecture Decision Records).
//!
//! Inspired by mattpocock/skills: a shared domain glossary and decision log that
//! persists across sessions, enriching the agent's understanding of the project.
//!
//! Integrates with the `memory` module's `AutoMemory` / `ProjectMemory` to provide
//! cross-session domain knowledge.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

// ============================================================================
// DomainGlossary — shared terminology extracted from CONTEXT.md
// ============================================================================

/// A single domain term with explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainTerm {
    /// The term (e.g., "Aggregate", "Bounded Context").
    pub term: String,
    /// Plain-language explanation.
    pub definition: String,
    /// Optional source reference.
    pub source: Option<String>,
    /// Related terms.
    pub related: Vec<String>,
}

/// Project domain glossary parsed from CONTEXT.md.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DomainGlossary {
    pub terms: Vec<DomainTerm>,
    pub domain: Option<String>,
}

impl DomainGlossary {
    /// Parse domain terms from a CONTEXT.md file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read CONTEXT.md at {}", path.display()))?;
        Ok(Self::parse(&content))
    }

    /// Parse glossary from markdown content.
    ///
    /// Looks for a `## Glossary` or `## Domain Language` section and parses
    /// term-definition pairs.
    pub fn parse(content: &str) -> Self {
        let mut glossary = DomainGlossary::default();

        // Extract domain from first heading
        for line in content.lines() {
            if let Some(domain) = line.strip_prefix("# ").or_else(|| line.strip_prefix("# ")) {
                glossary.domain = Some(domain.trim().to_string());
                break;
            }
        }

        // Find glossary section
        let lower = content.to_lowercase();
        let glossary_start = lower.find("## glossary")
            .or_else(|| lower.find("## domain language"))
            .or_else(|| lower.find("## domain terms"));

        if let Some(start) = glossary_start {
            let section = &content[start..];
            let section_end = section.find("\n## ")
                .map(|i| i)
                .unwrap_or(section.len());
            let section_body = &section[..section_end];

            // Parse lines: `- **Term**: Definition`
            for line in section_body.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed
                    .strip_prefix("- **")
                    .or_else(|| trimmed.strip_prefix("* **"))
                {
                    if let Some((term, def)) = rest.split_once("**:") {
                        glossary.terms.push(DomainTerm {
                            term: term.trim().to_string(),
                            definition: def.trim().to_string(),
                            source: None,
                            related: Vec::new(),
                        });
                    }
                }
            }
        }

        glossary
    }

    /// Find a term by name (case-insensitive).
    pub fn find_term(&self, term: &str) -> Option<&DomainTerm> {
        let lower = term.to_lowercase();
        self.terms.iter().find(|t| t.term.to_lowercase() == lower)
    }

    /// Check if any terms match the given text.
    pub fn match_terms(&self, text: &str) -> Vec<&DomainTerm> {
        let lower = text.to_lowercase();
        self.terms
            .iter()
            .filter(|t| lower.contains(&t.term.to_lowercase()))
            .collect()
    }

    /// Generate a prompt-friendly glossary string.
    pub fn to_prompt(&self) -> String {
        if self.terms.is_empty() {
            return String::new();
        }
        let mut s = String::from("## Project Domain Glossary\n\n");
        for term in &self.terms {
            s.push_str(&format!("- **{}**: {}\n", term.term, term.definition));
        }
        s
    }
}

// ============================================================================
// AdrEntry — Architecture Decision Record
// ============================================================================

/// Status of an ADR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdrStatus {
    Proposed,
    Accepted,
    Deprecated,
    Superseded,
}

impl std::fmt::Display for AdrStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdrStatus::Proposed => write!(f, "Proposed"),
            AdrStatus::Accepted => write!(f, "Accepted"),
            AdrStatus::Deprecated => write!(f, "Deprecated"),
            AdrStatus::Superseded => write!(f, "Superseded"),
        }
    }
}

/// A single Architecture Decision Record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdrEntry {
    /// ADR number (sequential).
    pub number: u32,
    /// Short title.
    pub title: String,
    /// Status.
    pub status: AdrStatus,
    /// Date of decision.
    pub date: String,
    /// Context — why this decision was needed.
    pub context: String,
    /// Decision made.
    pub decision: String,
    /// Consequences of the decision.
    pub consequences: String,
    /// Related ADR numbers.
    pub related: Vec<u32>,
}

/// Manages Architecture Decision Records.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdrLog {
    pub records: Vec<AdrEntry>,
}

impl AdrLog {
    /// Load ADRs from a directory of `ADR-{number}.md` files.
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut records = Vec::new();
        if !dir.exists() {
            return Self { records };
        }

        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().map(|e| e == "md").unwrap_or(false)
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with("ADR-"))
                        .unwrap_or(false)
            })
            .collect();
        entries.sort();

        for path in entries {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(adr) = Self::parse_adr(&content) {
                    records.push(adr);
                }
            }
        }

        Self { records }
    }

    /// Parse an ADR from markdown content.
    pub fn parse_adr(content: &str) -> Result<AdrEntry> {
        let lines: Vec<&str> = content.lines().collect();
        let mut number = 0u32;
        let mut title = String::new();
        let mut status = AdrStatus::Proposed;
        let mut date = String::new();
        let mut context = String::new();
        let mut decision = String::new();
        let mut consequences = String::new();
        let mut related = Vec::new();

        let mut current_section = String::new();

        for line in &lines {
            if let Some(n) = line.strip_prefix("# ADR-") {
                // Parse number from title line
                let rest = n.trim();
                if let Some((num_str, rest_title)) = rest.split_once(' ') {
                    number = num_str.parse().unwrap_or(0);
                    title = rest_title.trim().to_string();
                } else {
                    title = rest.to_string();
                }
                continue;
            }

            if let Some(s) = line.strip_prefix("## Status: ") {
                status = match s.trim() {
                    "Accepted" => AdrStatus::Accepted,
                    "Deprecated" => AdrStatus::Deprecated,
                    "Superseded" => AdrStatus::Superseded,
                    _ => AdrStatus::Proposed,
                };
                continue;
            }

            if let Some(d) = line.strip_prefix("## Date: ") {
                date = d.trim().to_string();
                continue;
            }

            if line.starts_with("## Context") || line.starts_with("## Context & Problem") {
                current_section = "context".to_string();
                continue;
            }
            if line.starts_with("## Decision") {
                current_section = "decision".to_string();
                continue;
            }
            if line.starts_with("## Consequences") {
                current_section = "consequences".to_string();
                continue;
            }
            if line.starts_with("## Related") {
                current_section = "related".to_string();
                continue;
            }

            match current_section.as_str() {
                "context" => {
                    if !line.trim().is_empty() && !line.starts_with('#') {
                        context.push_str(line);
                        context.push('\n');
                    }
                }
                "decision" => {
                    if !line.trim().is_empty() && !line.starts_with('#') {
                        decision.push_str(line);
                        decision.push('\n');
                    }
                }
                "consequences" => {
                    if !line.trim().is_empty() && !line.starts_with('#') {
                        consequences.push_str(line);
                        consequences.push('\n');
                    }
                }
                "related" => {
                    if let Some(r) = line.trim().strip_prefix("- ADR-") {
                        if let Ok(n) = r.trim().parse::<u32>() {
                            related.push(n);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(AdrEntry {
            number,
            title,
            status,
            date,
            context: context.trim().to_string(),
            decision: decision.trim().to_string(),
            consequences: consequences.trim().to_string(),
            related,
        })
    }

    /// Generate a prompt-friendly summary of all ADRs.
    pub fn to_prompt_summary(&self) -> String {
        if self.records.is_empty() {
            return String::new();
        }
        let mut s = String::from("## Architecture Decision Records\n\n");
        for adr in &self.records {
            s.push_str(&format!(
                "- **ADR-{}. {}** ({}) — {:.80}...\n",
                adr.number, adr.title, adr.status, adr.decision
            ));
        }
        s
    }
}

// ============================================================================
// SharedDomain — combines glossary + ADR + project memory
// ============================================================================

/// The shared domain language system — combines CONTEXT.md glossary, ADR log,
/// and project memory into a cohesive domain knowledge base.
#[derive(Debug, Clone, Default)]
pub struct SharedDomain {
    pub glossary: DomainGlossary,
    pub adr_log: AdrLog,
}

impl SharedDomain {
    /// Load domain knowledge from a project root.
    ///
    /// Looks for:
    /// - `CONTEXT.md` or `docs/CONTEXT.md` — domain glossary
    /// - `docs/adr/` or `ADR/` — ADR files
    pub fn load(project_root: &Path) -> Self {
        let glossary = Self::find_and_load_glossary(project_root);
        let adr_log = Self::find_and_load_adrs(project_root);

        info!(
            glossary_terms = glossary.terms.len(),
            adr_count = adr_log.records.len(),
            "Loaded shared domain knowledge"
        );

        Self { glossary, adr_log }
    }

    /// Find and load CONTEXT.md from common locations.
    fn find_and_load_glossary(project_root: &Path) -> DomainGlossary {
        let candidates = [
            project_root.join("CONTEXT.md"),
            project_root.join("docs/CONTEXT.md"),
            project_root.join(".carp/CONTEXT.md"),
        ];
        for path in &candidates {
            if path.exists() {
                if let Ok(glossary) = DomainGlossary::from_file(path) {
                    return glossary;
                }
            }
        }
        DomainGlossary::default()
    }

    /// Find and load ADRs from common locations.
    fn find_and_load_adrs(project_root: &Path) -> AdrLog {
        let candidates = [
            project_root.join("docs").join("adr"),
            project_root.join("ADR"),
            project_root.join(".carp").join("adr"),
        ];
        for path in &candidates {
            if path.exists() {
                let log = AdrLog::load_from_dir(path);
                if !log.records.is_empty() {
                    return log;
                }
            }
        }
        AdrLog::default()
    }

    /// Generate a combined prompt context string.
    pub fn to_prompt_context(&self) -> String {
        let mut parts = Vec::new();

        let glossary_str = self.glossary.to_prompt();
        if !glossary_str.is_empty() {
            parts.push(glossary_str);
        }

        let adr_str = self.adr_log.to_prompt_summary();
        if !adr_str.is_empty() {
            parts.push(adr_str);
        }

        parts.join("\n")
    }

    /// Find domain terms matching a query.
    pub fn matching_terms(&self, query: &str) -> Vec<&DomainTerm> {
        self.glossary.match_terms(query)
    }
}

// ============================================================================
// Integration with memory module
// ============================================================================

/// Enrich project memory with domain knowledge from CONTEXT.md/ADR.
pub fn enrich_project_memory(
    project_root: &Path,
    memory: &mut crate::memory::auto_memory::ProjectMemory,
) {
    let domain = SharedDomain::load(project_root);

    // Add domain terms as architecture notes
    for term in &domain.glossary.terms {
        let note = format!("Domain term — {}: {}", term.term, term.definition);
        if !memory.architecture_notes.contains(&note) {
            memory.architecture_notes.push(note);
        }
    }

    // Add ADR summaries as architecture notes
    for adr in &domain.adr_log.records {
        let note = format!("ADR-{}: {} ({})", adr.number, adr.title, adr.status);
        if !memory.architecture_notes.contains(&note) {
            memory.architecture_notes.push(note);
        }
    }

    memory.session_count += 1;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_glossary() {
        let glossary = DomainGlossary::parse("# Project\nSome text");
        assert!(glossary.terms.is_empty());
    }

    #[test]
    fn test_parse_glossary_with_terms() {
        let content = r#"# My Project

## Glossary

- **Aggregate**: A cluster of domain objects treated as a single unit.
- **Value Object**: An immutable object with identity based on its state.
"#;
        let glossary = DomainGlossary::parse(content);
        assert_eq!(glossary.terms.len(), 2);
        assert_eq!(glossary.terms[0].term, "Aggregate");
        assert!(glossary.terms[1].definition.contains("immutable"));
    }

    #[test]
    fn test_find_term() {
        let content = r#"## Glossary
- **Bounded Context**: A logical boundary around a domain model.
"#;
        let glossary = DomainGlossary::parse(content);
        assert!(glossary.find_term("Bounded Context").is_some());
        assert!(glossary.find_term("bounded context").is_some());
        assert!(glossary.find_term("Nonexistent").is_none());
    }

    #[test]
    fn test_match_terms() {
        let content = r#"## Glossary
- **Foo**: The foo pattern.
- **Bar**: The bar utility.
"#;
        let glossary = DomainGlossary::parse(content);
        let matched = glossary.match_terms("use the Foo pattern with Bar");
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn test_parse_adr() {
        let content = r#"# ADR-1 Use REST API

## Status: Accepted

## Date: 2025-06-01

## Context

We need an API for the frontend.

## Decision

Use REST over gRPC for simplicity.

## Consequences

Easier to debug, but less efficient.

## Related

- ADR-2
"#;
        let adr = AdrLog::parse_adr(content).unwrap();
        assert_eq!(adr.number, 1);
        assert!(matches!(adr.status, AdrStatus::Accepted));
        assert!(adr.decision.contains("REST"));
        assert!(adr.related.contains(&2));
    }

    #[test]
    fn test_shared_domain_load_no_files() {
        let domain = SharedDomain::load(Path::new("/nonexistent"));
        assert!(domain.glossary.terms.is_empty());
        assert!(domain.adr_log.records.is_empty());
    }

    #[test]
    fn test_glossary_to_prompt() {
        let mut glossary = DomainGlossary::default();
        glossary.terms.push(DomainTerm {
            term: "Test".into(),
            definition: "A test term".into(),
            source: None,
            related: vec![],
        });
        let prompt = glossary.to_prompt();
        assert!(prompt.contains("Test"));
        assert!(prompt.contains("A test term"));
    }

    #[test]
    fn test_adr_log_empty_prompt() {
        let log = AdrLog::default();
        assert!(log.to_prompt_summary().is_empty());
    }
}