//! Prefix Cache - Reuses KV cache for common prompt prefixes.
//!
//! This module provides:
//! - Prefix-based KV cache reuse
//! - Token matching and alignment
//! - Cache invalidation on context changes

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cached prefix entry.
#[derive(Debug, Clone)]
pub struct PrefixCacheEntry {
    /// The prefix tokens.
    pub prefix: Vec<String>,
    /// Cached KV state (serialized).
    pub kv_cache: Vec<Vec<f32>>,
    /// Number of tokens in prefix.
    pub token_count: usize,
    /// Last access timestamp.
    pub last_access: u64,
    /// Hit count.
    pub hit_count: usize,
}

/// Prefix cache statistics.
#[derive(Debug, Clone, Default)]
pub struct PrefixCacheStats {
    pub total_requests: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub prefix_reuses: usize,
    pub tokens_saved: usize,
}

/// Prefix cache manager.
pub struct PrefixCache {
    entries: Arc<RwLock<HashMap<String, PrefixCacheEntry>>>,
    lru_order: Arc<RwLock<VecDeque<String>>>,
    stats: Arc<RwLock<PrefixCacheStats>>,
    max_entries: usize,
    max_prefix_len: usize,
}

impl PrefixCache {
    pub fn new(max_entries: usize, max_prefix_len: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            lru_order: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(PrefixCacheStats::default())),
            max_entries,
            max_prefix_len,
        }
    }

    /// Get cache entry for prefix.
    pub async fn get(&self, tokens: &[String]) -> Option<PrefixCacheEntry> {
        let prefix_key = self.tokens_to_key(tokens);

        // Try exact match first
        {
            let entries = self.entries.read().await;
            if let Some(entry) = entries.get(&prefix_key).cloned() {
                drop(entries);
                // Update LRU
                self.touch(&prefix_key).await;

                // Update stats
                let mut stats = self.stats.write().await;
                stats.cache_hits += 1;
                stats.tokens_saved += entry.token_count;

                let mut entry = entry;
                entry.hit_count += 1;
                entry.last_access = current_timestamp();

                return Some(entry);
            }
        }

        // Try prefix match
        let prefix_key = self.find_longest_prefix(tokens).await?;

        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(&prefix_key).cloned() {
            drop(entries);
            self.touch(&prefix_key).await;

            let mut stats = self.stats.write().await;
            stats.cache_hits += 1;
            stats.prefix_reuses += 1;
            stats.tokens_saved += entry.token_count;

            let mut entry = entry;
            entry.hit_count += 1;
            entry.last_access = current_timestamp();

            Some(entry)
        } else {
            let mut stats = self.stats.write().await;
            stats.cache_misses += 1;
            None
        }
    }

    /// Store KV cache for prefix.
    pub async fn store(&self, tokens: &[String], kv_cache: Vec<Vec<f32>>) {
        if tokens.len() > self.max_prefix_len {
            return;
        }

        let prefix_key = self.tokens_to_key(tokens);

        let entry = PrefixCacheEntry {
            prefix: tokens.to_vec(),
            kv_cache,
            token_count: tokens.len(),
            last_access: current_timestamp(),
            hit_count: 0,
        };

        let mut entries = self.entries.write().await;

        // Evict if necessary
        if entries.len() >= self.max_entries && !entries.contains_key(&prefix_key) {
            self.evict_lru(&mut entries).await;
        }

        entries.insert(prefix_key.clone(), entry);

        // Update LRU
        drop(entries);
        let mut lru = self.lru_order.write().await;
        lru.retain(|k| k != &prefix_key);
        lru.push_back(prefix_key);
    }

    /// Find longest matching prefix.
    async fn find_longest_prefix(&self, tokens: &[String]) -> Option<String> {
        let entries = self.entries.read().await;

        let mut longest = None;
        let mut longest_len = 0;

        for entry in entries.values() {
            if tokens.starts_with(&entry.prefix) && entry.prefix.len() > longest_len {
                longest_len = entry.prefix.len();
                longest = Some(self.tokens_to_key(&entry.prefix));
            }
        }

        longest
    }

    /// Convert tokens to cache key.
    fn tokens_to_key(&self, tokens: &[String]) -> String {
        // Use first N and last M tokens as key for efficiency
        let max_display = 5;
        let prefix: Vec<String> = tokens.iter().take(max_display).cloned().collect();
        let suffix: Vec<String> = tokens.iter().rev().take(max_display).cloned().collect();

        format!("{:?}|{:?}", prefix, suffix)
    }

    /// Update LRU on access.
    async fn touch(&self, key: &str) {
        let mut lru = self.lru_order.write().await;
        lru.retain(|k| k != key);
        lru.push_back(key.to_string());
    }

    /// Evict LRU entry.
    async fn evict_lru(&self, entries: &mut HashMap<String, PrefixCacheEntry>) {
        let mut lru = self.lru_order.write().await;
        if let Some(key) = lru.pop_front() {
            entries.remove(&key);
        }
    }

    /// Invalidate cache for a given prefix.
    pub async fn invalidate(&self, prefix: &[String]) {
        let key = self.tokens_to_key(prefix);
        let mut entries = self.entries.write().await;
        entries.remove(&key);
    }

    /// Clear all cache.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
        let mut lru = self.lru_order.write().await;
        lru.clear();
    }

    /// Get statistics.
    pub async fn stats(&self) -> PrefixCacheStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Get cache hit rate.
    pub async fn hit_rate(&self) -> f32 {
        let stats = self.stats.read().await;
        let total = stats.cache_hits + stats.cache_misses;
        if total == 0 {
            0.0
        } else {
            stats.cache_hits as f32 / total as f32
        }
    }
}

impl Default for PrefixCache {
    fn default() -> Self {
        Self::new(100, 1000)
    }
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: prefix_cache.rs:224")
        .as_secs()
}

/// KV Cache alignment utilities.
pub struct KvCacheAligner;

impl KvCacheAligner {
    /// Align cached KV state with new tokens.
    pub fn align(cached: &[Vec<f32>], new_tokens: &[String], cached_tokens: &[String]) -> (Vec<Vec<f32>>, Vec<String>) {
        let cached_len = cached_tokens.len();
        let new_len = new_tokens.len();

        if new_len <= cached_len {
            // All tokens are cached
            return (cached[..new_len].to_vec(), new_tokens.to_vec());
        }

        // Partial reuse - return cached + new portion
        let remaining = new_len - cached_len;
        let cached_part = cached.to_vec();
        let new_part: Vec<Vec<f32>> = (0..remaining)
            .map(|_| vec![0.0; 512]) // Placeholder for new KV
            .collect();

        let mut combined = cached_part;
        combined.extend(new_part);

        (combined, new_tokens.to_vec())
    }

    /// Check if prefix matches.
    pub fn prefix_matches(cached: &[String], new: &[String]) -> bool {
        if cached.len() > new.len() {
            return false;
        }

        cached.iter().zip(new.iter()).all(|(a, b)| a == b)
    }

    /// Compute prefix length.
    pub fn common_prefix_len(a: &[String], b: &[String]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_prefix_cache_basic() {
        let cache = PrefixCache::new(10, 100);

        let tokens = vec!["hello".to_string(), "world".to_string()];
        let kv = vec![vec![1.0, 2.0]];

        cache.store(&tokens, kv).await;

        let result = cache.get(&tokens).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().token_count, 2);
    }

    #[tokio::test]
    async fn test_prefix_cache_hit_rate() {
        let cache = PrefixCache::default();

        let tokens = vec!["test".to_string()];
        cache.store(&tokens, vec![vec![1.0]]).await;

        // Miss
        cache.get(&["other".to_string()]).await;
        // Hit
        cache.get(&tokens).await;

        let rate = cache.hit_rate().await;
        assert_eq!(rate, 0.5);
    }

    #[test]
    fn test_kv_aligner() {
        let cached = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let matches = KvCacheAligner::prefix_matches(&cached, &new);
        assert!(matches);

        let len = KvCacheAligner::common_prefix_len(&cached, &new);
        assert_eq!(len, 2);
    }
}
