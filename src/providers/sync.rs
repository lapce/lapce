//! Thread-safe generic data structures — inspired by Crush's `csync` package.
//!
//! Crush (Go) provides `csync.Value[T]`, `csync.Slice[T]`, `csync.Map[K,V]`,
//! and `csync.VersionedMap[K,V]` as composable thread-safe wrappers.
//!
//! These Rust equivalents use `Arc<RwLock<T>>` under the hood, providing
//! the same ergonomic API with zero unsafe code.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

// ============================================================================
// CsyncValue<T> — Thread-safe single value
// ============================================================================

/// A thread-safe, cloneable wrapper around a single value `T`.
///
/// Equivalent to Crush's `csync.Value[T]`.
///
/// ```no_run
/// use deepseek_carp::providers::sync::CsyncValue;
///
/// let counter = CsyncValue::new(0u64);
/// counter.set(42);
/// assert_eq!(counter.get(), 42);
/// ```
#[derive(Debug)]
pub struct CsyncValue<T: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<T>>,
}

impl<T: Clone + Send + Sync + 'static> CsyncValue<T> {
    pub fn new(val: T) -> Self {
        Self { inner: Arc::new(RwLock::new(val)) }
    }

    pub async fn get(&self) -> T {
        self.inner.read().await.clone()
    }

    pub async fn set(&self, val: T) {
        *self.inner.write().await = val;
    }

    pub async fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut guard = self.inner.write().await;
        f(&mut *guard);
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for CsyncValue<T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

// ============================================================================
// CsyncSlice<T> — Thread-safe growable slice
// ============================================================================

/// A thread-safe, cloneable wrapper around a `Vec<T>`.
///
/// Equivalent to Crush's `csync.Slice[T]`. Used for hot-updatable lists
/// like tool registrations, MCP endpoints, or provider overrides.
#[derive(Debug)]
pub struct CsyncSlice<T: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<Vec<T>>>,
}

impl<T: Clone + Send + Sync + 'static> CsyncSlice<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self { inner: Arc::new(RwLock::new(items)) }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub async fn get_all(&self) -> Vec<T> {
        self.inner.read().await.clone()
    }

    pub async fn push(&self, item: T) {
        self.inner.write().await.push(item);
    }

    pub async fn extend(&self, items: impl IntoIterator<Item = T>) {
        self.inner.write().await.extend(items);
    }

    pub async fn replace_all(&self, items: Vec<T>) {
        *self.inner.write().await = items;
    }

    pub async fn retain<F>(&self, f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.inner.write().await.retain(f);
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for CsyncSlice<T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

// ============================================================================
// CsyncMap<K, V> — Thread-safe HashMap
// ============================================================================

/// A thread-safe, cloneable wrapper around `HashMap<K, V>`.
///
/// Equivalent to Crush's `csync.Map[K,V]`. Used for active requests,
/// message queues, or any dynamic key-value store shared across tasks.
#[derive(Debug)]
pub struct CsyncMap<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<HashMap<K, V>>>,
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> CsyncMap<K, V> {
    pub fn new(map: HashMap<K, V>) -> Self {
        Self { inner: Arc::new(RwLock::new(map)) }
    }

    pub fn empty() -> Self {
        Self::new(HashMap::new())
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: K, val: V) -> Option<V> {
        self.inner.write().await.insert(key, val)
    }

    pub async fn remove(&self, key: &K) -> Option<V> {
        self.inner.write().await.remove(key)
    }

    pub async fn contains_key(&self, key: &K) -> bool {
        self.inner.read().await.contains_key(key)
    }

    pub async fn keys(&self) -> Vec<K> {
        self.inner.read().await.keys().cloned().collect()
    }

    pub async fn values(&self) -> Vec<V> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> Clone for CsyncMap<K, V> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

// ============================================================================
// CsyncVersionedMap<K, V> — Thread-safe map with version tracking
// ============================================================================

/// A thread-safe HashMap with an auto-incrementing version counter.
/// Every mutation bumps the version, allowing consumers to detect changes
/// without polling the full map contents.
///
/// Equivalent to Crush's `csync.VersionedMap[K,V]`.
/// Used for configuration hot-reload: UI polls `version()` to know when
/// provider settings have changed.
#[derive(Debug)]
pub struct CsyncVersionedMap<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<HashMap<K, V>>>,
    version: Arc<AtomicU64>,
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> CsyncVersionedMap<K, V> {
    pub fn new(map: HashMap<K, V>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(map)),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn empty() -> Self {
        Self::new(HashMap::new())
    }

    /// Get the current version number. Changes on every mutation.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: K, val: V) -> Option<V> {
        let old = self.inner.write().await.insert(key, val);
        self.version.fetch_add(1, Ordering::Release);
        old
    }

    pub async fn remove(&self, key: &K) -> Option<V> {
        let old = self.inner.write().await.remove(key);
        if old.is_some() {
            self.version.fetch_add(1, Ordering::Release);
        }
        old
    }

    pub async fn get_all(&self) -> HashMap<K, V> {
        self.inner.read().await.clone()
    }

    /// Get snapshot: (data, version). Caller compares version to detect staleness.
    pub async fn snapshot(&self) -> (HashMap<K, V>, u64) {
        let data = self.inner.read().await.clone();
        let ver = self.version.load(Ordering::Acquire);
        (data, ver)
    }
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static, V: Clone + Send + Sync + 'static> Clone for CsyncVersionedMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            version: Arc::clone(&self.version),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_csync_value_basic() {
        let v = CsyncValue::new(42i32);
        assert_eq!(v.get().await, 42);
        v.set(99).await;
        assert_eq!(v.get().await, 99);
    }

    #[tokio::test]
    async fn test_csync_value_clone_shares_state() {
        let a = CsyncValue::new(1);
        let b = a.clone();
        b.set(2).await;
        assert_eq!(a.get().await, 2);
    }

    #[tokio::test]
    async fn test_csync_slice_push_and_retain() {
        let s = CsyncSlice::new(vec![1, 2, 3, 4, 5]);
        s.push(6).await;
        s.retain(|x| x % 2 == 0).await;
        assert_eq!(s.get_all().await, vec![2, 4, 6]);
    }

    #[tokio::test]
    async fn test_csync_map_insert_remove() {
        let m = CsyncMap::<String, i32>::empty();
        m.insert("key".into(), 42).await;
        assert_eq!(m.get(&"key".into()).await, Some(42));
        m.remove(&"key".into()).await;
        assert!(m.get(&"key".into()).await.is_none());
    }

    #[tokio::test]
    async fn test_versioned_map_version_bumps() {
        let m = CsyncVersionedMap::<String, String>::empty();
        let v0 = m.version();
        m.insert("a".into(), "1".into()).await;
        let v1 = m.version();
        assert!(v1 > v0);
        m.insert("b".into(), "2".into()).await;
        assert!(m.version() > v1);
    }

    #[tokio::test]
    async fn test_versioned_map_snapshot() {
        let m: CsyncVersionedMap<String, i32> = CsyncVersionedMap::empty();
        m.insert("x".into(), 10).await;
        let (data, ver1) = m.snapshot().await;
        assert_eq!(data.get("x"), Some(&10));

        m.insert("y".into(), 20).await;
        let (_data, ver2) = m.snapshot().await;
        assert!(ver2 > ver1, "version should increment on mutation");
    }
}
