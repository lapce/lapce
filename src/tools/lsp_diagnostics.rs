//! LSP Diagnostic Injection — post-edit validation via language servers.
//!
//! After AI edits files, this tool runs language-specific diagnostics and
//! feeds errors back to the LLM for auto-fixing. Integrates with the
//! existing CompileEngine for Rust projects.
//!
//! ## Supported languages
//! - Rust: cargo check (via CompileEngine)
//! - TypeScript/JavaScript: tsc --noEmit
//! - Python: ruff check / mypy
//! - Go: go vet
//!
//! ## Integration with CompileEngine
//! For Rust projects, this delegates to CompileEngine::check() with
//! targeted crate checking. For other languages, uses CLI tools.

use std::path::Path;

/// Result of LSP diagnostic check.
#[derive(Debug, Clone)]
pub struct LspDiagnosticResult {
    /// Whether the check passed (no errors).
    pub success: bool,
    /// Formatted diagnostic messages for LLM consumption.
    pub diagnostics: String,
    /// Number of errors found.
    pub error_count: usize,
    /// Number of warnings found.
    pub warning_count: usize,
    /// Raw output from the diagnostic tool.
    pub raw_output: String,
}

/// Run LSP diagnostics for a file, auto-detecting the language.
pub fn run_diagnostics(file_path: &str, workspace_root: &str) -> LspDiagnosticResult {
    let path = Path::new(file_path);
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "rs" => rust_diagnostics(workspace_root),
        "ts" | "tsx" | "js" | "jsx" => tsc_diagnostics(workspace_root),
        "py" => python_diagnostics(workspace_root),
        "go" => go_diagnostics(workspace_root),
        _ => LspDiagnosticResult {
            success: true,
            diagnostics: format!("No diagnostics available for .{} files", ext),
            error_count: 0,
            warning_count: 0,
            raw_output: String::new(),
        },
    }
}

/// Run cargo check for Rust projects.
fn rust_diagnostics(workspace_root: &str) -> LspDiagnosticResult {
    let engine = crate::agent::CompileEngine::new(workspace_root);
    let result = engine.check();

    let mut diagnostics = String::new();
    if result.success {
        diagnostics.push_str("Rust: compilation passed.\n");
    } else {
        diagnostics.push_str(&format!(
            "Rust: {} errors, {} warnings found:\n",
            result.errors.len(),
            result.warnings
        ));
        for err in result.errors.iter().take(10) {
            diagnostics.push_str(&format!(
                "  {}:{}:{} — {}\n",
                err.file, err.line, err.column, err.message
            ));
        }
        if result.errors.len() > 10 {
            diagnostics.push_str(&format!("  ... and {} more errors\n", result.errors.len() - 10));
        }
    }

    LspDiagnosticResult {
        success: result.success,
        error_count: result.errors.len(),
        warning_count: result.warnings,
        diagnostics,
        raw_output: result.output,
    }
}

/// Run tsc --noEmit for TypeScript/JavaScript projects.
fn tsc_diagnostics(workspace_root: &str) -> LspDiagnosticResult {
    let output = std::process::Command::new("npx")
        .args(["tsc", "--noEmit", "--pretty", "false"])
        .current_dir(workspace_root)
        .output();

    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{}{}", stdout, stderr);

            let error_count = combined.lines().filter(|l| l.contains("error TS")).count();
            let warning_count = combined.lines().filter(|l| l.contains("warning TS")).count();
            let success = out.status.success();

            let truncated = if combined.lines().count() > 50 {
                let lines: Vec<&str> = combined.lines().take(50).collect();
                format!("{}\n... truncated ({} lines total)", lines.join("\n"), combined.lines().count())
            } else {
                combined.clone()
            };

            let raw_output = combined.clone();
            LspDiagnosticResult {
                success,
                diagnostics: if success {
                    "TypeScript: type-checking passed.".into()
                } else {
                    format!("TypeScript: {} errors, {} warnings:\n{}", error_count, warning_count, truncated)
                },
                error_count,
                warning_count,
                raw_output,
            }
        }
        Err(e) => LspDiagnosticResult {
            success: true, // Don't block — tool not available
            diagnostics: format!("tsc not available: {}. Install with: npm install -g typescript", e),
            error_count: 0,
            warning_count: 0,
            raw_output: e.to_string(),
        },
    }
}

/// Run ruff check for Python projects.
fn python_diagnostics(workspace_root: &str) -> LspDiagnosticResult {
    // Try ruff first, fallback to flake8
    let output = std::process::Command::new("ruff")
        .args(["check", "--output-format", "concise"])
        .current_dir(workspace_root)
        .output();

    match output {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let error_count = combined.lines().filter(|l| !l.is_empty()).count();
            let success = out.status.success() || error_count == 0;

            let raw_output = combined.clone();
            LspDiagnosticResult {
                success,
                diagnostics: if success {
                    "Python (ruff): no issues found.".into()
                } else {
                    let truncated = if combined.lines().count() > 30 {
                        let lines: Vec<&str> = combined.lines().take(30).collect();
                        format!("{}\n... truncated ({} issues total)", lines.join("\n"), combined.lines().count())
                    } else {
                        combined.clone()
                    };
                    format!("Python (ruff): {} issues:\n{}", error_count, truncated)
                },
                error_count,
                warning_count: 0,
                raw_output,
            }
        }
        Err(_) => LspDiagnosticResult {
            success: true,
            diagnostics: "ruff not available. Install with: pip install ruff".into(),
            error_count: 0,
            warning_count: 0,
            raw_output: String::new(),
        },
    }
}

/// Run go vet for Go projects.
fn go_diagnostics(workspace_root: &str) -> LspDiagnosticResult {
    let output = std::process::Command::new("go")
        .args(["vet", "./..."])
        .current_dir(workspace_root)
        .output();

    match output {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let error_count = combined.lines().filter(|l| !l.is_empty()).count();
            let success = out.status.success();

            LspDiagnosticResult {
                success,
                diagnostics: if success {
                    "Go (go vet): no issues found.".into()
                } else {
                    format!("Go (go vet): {} issues:\n{}", error_count, combined)
                },
                error_count,
                warning_count: 0,
                raw_output: combined,
            }
        }
        Err(e) => LspDiagnosticResult {
            success: true,
            diagnostics: format!("go vet not available: {}", e),
            error_count: 0,
            warning_count: 0,
            raw_output: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_diagnostics_unknown_language() {
        let result = run_diagnostics("README.md", ".");
        assert!(result.success);
        assert!(result.diagnostics.contains("No diagnostics available"));
    }
}
