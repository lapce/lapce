//! Tree-sitter powered semantic code chunker.
//!
//! Replaces the naive 50-line window chunking with AST-aware splitting.
//! Chunks are aligned to function/struct/enum/impl boundaries,
//! dramatically improving RAG retrieval precision.
//!
//! ## Before (line-based)
//! ```text
//! Chunk 1: lines 1-50   — may cut a function in half
//! Chunk 2: lines 51-100 — may contain 3 unrelated functions
//! ```
//!
//! ## After (AST-based)
//! ```text
//! Chunk 1: fn main() { ... }          — 15 lines, self-contained
//! Chunk 2: struct User { ... }         — 20 lines, self-contained
//! Chunk 3: impl User { fn new() ... }  — 30 lines, self-contained
//! ```

use std::path::Path;
use super::rag::CodeChunk;

/// Tree-sitter powered semantic chunker.
pub struct SemanticChunker {
    /// Placeholder for feature-gated parser.
    _private: (),
}

#[cfg(not(feature = "semantic-chunk"))]
impl Default for SemanticChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticChunker {
    /// Create a new semantic chunker.
    #[cfg(feature = "semantic-chunk")]
    pub fn new() -> Self {
        Self { _private: () }
    }

    #[cfg(not(feature = "semantic-chunk"))]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Check if semantic chunking is available.
    pub fn available(&self) -> bool {
        cfg!(feature = "semantic-chunk")
    }

    /// Chunk a file into AST-aligned pieces.
    /// Falls back to simple line-based chunking for non-Rust files.
    pub fn chunk_file(
        &mut self,
        file_path: &Path,
        content: &str,
        language: &str,
    ) -> Vec<CodeChunk> {
        let rel = file_path;

        // Only use AST chunking for Rust when feature enabled
        #[cfg(feature = "semantic-chunk")]
        if language == "rust" && self.available() {
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&tree_sitter_rust::language()).ok();
            if let Some(tree) = parser.parse(content, None) {
                let root = tree.root_node();
                let mut chunks = Vec::new();
                let mut start = 0usize;
                let mut end = 0usize;
                let mut sym: Option<String> = None;
                let ast_nodes: &[&str] = &[
                    "function_item", "impl_item", "struct_item", "enum_item",
                    "trait_item", "module",
                ];
                Self::walk_node(root, content, ast_nodes, &mut chunks, &mut start, &mut end, &mut sym);
                if end > start {
                    let text = &content[start..end];
                    Self::add_chunk(&mut chunks, rel, text, "rust", start, end, sym);
                }
                return if chunks.is_empty() {
                    Self::chunk_lines(rel, content, "rust")
                } else {
                    chunks
                };
            }
        }
        let _ = language; // Used in the feature-gated path

        // Fallback: line-based chunking for other languages
        Self::chunk_lines(rel, content, language)
    }

    /// Walk the tree-sitter AST and collect semantic chunks.
    #[cfg(feature = "semantic-chunk")]
    fn walk_node(
        node: tree_sitter::Node,
        content: &str,
        ast_nodes: &[&str],
        chunks: &mut Vec<CodeChunk>,
        start: &mut usize,
        end: &mut usize,
        sym: &mut Option<String>,
    ) {
        let kind = node.kind();
        if ast_nodes.contains(&kind) && node.start_position().row > 0 {
            if *end > *start {
                let text = &content[*start..*end];
                Self::add_chunk(chunks, &std::path::Path::new(""), text, "rust", *start, *end, sym.clone());
            }
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                    *sym = Some(name.to_string());
                }
            }
            *start = node.start_byte();
        }
        *end = (*end).max(node.end_byte());
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                Self::walk_node(child, content, ast_nodes, chunks, start, end, sym);
            }
        }
    }

    /// Add a chunk to the results.
    pub fn add_chunk(
        chunks: &mut Vec<CodeChunk>,
        file_path: &Path,
        text: &str,
        language: &str,
        start_byte: usize,
        end_byte: usize,
        symbol: Option<String>,
    ) {
        let start_line = text[..start_byte.min(text.len())].lines().count().max(1);
        let chunk_content = &text[start_byte.min(text.len())..end_byte.min(text.len())];
        let keywords = super::rag::CodeIndex::extract_keywords_static(chunk_content);

        chunks.push(CodeChunk {
            file: file_path.to_path_buf(),
            language: language.to_string(),
            symbol,
            start_line,
            end_line: start_line + chunk_content.lines().count(),
            content: chunk_content.to_string(),
            keywords,
        });
    }

    /// Fallback: simple line-based chunking (original behavior).
    fn chunk_lines(file_path: &Path, content: &str, language: &str) -> Vec<CodeChunk> {
        let mut chunks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let chunk_size = 50;
        let mut i = 0;

        while i < lines.len() {
            let end = (i + chunk_size).min(lines.len());
            let chunk_content = lines[i..end].join("\n");
            let symbol = crate::context::rag::CodeIndex::detect_symbol_static(lines[i]);
            let keywords = crate::context::rag::CodeIndex::extract_keywords_static(&chunk_content);

            chunks.push(CodeChunk {
                file: file_path.to_path_buf(),
                language: language.to_string(),
                symbol,
                start_line: i + 1,
                end_line: end,
                content: chunk_content,
                keywords,
            });

            i = end;
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_lines_works() {
        let code = "fn a() {}\nfn b() {}\nfn c() {}\n".repeat(30);
        let chunks = SemanticChunker::chunk_lines(Path::new("test.rs"), &code, "rust");
        assert!(!chunks.is_empty());
        // Each chunk should have a start_line that makes sense
        assert_eq!(chunks[0].start_line, 1);
    }

    #[cfg(feature = "semantic-chunk")]
    #[test]
    fn test_semantic_chunk_rust() {
        let mut chunker = SemanticChunker::new();
        assert!(chunker.available());

        let code = r#"
/// Module docs
mod utils {
    pub fn helper() -> i32 { 42 }
}

struct Config {
    name: String,
}

impl Config {
    pub fn new(name: &str) -> Self {
        Config { name: name.to_string() }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }
}

fn main() {
    let config = Config::new("test");
    println!("{}", config.get_name());
}
"#;
        let chunks = chunker.chunk_file(Path::new("test.rs"), code, "rust");
        assert!(chunks.len() >= 3, "Should have at least: mod, struct, impl, fn chunks");
    }
}
