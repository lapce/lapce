//! Cross-Session Cache - Persists cache across sessions.
//!
//! This module provides:
//! - Persistent cache storage
//! - Session-to-session reuse
//! - Cache sharing across users
//! - Cache invalidation strategies

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cached response entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrossSessionCacheEntry {
    pub id: String,
    pub request_hash: String,
    pub request: String,
    pub response: String,
    pub created_at: u64,
    pub last_used: u64,
    pub use_count: usize,
    pub session_ids: Vec<String>,
    pub tags: Vec<String>,
}

/// Cross-session cache configuration.
#[derive(Debug, Clone)]
pub struct CrossSessionCacheConfig {
    pub storage_path: Option<String>,
    pub max_entries: usize,
    pub ttl_days: u32,
    pub min_use_count: usize,
    pub enable_persistence: bool,
    pub sync_interval_secs: u64,
}

impl Default for CrossSessionCacheConfig {
    fn default() -> Self {
        Self {
            storage_path: Some(".deepseek_cache".to_string()),
            max_entries: 10000,
            ttl_days: 7,
            min_use_count: 2,
            enable_persistence: true,
            sync_interval_secs: 300,
        }
    }
}

/// Cross-session cache manager.
pub struct CrossSessionCache {
    config: CrossSessionCacheConfig,
    entries: Arc<RwLock<HashMap<String, CrossSessionCacheEntry>>>,
    request_index: Arc<RwLock<HashMap<String, String>>>,  // request_hash -> entry_id
    tag_index: Arc<RwLock<HashMap<String, Vec<String>>>>, // tag -> entry_ids
    stats: Arc<RwLock<CrossSessionStats>>,
}

impl CrossSessionCache {
    pub fn new(config: CrossSessionCacheConfig) -> Self {
        Self {
            config,
            entries: Arc::new(RwLock::new(HashMap::new())),
            request_index: Arc::new(RwLock::new(HashMap::new())),
            tag_index: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CrossSessionStats::default())),
        }
    }

    /// Get cached response for a request.
    pub async fn get(&self, request: &str, session_id: &str) -> Option<String> {
        let request_hash = self.hash_request(request);

        let mut entries = self.entries.write().await;

        if let Some(entry_id) = self.request_index.read().await.get(&request_hash) {
            if let Some(entry) = entries.get_mut(entry_id) {
                // Check TTL
                let age_days = (current_timestamp() - entry.created_at) / 86400;
                if age_days > self.config.ttl_days as u64 {
                    entries.remove(entry_id);
                    drop(entries);
                    self.remove_from_indices(entry_id).await;
                    return None;
                }

                // Update stats
                entry.last_used = current_timestamp();
                entry.use_count += 1;

                // Track session usage
                if !entry.session_ids.contains(&session_id.to_string()) {
                    entry.session_ids.push(session_id.to_string());
                }

                let response_clone = entry.response.clone();
                drop(entries);
                self.update_stats_hit().await;
                return Some(response_clone);
            }
        }

        drop(entries);
        self.update_stats_miss().await;
        None
    }

    /// Store a response in cache.
    pub async fn put(&self, request: &str, response: &str, session_id: &str, tags: Vec<String>) {
        let request_hash = self.hash_request(request);
        let entry_id = format!("entry_{}", current_timestamp());

        let entry = CrossSessionCacheEntry {
            id: entry_id.clone(),
            request_hash: request_hash.clone(),
            request: request.to_string(),
            response: response.to_string(),
            created_at: current_timestamp(),
            last_used: current_timestamp(),
            use_count: 1,
            session_ids: vec![session_id.to_string()],
            tags: tags.clone(),
        };

        let mut entries = self.entries.write().await;

        // Evict old entries if necessary
        if entries.len() >= self.config.max_entries {
            self.evict_oldest(&mut entries).await;
        }

        entries.insert(entry_id.clone(), entry);

        // Update indices
        self.request_index.write().await.insert(request_hash, entry_id.clone());

        drop(entries);

        // Update tag index
        let mut tag_index = self.tag_index.write().await;
        for tag in tags {
            tag_index.entry(tag).or_insert_with(Vec::new).push(entry_id.clone());
        }
    }

    /// Get responses by tag.
    pub async fn get_by_tag(&self, tag: &str) -> Vec<(String, String)> {
        let tag_index = self.tag_index.read().await;
        let entries = self.entries.read().await;

        let mut results = Vec::new();

        if let Some(entry_ids) = tag_index.get(tag) {
            for entry_id in entry_ids {
                if let Some(entry) = entries.get(entry_id) {
                    results.push((entry.request.clone(), entry.response.clone()));
                }
            }
        }

        results
    }

    /// Get entries used by multiple sessions.
    pub async fn get_cross_session_entries(&self) -> Vec<CrossSessionCacheEntry> {
        let entries = self.entries.read().await;

        entries
            .values()
            .filter(|e| e.session_ids.len() > 1)
            .cloned()
            .collect()
    }

    /// Get popular entries.
    pub async fn get_popular_entries(&self, limit: usize) -> Vec<CrossSessionCacheEntry> {
        let entries = self.entries.read().await;

        let mut sorted: Vec<_> = entries.values().cloned().collect();
        sorted.sort_by(|a, b| b.use_count.cmp(&a.use_count));

        sorted.into_iter().take(limit).collect()
    }

    /// Get recent entries.
    pub async fn get_recent_entries(&self, limit: usize) -> Vec<CrossSessionCacheEntry> {
        let entries = self.entries.read().await;

        let mut sorted: Vec<_> = entries.values().cloned().collect();
        sorted.sort_by(|a, b| b.last_used.cmp(&a.last_used));

        sorted.into_iter().take(limit).collect()
    }

    /// Search cache entries.
    pub async fn search(&self, query: &str) -> Vec<CrossSessionCacheEntry> {
        let entries = self.entries.read().await;
        let query_lower = query.to_lowercase();

        entries
            .values()
            .filter(|e| {
                e.request.to_lowercase().contains(&query_lower) ||
                e.response.to_lowercase().contains(&query_lower) ||
                e.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .cloned()
            .collect()
    }

    /// Remove entry from indices.
    async fn remove_from_indices(&self, entry_id: &str) {
        if let Some(entry) = self.entries.read().await.get(entry_id) {
            self.request_index.write().await.remove(&entry.request_hash);

            for tag in &entry.tags {
                let mut tag_index = self.tag_index.write().await;
                if let Some(ids) = tag_index.get_mut(tag) {
                    ids.retain(|id| id != entry_id);
                }
            }
        }
    }

    /// Evict oldest entry.
    async fn evict_oldest(&self, entries: &mut HashMap<String, CrossSessionCacheEntry>) {
        // Find entry with lowest use count and oldest last_used
        let evict_id = entries.iter()
            .filter(|(_, e)| e.use_count < self.config.min_use_count)
            .min_by(|(_, a), (_, b)| a.last_used.cmp(&b.last_used))
            .map(|(id, _)| id.clone());

        if let Some(id) = evict_id {
            self.remove_from_indices(&id).await;
            entries.remove(&id);
        }
    }

    /// Hash request for cache key.
    fn hash_request(&self, request: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        request.hash(&mut hasher);
        format!("{:x}", hasher.finish())
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

    /// Clear cache.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();

        let mut request_index = self.request_index.write().await;
        request_index.clear();

        let mut tag_index = self.tag_index.write().await;
        tag_index.clear();
    }

    /// Get statistics.
    pub async fn stats(&self) -> CrossSessionStats {
        let entries = self.entries.read().await;
        let stats = self.stats.read().await;

        let total_use_count: usize = entries.values().map(|e| e.use_count).sum();
        let avg_use = if entries.is_empty() { 0.0 } else { total_use_count as f32 / entries.len() as f32 };

        let cross_session_count = entries.values().filter(|e| e.session_ids.len() > 1).count();

        CrossSessionStats {
            total_entries: entries.len(),
            max_entries: self.config.max_entries,
            hits: stats.hits,
            misses: stats.misses,
            total_requests: stats.total_requests,
            hit_rate: if stats.total_requests > 0 { stats.hits as f32 / stats.total_requests as f32 } else { 0.0 },
            avg_use_count: avg_use,
            cross_session_entries: cross_session_count,
        }
    }

    /// Load cache from disk.
    pub async fn load_from_disk(&self) -> std::io::Result<()> {
        if !self.config.enable_persistence {
            return Ok(());
        }

        if let Some(ref path) = self.config.storage_path {
            let cache_file = Path::new(path).join("cross_session_cache.json");

            if cache_file.exists() {
                let content = std::fs::read_to_string(&cache_file)?;
                let entries: Vec<CrossSessionCacheEntry> = serde_json::from_str(&content)?;

                let mut entries_map = self.entries.write().await;
                let mut request_idx = self.request_index.write().await;
                let mut tag_idx = self.tag_index.write().await;

                for entry in entries {
                    let entry_id = entry.id.clone();
                    request_idx.insert(entry.request_hash.clone(), entry_id.clone());
                    for tag in &entry.tags {
                        tag_idx.entry(tag.clone()).or_insert_with(Vec::new).push(entry_id.clone());
                    }
                    entries_map.insert(entry_id, entry);
                }
            }
        }

        Ok(())
    }

    /// Save cache to disk.
    pub async fn save_to_disk(&self) -> std::io::Result<()> {
        if !self.config.enable_persistence {
            return Ok(());
        }

        if let Some(ref path) = self.config.storage_path {
            let cache_dir = Path::new(path);
            std::fs::create_dir_all(cache_dir)?;

            let entries = self.entries.read().await;
            let entries_vec: Vec<_> = entries.values().cloned().collect();

            let cache_file = cache_dir.join("cross_session_cache.json");
            let content = serde_json::to_string_pretty(&entries_vec)?;
            std::fs::write(&cache_file, content)?;
        }

        Ok(())
    }
}

impl Default for CrossSessionCache {
    fn default() -> Self {
        Self::new(CrossSessionCacheConfig::default())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CrossSessionStats {
    pub total_entries: usize,
    pub max_entries: usize,
    pub hits: usize,
    pub misses: usize,
    pub total_requests: usize,
    pub hit_rate: f32,
    pub avg_use_count: f32,
    pub cross_session_entries: usize,
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: cross_session_cache.rs:373")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_put_get() {
        let cache = CrossSessionCache::default();

        cache.put("test request", "test response", "session1", vec!["test".to_string()]).await;

        let response = cache.get("test request", "session1").await;
        assert_eq!(response, Some("test response".to_string()));
    }

    #[tokio::test]
    async fn test_cross_session() {
        let cache = CrossSessionCache::default();

        cache.put("shared request", "shared response", "session1", vec![]).await;
        cache.get("shared request", "session1").await;
        cache.get("shared request", "session2").await;

        let entries = cache.get_cross_session_entries().await;
        assert!(!entries.is_empty());
    }

    #[tokio::test]
    async fn test_popular_entries() {
        let cache = CrossSessionCache::default();

        for i in 0..5 {
            cache.put(&format!("req{}", i), &format!("resp{}", i), "s1", vec![]).await;
        }

        // Use one entry multiple times
        for _ in 0..3 {
            cache.get("req0", "s1").await;
        }

        let popular = cache.get_popular_entries(3).await;
        assert!(!popular.is_empty());
    }

    #[tokio::test]
    async fn test_stats() {
        let cache = CrossSessionCache::default();

        cache.get("req1", "s1").await;
        cache.get("req1", "s1").await;
        cache.get("req2", "s1").await;

        let stats = cache.stats().await;
        assert_eq!(stats.total_requests, 3);
    }
}
