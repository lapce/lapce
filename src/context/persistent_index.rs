use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use notify::Watcher;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::RwLock;

use super::semantic_index_v2::{SemanticIndexV2, SymbolInfo};

#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn store(&self, key: &str, embedding: Vec<f32>) -> Result<()>;
    async fn query(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;
    async fn remove(&self, key: &str) -> Result<()>;
    async fn load_all(&self) -> Result<Vec<(String, Vec<f32>)>>;
}

#[cfg(feature = "sqlite-storage")]
pub struct SqliteVectorStore {
    conn: Arc<parking_lot::Mutex<rusqlite::Connection>>,
    dimension: usize,
}

#[cfg(feature = "sqlite-storage")]
impl SqliteVectorStore {
    pub fn new(db_path: &Path, dimension: usize) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(db_path)
            .context("open vector store sqlite db")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vectors (
                key TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );
             CREATE INDEX IF NOT EXISTS idx_vectors_updated ON vectors(updated_at);",
        )
        .context("create vectors table")?;
        Ok(Self {
            conn: Arc::new(parking_lot::Mutex::new(conn)),
            dimension,
        })
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum().sqrt();
        if norm_a < 1e-8 || norm_b < 1e-8 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }
}

#[cfg(feature = "sqlite-storage")]
#[async_trait::async_trait]
impl VectorStore for SqliteVectorStore {
    async fn store(&self, key: &str, embedding: Vec<f32>) -> Result<()> {
        let conn = self.conn.lock();
        let blob = bytemuck::cast_slice(&embedding).to_vec();
        conn.execute(
            "INSERT OR REPLACE INTO vectors (key, embedding, updated_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![key, blob, chrono::Utc::now().timestamp()],
        )
        .context("store vector")?;
        Ok(())
    }

    async fn query(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT key, embedding FROM vectors")
            .context("prepare query")?;
        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((key, blob))
            })
            .context("query vectors")?;
        let mut scored: Vec<(String, f32)> = rows
            .filter_map(|r| r.ok())
            .map(|(key, blob)| {
                let vec: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0; 4])))
                    .collect();
                let score = Self::cosine_similarity(embedding, &vec);
                (key, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM vectors WHERE key = ?1", [key])
            .context("remove vector")?;
        Ok(())
    }

    async fn load_all(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT key, embedding FROM vectors")
            .context("prepare load_all")?;
        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((key, blob))
            })
            .context("load_all vectors")?;
        let result: Vec<_> = rows
            .filter_map(|r| r.ok())
            .map(|(key, blob)| {
                let vec: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0; 4])))
                    .collect();
                (key, vec)
            })
            .collect();
        Ok(result)
    }
}

pub fn complexity_score(file_content: &str) -> f32 {
    let lines: Vec<&str> = file_content.lines().collect();
    if lines.is_empty() {
        return 0.0;
    }
    let line_count = lines.len();
    let max_lines = 5000.0;
    let line_score = ((line_count as f32) / max_lines).min(1.0);

    let import_patterns: &[&str] = &[
        "use ", "import ", "from ", "#include ", "require(", "extern ",
    ];
    let import_count = lines
        .iter()
        .filter(|l| import_patterns.iter().any(|p| l.contains(p)))
        .count();
    let import_score = ((import_count as f32) / 50.0).min(1.0);

    let mut max_depth = 0usize;
    let mut cyclomatic = 1usize;
    for line in &lines {
        let trimmed = line.trim();
        let open = trimmed.matches('{').count();
        let close = trimmed.matches('}').count();
        let mut depth = open.saturating_sub(close);
        depth = depth.min(max_depth + 1);
        if depth > max_depth {
            max_depth = depth;
        }
        let branch_keywords = ["if ", "else if", "match ", "for ", "while ", "&& ", "|| "];
        cyclomatic += branch_keywords.iter().filter(|kw| trimmed.contains(**kw)).count();
    }
    let depth_score = ((max_depth as f32) / 10.0).min(1.0);
    let cyclo_score = ((cyclomatic as f32) / 100.0).min(1.0);

    let weights = (line_score * 0.25) + (import_score * 0.15) + (depth_score * 0.30) + (cyclo_score * 0.30);
    weights.min(1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Create,
    Modify,
    Delete,
}

#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub kind: FileChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEntry {
    pub uri: String,
    pub file_hash: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub version: u32,
    pub entries: Vec<PersistedEntry>,
}

/// RAG 检索结果上下文，包含匹配文件的内容片段与相关性评分。
#[derive(Debug, Clone)]
pub struct RagContext {
    pub uri: String,
    pub content_snippet: String,
    pub score: f32,
    pub language: Option<String>,
}

impl Default for PersistedIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

const BATCH_SIZE: usize = 10;
const DEFAULT_DEBOUNCE_MS: u64 = 500;
const DEFAULT_FLUSH_EVERY_EVENTS: usize = 25;
const INDEX_STALE_SECS: u64 = 300;
const VECTOR_DIM: usize = 384;

fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        _ => "generic",
    }
}

fn code_semantic_hash(file_text: &str) -> String {
    let mut hasher = Sha256::new();
    let tokens: Vec<&str> = file_text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let stable = tokens.join("|");
    hasher.update(stable.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

fn index_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let root = std::env::var("DEEPCARP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".deepseek-carp"));
    let _ = std::fs::create_dir_all(&root);
    root.join("index.json")
}

fn vector_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let root = std::env::var("DEEPCARP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".deepseek-carp"));
    let _ = std::fs::create_dir_all(&root);
    root.join("vectors.db")
}

fn compute_text_embedding(text: &str, dim: usize) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut vec = Vec::with_capacity(dim);
    let bytes = text.as_bytes();
    for i in 0..dim {
        let mut hasher = DefaultHasher::new();
        (i as u64).hash(&mut hasher);
        bytes.hash(&mut hasher);
        let h = hasher.finish();
        let f = (h as f32) / (u64::MAX as f32) * 2.0 - 1.0;
        vec.push(f);
    }
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
    vec
}

fn should_skip(path: &Path) -> bool {
    let skip_segments: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|seg| skip_segments.contains(&seg))
}

fn is_text_file(path: &Path) -> bool {
    const EXT: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cpp", "h", "hpp",
        "cs", "rb", "kt", "swift", "php", "sh", "bash", "zsh", "ps1", "proto", "toml",
        "json", "yaml", "yml", "md", "html", "css", "sql", "rs", "vue", "svelte",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXT.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct PersistentSemanticIndex {
    inner: SemanticIndexV2,
    state: Arc<RwLock<PersistedIndex>>,
    path_hashes: Arc<RwLock<HashMap<String, String>>>,
    event_count: Arc<Mutex<usize>>,
    flush_every_events: usize,
    debounce_ms: u64,
    #[cfg(feature = "sqlite-storage")]
    vector_store: Option<Arc<SqliteVectorStore>>,
    last_index_update: Arc<Mutex<Instant>>,
}

impl PersistentSemanticIndex {
    pub async fn new(flush_every_events: usize, debounce_ms: u64) -> Self {
        let idx = Self {
            inner: SemanticIndexV2::new(super::semantic_index_v2::IndexConfig::default()),
            state: Arc::new(RwLock::new(PersistedIndex::default())),
            path_hashes: Arc::new(RwLock::new(HashMap::new())),
            event_count: Arc::new(Mutex::new(0)),
            flush_every_events,
            debounce_ms,
            #[cfg(feature = "sqlite-storage")]
            vector_store: None,
            last_index_update: Arc::new(Mutex::new(Instant::now())),
        };
        let _ = idx.load().await;
        idx
    }

    pub async fn with_vector_store(
        flush_every_events: usize,
        debounce_ms: u64,
        _db_path: &Path,
    ) -> Result<Self> {
        let idx = Self::new(flush_every_events, debounce_ms).await;
        #[cfg(feature = "sqlite-storage")]
        {
            let store = SqliteVectorStore::new(db_path, VECTOR_DIM)?;
            idx.restore_vectors_from_store(&store).await?;
            idx.vector_store = Some(Arc::new(store));
        }
        Ok(idx)
    }

    pub async fn default() -> Self {
        Self::new(DEFAULT_FLUSH_EVERY_EVENTS, DEFAULT_DEBOUNCE_MS).await
    }

    pub fn inner(&self) -> &SemanticIndexV2 {
        &self.inner
    }

    pub fn is_fresh(&self) -> bool {
        *self.last_index_update.lock() + Duration::from_secs(INDEX_STALE_SECS) > Instant::now()
    }

    #[cfg(feature = "sqlite-storage")]
    async fn restore_vectors_from_store(&self, store: &SqliteVectorStore) -> Result<()> {
        let all = store.load_all().await?;
        for (_key, _embedding) in all {
            let _ = self.inner.index_file(&_key, "", "restored").await;
        }
        *self.last_index_update.lock() = Instant::now();
        Ok(())
    }

    pub async fn persist(&self) -> Result<()> {
        let file = index_file_path();
        let data = self.state.read().await;
        let bytes = serde_json::to_vec_pretty(&*data).context("serialize index")?;
        fs::write(&file, bytes).await.context("write index file")?;
        Ok(())
    }

    pub async fn load(&self) -> Result<()> {
        let file = index_file_path();
        if !file.exists() {
            return Ok(());
        }
        let bytes = fs::read(&file).await.context("read index file")?;
        if bytes.is_empty() {
            return Ok(());
        }
        let loaded: PersistedIndex =
            serde_json::from_slice(&bytes).context("deserialize index file")?;
        let mut guard = self.state.write().await;
        *guard = loaded.clone();
        let mut hashes = self.path_hashes.write().await;
        for e in &loaded.entries {
            hashes.insert(e.uri.clone(), e.file_hash.clone());
        }
        Ok(())
    }

    pub async fn process_event(&self, event: FileChangeEvent) -> Result<()> {
        match event.kind {
            FileChangeKind::Create | FileChangeKind::Modify => {
                self.reparse_file(&event.path).await?;
            }
            FileChangeKind::Delete => {
                let uri = event.path.to_string_lossy().to_string();
                self.inner.clear_file(&uri).await;
                self.path_hashes.write().await.remove(&uri);
                #[cfg(feature = "sqlite-storage")]
                if let Some(ref vs) = self.vector_store {
                    let _ = vs.remove(&uri).await;
                }
                {
                    let mut s = self.state.write().await;
                    s.entries.retain(|e| e.uri != uri);
                }
            }
        }
        *self.last_index_update.lock() = Instant::now();
        let mut counter = self.event_count.lock();
        *counter += 1;
        let should_flush = *counter >= self.flush_every_events;
        if should_flush {
            *counter = 0;
            drop(counter);
            let _ = self.persist().await;
        } else {
            drop(counter);
        }
        Ok(())
    }

    pub async fn reparse_file(&self, path: &Path) -> Result<Vec<SymbolInfo>> {
        let uri = path.to_string_lossy().to_string();
        let content = fs::read_to_string(path).await.context("read file for reparse")?;
        let language = detect_language(path);
        let hash = code_semantic_hash(&content);

        let existing = self.path_hashes.read().await.get(&uri).cloned();
        if existing.as_deref() == Some(&hash) {
            let _ = self.inner.index_file(&uri, &content, language).await;
            return Ok(vec![]);
        }

        let symbols = self.inner.index_file(&uri, &content, language).await;
        self.path_hashes.write().await.insert(uri.clone(), hash.clone());

        #[cfg(feature = "sqlite-storage")]
        {
            if let Some(ref vs) = self.vector_store {
                let embedding = compute_text_embedding(&content, VECTOR_DIM);
                let _ = vs.store(&uri, embedding).await;
            }
        }

        {
            let mut s = self.state.write().await;
            s.entries.retain(|e| e.uri != uri);
            s.entries.push(PersistedEntry {
                uri: uri.clone(),
                file_hash: hash,
                language: Some(language.to_string()),
            });
        }
        *self.last_index_update.lock() = Instant::now();
        Ok(symbols)
    }

    /// 启动时检查索引是否过期（>5min），若过期则对整个工作区执行全量索引。
    /// 返回本次索引到的符号总数。
    pub async fn ensure_indexed(&self, project_dir: &Path) -> Result<usize> {
        if self.is_fresh() {
            return Ok(0);
        }
        let count = FileSystemWatcher::build_index(self, project_dir).await?;
        let _ = self.persist().await;
        Ok(count)
    }

    /// RAG 检索：对 query 计算嵌入向量，查询向量库（或回退到内存符号搜索），
    /// 返回 top_k 个匹配结果，每个包含文件 URI、前 200 字符内容片段、相关性评分和语言。
    pub async fn rag_retrieve(&self, query: &str, top_k: usize) -> Vec<RagContext> {
        let _embedding = compute_text_embedding(query, VECTOR_DIM);

        #[cfg(feature = "sqlite-storage")]
        {
            if let Some(ref vs) = self.vector_store {
                match vs.query(&embedding, top_k).await {
                    Ok(results) => {
                        let mut contexts = Vec::with_capacity(results.len());
                        for (uri, score) in results {
                            let snippet = fs::read_to_string(&uri)
                                .await
                                .map(|s| s.chars().take(200).collect())
                                .unwrap_or_default();
                            let lang = self
                                .state
                                .read()
                                .await
                                .entries
                                .iter()
                                .find(|e| e.uri == uri)
                                .and_then(|e| e.language.clone());
                            contexts.push(RagContext {
                                uri,
                                content_snippet: snippet,
                                score,
                                language: lang,
                            });
                        }
                        return contexts;
                    }
                    Err(_) => {}
                }
            }
        }

        // 回退：使用内存中的符号搜索
        let symbols = self.inner.search_symbols(query).await;
        let mut seen = HashSet::new();
        symbols
            .into_iter()
            .filter_map(|(info, score)| {
                if seen.contains(&info.uri) || seen.len() >= top_k {
                    None
                } else {
                    seen.insert(info.uri.clone());
                    Some(RagContext {
                        uri: info.uri,
                        content_snippet: info.name.chars().take(200).collect(),
                        score,
                        language: None,
                    })
                }
            })
            .collect()
    }

    /// 基于用户 prompt 估算任务复杂度（0.0–1.0）。
    /// < 0.6 表示可本地处理，>= 0.6 建议考虑云端。
    pub fn estimate_task_complexity(prompt: &str) -> f32 {
        let lower = prompt.to_lowercase();

        // 单行补全 / FIM → 极简，始终本地
        if lower.lines().count() <= 2 && !lower.contains("refactor") && !lower.contains("architecture") {
            return 0.1;
        }

        // 架构类问题 → 高复杂度
        let arch_keywords = [
            "architecture", "design pattern", "system design",
            "架构设计", "技术方案", "整体重构",
        ];
        if arch_keywords.iter().any(|kw| lower.contains(kw)) {
            return 0.9;
        }

        // 多文件重构 → 高复杂度
        let refactor_keywords = ["refactor", "重构", "migrate", "迁移"];
        if refactor_keywords.iter().any(|kw| lower.contains(kw)) {
            return 0.8;
        }

        // 简单 bug 修复 → 中低复杂度
        let bug_keywords = ["fix", "bug", "修复", "error", "错误", "issue"];
        if bug_keywords.iter().any(|kw| lower.contains(kw)) {
            return 0.4;
        }

        // 默认：用 complexity_score 对 prompt 文本本身打分
        complexity_score(prompt).max(0.3).min(0.7)
    }

    /// 关闭索引服务，将所有未持久化的数据刷盘。
    /// **必须在进程退出前调用**，否则可能丢失最近的事件变更。
    /// 推荐在 main 函数退出路径或 signal handler 中调用。
    pub async fn shutdown(&self) -> Result<()> {
        self.persist().await
    }
}

pub struct FileSystemWatcher;

impl FileSystemWatcher {
    pub async fn walk(project_dir: &Path) -> Result<Vec<PathBuf>> {
        let mut results = Vec::new();
        Self::walk_sync(project_dir, &mut results);
        Ok(results)
    }

    fn walk_sync(dir: &Path, out: &mut Vec<PathBuf>) {
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if should_skip(&path) {
                continue;
            }
            if path.is_dir() {
                Self::walk_sync(&path, out);
            } else if is_text_file(&path) {
                out.push(path);
            }
        }
    }

    pub async fn build_index(
        index: &PersistentSemanticIndex,
        project_dir: &Path,
    ) -> Result<usize> {
        let files = Self::walk(project_dir).await?;
        let mut iter = files.into_iter();
        let mut total = 0usize;
        loop {
            let batch: Vec<PathBuf> = iter.by_ref().take(BATCH_SIZE).collect();
            if batch.is_empty() {
                break;
            }
            let mut futs: FuturesUnordered<std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<super::semantic_index_v2::SymbolInfo>>>>>> = batch
                .into_iter()
                .map(|p| { let p_owned: PathBuf = p.clone(); Box::pin(async move { index.reparse_file(&p_owned).await }) as _ })
                .collect();
            while let Some(res) = futs.next().await {
                if let Ok(syms) = res { total += syms.len() }
            }
        }
        Ok(total)
    }
}

#[derive(Debug, Clone)]
pub struct DebouncedEvent {
    pub path: PathBuf,
    pub kind: FileChangeKind,
    pub deadline: Instant,
}

#[derive(Clone)]
pub struct Debouncer {
    inner: Arc<Mutex<HashMap<PathBuf, DebouncedEvent>>>,
    pub debounce_ms: u64,
}

impl Debouncer {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            debounce_ms,
        }
    }

    pub fn submit(&self, path: PathBuf, kind: FileChangeKind) {
        let mut g = self.inner.lock();
        let deadline = Instant::now() + Duration::from_millis(self.debounce_ms);
        g.insert(path.clone(), DebouncedEvent {
            path: path.clone(),
            kind,
            deadline,
        });
    }

    pub fn drain_due(&self, now: Instant) -> Vec<DebouncedEvent> {
        let mut g = self.inner.lock();
        let mut due = Vec::new();
        let mut keep = HashMap::new();
        for (p, ev) in g.drain() {
            if ev.deadline <= now {
                due.push(ev);
            } else {
                keep.insert(p, ev);
            }
        }
        *g = keep;
        due
    }

    pub fn pending_count(&self) -> usize {
        self.inner.lock().len()
    }
}

pub struct IndexWatcher {
    pub watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    pub index: Arc<PersistentSemanticIndex>,
    pub debouncer: Debouncer,
    pub watched_roots: Arc<Mutex<HashSet<PathBuf>>>,
}

impl IndexWatcher {
    pub fn new(index: Arc<PersistentSemanticIndex>, debounce_ms: u64) -> Self {
        Self {
            watcher: Arc::new(Mutex::new(None)),
            index,
            debouncer: Debouncer::new(debounce_ms),
            watched_roots: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn watch(&self, root: &Path) -> Result<()> {
        let mut guard = self.watcher.lock();
        let debouncer = self.debouncer.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(ev) = res {
                if ev.kind.is_access() {
                    return;
                }
                for path in &ev.paths {
                    if should_skip(path) || !is_text_file(path) && path.is_file() {
                        continue;
                    }
                    let kind = if ev.kind.is_remove() {
                        FileChangeKind::Delete
                    } else if ev.kind.is_create() {
                        FileChangeKind::Create
                    } else {
                        FileChangeKind::Modify
                    };
                    debouncer.submit(path.to_path_buf(), kind);
                }
            }
        })
        .map_err(|e| anyhow!("notify watcher init: {e}"))?;
        watcher
            .watch(root, notify::RecursiveMode::Recursive)
            .map_err(|e| anyhow!("watch root: {e}"))?;
        *guard = Some(watcher);
        self.watched_roots.lock().insert(root.to_path_buf());
        Ok(())
    }

    pub async fn drain_and_apply(&self) -> Result<usize> {
        let now = Instant::now();
        let events = self.debouncer.drain_due(now);
        let mut applied = 0usize;
        for ev in events {
            let _ = self
                .index
                .process_event(FileChangeEvent { path: ev.path, kind: ev.kind })
                .await;
            applied += 1;
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_code_semantic_hash_stable() {
        let a = "fn main() { let x = 1; }";
        let b = "fn main() { let x = 1; }";
        assert_eq!(code_semantic_hash(a), code_semantic_hash(b));
    }

    #[test]
    fn test_code_semantic_hash_sensitive_to_tokens() {
        let a = "fn main() { let x = 1; }";
        let b = "fn main() { let y = 1; }";
        assert_ne!(code_semantic_hash(a), code_semantic_hash(b));
    }

    #[test]
    fn test_skip_paths() {
        assert!(should_skip(Path::new("project/target/debug/foo")));
        assert!(should_skip(Path::new("project/.git/HEAD")));
        assert!(should_skip(Path::new("project/node_modules/foo.js")));
        assert!(!should_skip(Path::new("project/src/main.rs")));
    }

    #[tokio::test]
    async fn test_persist_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let idx = index_file_path_backend_only_for_test();
        let persisted = PersistedIndex {
            version: 1,
            entries: vec![PersistedEntry {
                uri: "a.rs".into(),
                file_hash: "abc123".into(),
                language: Some("rust".into()),
            }],
        };
        let json = serde_json::to_string_pretty(&persisted).unwrap();
        fs::write(idx.clone(), json.as_bytes()).await.unwrap();

        let read = fs::read(idx).await.unwrap();
        let loaded: PersistedIndex = serde_json::from_slice(&read).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].uri, "a.rs");
        drop(dir);
    }

    fn index_file_path_backend_only_for_test() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let root = std::env::var("DEEPCARP_HOME")
            .map(|p| PathBuf::from(p))
            .unwrap_or_else(|_| home.join(".deepseek-carp"));
        std::fs::create_dir_all(&root).ok();
        root.join("index.json")
    }

    #[tokio::test]
    async fn test_incremental_reparse_updates_only_changed() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.rs");
        let file_b = dir.path().join("b.rs");
        fs::write(&file_a, "fn alpha() {}\n").await.unwrap();
        fs::write(&file_b, "fn beta() {}\n").await.unwrap();

        let idx = PersistentSemanticIndex::new(usize::MAX, DEFAULT_DEBOUNCE_MS).await;
        idx.reparse_file(&file_a).await.unwrap();
        idx.reparse_file(&file_b).await.unwrap();

        let a_before = idx.inner.search_symbols("alpha").await;
        let b_before = idx.inner.search_symbols("beta").await;
        assert!(!a_before.is_empty());
        assert!(!b_before.is_empty());

        fs::write(&file_a, "fn alpha_renamed() {}\n").await.unwrap();
        idx.reparse_file(&file_a).await.unwrap();

        let a_after = idx.inner.search_symbols("alpha").await;
        let a_renamed = idx.inner.search_symbols("alpha_renamed").await;
        let b_after = idx.inner.search_symbols("beta").await;
        assert!(a_after.is_empty(), "old symbol should be gone");
        assert!(!a_renamed.is_empty(), "new symbol should exist");
        assert!(!b_after.is_empty(), "other file untouched");
        drop(dir);
    }

    #[tokio::test]
    async fn test_debouncer_merges_rapid_changes() {
        let deb = Debouncer::new(200);
        let p = PathBuf::from("/tmp/project/main.rs");
        deb.submit(p.clone(), FileChangeKind::Modify);
        deb.submit(p.clone(), FileChangeKind::Modify);
        deb.submit(p.clone(), FileChangeKind::Modify);
        deb.submit(p.clone(), FileChangeKind::Modify);
        assert_eq!(deb.pending_count(), 1);

        tokio::time::sleep(StdDuration::from_millis(300)).await;
        let due = deb.drain_due(Instant::now());
        assert_eq!(due.len(), 1);
    }

    #[tokio::test]
    async fn test_file_change_delete_clears_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.rs");
        fs::write(&f, "fn hello() {}\n").await.unwrap();

        let idx = PersistentSemanticIndex::new(usize::MAX, DEFAULT_DEBOUNCE_MS).await;
        idx.reparse_file(&f).await.unwrap();
        let before = idx.inner.search_symbols("hello").await;
        assert!(!before.is_empty());

        idx.process_event(FileChangeEvent {
            path: f.clone(),
            kind: FileChangeKind::Delete,
        })
        .await
        .unwrap();

        let after = idx.inner.search_symbols("hello").await;
        assert!(after.is_empty());
        drop(dir);
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(Path::new("x.rs")), "rust");
        assert_eq!(detect_language(Path::new("x.ts")), "typescript");
        assert_eq!(detect_language(Path::new("x.py")), "python");
        assert_eq!(detect_language(Path::new("x.go")), "go");
        assert_eq!(detect_language(Path::new("x.unknown")), "generic");
    }
}
