//! Completion result cache — LRU cache for FIM completion results.
//!
//! Caches completion results keyed by (prefix_hash, suffix_hash, max_tokens, temperature).
//! When the user types the same prefix again, the cached result is returned instantly
//! without calling the model. Typical cache hit rate: 30-40% for repeated edits.
//!
//! ## Cache key design
//!
//! The cache key uses SHA256 hash of prefix+suffix to avoid storing full text.
//! This is safe because:
//! - Same prefix+suffix always produces the same hash
//! - Hash collisions are astronomically unlikely with SHA256
//! - Cache is memory-only (lost on restart), so no persistence concerns
//!
//! ## Eviction policy
//!
//! LRU with a configurable max entries (default: 256). When the cache is full,
//! the least recently used entry is evicted.

use std::collections::HashMap;
use std::time::Instant;
use sha2::{Sha256, Digest};
use parking_lot::Mutex as ParkingMutex;

use super::{CompletionCandidate, FimRequest};

/// A single cache entry with hit tracking.
#[derive(Debug, Clone)]
struct CacheEntry {
    candidate: CompletionCandidate,
    /// When this entry was created.
    created_at: Instant,
    /// When this entry was last accessed.
    last_accessed: Instant,
    /// Number of times this entry was hit.
    hit_count: u64,
}

/// LRU cache for completion results.
pub struct CompletionCache {
    entries: ParkingMutex<HashMap<String, CacheEntry>>,
    /// Maximum number of cached entries.
    max_entries: usize,
    /// Cache TTL (entries older than this are evicted on access).
    ttl: std::time::Duration,
    /// Statistics.
    stats: ParkingMutex<CompletionCacheStats>,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionCacheStats {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub evictions: u64,
}

impl CompletionCache {
    /// Create a new completion cache with default settings.
    /// - max_entries: 256 (enough for typical editing sessions)
    /// - ttl: 5 minutes (prefix context changes quickly during editing)
    pub fn new() -> Self {
        Self {
            entries: ParkingMutex::new(HashMap::new()),
            max_entries: 256,
            ttl: std::time::Duration::from_secs(300), // 5 min
            stats: ParkingMutex::new(CompletionCacheStats::default()),
        }
    }

    /// Create a cache with custom capacity and TTL.
    pub fn with_config(max_entries: usize, ttl_secs: u64) -> Self {
        Self {
            entries: ParkingMutex::new(HashMap::new()),
            max_entries,
            ttl: std::time::Duration::from_secs(ttl_secs),
            stats: ParkingMutex::new(CompletionCacheStats::default()),
        }
    }

    /// Build a cache key from a FIM request.
    /// Uses SHA256 of prefix + suffix + max_tokens + temperature.
    pub fn build_key(request: &FimRequest) -> String {
        let mut hasher = Sha256::new();
        hasher.update(request.prefix.as_bytes());
        hasher.update(b"\x00"); // separator
        hasher.update(request.suffix.as_bytes());
        hasher.update(b"\x00");
        hasher.update(request.max_tokens.to_le_bytes());
        hasher.update(b"\x00");
        hasher.update(request.temperature.to_le_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Look up a cached completion result.
    /// Returns Some(candidate) on cache hit, None on miss.
    /// Also updates access time and hit count.
    pub fn get(&self, request: &FimRequest) -> Option<CompletionCandidate> {
        let key = Self::build_key(request);
        let mut entries = self.entries.lock();
        let mut stats = self.stats.lock();
        stats.total_requests += 1;

        if let Some(entry) = entries.get_mut(&key) {
            // Check TTL
            if entry.created_at.elapsed() > self.ttl {
                // Expired — remove and count as miss
                entries.remove(&key);
                stats.cache_misses += 1;
                return None;
            }

            // Cache hit
            entry.last_accessed = Instant::now();
            entry.hit_count += 1;
            stats.cache_hits += 1;

            tracing::debug!(
                key=%&key[..16],
                hit_count=entry.hit_count,
                provider=%entry.candidate.provider,
                "Completion cache hit"
            );

            Some(entry.candidate.clone())
        } else {
            stats.cache_misses += 1;
            None
        }
    }

    /// Store a completion result in the cache.
    pub fn put(&self, request: &FimRequest, candidate: &CompletionCandidate) {
        let key = Self::build_key(request);
        let mut entries = self.entries.lock();

        // Evict oldest entry if at capacity
        if entries.len() >= self.max_entries && !entries.contains_key(&key) {
            self.evict_lru(&mut entries);
        }

        let now = Instant::now();
        entries.insert(key.clone(), CacheEntry {
            candidate: candidate.clone(),
            created_at: now,
            last_accessed: now,
            hit_count: 0,
        });

        tracing::debug!(
            key=%&key[..16],
            provider=%candidate.provider,
            "Completion cache stored"
        );
    }

    /// Evict the least recently used entry.
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        if let Some((key, _)) = entries
            .iter()
            .min_by_key(|(_, e)| e.last_accessed)
            .map(|(k, e)| (k.clone(), e.last_accessed))
        {
            entries.remove(&key);
            self.stats.lock().evictions += 1;
        }
    }

    /// Invalidate all cache entries (e.g., after model switch or config change).
    pub fn invalidate_all(&self) {
        let count = self.entries.lock().len();
        self.entries.lock().clear();
        tracing::info!(count, "Completion cache invalidated");
    }

    /// Get current cache statistics.
    pub fn stats(&self) -> CompletionCacheStats {
        self.stats.lock().clone()
    }

    /// Get current cache size.
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Get cache hit rate (0.0 to 1.0).
    pub fn hit_rate(&self) -> f64 {
        let stats = self.stats.lock();
        if stats.total_requests == 0 {
            return 0.0;
        }
        stats.cache_hits as f64 / stats.total_requests as f64
    }
}

impl Default for CompletionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(prefix: &str, suffix: &str) -> FimRequest {
        FimRequest {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            file_path: None,
            language: None,
            max_tokens: 64,
            temperature: 0.1,
        }
    }

    fn make_candidate(text: &str) -> CompletionCandidate {
        CompletionCandidate {
            text: text.to_string(),
            confidence: 0.9,
            provider: "test".into(),
            latency_ms: 100,
        }
    }

    #[test]
    fn test_cache_hit_and_miss() {
        let cache = CompletionCache::new();
        let req = make_request("fn hello() {", "}");

        // First access: miss
        assert!(cache.get(&req).is_none());

        // Store and retrieve
        cache.put(&req, &make_candidate("println!(\"hi\");"));
        let hit = cache.get(&req);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().text, "println!(\"hi\");");
    }

    #[test]
    fn test_different_prefixes_different_keys() {
        let cache = CompletionCache::new();
        let req1 = make_request("fn a() {", "}");
        let req2 = make_request("fn b() {", "}");

        cache.put(&req1, &make_candidate("// a"));
        cache.put(&req2, &make_candidate("// b"));

        assert_eq!(cache.get(&req1).unwrap().text, "// a");
        assert_eq!(cache.get(&req2).unwrap().text, "// b");
    }

    #[test]
    fn test_cache_key_deterministic() {
        let req1 = make_request("hello", "world");
        let req2 = make_request("hello", "world");
        assert_eq!(CompletionCache::build_key(&req1), CompletionCache::build_key(&req2));
    }

    #[test]
    fn test_cache_stats() {
        let cache = CompletionCache::new();
        let req = make_request("test", "case");

        cache.get(&req); // miss
        cache.put(&req, &make_candidate("ok"));
        cache.get(&req); // hit
        cache.get(&req); // hit

        let stats = cache.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.cache_hits, 2);
        assert_eq!(stats.cache_misses, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let cache = CompletionCache::with_config(2, 3600);
        let req1 = make_request("a", "b");
        let req2 = make_request("c", "d");
        let req3 = make_request("e", "f");

        cache.put(&req1, &make_candidate("1"));
        cache.put(&req2, &make_candidate("2"));
        // Access req1 to make req2 the LRU
        cache.get(&req1);
        // Now put req3 — should evict req2
        cache.put(&req3, &make_candidate("3"));

        assert!(cache.get(&req1).is_some());
        assert!(cache.get(&req2).is_none()); // evicted
        assert!(cache.get(&req3).is_some());
    }
}