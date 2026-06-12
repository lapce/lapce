//! DomainMapper — Auto-classify code files into business domains.
//!
//! Inspired by Understand Anything's Business Domain View.
//! Uses file path conventions, naming patterns, and optional tree-sitter analysis
//! to map code files to business domains.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Business domain levels (hierarchical).
#[derive(Debug, Clone, PartialEq)]
pub struct DomainInfo {
    pub level1: String,  // Top-level domain e.g. "Payment", "Order", "User"
    pub level2: String,  // Sub-domain e.g. "Refund", "Checkout"
    pub level3: String,  // Feature area e.g. "RefundPolicy", "RefundProcess"
    pub confidence: f64, // 0.0-1.0
}

/// Pre-defined domain patterns for common project structures.
#[derive(Debug, Clone)]
pub struct DomainConfig {
    /// File path patterns (glob-like) to domain mapping
    pub path_patterns: Vec<(String, DomainInfo)>,
    /// Package/module name patterns to domain mapping
    pub module_patterns: Vec<(String, DomainInfo)>,
    /// Default domain for unrecognized files
    pub default_domain: DomainInfo,
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            path_patterns: vec![
                // Enterprise CRUD patterns
                ("**/controller/**".into(), DomainInfo { level1: "API".into(), level2: "Controller".into(), level3: "".into(), confidence: 0.8 }),
                ("**/service/**".into(), DomainInfo { level1: "Business".into(), level2: "Service".into(), level3: "".into(), confidence: 0.8 }),
                ("**/repository/**".into(), DomainInfo { level1: "Data".into(), level2: "Repository".into(), level3: "".into(), confidence: 0.8 }),
                ("**/model/**".into(), DomainInfo { level1: "Data".into(), level2: "Model".into(), level3: "".into(), confidence: 0.8 }),
                ("**/dto/**".into(), DomainInfo { level1: "API".into(), level2: "DTO".into(), level3: "".into(), confidence: 0.7 }),
                ("**/entity/**".into(), DomainInfo { level1: "Data".into(), level2: "Entity".into(), level3: "".into(), confidence: 0.8 }),
                ("**/handler/**".into(), DomainInfo { level1: "API".into(), level2: "Handler".into(), level3: "".into(), confidence: 0.7 }),
                ("**/middleware/**".into(), DomainInfo { level1: "Infrastructure".into(), level2: "Middleware".into(), level3: "".into(), confidence: 0.8 }),
                ("**/config/**".into(), DomainInfo { level1: "Infrastructure".into(), level2: "Config".into(), level3: "".into(), confidence: 0.9 }),
                ("**/util/**".into(), DomainInfo { level1: "Infrastructure".into(), level2: "Utility".into(), level3: "".into(), confidence: 0.6 }),
                ("**/helper/**".into(), DomainInfo { level1: "Infrastructure".into(), level2: "Utility".into(), level3: "".into(), confidence: 0.5 }),

                // Java/Rust domain-driven patterns
                ("**/api/**".into(), DomainInfo { level1: "API".into(), level2: "".into(), level3: "".into(), confidence: 0.7 }),
                ("**/domain/**".into(), DomainInfo { level1: "Domain".into(), level2: "".into(), level3: "".into(), confidence: 0.9 }),
                ("**/infra/**".into(), DomainInfo { level1: "Infrastructure".into(), level2: "".into(), level3: "".into(), confidence: 0.8 }),
            ],
            module_patterns: vec![],
            default_domain: DomainInfo {
                level1: "Uncategorized".into(),
                level2: "".into(),
                level3: "".into(),
                confidence: 0.3,
            },
        }
    }
}

/// Maps files to business domains.
pub struct DomainMapper {
    config: DomainConfig,
    /// Cached mappings: file_path → domain
    cache: Mutex<HashMap<String, DomainInfo>>,
}

impl DomainMapper {
    pub fn new() -> Self {
        Self {
            config: DomainConfig::default(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_config(config: DomainConfig) -> Self {
        Self {
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Map a file to its business domain.
    pub fn map_file(&self, file_path: &Path) -> DomainInfo {
        let path_str = file_path.to_string_lossy().replace('\\', "/");

        // Check cache first
        if let Ok(cache) = self.cache.lock() {
            if let Some(domain) = cache.get(&path_str) {
                return domain.clone();
            }
        }

        // Check path patterns (simple glob matching)
        for (pattern, domain) in &self.config.path_patterns {
            if self.match_pattern(&path_str, pattern) {
                // Cache and return
                if let Ok(mut cache) = self.cache.lock() {
                    cache.insert(path_str.clone(), domain.clone());
                }
                return domain.clone();
            }
        }

        // Fall back to default
        let default = self.config.default_domain.clone();
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path_str, default.clone());
        }
        default
    }

    /// Simple glob-like pattern matching.
    /// Supports `**/` as prefix/suffix and `/**/` as directory wildcard.
    fn match_pattern(&self, path: &str, pattern: &str) -> bool {
        if pattern == "**" {
            return true;
        }

        let (pat, path_lower) = (pattern.to_lowercase(), path.to_lowercase());

        if pat.starts_with("**/") && pat.ends_with("/**") {
            let mid = &pat[3..pat.len() - 3];
            path_lower.contains(mid)
        } else if let Some(suffix) = pat.strip_prefix("**/") {
            if suffix.ends_with('/') {
                path_lower.contains(&suffix[..suffix.len() - 1])
            } else {
                path_lower.ends_with(suffix)
            }
        } else if pat.ends_with("/**") {
            let prefix = &pat[..pat.len() - 3];
            path_lower.starts_with(&prefix.to_lowercase())
        } else {
            path_lower == pat
        }
    }

    /// Get all files in a given domain.
    pub fn files_in_domain<'a>(
        &self,
        files: &'a [std::path::PathBuf],
        level: DomainLevel,
        domain: &str,
    ) -> Vec<&'a std::path::PathBuf> {
        files
            .iter()
            .filter(|f| {
                let info = self.map_file(f);
                match level {
                    DomainLevel::L1 => info.level1 == domain,
                    DomainLevel::L2 => info.level2 == domain,
                    DomainLevel::L3 => info.level3 == domain,
                }
            })
            .collect()
    }

    /// Generate a domain summary for a set of files.
    pub fn domain_summary(&self, files: &[std::path::PathBuf]) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for f in files {
            let info = self.map_file(f);
            *counts.entry(info.level1.clone()).or_insert(0) += 1;
        }
        let mut result: Vec<_> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// Clear the cache.
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Get a reference to the config (for inspection).
    pub fn config(&self) -> &DomainConfig {
        &self.config
    }
}

impl Default for DomainMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainLevel {
    L1,
    L2,
    L3,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_domain_mapper_default() {
        let mapper = DomainMapper::new();
        let path = Path::new("src/main.rs");
        let domain = mapper.map_file(path);
        // "src/main.rs" doesn't match any pattern, should get default
        assert_eq!(domain.level1, "Uncategorized");
        assert!((domain.confidence - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_domain_mapper_api_pattern() {
        let mapper = DomainMapper::new();
        let path = Path::new("src/api/users.rs");
        let domain = mapper.map_file(path);
        assert_eq!(domain.level1, "API");
        assert!((domain.confidence - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_domain_mapper_service_pattern() {
        let mapper = DomainMapper::new();
        let path = Path::new("src/service/order_service.rs");
        let domain = mapper.map_file(path);
        assert_eq!(domain.level1, "Business");
        assert_eq!(domain.level2, "Service");
    }

    #[test]
    fn test_domain_mapper_domain_summary() {
        let mapper = DomainMapper::new();
        let files = vec![
            PathBuf::from("src/api/users.rs"),
            PathBuf::from("src/api/products.rs"),
            PathBuf::from("src/service/order.rs"),
            PathBuf::from("src/config/settings.rs"),
            PathBuf::from("src/main.rs"),
        ];
        let summary = mapper.domain_summary(&files);
        // API should have 2, Business 1, Infrastructure 1, Uncategorized 1
        assert!(summary.iter().any(|(d, c)| d == "API" && *c == 2));
        assert!(summary.iter().any(|(d, c)| d == "Business" && *c == 1));
        assert!(summary.iter().any(|(d, c)| d == "Infrastructure" && *c == 1));
        assert!(summary.iter().any(|(d, c)| d == "Uncategorized" && *c == 1));
    }

    #[test]
    fn test_domain_mapper_files_in_domain() {
        let mapper = DomainMapper::new();
        let files = vec![
            PathBuf::from("src/api/users.rs"),
            PathBuf::from("src/service/order.rs"),
            PathBuf::from("src/model/user.rs"),
        ];
        let api_files = mapper.files_in_domain(&files, DomainLevel::L1, "API");
        assert_eq!(api_files.len(), 1);
        assert_eq!(api_files[0].to_string_lossy(), "src/api/users.rs");

        let data_files = mapper.files_in_domain(&files, DomainLevel::L1, "Data");
        assert_eq!(data_files.len(), 1);
        assert_eq!(data_files[0].to_string_lossy(), "src/model/user.rs");
    }

    #[test]
    fn test_domain_mapper_cache() {
        let mapper = DomainMapper::new();
        let path = Path::new("src/config/app.rs");
        let d1 = mapper.map_file(path);
        assert_eq!(d1.level1, "Infrastructure");

        // Clear cache and verify it's cleared (map again, should still work)
        mapper.clear_cache();
        let d2 = mapper.map_file(path);
        assert_eq!(d2.level1, "Infrastructure");
    }
}