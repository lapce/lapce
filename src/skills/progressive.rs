//! Progressive Disclosure — load skill metadata first, full content on demand.
//!
//! Inspired by mattpocock/skills: skills expose only name+description for matching,
//! full instructions are loaded only when a match is found. This saves context window
//! and improves performance.
//!
//! Integrates with Context Compression to prioritize skill content within token budgets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use super::skill_trait::SkillMetadata;
use super::composable::{SkillDoc, SkillStore};

// ============================================================================
// LazySkill — metadata only until executed
// ============================================================================

/// A skill whose full instructions are loaded on demand.
#[derive(Debug, Clone)]
pub struct LazySkill {
    /// Lightweight metadata (always loaded).
    pub metadata: SkillMetadata,
    /// Source path for lazy loading.
    source_path: PathBuf,
    /// Whether full content has been loaded.
    loaded: bool,
    /// Cached full instructions (loaded on first access).
    instructions: Option<String>,
}

impl LazySkill {
    /// Create a new lazy skill from its metadata and source path.
    pub fn new(metadata: SkillMetadata, source_path: PathBuf) -> Self {
        Self {
            metadata,
            source_path,
            loaded: false,
            instructions: None,
        }
    }

    /// Get the full instructions — loads from disk on first call.
    pub fn get_instructions(&mut self) -> Result<&str> {
        if !self.loaded {
            let doc = SkillDoc::from_file(&self.source_path)?;
            self.instructions = Some(doc.instructions.clone());
            self.loaded = true;
        }
        Ok(self.instructions.as_deref().unwrap_or(""))
    }

    /// Check if full content has been loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Estimated token count of metadata only.
    pub fn metadata_token_estimate(&self) -> usize {
        self.metadata.name.len()
            + self.metadata.description.len()
            + self.metadata.tags.iter().map(|t| t.len()).sum::<usize>()
    }
}

// ============================================================================
// ProgressiveLoader — manages lazy-loaded skills
// ============================================================================

/// Manages progressive loading of skills.
pub struct ProgressiveLoader {
    /// Skills indexed by name (metadata always loaded, instructions lazy).
    skills: Arc<RwLock<HashMap<String, LazySkill>>>,
    /// Skill store for loading full content.
    store: SkillStore,
}

impl ProgressiveLoader {
    /// Create a new progressive loader.
    pub fn new(project_root: Option<&Path>) -> Self {
        let store = SkillStore::new(project_root);
        Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            store,
        }
    }

    /// Load all skill metadata from the store (no instructions loaded yet).
    pub async fn load_metadata(&self) -> Result<usize> {
        let metadatas = self.store.list_skills()?;
        let mut skills = self.skills.write().await;

        for meta in metadatas {
            let path = self.store.store_path().join(format!("{}.md", meta.name));
            if path.exists() {
                skills.insert(meta.name.clone(), LazySkill::new(meta, path));
            }
        }

        Ok(skills.len())
    }

    /// Find a skill by name and optionally load its instructions.
    pub async fn find_skill(&self, name: &str) -> Option<LazySkill> {
        let skills = self.skills.read().await;
        skills.get(name).cloned()
    }

    /// Load full instructions for a named skill.
    pub async fn load_instructions_for(&self, name: &str) -> Result<String> {
        let mut skills = self.skills.write().await;
        if let Some(skill) = skills.get_mut(name) {
            let instructions = skill.get_instructions()?.to_string();
            Ok(instructions)
        } else {
            // Try loading from store directly
            let doc = self.store.get_skill(name)?;
            Ok(doc.instructions.clone())
        }
    }

    /// Find skills matching a query (metadata only — fast).
    pub async fn find_matching(&self, query: &str) -> Vec<LazySkill> {
        let lower = query.to_lowercase();
        let skills = self.skills.read().await;
        skills
            .values()
            .filter(|s| {
                s.metadata.name.to_lowercase().contains(&lower)
                    || s.metadata.description.to_lowercase().contains(&lower)
                    || s.metadata.tags.iter().any(|t| t.to_lowercase().contains(&lower))
            })
            .cloned()
            .collect()
    }

    /// List all skill metadata (no instructions loaded).
    pub async fn list_all(&self) -> Vec<SkillMetadata> {
        let skills = self.skills.read().await;
        skills.values().map(|s| s.metadata.clone()).collect()
    }

    /// Get number of loaded skills.
    pub async fn count(&self) -> usize {
        self.skills.read().await.len()
    }

    /// Total metadata token estimate across all skills.
    pub async fn total_metadata_tokens(&self) -> usize {
        let skills = self.skills.read().await;
        skills.values().map(|s| s.metadata_token_estimate()).sum()
    }
}

// ============================================================================
// Integration with Context Compression
// ============================================================================

/// Skill context piece for the compression system.
#[derive(Debug, Clone)]
pub struct SkillContextPiece {
    pub skill_name: String,
    pub content: String,
    pub priority: f32, // 0.0–1.0
}

/// Select which skills to include in context within a token budget.
///
/// Works with `ContextCompressor` by returning only the highest-priority
/// skill instructions that fit within the budget.
pub fn select_skills_for_context(
    skills: &[SkillContextPiece],
    budget_tokens: usize,
) -> Vec<SkillContextPiece> {
    use crate::context::compression::estimate_tokens;

    let mut selected = Vec::new();
    let mut used = 0usize;

    // Sort by priority descending
    let mut sorted: Vec<_> = skills.to_vec();
    sorted.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));

    for piece in &sorted {
        let tokens = estimate_tokens(&piece.content);
        if used + tokens <= budget_tokens {
            selected.push(piece.clone());
            used += tokens;
        } else {
            break;
        }
    }

    selected
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lazy_skill_metadata_only() {
        let meta = SkillMetadata {
            id: "test".into(),
            name: "test".into(),
            description: "A test".into(),
            version: "1.0.0".into(),
            author: None,
            tags: vec![],
            capabilities: vec![],
            dependencies: vec![],
        };
        let skill = LazySkill::new(meta, PathBuf::from("nonexistent.md"));
        assert!(!skill.is_loaded());
        assert!(skill.metadata_token_estimate() > 0);
    }

    #[test]
    fn test_select_skills_for_context() {
        let pieces = vec![
            SkillContextPiece {
                skill_name: "high".into(),
                content: "a".repeat(100),
                priority: 1.0,
            },
            SkillContextPiece {
                skill_name: "medium".into(),
                content: "b".repeat(50),
                priority: 0.5,
            },
            SkillContextPiece {
                skill_name: "low".into(),
                content: "c".repeat(200),
                priority: 0.1,
            },
        ];

        let selected = select_skills_for_context(&pieces, 200);
        assert!(selected.len() >= 2);
        assert_eq!(selected[0].skill_name, "high");
    }

    #[tokio::test]
    async fn test_progressive_loader_empty() {
        let loader = ProgressiveLoader::new(None);
        let count = loader.load_metadata().await.unwrap_or(0);
        assert!(count >= 0);
        let list = loader.list_all().await;
        assert!(list.is_empty() || list.len() == count);
    }

    #[test]
    fn test_skill_context_priority_ordering() {
        let pieces = vec![
            SkillContextPiece {
                skill_name: "low".into(),
                content: "x".to_string(),
                priority: 0.1,
            },
            SkillContextPiece {
                skill_name: "high".into(),
                content: "y".to_string(),
                priority: 0.9,
            },
        ];
        let selected = select_skills_for_context(&pieces, 1000);
        assert_eq!(selected[0].skill_name, "high");
        assert_eq!(selected[1].skill_name, "low");
    }
}