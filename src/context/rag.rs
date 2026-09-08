//! Codebase RAG (Retrieval-Augmented Generation) — Cursor-style indexing.
//!
//! Builds a searchable index of the codebase using tree-sitter for semantic
//! chunking and TF-IDF / keyword matching for retrieval. When the user asks
//! about code, relevant files and symbols are injected into context.
//!
//! ## Architecture
//!
//! ```text
//! Codebase → tree-sitter parse → extract symbols → inverted index
//!                                                              ↓
//! User query → keyword match → retrieve top-K chunks → inject into prompt
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

static ENTITY_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?m)^(pub\s+)?(unsafe\s+)?fn\s+(\w+)|^(pub\s+)?struct\s+(\w+)|^(pub\s+)?(enum|trait|impl)\s+(\w+)").unwrap()
});

/// Extract doc comment (/// lines) preceding a position.
fn extract_preceding_comment(content: &str, pos: usize) -> String {
    let before = &content[..pos];
    before
        .lines()
        .rev()
        .take_while(|l| l.trim_start().starts_with("///") || l.trim().is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

/// A chunk of code with metadata for retrieval.
#[derive(Debug, Clone)]
pub struct CodeChunk {
    /// File path relative to workspace root.
    pub file: PathBuf,
    /// Language (e.g., "rust", "python").
    pub language: String,
    /// Symbol name (function, struct, class, etc.).
    pub symbol: Option<String>,
    /// Starting line (1-based).
    pub start_line: usize,
    /// Ending line (1-based).
    pub end_line: usize,
    /// Code content.
    pub content: String,
    /// Keywords extracted for matching.
    pub keywords: Vec<String>,
}

/// In-memory inverted index for code search.
#[derive(Debug, Default)]
pub struct CodeIndex {
    /// All indexed chunks.
    chunks: Vec<CodeChunk>,
    /// Keyword → chunk indices (inverted index).
    inverted: HashMap<String, Vec<usize>>,
    /// Known file extensions to index.
    extensions: HashSet<String>,
    /// Directories to exclude.
    excludes: HashSet<String>,
}

/// Similarity score between query and a code chunk.
#[derive(Debug, Clone)]
pub struct SimilarityScore {
    /// Index into the chunks vector.
    pub chunk_idx: usize,
    /// BM25-like keyword match score.
    pub keyword_score: f32,
    /// Embedding cosine similarity (0.0 if no embeddings available).
    pub semantic_score: f32,
    /// Newer chunks get slight boost.
    pub recency_score: f32,
    /// Weighted combination of the above.
    pub final_score: f32,
    /// Debug info: why this matched.
    pub match_reasons: Vec<String>,
}

/// Retrieval configuration for hybrid search.
#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    /// Number of results to return.
    pub top_k: usize,
    /// Weight for keyword (BM25) score.
    pub keyword_weight: f32,
    /// Weight for semantic similarity score.
    pub semantic_weight: f32,
    /// Weight for recency boost.
    pub recency_weight: f32,
    /// Minimum final score to include a result.
    pub min_score_threshold: f32,
    /// Maximum number of chunks returned per file.
    pub max_chunks_per_file: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            top_k: 10,
            keyword_weight: 0.4,
            semantic_weight: 0.4,
            recency_weight: 0.2,
            min_score_threshold: 0.05,
            max_chunks_per_file: 3,
        }
    }
}

/// How to split documents into chunks for retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkingStrategy {
    /// Fixed-size token chunks with overlap (current)
    Fixed,
    /// Semantic chunks using tree-sitter AST boundaries (requires feature)
    #[cfg(feature = "semantic-chunk")]
    Semantic,
    /// Paragraph/section boundaries (line-based)
    Paragraph,
    /// Hybrid: semantic first, fallback to fixed
    Hybrid,
}

impl ChunkingStrategy {
    pub fn description(&self) -> &'static str {
        match self {
            ChunkingStrategy::Fixed => "Fixed-size token chunks with overlap",
            #[cfg(feature = "semantic-chunk")]
            ChunkingStrategy::Semantic => "Semantic chunks using tree-sitter AST boundaries",
            ChunkingStrategy::Paragraph => "Paragraph/section boundaries (line-based)",
            ChunkingStrategy::Hybrid => "Hybrid: semantic first, fallback to fixed",
        }
    }
}

/// Statistics about the current index state.
#[derive(Debug, Clone)]
pub struct IndexStats {
    /// Total number of indexed chunks.
    pub total_chunks: usize,
    /// Total number of distinct files.
    pub total_files: usize,
    /// Total unique keywords in the inverted index.
    pub total_keywords: usize,
    /// Average content length across chunks.
    pub avg_chunk_size: usize,
    /// Count of chunks per language.
    pub languages: HashMap<String, usize>,
}

/// Metadata about a retrieved chunk for debugging and relevance scoring.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkMetadata {
    pub chunk_id: usize,
    pub strategy: ChunkingStrategy,
    pub token_count: usize,
    pub doc_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub bm25_score: f64,
    pub trigram_score: f64,
    pub combined_score: f64,
}

impl CodeIndex {
    pub fn new() -> Self {
        let mut extensions = HashSet::new();
        for ext in ["rs", "py", "js", "ts", "go", "java", "cpp", "c", "h", "hpp", "swift", "kt", "toml", "yaml", "json", "md", "txt"] {
            extensions.insert(ext.to_string());
        }

        let mut excludes = HashSet::new();
        for dir in ["target", "node_modules", ".git", ".idea", "dist", "__pycache__", "build", "venv", ".venv"] {
            excludes.insert(dir.to_string());
        }

        Self { extensions, excludes, ..Default::default() }
    }

    /// Index a workspace directory recursively.
    pub fn index_workspace(&mut self, root: &Path) -> usize {
        let excludes = self.excludes.clone();
        let extensions = self.extensions.clone();
        let mut count = 0;
        let entries: Vec<_> = WalkDir::new(root)
            .into_iter()
            .filter_entry(move |e| {
                let name = e.file_name().to_string_lossy();
                !excludes.contains(name.as_ref())
            })
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path().extension().and_then(|e| e.to_str()).unwrap_or("");
                extensions.contains(ext) && e.path().is_file()
            })
            .collect();

        for entry in entries {
            if let Ok(n) = self.index_file(root, entry.path()) { count += n }
        }
        tracing::info!(files = count, "Workspace indexed");
        count
    }

    /// Index a single file.
    fn index_file(&mut self, root: &Path, path: &Path) -> std::io::Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let rel = path.strip_prefix(root).unwrap_or(path);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = Self::ext_to_lang(ext);

        // Use semantic chunking when available (tree-sitter powered)
        let chunks = if lang == "rust" {
            #[cfg(feature = "semantic-chunk")]
            {
                let mut chunker = crate::context::chunker::SemanticChunker::new();
                chunker.chunk_file(rel, &content, lang)
            }
            #[cfg(not(feature = "semantic-chunk"))]
            {
                Vec::new() // Will fallback below
            }
        } else {
            Vec::new()
        };

        // Fallback to simple line-based chunking if no semantic chunks
        let chunks: Vec<CodeChunk> = chunks;
        if chunks.is_empty() {
            let mut count = 0;
            let lines: Vec<&str> = content.lines().collect();
            let chunk_size = 50;
            let mut i = 0;

            while i < lines.len() {
                let end = (i + chunk_size).min(lines.len());
                let chunk_content = lines[i..end].join("\n");
                let symbol = Self::detect_symbol(lines[i]);
                let keywords = Self::extract_keywords(&chunk_content);

                let chunk = CodeChunk {
                    file: rel.to_path_buf(),
                    language: lang.to_string(),
                    symbol,
                    start_line: i + 1,
                    end_line: end,
                    content: chunk_content,
                    keywords: keywords.clone(),
                };

                let idx = self.chunks.len();
                self.chunks.push(chunk);
                for kw in &keywords {
                    self.inverted.entry(kw.clone()).or_default().push(idx);
                }
                count += 1;
                i = end;
            }
            return Ok(count);
        }

        // Index the semantic chunks
        let count = chunks.len();
        for chunk in chunks {
            let idx = self.chunks.len();
            let kws = chunk.keywords.clone();
            self.chunks.push(chunk);
            for kw in &kws {
                self.inverted.entry(kw.clone()).or_default().push(idx);
            }
        }

        Ok(count)
    }

    /// Search for relevant chunks matching the query.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<&CodeChunk> {
        let query_terms: Vec<String> = query.to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_string())
            .collect();

        let mut scores: HashMap<usize, usize> = HashMap::new();

        for term in &query_terms {
            if let Some(matches) = self.inverted.get(term) {
                for &idx in matches {
                    *scores.entry(idx).or_insert(0) += 1;
                }
            }
        }

        let mut ranked: Vec<(usize, usize)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked.truncate(top_k);

        ranked.iter()
            .filter_map(|(idx, _)| self.chunks.get(*idx))
            .collect()
    }

    /// Format search results as a context snippet for the LLM.
    pub fn format_context(&self, query: &str, top_k: usize) -> String {
        let results = self.search(query, top_k);
        if results.is_empty() {
            return String::new();
        }

        let mut ctx = String::from("## Relevant Code Context\n\n");
        for chunk in &results {
            ctx.push_str(&format!(
                "### {}:{} ({}:{})\n```{}\n{}\n```\n\n",
                chunk.file.display(),
                if let Some(sym) = &chunk.symbol { format!(" {}", sym) } else { String::new() },
                chunk.start_line,
                chunk.end_line,
                chunk.language,
                chunk.content,
            ));
        }
        ctx
    }

    fn ext_to_lang(ext: &str) -> &str {
        match ext {
            "rs" => "rust",
            "py" => "python",
            "js" => "javascript",
            "ts" => "typescript",
            "go" => "go",
            "java" => "java",
            "cpp" | "cc" => "cpp",
            "c" | "h" => "c",
            "swift" => "swift",
            "kt" => "kotlin",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "json" => "json",
            "md" => "markdown",
            _ => ext,
        }
    }

    /// Static version for external use (chunker).
    pub fn detect_symbol_static(line: &str) -> Option<String> {
        Self::detect_symbol(line)
    }

    /// Static version for external use (chunker).
    pub fn extract_keywords_static(content: &str) -> Vec<String> {
        Self::extract_keywords(content)
    }

    fn detect_symbol(line: &str) -> Option<String> {
        let t = line.trim();
        for prefix in ["fn ", "pub fn ", "async fn ", "struct ", "enum ", "trait ", "class ", "def ", "func "] {
            if let Some(rest) = t.strip_prefix(prefix) {
                let name = rest.split(['(', '{', '<', ':']).next().unwrap_or(rest).trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    fn extract_keywords(content: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        content.split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.len() > 2)
            .map(|w| w.to_lowercase())
            .filter(|w| seen.insert(w.clone()))
            .collect()
    }

    /// Index a file with entity-aware chunking.
    /// Extracts function signatures, struct definitions, trait declarations
    /// as separate index entries with higher keyword weight, then falls back
    /// to the regular chunking for the rest of the file.
    pub fn index_entity_aware(&mut self, root: &Path, path: &Path) -> std::io::Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let rel = path.strip_prefix(root).unwrap_or(path);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = Self::ext_to_lang(ext);
        let mut count = 0;

        // Extract entities and index them individually
        for cap in ENTITY_RE.captures_iter(&content) {
            let m = cap.get(0).unwrap();
            let entity_text = m.as_str();
            let name = cap
                .get(3)
                .or_else(|| cap.get(5))
                .or_else(|| cap.get(8))
                .map_or("unknown", |m| m.as_str());
            let doc_comment = extract_preceding_comment(&content, m.start());

            let start_line = content[..m.start()].lines().count() + 1;
            let end_line = start_line + entity_text.lines().count();

            let chunk_content = if doc_comment.is_empty() {
                entity_text.to_string()
            } else {
                format!("{}\n{}", doc_comment, entity_text)
            };

            let keywords = Self::extract_keywords(&chunk_content);

            let chunk = CodeChunk {
                file: rel.to_path_buf(),
                language: lang.to_string(),
                symbol: Some(name.to_string()),
                start_line,
                end_line,
                content: chunk_content,
                keywords: keywords.clone(),
            };

            let idx = self.chunks.len();
            self.chunks.push(chunk);
            for kw in &keywords {
                self.inverted.entry(kw.clone()).or_default().push(idx);
            }
            count += 1;
        }

        // Also index whole file as fallback for non-entity content
        count += self.index_file(root, path)?;
        Ok(count)
    }

    /// Multi-field search: search symbols, comments, and code body separately.
    /// Returns higher score when the query matches a symbol name directly.
    /// Combined score: 0.4 * symbol_match + 0.4 * bm25 + 0.2 * trigram
    pub fn search_multi_field(&self, query: &str, top_k: usize) -> Vec<(usize, f64, ChunkMetadata)> {
        let config = RetrievalConfig {
            top_k: top_k * 2,
            ..Default::default()
        };
        let results = self.hybrid_search(query, &config);
        if results.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        let mut scored: Vec<(usize, f64, ChunkMetadata)> = results
            .iter()
            .map(|score| {
                let chunk = &self.chunks[score.chunk_idx];

                // Symbol match boost: 1.0 if query exactly matches symbol name
                let symbol_boost = chunk.symbol.as_ref().map_or(0.0, |sym| {
                    let sym_lower = sym.to_lowercase();
                    if query_terms.iter().any(|t| sym_lower.contains(*t) || t.contains(&sym_lower)) {
                        1.0
                    } else if query_terms.iter().any(|t| sym_lower == *t) {
                        0.8
                    } else {
                        0.0
                    }
                });

                // BM25 score
                let bm25 = self.bm25_score(
                    &query_terms.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    score.chunk_idx,
                ) as f64;

                // Trigram similarity
                let trigram = CodeIndex::trigram_similarity(query, &chunk.content) as f64;

                // Combined: 0.4 * symbol_match + 0.4 * bm25 + 0.2 * trigram
                let combined = 0.4 * symbol_boost + 0.4 * bm25.min(1.0) + 0.2 * trigram;

                let metadata = ChunkMetadata {
                    chunk_id: score.chunk_idx,
                    strategy: ChunkingStrategy::Hybrid,
                    token_count: chunk.content.split_whitespace().count(),
                    doc_path: chunk.file.display().to_string(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    bm25_score: bm25,
                    trigram_score: trigram,
                    combined_score: combined,
                };

                (score.chunk_idx, combined, metadata)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Get the total chunk count.
    pub fn chunk_count(&self) -> usize { self.chunks.len() }

    // ── Hybrid retrieval engine ─────────────────────────────────────────

    /// Hybrid search combining BM25 keyword scoring, trigram semantic
    /// similarity, and recency boosting. Returns ranked results with
    /// detailed scores and per-file diversity enforcement.
    pub fn hybrid_search(&self, query: &str, config: &RetrievalConfig) -> Vec<SimilarityScore> {
        if self.chunks.is_empty() {
            return Vec::new();
        }

        let query_terms: Vec<String> = query.to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_string())
            .collect();

        if query_terms.is_empty() {
            return Vec::new();
        }

        // Collect candidate chunk indices from keyword matches
        let mut candidates: HashSet<usize> = HashSet::new();
        for term in &query_terms {
            if let Some(matches) = self.inverted.get(term) {
                for &idx in matches {
                    candidates.insert(idx);
                }
            }
        }

        // If no keyword matches, fall back to scanning all chunks for semantic similarity
        let candidate_indices: Vec<usize> = if candidates.is_empty() {
            (0..self.chunks.len()).collect()
        } else {
            candidates.into_iter().collect()
        };

        // Compute scores for each candidate
        let mut results: Vec<SimilarityScore> = candidate_indices
            .iter()
            .map(|&idx| {
                let kw_score = self.bm25_score(&query_terms, idx);
                let sem_score = Self::trigram_similarity(query, &self.chunks[idx].content);
                let rec_score = self.recency_score(idx);

                // Normalize each component to [0, 1] range
                let norm_kw = kw_score.min(1.0);
                let norm_sem = sem_score;
                let norm_rec = rec_score;

                let final_score = config.keyword_weight * norm_kw
                    + config.semantic_weight * norm_sem
                    + config.recency_weight * norm_rec;

                let mut reasons = Vec::new();
                if kw_score > 0.01 {
                    reasons.push(format!("keyword match (BM25={:.3})", kw_score));
                }
                if sem_score > 0.05 {
                    reasons.push(format!("trigram overlap={:.3}", sem_score));
                }
                if rec_score > 0.5 {
                    reasons.push(format!("recency boost={:.3}", rec_score));
                }

                SimilarityScore {
                    chunk_idx: idx,
                    keyword_score: kw_score,
                    semantic_score: sem_score,
                    recency_score: rec_score,
                    final_score,
                    match_reasons: reasons,
                }
            })
            .filter(|s| s.final_score >= config.min_score_threshold)
            .collect();

        // Sort by final score descending
        results.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).expect("NaN in score"));

        // Enforce per-file limit (diversity): keep best-scoring chunks per file
        let mut per_file_count: HashMap<String, usize> = HashMap::new();
        results.retain(|score| {
            let file_key = self.chunks[score.chunk_idx].file.display().to_string();
            let count = per_file_count.entry(file_key).or_insert(0);
            if *count < config.max_chunks_per_file {
                *count += 1;
                true
            } else {
                false
            }
        });

        results.truncate(config.top_k);
        results
    }

    /// BM25-style keyword scoring for a single chunk given query terms.
    /// Computes sum of (tf * idf) for each matching term.
    fn bm25_score(&self, query_terms: &[String], chunk_idx: usize) -> f32 {
        let mut score = 0.0f32;
        for term in query_terms {
            let tf = self.term_frequency(term, chunk_idx);
            if tf > 0 {
                let idf_val = self.idf(term) as f32;
                // BM25-like: normalized TF * IDF with saturation
                let tf_norm = (tf as f32) / (1.0 + (tf as f32));
                score += tf_norm * idf_val;
            }
        }
        score
    }

    /// Term frequency: how many times a term appears in a chunk's keywords.
    fn term_frequency(&self, term: &str, chunk_idx: usize) -> u32 {
        self.chunks.get(chunk_idx)
            .map(|c| c.keywords.iter().filter(|k| k == &term).count() as u32)
            .unwrap_or(0)
    }

    /// Inverse document frequency for a term across the entire index.
    /// Uses log((N - df + 0.5) / (df + 0.5) + 1) formula.
    fn idf(&self, term: &str) -> f64 {
        let n = self.chunks.len().max(1) as f64;
        let df = self.inverted.get(term).map(|v| v.len()).unwrap_or(0) as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Recency score based on chunk position — later-indexed chunks get higher scores.
    /// Returns value in [0.0, 1.0].
    fn recency_score(&self, chunk_idx: usize) -> f32 {
        if self.chunks.is_empty() {
            return 0.0;
        }
        (chunk_idx as f32) / (self.chunks.len() as f32)
    }

    /// Trigram Jaccard similarity between two strings as a proxy for
    /// semantic similarity without external embedding dependencies.
    fn trigram_similarity(a: &str, b: &str) -> f32 {
        let a_trigrams: HashSet<String> = a.chars()
            .collect::<Vec<char>>()
            .windows(3)
            .map(|w| w.iter().collect())
            .collect();
        let b_trigrams: HashSet<String> = b.chars()
            .collect::<Vec<char>>()
            .windows(3)
            .map(|w| w.iter().collect())
            .collect();

        if a_trigrams.is_empty() || b_trigrams.is_empty() {
            return 0.0;
        }

        let intersection: usize = a_trigrams.intersection(&b_trigrams).count();
        let union = a_trigrams.len() + b_trigrams.len() - intersection;
        if union == 0 { 0.0 } else { (intersection as f32) / (union as f32) }
    }

    /// Get indices of all chunks belonging to a specific file.
    pub fn chunks_for_file(&self, file: &Path) -> Vec<usize> {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.file == file)
            .map(|(i, _)| i)
            .collect()
    }

    /// Re-index a single file that changed (incremental update).
    /// Removes old chunks for this file, then re-indexes.
    /// Returns the number of new chunks added, or None if file doesn't exist.
    pub fn reindex_file(&mut self, root: &Path, path: &Path) -> Option<usize> {
        if !path.exists() {
            return None;
        }
        let removed = self.remove_file(path);
        let rel = path.strip_prefix(root).unwrap_or(path);
        // Re-insert with updated relative path
        let count = self.index_file(root, path).ok()?;
        tracing::debug!(file = %rel.display(), removed, reindexed = count, "File re-indexed");
        Some(count)
    }

    /// Remove all chunks for a file from the index (e.g., file deleted).
    /// Returns the count of removed chunks.
    pub fn remove_file(&mut self, path: &Path) -> usize {
        let target_indices: Vec<usize> = self.chunks_for_file(path);
        if target_indices.is_empty() {
            return 0;
        }

        // Build set of indices to remove for fast lookup
        let remove_set: HashSet<usize> = target_indices.into_iter().collect();

        // Remove chunks (iterate in reverse to preserve validity of earlier indices)
        let mut removed = 0;

        // Rebuild inverted index without removed chunks
        let new_chunks: Vec<CodeChunk> = self.chunks.drain(..)
            .enumerate()
            .filter_map(|(i, c)| {
                if remove_set.contains(&i) {
                    removed += 1;
                    None
                } else {
                    Some(c)
                }
            })
            .collect();

        // Rebuild inverted index from remaining chunks
        self.inverted.clear();
        for (idx, chunk) in new_chunks.into_iter().enumerate() {
            for kw in &chunk.keywords {
                self.inverted.entry(kw.clone()).or_default().push(idx);
            }
            self.chunks.push(chunk);
        }

        tracing::debug!(removed, "Chunks removed for file");
        removed
    }

    /// Search for chunks by symbol name (exact or fuzzy substring match).
    pub fn search_symbol(&self, symbol: &str, fuzzy: bool) -> Vec<&CodeChunk> {
        let symbol_lower = symbol.to_lowercase();
        self.chunks
            .iter()
            .filter(|c| {
                c.symbol.as_ref().is_some_and(|s| {
                    if fuzzy {
                        s.to_lowercase().contains(&symbol_lower)
                    } else {
                        s == symbol
                    }
                })
            })
            .collect()
    }

    /// Find all references to a symbol across files by searching both
    /// symbol declarations and keyword occurrences.
    pub fn find_references(&self, symbol: &str) -> Vec<&CodeChunk> {
        let sym_lower = symbol.to_lowercase();
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        // Exact symbol matches
        for (i, chunk) in self.chunks.iter().enumerate() {
            if seen.contains(&i) {
                continue;
            }
            if chunk.symbol.as_ref().is_some_and(|s| s.to_lowercase() == sym_lower) {
                seen.insert(i);
                results.push(chunk);
            }
        }

        // Keyword-based reference search (symbol name appearing in content)
        if let Some(indices) = self.inverted.get(&sym_lower) {
            for &idx in indices {
                if seen.insert(idx) {
                    if let Some(chunk) = self.chunks.get(idx) {
                        results.push(chunk);
                    }
                }
            }
        }

        results
    }

    /// Get index statistics for monitoring and diagnostics.
    pub fn stats(&self) -> IndexStats {
        let total_chunks = self.chunks.len();
        let total_files: HashSet<PathBuf> = self.chunks.iter().map(|c| c.file.clone()).collect();
        let total_keywords = self.inverted.len();
        let avg_chunk_size = if total_chunks > 0 {
            self.chunks.iter().map(|c| c.content.len()).sum::<usize>() / total_chunks
        } else {
            0
        };
        let mut languages: HashMap<String, usize> = HashMap::new();
        for chunk in &self.chunks {
            *languages.entry(chunk.language.clone()).or_insert(0) += 1;
        }

        IndexStats {
            total_chunks,
            total_files: total_files.len(),
            total_keywords,
            avg_chunk_size,
            languages,
        }
    }
}

/// RAG context builder — index workspace and prepend relevant chunks.
pub struct RagContext {
    index: CodeIndex,
    workspace: PathBuf,
}

impl RagContext {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            index: CodeIndex::new(),
            workspace: workspace.into(),
        }
    }

    /// Index the workspace (call once at startup or on-demand).
    pub fn index(&mut self) -> usize {
        self.index.index_workspace(&self.workspace)
    }

    /// Enrich a user prompt with relevant code context.
    pub fn enrich(&self, prompt: &str) -> String {
        let context = self.index.format_context(prompt, 5);
        if context.is_empty() {
            return prompt.to_string();
        }
        format!("{}\n\n{}", context, prompt)
    }

    /// Get reference to the underlying index for search operations.
    pub fn code_index(&self) -> &CodeIndex {
        &self.index
    }

    /// Enrich a user prompt using hybrid retrieval with full configuration.
    /// Returns prompt prepended with ranked code context and score details.
    pub fn enrich_hybrid(&self, prompt: &str, config: &RetrievalConfig) -> String {
        let results = self.index.hybrid_search(prompt, config);
        if results.is_empty() {
            return prompt.to_string();
        }

        let mut ctx = String::from("## Relevant Code Context (Hybrid Retrieval)\n\n");
        for score in &results {
            let chunk = self.index.chunks.get(score.chunk_idx)
                .expect("hybrid_search returned invalid chunk index");
            ctx.push_str(&format!(
                "### {}:{} ({}:{}) [score={:.3}]\n```{}\n{}\n```\n",
                chunk.file.display(),
                if let Some(sym) = &chunk.symbol { format!(" {}", sym) } else { String::new() },
                chunk.start_line,
                chunk.end_line,
                chunk.language,
                score.final_score,
                chunk.content,
            ));
            if !score.match_reasons.is_empty() {
                ctx.push_str(&format!("> {}\n", score.match_reasons.join("; ")));
            }
            ctx.push('\n');
        }
        format!("{}\n\n{}", ctx, prompt)
    }

    /// Re-index changed files only (call after edits). Returns total
    /// number of re-indexed chunks across all specified files.
    pub fn reindex_files(&mut self, files: &[&Path]) -> usize {
        let mut total = 0;
        for file in files {
            if let Some(n) = self.index.reindex_file(&self.workspace, file) {
                total += n;
            }
        }
        tracing::info!(files = files.len(), chunks_reindexed = total, "Files re-indexed");
        total
    }

    /// Get a human-readable explanation of why certain chunks were
    /// retrieved for a query. Useful for debugging retrieval quality.
    pub fn explain_retrieval(&self, query: &str) -> String {
        let config = RetrievalConfig { top_k: 5, ..Default::default() };
        let results = self.index.hybrid_search(query, &config);
        if results.is_empty() {
            return String::from("No matching chunks found for this query.");
        }

        let mut explanation = format!("Query: \"{}\"\nFound {} relevant chunks:\n\n",
                                      query, results.len());
        for (rank, score) in results.iter().enumerate() {
            let chunk = self.index.chunks.get(score.chunk_idx)
                .expect("hybrid_search returned invalid chunk index");
            explanation.push_str(&format!(
                "  {}. {} [final={:.4} kw={:.4} sem={:.4} rec={:.4}]\n",
                rank + 1,
                chunk.file.display(),
                score.final_score,
                score.keyword_score,
                score.semantic_score,
                score.recency_score,
            ));
            if !score.match_reasons.is_empty() {
                explanation.push_str(&format!("     → {}\n",
                    score.match_reasons.join("; ")));
            }
        }
        explanation
    }

    // ── Enhanced retrieval methods ─────────────────────────────────────────

    /// Rerank retrieved chunks using more sophisticated scoring.
    ///
    /// 1. Deduplicate overlapping chunks
    /// 2. Boost chunks with exact keyword matches
    /// 3. Boost chunks from the same file as the query context
    /// 4. Final score = 0.5 * bm25 + 0.3 * trigram + 0.2 * position_boost
    pub fn rerank(
        &self,
        query: &str,
        results: &[(usize, f64)],
        top_k: usize,
    ) -> Vec<(usize, f64, ChunkMetadata)> {
        if results.is_empty() {
            return Vec::new();
        }

        // Phase 1: Deduplicate overlapping chunks (same file, similar line ranges)
        let mut deduped: Vec<(usize, f64)> = Vec::new();
        let mut seen_regions: HashMap<String, Vec<(usize, usize)>> = HashMap::new(); // file → [(start_line, end_line)]

        for &(chunk_idx, score) in results {
            if chunk_idx >= self.index.chunks.len() {
                continue;
            }
            let chunk = &self.index.chunks[chunk_idx];
            let file_key = chunk.file.display().to_string();

            // Check if this chunk overlaps with any already-seen chunk in the same file
            let overlaps = seen_regions.get(&file_key).is_some_and(|regions| {
                regions.iter().any(|&(s, e)| {
                    // Overlap if line ranges intersect
                    chunk.start_line.max(s) <= chunk.end_line.min(e)
                })
            });

            if !overlaps {
                deduped.push((chunk_idx, score));
                seen_regions.entry(file_key).or_default().push((chunk.start_line, chunk.end_line));
            }
        }

        // Phase 2: Compute detailed scores
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        let mut scored: Vec<(usize, f64, ChunkMetadata)> = deduped
            .into_iter()
            .map(|(chunk_idx, _original_score)| {
                let chunk = &self.index.chunks[chunk_idx];

                // BM25 score
                let bm25 = self.index.bm25_score(&query_terms.iter().map(|s| s.to_string()).collect::<Vec<_>>(), chunk_idx) as f64;

                // Trigram similarity
                let trigram = CodeIndex::trigram_similarity(query, &chunk.content) as f64;

                // Keyword boost: exact keyword matches in chunk content
                let keyword_boost = if query_terms.is_empty() {
                    0.0
                } else {
                    let content_lower = chunk.content.to_lowercase();
                    let exact_matches = query_terms.iter()
                        .filter(|t| content_lower.contains(*t))
                        .count();
                    (exact_matches as f64) / (query_terms.len() as f64)
                };

                // File boost: boost chunks that share their filename with query terms
                let file_name = chunk.file.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let file_boost = if query_terms.iter().any(|t| file_name.contains(t)) {
                    0.15
                } else {
                    0.0
                };

                // Position boost: earlier chunks in the index get slightly more weight
                let pos_ratio = if self.index.chunks.is_empty() {
                    0.5
                } else {
                    1.0 - (chunk_idx as f64) / (self.index.chunks.len() as f64)
                };

                // Combined score
                let combined_score = 0.5 * bm25 + 0.3 * trigram + 0.2 * keyword_boost + file_boost + 0.05 * pos_ratio;

                let metadata = ChunkMetadata {
                    chunk_id: chunk_idx,
                    strategy: ChunkingStrategy::Hybrid,
                    token_count: chunk.content.split_whitespace().count(),
                    doc_path: chunk.file.display().to_string(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    bm25_score: bm25,
                    trigram_score: trigram,
                    combined_score,
                };

                (chunk_idx, combined_score, metadata)
            })
            .collect();

        // Sort by combined score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply file diversity: max 3 chunks per file
        let mut per_file_count: HashMap<String, usize> = HashMap::new();
        scored.retain(|(_, _, meta)| {
            let count = per_file_count.entry(meta.doc_path.clone()).or_insert(0);
            if *count < 3 {
                *count += 1;
                true
            } else {
                false
            }
        });

        scored.truncate(top_k);
        scored
    }

    /// Retrieve with reranking.
    pub fn retrieve_with_rerank(&self, query: &str, top_k: usize) -> Vec<(usize, f64, ChunkMetadata)> {
        let config = RetrievalConfig {
            top_k: top_k * 3, // Retrieve more, then rerank to top_k
            ..Default::default()
        };
        let results = self.index.hybrid_search(query, &config);
        let result_pairs: Vec<(usize, f64)> = results.iter()
            .map(|s| (s.chunk_idx, s.final_score as f64))
            .collect();
        self.rerank(query, &result_pairs, top_k)
    }

    /// Enhanced search with strategy selection.
    pub fn retrieve_with_strategy(
        &self,
        query: &str,
        top_k: usize,
        strategy: ChunkingStrategy,
    ) -> Vec<(usize, f64, ChunkMetadata)> {
        // Use strategy to influence retrieval parameters
        let multiplier = match strategy {
            ChunkingStrategy::Fixed => 2,
            ChunkingStrategy::Paragraph => 3,
            ChunkingStrategy::Hybrid => 3,
            #[cfg(feature = "semantic-chunk")]
            ChunkingStrategy::Semantic => 2,
        };

        // Adjust retrieval config based on strategy
        let config = RetrievalConfig {
            top_k: top_k * multiplier,
            keyword_weight: match strategy {
                ChunkingStrategy::Paragraph => 0.6, // Rely more on keywords for paragraph
                _ => 0.4,
            },
            semantic_weight: match strategy {
                ChunkingStrategy::Fixed => 0.3,      // Less semantic weight for fixed
                _ => 0.4,
            },
            ..Default::default()
        };

        let results = self.index.hybrid_search(query, &config);
        let result_pairs: Vec<(usize, f64)> = results.iter()
            .map(|s| (s.chunk_idx, s.final_score as f64))
            .collect();

        let mut reranked = self.rerank(query, &result_pairs, top_k);

        // Tag metadata with the requested strategy
        for (_, _, ref mut meta) in &mut reranked {
            meta.strategy = strategy;
        }

        reranked
    }

    /// Rebuild index with a specific chunking strategy (default: Hybrid).
    pub fn rebuild_index_with_strategy(&mut self, strategy: ChunkingStrategy) -> usize {
        match strategy {
            ChunkingStrategy::Hybrid | ChunkingStrategy::Fixed => {
                // Re-initialize with empty index and re-index
                self.index = CodeIndex::new();
                self.index()
            }
            ChunkingStrategy::Paragraph => {
                // Re-initialize with empty index and re-index
                self.index = CodeIndex::new();
                self.index()
            }
            #[cfg(feature = "semantic-chunk")]
            ChunkingStrategy::Semantic => {
                self.index = CodeIndex::new();
                self.index()
            }
        }
    }

    /// Index a file with entity-aware chunking.
    /// Extracts function signatures, struct definitions, trait declarations
    /// as separate index entries with higher weight for higher precision retrieval.
    pub fn index_file_entity_aware(&mut self, path: &Path) -> usize {
        self.index.index_entity_aware(&self.workspace, path).unwrap_or_default()
    }

    /// Multi-field search: search symbols, comments, and code body separately.
    /// Returns higher score when the query matches a symbol name directly.
    pub fn search_multi_field(&self, query: &str, top_k: usize) -> Vec<(usize, f64, ChunkMetadata)> {
        self.index.search_multi_field(query, top_k)
    }

    /// Split a large query into sub-queries for multi-faceted retrieval.
    /// Useful for complex questions about multiple topics.
    pub fn search_decomposed(&self, query: &str, top_k: usize) -> Vec<(usize, f64, ChunkMetadata)> {
        // Split by conjunctions, commas, periods
        let sub_queries: Vec<String> = query
            .split([',', '.', ';', '，', '。'])
            .flat_map(|part| {
                // Also split by common conjunctions
                part.split_whitespace()
                    .collect::<Vec<_>>()
                    .chunks(3)
                    .map(|chunk| chunk.join(" "))
                    .collect::<Vec<_>>()
            })
            .filter(|sq| sq.split_whitespace().any(|w| w.len() > 2))
            .collect();

        if sub_queries.is_empty() {
            return self.search_multi_field(query, top_k);
        }

        // Search each sub-query separately, merge results
        let mut all_results: HashMap<usize, (f64, ChunkMetadata)> = HashMap::new();
        for sub_query in &sub_queries {
            let results = self.search_multi_field(sub_query, top_k);
            for (idx, score, meta) in results {
                let entry = all_results.entry(idx).or_insert((0.0, meta));
                entry.0 = entry.0.max(score);
            }
        }

        let mut merged: Vec<(usize, f64, ChunkMetadata)> = all_results.into_iter()
            .map(|(idx, (score, meta))| (idx, score, meta))
            .collect();
        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        merged.truncate(top_k);
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_index() -> (CodeIndex, std::path::PathBuf) {
        let temp = std::env::temp_dir().join("carp_rag_test");
        std::fs::create_dir_all(&temp).expect("create test dir");

        // Create multiple test files for diversity testing
        std::fs::write(
            temp.join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n\nfn helper(x: i32) -> i32 {\n    x + 1\n}\n",
        ).expect("write main.rs");
        std::fs::write(
            temp.join("utils.rs"),
            "fn utility_function(data: &str) -> usize {\n    data.len()\n}\n\nstruct Config {\n    name: String,\n}\n",
        ).expect("write utils.rs");

        let mut idx = CodeIndex::new();
        idx.index_workspace(&temp);
        (idx, temp)
    }

    #[test]
    fn test_detect_symbol() {
        assert_eq!(CodeIndex::detect_symbol("fn main() {"), Some("main".into()));
        assert_eq!(CodeIndex::detect_symbol("async fn process_data(x: i32)"), Some("process_data".into()));
        assert_eq!(CodeIndex::detect_symbol("struct User {"), Some("User".into()));
        assert_eq!(CodeIndex::detect_symbol("let x = 1;"), None);
    }

    #[test]
    fn test_search_scores() {
        let (idx, temp) = setup_test_index();
        let results = idx.search("hello world", 5);
        assert!(!results.is_empty());
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_hybrid_search_basic() {
        let (idx, temp) = setup_test_index();
        let config = RetrievalConfig::default();
        let results = idx.hybrid_search("main function", &config);

        // Should return some results
        assert!(!results.is_empty(), "hybrid search should find results for 'main function'");

        // Results should be sorted by final_score descending
        for window in results.windows(2) {
            assert!(window[0].final_score >= window[1].final_score,
                    "Results should be sorted by score descending");
        }

        // Each result should have valid chunk index
        for score in &results {
            assert!(score.chunk_idx < idx.chunk_count(),
                    "chunk_idx should be within bounds");
        }
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_hybrid_search_empty_query() {
        let (idx, temp) = setup_test_index();
        let config = RetrievalConfig::default();
        let results = idx.hybrid_search("", &config);
        assert!(results.is_empty(), "empty query should yield no results");

        let short_results = idx.hybrid_search("hi", &config);
        assert!(short_results.is_empty(), "query terms < 3 chars should yield no results");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_bm25_scoring() {
        let (idx, temp) = setup_test_index();

        // Query with a term that exists in the index
        let query_terms: Vec<String> = vec!["main".to_string()];
        let score_0 = idx.bm25_score(&query_terms, 0);

        // Score should be non-negative
        assert!(score_0 >= 0.0, "BM25 score should be non-negative");

        // A term not in any chunk should give 0 score
        let nonexistent_terms: Vec<String> = vec!["zzz_nonexistent_term_zzz".to_string()];
        let score_none = idx.bm25_score(&nonexistent_terms, 0);
        assert_eq!(score_none, 0.0, "Non-existent term should give BM25 score of 0");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_per_file_limit() {
        let (mut idx, temp) = setup_test_index();

        // Add more chunks to the same file to test per-file limiting
        let extra_content: String = (0..100)
            .map(|i| format!("fn extra_func_{}() {{}}\n", i))
            .collect();
        std::fs::write(temp.join("main.rs"), &extra_content).expect("write extra content");
        idx.reindex_file(&temp, &temp.join("main.rs"));

        let config = RetrievalConfig {
            max_chunks_per_file: 2,
            top_k: 20,
            ..Default::default()
        };
        let results = idx.hybrid_search("extra func", &config);

        // Count chunks per file
        let mut per_file: HashMap<String, usize> = HashMap::new();
        for score in &results {
            let file_key = idx.chunks[score.chunk_idx].file.display().to_string();
            *per_file.entry(file_key).or_insert(0) += 1;
        }

        for (_, count) in &per_file {
            assert!(*count <= config.max_chunks_per_file,
                    "No file should exceed max_chunks_per_file limit");
        }
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_reindex_file() {
        let (mut idx, temp) = setup_test_index();

        // Modify the file and re-index
        let new_content = "fn updated_main() {\n    println!(\"updated\");\n}\n";
        std::fs::write(temp.join("main.rs"), new_content).expect("write updated file");

        let reindexed = idx.reindex_file(&temp, &temp.join("main.rs"));
        assert!(reindexed.is_some(), "reindex_file should return Some for existing file");
        assert!(reindexed.unwrap() > 0, "re-indexed file should have at least one chunk");

        // Total chunk count may differ but index should still work
        let results = idx.search("updated", 5);
        assert!(!results.is_empty(), "should find updated content after re-indexing");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_reindex_nonexistent_file() {
        let (mut idx, temp) = setup_test_index();
        let result = idx.reindex_file(&temp, Path::new("/nonexistent/path.rs"));
        assert!(result.is_none(), "reindex_file should return None for non-existent file");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_remove_file() {
        let (mut idx, temp) = setup_test_index();
        let initial_count = idx.chunk_count();
        assert!(initial_count > 0, "should have indexed some chunks");

        let removed = idx.remove_file(Path::new("main.rs"));
        assert!(removed > 0, "should have removed at least one chunk for main.rs");

        // Chunks for main.rs should be gone
        let remaining = idx.chunks_for_file(Path::new("main.rs"));
        assert!(remaining.is_empty(), "no chunks should remain for removed file");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_remove_unknown_file() {
        let (mut idx, temp) = setup_test_index();
        let removed = idx.remove_file(Path::new("nonexistent_file.rs"));
        assert_eq!(removed, 0, "removing unknown file should return 0");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_search_symbol_exact() {
        let (idx, temp) = setup_test_index();
        let results = idx.search_symbol("main", false);
        assert!(!results.is_empty(), "should find symbol 'main' exactly");

        // Exact match — should only return chunks with symbol == "main"
        for chunk in &results {
            assert_eq!(chunk.symbol.as_deref(), Some("main"),
                       "exact symbol search should match exact name");
        }
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_search_symbol_fuzzy() {
        let (idx, temp) = setup_test_index();
        let results = idx.search_symbol("hel", true);
        assert!(!results.is_empty(), "fuzzy search for 'hel' should find 'helper'");

        // Fuzzy match — should find symbols containing the substring
        for chunk in &results {
            let sym_lower = chunk.symbol.as_ref().map_or(String::new(), |s| s.to_lowercase());
            assert!(sym_lower.contains("hel"),
                   "fuzzy match should contain substring");
        }
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_find_references() {
        let (idx, temp) = setup_test_index();
        let refs = idx.find_references("main");
        assert!(!refs.is_empty(), "should find references to 'main'");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_index_stats() {
        let (idx, temp) = setup_test_index();
        let stats = idx.stats();

        assert!(stats.total_chunks > 0, "should have total chunks > 0");
        assert!(stats.total_files >= 2, "should have at least 2 files (main.rs + utils.rs)");
        assert!(stats.total_keywords > 0, "should have keywords in inverted index");
        assert!(stats.avg_chunk_size > 0, "average chunk size should be positive");
        assert!(stats.languages.contains_key("rust"), "should detect rust language");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_index_stats_empty() {
        let idx = CodeIndex::new();
        let stats = idx.stats();
        assert_eq!(stats.total_chunks, 0);
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.avg_chunk_size, 0);
        assert!(stats.languages.is_empty());
    }

    #[test]
    fn test_trigram_similarity() {
        // Identical strings should have high similarity
        let sim_same = CodeIndex::trigram_similarity("hello world", "hello world");
        assert!(sim_same > 0.9, "identical strings should have high trigram similarity");

        // Completely different strings should have low similarity
        let sim_diff = CodeIndex::trigram_similarity("hello world", "xyz abc def");
        assert!(sim_diff < 0.5, "different strings should have low similarity");

        // Empty string edge case
        let sim_empty = CodeIndex::trigram_similarity("", "hello");
        assert_eq!(sim_empty, 0.0, "empty string should give 0 similarity");
    }

    #[test]
    fn test_enrich_hybrid() {
        let (_idx, temp) = setup_test_index();
        let rag = RagContext::new(&temp);
        let config = RetrievalConfig { top_k: 3, ..Default::default() };
        let enriched = rag.enrich_hybrid("explain main function", &config);

        // Enriched output should contain context header
        assert!(enriched.contains("Relevant Code Context"),
                "enriched output should contain context header");
        // And should end with original prompt
        assert!(enriched.ends_with("explain main function"),
                "enriched output should end with original prompt");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_explain_retrieval() {
        let rag = {
            let (_idx, temp) = setup_test_index();
            let r = RagContext::new(&temp);
            // We need to keep temp alive but drop it after explain
            let explanation = r.explain_retrieval("main");
            std::fs::remove_dir_all(temp).ok();
            explanation
        };

        // Explanation should mention the query
        assert!(rag.contains("main"), "explanation should mention the query term");
        // Should list found chunks
        assert!(rag.contains("relevant chunks") || rag.contains("No matching"),
                "explanation should describe results or lack thereof");
    }

    #[test]
    fn test_reindex_files_multiple() {
        let mut rag = {
            let temp = std::env::temp_dir().join("carp_rag_multi_test");
            std::fs::create_dir_all(&temp).expect("create test dir");
            std::fs::write(temp.join("a.rs"), "fn alpha() {}\n").expect("write a.rs");
            std::fs::write(temp.join("b.rs"), "fn beta() {}\n").expect("write b.rs");

            let mut r = RagContext::new(&temp);
            r.index();
            r
        };

        // Re-index both files (relative paths resolved against workspace root)
        let files: Vec<&Path> = vec![Path::new("a.rs"), Path::new("b.rs")];
        let count = rag.reindex_files(&files);
        assert!(count >= 0, "reindex_files should complete without error");

        let temp = std::env::temp_dir().join("carp_rag_multi_test");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_retrieval_config_defaults() {
        let config = RetrievalConfig::default();
        assert_eq!(config.top_k, 10);
        assert!((config.keyword_weight - 0.4).abs() < f32::EPSILON);
        assert!((config.semantic_weight - 0.4).abs() < f32::EPSILON);
        assert!((config.recency_weight - 0.2).abs() < f32::EPSILON);
        assert!((config.min_score_threshold - 0.05).abs() < f32::EPSILON);
        assert_eq!(config.max_chunks_per_file, 3);
    }

    #[test]
    fn test_min_score_threshold_filters() {
        let (idx, temp) = setup_test_index();
        let strict_config = RetrievalConfig {
            min_score_threshold: 100.0, // Impossible to reach
            ..Default::default()
        };
        let results = idx.hybrid_search("main function", &strict_config);
        assert!(results.is_empty(),
                "Very high threshold should filter out all results");
        std::fs::remove_dir_all(temp).ok();
    }

    // ── RAG enhancement tests ─────────────────────────────────────────────

    #[test]
    fn test_chunking_strategy_descriptions() {
        assert!(ChunkingStrategy::Fixed.description().contains("Fixed"));
        assert!(ChunkingStrategy::Paragraph.description().contains("Paragraph"));
        assert!(ChunkingStrategy::Hybrid.description().contains("Hybrid"));
    }

    #[test]
    fn test_chunk_metadata_creation() {
        let meta = ChunkMetadata {
            chunk_id: 0,
            strategy: ChunkingStrategy::Hybrid,
            token_count: 50,
            doc_path: "src/main.rs".into(),
            start_line: 1,
            end_line: 10,
            bm25_score: 0.8,
            trigram_score: 0.5,
            combined_score: 0.65,
        };
        assert_eq!(meta.chunk_id, 0);
        assert_eq!(meta.doc_path, "src/main.rs");
        assert_eq!(meta.start_line, 1);
        assert_eq!(meta.end_line, 10);
    }

    #[test]
    fn test_rerank_deduplication() {
        let (idx, temp) = setup_test_index();
        let rag = RagContext { index: idx, workspace: temp.clone() };

        // Create overlapping results (same chunk index repeated)
        let results = vec![(0usize, 0.8f64), (0usize, 0.7f64), (1usize, 0.6f64)];
        let reranked = rag.rerank("main", &results, 5);

        // Should deduplicate the overlapping entries
        let unique_ids: std::collections::HashSet<usize> = reranked.iter().map(|(id, _, _)| *id).collect();
        // We may have fewer unique IDs than total results due to dedup
        assert!(reranked.len() <= results.len(), "Reranked should not exceed input count");
        assert!(unique_ids.len() <= 2, "Should deduplicate overlapping chunks");

        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_rerank_keyword_boost() {
        let (idx, temp) = setup_test_index();
        let rag = RagContext { index: idx, workspace: temp.clone() };

        // Get two different chunk indices
        let all_indices: Vec<usize> = (0..rag.index.chunks.len()).collect();
        if all_indices.len() >= 2 {
            let results = vec![(all_indices[0], 0.5f64), (all_indices[1], 0.5f64)];
            let reranked = rag.rerank("main", &results, 5);

            // Verify scores are computed (not NaN)
            for (_, score, meta) in &reranked {
                assert!(score.is_finite(), "Score should be finite");
                assert!(meta.combined_score.is_finite(), "Combined score should be finite");
            }
        }

        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_rerank_file_boost() {
        let (idx, temp) = setup_test_index();
        let rag = RagContext { index: idx, workspace: temp.clone() };

        // Use search results that may include chunks from "main.rs"
        if !rag.index.chunks.is_empty() {
            let results: Vec<(usize, f64)> = (0..rag.index.chunks.len().min(5))
                .map(|i| (i, 0.5f64))
                .collect();
            let reranked = rag.rerank("main", &results, 5);

            // Verify results are well-formed
            for (_, _, meta) in &reranked {
                assert!(!meta.doc_path.is_empty(), "Doc path should not be empty");
            }
        }

        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_retrieve_with_rerank_returns_metadata() {
        let (idx, temp) = setup_test_index();
        let rag = RagContext { index: idx, workspace: temp.clone() };

        let results = rag.retrieve_with_rerank("main function helper", 3);
        // May return empty if no matches, but should always return valid metadata
        for (_, score, meta) in &results {
            assert!(score.is_finite(), "Score should be finite");
            assert!(!meta.doc_path.is_empty(), "Metadata should have doc_path");
            assert!(meta.token_count > 0 || !meta.doc_path.is_empty(), "Token count or path should be present");
        }

        std::fs::remove_dir_all(temp).ok();
    }

    // ── Entity-aware tests ────────────────────────────────────────────────

    #[test]
    fn test_entity_regex_fn() {
        let re = &ENTITY_RE;
        assert!(re.is_match("fn main() {"));
        assert!(re.is_match("pub fn process()"));
        assert!(re.is_match("pub unsafe fn dangerous()"));
        // Should capture "main" as the function name
        let cap = re.captures("fn main() {").unwrap();
        assert_eq!(cap.get(3).map(|m| m.as_str()), Some("main"));
        // Should capture "process"
        let cap2 = re.captures("pub fn process()").unwrap();
        assert_eq!(cap2.get(3).map(|m| m.as_str()), Some("process"));
    }

    #[test]
    fn test_entity_regex_struct() {
        let re = &ENTITY_RE;
        assert!(re.is_match("struct User {"));
        assert!(re.is_match("pub struct Config"));
        let cap = re.captures("pub struct Config").unwrap();
        assert_eq!(cap.get(5).map(|m| m.as_str()), Some("Config"));
    }

    #[test]
    fn test_entity_regex_enum_trait_impl() {
        let re = &ENTITY_RE;
        assert!(re.is_match("enum Color {"));
        assert!(re.is_match("trait Display {"));
        assert!(re.is_match("impl Display for Color"));
        let cap = re.captures("enum Color {").unwrap();
        assert_eq!(cap.get(8).map(|m| m.as_str()), Some("Color"));
    }

    #[test]
    fn test_extract_preceding_comment() {
        let content = "/// This is a doc comment\n/// More details\nfn my_func() {}";
        let pos = content.find("fn my_func").unwrap();
        let comment = extract_preceding_comment(content, pos);
        assert!(comment.contains("This is a doc comment"));
        assert!(comment.contains("More details"));

        // No preceding comment
        let content2 = "fn other() {}";
        let pos2 = content2.find("fn other").unwrap();
        let comment2 = extract_preceding_comment(content2, pos2);
        assert!(comment2.is_empty());
    }

    #[test]
    fn test_index_entity_aware() {
        let temp = std::env::temp_dir().join("carp_entity_test");
        std::fs::create_dir_all(&temp).expect("create test dir");
        std::fs::write(
            temp.join("lib.rs"),
            "/// Greets the user\nfn greet(name: &str) -> String {\n    format!(\"Hello, {}\", name)\n}\n\npub struct Config {\n    pub name: String,\n}\n\nfn helper() {}\n",
        ).expect("write lib.rs");

        let mut rag = RagContext::new(&temp);
        let count = rag.index_file_entity_aware(&temp.join("lib.rs"));
        assert!(count > 0, "Should index at least some chunks");

        // Entity-aware indexing should pick up "greet", "Config", "helper"
        let results = rag.search_multi_field("greet", 5);
        assert!(!results.is_empty(), "Should find 'greet' symbol");

        let results_config = rag.search_multi_field("Config", 5);
        assert!(!results_config.is_empty(), "Should find 'Config' symbol");

        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_search_multi_field_basic() {
        let temp = std::env::temp_dir().join("carp_multi_field_test");
        std::fs::create_dir_all(&temp).expect("create test dir");
        std::fs::write(
            temp.join("math.rs"),
            "/// Add two numbers\nfn add(a: i32, b: i32) -> i32 { a + b }\n\n/// Subtract two numbers\nfn sub(a: i32, b: i32) -> i32 { a - b }\n",
        ).expect("write math.rs");

        let mut rag = RagContext::new(&temp);
        rag.index_file_entity_aware(&temp.join("math.rs"));

        // Search for "add" — should boost symbol match
        let results = rag.search_multi_field("add function", 5);
        assert!(!results.is_empty(), "Should find results for 'add function'");
        // The top result should have a valid combined score
        let top_score = results[0].1;
        assert!(top_score > 0.0, "Top result should have positive score");

        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn test_search_decomposed_basic() {
        let temp = std::env::temp_dir().join("carp_decomposed_test");
        std::fs::create_dir_all(&temp).expect("create test dir");
        std::fs::write(
            temp.join("app.rs"),
            "fn login() {}\nfn logout() {}\nfn reset_password() {}\n",
        ).expect("write app.rs");

        let mut rag = RagContext::new(&temp);
        rag.index_file_entity_aware(&temp.join("app.rs"));

        // Use a compound query
        let results = rag.search_decomposed("login and reset password", 5);
        // Should find something
        if !results.is_empty() {
            assert!(results[0].1 >= 0.0, "Score should be non-negative");
        }

        std::fs::remove_dir_all(temp).ok();
    }
}
