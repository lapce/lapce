//! @-file reference resolution — parse and inject file references in prompts.
//!
//! Claude Code / Cursor both use @-syntax to let users reference files
//! in their prompts. This module implements the same pattern for deepseek-carp.
//!
//! ```
//! User input: "Explain the bug in @src/main.rs and @src/config/"
//! Resolved:    "Explain the bug in [file: src/main.rs] <content>..."
//! ```

use std::path::{Path, PathBuf};

/// A resolved file/directory reference from user input.
#[derive(Debug, Clone)]
pub struct FileReference {
    /// Original @syntax in the prompt, e.g., "@src/main.rs"
    pub raw: String,
    /// Resolved absolute path.
    pub path: PathBuf,
    /// File content (for files) or directory listing (for dirs).
    pub content: String,
    /// Whether this is a file, directory, or failed resolution.
    pub kind: ReferenceKind,
    /// File size hint (for truncation decisions).
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceKind {
    File,
    Directory,
    /// Glob pattern matched multiple files.
    Glob(Vec<PathBuf>),
    NotFound,
    /// File too large to include (> 100KB).
    TooLarge,
}

/// Context resolver that parses @references from user prompts.
pub struct ReferenceResolver {
    /// Working directory for relative path resolution.
    workdir: PathBuf,
    /// Maximum file size to include in context (bytes).
    max_file_size: u64,
    /// Maximum directory listing entries.
    max_dir_entries: usize,
}

impl Default for ReferenceResolver {
    fn default() -> Self {
        Self {
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            max_file_size: 100 * 1024, // 100KB
            max_dir_entries: 50,
        }
    }
}

impl ReferenceResolver {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            ..Default::default()
        }
    }

    /// Parse all @references from a user prompt and return resolved FileReferences.
    pub fn resolve(&self, prompt: &str) -> Vec<FileReference> {
        let mut refs = Vec::new();
        let mut chars = prompt.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '@' {
                let _start = chars.clone().count();
                let mut ref_str = String::new();

                // Collect reference string (until whitespace or special char)
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || next == ',' || next == ')' || next == '}' {
                        break;
                    }
                    ref_str.push(next);
                    chars.next();
                }

                if !ref_str.is_empty() && !ref_str.starts_with('{') {
                    if let Some(resolved) = self.resolve_single(&ref_str) {
                        refs.push(resolved);
                    }
                }
            }
        }
        refs
    }

    /// Resolve a single reference string to a FileReference.
    fn resolve_single(&self, ref_str: &str) -> Option<FileReference> {
        let path = self.resolve_path(ref_str);

        // Handle glob patterns
        if ref_str.contains('*') || ref_str.contains('?') {
            return self.resolve_glob(path, ref_str);
        }

        match path.metadata() {
            Ok(meta) if meta.is_file() => self.resolve_file(&path, ref_str, meta.len()),
            Ok(meta) if meta.is_dir() => self.resolve_dir(&path, ref_str),
            _ => Some(FileReference {
                raw: format!("@{}", ref_str),
                path,
                content: String::new(),
                kind: ReferenceKind::NotFound,
                size_bytes: 0,
            }),
        }
    }

    fn resolve_file(&self, path: &Path, raw: &str, size: u64) -> Option<FileReference> {
        if size > self.max_file_size {
            return Some(FileReference {
                raw: format!("@{}", raw),
                path: path.to_path_buf(),
                content: format!("[File too large: {}KB, max {}KB]", size / 1024, self.max_file_size / 1024),
                kind: ReferenceKind::TooLarge,
                size_bytes: size,
            });
        }

        let content = std::fs::read_to_string(path).unwrap_or_default();
        Some(FileReference {
            raw: format!("@{}", raw),
            path: path.to_path_buf(),
            content,
            kind: ReferenceKind::File,
            size_bytes: size,
        })
    }

    fn resolve_dir(&self, path: &Path, raw: &str) -> Option<FileReference> {
        let mut listing = String::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut count = 0;
            for entry in entries.flatten() {
                if count >= self.max_dir_entries {
                    listing.push_str(&format!("  ... and more ({} entries max)\n", self.max_dir_entries));
                    break;
                }
                let ftype = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    "/"
                } else {
                    ""
                };
                listing.push_str(&format!(
                    "  {}{}\n",
                    entry.file_name().to_string_lossy(),
                    ftype
                ));
                count += 1;
            }
        }

        Some(FileReference {
            raw: format!("@{}", raw),
            path: path.to_path_buf(),
            content: listing,
            kind: ReferenceKind::Directory,
            size_bytes: 0,
        })
    }

    fn resolve_glob(&self, _pattern: PathBuf, raw: &str) -> Option<FileReference> {
        // Use the glob crate for pattern matching
        let pattern = raw.to_string();
        let mut matches = Vec::new();
        if let Ok(entries) = glob::glob(&pattern) {
            for entry in entries.flatten() {
                if matches.len() >= self.max_dir_entries {
                    break;
                }
                matches.push(entry);
            }
        }
        Some(FileReference {
            raw: format!("@{}", raw),
            path: PathBuf::from(raw),
            content: matches.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            kind: ReferenceKind::Glob(matches),
            size_bytes: 0,
        })
    }

    fn resolve_path(&self, path_str: &str) -> PathBuf {
        let p = Path::new(path_str);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workdir.join(p)
        }
    }
}

/// Convenience: resolve @references in prompt and inject file contents.
pub fn resolve_references(prompt: &str, workdir: Option<&Path>) -> (String, Vec<FileReference>) {
    let resolver = if let Some(wd) = workdir {
        ReferenceResolver::new(wd.to_path_buf())
    } else {
        ReferenceResolver::default()
    };

    let refs = resolver.resolve(prompt);
    if refs.is_empty() {
        return (prompt.to_string(), refs);
    }

    let mut enriched = prompt.to_string();
    for r in &refs {
        if r.kind == ReferenceKind::NotFound || r.kind == ReferenceKind::TooLarge {
            enriched.push_str(&format!("\n[{}: {}]", r.raw, r.content));
        } else {
            enriched.push_str(&format!("\n[{}]\n```\n{}\n```", r.raw, r.content));
        }
    }

    (enriched, refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_at_references() {
        let resolver = ReferenceResolver::default();
        let refs = resolver.resolve("Look at @src/main.rs and @Cargo.toml");
        assert!(!refs.is_empty());
        let names: Vec<&str> = refs.iter().map(|r| r.raw.as_str()).collect();
        assert!(names.contains(&"@src/main.rs"));
        assert!(names.contains(&"@Cargo.toml"));
    }

    #[test]
    fn test_no_references() {
        let resolver = ReferenceResolver::default();
        let refs = resolver.resolve("Just a normal question");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_resolve_references_enriches_prompt() {
        // This test requires actual files to exist
        let (result, refs) = resolve_references("Check @Cargo.toml", None);
        assert!(result.contains("Cargo.toml"));
        assert!(!refs.is_empty());
    }
}
