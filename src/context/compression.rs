//! Smart Context Compression - Importance scoring and selective retention.
//!
//! This module provides intelligent context compression that:
//! - Scores context pieces by importance
//! - Selectively retains high-importance pieces
//! - Compresses low-importance pieces
//! - Maintains semantic coherence

use std::collections::HashMap;
use std::path::PathBuf;

/// Importance score for a context piece.
#[derive(Debug, Clone)]
pub struct ImportanceScore {
    /// Base score (0.0 - 1.0)
    pub base: f32,
    /// Recency boost (more recent = higher)
    pub recency: f32,
    /// Reference count boost (more referenced = higher)
    pub references: f32,
    /// Semantic relevance boost
    pub relevance: f32,
    /// Final weighted score
    pub total: f32,
}

impl ImportanceScore {
    pub fn new(base: f32) -> Self {
        Self {
            base,
            recency: 0.0,
            references: 0.0,
            relevance: 0.0,
            total: base,
        }
    }

    /// Calculate final weighted score.
    pub fn finalize(&mut self) {
        self.total = self.base * 0.4 + self.recency * 0.2 + self.references * 0.2 + self.relevance * 0.2;
    }
}

/// A piece of context with its importance score.
#[derive(Debug, Clone)]
pub struct ContextPiece {
    /// Unique identifier.
    pub id: String,
    /// Content text.
    pub content: String,
    /// Importance score.
    pub score: ImportanceScore,
    /// Source file (if applicable).
    pub source: Option<PathBuf>,
    /// Type of context.
    pub context_type: ContextType,
    /// When this piece was created.
    pub timestamp: u64,
    /// Original size in tokens.
    pub original_tokens: usize,
    /// Compressed size in tokens.
    pub compressed_tokens: Option<usize>,
}

/// Type of context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextType {
    /// Function definition.
    Function,
    /// Class definition.
    Class,
    /// Import statements.
    Import,
    /// Variable definition.
    Variable,
    /// Comment or documentation.
    Comment,
    /// Test code.
    Test,
    /// Configuration.
    Config,
    /// Regular code.
    Regular,
}

/// Smart context compressor.
pub struct ContextCompressor {
    /// Maximum tokens to retain.
    max_tokens: usize,
    /// Minimum importance score to keep.
    min_score: f32,
    /// Compression strategies per type.
    strategies: HashMap<ContextType, CompressAction>,
}

/// Compression strategy for different context types.
#[derive(Debug, Clone, Copy)]
pub enum CompressAction {
    /// Keep as-is, no compression.
    Keep,
    /// Summarize the content.
    Summarize,
    /// Extract key points only.
    Extract,
    /// Discard entirely.
    Discard,
}

impl ContextCompressor {
    pub fn new(max_tokens: usize) -> Self {
        let mut strategies = HashMap::new();
        strategies.insert(ContextType::Function, CompressAction::Keep);
        strategies.insert(ContextType::Class, CompressAction::Keep);
        strategies.insert(ContextType::Import, CompressAction::Keep);
        strategies.insert(ContextType::Variable, CompressAction::Summarize);
        strategies.insert(ContextType::Comment, CompressAction::Extract);
        strategies.insert(ContextType::Test, CompressAction::Discard);
        strategies.insert(ContextType::Config, CompressAction::Keep);
        strategies.insert(ContextType::Regular, CompressAction::Summarize);

        Self {
            max_tokens,
            min_score: 0.3,
            strategies,
        }
    }

    /// Compress a list of context pieces to fit within token budget.
    pub fn compress(&self, pieces: Vec<ContextPiece>) -> Vec<ContextPiece> {
        // Step 1: Score all pieces
        let mut scored: Vec<ContextPiece> = pieces.into_iter().map(|mut p| {
            p.score.finalize();
            p
        }).collect();

        // Step 2: Sort by importance (highest first)
        scored.sort_by(|a, b| b.score.total.partial_cmp(&a.score.total).expect("unwrap failed: compression.rs:137"));

        // Step 3: Select pieces to keep
        let mut total_tokens = 0;
        let mut kept = Vec::new();

        for mut piece in scored {
            let strategy = self.strategies.get(&piece.context_type).copied().unwrap_or(CompressAction::Summarize);

            match strategy {
                CompressAction::Keep => {
                    let tokens = piece.original_tokens;
                    if total_tokens + tokens <= self.max_tokens {
                        kept.push(piece);
                        total_tokens += tokens;
                    }
                }
                CompressAction::Summarize => {
                    let compressed = self.summarize(&piece);
                    let tokens = compressed.len() / 4; // Rough estimate
                    if total_tokens + tokens <= self.max_tokens && piece.score.total >= self.min_score {
                        piece.compressed_tokens = Some(tokens);
                        kept.push(piece);
                        total_tokens += tokens;
                    }
                }
                CompressAction::Extract => {
                    let extracted = self.extract(&piece);
                    let tokens = extracted.len() / 4;
                    if total_tokens + tokens <= self.max_tokens && piece.score.total >= self.min_score * 1.5 {
                        piece.content = extracted;
                        piece.compressed_tokens = Some(tokens);
                        kept.push(piece);
                        total_tokens += tokens;
                    }
                }
                CompressAction::Discard => {
                    // Skip low-importance pieces
                    let tokens = piece.original_tokens;
                    if piece.score.total >= 0.7 && total_tokens + tokens <= self.max_tokens {
                        kept.push(piece);
                        total_tokens += tokens;
                    }
                }
            }
        }

        // Step 4: Sort by original order (preserve context flow)
        kept.sort_by_key(|a| a.timestamp);

        kept
    }

    /// Summarize a context piece 鈥?true summary via TF-IDF top-k sentence picking.
    ///
    /// Picks the k most "central" sentences (by TF-IDF term overlap with the rest of
    /// the piece) and emits them as a numbered, length-capped summary.
    fn summarize(&self, piece: &ContextPiece) -> String {
        let sent = split_sentences(&piece.content);
        if sent.is_empty() {
            return String::new();
        }
        if sent.len() <= 2 {
            return piece.content.clone();
        }

        let k = 3.min(sent.len() / 2);
        let ranked = rank_sentences_tfidf(&sent);
        let mut picked: Vec<usize> = ranked.into_iter().take(k).map(|(idx, _)| idx).collect();
        picked.sort_unstable();

        let stops: &[char] = &['.', '!', '?', '\u{3002}', '\u{FF01}', '\u{FF1F}', ';', '\u{FF1B}'];
        let mut out = String::from("[Summary]\n");
        for i in picked {
            let trimmed: String = sent[i].trim().chars().take_while(|c| !stops.contains(c)).collect();
            let cleaned = trimmed.trim();
            if !cleaned.is_empty() {
                out.push_str(cleaned);
                out.push('.');
            }
        }
        let cap = 400;
        if out.chars().count() > cap {
            let trimmed: String = out.chars().take(cap).collect();
            out = trimmed + "...";
        }
        out
    }

    /// Extract key points from a context piece (code-aware).
    fn extract(&self, piece: &ContextPiece) -> String {
        let lines: Vec<&str> = piece.content.lines().collect();
        let mut extracted = Vec::new();

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("class ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("trait ")
                || trimmed.starts_with("impl ")
                || trimmed.starts_with("///")
                || trimmed.starts_with("//!")
                || trimmed.starts_with("#[")
            {
                extracted.push(*line);
            }
        }

        if extracted.is_empty() {
            self.summarize(piece)
        } else {
            extracted.join("\n")
        }
    }

    /// Update importance scores based on recent interactions.
    pub fn boost_recency(&self, pieces: &mut [ContextPiece], current_time: u64) {
        for piece in pieces.iter_mut() {
            let age = current_time.saturating_sub(piece.timestamp);
            // Exponential decay based on age
            let recency_score = (1.0 - (age as f32 / 3600.0 / 24.0)).max(0.0).min(1.0);
            piece.score.recency = recency_score;
        }
    }

    /// Update importance scores based on reference count.
    pub fn boost_references(&self, pieces: &mut [ContextPiece], reference_counts: &HashMap<String, u32>) {
        let max_refs = reference_counts.values().max().copied().unwrap_or(1).max(1) as f32;
        for piece in pieces.iter_mut() {
            let refs = reference_counts.get(&piece.id).copied().unwrap_or(0) as f32;
            piece.score.references = refs / max_refs;
        }
    }

    /// Update importance scores based on semantic relevance.
    pub fn boost_relevance(&self, pieces: &mut [ContextPiece], query: &str) {
        let query_terms: Vec<&str> = query.split_whitespace().collect();
        for piece in pieces.iter_mut() {
            let mut relevance = 0.0;
            for term in &query_terms {
                if piece.content.to_lowercase().contains(&term.to_lowercase()) {
                    relevance += 1.0;
                }
            }
            piece.score.relevance = (relevance / query_terms.len() as f32).min(1.0);
        }
    }
}

/// Estimate token count for a string (rough approximation).
pub fn estimate_tokens(text: &str) -> usize {
    // Rough estimate: 1 token 鈮?4 characters for English
    text.len() / 4
}

/// Compress conversation history to fit within token budget.
pub fn compress_history(messages: Vec<String>, max_tokens: usize) -> Vec<String> {
    let compressor = ContextCompressor::new(max_tokens);
    let len = messages.len();
    let mut pieces: Vec<ContextPiece> = messages.into_iter().enumerate().map(|(i, content)| {
        ContextPiece {
            id: format!("msg_{}", i),
            content: content.clone(),
            score: ImportanceScore::new(0.5),
            source: None,
            context_type: ContextType::Regular,
            timestamp: i as u64,
            original_tokens: estimate_tokens(&content),
            compressed_tokens: None,
        }
    }).collect();

    // Boost recent messages
    compressor.boost_recency(&mut pieces, len as u64);

    let compressed = compressor.compress(pieces);
    compressed.into_iter().map(|p| p.content).collect()
}

// === Agent Loop Integration Helpers ===

/// Strategy configuration for different phases of conversation.
#[derive(Debug, Clone)]
pub struct CompressionProfile {
    /// For early conversation (turns 1-5): keep everything.
    pub early_max_tokens: usize,
    /// For mid conversation (turns 6-20): moderate compression.
    pub mid_max_tokens: usize,
    /// For long conversation (20+): aggressive compression.
    pub late_max_tokens: usize,
    /// Always preserve these context types regardless of score.
    pub always_keep: Vec<ContextType>,
}

impl Default for CompressionProfile {
    fn default() -> Self {
        Self {
            early_max_tokens: 100_000,  // Keep almost everything early on
            mid_max_tokens: 60_000,     // Start compressing
            late_max_tokens: 32_000,    // Aggressive for very long sessions
            always_keep: vec![
                ContextType::Function,
                ContextType::Class,
                ContextType::Import,
            ],
        }
    }
}

/// Smart compressor that adapts to conversation phase.
pub struct AdaptiveCompressor {
    base: ContextCompressor,
    profile: CompressionProfile,
    turn_count: u32,
    /// Token budget tracker (diminishing-returns detector).
    pub tracker: BudgetTracker,
}

/// Detects diminishing returns so the agent stops wasting tokens.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    /// How many times we've already asked for a budget continuation.
    pub continuation_count: u32,
    /// Global tokens used the previous turn.
    pub last_global_turn_tokens: usize,
    /// Global tokens used on the turn before that (so we can compute delta).
    pub last_last_global_turn_tokens: usize,
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self {
            continuation_count: 0,
            last_global_turn_tokens: 0,
            last_last_global_turn_tokens: 0,
        }
    }
}

impl BudgetTracker {
    const COMPLETION_THRESHOLD: f32 = 0.90;
    const DIMINISHING_DELTA: usize = 500;
    const MAX_CONTINUATIONS: u32 = 2;

    /// Update with the latest global token count after one full turn.
    pub fn record_turn(&mut self, global_turn_tokens: usize) {
        self.last_last_global_turn_tokens = self.last_global_turn_tokens;
        self.last_global_turn_tokens = global_turn_tokens;
    }

    /// Decision for whether to continue or stop.
    ///
    /// Returns `Ok` if continuing is justified, `Err(reason)` when we should stop.
    pub fn decide(&mut self, budget: usize) -> Result<BudgetDecision, &'static str> {
        if budget == 0 {
            return Err("budget zero");
        }
        let used = self.last_global_turn_tokens;
        let pct = (used as f32) / (budget as f32);
        let delta_now = used.saturating_sub(self.last_last_global_turn_tokens);

        // Diminishing returns: 2 consecutive deltas < 500 tokens with >= 3 total tools
        let is_diminishing =
            self.continuation_count >= 1 &&
            delta_now < Self::DIMINISHING_DELTA &&
            (used.saturating_sub(self.last_last_global_turn_tokens)) < Self::DIMINISHING_DELTA;

        // Hard stop: > 90% consumed AND at least one continuation already happened
        let hard_stop = pct >= Self::COMPLETION_THRESHOLD && self.continuation_count >= 1;

        if is_diminishing {
            self.continuation_count += 1;
            return Err("diminishing returns (token delta too low for 2+ turns)");
        }
        if hard_stop || self.continuation_count > Self::MAX_CONTINUATIONS {
            self.continuation_count += 1;
            return Err("budget exhausted");
        }

        self.continuation_count += 1;
        Ok(BudgetDecision {
            pct,
            used,
            budget,
            delta_last: delta_now,
        })
    }
}

/// Result of a single budget decision.
#[derive(Debug, Clone)]
pub struct BudgetDecision {
    pub pct: f32,
    pub used: usize,
    pub budget: usize,
    pub delta_last: usize,
}

impl AdaptiveCompressor {
    /// Create a new adaptive compressor with the given profile.
    pub fn new(profile: CompressionProfile) -> Self {
        let base = ContextCompressor::new(profile.early_max_tokens);
        Self {
            base,
            profile,
            turn_count: 0,
            tracker: BudgetTracker::default(),
        }
    }

    /// Compress based on current conversation phase.
    pub fn adapt_compress(&mut self, pieces: Vec<ContextPiece>) -> Vec<ContextPiece> {
        let budget = self.current_budget();
        let compressor = ContextCompressor::new(budget);
        compressor.compress(pieces)
    }

    /// Advance turn counter (call each turn).
    pub fn advance_turn(&mut self) {
        self.turn_count += 1;
    }

    /// Record the full-turn token usage and return a decision.
    pub fn check_budget(&mut self, global_tokens: usize) -> Result<BudgetDecision, &'static str> {
        let budget = self.current_budget();
        self.tracker.record_turn(global_tokens);
        self.tracker.decide(budget)
    }

    /// Get current turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    /// Get current token budget based on conversation phase.
    fn current_budget(&self) -> usize {
        if self.turn_count <= 5 {
            self.profile.early_max_tokens
        } else if self.turn_count <= 20 {
            self.profile.mid_max_tokens
        } else {
            self.profile.late_max_tokens
        }
    }
}

/// Conversation phase for adaptive compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Phase {
    /// Early conversation (turns 1-5): keep everything.
    Early,
    /// Mid conversation (turns 6-20): moderate compression.
    Mid,
    /// Late conversation (20+): aggressive compression.
    Late,
}

/// Collapsible tool categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// Read (file content) 鈥?fold to head+tail lines.
    Read,
    /// Search (grep/glob/lsp search) 鈥?fold to count + top hits.
    Search,
    /// List (ls/tree) 鈥?fold to count.
    List,
    /// Edit/write/apply 鈥?keep verbatim.
    Write,
    /// Bash command output 鈥?depends on command content.
    Bash,
    /// MCP tool 鈥?depends on server.
    Mcp,
    /// Not collapsible.
    Other,
}

fn classify_tool(name: &str, input_preview: &str) -> ToolClass {
    let n = name.to_lowercase();
    let i = input_preview.to_lowercase();
    if n.contains("read") || n.contains("cat") {
        return ToolClass::Read;
    }
    if n.contains("search") || n.contains("grep") || n.contains("glob")
        || n.contains("find") || n.contains("ripgrep") {
        return ToolClass::Search;
    }
    if n.contains("list") || n.contains("dir") || n.contains("tree") {
        return ToolClass::List;
    }
    if n.contains("write") || n.contains("edit") || n.contains("apply")
        || n.contains("replace") || n.contains("patch") {
        return ToolClass::Write;
    }
    if n.contains("bash") || n.contains("shell") || n.contains("exec") {
        // If bash command is a pure search (grep/head/cat | head), fold it.
        if i.contains("grep") || i.contains("rg ") || i.contains("find ")
            || i.contains("cat ") && i.contains(" | head") {
            return ToolClass::Search;
        }
        return ToolClass::Bash;
    }
    if n.contains("mcp") {
        return ToolClass::Mcp;
    }
    ToolClass::Other
}

/// Collapse a tool output for context budget.
///
/// Read 鈫?head(8) + tail(5) + "鈥?(N lines total)".
/// Search 鈫?N matches + top 5 hits.
/// List 鈫?N items + sample 3.
/// Write 鈫?keep (never collapsed).
/// Bash 鈫?if search-like: same as Search; else keep.
///
/// Returns (collapsed_text, was_collapsed).
pub fn collapse_tool_output(
    tool_name: &str,
    tool_input_preview: &str,
    output: &str,
) -> (String, bool) {
    let cls = classify_tool(tool_name, tool_input_preview);
    let was_collapsed = true;

    match cls {
        ToolClass::Write => return (output.to_string(), false),
        ToolClass::Other | ToolClass::Mcp => return (output.to_string(), false),
        ToolClass::Read => {
            let lines: Vec<&str> = output.lines().collect();
            let total = lines.len();
            if total <= 40 {
                return (output.to_string(), false);
            }
            let head = lines.iter().take(8).cloned().collect::<Vec<_>>().join("\n");
            let tail = lines.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join("\n");
            let size_hint = format!("鈥?({total} lines total)");
            return (format!("{head}\n{size_hint}\n{tail}"), was_collapsed);
        }
        ToolClass::Search => {
            let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
            let total = lines.len();
            if total <= 30 {
                return (output.to_string(), false);
            }
            let top = lines.iter().take(12).cloned().collect::<Vec<_>>().join("\n");
            let size_hint = format!("鈥?({total} matches total)");
            return (format!("{top}\n{size_hint}"), was_collapsed);
        }
        ToolClass::List => {
            let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
            let total = lines.len();
            if total <= 30 {
                return (output.to_string(), false);
            }
            let sample = lines.iter().take(10).cloned().collect::<Vec<_>>().join("\n");
            let size_hint = format!("鈥?({total} items total)");
            return (format!("{sample}\n{size_hint}"), was_collapsed);
        }
        ToolClass::Bash => {
            // Pure bash output: if short, keep; else head+tail.
            let lines: Vec<&str> = output.lines().collect();
            let total = lines.len();
            if total <= 50 {
                return (output.to_string(), false);
            }
            let head = lines.iter().take(10).cloned().collect::<Vec<_>>().join("\n");
            let tail = lines.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join("\n");
            let size_hint = format!("鈥?({total} lines total)");
            return (format!("{head}\n{size_hint}\n{tail}"), was_collapsed);
        }
    }
}

impl AdaptiveCompressor {
    /// Compress with quality assessment (how much information preserved).
    pub fn compress_with_quality(&self, context: &str, budget: usize) -> CompressedResult {
        let original_len = context.len();

        // Build pieces from context
        let pieces = vec![ContextPiece {
            id: "ctx_0".to_string(),
            content: context.to_string(),
            score: ImportanceScore::new(0.8),
            source: None,
            context_type: ContextType::Regular,
            timestamp: 0,
            original_tokens: estimate_tokens(context),
            compressed_tokens: None,
        }];

        // Use current phase budget
        let compressor = ContextCompressor::new(budget);
        let compressed_pieces = compressor.compress(pieces);
        let compressed_str = compressed_pieces
            .into_iter()
            .map(|p| p.content)
            .collect::<Vec<_>>()
            .join("\n");
        let compressed_len = compressed_str.len();

        // Quality metrics
        let compression_ratio = if original_len > 0 {
            compressed_len as f64 / original_len as f64
        } else {
            1.0
        };

        // Information preservation estimate
        let code_blocks_preserved = self.count_code_blocks(&compressed_str) as f64
            / self.count_code_blocks(context).max(1) as f64;

        let key_content_preserved = self.estimate_key_content_preserved(context, &compressed_str);

        let phase = if self.turn_count <= 5 {
            Phase::Early
        } else if self.turn_count <= 20 {
            Phase::Mid
        } else {
            Phase::Late
        };

        CompressedResult {
            original_size: original_len,
            compressed_size: compressed_len,
            compression_ratio,
            code_blocks_preserved,
            key_content_preserved,
            strategy_used: phase,
        }
    }

    /// Estimate how much key content was preserved.
    fn estimate_key_content_preserved(&self, original: &str, compressed: &str) -> f64 {
        let keywords = [
            "fn ", "struct ", "enum ", "trait ", "impl ", "pub ", "async ", "match ", "if ",
            "for ",
        ];
        let orig_count = keywords.iter().filter(|kw| original.contains(*kw)).count();
        let comp_count = keywords
            .iter()
            .filter(|kw| compressed.contains(*kw))
            .count();

        if orig_count > 0 {
            comp_count as f64 / orig_count as f64
        } else {
            1.0
        }
    }

    /// Count code blocks in text.
    fn count_code_blocks(&self, text: &str) -> usize {
        text.matches("```").count() / 2 + text.matches('\n').count() / 10
    }

    /// Adaptive strategy selection based on context characteristics.
    pub fn select_strategy(context: &str) -> CompressionStrategy {
        let has_code =
            context.contains("```") || context.contains("fn ") || context.contains("def ");
        let has_long_lines = context.lines().any(|l| l.len() > 200);
        let has_many_files = context.matches("file:").count() > 5;

        if has_code && !has_long_lines {
            CompressionStrategy::CodePreserving
        } else if has_many_files {
            CompressionStrategy::FileSummarized
        } else if context.len() > 50_000 {
            CompressionStrategy::Aggressive
        } else {
            CompressionStrategy::Conservative
        }
    }

    /// Compress with BM25 quality assessment.
    pub fn compress_with_bm25_quality(&mut self, context: &str, budget: usize) -> CompressedBm25Result {
        let start = std::time::Instant::now();
        let compressed = self.compress_context(context, budget);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let mut scorer = Bm25QualityScorer::new();
        let quality = scorer.score_compression_quality(context, &compressed);

        let phase = if self.turn_count <= 5 {
            Phase::Early
        } else if self.turn_count <= 20 {
            Phase::Mid
        } else {
            Phase::Late
        };

        CompressedBm25Result {
            original_size: context.len(),
            compressed_size: compressed.len(),
            compression_ratio: if !context.is_empty() {
                compressed.len() as f64 / context.len() as f64
            } else {
                1.0
            },
            bm25_quality_score: quality,
            latency_ms: elapsed_ms,
            strategy_used: phase,
            is_acceptable: quality > 0.6,
        }
    }

    /// Helper to compress a context string using the current phase budget.
    fn compress_context(&self, context: &str, budget: usize) -> String {
        let pieces = vec![ContextPiece {
            id: "ctx_0".to_string(),
            content: context.to_string(),
            score: ImportanceScore::new(0.8),
            source: None,
            context_type: ContextType::Regular,
            timestamp: 0,
            original_tokens: estimate_tokens(context),
            compressed_tokens: None,
        }];
        let compressor = ContextCompressor::new(budget);
        let compressed_pieces = compressor.compress(pieces);
        compressed_pieces
            .into_iter()
            .map(|p| p.content)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Strategy for adaptive compression selection.
#[derive(Debug, Clone)]
pub enum CompressionStrategy {
    /// Conservative compression, preserve most content.
    Conservative,
    /// Preserve code blocks and structure.
    CodePreserving,
    /// Summarize individual files.
    FileSummarized,
    /// Aggressive compression for large contexts.
    Aggressive,
}

/// Result of a compression with quality assessment.
#[derive(Debug, Clone)]
pub struct CompressedResult {
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f64,
    pub code_blocks_preserved: f64,
    pub key_content_preserved: f64,
    pub strategy_used: Phase,
}

// 鈹€鈹€鈹€ BM25-based Compression Quality Assessment 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

/// True if the character is a CJK ideograph (simplified/traditional/ext A/compat).
pub fn is_zh_char(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x20000..=0x2A6DF).contains(&cp)
}

/// Extract maximal CJK runs from text (split by non-CJK).
pub fn zh_runs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if is_zh_char(c) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split text into sentences (bilingual 鈥?handles both ASCII and CJK stops).
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        let is_stop = c == '.' || c == '!' || c == '?' || c == ';'
            || c == '\u{3002}' || c == '\u{FF01}' || c == '\u{FF1F}' || c == '\u{FF1B}';
        let newline = c == '\n';
        if (is_stop || newline) && cur.trim().chars().count() >= 6 {
            sentences.push(cur.trim().to_string());
            cur = String::new();
        } else if newline {
            cur.clear();
        }
    }
    let tail = cur.trim().to_string();
    if tail.chars().count() >= 6 {
        sentences.push(tail);
    }
    sentences
}

fn term_tokens_for(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for term in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() >= 2)
    {
        if term.chars().all(|c| !is_zh_char(c)) {
            out.push(term.to_string());
        }
    }
    for run in zh_runs(text) {
        let chars: Vec<char> = run.chars().collect();
        if chars.len() >= 2 {
            for w in chars.windows(2) {
                out.push(w.iter().collect());
            }
        } else if chars.len() == 1 {
            out.push(chars[0].to_string());
        }
    }
    out
}

fn rank_sentences_tfidf(sentences: &[String]) -> Vec<(usize, f64)> {
    let n = sentences.len();
    let mut tf: Vec<HashMap<String, f64>> = (0..n).map(|_| HashMap::new()).collect();
    let mut df: HashMap<String, f64> = HashMap::new();

    for (i, s) in sentences.iter().enumerate() {
        let tokens = term_tokens_for(s);
        let mut total = 0usize;
        for t in &tokens {
            *tf[i].entry(t.clone()).or_insert(0.0) += 1.0;
            total += 1;
        }
        for v in tf[i].values_mut() {
            if total > 0 {
                *v /= total as f64;
            }
        }
        for (term, _) in tf[i].iter() {
            *df.entry(term.clone()).or_insert(0.0) += 1.0;
        }
    }

    let idf: HashMap<String, f64> = df
        .into_iter()
        .map(|(t, d)| (t, ((n as f64 + 1.0) / (d + 1.0)).ln() + 1.0))
        .collect();

    let mut scores: Vec<(usize, f64)> = (0..n)
        .map(|i| {
            let mut score = 0.0;
            for (term, f) in &tf[i] {
                if let Some(idf_v) = idf.get(term) {
                    score += f * idf_v;
                }
            }
            (i, score)
        })
        .collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
}

/// BM25 parameters for term frequency weighting.
#[derive(Debug, Clone)]
pub struct Bm25Params {
    pub k1: f32, // Term frequency saturation (default: 1.5)
    pub b: f32,  // Length normalization (default: 0.75)
}

impl Default for Bm25Params {
    fn default() -> Self {
        Self { k1: 1.5, b: 0.75 }
    }
}

/// BM25 scorer for compression quality evaluation.
pub struct Bm25QualityScorer {
    params: Bm25Params,
    /// Average document length (for normalization)
    avg_doc_len: f32,
    /// Inverse document frequency cache
    idf_cache: HashMap<String, f32>,
}

impl Default for Bm25QualityScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl Bm25QualityScorer {
    pub fn new() -> Self {
        Self {
            params: Bm25Params::default(),
            avg_doc_len: 0.0,
            idf_cache: HashMap::new(),
        }
    }

    /// Score the quality of compressed text against original.
    /// Higher score = better information preservation.
    pub fn score_compression_quality(&mut self, original: &str, compressed: &str) -> f64 {
        if original.is_empty() {
            return 1.0;
        }
        if compressed.is_empty() {
            return 0.0;
        }

        let orig_terms = self.tokenize(original);
        let comp_terms = self.tokenize(compressed);

        // Update IDF from original
        self.update_idf(&orig_terms);

        // Calculate BM25 score for compressed vs original
        let mut score = 0.0f32;
        let avg_dl = orig_terms.len() as f32;

        for term in &comp_terms {
            let tf = comp_terms.iter().filter(|t| *t == term).count() as f32;
            let idf = self.idf(term);
            let doc_len = comp_terms.len() as f32;

            // BM25 formula
            let numerator = tf * (self.params.k1 + 1.0);
            let denominator =
                tf + self.params.k1 * (1.0 - self.params.b + self.params.b * doc_len / avg_dl);
            score += idf * numerator / denominator;
        }

        // Normalize by original score
        let orig_score = self.score_text(&orig_terms);
        if orig_score > 0.0 {
            (score / orig_score) as f64
        } else {
            0.5 // Fallback for empty texts
        }
    }

    /// Tokenize text into terms 鈥?bilingual (en + zh) aware.
    ///
    /// - English: word-based lowercase tokenization (existing behavior).
    /// - Chinese: char bigram sliding window so BM25 can match partial words
    ///   even without a proper Chinese segmenter.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let has_zh = text.chars().any(|c| is_zh_char(c));
        let mut out: Vec<String> = Vec::new();

        // English / alphanumeric words
        for term in text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() >= 2)
        {
            if term.chars().all(|c| !is_zh_char(c)) {
                out.push(term.to_string());
            }
        }

        // Chinese char bigrams (sliding window of size 2 over CJK run)
        if has_zh {
            for run in zh_runs(text) {
                let chars: Vec<char> = run.chars().collect();
                if chars.len() >= 2 {
                    for w in chars.windows(2) {
                        let bigram: String = w.iter().collect();
                        out.push(bigram);
                    }
                } else if chars.len() == 1 {
                    out.push(chars[0].to_string());
                }
            }
        }

        out
    }

    /// Score a single document against the IDF corpus.
    fn score_text(&self, terms: &[String]) -> f32 {
        let mut score = 0.0f32;
        let doc_len = terms.len() as f32;
        for term in terms {
            let tf = terms.iter().filter(|t| *t == term).count() as f32;
            let idf = self.idf(term);
            let numerator = tf * (self.params.k1 + 1.0);
            let denominator =
                tf + self.params.k1 * (1.0 - self.params.b + self.params.b * doc_len / doc_len);
            score += idf * numerator / denominator;
        }
        score
    }

    fn idf(&self, term: &str) -> f32 {
        self.idf_cache.get(term).copied().unwrap_or(1.0)
    }

    fn update_idf(&mut self, terms: &[String]) {
        let n = terms.len() as f32;
        for term in terms {
            let count = terms.iter().filter(|t| *t == term).count() as f32;
            self.idf_cache
                .insert(term.clone(), (n - count + 0.5).ln() / (count + 0.5).ln() + 1.0);
        }
    }
}

/// Result of compression with BM25 quality assessment.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompressedBm25Result {
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f64,
    pub bm25_quality_score: f64,
    pub latency_ms: u64,
    pub strategy_used: Phase,
    pub is_acceptable: bool,
}

/// Build context pieces from chat history for compression.
pub fn history_to_pieces(messages: &[String]) -> Vec<ContextPiece> {
    messages
        .iter()
        .enumerate()
        .map(|(i, content)| {
            // Detect if message looks like code vs natural language
            let ctx_type = detect_message_type(content);
            ContextPiece {
                id: format!("hist_{}", i),
                content: content.clone(),
                score: ImportanceScore::new(base_score_for_type(ctx_type)),
                source: None,
                context_type: ctx_type,
                timestamp: i as u64,
                original_tokens: estimate_tokens(content),
                compressed_tokens: None,
            }
        })
        .collect()
}

/// Calculate how much context budget is available given total window size.
pub fn available_budget(
    total_window: usize,
    system_size: usize,
    rag_size: usize,
) -> usize {
    total_window
        .saturating_sub(system_size)
        .saturating_sub(rag_size)
}

// --- Private helpers for agent-loop integration ---

/// Heuristically detect the type of a chat message.
fn detect_message_type(content: &str) -> ContextType {
    let lines: Vec<&str> = content.lines().collect();
    let code_like = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("fn ")
                || t.starts_with("pub ")
                || t.starts_with("def ")
                || t.starts_with("class ")
                || t.starts_with("import ")
                || t.starts_with("use ")
                || t.starts_with("//")
                || t.starts_with("#")
                || t.contains("{")
                || t.contains("(")
        })
        .count();

    // If more than half of non-empty lines look like code, treat as Regular (code)
    let non_empty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    if non_empty > 0 && code_like * 2 > non_empty {
        ContextType::Regular
    } else if content.contains("```") {
        ContextType::Comment // Mixed 鈥?treat as comment/docs
    } else {
        ContextType::Comment // Natural language
    }
}

/// Base importance score depending on detected context type.
fn base_score_for_type(ct: ContextType) -> f32 {
    match ct {
        ContextType::Function => 0.9,
        ContextType::Class => 0.85,
        ContextType::Import => 0.7,
        ContextType::Variable => 0.6,
        ContextType::Comment => 0.4,
        ContextType::Test => 0.3,
        ContextType::Config => 0.65,
        ContextType::Regular => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_importance_score() {
        let mut score = ImportanceScore::new(0.5);
        score.recency = 0.8;
        score.references = 0.6;
        score.relevance = 0.4;
        score.finalize();

        assert!(score.total > 0.4);
        assert!(score.total < 1.0);
    }

    #[test]
    fn test_context_compression() {
        let compressor = ContextCompressor::new(100);

        let pieces = vec![
            ContextPiece {
                id: "1".to_string(),
                content: "fn main() { println!(\"hello\"); }".to_string(),
                score: ImportanceScore::new(0.9),
                source: None,
                context_type: ContextType::Function,
                timestamp: 1,
                original_tokens: 10,
                compressed_tokens: None,
            },
            ContextPiece {
                id: "2".to_string(),
                content: "// TODO: fix this later".to_string(),
                score: ImportanceScore::new(0.3),
                source: None,
                context_type: ContextType::Comment,
                timestamp: 2,
                original_tokens: 5,
                compressed_tokens: None,
            },
        ];

        let compressed = compressor.compress(pieces);
        assert!(!compressed.is_empty());
    }

    #[test]
    fn test_token_estimate() {
        assert!(estimate_tokens("hello world") >= 1);
        assert!(estimate_tokens("fn main() {}") >= 3);
    }

    // --- Agent Loop Integration Helper Tests ---

    #[test]
    fn test_compression_profile_default() {
        let profile = CompressionProfile::default();
        assert_eq!(profile.early_max_tokens, 100_000);
        assert_eq!(profile.mid_max_tokens, 60_000);
        assert_eq!(profile.late_max_tokens, 32_000);
        assert!(profile.always_keep.contains(&ContextType::Function));
        assert!(profile.always_keep.contains(&ContextType::Class));
        assert!(profile.always_keep.contains(&ContextType::Import));
    }

    #[test]
    fn test_adaptive_compressor_early_phase() {
        let profile = CompressionProfile::default();
        let mut compressor = AdaptiveCompressor::new(profile);

        // Turn 0-5 should use early budget
        assert_eq!(compressor.turn_count(), 0);
        // adapt_compress with empty pieces should return empty
        let result = compressor.adapt_compress(vec![]);
        assert!(result.is_empty());

        compressor.advance_turn();
        assert_eq!(compressor.turn_count(), 1);
        // Still in early phase (<=5)
        compressor.advance_turn();
        compressor.advance_turn();
        compressor.advance_turn();
        compressor.advance_turn();
        assert_eq!(compressor.turn_count(), 5);
    }

    #[test]
    fn test_adaptive_compressor_mid_phase() {
        let profile = CompressionProfile::default();
        let mut compressor = AdaptiveCompressor::new(profile);

        for _ in 0..10 {
            compressor.advance_turn();
        }
        assert_eq!(compressor.turn_count(), 10);
        // Should be in mid phase 鈥?compress some pieces
        let pieces = vec![
            ContextPiece {
                id: "1".to_string(),
                content: "fn important() {}".to_string(),
                score: ImportanceScore::new(0.9),
                source: None,
                context_type: ContextType::Function,
                timestamp: 1,
                original_tokens: 10,
                compressed_tokens: None,
            },
            ContextPiece {
                id: "2".to_string(),
                content: "// TODO: fix this later".to_string(),
                score: ImportanceScore::new(0.2),
                source: None,
                context_type: ContextType::Comment,
                timestamp: 2,
                original_tokens: 200,
                compressed_tokens: None,
            },
        ];
        let result = compressor.adapt_compress(pieces);
        // High-score function should survive; low-score comment may be dropped
        assert!(!result.is_empty());
    }

    #[test]
    fn test_adaptive_compressor_late_phase() {
        let profile = CompressionProfile::default();
        let mut compressor = AdaptiveCompressor::new(profile);

        for _ in 0..25 {
            compressor.advance_turn();
        }
        assert!(compressor.turn_count() > 20); // Late phase
    }

    #[test]
    fn test_history_to_pieces_code_message() {
        let messages = vec![
            "fn main() {\n    println!(\"hello\");\n}".to_string(),
            "Please refactor this function".to_string(),
        ];
        let pieces = history_to_pieces(&messages);
        assert_eq!(pieces.len(), 2);
        // First message is code-like 鈫?Regular type
        assert_eq!(pieces[0].context_type, ContextType::Regular);
        // Second is natural language 鈫?Comment type
        assert_eq!(pieces[1].context_type, ContextType::Comment);
    }

    #[test]
    fn test_history_to_pieces_preserves_content() {
        let messages = vec!["hello world".to_string()];
        let pieces = history_to_pieces(&messages);
        assert_eq!(pieces[0].content, "hello world");
        assert_eq!(pieces[0].id, "hist_0");
    }

    #[test]
    fn test_available_budget_calculation() {
        let budget = available_budget(128_000, 2_000, 10_000);
        assert_eq!(budget, 116_000);

        // Edge case: over-budget inputs saturate at zero
        let zero_budget = available_budget(100, 200, 300);
        assert_eq!(zero_budget, 0);
    }

    // -- New tests: compress_with_quality, select_strategy, etc. --

    #[test]
    fn test_compress_with_quality() {
        let profile = CompressionProfile::default();
        let compressor = AdaptiveCompressor::new(profile);
        let context = "fn hello() { println!(\"world\"); }";

        let result = compressor.compress_with_quality(context, 1000);
        assert!(result.original_size > 0);
        assert!(result.compression_ratio > 0.0);
        assert!(result.code_blocks_preserved >= 0.0);
        assert!(result.key_content_preserved >= 0.0);
    }

    #[test]
    fn test_select_strategy_code() {
        let context = "```rust\nfn main() {}\n```";
        let strategy = AdaptiveCompressor::select_strategy(context);
        assert!(matches!(strategy, CompressionStrategy::CodePreserving));
    }

    #[test]
    fn test_select_strategy_long_lines() {
        let long_line = "a".repeat(300);
        let context = format!("file: src/main.rs\n{}\nfile: src/lib.rs\n", long_line);
        let strategy = AdaptiveCompressor::select_strategy(&context);
        // Long lines nullifies code detection, many files triggers FileSummarized
        assert!(matches!(strategy, CompressionStrategy::FileSummarized));
    }

    #[test]
    fn test_estimate_key_content_preserved() {
        let profile = CompressionProfile::default();
        let compressor = AdaptiveCompressor::new(profile);
        let original = "fn hello() {}\nstruct Foo {}\nenum Bar {}";
        let compressed = "fn hello() {}";

        let preserved = compressor.estimate_key_content_preserved(original, compressed);
        assert!(preserved > 0.0);
        assert!(preserved <= 1.0);
    }

    #[test]
    fn test_compression_ratio_basic() {
        let profile = CompressionProfile::default();
        let compressor = AdaptiveCompressor::new(profile);
        // A small context with very tight budget should compress aggressively
        let context = "fn a() {}\nfn b() {}\nfn c() {}";
        let result = compressor.compress_with_quality(context, 5);
        // Compression ratio should be <= 1.0
        assert!(result.compression_ratio <= 1.0);
        assert!(result.compression_ratio >= 0.0);
    }

    #[test]
    fn test_code_blocks_preserved() {
        let profile = CompressionProfile::default();
        let compressor = AdaptiveCompressor::new(profile);
        let text = "```rust\nfn main() {}\n```\n```python\ndef foo():\n    pass\n```";
        let count = compressor.count_code_blocks(text);
        assert!(count >= 2);
    }

    // 鈹€鈹€ BM25 Quality Assessment Tests 鈹€鈹€

    #[test]
    fn test_bm25_quality_scorer_tokenize() {
        let scorer = Bm25QualityScorer::new();
        let terms = scorer.tokenize("hello world test_fn");
        assert!(terms.contains(&"hello".to_string()));
        assert!(terms.contains(&"world".to_string()));
        assert!(terms.contains(&"test_fn".to_string()));
        // Single chars should be filtered out
        assert!(!terms.contains(&"a".to_string()));
        assert!(!terms.contains(&"I".to_string()));
    }

    #[test]
    fn test_bm25_quality_scorer_identical() {
        let mut scorer = Bm25QualityScorer::new();
        let text = "fn hello world fn test some code here";
        let score = scorer.score_compression_quality(text, text);
        // Identical texts should score very close to 1.0
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_bm25_quality_scorer_empty() {
        let mut scorer = Bm25QualityScorer::new();
        // Both empty
        assert_eq!(scorer.score_compression_quality("", ""), 1.0);
        // Original empty returns 1.0
        assert_eq!(scorer.score_compression_quality("", "content"), 1.0);
        // Compressed empty returns 0.0
        assert_eq!(scorer.score_compression_quality("original", ""), 0.0);
    }

    #[test]
    fn test_bm25_params_default() {
        let params = Bm25Params::default();
        assert_eq!(params.k1, 1.5);
        assert_eq!(params.b, 0.75);
    }

    #[test]
    fn test_compressed_bm25_result_serialize() {
        let result = CompressedBm25Result {
            original_size: 1000,
            compressed_size: 500,
            compression_ratio: 0.5,
            bm25_quality_score: 0.85,
            latency_ms: 10,
            strategy_used: Phase::Mid,
            is_acceptable: true,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("\"original_size\":1000"));
        assert!(json.contains("\"compressed_size\":500"));
        assert!(json.contains("\"bm25_quality_score\":0.85"));
        assert!(json.contains("\"is_acceptable\":true"));
        assert!(json.contains("\"strategy_used\":\"Mid\""));
    }

    #[test]
    fn test_compress_with_bm25_quality() {
        let profile = CompressionProfile::default();
        let mut compressor = AdaptiveCompressor::new(profile);
        let context = "fn hello() { println!(\"world\"); }";
        let result = compressor.compress_with_bm25_quality(context, 1000);
        assert!(result.original_size > 0);
        assert!(result.compression_ratio > 0.0);
        assert!(result.bm25_quality_score >= 0.0);
        assert!(result.latency_ms >= 0);
        // With large budget, quality should be acceptable
        assert!(result.is_acceptable);
    }
}
