//! Code Style Adapter - Learns and applies project-specific code styles.
//!
//! This module provides:
//! - Automatic detection of project coding style
//! - Style profile generation from existing codebase
//! - Style-consistent code generation
//! - Configurable style preferences

use std::collections::HashMap;
use std::path::Path;

/// Indentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Spaces(usize),
    Tabs,
}

/// Naming convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingConvention {
    CamelCase,
    SnakeCase,
    PascalCase,
    KebabCase,
}

/// Quote style for strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    Double,
    Single,
}

/// Line ending style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    LF,
    CRLF,
    Auto,
}

/// A detected code style profile.
#[derive(Debug, Clone)]
pub struct StyleProfile {
    pub language: String,
    pub indent: IndentStyle,
    pub naming: NamingConventions,
    pub quote_style: QuoteStyle,
    pub line_ending: LineEnding,
    pub max_line_length: usize,
    pub blank_line_after_decls: bool,
    pub space_before_paren: bool,
    pub brace_style: BraceStyle,
    pub trailing_comma: bool,
}

#[derive(Debug, Clone)]
pub struct NamingConventions {
    pub variables: NamingConvention,
    pub functions: NamingConvention,
    pub types: NamingConvention,
    pub constants: NamingConvention,
    pub files: NamingConvention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraceStyle {
    KAndR,
    Allman,
    Othello,
}

#[derive(Debug, Clone)]
pub struct StyleAnalysis {
    pub file: std::path::PathBuf,
    pub indent_size: Option<usize>,
    pub uses_tabs: bool,
    pub uses_lf: bool,
    pub avg_line_length: f64,
    pub max_line_length: usize,
    pub comment_style: CommentStyle,
    pub has_semicolons: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentStyle {
    DoubleSlash,
    Hash,
    SlashStar,
}

/// Style detector that analyzes existing code.
pub struct StyleDetector;

impl StyleDetector {
    pub fn new() -> Self {
        Self
    }

    /// Analyze a single file.
    pub fn analyze_file(&self, path: &Path, content: &str) -> StyleAnalysis {
        let lines: Vec<&str> = content.lines().collect();

        let (indent_size, uses_tabs) = self.detect_indent(content);
        let uses_lf = !content.contains("\r\n");

        let line_lengths: Vec<usize> = lines.iter().map(|l| l.len()).collect();
        let avg_line_length = if line_lengths.is_empty() {
            0.0
        } else {
            line_lengths.iter().sum::<usize>() as f64 / line_lengths.len() as f64
        };
        let max_line_length = line_lengths.iter().max().copied().unwrap_or(0);

        let comment_style = self.detect_comment_style(content);
        let has_semicolons = content.contains(';');

        StyleAnalysis {
            file: path.to_path_buf(),
            indent_size,
            uses_tabs,
            uses_lf,
            avg_line_length,
            max_line_length,
            comment_style,
            has_semicolons,
        }
    }

    fn detect_indent(&self, content: &str) -> (Option<usize>, bool) {
        let lines: Vec<&str> = content.lines().collect();
        let mut space_counts: HashMap<usize, usize> = HashMap::new();
        let mut tab_count = 0;

        for line in lines {
            if line.is_empty() {
                continue;
            }

            let leading_spaces = line.len() - line.trim_start().len();
            let leading_tabs = line.len() - line.trim_start_matches('\t').len();

            if leading_tabs > 0 {
                tab_count += 1;
            } else if leading_spaces > 0 {
                *space_counts.entry(leading_spaces).or_insert(0) += 1;
            }
        }

        if tab_count > space_counts.values().sum::<usize>() {
            (None, true)
        } else {
            let most_common = space_counts.iter()
                .filter(|(size, _)| [2, 4, 8].contains(size))
                .max_by_key(|(_, count)| *count);

            if let Some((size, _)) = most_common {
                (Some(*size), false)
            } else {
                (Some(4), false)
            }
        }
    }

    fn detect_comment_style(&self, content: &str) -> CommentStyle {
        if content.contains("//") {
            CommentStyle::DoubleSlash
        } else if content.contains("/*") {
            CommentStyle::SlashStar
        } else if content.contains('#') {
            CommentStyle::Hash
        } else {
            CommentStyle::DoubleSlash
        }
    }

    /// Build a style profile from language.
    pub fn build_profile(&self, language: &str) -> StyleProfile {
        let naming = match language {
            "rust" | "go" | "python" => NamingConventions {
                variables: NamingConvention::SnakeCase,
                functions: NamingConvention::SnakeCase,
                types: NamingConvention::PascalCase,
                constants: NamingConvention::SnakeCase,
                files: NamingConvention::SnakeCase,
            },
            "java" | "javascript" | "typescript" => NamingConventions {
                variables: NamingConvention::CamelCase,
                functions: NamingConvention::CamelCase,
                types: NamingConvention::PascalCase,
                constants: NamingConvention::PascalCase,
                files: NamingConvention::PascalCase,
            },
            _ => NamingConventions {
                variables: NamingConvention::CamelCase,
                functions: NamingConvention::CamelCase,
                types: NamingConvention::PascalCase,
                constants: NamingConvention::PascalCase,
                files: NamingConvention::SnakeCase,
            },
        };

        let (indent, max_line) = match language {
            "python" => (IndentStyle::Spaces(4), 88),
            "go" => (IndentStyle::Tabs, 120),
            _ => (IndentStyle::Spaces(4), 100),
        };

        StyleProfile {
            language: language.to_string(),
            indent,
            naming,
            quote_style: QuoteStyle::Double,
            line_ending: LineEnding::LF,
            max_line_length: max_line,
            blank_line_after_decls: true,
            space_before_paren: false,
            brace_style: BraceStyle::KAndR,
            trailing_comma: true,
        }
    }
}

impl Default for StyleDetector {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StyleAdapter {
    profile: Option<StyleProfile>,
}

impl StyleAdapter {
    pub fn new() -> Self {
        Self { profile: None }
    }

    pub fn set_profile(&mut self, profile: StyleProfile) {
        self.profile = Some(profile);
    }

    pub fn get_profile(&self) -> Option<&StyleProfile> {
        self.profile.as_ref()
    }

    pub fn apply(&self, code: &str) -> String {
        if let Some(profile) = &self.profile {
            self.apply_with_profile(code, profile)
        } else {
            code.to_string()
        }
    }

    fn apply_with_profile(&self, code: &str, profile: &StyleProfile) -> String {
        let lines: Vec<String> = code.lines().map(|line| {
            match profile.indent {
                IndentStyle::Spaces(size) => {
                    let leading = line.len() - line.trim_start().len();
                    let indent_level = leading / size;
                    " ".repeat(indent_level * size).to_string() + line.trim_start()
                }
                IndentStyle::Tabs => {
                    let leading_tabs = line.len() - line.trim_start_matches('\t').len();
                    "\t".repeat(leading_tabs).to_string() + line.trim_start()
                }
            }
        }).collect();

        let joined = lines.join(match profile.line_ending {
            LineEnding::LF => "\n",
            LineEnding::CRLF => "\r\n",
            LineEnding::Auto => "\n",
        });

        if profile.quote_style == QuoteStyle::Double {
            joined.replace("'", "\"")
        } else {
            joined
        }
    }
}

impl Default for StyleAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indent_detection() {
        let detector = StyleDetector::new();
        let content = "fn main() {\n    let x = 1;\n}";
        let (indent_size, uses_tabs) = detector.detect_indent(content);
        assert_eq!(indent_size, Some(4));
        assert!(!uses_tabs);
    }

    #[test]
    fn test_style_profile() {
        let detector = StyleDetector::new();
        let profile = detector.build_profile("rust");
        assert_eq!(profile.language, "rust");
        assert!(matches!(profile.naming.variables, NamingConvention::SnakeCase));
    }
}
