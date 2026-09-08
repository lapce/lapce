//! Plan mode — task decomposition and execution (Claude Code pattern).
//!
//! Claude Code's /plan mode: AI first proposes a plan in a markdown file,
//! user reviews and approves, then AI executes step by step.
//!
//! ## Workflow
//!
//! ```text
//! /plan "Add user auth" → AI writes ~/.deepseek-carp/plans/add-user-auth.md
//! User reviews plan → /execute      → AI runs tools per the plan
//!                          /modify  → iterate on plan before executing
//! ```
//!
//! Plan files are stored as markdown with YAML frontmatter for metadata.

use std::path::PathBuf;
use crate::config::paths;
use serde::{Deserialize, Serialize};

/// A plan stored on disk as a markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Short slug for the plan file.
    pub slug: String,
    /// Human-readable title.
    pub title: String,
    /// Plan content (markdown with task list).
    pub content: String,
    /// Plan status.
    pub status: PlanStatus,
    /// When the plan was created.
    pub created_at: String,
    /// When the plan was last updated.
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlanStatus {
    /// AI is drafting the plan.
    Drafting,
    /// Plan is ready for review.
    Draft,
    /// User approved — execution in progress.
    Executing,
    /// Plan execution complete.
    Completed,
    /// Plan was cancelled/rejected.
    Cancelled,
}

/// Plan manager — create, list, load, execute plans.
pub struct PlanManager {
    plans_dir: PathBuf,
}

impl Default for PlanManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanManager {
    pub fn new() -> Self {
        let dir = paths::config_file().parent().unwrap_or(std::path::Path::new(".")).join("plans");
        std::fs::create_dir_all(&dir).ok();
        Self { plans_dir: dir }
    }

    /// Generate a unique slug from a title.
    pub fn slugify(title: &str) -> String {
        title.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .take(50)
            .collect()
    }

    /// Create a new plan file.
    pub fn create(&self, title: &str, content: &str) -> std::io::Result<Plan> {
        let slug = Self::slugify(title);
        let plan = Plan {
            slug: slug.clone(),
            title: title.to_string(),
            content: content.to_string(),
            status: PlanStatus::Draft,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        self.save(&plan)?;
        Ok(plan)
    }

    /// Save a plan to disk.
    pub fn save(&self, plan: &Plan) -> std::io::Result<()> {
        let path = self.path_for(&plan.slug);
        let mut content = String::new();
        content.push_str(&format!("# {}\n\n", plan.title));
        content.push_str(&format!("> Status: {:?} | Created: {} | Updated: {}\n\n", plan.status, plan.created_at, plan.updated_at));
        content.push_str("---\n\n");
        content.push_str(&plan.content);
        content.push_str("\n\n## Execution Steps\n\n");
        self.extract_tasks(&plan.content).iter().for_each(|task| {
            content.push_str(&format!("- [ ] {}\n", task));
        });
        std::fs::write(&path, &content)
    }

    /// Load a plan from disk.
    pub fn load(&self, slug: &str) -> Option<Plan> {
        let path = self.path_for(slug);
        let raw = std::fs::read_to_string(&path).ok()?;
        let title = raw.lines().next()
            .unwrap_or("Untitled")
            .trim_start_matches("# ")
            .to_string();
        let content = raw.lines()
            .skip_while(|l| !l.starts_with("---"))
            .skip(1)
            .take_while(|l| !l.starts_with("## Execution"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        Some(Plan {
            slug: slug.to_string(),
            title,
            content,
            status: PlanStatus::Draft,
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    /// List all saved plans.
    pub fn list(&self) -> Vec<String> {
        let mut plans = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.plans_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".md") {
                        plans.push(name.trim_end_matches(".md").to_string());
                    }
                }
            }
        }
        plans
    }

    /// Delete a plan.
    pub fn delete(&self, slug: &str) -> std::io::Result<()> {
        let path = self.path_for(slug);
        std::fs::remove_file(path)
    }

    /// Extract actionable tasks from plan content.
    /// Lines starting with `- []` or numbered lists.
    pub fn extract_tasks(&self, content: &str) -> Vec<String> {
        content.lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("- [ ]") || t.starts_with("-[]") || (t.starts_with(char::is_numeric) && t.contains('.'))
            })
            .map(|l| l.trim().to_string())
            .collect()
    }

    fn path_for(&self, slug: &str) -> PathBuf {
        self.plans_dir.join(format!("{}.md", slug))
    }
}

/// Build a plan-mode system prompt that instructs the AI to
/// produce not just a markdown task list but also the four OpenSpec
/// artifacts (Mermaid architecture, OpenAPI schema, Mermaid sequence,
/// HTML wireframe) — see `artifacts::openspec_plan_prompt`.
///
/// This replaces the plain prompt with the full OpenSpec plan prompt.
pub fn plan_mode_prompt(user_request: &str) -> String {
    crate::agent::plan::artifacts::openspec_plan_prompt(user_request)
}

/// Build an execution-mode prompt for running a specific plan.
pub fn execute_mode_prompt(plan_content: &str) -> String {
    format!(
        "Execute the following plan step by step:\n\n{}\n\n\
         Instructions:\n\
         - Execute each step in order\n\
         - Report results after each step\n\
         - If a step fails, stop and explain why\n\
         - Mark completed steps as done",
        plan_content
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        let slug = PlanManager::slugify("Add User Authentication System");
        assert_eq!(slug, "add-user-authentication-system");
    }

    #[test]
    fn test_extract_tasks() {
        let mgr = PlanManager::new();
        let content = "## Plan\n\n- [ ] Create User model\n- [ ] Add login endpoint\n- [ ] Write tests";
        let tasks = mgr.extract_tasks(content);
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn test_create_and_load_plan() {
        let mgr = PlanManager::new();
        let plan = mgr.create("Test Plan", "## Steps\n\n- [ ] Do something").unwrap();
        assert_eq!(plan.slug, "test-plan");

        let loaded = mgr.load("test-plan");
        assert!(loaded.is_some());

        mgr.delete("test-plan").unwrap();
    }
}
