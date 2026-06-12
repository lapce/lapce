//! Multi-file Semantic Index - Project-level code indexing.
//!
//! This module provides:
//! - Code symbol indexing
//! - Cross-reference tracking
//! - Semantic search
//! - Index persistence

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use futures::executor::block_on;

/// A code symbol (function, class, struct, etc.).
#[derive(Debug, Clone)]
pub struct CodeSymbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub visibility: Visibility,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub dependencies: Vec<String>,
}

impl CodeSymbol {
    /// Convert symbol kind to a human-readable string for embedding text.
    pub fn kind_to_str(&self) -> &'static str {
        match self.kind {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Module => "module",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Type => "type",
            SymbolKind::Interface => "interface",
        }
    }
}

/// Kind of symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Module,
    Constant,
    Variable,
    Type,
    Interface,
}

/// Visibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

/// An index entry.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub symbol: CodeSymbol,
    pub embeddings: Vec<f32>,
    pub references: Vec<Reference>,
    pub children: Vec<String>,
}

/// A reference to a symbol.
#[derive(Debug, Clone)]
pub struct Reference {
    pub file: String,
    pub line: usize,
    pub context: String,
}

/// Semantic index configuration.
#[derive(Debug, Clone)]
pub struct SemanticIndexConfig {
    pub max_embeddings: usize,
    pub embedding_dim: usize,
    pub index_updates_interval_secs: u64,
    pub enable_incremental_updates: bool,
    /// Embedding generation strategy.
    pub strategy: EmbeddingStrategy,
}

impl Default for SemanticIndexConfig {
    fn default() -> Self {
        Self {
            max_embeddings: 100000,
            embedding_dim: 384,
            index_updates_interval_secs: 300,
            enable_incremental_updates: true,
            strategy: EmbeddingStrategy::TfIdf,
        }
    }
}

// ════════════════════════════════════════════════════════════════
// Embedding Provider — pluggable backend for real semantic vectors
// ════════════════════════════════════════════════════════════════

/// Strategy for generating text embeddings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingStrategy {
    /// TF-IDF weighted bag-of-words (no external dependency).
    TfIdf,
    /// Hash-based fallback (original behavior, lowest quality).
    #[default]
    Hash,
    /// Remote API embedding (DeepSeek / OpenAI-compatible).
    Api,
}

/// Configuration for API-based embedding backends.
#[derive(Debug, Clone)]
pub struct ApiEmbeddingConfig {
    /// Base URL for the embedding API endpoint.
    pub endpoint: String,
    /// API key for authentication.
    pub api_key: String,
    /// Model name to use for embeddings.
    pub model: String,
    /// Dimension of the output vectors (e.g., 1536 for text-embedding-ada-002, 384 for local models).
    pub dimension: usize,
}

impl Default for ApiEmbeddingConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.deepseek.com/v1/embeddings".into(),
            api_key: String::new(),
            model: "deepseek-embedding".into(),
            dimension: 1536,
        }
    }
}

/// Trait for embedding providers — allows swapping backends without
/// changing SemanticIndex internals.
pub trait EmbeddingProvider: Send + Sync {
    /// Encode a single text string into a vector of f32 values.
    fn embed(&self, text: &str) -> Vec<f32>;

    /// Encode multiple texts in batch (may be more efficient than one-by-one).
    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Return the dimensionality of vectors produced by this provider.
    fn dimension(&self) -> usize;
}

// ── TF-IDF Provider ──────────────────────────────────────────────

/// A simple but effective TF-IDF embedding provider that runs entirely locally.
///
/// Builds a document frequency table from indexed symbols and produces
/// weighted term-frequency vectors. While not a neural embedding, TF-IDF
/// captures meaningful semantic similarity for code symbols (function names
/// share terms → higher similarity).
pub struct TfIdfEmbedder {
    /// Global document frequency: term → number of documents containing it.
    df: std::collections::HashMap<String, usize>,
    /// Total number of documents seen.
    total_docs: usize,
    /// Output dimension (vocabulary size cap).
    dim: usize,
    /// Term → index mapping (fixed after first index pass).
    vocab: std::collections::HashMap<String, usize>,
    /// Whether vocabulary has been finalized.
    vocab_finalized: bool,
}

impl TfIdfEmbedder {
    /// Create a new TF-IDF embedder with the given dimension cap.
    pub fn new(dim: usize) -> Self {
        Self {
            df: std::collections::HashMap::new(),
            total_docs: 0,
            dim,
            vocab: std::collections::HashMap::new(),
            vocab_finalized: false,
        }
    }

    /// Feed a document to build the DF table. Call this for all documents
    /// before calling `finalize_vocab()`.
    pub fn feed(&mut self, text: &str) {
        let terms = Self::tokenize(text);
        let mut seen = std::collections::HashSet::new();
        for term in &terms {
            if seen.insert(term.clone()) {
                *self.df.entry(term.clone()).or_insert(0) += 1;
            }
        }
        self.total_docs += 1;
    }

    /// Finalize the vocabulary from observed terms. After this call,
    /// no more documents can be fed and `embed()` becomes available.
    pub fn finalize_vocab(&mut self) {
        // Sort terms by frequency (most common first), take top `dim`
        let mut term_freqs: Vec<(String, usize)> = self.drain();
        term_freqs.sort_by(|a, b| b.1.cmp(&a.1));

        self.vocab.clear();
        for (i, (term, _)) in term_freqs.into_iter().take(self.dim).enumerate() {
            self.vocab.insert(term, i);
        }
        self.vocab_finalized = true;
    }

    fn drain(&mut self) -> Vec<(String, usize)> {
        let mut result = Vec::with_capacity(self.df.len());
        for (k, v) in self.df.drain() {
            result.push((k, v));
        }
        result
    }

    /// Tokenize text into terms (lowercased alphanumeric n-grams + words).
    fn tokenize(text: &str) -> Vec<String> {
        let mut terms = Vec::new();
        // Whole words (lowercase)
        for word in text.split_whitespace() {
            let cleaned: String = word.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect();
            if !cleaned.is_empty() && cleaned.len() >= 2 {
                terms.push(cleaned);
            }
        }
        // Character bigrams for short symbols
        let lower: String = text.chars().flat_map(|c| c.to_lowercase()).collect();
        for window in lower.as_bytes().windows(2) {
            if window[0].is_ascii_alphanumeric() && window[1].is_ascii_alphanumeric() {
                terms.push(String::from_utf8_lossy(window).into_owned());
            }
        }
        terms
    }
}

impl EmbeddingProvider for TfIdfEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        if !self.vocab_finalized || self.vocab.is_empty() {
            return vec![0.0; self.dim];
        }

        let mut vec = vec![0.0f32; self.dim];
        let terms = Self::tokenize(text);
        let mut tf: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for term in &terms {
            *tf.entry(term.clone()).or_insert(0) += 1;
        }

        let norm = terms.len().max(1) as f32;

        for (term, count) in &tf {
            if let Some(&idx) = self.vocab.get(term) {
                let tf_val = (*count as f32) / norm;
                let idf_val = ((self.total_docs as f32)
                    / (*self.df.get(term).unwrap_or(&1) as f32 + 1.0)).ln() + 1.0;
                vec[idx] = tf_val * idf_val;
            }
        }

        // L2 normalize
        let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag > 0.0 {
            for v in vec.iter_mut() {
                *v /= mag;
            }
        }

        vec
    }

    fn dimension(&self) -> usize { self.dim }
}

// ── API Embedding Provider ────────────────────────────────────────

/// Embedding provider that calls a remote API (OpenAI-compatible format).
///
/// Supports DeepSeek, GLM, Kimi, Minimax, and any OpenAI-compatible
/// embedding endpoint. Falls back to zero vector on network failure.
pub struct ApiEmbedder {
    config: ApiEmbeddingConfig,
    client: reqwest::Client,
}

impl ApiEmbedder {
    pub fn new(config: ApiEmbeddingConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Create an API embedder with default DeepSeek embedding config.
    pub fn deepseek_default(api_key: &str) -> Self {
        Self::new(ApiEmbeddingConfig {
            endpoint: "https://api.deepseek.com/v1/embeddings".into(),
            api_key: api_key.to_string(),
            model: "deepseek-embedding".into(),
            dimension: 1536,
        })
    }
}

impl EmbeddingProvider for ApiEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let body = serde_json::json!({
            "input": text,
            "model": self.config.model,
        });

        match block_on(
            self.client
                .post(&self.config.endpoint)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .json(&body)
                .send(),
        ) {
            Ok(resp) => match block_on(resp.json::<serde_json::Value>()) {
                Ok(json) => {
                    if let Some(data) = json.get("data").and_then(|d| d.get(0)) {
                        if let Some(embedding) = data.get("embedding") {
                            return embedding.as_array()
                                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).map(|v| v as f32).collect())
                                .unwrap_or_else(|| vec![0.0; self.config.dimension]);
                        }
                    }
                    tracing::warn!("API embedding response missing 'data' field");
                    vec![0.0; self.config.dimension]
                }
                Err(e) => {
                    tracing::warn!(error=%e, "Failed to parse API embedding response");
                    vec![0.0; self.config.dimension]
                }
            },
            Err(e) => {
                tracing::warn!(error=%e, "API embedding request failed");
                vec![0.0; self.config.dimension]
            }
        }
    }

    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        let body = serde_json::json!({
            "input": texts,
            "model": self.config.model,
        });

        match block_on(
            self.client
                .post(&self.config.endpoint)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .json(&body)
                .send(),
        ) {
            Ok(resp) => match block_on(resp.json::<serde_json::Value>()) {
                Ok(json) => {
                    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                        let mut results = Vec::with_capacity(texts.len());
                        for item in data {
                            if let Some(embedding) = item.get("embedding") {
                                results.push(
                                    embedding.as_array()
                                        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).map(|v| v as f32).collect())
                                        .unwrap_or(vec![0.0; self.config.dimension])
                                );
                            } else {
                                results.push(vec![0.0; self.config.dimension]);
                            }
                        }
                        return results;
                    }
                    texts.iter().map(|_| vec![0.0; self.config.dimension]).collect()
                }
                Err(_) => texts.iter().map(|_| vec![0.0; self.config.dimension]).collect(),
            },
            Err(_) => texts.iter().map(|_| vec![0.0; self.config.dimension]).collect(),
        }
    }

    fn dimension(&self) -> usize { self.config.dimension }
}

/// Multi-file semantic index.
pub struct SemanticIndex {
    config: SemanticIndexConfig,
    symbols: Arc<RwLock<HashMap<String, IndexEntry>>>,
    file_index: Arc<RwLock<HashMap<String, Vec<String>>>>,  // file -> symbol IDs
    name_index: Arc<RwLock<HashMap<String, Vec<String>>>>,  // name -> symbol IDs
    kind_index: Arc<RwLock<HashMap<SymbolKind, Vec<String>>>>,
    /// Embedding strategy configuration.
    pub strategy: EmbeddingStrategy,
    /// The active embedding provider (Box<dyn> for runtime polymorphism).
    embedder: Option<Box<dyn EmbeddingProvider>>,
    /// TF-IDF embedder state (kept separate for feed/finalize lifecycle).
    tfidf: Option<TfIdfEmbedder>,
}

impl SemanticIndex {
    pub fn new(config: SemanticIndexConfig) -> Self {
        let strategy = config.strategy;
        let embedding_dim = config.embedding_dim;
        Self {
            config,
            symbols: Arc::new(RwLock::new(HashMap::new())),
            file_index: Arc::new(RwLock::new(HashMap::new())),
            name_index: Arc::new(RwLock::new(HashMap::new())),
            kind_index: Arc::new(RwLock::new(HashMap::new())),
            strategy,
            embedder: None,
            tfidf: Some(TfIdfEmbedder::new(embedding_dim)),
        }
    }

    /// Index a project directory (sync wrapper).
    pub fn index_project_sync(&self, project_path: &Path) -> IndexStats {
        let mut stats = IndexStats::default();

        if let Ok(entries) = std::fs::read_dir(project_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.') && !Self::is_excluded_dir(name) {
                            let sub_stats = self.index_project_sync(&path);
                            stats.merge(sub_stats);
                        }
                    }
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lang) = Self::extension_to_language(ext) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let file_path = path.to_string_lossy().to_string();
                            let file_stats = self.index_file_sync(&file_path, &content, &lang);
                            stats.merge(file_stats);
                        }
                    }
                }
            }
        }

        stats
    }

    /// Index a project directory (async wrapper).
    pub async fn index_project(&self, project_path: &Path) -> IndexStats {
        self.index_project_sync(project_path)
    }

    /// Index a single file (sync version).
    pub fn index_file_sync(&self, file_path: &str, content: &str, language: &str) -> IndexStats {
        let mut stats = IndexStats::default();
        let symbols = Self::extract_symbols(content, file_path, language);

        let mut file_symbols = Vec::new();
        let mut new_entries: HashMap<String, IndexEntry> = HashMap::new();
        let mut new_name_entries: HashMap<String, Vec<String>> = HashMap::new();
        let mut new_kind_entries: HashMap<SymbolKind, Vec<String>> = HashMap::new();

        for symbol in symbols {
            let entry = IndexEntry {
                symbol: symbol.clone(),
                embeddings: self.generate_embeddings(&symbol),
                references: Vec::new(),
                children: Vec::new(),
            };

            let symbol_id = symbol.id.clone();
            new_entries.insert(symbol_id.clone(), entry);
            file_symbols.push(symbol_id.clone());

            // Update name index
            new_name_entries
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol_id.clone());

            // Update kind index
            new_kind_entries
                .entry(symbol.kind)
                .or_default()
                .push(symbol_id.clone());

            stats.symbols_indexed += 1;
        }

        // Batch update indices (sync)
        {
            let mut symbols_guard = block_on(self.symbols.write());
            symbols_guard.extend(new_entries);
        }
        {
            let mut name_guard = block_on(self.name_index.write());
            for (name, ids) in new_name_entries {
                name_guard.entry(name).or_default().extend(ids);
            }
        }
        {
            let mut kind_guard = block_on(self.kind_index.write());
            for (kind, ids) in new_kind_entries {
                kind_guard.entry(kind).or_default().extend(ids);
            }
        }

        {
            let mut file_guard = block_on(self.file_index.write());
            file_guard.insert(file_path.to_string(), file_symbols);
        }

        stats
    }

    /// Async version that delegates to sync.
    pub async fn index_file(&self, file_path: &str, content: &str, language: &str) -> IndexStats {
        self.index_file_sync(file_path, content, language)
    }

    /// Extract symbols from code content.
    fn extract_symbols(content: &str, file_path: &str, language: &str) -> Vec<CodeSymbol> {
        let mut symbols = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Extract based on language
            match language {
                "rust" => {
                    if let Some(symbol) = Self::parse_rust_symbol(trimmed, file_path, i + 1) {
                        symbols.push(symbol);
                    }
                }
                "python" => {
                    if let Some(symbol) = Self::parse_python_symbol(trimmed, file_path, i + 1) {
                        symbols.push(symbol);
                    }
                }
                "typescript" | "javascript" => {
                    if let Some(symbol) = Self::parse_js_symbol(trimmed, file_path, i + 1) {
                        symbols.push(symbol);
                    }
                }
                _ => {}
            }
        }

        symbols
    }

    /// Parse Rust symbols.
    fn parse_rust_symbol(line: &str, file: &str, line_num: usize) -> Option<CodeSymbol> {
        if line.starts_with("pub fn ") || line.starts_with("fn ") {
            let name = line.trim_start_matches("pub fn ")
                .trim_start_matches("fn ")
                .split('(')
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Function,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 10, // Approximate
                visibility: if line.starts_with("pub ") { Visibility::Public } else { Visibility::Private },
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        if line.starts_with("pub struct ") || line.starts_with("struct ") {
            let name = line.trim_start_matches("pub struct ")
                .trim_start_matches("struct ")
                .split_whitespace()
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Struct,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 20,
                visibility: if line.starts_with("pub ") { Visibility::Public } else { Visibility::Private },
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        if line.starts_with("pub enum ") || line.starts_with("enum ") {
            let name = line.trim_start_matches("pub enum ")
                .trim_start_matches("enum ")
                .split_whitespace()
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Enum,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 15,
                visibility: if line.starts_with("pub ") { Visibility::Public } else { Visibility::Private },
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        None
    }

    /// Parse Python symbols.
    fn parse_python_symbol(line: &str, file: &str, line_num: usize) -> Option<CodeSymbol> {
        if line.starts_with("def ") {
            let name = line.trim_start_matches("def ")
                .split('(')
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Function,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 5,
                visibility: Visibility::Public,
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        if line.starts_with("class ") {
            let name = line.trim_start_matches("class ")
                .split('(')
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Class,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 30,
                visibility: Visibility::Public,
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        None
    }

    /// Parse JavaScript/TypeScript symbols.
    fn parse_js_symbol(line: &str, file: &str, line_num: usize) -> Option<CodeSymbol> {
        if line.contains("function ") || line.contains("=> {") {
            let name = if line.contains("function ") {
                line.split("function ").nth(1)?.split('(').next()?.trim()
            } else {
                line.split("=>").next()?.trim()
            };

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Function,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 10,
                visibility: Visibility::Public,
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        if line.starts_with("class ") {
            let name = line.trim_start_matches("class ")
                .split_whitespace()
                .next()?
                .trim();

            return Some(CodeSymbol {
                id: format!("{}:{}:{}", file, line_num, name),
                name: name.to_string(),
                kind: SymbolKind::Class,
                file: file.to_string(),
                line: line_num,
                column: 0,
                end_line: line_num + 30,
                visibility: Visibility::Public,
                signature: Some(line.to_string()),
                doc_comment: None,
                dependencies: Vec::new(),
            });
        }

        None
    }

    /// Generate embeddings for a symbol.
    fn generate_embeddings(&self, symbol: &CodeSymbol) -> Vec<f32> {
        match &self.embedder {
            Some(provider) => {
                // Build a rich text representation of the symbol
                let text = format!(
                    "{} {} {} {}",
                    symbol.name,
                    symbol.kind_to_str(),
                    symbol.signature.as_deref().unwrap_or(""),
                    symbol.doc_comment.as_deref().unwrap_or("")
                );
                provider.embed(&text)
            }
            None => {
                // Fallback: use TF-IDF or hash based on strategy
                match self.strategy {
                    EmbeddingStrategy::TfIdf => {
                        // TF-IDF requires pre-built vocab; use hash as fallback
                        let text = &symbol.name;
                        let mut embedding = Vec::with_capacity(self.config.embedding_dim);
                        let bytes = text.as_bytes();
                        for i in 0..self.config.embedding_dim {
                            let idx = i % bytes.len();
                            embedding.push((bytes[idx] as f32 - 128.0) / 128.0);
                        }
                        embedding
                    }
                    EmbeddingStrategy::Hash => {
                        // Original hash-based approach (lowest quality)
                        let name_bytes = symbol.name.as_bytes();
                        let mut embedding = Vec::with_capacity(self.config.embedding_dim);
                        for i in 0..self.config.embedding_dim {
                            let idx = i % name_bytes.len();
                            embedding.push((name_bytes[idx] as f32 - 128.0) / 128.0);
                        }
                        let kind_val = match symbol.kind {
                            SymbolKind::Function => 1.0,
                            SymbolKind::Class => 2.0,
                            SymbolKind::Struct => 3.0,
                            SymbolKind::Enum => 4.0,
                            _ => 0.0,
                        };
                        for i in 0..self.config.embedding_dim {
                            embedding[i] = (embedding[i] + kind_val) / 2.0;
                        }
                        embedding
                    }
                    EmbeddingStrategy::Api => {
                        // No API embedder configured, fall back to hash
                        let name_bytes = symbol.name.as_bytes();
                        let mut embedding = Vec::with_capacity(self.config.embedding_dim);
                        for i in 0..self.config.embedding_dim {
                            let idx = i % name_bytes.len();
                            embedding.push((name_bytes[idx] as f32 - 128.0) / 128.0);
                        }
                        embedding
                    }
                }
            }
        }
    }

    /// Search for symbols by name.
    pub async fn search_by_name(&self, name: &str) -> Vec<CodeSymbol> {
        let name_index = self.name_index.read().await;
        let symbols = self.symbols.read().await;

        let mut results = Vec::new();

        if let Some(ids) = name_index.get(name) {
            for id in ids {
                if let Some(entry) = symbols.get(id) {
                    results.push(entry.symbol.clone());
                }
            }
        }

        // Fuzzy match
        let name_lower = name.to_lowercase();
        for (sym_name, ids) in name_index.iter() {
            if sym_name.to_lowercase().contains(&name_lower) && !results.iter().any(|s: &CodeSymbol| s.name == *sym_name) {
                for id in ids {
                    if let Some(entry) = symbols.get(id) {
                        results.push(entry.symbol.clone());
                    }
                }
            }
        }

        results
    }

    /// Search for symbols by kind.
    pub async fn search_by_kind(&self, kind: SymbolKind) -> Vec<CodeSymbol> {
        let kind_index = self.kind_index.read().await;
        let symbols = self.symbols.read().await;

        let mut results = Vec::new();

        if let Some(ids) = kind_index.get(&kind) {
            for id in ids {
                if let Some(entry) = symbols.get(id) {
                    results.push(entry.symbol.clone());
                }
            }
        }

        results
    }

    /// Search by semantic similarity.
    pub async fn semantic_search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_embedding = self.embed_text(query);
        let symbols = self.symbols.read().await;

        let mut results: Vec<(String, f32)> = symbols.iter()
            .map(|(id, entry)| {
                let similarity = Self::cosine_similarity(&query_embedding, &entry.embeddings);
                (id.clone(), similarity)
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results
            .into_iter()
            .take(limit)
            .filter_map(|(id, score)| {
                symbols.get(&id).map(|entry| SearchResult {
                    symbol: entry.symbol.clone(),
                    score,
                })
            })
            .collect()
    }

    /// Embed text to vector.
    fn embed_text(&self, text: &str) -> Vec<f32> {
        match &self.embedder {
            Some(provider) => provider.embed(text),
            None => {
                // Enhanced hash fallback with word-level tokenization
                let mut embedding = Vec::with_capacity(self.config.embedding_dim);
                let tokens: Vec<&str> = text.split_whitespace().collect();
                if tokens.is_empty() {
                    let bytes = text.as_bytes();
                    for i in 0..self.config.embedding_dim {
                        let idx = i % bytes.len().max(1);
                        embedding.push((bytes[idx] as f32 - 128.0) / 128.0);
                    }
                } else {
                    for i in 0..self.config.embedding_dim {
                        let token = tokens[i % tokens.len()];
                        let hash = Self::simple_hash(token);
                        embedding.push((hash as f32) / (u32::MAX as f32) * 2.0 - 1.0);
                    }
                }
                embedding
            }
        }
    }

    /// Simple DJB2-style hash for token-to-vector mapping.
    fn simple_hash(s: &str) -> u32 {
        let mut hash: u32 = 5381;
        for b in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(b as u32);
        }
        hash
    }

    /// Switch to an API-based embedding provider.
    ///
    /// After calling this, all subsequent `generate_embeddings()` and
    /// `embed_text()` calls will use the remote API.
    pub fn set_api_embedder(&mut self, config: ApiEmbeddingConfig) {
        self.strategy = EmbeddingStrategy::Api;
        let endpoint = config.endpoint.clone();
        let model = config.model.clone();
        let dim = config.dimension;
        self.embedder = Some(Box::new(ApiEmbedder::new(config)));
        tracing::info!(
            endpoint=%endpoint,
            model=%model,
            dim,
            "Switched to API embedding provider"
        );
    }

    /// Build TF-IDF vocabulary from all currently indexed symbols.
    ///
    /// Must be called after indexing is complete for best results.
    /// After this, `set_tfidf_provider()` activates TF-IDF mode.
    pub fn build_tfidf_vocab(&mut self) {
        if let Some(ref mut tfidf) = self.tfidf {
            // Feed all symbol texts into TF-IDF
            let symbols = block_on(self.symbols.read());
            for entry in symbols.values() {
                let text = format!(
                    "{} {} {}",
                    entry.symbol.name,
                    entry.symbol.kind_to_str(),
                    entry.symbol.signature.as_deref().unwrap_or("")
                );
                tfidf.feed(&text);
            }
            tfidf.finalize_vocab();
            tracing::info!(
                vocab_size=tfidf.vocab.len(),
                total_docs=tfidf.total_docs,
                "TF-IDF vocabulary built"
            );
        }
    }

    /// Activate the TF-IDF embedding provider (after build_tfidf_vocab).
    pub fn activate_tfidf(&mut self) {
        if let Some(tfidf) = self.tfidf.take() {
            self.strategy = EmbeddingStrategy::TfIdf;
            self.embedder = Some(Box::new(tfidf));
            tracing::info!("TF-IDF embedding provider activated");
        }
    }

    /// Cosine similarity.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if mag_a == 0.0 || mag_b == 0.0 {
            0.0
        } else {
            dot / (mag_a * mag_b)
        }
    }

    /// Get symbols in a file.
    pub async fn get_file_symbols(&self, file_path: &str) -> Vec<CodeSymbol> {
        let file_index = self.file_index.read().await;
        let symbols = self.symbols.read().await;

        let mut results = Vec::new();

        if let Some(ids) = file_index.get(file_path) {
            for id in ids {
                if let Some(entry) = symbols.get(id) {
                    results.push(entry.symbol.clone());
                }
            }
        }

        results
    }

    /// Get index statistics.
    pub async fn stats(&self) -> IndexStats {
        let symbols = self.symbols.read().await;
        let file_index = self.file_index.read().await;

        let mut kind_counts = HashMap::new();
        for entry in symbols.values() {
            *kind_counts.entry(entry.symbol.kind).or_insert(0) += 1;
        }

        IndexStats {
            total_symbols: symbols.len(),
            total_files: file_index.len(),
            by_kind: kind_counts,
            symbols_indexed: 0,
        }
    }

    /// Check if directory should be excluded.
    fn is_excluded_dir(name: &str) -> bool {
        matches!(
            name,
            "target" | "node_modules" | "__pycache__" | ".git" | "dist" | "build" | ".next" | "vendor"
        )
    }

    /// Map extension to language.
    fn extension_to_language(ext: &str) -> Option<String> {
        match ext {
            "rs" => Some("rust".to_string()),
            "py" => Some("python".to_string()),
            "js" | "jsx" => Some("javascript".to_string()),
            "ts" | "tsx" => Some("typescript".to_string()),
            "go" => Some("go".to_string()),
            "java" => Some("java".to_string()),
            _ => None,
        }
    }
}

impl Default for SemanticIndex {
    fn default() -> Self {
        Self::new(SemanticIndexConfig::default())
    }
}

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub total_symbols: usize,
    pub total_files: usize,
    pub symbols_indexed: usize,
    pub by_kind: HashMap<SymbolKind, usize>,
}

impl IndexStats {
    pub fn merge(&mut self, other: IndexStats) {
        self.total_symbols += other.total_symbols;
        self.total_files += other.total_files;
        self.symbols_indexed += other.symbols_indexed;
        for (kind, count) in other.by_kind {
            *self.by_kind.entry(kind).or_insert(0) += count;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub symbol: CodeSymbol,
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EmbeddingStrategy tests ───────────────────────────────────

    #[test]
    fn test_embedding_strategy_default() {
        assert_eq!(EmbeddingStrategy::default(), EmbeddingStrategy::Hash);
    }

    #[test]
    fn test_embedding_strategy_equality() {
        assert_eq!(EmbeddingStrategy::TfIdf, EmbeddingStrategy::TfIdf);
        assert_ne!(EmbeddingStrategy::TfIdf, EmbeddingStrategy::Hash);
        assert_ne!(EmbeddingStrategy::Hash, EmbeddingStrategy::Api);
    }

    // ── ApiEmbeddingConfig tests ──────────────────────────────────

    #[test]
    fn test_api_config_default() {
        let config = ApiEmbeddingConfig::default();
        assert_eq!(config.endpoint, "https://api.deepseek.com/v1/embeddings");
        assert!(config.api_key.is_empty());
        assert_eq!(config.model, "deepseek-embedding");
        assert_eq!(config.dimension, 1536);
    }

    #[test]
    fn test_api_config_custom() {
        let config = ApiEmbeddingConfig {
            endpoint: "http://localhost:8080/embed".into(),
            api_key: "test-key".into(),
            model: "local-model".into(),
            dimension: 384,
        };
        assert_eq!(config.dimension, 384);
        assert_eq!(config.api_key, "test-key");
    }

    // ── CodeSymbol::kind_to_str tests ─────────────────────────────

    #[test]
    fn test_kind_to_str() {
        let sym = CodeSymbol {
            id: "test".into(),
            name: "foo".into(),
            kind: SymbolKind::Function,
            file: "".into(),
            line: 0,
            column: 0,
            end_line: 0,
            visibility: Visibility::Public,
            signature: None,
            doc_comment: None,
            dependencies: Vec::new(),
        };
        assert_eq!(sym.kind_to_str(), "function");

        let sym_class = CodeSymbol { kind: SymbolKind::Class, ..sym.clone() };
        assert_eq!(sym_class.kind_to_str(), "class");

        let sym_struct = CodeSymbol { kind: SymbolKind::Struct, ..sym.clone() };
        assert_eq!(sym_struct.kind_to_str(), "struct");

        let sym_enum = CodeSymbol { kind: SymbolKind::Enum, ..sym.clone() };
        assert_eq!(sym_enum.kind_to_str(), "enum");

        let sym_trait = CodeSymbol { kind: SymbolKind::Trait, ..sym.clone() };
        assert_eq!(sym_trait.kind_to_str(), "trait");

        let sym_method = CodeSymbol { kind: SymbolKind::Method, ..sym.clone() };
        assert_eq!(sym_method.kind_to_str(), "method");

        let sym_module = CodeSymbol { kind: SymbolKind::Module, ..sym.clone() };
        assert_eq!(sym_module.kind_to_str(), "module");

        let sym_constant = CodeSymbol { kind: SymbolKind::Constant, ..sym.clone() };
        assert_eq!(sym_constant.kind_to_str(), "constant");

        let sym_variable = CodeSymbol { kind: SymbolKind::Variable, ..sym.clone() };
        assert_eq!(sym_variable.kind_to_str(), "variable");

        let sym_type = CodeSymbol { kind: SymbolKind::Type, ..sym.clone() };
        assert_eq!(sym_type.kind_to_str(), "type");

        let sym_interface = CodeSymbol { kind: SymbolKind::Interface, ..sym.clone() };
        assert_eq!(sym_interface.kind_to_str(), "interface");
    }

    // ── TfIdfEmbedder tests ───────────────────────────────────────

    #[test]
    fn test_tfidf_new() {
        let embedder = TfIdfEmbedder::new(128);
        assert_eq!(embedder.dimension(), 128);
        assert!(!embedder.vocab_finalized);
        assert!(embedder.vocab.is_empty());
    }

    #[test]
    fn test_tfidf_feed_and_finalize() {
        let mut embedder = TfIdfEmbedder::new(64);
        embedder.feed("fn calculate_sum");
        embedder.feed("fn print_hello");
        embedder.feed("struct Point");
        assert_eq!(embedder.total_docs, 3);

        embedder.finalize_vocab();
        assert!(embedder.vocab_finalized);
        assert!(!embedder.vocab.is_empty());
        // Vocabulary should be capped at dim
        assert!(embedder.vocab.len() <= 64);
    }

    #[test]
    fn test_tfidf_embed_before_finalize_returns_zero() {
        let embedder = TfIdfEmbedder::new(32);
        let vec = embedder.embed("hello world");
        assert_eq!(vec, vec![0.0f32; 32]);
    }

    #[test]
    fn test_tfidf_embed_after_finalize() {
        let mut embedder = TfIdfEmbedder::new(64);
        embedder.feed("fn calculate_sum");
        embedder.feed("fn calculate_product");
        embedder.finalize_vocab();

        let vec1 = embedder.embed("calculate_sum function");
        let vec2 = embedder.embed("print_hello unrelated");

        assert_eq!(vec1.len(), 64);
        assert_eq!(vec2.len(), 64);
        // vec1 should be non-zero since it shares terms with fed documents
        let mag1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(mag1 > 0.0, "TF-IDF embedding for related text should be non-zero");
    }

    #[test]
    fn test_tfidf_l2_normalized() {
        let mut embedder = TfIdfEmbedder::new(16);
        embedder.feed("hello world test");
        embedder.finalize_vocab();

        let vec = embedder.embed("hello world");
        let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 0.001, "L2 norm should be ~1.0, got {}", mag);
    }

    #[test]
    fn test_tfidf_embed_batch_default() {
        let mut embedder = TfIdfEmbedder::new(8);
        embedder.feed("test doc");
        embedder.finalize_vocab();

        let texts = vec!["hello".to_string(), "world".to_string()];
        let results = embedder.embed_batch(&texts);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].len(), 8);
        assert_eq!(results[1].len(), 8);
    }

    // ── ApiEmbedder tests ─────────────────────────────────────────

    #[test]
    fn test_api_embedder_creation() {
        let config = ApiEmbeddingConfig::default();
        let embedder = ApiEmbedder::new(config);
        assert_eq!(embedder.dimension(), 1536);
    }

    #[test]
    fn test_api_embedder_deepseek_default() {
        let embedder = ApiEmbedder::deepseek_default("test-key");
        assert_eq!(embedder.dimension(), 1536);
    }

    #[test]
    fn test_api_embedder_fallback_on_error() {
        // Using invalid endpoint — should return zero vector
        let config = ApiEmbeddingConfig {
            endpoint: "http://localhost:1/nonexistent".into(),
            api_key: "none".into(),
            model: "test".into(),
            dimension: 8,
        };
        let embedder = ApiEmbedder::new(config);
        let vec = embedder.embed("test");
        assert_eq!(vec, vec![0.0f32; 8]);
    }

    // ── SemanticIndex strategy tests ──────────────────────────────

    #[tokio::test]
    async fn test_index_with_hash_strategy() {
        let config = SemanticIndexConfig {
            strategy: EmbeddingStrategy::Hash,
            ..SemanticIndexConfig::default()
        };
        let index = SemanticIndex::new(config);
        assert_eq!(index.strategy, EmbeddingStrategy::Hash);

        let stats = index.index_file("test.rs", "fn add(a: i32) -> i32 { a }", "rust").await;
        assert_eq!(stats.symbols_indexed, 1);
    }

    #[tokio::test]
    async fn test_index_with_tfidf_strategy() {
        let config = SemanticIndexConfig {
            strategy: EmbeddingStrategy::TfIdf,
            ..SemanticIndexConfig::default()
        };
        let index = SemanticIndex::new(config);
        assert_eq!(index.strategy, EmbeddingStrategy::TfIdf);

        let stats = index.index_file("test.rs", "fn add(a: i32) -> i32 { a }", "rust").await;
        assert_eq!(stats.symbols_indexed, 1);
    }

    #[tokio::test]
    async fn test_set_api_embedder() {
        let mut index = SemanticIndex::default();
        let config = ApiEmbeddingConfig {
            endpoint: "http://localhost:1/test".into(),
            api_key: "key".into(),
            model: "model".into(),
            dimension: 16,
        };
        index.set_api_embedder(config);
        assert_eq!(index.strategy, EmbeddingStrategy::Api);
        assert!(index.embedder.is_some());
    }

    #[tokio::test]
    async fn test_build_and_activate_tfidf() {
        let mut index = SemanticIndex::default();
        index.index_file("test.rs", "fn calculate_sum(a: i32) -> i32 { a + b }", "rust").await;
        index.index_file("test.rs", "fn print_hello() { println!(\"hi\") }", "rust").await;

        // Build vocab and activate TF-IDF
        index.build_tfidf_vocab();
        index.activate_tfidf();

        assert_eq!(index.strategy, EmbeddingStrategy::TfIdf);
        assert!(index.embedder.is_some());
        assert!(index.tfidf.is_none()); // tfidf was taken by activate_tfidf
    }

    #[tokio::test]
    async fn test_semantic_search_with_tfidf() {
        let mut index = SemanticIndex::default();
        index.index_file("test.rs", "fn calculate_sum(a: i32, b: i32) -> i32 { a + b }", "rust").await;
        index.index_file("test.rs", "fn print_hello() { println!(\"hello\") }", "rust").await;

        index.build_tfidf_vocab();
        index.activate_tfidf();

        let results = index.semantic_search("math calculation", 5).await;
        assert!(results.len() <= 5);
    }

    // ── simple_hash tests ─────────────────────────────────────────

    #[test]
    fn test_simple_hash_deterministic() {
        let h1 = SemanticIndex::simple_hash("hello");
        let h2 = SemanticIndex::simple_hash("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_simple_hash_different_inputs() {
        let h1 = SemanticIndex::simple_hash("hello");
        let h2 = SemanticIndex::simple_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_simple_hash_empty_string() {
        let h = SemanticIndex::simple_hash("");
        assert_eq!(h, 5381); // DJB2 initial value for empty string
    }

    // ── Original backward-compat tests ─────────────────────────────

    #[tokio::test]
    async fn test_index_file() {
        let index = SemanticIndex::default();

        let code = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point {
    x: f32,
    y: f32,
}
"#;

        let stats = index.index_file("test.rs", code, "rust").await;
        assert_eq!(stats.symbols_indexed, 2);
    }

    #[tokio::test]
    async fn test_search_by_name() {
        let index = SemanticIndex::default();

        index.index_file("test.rs", "fn add(a: i32) -> i32 { a }", "rust").await;
        index.index_file("test.rs", "fn subtract(a: i32) -> i32 { a }", "rust").await;

        let results = index.search_by_name("add").await;
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "add");
    }

    #[tokio::test]
    async fn test_semantic_search() {
        let index = SemanticIndex::default();

        index.index_file("test.rs", "fn calculate_sum(a: i32, b: i32) -> i32 { a + b }", "rust").await;
        index.index_file("test.rs", "fn print_hello() { println!(\"hello\") }", "rust").await;

        let results = index.semantic_search("math calculation", 5).await;
        assert!(results.len() <= 5);
    }
}
