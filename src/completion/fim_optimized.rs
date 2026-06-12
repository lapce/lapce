use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::fim::{FimBackend, FimEngine, FimRequest as FimEngineRequest, FimResult as FimEngineResult};

pub struct LruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    capacity: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::with_capacity(capacity.max(1)),
            order: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    pub fn default_capacity() -> usize {
        128
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn put(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.move_to_back(&key);
            return;
        }

        while self.map.len() >= self.capacity {
            if let Some(evict_key) = self.order.pop_front() {
                self.map.remove(&evict_key);
            } else {
                break;
            }
        }

        self.map.insert(key.clone(), value);
        self.order.push_back(key);
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.move_to_back(key);
        self.map.get(key)
    }

    fn move_to_back(&mut self, key: &K) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
            self.order.push_back(key.clone());
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

impl<K, V> Default for LruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::new(128)
    }
}

pub struct FimDebouncerEntry {
    request: FimEngineRequest,
    enqueued_at: Instant,
}

pub struct FimDebouncer {
    window_ms: u64,
    burst_threshold: usize,
    queue: Arc<Mutex<Vec<FimDebouncerEntry>>>,
}

impl FimDebouncer {
    pub fn new() -> Self {
        Self {
            window_ms: 15,
            burst_threshold: 2,
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_window(mut self, ms: u64) -> Self {
        self.window_ms = ms;
        self
    }

    pub fn with_burst_threshold(mut self, threshold: usize) -> Self {
        self.burst_threshold = threshold;
        self
    }

    pub async fn enqueue(&self, request: FimEngineRequest) {
        let mut q = self.queue.lock().await;
        q.push(FimDebouncerEntry {
            request,
            enqueued_at: Instant::now(),
        });
    }

    pub async fn drain(&self) -> Vec<FimEngineRequest> {
        let mut q = self.queue.lock().await;
        let reqs: Vec<FimEngineRequest> = q.drain(..).map(|e| e.request).collect();
        reqs
    }

    pub async fn flush(&self) -> Vec<FimEngineRequest> {
        let mut q = self.queue.lock().await;
        let reqs: Vec<FimEngineRequest> = q.drain(..).map(|e| e.request).collect();
        reqs
    }

    pub async fn buffered_count(&self) -> usize {
        self.queue.lock().await.len()
    }

    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    pub fn burst_threshold(&self) -> usize {
        self.burst_threshold
    }
}

impl Default for FimDebouncer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CompletionPrefetcher {
    engine: FimEngine,
    recent_positions: Arc<Mutex<Vec<(usize, String, String)>>>,
    active_fetches: Arc<Mutex<usize>>,
}

impl CompletionPrefetcher {
    pub fn new(engine: FimEngine) -> Self {
        Self {
            engine,
            recent_positions: Arc::new(Mutex::new(Vec::with_capacity(8))),
            active_fetches: Arc::new(Mutex::new(0)),
        }
    }

    pub fn with_engine(mut self, engine: FimEngine) -> Self {
        self.engine = engine;
        self
    }

    pub async fn record_edit(&self, line: usize, prefix: &str, suffix: &str) {
        let mut recent = self.recent_positions.lock().await;
        let entry = (line, prefix.to_string(), suffix.to_string());
        recent.retain(|r| r.0 != line);
        recent.push(entry);
        if recent.len() > 3 {
            recent.remove(0);
        }
    }

    pub async fn candidates(&self) -> Vec<(FimEngineRequest, FimEngine)> {
        let recent = self.recent_positions.lock().await;
        if recent.is_empty() {
            return Vec::new();
        }
        let engine = self.engine.clone();
        let mut out = Vec::new();
        for (_, prefix, suffix) in recent.iter().take(3) {
            let mut next_line_prefix = prefix.clone();
            if !next_line_prefix.ends_with('\n') {
                next_line_prefix.push('\n');
            }
            let req = FimEngineRequest::new(&next_line_prefix, suffix);
            out.push((req, engine.clone()));
        }
        out
    }

    pub async fn trigger(&self) {
        let candidates = self.candidates().await;
        if candidates.is_empty() {
            return;
        }
        let engine = self.engine.clone();
        let pos_arc = self.recent_positions.clone();
        let active_arc = self.active_fetches.clone();

        tokio::spawn(async move {
            let recent = pos_arc.lock().await;
            let mut targets: Vec<FimEngineRequest> = Vec::new();
            for (_, prefix, suffix) in recent.iter().take(3) {
                let mut next_line_prefix = prefix.clone();
                if !next_line_prefix.ends_with('\n') {
                    next_line_prefix.push('\n');
                }
                targets.push(FimEngineRequest::new(&next_line_prefix, suffix));
            }
            drop(recent);

            let mut active = active_arc.lock().await;
            let count = *active;
            *active = count + targets.len();
            drop(active);

            for req in targets {
                let _ = engine.complete(&req).await;
            }

            let mut active = active_arc.lock().await;
            let c = *active;
            *active = c.saturating_sub(3);
        });
    }

    pub async fn active_fetches(&self) -> usize {
        *self.active_fetches.lock().await
    }
}

fn fim_request_key(req: &FimEngineRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    req.prefix.hash(&mut hasher);
    req.suffix.hash(&mut hasher);
    req.max_tokens.hash(&mut hasher);
    hasher.finish()
}

struct CacheState {
    cache: LruCache<u64, FimEngineResult>,
}

pub struct OptimizedFimEngine {
    engine: FimEngine,
    cache: Arc<Mutex<CacheState>>,
    debouncer: FimDebouncer,
    prefetcher: CompletionPrefetcher,
}

impl OptimizedFimEngine {
    pub fn new(backend: FimBackend) -> Self {
        let engine = FimEngine::new(backend);
        Self::from_engine(engine)
    }

    pub fn from_engine(engine: FimEngine) -> Self {
        Self {
            prefetcher: CompletionPrefetcher::new(engine.clone()),
            engine,
            cache: Arc::new(Mutex::new(CacheState { cache: LruCache::new(128) })),
            debouncer: FimDebouncer::new(),
        }
    }

    pub fn engine(&self) -> &FimEngine {
        &self.engine
    }

    pub async fn complete(&self, request: &FimEngineRequest) -> anyhow::Result<FimEngineResult> {
        let key = fim_request_key(request);

        {
            let mut cs = self.cache.lock().await;
            if let Some(res) = cs.cache.get(&key) {
                return Ok(res.clone());
            }
        }

        self.debouncer.enqueue(request.clone()).await;

        tokio::time::sleep(Duration::from_millis(self.debouncer.window_ms())).await;

        let buffered = self.debouncer.drain().await;
        let to_send: Vec<FimEngineRequest> = if buffered.len() > self.debouncer.burst_threshold() {
            buffered
        } else {
            vec![request.clone()]
        };

        let mut best: Option<FimEngineResult> = None;
        let mut last_err: Option<anyhow::Error> = None;

        for req in to_send.into_iter() {
            match self.engine.complete(&req).await {
                Ok(res) => {
                    let k = fim_request_key(&req);
                    {
                        let mut cs = self.cache.lock().await;
                        cs.cache.put(k, res.clone());
                    }
                    if best.is_none() {
                        best = Some(res);
                    }
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        let line = request.prefix.lines().count();
        self.prefetcher.record_edit(line, &request.prefix, &request.suffix).await;
        self.prefetcher.trigger().await;

        if let Some(res) = best {
            Ok(res)
        } else if let Some(e) = last_err {
            Err(e)
        } else {
            anyhow::bail!("FIM completion returned no result")
        }
    }

    pub async fn invalidate_cache(&self) {
        let mut cs = self.cache.lock().await;
        cs.cache.clear();
    }

    pub async fn cache_len(&self) -> usize {
        let cs = self.cache.lock().await;
        cs.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_hit_miss() {
        let mut cache: LruCache<String, i32> = LruCache::new(4);
        cache.put("a".into(), 1);
        cache.put("b".into(), 2);

        assert_eq!(*cache.get(&"a".into()).unwrap(), 1);
        assert_eq!(*cache.get(&"b".into()).unwrap(), 2);
        assert!(cache.get(&"c".into()).is_none());

        for i in 0..10 {
            cache.put(format!("k{}", i), i);
        }
        assert!(cache.len() <= cache.capacity());
    }

    #[tokio::test]
    async fn test_debouncer_batches_three_into_one() {
        let db = FimDebouncer::new().with_window(50);
        let r1 = FimEngineRequest::new("p1", "s1");
        let r2 = FimEngineRequest::new("p2", "s2");
        let r3 = FimEngineRequest::new("p3", "s3");

        db.enqueue(r1.clone()).await;
        db.enqueue(r2.clone()).await;
        db.enqueue(r3.clone()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(db.buffered_count().await, 3);

        tokio::time::sleep(Duration::from_millis(60)).await;
        let drained = db.flush().await;
        assert_eq!(drained.len(), 3);
    }

    #[tokio::test]
    async fn test_prefetcher_fires_background_task() {
        let engine = FimEngine::new(FimBackend::Local);
        let prefetcher = CompletionPrefetcher::new(engine.clone());

        prefetcher.record_edit(10, "fn test() {", "    return 1;\n}").await;
        prefetcher.record_edit(12, "let x = ", "").await;

        let candidates = prefetcher.candidates().await;
        assert!(candidates.len() >= 1, "should have at least 1 candidate");
        assert!(candidates.len() <= 3, "should have at most 3 candidates");

        prefetcher.trigger().await;

        tokio::time::sleep(Duration::from_millis(5)).await;
        let _active = prefetcher.active_fetches().await;
    }
}
