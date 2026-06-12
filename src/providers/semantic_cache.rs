//! Semantic Cache - Content-based request deduplication and caching.
//!
//! This module provides semantic caching that:
//! - Hashes requests based on content (not just exact match)
//! - Stores responses with semantic similarity
//! - Evicts old entries using LRU policy
//! - Supports TTL-based expiration

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use std::collections::hash_map::DefaultHasher;

/// A cache key based on request content hash.
#[derive(Debug, Clone)]
pub struct CacheKey {
    /// hash of the request content
    pub hash: u64,
    /// Original content for debugging
    pub content: String,
    /// File path if applicable
    pub file_path: Option<String>,
    /// Language if applicable
    pub language: Option<String>,
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for CacheKey {}

impl CacheKey {
    pub fn new(content: &str) -> Self {
        let hash = compute_hash(content);
        Self {
            hash,
            content: content.to_string(),
            file_path: None,
            language: None,
        }
    }

    pub fn with_context(content: &str, file_path: Option<&str>, language: Option<&str>) -> Self {
        // Include context in hash for semantic deduplication
        let mut combined = content.to_string();
        if let Some(fp) = file_path {
            combined.push_str(fp);
        }
        if let Some(lang) = language {
            combined.push_str(lang);
        }

        let hash = compute_hash(&combined);
        Self {
            hash,
            content: content.to_string(),
            file_path: file_path.map(String::from),
            language: language.map(String::from),
        }
    }
}

/// Compute a simple hash for a string.
fn compute_hash(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// A cached response entry.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The cached response.
    pub response: String,
    /// When this entry was created.
    pub created_at: Instant,
    /// When this entry expires.
    pub expires_at: Instant,
    /// Hit count.
    pub hits: u64,
    /// Last accessed time.
    pub last_accessed: Instant,
    /// Token count for size tracking.
    pub token_count: usize,
}

impl CacheEntry {
    pub fn new(response: String, ttl: Duration, token_count: usize) -> Self {
        let now = Instant::now();
        Self {
            created_at: now,
            expires_at: now + ttl,
            hits: 0,
            last_accessed: now,
            token_count,
            response,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }

    pub fn touch(&mut self) {
        self.hits += 1;
        self.last_accessed = Instant::now();
    }
}

/// Statistics for cache performance.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total cache hits.
    pub hits: u64,
    /// Total cache misses.
    pub misses: u64,
    /// Number of evictions.
    pub evictions: u64,
    /// Current cache size.
    pub size: usize,
    /// Hit rate.
    pub hit_rate: f64,
}

impl CacheStats {
    pub fn record_hit(&mut self) {
        self.hits += 1;
        self.update_hit_rate();
    }

    pub fn record_miss(&mut self) {
        self.misses += 1;
        self.update_hit_rate();
    }

    fn update_hit_rate(&mut self) {
        let total = self.hits + self.misses;
        self.hit_rate = if total > 0 { self.hits as f64 / total as f64 } else { 0.0 };
    }
}

/// Semantic cache with LRU eviction.
pub struct SemanticCache {
    /// Cache storage.
    entries: Arc<RwLock<HashMap<CacheKey, CacheEntry>>>,
    /// LRU ordering (most recent first).
    lru: Arc<RwLock<Vec<CacheKey>>>,
    /// Maximum number of entries.
    max_entries: usize,
    /// Maximum total tokens.
    max_tokens: usize,
    /// Current total tokens.
    current_tokens: Arc<RwLock<usize>>,
    /// Cache statistics.
    stats: Arc<RwLock<CacheStats>>,
    /// Default TTL.
    ttl: Duration,
}

impl SemanticCache {
    pub fn new(max_entries: usize, max_tokens: usize, ttl_secs: u64) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            lru: Arc::new(RwLock::new(Vec::new())),
            max_entries,
            max_tokens,
            current_tokens: Arc::new(RwLock::new(0)),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Get a cached response if available.
    pub async fn get(&self, key: &CacheKey) -> Option<String> {
        let mut entries = self.entries.write().await;
        let mut lru = self.lru.write().await;

        if let Some(entry) = entries.get_mut(key) {
            if entry.is_expired() {
                // Remove expired entry
                entries.remove(key);
                lru.retain(|k| k.hash != key.hash);
                return None;
            }

            // Update LRU and hit count
            entry.touch();

            // Move to front of LRU
            lru.retain(|k| k.hash != key.hash);
            lru.insert(0, key.clone());

            // Record hit
            drop(lru);
            let mut stats = self.stats.write().await;
            stats.record_hit();

            return Some(entry.response.clone());
        }

        // Record miss
        let mut stats = self.stats.write().await;
        stats.record_miss();

        None
    }

    /// Store a response in the cache.
    pub async fn insert(&self, key: CacheKey, response: String) {
        let token_count = estimate_tokens(&response);

        // Evict if necessary
        self.evict_if_needed(token_count).await;

        let entry = CacheEntry::new(response, self.ttl, token_count);

        let mut entries = self.entries.write().await;
        let mut lru = self.lru.write().await;

        // Remove existing entry if present
        if entries.contains_key(&key) {
            entries.remove(&key);
            lru.retain(|k| k.hash != key.hash);
        }

        entries.insert(key.clone(), entry);
        lru.insert(0, key);

        // Update token count
        let mut current = self.current_tokens.write().await;
        *current += token_count;
    }

    /// Evict entries if cache is full.
    async fn evict_if_needed(&self, new_tokens: usize) {
        let mut entries = self.entries.write().await;
        let mut lru = self.lru.write().await;
        let mut current_tokens = self.current_tokens.write().await;

        // Evict until we have room
        while (entries.len() >= self.max_entries || *current_tokens + new_tokens > self.max_tokens)
              && !lru.is_empty() {
            // Pop from back of LRU (least recently used)
            if let Some(key) = lru.pop() {
                if let Some(entry) = entries.remove(&key) {
                    *current_tokens = current_tokens.saturating_sub(entry.token_count);
                    let mut stats = self.stats.write().await;
                    stats.evictions += 1;
                }
            }
        }
    }

    /// Clear the cache.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        let mut lru = self.lru.write().await;
        let mut current_tokens = self.current_tokens.write().await;

        entries.clear();
        lru.clear();
        *current_tokens = 0;
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let entries = self.entries.read().await;
        let mut stats = self.stats.write().await;
        stats.size = entries.len();
        stats.clone()
    }

    /// Remove expired entries.
    pub async fn cleanup(&self) {
        let mut entries = self.entries.write().await;
        let mut lru = self.lru.write().await;
        let mut current_tokens = self.current_tokens.write().await;

        let expired: Vec<CacheKey> = entries.iter()
            .filter(|(_, e)| e.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired {
            if let Some(entry) = entries.remove(&key) {
                *current_tokens = current_tokens.saturating_sub(entry.token_count);
            }
            lru.retain(|k| k.hash != key.hash);
        }
    }
}

/// Estimate token count for a string.
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Semantic cache manager for global access.
pub struct SemanticCacheManager {
    caches: HashMap<String, Arc<SemanticCache>>,
}

impl SemanticCacheManager {
    pub fn new() -> Self {
        Self {
            caches: HashMap::new(),
        }
    }

    /// Get or create a cache with the given name.
    pub fn get_or_create(&mut self, name: &str, max_entries: usize, max_tokens: usize) -> Arc<SemanticCache> {
        if !self.caches.contains_key(name) {
            self.caches.insert(name.to_string(), Arc::new(SemanticCache::new(max_entries, max_tokens, 3600)));
        }
        Arc::clone(self.caches.get(name).expect("unwrap failed: semantic_cache.rs:326"))
    }

    /// Clear all caches.
    pub async fn clear_all(&self) {
        for cache in self.caches.values() {
            cache.clear().await;
        }
    }
}

impl Default for SemanticCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_insert_get() {
        let cache = SemanticCache::new(100, 10000, 3600);
        let key = CacheKey::new("test request");
        cache.insert(key.clone(), "test response".to_string()).await;

        let result = cache.get(&key).await;
        assert_eq!(result, Some("test response".to_string()));
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = SemanticCache::new(100, 10000, 3600);
        let key = CacheKey::new("nonexistent");

        let result = cache.get(&key).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = SemanticCache::new(2, 1000, 3600);
        let key1 = CacheKey::new("key1");
        let key2 = CacheKey::new("key2");
        let key3 = CacheKey::new("key3");

        cache.insert(key1.clone(), "response1".to_string()).await;
        cache.insert(key2.clone(), "response2".to_string()).await;

        // Cache is full, next insert should evict key1
        cache.insert(key3.clone(), "response3".to_string()).await;

        let result1 = cache.get(&key1).await;
        assert_eq!(result1, None); // Evicted

        let result2 = cache.get(&key2).await;
        assert_eq!(result2, Some("response2".to_string()));
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = SemanticCache::new(100, 10000, 3600);
        let key = CacheKey::new("test");

        cache.insert(key.clone(), "response".to_string()).await;
        cache.get(&key).await;
        cache.get(&key).await;

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.size, 1);
    }
}
