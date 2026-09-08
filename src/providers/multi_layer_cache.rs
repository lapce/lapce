//! Multi-layer Cache System V2 - Enhanced Performance Optimization
//!
//! Based on Claude Code's caching strategy, this module provides:
//! - Multi-tier caching (L1/L2/L3)
//! - Smart cache invalidation
//! - Compression support
//! - Cache warming
//! - Metrics and monitoring

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    pub value: T,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub access_count: usize,
    pub size_bytes: usize,
    pub tags: HashSet<String>,
}

impl<T> CacheEntry<T> {
    pub fn new(value: T, size_bytes: usize) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            access_count: 1,
            size_bytes,
            tags: HashSet::new(),
        }
    }

    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }

    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    pub fn idle_time(&self) -> Duration {
        self.last_accessed.elapsed()
    }

    pub fn hit_rate_score(&self) -> f64 {
        let age = self.age().as_secs_f64().max(1.0);
        self.access_count as f64 / age
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub insertions: usize,
    pub current_size_bytes: usize,
    pub current_items: usize,
    pub total_accesses: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_items: usize,
    pub max_size_bytes: usize,
    pub ttl: Duration,
    pub idle_ttl: Duration,
    pub compression_enabled: bool,
    pub compression_threshold: usize,
    pub eviction_policy: EvictionPolicy,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_items: 1000,
            max_size_bytes: 100 * 1024 * 1024, // 100MB
            ttl: Duration::from_secs(3600),     // 1 hour
            idle_ttl: Duration::from_secs(300), // 5 minutes
            compression_enabled: true,
            compression_threshold: 1024, // Compress if > 1KB
            eviction_policy: EvictionPolicy::LRU,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EvictionPolicy {
    LRU,      // Least Recently Used
    LFU,      // Least Frequently Used
    FIFO,     // First In First Out
    TTL,      // Time To Live based
    Size,     // Size based
    Adaptive, // Adaptive policy
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        EvictionPolicy::LRU
    }
}

/// Multi-layer cache system
pub struct MultiLayerCache<K, V> {
    layers: Vec<CacheLayer<K, V>>,
    stats: Arc<RwLock<CacheStats>>,
    config: CacheConfig,
}

struct CacheLayer<K, V> {
    name: String,
    cache: HashMap<K, CacheEntry<V>>,
    access_order: VecDeque<K>,
    max_items: usize,
    max_size_bytes: usize,
}

impl<K: Clone + Hash + Eq, V: Clone> CacheLayer<K, V> {
    fn new(name: &str, max_items: usize, max_size_bytes: usize) -> Self {
        Self {
            name: name.to_string(),
            cache: HashMap::new(),
            access_order: VecDeque::new(),
            max_items,
            max_size_bytes,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        if let Some(entry) = self.cache.get_mut(key) {
            entry.touch();
            // Update access order
            self.access_order.retain(|k| k != key);
            self.access_order.push_back(key.clone());
            Some(entry.value.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V, size_bytes: usize) -> Option<K> {
        let evicted_key = if self.cache.len() >= self.max_items {
            self.evict_one()
        } else {
            None
        };

        let entry = CacheEntry::new(value, size_bytes);
        self.cache.insert(key.clone(), entry);
        self.access_order.push_back(key);
        evicted_key
    }

    fn evict_one(&mut self) -> Option<K> {
        let key = self.access_order.pop_front()?;
        self.cache.remove(&key);
        Some(key)
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.access_order.retain(|k| k != key);
        self.cache.remove(key).map(|e| e.value)
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    fn total_size(&self) -> usize {
        self.cache.values().map(|e| e.size_bytes).sum()
    }

    fn evict_by_policy(&mut self, policy: EvictionPolicy) -> Option<K> {
        if self.cache.is_empty() {
            return None;
        }

        let key_to_evict = match policy {
            EvictionPolicy::LRU => self.access_order.front().cloned(),
            EvictionPolicy::LFU => {
                self.cache.iter()
                    .min_by_key(|(_, e)| e.access_count)
                    .map(|(k, _)| k.clone())
            }
            EvictionPolicy::FIFO => self.access_order.front().cloned(),
            EvictionPolicy::TTL => {
                self.cache.iter()
                    .filter(|(_, e)| e.age() > Duration::from_secs(3600))
                    .next()
                    .map(|(k, _)| k.clone())
            }
            EvictionPolicy::Size => {
                self.cache.iter()
                    .max_by_key(|(_, e)| e.size_bytes)
                    .map(|(k, _)| k.clone())
            }
            EvictionPolicy::Adaptive => {
                // Combine LRU and LFU
                let lru_key = self.access_order.front().cloned();
                let lfu_key = self.cache.iter()
                    .min_by_key(|(_, e)| e.access_count)
                    .map(|(k, _)| k.clone());
                
                // Pick the one with lower score
                match (lru_key.clone(), lfu_key.clone()) {
                    (Some(lru), Some(lfu)) => {
                        let lru_entry = self.cache.get(&lru).expect("unwrap failed: multi_layer_cache.rs:241");
                        let lfu_entry = self.cache.get(&lfu).expect("unwrap failed: multi_layer_cache.rs:242");
                        if lru_entry.hit_rate_score() < lfu_entry.hit_rate_score() {
                            Some(lru)
                        } else {
                            Some(lfu)
                        }
                    }
                    (Some(k), None) | (None, Some(k)) => Some(k),
                    (None, None) => None,
                }
            }
        };

        if let Some(key) = key_to_evict {
            self.cache.remove(&key);
            self.access_order.retain(|k| k != &key);
        }

        key_to_evict
    }
}

impl<K: Clone + Hash + Eq + std::fmt::Debug, V: Clone> MultiLayerCache<K, V> {
    pub fn new(config: CacheConfig) -> Self {
        // L1: Hot cache - small, fast
        let l1_config = CacheConfig {
            max_items: 100,
            max_size_bytes: 10 * 1024 * 1024, // 10MB
            ..config.clone()
        };

        // L2: Warm cache - medium
        let l2_config = CacheConfig {
            max_items: 500,
            max_size_bytes: 50 * 1024 * 1024, // 50MB
            ..config.clone()
        };

        // L3: Cold cache - large, slower
        let l3_config = CacheConfig {
            max_items: 5000,
            max_size_bytes: 500 * 1024 * 1024, // 500MB
            ..config.clone()
        };

        Self {
            layers: vec![
                CacheLayer::new("L1-Hot", l1_config.max_items, l1_config.max_size_bytes),
                CacheLayer::new("L2-Warm", l2_config.max_items, l2_config.max_size_bytes),
                CacheLayer::new("L3-Cold", l3_config.max_items, l3_config.max_size_bytes),
            ],
            stats: Arc::new(RwLock::new(CacheStats::default())),
            config,
        }
    }

    /// Get from cache (checks all layers)
    pub fn get(&self, key: &K) -> Option<V> {
        let mut stats = self.stats.write().expect("unwrap failed: multi_layer_cache.rs:300");
        
        // Check L1 first
        if let Some(value) = self.layers[0].cache.get(key) {
            stats.hits += 1;
            stats.total_accesses += 1;
            return Some(value.value.clone());
        }

        // Check L2
        {
            let mut l2 = &mut self.layers[1] as *mut CacheLayer<K, V>;
            unsafe {
                if let Some(value) = (*l2).cache.get(key) {
                    stats.hits += 1;
                    stats.total_accesses += 1;
                    // Promote to L1
                    let promoted = value.value.clone();
                    let size = value.size_bytes;
                    drop(value);
                    if let Some(evicted) = (*l2).evict_by_policy(self.config.eviction_policy) {
                        (*l2).cache.remove(&evicted);
                    }
                    self.layers[0].cache.insert(key.clone(), CacheEntry::new(promoted.clone(), size));
                    return Some(promoted);
                }
            }
        }

        // Check L3
        {
            let mut l3 = &mut self.layers[2] as *mut CacheLayer<K, V>;
            unsafe {
                if let Some(value) = (*l3).cache.get(key) {
                    stats.hits += 1;
                    stats.total_accesses += 1;
                    // Promote to L2, then L1
                    let promoted = value.value.clone();
                    let size = value.size_bytes;
                    (*l3).evict_by_policy(self.config.eviction_policy);
                    if let Some(evicted) = (*l3).evict_by_policy(self.config.eviction_policy) {
                        (*l3).cache.remove(&evicted);
                    }
                    self.layers[1].cache.insert(key.clone(), CacheEntry::new(promoted.clone(), size));
                    return Some(promoted);
                }
            }
        }

        // Cache miss
        stats.misses += 1;
        stats.total_accesses += 1;
        None
    }

    /// Insert into cache
    pub fn insert(&self, key: K, value: V, size_bytes: usize) {
        let mut stats = self.stats.write().expect("unwrap failed: multi_layer_cache.rs:357");
        
        // Insert into L1
        if let Some(evicted) = self.layers[0].insert(key.clone(), value, size_bytes) {
            // Demote evicted item to L2
            if let Some(entry) = self.layers[0].cache.remove(&evicted) {
                self.layers[1].cache.insert(evicted, entry);
            }
            stats.evictions += 1;
        }
        
        stats.insertions += 1;
        drop(stats);

        // Check and enforce size limits
        self.enforce_size_limits();
    }

    /// Remove from cache
    pub fn remove(&self, key: &K) {
        for layer in &self.layers {
            layer.remove(key);
        }
    }

    /// Clear all caches
    pub fn clear(&self) {
        for layer in &self.layers {
            layer.clear();
        }
        self.stats.write().expect("unwrap failed: multi_layer_cache.rs:387").reset();
    }

    /// Get statistics
    pub fn stats(&self) -> CacheStats {
        self.stats.read().cloned().unwrap_or_default()
    }

    /// Warm up cache with keys
    pub fn warm_up<F>(&self, keys: Vec<K>, loader: F) 
    where 
        F: Fn(&K) -> Option<(V, usize)>
    {
        for key in keys {
            if self.get(&key).is_none() {
                if let Some((value, size)) = loader(&key) {
                    self.insert(key, value, size);
                }
            }
        }
    }

    /// Enforce size limits across layers
    fn enforce_size_limits(&self) {
        for layer in &self.layers {
            while layer.total_size() > layer.max_size_bytes || layer.len() > layer.max_items {
                if layer.evict_by_policy(self.config.eviction_policy).is_none() {
                    break;
                }
            }
        }
    }

    /// Clean up expired entries
    pub fn cleanup_expired(&self) {
        for layer in &self.layers {
            let now = Instant::now();
            let keys_to_remove: Vec<_> = layer.cache.keys()
                .filter(|k| {
                    if let Some(entry) = layer.cache.get(*k) {
                        entry.age() > self.config.ttl || entry.idle_time() > self.config.idle_ttl
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();

            for key in keys_to_remove {
                layer.remove(&key);
            }
        }
    }

    /// Get cache info
    pub fn info(&self) -> CacheInfo {
        let stats = self.stats.read().expect("unwrap failed: multi_layer_cache.rs:443");
        CacheInfo {
            layers: self.layers.iter().enumerate().map(|(i, l)| LayerInfo {
                name: l.name.clone(),
                items: l.len(),
                size_bytes: l.total_size(),
                max_items: l.max_items,
                max_size_bytes: l.max_size_bytes,
            }).collect(),
            stats: stats.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheInfo {
    pub layers: Vec<LayerInfo>,
    pub stats: CacheStats,
}

#[derive(Debug, Clone)]
pub struct LayerInfo {
    pub name: String,
    pub items: usize,
    pub size_bytes: usize,
    pub max_items: usize,
    pub max_size_bytes: usize,
}

impl LayerInfo {
    pub fn usage_percent(&self) -> f64 {
        if self.max_items == 0 {
            0.0
        } else {
            (self.items as f64 / self.max_items as f64) * 100.0
        }
    }

    pub fn size_percent(&self) -> f64 {
        if self.max_size_bytes == 0 {
            0.0
        } else {
            (self.size_bytes as f64 / self.max_size_bytes as f64) * 100.0
        }
    }
}

/// Cache key utilities
pub trait CacheKey {
    fn to_cache_key(&self) -> String;
}

impl CacheKey for String {
    fn to_cache_key(&self) -> String {
        self.clone()
    }
}

impl<'a> CacheKey for &'a str {
    fn to_cache_key(&self) -> String {
        self.to_string()
    }
}

impl<K: Hash> CacheKey for K {
    fn to_cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}
