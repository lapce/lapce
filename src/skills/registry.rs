//! Skill Registry - Skill registration and discovery

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use super::skill_trait::{Skill, SkillMetadata, SkillParams, SkillResult, SkillError, SkillInstance, SkillState};

/// Skill registry - manages all registered skills
pub struct SkillRegistry {
    skills: Arc<RwLock<HashMap<String, Arc<dyn Skill>>>>,
    instances: Arc<RwLock<HashMap<String, SkillInstance>>>,
    aliases: Arc<RwLock<HashMap<String, String>>>, // alias -> skill_id
    tags: Arc<RwLock<HashMap<String, Vec<String>>>>, // tag -> skill_ids
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
            aliases: Arc::new(RwLock::new(HashMap::new())),
            tags: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a skill
    pub async fn register<S: Skill + 'static>(&self, skill: S) -> Result<(), SkillError> {
        let metadata = skill.metadata();
        let skill_id = metadata.id.clone();

        // Check if already registered
        {
            let skills = self.skills.read().await;
            if skills.contains_key(&skill_id) {
                return Err(SkillError {
                    code: "ALREADY_REGISTERED".to_string(),
                    message: format!("Skill '{}' is already registered", skill_id),
                    details: None,
                    recoverable: false,
                });
            }
        }

        // Add to skills
        {
            let mut skills = self.skills.write().await;
            skills.insert(skill_id.clone(), Arc::new(skill));
        }

        // Add instance
        {
            let mut instances = self.instances.write().await;
            instances.insert(skill_id.clone(), SkillInstance::new(metadata.clone()));
        }

        // Index by tags
        {
            let mut tags = self.tags.write().await;
            for tag in &metadata.tags {
                tags.entry(tag.clone())
                    .or_insert_with(Vec::new)
                    .push(skill_id.clone());
            }
        }

        Ok(())
    }

    /// Register an alias for a skill
    pub async fn register_alias(&self, alias: &str, skill_id: &str) -> Result<(), SkillError> {
        let skills = self.skills.read().await;
        if !skills.contains_key(skill_id) {
            return Err(SkillError {
                code: "SKILL_NOT_FOUND".to_string(),
                message: format!("Skill '{}' not found", skill_id),
                details: None,
                recoverable: false,
            });
        }

        drop(skills);

        let mut aliases = self.aliases.write().await;
        aliases.insert(alias.to_string(), skill_id.to_string());

        Ok(())
    }

    /// Get a skill by ID
    pub async fn get(&self, skill_id: &str) -> Option<Arc<dyn Skill>> {
        // First check aliases
        let resolved_id = {
            let aliases = self.aliases.read().await;
            aliases.get(skill_id).cloned().unwrap_or_else(|| skill_id.to_string())
        };

        let skills = self.skills.read().await;
        skills.get(&resolved_id).cloned()
    }

    /// List all skills
    pub async fn list(&self) -> Vec<SkillMetadata> {
        let skills = self.skills.read().await;
        skills.values()
            .map(|s| s.metadata().clone())
            .collect()
    }

    /// Find skills by tag
    pub async fn find_by_tag(&self, tag: &str) -> Vec<SkillMetadata> {
        let skill_ids = {
            let tags = self.tags.read().await;
            tags.get(tag).cloned().unwrap_or_default()
        };

        let skills = self.skills.read().await;
        skill_ids.iter()
            .filter_map(|id| skills.get(id).map(|s| s.metadata().clone()))
            .collect()
    }

    /// Search skills by name or description
    pub async fn search(&self, query: &str) -> Vec<SkillMetadata> {
        let query_lower = query.to_lowercase();
        let skills = self.skills.read().await;
        
        skills.values()
            .filter_map(|s| {
                let meta = s.metadata();
                if meta.name.to_lowercase().contains(&query_lower) ||
                   meta.description.to_lowercase().contains(&query_lower) {
                    Some(meta.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Execute a skill by ID
    pub async fn execute(&self, skill_id: &str, params: SkillParams) -> Result<SkillResult, SkillError> {
        let skill = self.get(skill_id).await
            .ok_or_else(|| SkillError {
                code: "SKILL_NOT_FOUND".to_string(),
                message: format!("Skill '{}' not found", skill_id),
                details: None,
                recoverable: false,
            })?;

        // Validate parameters
        skill.validate(&params)?;

        // Update instance state
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(skill_id) {
                instance.state = SkillState::Initializing;
                instance.state_since = std::time::Instant::now();
            }
        }

        // Execute
        let result = skill.execute(params);

        // Update instance state
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(skill_id) {
                instance.state = match &result {
                    Ok(r) if r.success => SkillState::Completed,
                    Ok(_) => SkillState::Failed("Execution failed".to_string()),
                    Err(e) => SkillState::Failed(e.message.clone()),
                };
                instance.state_since = std::time::Instant::now();
            }
        }

        result
    }

    /// Get skill instance info
    pub async fn get_instance(&self, skill_id: &str) -> Option<SkillInstance> {
        let instances = self.instances.read().await;
        instances.get(skill_id).cloned()
    }

    /// Get all instances
    pub async fn get_all_instances(&self) -> Vec<SkillInstance> {
        let instances = self.instances.read().await;
        instances.values().cloned().collect()
    }

    /// Unregister a skill
    pub async fn unregister(&self, skill_id: &str) -> Result<(), SkillError> {
        // Remove from skills
        {
            let mut skills = self.skills.write().await;
            skills.remove(skill_id);
        }

        // Remove instance
        {
            let mut instances = self.instances.write().await;
            instances.remove(skill_id);
        }

        // Remove aliases
        {
            let mut aliases = self.aliases.write().await;
            aliases.retain(|_, v| v != skill_id);
        }

        // Remove from tags
        {
            let mut tags = self.tags.write().await;
            for (_, skill_ids) in tags.iter_mut() {
                skill_ids.retain(|id| id != skill_id);
            }
        }

        Ok(())
    }

    /// Check if a skill exists
    pub async fn contains(&self, skill_id: &str) -> bool {
        self.get(skill_id).await.is_some()
    }

    /// Get count of registered skills
    pub async fn count(&self) -> usize {
        let skills = self.skills.read().await;
        skills.len()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Skill loader - dynamically loads skills
pub struct SkillLoader {
    registry: Arc<SkillRegistry>,
    search_paths: Vec<PathBuf>,
}

impl SkillLoader {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry,
            search_paths: Vec::new(),
        }
    }

    /// Add a search path for skills
    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Set default search paths
    pub fn set_default_paths(&mut self) {
        self.search_paths = vec![
            PathBuf::from("./skills"),
            PathBuf::from("~/.config/deepseek-carp/skills"),
            PathBuf::from("/usr/local/share/deepseek-carp/skills"),
        ];
    }

    /// Load built-in skills
    pub async fn load_builtin_skills(&self) -> Result<usize, SkillError> {
        let mut count = 0;

        // Git skill
        self.registry.register(crate::skills::builtin::GitSkill::new()).await?;
        count += 1;

        // Terminal skill
        self.registry.register(crate::skills::builtin::TerminalSkill::new()).await?;
        count += 1;

        // Test skill
        self.registry.register(crate::skills::builtin::TestSkill::new()).await?;
        count += 1;

        // Search skill
        self.registry.register(crate::skills::builtin::SearchSkill::new()).await?;
        count += 1;

        Ok(count)
    }

    /// Load skills from directory
    pub async fn load_from_directory(&self, path: &PathBuf) -> Result<usize, SkillError> {
        use std::fs;

        if !path.exists() {
            return Ok(0);
        }

        let mut count = 0;

        // Note: Dynamic loading of Rust skills requires compile-time linking
        // For now, we support loading skill configurations
        let entries = fs::read_dir(path)
            .map_err(|e| SkillError {
                code: "IO_ERROR".to_string(),
                message: format!("Failed to read directory: {}", e),
                details: None,
                recoverable: false,
            })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(_config) = serde_json::from_str::<serde_json::Value>(&content) {
                        // In a full implementation, we would load the skill here
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Load all configured skills
    pub async fn load_all(&self) -> Result<usize, SkillError> {
        let mut total = 0;

        // Load built-in skills
        total += self.load_builtin_skills().await?;

        // Load from search paths
        for path in &self.search_paths {
            total += self.load_from_directory(path).await?;
        }

        Ok(total)
    }
}
