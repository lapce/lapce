//! Multi-Project Isolation — company profiles with data separation.
//!
//! Inspired by Paperclip's single-deployment-multi-company pattern.
//! Each "company" has its own `.carp/` config directory, skill store, audit log,
//! and data directory. The active company is stored in `~/.carp/active_company`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

// ============================================================================
// CompanyProfile
// ============================================================================

/// A company profile with isolated data directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyProfile {
    /// Company name (identifier).
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Path to company data root.
    pub data_dir: PathBuf,
    /// Optional git remote for syncing company config.
    #[serde(default)]
    pub git_remote: Option<String>,
    /// Environment variables for this company.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
}

impl CompanyProfile {
    /// Create a new company profile with default paths.
    pub fn new(name: &str, base_dir: &Path) -> Self {
        let data_dir = base_dir.join("companies").join(name);
        Self {
            name: name.to_string(),
            display_name: name.to_string(),
            data_dir,
            git_remote: None,
            env: HashMap::new(),
            description: None,
        }
    }

    /// Ensure all company directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(self.data_dir.join("skills"))?;
        fs::create_dir_all(self.data_dir.join("config"))?;
        fs::create_dir_all(self.data_dir.join("audit"))?;
        Ok(())
    }

    /// Get the skills directory for this company.
    pub fn skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    /// Get the config directory for this company.
    pub fn config_dir(&self) -> PathBuf {
        self.data_dir.join("config")
    }

    /// Get the audit log directory for this company.
    pub fn audit_dir(&self) -> PathBuf {
        self.data_dir.join("audit")
    }

    /// Get the full path to the company's config file.
    pub fn config_path(&self) -> PathBuf {
        self.config_dir().join("config.toml")
    }
}

// ============================================================================
// CompanyManager
// ============================================================================

/// Manages company profiles and active company switching.
pub struct CompanyManager {
    /// Root directory for all company data.
    base_dir: PathBuf,
    /// All registered companies.
    companies: HashMap<String, CompanyProfile>,
    /// Currently active company name.
    active_company: Option<String>,
}

impl CompanyManager {
    /// Open the company manager at the default location (`~/.carp/`).
    pub fn open() -> Result<Self> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow::anyhow!("Cannot determine home directory"))?;

        let base_dir = PathBuf::from(&home).join(".carp");
        Self::open_at(&base_dir)
    }

    /// Open the company manager at a specific base directory.
    pub fn open_at(base_dir: &Path) -> Result<Self> {
        fs::create_dir_all(base_dir)?;
        fs::create_dir_all(base_dir.join("companies"))?;

        // Read all company profiles
        let mut companies = HashMap::new();
        let companies_dir = base_dir.join("companies");
        if companies_dir.exists() {
            for entry in fs::read_dir(&companies_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let profile = CompanyProfile::new(&name, base_dir);
                    companies.insert(name, profile);
                }
            }
        }

        // Read active company
        let active_path = base_dir.join("active_company");
        let active_company = if active_path.exists() {
            Some(fs::read_to_string(&active_path)
                .map(|s| s.trim().to_string())
                .unwrap_or_default())
        } else {
            None
        };

        Ok(Self {
            base_dir: base_dir.to_path_buf(),
            companies,
            active_company,
        })
    }

    /// List all registered companies.
    pub fn list_companies(&self) -> Vec<&CompanyProfile> {
        self.companies.values().collect()
    }

    /// Get the active company profile.
    pub fn active(&self) -> Option<&CompanyProfile> {
        self.active_company.as_ref().and_then(|name| self.companies.get(name))
    }

    /// Get a company by name.
    pub fn get(&self, name: &str) -> Option<&CompanyProfile> {
        self.companies.get(name)
    }

    /// Create a new company profile.
    pub fn create(&mut self, name: &str, display_name: Option<&str>) -> Result<&CompanyProfile> {
        if self.companies.contains_key(name) {
            anyhow::bail!("Company '{}' already exists", name);
        }

        let mut profile = CompanyProfile::new(name, &self.base_dir);
        if let Some(dn) = display_name {
            profile.display_name = dn.to_string();
        }
        profile.ensure_dirs()?;

        self.companies.insert(name.to_string(), profile.clone());
        info!("Created company profile '{}'", name);
        Ok(self.companies.get(name).unwrap())
    }

    /// Switch active company.
    pub fn switch(&mut self, name: &str) -> Result<()> {
        if !self.companies.contains_key(name) {
            anyhow::bail!("Company '{}' not found. Create it first with `carp company init {}`", name, name);
        }

        let active_path = self.base_dir.join("active_company");
        fs::write(&active_path, name)?;
        self.active_company = Some(name.to_string());
        info!("Switched to company '{}'", name);
        Ok(())
    }

    /// Remove a company profile.
    pub fn remove(&mut self, name: &str) -> Result<()> {
        if !self.companies.contains_key(name) {
            anyhow::bail!("Company '{}' not found", name);
        }

        let profile = &self.companies[name];
        if profile.data_dir.exists() {
            fs::remove_dir_all(&profile.data_dir)?;
        }

        self.companies.remove(name);
        if self.active_company.as_deref() == Some(name) {
            self.active_company = None;
            let active_path = self.base_dir.join("active_company");
            let _ = fs::remove_file(&active_path);
        }

        info!("Removed company '{}'", name);
        Ok(())
    }

    /// Check if there's an active company.
    pub fn has_active(&self) -> bool {
        self.active_company.is_some()
    }

    /// Get company skill store path (if active).
    pub fn active_skills_dir(&self) -> Option<PathBuf> {
        self.active().map(|p| p.skills_dir())
    }

    /// Get company audit log path (if active).
    pub fn active_audit_dir(&self) -> Option<PathBuf> {
        self.active().map(|p| p.audit_dir())
    }

    /// Format company list for display.
    pub fn format_list(&self) -> String {
        let mut output = String::new();
        output.push_str("═══ Companies ═══\n\n");

        for company in self.companies.values() {
            let is_active = self.active_company.as_deref() == Some(&company.name);
            let marker = if is_active { "▶ " } else { "  " };
            output.push_str(&format!(
                "{} {} ({})\n",
                marker, company.display_name, company.name
            ));
            if let Some(ref desc) = company.description {
                output.push_str(&format!("       {}\n", desc));
            }
            output.push_str(&format!("       Data: {}\n", company.data_dir.display()));
        }

        if self.companies.is_empty() {
            output.push_str("  No companies created yet.\n");
            output.push_str("  Use `carp company init <name>` to create one.\n");
        }

        output
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_base_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("carp_company_test_{}", std::process::id()));
        p
    }

    #[test]
    fn test_company_profile_new() {
        let base = test_base_dir();
        let profile = CompanyProfile::new("test-co", &base);
        assert_eq!(profile.name, "test-co");
        assert!(profile.data_dir.to_string_lossy().contains("companies/test-co"));
    }

    #[test]
    fn test_company_manager_create_and_switch() {
        let base = test_base_dir();
        let mut mgr = CompanyManager::open_at(&base).unwrap();

        // Initially no active company
        assert!(!mgr.has_active());

        // Create a company
        mgr.create("dev", Some("Development")).unwrap();
        assert_eq!(mgr.list_companies().len(), 1);

        // Switch to it
        mgr.switch("dev").unwrap();
        assert!(mgr.has_active());
        assert_eq!(mgr.active().unwrap().name, "dev");
        assert_eq!(mgr.active().unwrap().display_name, "Development");

        // Cleanup
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_company_manager_list() {
        let base = test_base_dir();
        let mut mgr = CompanyManager::open_at(&base).unwrap();

        mgr.create("co1", None).unwrap();
        mgr.create("co2", None).unwrap();

        let list = mgr.list_companies();
        assert_eq!(list.len(), 2);

        // Cleanup
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_company_manager_remove() {
        let base = test_base_dir();
        let mut mgr = CompanyManager::open_at(&base).unwrap();

        mgr.create("temp", None).unwrap();
        assert_eq!(mgr.list_companies().len(), 1);

        mgr.remove("temp").unwrap();
        assert_eq!(mgr.list_companies().len(), 0);

        // Cleanup
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_company_manager_switch_nonexistent() {
        let base = test_base_dir();
        let mut mgr = CompanyManager::open_at(&base).unwrap();

        let result = mgr.switch("ghost");
        assert!(result.is_err());

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_company_manager_format_list() {
        let base = test_base_dir();
        let mut mgr = CompanyManager::open_at(&base).unwrap();

        let empty = mgr.format_list();
        assert!(empty.contains("No companies created"));

        mgr.create("alpha", Some("Alpha Corp")).unwrap();
        mgr.switch("alpha").unwrap();

        let list = mgr.format_list();
        assert!(list.contains("▶"));
        assert!(list.contains("Alpha Corp"));

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_company_ensure_dirs() {
        let base = test_base_dir();
        let profile = CompanyProfile::new("test-co", &base);
        profile.ensure_dirs().unwrap();

        assert!(profile.skills_dir().exists());
        assert!(profile.config_dir().exists());
        assert!(profile.audit_dir().exists());

        fs::remove_dir_all(&base).ok();
    }
}