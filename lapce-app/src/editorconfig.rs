//! EditorConfig support for Lapce.
//!
//! This module provides integration with `.editorconfig` files to automatically
//! configure editor settings on a per-file basis.

use std::path::Path;

use lapce_core::{indent::IndentStyle, line_ending::LineEnding};

/// Properties parsed from an `.editorconfig` file for a specific file.
#[derive(Debug, Clone, Default)]
pub struct EditorConfigProperties {
    /// The indent style (spaces or tabs).
    pub indent_style: Option<IndentStyle>,
    /// The number of columns used for each indentation level.
    pub indent_size: Option<usize>,
    /// The number of columns used to represent a tab character.
    pub tab_width: Option<usize>,
    /// The line ending style.
    pub end_of_line: Option<LineEnding>,
    /// Whether to ensure a final newline at the end of the file.
    pub insert_final_newline: Option<bool>,
    /// Whether to trim trailing whitespace.
    pub trim_trailing_whitespace: Option<bool>,
}

impl EditorConfigProperties {
    /// Get the effective indent style, considering tab_width and indent_size.
    pub fn effective_indent_style(&self) -> Option<IndentStyle> {
        if let Some(style) = self.indent_style {
            match style {
                IndentStyle::Tabs => Some(IndentStyle::Tabs),
                IndentStyle::Spaces(_) => {
                    // Use indent_size if specified, otherwise use the default from indent_style
                    let size = self.indent_size.unwrap_or(4);
                    Some(IndentStyle::Spaces(size as u8))
                }
            }
        } else {
            None
        }
    }

    /// Get the effective tab width.
    pub fn effective_tab_width(&self) -> Option<usize> {
        self.tab_width.or(self.indent_size)
    }
}

/// Get EditorConfig properties for a given file path.
///
/// This function searches for `.editorconfig` files starting from the file's
/// directory and going up to the root, merging properties according to the
/// EditorConfig specification.
pub fn get_properties(path: &Path) -> EditorConfigProperties {
    let mut props = EditorConfigProperties::default();

    // Use ec4rs to get properties for this file
    let Ok(ec_props) = ec4rs::properties_of(path) else {
        return props;
    };

    // Parse indent_style
    if let Ok(style) = ec_props.get::<ec4rs::property::IndentStyle>() {
        props.indent_style = Some(match style {
            ec4rs::property::IndentStyle::Tabs => IndentStyle::Tabs,
            ec4rs::property::IndentStyle::Spaces => {
                // Default to 4 spaces, will be overridden by indent_size if present
                IndentStyle::Spaces(4)
            }
        });
    }

    // Parse indent_size
    if let Ok(size) = ec_props.get::<ec4rs::property::IndentSize>() {
        match size {
            ec4rs::property::IndentSize::Value(n) => {
                props.indent_size = Some(n);
            }
            ec4rs::property::IndentSize::UseTabWidth => {
                // Will use tab_width when available
            }
        }
    }

    // Parse tab_width
    if let Ok(ec4rs::property::TabWidth::Value(width)) =
        ec_props.get::<ec4rs::property::TabWidth>()
    {
        props.tab_width = Some(width);
    }

    // Parse end_of_line
    if let Ok(eol) = ec_props.get::<ec4rs::property::EndOfLine>() {
        props.end_of_line = Some(match eol {
            ec4rs::property::EndOfLine::Lf => LineEnding::Lf,
            ec4rs::property::EndOfLine::CrLf => LineEnding::CrLf,
            // CR alone is not supported by Lapce, default to Lf
            ec4rs::property::EndOfLine::Cr => LineEnding::Lf,
        });
    }

    // Parse insert_final_newline
    if let Ok(ec4rs::property::FinalNewline::Value(val)) =
        ec_props.get::<ec4rs::property::FinalNewline>()
    {
        props.insert_final_newline = Some(val);
    }

    // Parse trim_trailing_whitespace
    if let Ok(ec4rs::property::TrimTrailingWs::Value(val)) =
        ec_props.get::<ec4rs::property::TrimTrailingWs>()
    {
        props.trim_trailing_whitespace = Some(val);
    }

    props
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_editorconfig(dir: &Path, content: &str) {
        fs::write(dir.join(".editorconfig"), content).unwrap();
    }

    #[test]
    fn test_parse_indent_style_spaces() {
        let temp_dir = TempDir::new().unwrap();
        create_editorconfig(
            temp_dir.path(),
            r#"
root = true

[*]
indent_style = space
indent_size = 2
"#,
        );

        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "").unwrap();

        let props = get_properties(&test_file);
        assert!(matches!(props.indent_style, Some(IndentStyle::Spaces(_))));
        assert_eq!(props.indent_size, Some(2));
        assert!(matches!(
            props.effective_indent_style(),
            Some(IndentStyle::Spaces(2))
        ));
    }

    #[test]
    fn test_parse_indent_style_tabs() {
        let temp_dir = TempDir::new().unwrap();
        create_editorconfig(
            temp_dir.path(),
            r#"
root = true

[*]
indent_style = tab
tab_width = 4
"#,
        );

        let test_file = temp_dir.path().join("test.go");
        fs::write(&test_file, "").unwrap();

        let props = get_properties(&test_file);
        assert!(matches!(props.indent_style, Some(IndentStyle::Tabs)));
        assert_eq!(props.tab_width, Some(4));
    }

    #[test]
    fn test_parse_end_of_line() {
        let temp_dir = TempDir::new().unwrap();
        create_editorconfig(
            temp_dir.path(),
            r#"
root = true

[*.bat]
end_of_line = crlf

[*.sh]
end_of_line = lf
"#,
        );

        let bat_file = temp_dir.path().join("test.bat");
        fs::write(&bat_file, "").unwrap();
        let props = get_properties(&bat_file);
        assert!(matches!(props.end_of_line, Some(LineEnding::CrLf)));

        let sh_file = temp_dir.path().join("test.sh");
        fs::write(&sh_file, "").unwrap();
        let props = get_properties(&sh_file);
        assert!(matches!(props.end_of_line, Some(LineEnding::Lf)));
    }

    #[test]
    fn test_no_editorconfig() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "").unwrap();

        let props = get_properties(&test_file);
        assert!(props.indent_style.is_none());
        assert!(props.indent_size.is_none());
        assert!(props.end_of_line.is_none());
    }

    #[test]
    fn test_file_specific_patterns() {
        let temp_dir = TempDir::new().unwrap();
        create_editorconfig(
            temp_dir.path(),
            r#"
root = true

[*]
indent_style = space
indent_size = 4

[Makefile]
indent_style = tab

[*.py]
indent_size = 4

[*.{yaml,yml}]
indent_size = 2
"#,
        );

        // Test Makefile
        let makefile = temp_dir.path().join("Makefile");
        fs::write(&makefile, "").unwrap();
        let props = get_properties(&makefile);
        assert!(matches!(props.indent_style, Some(IndentStyle::Tabs)));

        // Test Python file
        let py_file = temp_dir.path().join("test.py");
        fs::write(&py_file, "").unwrap();
        let props = get_properties(&py_file);
        assert_eq!(props.indent_size, Some(4));

        // Test YAML file
        let yaml_file = temp_dir.path().join("test.yaml");
        fs::write(&yaml_file, "").unwrap();
        let props = get_properties(&yaml_file);
        assert_eq!(props.indent_size, Some(2));
    }
}
