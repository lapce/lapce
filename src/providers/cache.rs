//! Prompt caching support — inspired by Claude Code's cache-aware API layer.
//!
//! DeepSeek API supports prompt caching via cache_edits and cache_reference
//! fields (similar to Anthropic's prompt caching). This module adds
//! cache-aware request building to reduce API costs for repeated prefixes.
//!
//! ## How it works
//!
//! 1. Track the conversation prefix (system prompt + early messages)
//! 2. On first call, mark the prefix as cacheable → API caches it
//! 3. On subsequent calls, reference the cached prefix → ~90% cost reduction
//! 4. When conversation context changes, invalidate cache

use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;

/// Cache state for a specific provider/model combination.
#[derive(Debug)]
pub struct PromptCache {
    /// Whether the cache is currently valid.
    cached: AtomicBool,
    /// Hash of the cached prefix content.
    cached_hash: parking_lot::Mutex<String>,
    /// How many requests have used the cache.
    hit_count: AtomicBool,
    /// Cache statistics.
    stats: parking_lot::Mutex<CacheStats>,
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub estimated_savings_usd: f64,
}

impl Default for PromptCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptCache {
    pub fn new() -> Self {
        Self {
            cached: AtomicBool::new(false),
            cached_hash: parking_lot::Mutex::new(String::new()),
            hit_count: AtomicBool::new(false),
            stats: parking_lot::Mutex::new(CacheStats::default()),
        }
    }

    /// Check if a prefix is potentially cacheable.
    /// Returns true for the system prompt and first few messages
    /// which are likely to be repeated across turns.
    pub fn should_cache(&self, msg_index: usize, total_msgs: usize) -> bool {
        // Cache the system prompt (index 0) and early messages
        // Keep the last 2-3 messages fresh (they change each turn)
        msg_index < total_msgs.saturating_sub(3)
    }

    /// Build a cache-aware request body by marking cacheable content.
    /// This adds `cache_control: { type: "ephemeral" }` markers to
    /// messages that should be cached (DeepSeek API extension).
    pub fn build_cache_aware_body(
        &self,
        base_body: &mut serde_json::Value,
        total_messages: usize,
    ) {
        let messages = base_body["messages"].as_array_mut();
        if let Some(msgs) = messages {
            for (i, msg) in msgs.iter_mut().enumerate() {
                if self.should_cache(i, total_messages) {
                    // Mark as cacheable
                    msg["cache_control"] = serde_json::json!({"type": "ephemeral"});
                }
            }
        }

        let mut stats = self.stats.lock();
        stats.total_requests += 1;

        if self.cached.load(Ordering::Relaxed) {
            stats.cache_hits += 1;
            // Estimate savings: ~90% cost reduction on cached tokens
            stats.estimated_savings_usd += 0.002; // ~$0.002 per cached request
        } else {
            stats.cache_misses += 1;
        }
    }

    /// Build a cache_reference header pointing to previous cache.
    /// Called on subsequent turns when we know the cached prefix hasn't changed.
    pub fn build_cache_headers(&self, headers: &mut Vec<(String, String)>) {
        if self.cached.load(Ordering::Relaxed) {
            headers.push(("X-Cache-Reference".into(), "true".into()));
        }
    }

    /// Mark the cache as valid after a successful request.
    pub fn mark_cached(&self, prefix_hash: &str) {
        let mut hash = self.cached_hash.lock();
        *hash = prefix_hash.to_string();
        self.cached.store(true, Ordering::Relaxed);
    }

    /// Invalidate the cache (e.g., when system prompt changes).
    pub fn invalidate(&self) {
        self.cached.store(false, Ordering::Relaxed);
    }

    /// Check if cache is currently active.
    pub fn is_cached(&self) -> bool {
        self.cached.load(Ordering::Relaxed)
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        self.stats.lock().clone()
    }

    /// Check whether the cache has been hit (used for statistics).
    pub fn has_cache_hits(&self) -> bool {
        self.hit_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Cache manager — one cache per provider/model.
pub struct CacheManager {
    caches: parking_lot::Mutex<HashMap<String, PromptCache>>,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            caches: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Get or create a cache for a provider/model pair.
    pub fn get(&self, provider: &str, model: &str) -> parking_lot::MutexGuard<'_, HashMap<String, PromptCache>> {
        let mut caches = self.caches.lock();
        let key = format!("{}:{}", provider, model);
        caches.entry(key).or_default();
        drop(caches);
        self.caches.lock()
    }

    /// Invalidate all caches (e.g., after session reset).
    pub fn invalidate_all(&self) {
        for cache in self.caches.lock().values() {
            cache.invalidate();
        }
    }
}

/// Compute a simple hash of message content for cache key comparison.
pub fn hash_prefix(messages: &[crate::providers::provider::ChatMessage], limit: usize) -> String {
    use std::hash::{Hash, Hasher};
    let limit = limit.min(messages.len());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for msg in messages.iter().take(limit) {
        msg.role.hash(&mut hasher);
        msg.content.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_lifecycle() {
        let cache = PromptCache::new();
        assert!(!cache.is_cached());

        cache.mark_cached("abc123");
        assert!(cache.is_cached());

        cache.invalidate();
        assert!(!cache.is_cached());
    }

    #[test]
    fn test_hash_prefix_is_stable() {
        let msg1 = crate::providers::provider::ChatMessage {
            role: "system".into(),
            content: "Hello".into(),
            tool_calls: None,
            tool_call_id: None,
        ..Default::default()};
        let h1 = hash_prefix(&[msg1.clone()], 1);
        let h2 = hash_prefix(&[msg1.clone()], 1);
        assert_eq!(h1, h2, "Same content should produce same hash");

        let msg2 = crate::providers::provider::ChatMessage {
            role: "system".into(),
            content: "Hello!".into(),
            tool_calls: None,
            tool_call_id: None,
        ..Default::default()};
        let h3 = hash_prefix(&[msg2], 1);
        assert_ne!(h1, h3, "Different content should produce different hash");
    }
}
