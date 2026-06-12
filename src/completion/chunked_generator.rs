//! Chunked Code Generation - Streaming output with chunking for long code.
//!
//! This module provides:
//! - Long code chunking for streaming output
//! - Syntax-aware code boundaries
//! - Progress tracking and cancellation
//! - Memory-efficient processing

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// A chunk of generated code.
#[derive(Debug, Clone)]
pub struct CodeChunk {
    /// Chunk content.
    pub content: String,
    /// Chunk index (0-based).
    pub index: usize,
    /// Total chunks.
    pub total: usize,
    /// Whether this is the last chunk.
    pub is_last: bool,
    /// Syntax completeness (statement, function, block, etc.).
    pub completeness: ChunkCompleteness,
}

/// How complete this chunk is syntactically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkCompleteness {
    /// Incomplete - more content expected.
    Partial,
    /// Complete statement.
    Statement,
    /// Complete block (function, class, etc.).
    Block,
    /// Complete file.
    File,
}

/// Configuration for chunking.
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Maximum tokens per chunk.
    pub max_tokens: usize,
    /// Minimum chunk size in tokens.
    pub min_tokens: usize,
    /// Look ahead tokens for boundary detection.
    pub lookahead: usize,
    /// Enable syntax-aware chunking.
    pub syntax_aware: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: 512,
            min_tokens: 128,
            lookahead: 32,
            syntax_aware: true,
        }
    }
}

/// A chunk boundary found in code.
#[derive(Debug, Clone)]
pub struct ChunkBoundary {
    /// Position in the code.
    pub position: usize,
    /// Type of boundary.
    pub boundary_type: BoundaryType,
    /// Whether this is a safe break point.
    pub is_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryType {
    /// End of statement (; or newline).
    Statement,
    /// End of block (}).
    Block,
    /// End of function.
    Function,
    /// End of class.
    Class,
    /// End of line.
    Line,
    /// Natural chunk boundary (max_tokens reached).
    Natural,
}

/// Long code generator with streaming support.
pub struct ChunkedCodeGenerator {
    config: ChunkConfig,
}

impl ChunkedCodeGenerator {
    pub fn new(config: ChunkConfig) -> Self {
        Self { config }
    }

    /// Chunk generated code into smaller pieces.
    pub fn chunk(&self, code: &str) -> Vec<CodeChunk> {
        if self.config.syntax_aware {
            self.syntax_aware_chunk(code)
        } else {
            self.simple_chunk(code)
        }
    }

    /// Simple fixed-size chunking.
    fn simple_chunk(&self, code: &str) -> Vec<CodeChunk> {
        let tokens_per_chunk = self.config.max_tokens;
        let chars_per_token = 4; // Rough estimate
        let chars_per_chunk = tokens_per_chunk * chars_per_token;

        let mut chunks = Vec::new();
        let mut remaining = code;

        while !remaining.is_empty() {
            if remaining.len() <= chars_per_chunk {
                chunks.push(CodeChunk {
                    content: remaining.to_string(),
                    index: chunks.len(),
                    total: 0, // Will be set later
                    is_last: true,
                    completeness: ChunkCompleteness::File,
                });
                break;
            }

            let (chunk, rest) = remaining.split_at(chars_per_chunk);
            chunks.push(CodeChunk {
                content: chunk.to_string(),
                index: chunks.len(),
                total: 0,
                is_last: false,
                completeness: ChunkCompleteness::Partial,
            });
            remaining = rest;
        }

        // Update total
        let total = chunks.len();
        for chunk in &mut chunks {
            chunk.total = total;
        }

        chunks
    }

    /// Syntax-aware chunking that respects code structure.
    fn syntax_aware_chunk(&self, code: &str) -> Vec<CodeChunk> {
        let boundaries = self.find_boundaries(code);
        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let mut current_size = 0;

        for boundary in &boundaries {
            let chunk_size = boundary.position - chunk_start;
            let tokens = chunk_size / 4;

            // If adding this boundary would exceed max, start a new chunk
            if current_size + tokens > self.config.max_tokens && current_size > self.config.min_tokens {
                let content = &code[chunk_start..boundary.position];
                chunks.push(CodeChunk {
                    content: content.trim_end().to_string(),
                    index: chunks.len(),
                    total: 0,
                    is_last: false,
                    completeness: self.classify_chunk_end(content),
                });
                chunk_start = boundary.position;
                current_size = 0;
            }

            current_size += tokens;
        }

        // Add remaining code
        if chunk_start < code.len() {
            let content = code[chunk_start..].to_string();
            chunks.push(CodeChunk {
                content,
                index: chunks.len(),
                total: 0,
                is_last: true,
                completeness: ChunkCompleteness::File,
            });
        }

        // Update total
        let total = chunks.len();
        for chunk in &mut chunks {
            chunk.total = total;
        }

        chunks
    }

    /// Find all potential chunk boundaries in code.
    fn find_boundaries(&self, code: &str) -> Vec<ChunkBoundary> {
        let mut boundaries = Vec::new();
        let mut in_string = false;
        let mut in_block_comment = false;
        let mut paren_depth: i32 = 0;
        let mut brace_depth: i32 = 0;
        let mut bracket_depth: i32 = 0;

        let chars: Vec<char> = code.chars().collect();

        for (i, window) in chars.windows(2).enumerate() {
            let c1 = window[0];
            let c2 = window[1];

            // Handle string literals
            if c1 == '"' && !in_block_comment {
                in_string = !in_string;
            }

            // Handle block comments
            if c1 == '/' && c2 == '*' && !in_string {
                in_block_comment = true;
            }
            if c1 == '*' && c2 == '/' && in_block_comment {
                in_block_comment = false;
            }

            // Skip content in strings or comments
            if in_string || in_block_comment {
                continue;
            }

            // Track bracket depths
            match c1 {
                '(' | '{' | '[' => {
                    if c1 == '(' { paren_depth += 1; }
                    if c1 == '{' { brace_depth += 1; }
                    if c1 == '[' { bracket_depth += 1; }
                }
                ')' | '}' | ']' => {
                    if c1 == ')' { paren_depth = paren_depth.saturating_sub(1); }
                    if c1 == '}' { brace_depth = brace_depth.saturating_sub(1); }
                    if c1 == ']' { bracket_depth = bracket_depth.saturating_sub(1); }
                }
                _ => {}
            }

            // Find safe boundaries when brackets are balanced
            let is_balanced = paren_depth == 0 && brace_depth == 0 && bracket_depth == 0;

            // Statement endings
            if c1 == ';' && is_balanced {
                boundaries.push(ChunkBoundary {
                    position: i + 1,
                    boundary_type: BoundaryType::Statement,
                    is_safe: true,
                });
            }

            // Block endings
            if c1 == '}' && is_balanced {
                // Look ahead for newline or opening
                if i + 2 < chars.len() {
                    let next = chars[i + 1];
                    if next == '\n' || next == '\r' {
                        boundaries.push(ChunkBoundary {
                            position: i + 1,
                            boundary_type: BoundaryType::Block,
                            is_safe: true,
                        });
                    }
                }
            }

            // Line endings
            if c1 == '\n' && is_balanced {
                boundaries.push(ChunkBoundary {
                    position: i + 1,
                    boundary_type: BoundaryType::Line,
                    is_safe: paren_depth == 0,
                });
            }
        }

        boundaries
    }

    /// Classify how complete a chunk is.
    fn classify_chunk_end(&self, content: &str) -> ChunkCompleteness {
        let trimmed = content.trim();

        if trimmed.ends_with("}\n") || trimmed.ends_with("};") {
            ChunkCompleteness::Block
        } else if trimmed.ends_with(';') && !trimmed.contains("fn ") {
            ChunkCompleteness::Statement
        } else if trimmed.ends_with("fn ") || trimmed.ends_with("let ") {
            ChunkCompleteness::Partial
        } else {
            ChunkCompleteness::Partial
        }
    }

    /// Create a streaming channel for chunked output.
    pub fn create_streaming_channel(
        &self,
    ) -> (mpsc::Sender<CodeChunk>, mpsc::Receiver<CodeChunk>) {
        mpsc::channel(16)
    }
}

impl Default for ChunkedCodeGenerator {
    fn default() -> Self {
        Self::new(ChunkConfig::default())
    }
}

/// Streaming code generator that yields chunks.
pub struct StreamingGenerator {
    generator: ChunkedCodeGenerator,
    buffer: VecDeque<String>,
    cursor: usize,
}

impl StreamingGenerator {
    pub fn new() -> Self {
        Self {
            generator: ChunkedCodeGenerator::default(),
            buffer: VecDeque::new(),
            cursor: 0,
        }
    }

    /// Feed more content to the generator.
    pub fn feed(&mut self, content: &str) {
        self.buffer.push_back(content.to_string());
    }

    /// Get next chunk if available.
    pub fn next_chunk(&mut self) -> Option<CodeChunk> {
        // Try to get from completed chunks first
        if !self.buffer.is_empty() {
            let full_code: String = self.buffer.iter().cloned().collect();
            let chunks = self.generator.chunk(&full_code);

            if !chunks.is_empty() {
                // Return the first uncompleted chunk
                for chunk in chunks {
                    if !chunk.is_last {
                        // Remove returned content from buffer
                        if chunk.content.len() <= self.buffer.front()?.len() {
                            self.buffer.pop_front();
                        }
                        return Some(chunk);
                    }
                }
            }
        }
        None
    }

    /// Check if generation is complete.
    pub fn is_complete(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the current cursor position for progress tracking.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
}

impl Default for StreamingGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Progress tracker for long-running generations.
pub struct GenerationProgress {
    total_tokens: usize,
    generated_tokens: Arc<RwLock<usize>>,
    chunks_generated: Arc<RwLock<usize>>,
}

impl GenerationProgress {
    pub fn new(total_tokens: usize) -> Self {
        Self {
            total_tokens,
            generated_tokens: Arc::new(RwLock::new(0)),
            chunks_generated: Arc::new(RwLock::new(0)),
        }
    }

    /// Update progress.
    pub async fn update(&self, tokens: usize) {
        *self.generated_tokens.write().await += tokens;
        *self.chunks_generated.write().await += 1;
    }

    /// Get current progress percentage.
    pub async fn progress(&self) -> f32 {
        let generated = *self.generated_tokens.read().await;
        if self.total_tokens == 0 {
            0.0
        } else {
            (generated as f32 / self.total_tokens as f32) * 100.0
        }
    }

    /// Get ETA in seconds.
    pub async fn eta(&self, tokens_per_second: f32) -> f32 {
        let generated = *self.generated_tokens.read().await;
        let remaining = self.total_tokens.saturating_sub(generated);
        if tokens_per_second > 0.0 {
            remaining as f32 / tokens_per_second
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_chunking() {
        let generator = ChunkedCodeGenerator::new(ChunkConfig {
            max_tokens: 10,
            min_tokens: 5,
            lookahead: 5,
            syntax_aware: false,
        });

        let code = "fn main() { println!(\"hello\"); }";
        let chunks = generator.chunk(code);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_syntax_aware_chunking() {
        let generator = ChunkedCodeGenerator::default();
        let code = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let result = add(1, 2);
    println!("{}", result);
}
"#;
        let chunks = generator.chunk(code);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_boundary_detection() {
        let generator = ChunkedCodeGenerator::default();
        let boundaries = generator.find_boundaries("let x = 1;\nlet y = 2;");
        assert!(!boundaries.is_empty());
        assert!(boundaries.iter().any(|b| b.boundary_type == BoundaryType::Statement));
    }
}
