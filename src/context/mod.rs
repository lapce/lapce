//! Context gathering & @-file reference resolution.
//!
//! Inspired by Claude Code and Cursor's @-file system.
//! Parses @references in user prompts and injects file contents
//! into the conversation context automatically.
//!
//! ## Supported references
//!
//! | Syntax | Example | Resolves to |
//! |--------|---------|-------------|
//! | `@filename.rs` | `@main.rs` | Single file content |
//! | `@src/` | `@src/` | Directory listing |
//! | `@*.rs` | `@src/*.rs` | Glob pattern 鈫?file list |
//! | `@@symbol` | `@@main` | Recent session reference |

pub mod references;
pub mod rag;
pub mod chunker;
pub mod compression;
pub mod incremental;
pub mod conversation;
pub mod semantic_index;
pub mod semantic_index_v2;
pub mod context_manager;
pub mod persistent_index;

pub use references::{resolve_references, FileReference, ReferenceResolver};
pub use rag::{CodeIndex, CodeChunk, RagContext, ChunkingStrategy, ChunkMetadata};
pub use chunker::SemanticChunker;
pub use compression::{AdaptiveCompressor, CompressionProfile, ContextCompressor, ContextPiece, CompressAction, CompressedResult, CompressedBm25Result, ImportanceScore, ContextType, Phase, Bm25QualityScorer, ToolClass, BudgetTracker, BudgetDecision, collapse_tool_output, is_zh_char, zh_runs, split_sentences};
pub use incremental::{IncrementalContextManager, FileChange, ChangeType};
pub use semantic_index::{CodeSymbol, SymbolKind, Visibility, SearchResult};
pub use semantic_index_v2::{SemanticIndexV2, SymbolInfo, CodeRange, Reference, Definition};
pub use context_manager::{ContextManager, ContextSnapshot, BuildContext};
pub use persistent_index::{
    PersistentSemanticIndex, FileSystemWatcher, FileChangeEvent, FileChangeKind,
    PersistedIndex, PersistedEntry, Debouncer, DebouncedEvent, IndexWatcher,
    VectorStore, complexity_score,
};
#[cfg(feature = "sqlite-storage")]
pub use persistent_index::SqliteVectorStore;
