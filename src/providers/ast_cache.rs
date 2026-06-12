//! AST Cache Enhancement - Incremental parsing optimization.
//!
//! This module provides:
//! - AST caching with invalidation
//! - Incremental re-parsing
//! - Cache warming
//! - Memory management

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cached AST node.
#[derive(Debug, Clone)]
pub struct AstCacheEntry {
    pub file_hash: u64,
    pub ast: CachedAst,
    pub created_at: u64,
    pub last_access: u64,
    pub access_count: usize,
    pub dependencies: Vec<u64>,
}

/// A simplified cached AST representation.
#[derive(Debug, Clone)]
pub struct CachedAst {
    pub root_node: AstNode,
    pub total_nodes: usize,
    pub language: String,
    pub version: u32,
}

/// An AST node representation.
#[derive(Debug, Clone)]
pub struct AstNode {
    pub kind: String,
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub children: Vec<AstNode>,
    pub metadata: HashMap<String, String>,
}

/// AST cache configuration.
#[derive(Debug, Clone)]
pub struct AstCacheConfig {
    pub max_entries: usize,
    pub max_memory_mb: usize,
    pub ttl_secs: u64,
    pub enable_incremental: bool,
}

impl Default for AstCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            max_memory_mb: 512,
            ttl_secs: 3600,
            enable_incremental: true,
        }
    }
}

/// An AST change for incremental updates.
#[derive(Debug, Clone)]
pub struct AstChange {
    pub start_byte: usize,
    pub end_byte: usize,
    pub old_text: String,
    pub new_text: String,
}

/// AST cache with incremental parsing support.
pub struct AstCache {
    config: AstCacheConfig,
    entries: Arc<RwLock<HashMap<u64, AstCacheEntry>>>,
    access_order: Arc<RwLock<VecDeque<u64>>>,
    stats: Arc<RwLock<AstCacheStats>>,
}

impl AstCache {
    pub fn new(config: AstCacheConfig) -> Self {
        Self {
            config,
            entries: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(AstCacheStats::default())),
        }
    }

    /// Get cached AST for a file.
    pub async fn get(&self, file_path: &str, file_content: &str) -> Option<CachedAst> {
        let file_hash = self.hash_content(file_content);
        let key = self.compute_key(file_path, file_hash);

        let mut entries = self.entries.write().await;

        if let Some(entry) = entries.get_mut(&key) {
            // Check TTL
            if current_timestamp() - entry.created_at > self.config.ttl_secs {
                entries.remove(&key);
                drop(entries);
                self.update_stats_miss().await;
                return None;
            }

            // Update access
            entry.last_access = current_timestamp();
            entry.access_count += 1;

            drop(entries);
            self.update_lru(key).await;
            self.update_stats_hit().await;

            let entries = self.entries.read().await;
            return entries.get(&key).map(|e| e.ast.clone());
        }

        drop(entries);
        self.update_stats_miss().await;
        None
    }

    /// Store AST in cache.
    pub async fn put(&self, file_path: &str, file_content: &str, ast: CachedAst, dependencies: Vec<u64>) {
        let file_hash = self.hash_content(file_content);
        let key = self.compute_key(file_path, file_hash);

        let entry = AstCacheEntry {
            file_hash,
            ast,
            created_at: current_timestamp(),
            last_access: current_timestamp(),
            access_count: 1,
            dependencies,
        };

        let mut entries = self.entries.write().await;

        // Evict if necessary
        if entries.len() >= self.config.max_entries {
            self.evict_lru(&mut entries).await;
        }

        entries.insert(key, entry);

        // Update LRU
        drop(entries);
        let mut order = self.access_order.write().await;
        order.push_back(key);
    }

    /// Compute incremental update for a file change.
    pub async fn get_incremental_update(
        &self,
        file_path: &str,
        old_content: &str,
        new_content: &str,
    ) -> Option<IncrementalAstUpdate> {
        if !self.config.enable_incremental {
            return None;
        }

        let old_hash = self.hash_content(old_content);
        let new_hash = self.hash_content(new_content);

        if old_hash == new_hash {
            return None;
        }

        // Get old AST
        let old_key = self.compute_key(file_path, old_hash);
        let entries = self.entries.read().await;
        let old_entry = entries.get(&old_key)?;

        // Find changed regions
        let changes = self.find_changes(old_content, new_content);

        // Compute affected nodes
        let affected_ranges = self.find_affected_ranges(&old_entry.ast.root_node, &changes);

        Some(IncrementalAstUpdate {
            old_ast: old_entry.ast.clone(),
            new_content: new_content.to_string(),
            changes,
            affected_ranges,
            invalidated: false,
        })
    }

    /// Invalidate cache entry.
    pub async fn invalidate(&self, file_path: &str, file_content: &str) {
        let file_hash = self.hash_content(file_content);
        let key = self.compute_key(file_path, file_hash);

        let mut entries = self.entries.write().await;
        entries.remove(&key);

        // Remove from LRU
        let mut order = self.access_order.write().await;
        order.retain(|&k| k != key);
    }

    /// Invalidate files that depend on a given file.
    pub async fn invalidate_dependents(&self, file_hash: u64) {
        let mut entries = self.entries.write().await;
        let to_remove: Vec<u64> = entries.iter()
            .filter(|(_, e)| e.dependencies.contains(&file_hash))
            .map(|(&k, _)| k)
            .collect();

        for key in to_remove {
            entries.remove(&key);
        }
    }

    /// Warm up cache with common files.
    pub async fn warmup(&self, files: Vec<(String, String, CachedAst)>) {
        for (path, content, ast) in files {
            self.put(&path, &content, ast, Vec::new()).await;
        }
    }

    /// Clear all cache.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();

        let mut order = self.access_order.write().await;
        order.clear();
    }

    /// Find changes between old and new content.
    fn find_changes(&self, old_content: &str, new_content: &str) -> Vec<AstChange> {
        let mut changes = Vec::new();

        // Simple line-by-line diff
        let old_lines: Vec<&str> = old_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();

        let mut i = 0;
        let mut j = 0;

        while i < old_lines.len() || j < new_lines.len() {
            if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
                i += 1;
                j += 1;
            } else if i < old_lines.len() && (j >= new_lines.len() || !old_lines[i].is_empty()) {
                // Line deleted
                let start = old_content.lines().take(i).map(|l| l.len() + 1).sum();
                let end = start + old_lines[i].len();

                changes.push(AstChange {
                    start_byte: start,
                    end_byte: end,
                    old_text: old_lines[i].to_string(),
                    new_text: String::new(),
                });
                i += 1;
            } else if j < new_lines.len() {
                // Line added
                let start = new_content.lines().take(j).map(|l| l.len() + 1).sum();
                let end = start + new_lines[j].len();

                changes.push(AstChange {
                    start_byte: start,
                    end_byte: end,
                    old_text: String::new(),
                    new_text: new_lines[j].to_string(),
                });
                j += 1;
            }
        }

        changes
    }

    /// Find AST node ranges affected by changes.
    fn find_affected_ranges(&self, root: &AstNode, changes: &[AstChange]) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();

        for change in changes {
            let affected = self.find_nodes_in_range(root, change.start_byte, change.end_byte);
            for node in affected {
                ranges.push((node.start_byte, node.end_byte));
            }
        }

        ranges
    }

    /// Find nodes within a byte range.
    fn find_nodes_in_range<'a>(&self, node: &'a AstNode, start: usize, end: usize) -> Vec<&'a AstNode> {
        let mut results = Vec::new();

        // Check if this node is within range
        if node.start_byte <= end && node.end_byte >= start {
            results.push(node);
        }

        // Recurse into children
        for child in &node.children {
            let child_results = self.find_nodes_in_range(child, start, end);
            results.extend(child_results);
        }

        results
    }

    /// Compute cache key.
    fn compute_key(&self, file_path: &str, content_hash: u64) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        file_path.hash(&mut hasher);
        content_hash.hash(&mut hasher);
        hasher.finish()
    }

    /// Hash file content.
    fn hash_content(&self, content: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Update LRU order.
    async fn update_lru(&self, key: u64) {
        let mut order = self.access_order.write().await;
        order.retain(|&k| k != key);
        order.push_back(key);
    }

    /// Evict LRU entry.
    async fn evict_lru(&self, entries: &mut HashMap<u64, AstCacheEntry>) {
        let mut order = self.access_order.write().await;
        if let Some(key) = order.pop_front() {
            entries.remove(&key);
        }
    }

    /// Update hit stats.
    async fn update_stats_hit(&self) {
        let mut stats = self.stats.write().await;
        stats.hits += 1;
        stats.total_requests += 1;
    }

    /// Update miss stats.
    async fn update_stats_miss(&self) {
        let mut stats = self.stats.write().await;
        stats.misses += 1;
        stats.total_requests += 1;
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> AstCacheStats {
        let entries = self.entries.read().await;
        let stats = self.stats.read().await;

        AstCacheStats {
            entries: entries.len(),
            max_entries: self.config.max_entries,
            memory_estimate_mb: entries.len() * 10 / 1024, // Rough estimate
            hits: stats.hits,
            misses: stats.misses,
            total_requests: stats.total_requests,
            hit_rate: if stats.total_requests > 0 {
                stats.hits as f32 / stats.total_requests as f32
            } else {
                0.0
            },
        }
    }
}

impl Default for AstCache {
    fn default() -> Self {
        Self::new(AstCacheConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct IncrementalAstUpdate {
    pub old_ast: CachedAst,
    pub new_content: String,
    pub changes: Vec<AstChange>,
    pub affected_ranges: Vec<(usize, usize)>,
    pub invalidated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AstCacheStats {
    pub entries: usize,
    pub max_entries: usize,
    pub memory_estimate_mb: usize,
    pub hits: usize,
    pub misses: usize,
    pub total_requests: usize,
    pub hit_rate: f32,
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: ast_cache.rs:406")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_put_get() {
        let cache = AstCache::default();

        let ast = CachedAst {
            root_node: AstNode {
                kind: "module".to_string(),
                text: "fn main() {}".to_string(),
                start_byte: 0,
                end_byte: 12,
                children: Vec::new(),
                metadata: HashMap::new(),
            },
            total_nodes: 1,
            language: "rust".to_string(),
            version: 1,
        };

        cache.put("test.rs", "fn main() {}", ast.clone(), vec![]).await;

        let cached = cache.get("test.rs", "fn main() {}").await;
        assert!(cached.is_some());
    }

    #[tokio::test]
    async fn test_incremental_update() {
        let cache = AstCache::default();

        let old_content = "fn main() {\n    println!(\"hello\");\n}";
        let new_content = "fn main() {\n    println!(\"hello world\");\n}";

        let update = cache.get_incremental_update("test.rs", old_content, new_content).await;
        assert!(update.is_some());
    }

    #[tokio::test]
    async fn test_stats() {
        let cache = AstCache::default();

        cache.get("test.rs", "fn main() {}").await;
        cache.get("test.rs", "fn main() {}").await;
        cache.get("test2.rs", "fn other() {}").await;

        let stats = cache.stats().await;
        assert_eq!(stats.total_requests, 3);
    }
}
