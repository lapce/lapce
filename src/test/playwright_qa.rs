//! Playwright-based E2E QA automation (gstack's /qa skill).
//!
//! Provides browser-level testing capabilities for web projects.
//! Integrates with the LoopEngine's TestMode to run automated
//! end-to-end tests after code changes.
//!
//! ## Architecture
//!
//! ```
//! TestMode → QaRunner → Playwright subprocess
//!   ├── navigate(url)
//!   ├── screenshot(path)
//!   ├── click(selector)
//!   ├── fill(selector, text)
//!   ├── assert_text(selector, expected)
//!   └── close()
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

/// A single QA test case.
#[derive(Debug, Clone)]
pub struct QaTestCase {
    /// Human-readable name.
    pub name: String,
    /// URL to navigate to (or file:// path).
    pub url: String,
    /// Steps to execute in order.
    pub steps: Vec<QaStep>,
}

/// A single QA step (action or assertion).
#[derive(Debug, Clone)]
pub enum QaStep {
    /// Navigate to a URL.
    Navigate { url: String },
    /// Click an element by CSS selector.
    Click { selector: String },
    /// Fill a form field.
    Fill { selector: String, value: String },
    /// Assert that element contains text.
    AssertText { selector: String, contains: String },
    /// Assert that element is visible.
    AssertVisible { selector: String },
    /// Wait for navigation or timeout.
    WaitFor { ms: u64 },
    /// Take a screenshot.
    Screenshot { path: PathBuf },
    /// Custom JavaScript evaluation.
    Evaluate { script: String },
}

/// Result of running a single test case.
#[derive(Debug, Clone)]
pub struct QaTestResult {
    pub name: String,
    pub passed: bool,
    pub steps_run: usize,
    pub total_steps: usize,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub screenshots: Vec<PathBuf>,
}

/// Aggregated QA report for all test cases.
#[derive(Debug, Clone, Default)]
pub struct QaReport {
    pub results: Vec<QaTestResult>,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub total_duration_ms: u64,
}

impl QaReport {
    /// Format as human-readable text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("╔════════════════════════════════════╗\n");
        out.push_str("║     QA Report (Playwright)          ║\n");
        out.push_str("╚════════════════════════════════════╝\n\n");
        out.push_str(&format!(
            "Tests: {}/{} | Total time: {:.1}s\n\n",
            self.passed_tests,
            self.total_tests,
            self.total_duration_ms as f64 / 1000.0
        ));

        for r in &self.results {
            let status = if r.passed { "PASS" } else { "FAIL" };
            out.push_str(&format!(
                "[{}] {} — {}/{} steps ({:.1}s)\n",
                status, r.name, r.steps_run, r.total_steps,
                r.duration_ms as f64 / 1000.0
            ));
            if let Some(ref err) = r.error {
                out.push_str(&format!("      Error: {}\n", err));
            }
        }
        out
    }

    /// Return true if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }
}

/// Runner that executes QA tests via Playwright.
///
/// Uses `npx playwright` as a subprocess to avoid requiring
/// Rust bindings. Falls back gracefully if Playwright is not installed.
pub struct QaRunner {
    /// Project root (for resolving relative paths).
    project_root: PathBuf,
    /// Screenshots output directory.
    screenshots_dir: PathBuf,
    /// Whether Playwright is available.
    playwright_available: bool,
}

impl QaRunner {
    /// Create a new QaRunner, detecting if Playwright is available.
    pub fn new(project_root: &Path) -> Self {
        // Check if npx playwright is available
        let check = Command::new("npx")
            .args(["playwright", "--version"])
            .output();
        let available = match check {
            Ok(o) => o.status.success(),
            Err(_) => false,
        };

        if !available {
            tracing::warn!("QaRunner: Playwright not found. Install with: npm i -D @playwright/test && npx playwright install");
        }

        Self {
            project_root: project_root.to_path_buf(),
            screenshots_dir: project_root.join(".carp").join("screenshots"),
            playwright_available: available,
        }
    }

    /// Check if Playwright is available.
    pub fn is_available(&self) -> bool {
        self.playwright_available
    }

    /// Run a single test case.
    ///
    /// Returns the result with details about each step executed.
    pub async fn run_test(&self, test: &QaTestCase) -> QaTestResult {
        use std::time::Instant;
        let start = Instant::now();

        if !self.playwright_available {
            return QaTestResult {
                name: test.name.clone(),
                passed: false,
                steps_run: 0,
                total_steps: test.steps.len(),
                error: Some("Playwright is not installed".into()),
                duration_ms: start.elapsed().as_millis() as u64,
                screenshots: Vec::new(),
            };
        }

        // Ensure screenshots directory exists
        let _ = std::fs::create_dir_all(&self.screenshots_dir);

        // Generate a temporary Playwright script from the test case
        let script = self.generate_playwright_script(test);

        // Write script to temp file
        let script_path = self.screenshots_dir.join(format!("test_{}.mjs", slugify(&test.name)));
        if let Err(e) = std::fs::write(&script_path, &script) {
            return QaTestResult {
                name: test.name.clone(),
                passed: false,
                steps_run: 0,
                total_steps: test.steps.len(),
                error: Some(format!("Failed to write script: {}", e)),
                duration_ms: start.elapsed().as_millis() as u64,
                screenshots: Vec::new(),
            };
        }

        // Execute via Node.js + Playwright
        let result = Command::new("npx")
            .args(["playwright", "test", &script_path.to_string_lossy()])
            .current_dir(&self.project_root)
            .output();

        let mut screenshots = Vec::new();
        let (passed, error) = match result {
            Ok(output) => {
                // Collect any generated screenshots
                if let Ok(entries) = std::fs::read_dir(&self.screenshots_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().and_then(|e| e.to_str()) == Some("png") {
                            screenshots.push(p);
                        }
                    }
                }
                (output.status.success(), None)
            }
            Err(e) => (false, Some(e.to_string())),
        };

        // Cleanup temp script
        let _ = std::fs::remove_file(&script_path);

        QaTestResult {
            name: test.name.clone(),
            passed,
            steps_run: test.steps.len(), // Assume all ran if no early failure
            total_steps: test.steps.len(),
            error,
            duration_ms: start.elapsed().as_millis() as u64,
            screenshots,
        }
    }

    /// Run multiple test cases and produce a report.
    pub async fn run_suite(&self, tests: &[QaTestCase]) -> QaReport {
        use std::time::Instant;
        let start = Instant::now();
        let mut results = Vec::with_capacity(tests.len());

        for test in tests {
            let result = self.run_test(test).await;
            results.push(result);
        }

        let passed = results.iter().filter(|r| r.passed).count();

        QaReport {
            results,
            total_tests: tests.len(),
            passed_tests: passed,
            total_duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Generate a Playwright test script from a QaTestCase.
    fn generate_playwright_script(&self, test: &QaTestCase) -> String {
        let mut script = format!(
            "// Auto-generated by deepseek-carp QaRunner\n\
             import {{ test, expect }} from '@playwright/test';\n\n\
             test('{}', async ({{ page }}) => {{\n",
            escape_js_string(&test.name)
        );

        for step in &test.steps {
            match step {
                QaStep::Navigate { url } => {
                    script.push_str(&format!("  await page.goto('{}');\n", escape_js_string(url)));
                }
                QaStep::Click { selector } => {
                    script.push_str(&format!("  await page.click('{}');\n", escape_js_string(selector)));
                }
                QaStep::Fill { selector, value } => {
                    script.push_str(&format!(
                        "  await page.fill('{}', '{}');\n",
                        escape_js_string(selector),
                        escape_js_string(value)
                    ));
                }
                QaStep::AssertText { selector, contains } => {
                    script.push_str(&format!(
                        "  await expect(page.locator('{}')).toContainText('{}');\n",
                        escape_js_string(selector),
                        escape_js_string(contains)
                    ));
                }
                QaStep::AssertVisible { selector } => {
                    script.push_str(&format!(
                        "  await expect(page.locator('{}')).toBeVisible();\n",
                        escape_js_string(selector)
                    ));
                }
                QaStep::WaitFor { ms } => {
                    script.push_str(&format!("  await page.waitForTimeout({});\n", ms));
                }
                QaStep::Screenshot { path } => {
                    script.push_str(&format!(
                        "  await page.screenshot({{ path: '{}' }});\n",
                        path.to_string_lossy().replace('\\', "/")
                    ));
                }
                QaStep::Evaluate { script: js } => {
                    script.push_str(&format!("  await page.evaluate(() => {{ {} }});\n", js));
                }
            }
        }

        script.push_str("});\n");
        script
    }
}

/// Simple string slugification for filenames.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' {
            c.to_ascii_lowercase()
        } else {
            '_'
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Escape string for JavaScript literal.
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qa_report_empty() {
        let report = QaReport::default();
        assert!(report.all_passed());
        assert_eq!(report.total_tests, 0);
    }

    #[test]
    fn test_qa_report_with_results() {
        let report = QaReport {
            results: vec![
                QaTestResult {
                    name: "login".into(),
                    passed: true,
                    steps_run: 3,
                    total_steps: 3,
                    error: None,
                    duration_ms: 1000,
                    screenshots: vec![],
                },
                QaTestResult {
                    name: "checkout".into(),
                    passed: false,
                    steps_run: 2,
                    total_steps: 5,
                    error: Some("element not found".into()),
                    duration_ms: 500,
                    screenshots: vec![],
                },
            ],
            total_tests: 2,
            passed_tests: 1,
            total_duration_ms: 1500,
        };
        assert!(!report.all_passed());
        let text = report.to_text();
        assert!(text.contains("PASS"));
        assert!(text.contains("FAIL"));
        assert!(text.contains("1/2"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Login Page Test"), "login_page_test");
        assert_eq!(slugify("  hello world  "), "hello_world");
        assert!(!slugify("").is_empty()); // empty becomes ""
    }

    #[test]
    fn test_qa_runner_new() {
        let runner = QaRunner::new(Path::new("."));
        // Should not panic; availability depends on system
        let _text = runner.is_available();
    }

    #[test]
    fn test_generate_playwright_script() {
        let runner = QaRunner::new(Path::new("."));
        let test = QaTestCase {
            name: "test_nav".into(),
            url: "http://localhost:3000".into(),
            steps: vec![
                QaStep::Navigate { url: "http://localhost:3000".into() },
                QaStep::AssertText { selector: "h1".into(), contains: "Hello".into() },
            ],
        };
        let script = runner.generate_playwright_script(&test);
        assert!(script.contains("page.goto"));
        assert!(script.contains("expect"));
        assert!(script.contains("'test_nav'"));
    }
}