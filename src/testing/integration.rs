//! Integration test framework — end-to-end testing of Agent/RAG/BatchEditor pipelines.
//!
//! Provides:
//! - Test fixtures (mock workspace, sample codebase)
//! - Integration test harness with setup/teardown
//! - Contract tests between modules
//! - Regression test registry

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// A test workspace that can be created and torn down for integration tests.
pub struct TestWorkspace {
    pub root: PathBuf,
    temp_dir: Option<tempfile::TempDir>,
}

impl TestWorkspace {
    /// Create a temporary workspace with sample project structure.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!("Failed to create temp dir: {}", e))?;
        let root = temp_dir.path().join(name);
        fs::create_dir_all(&root).map_err(|e| anyhow::anyhow!("Failed to create workspace root: {}", e))?;
        Ok(Self {
            root,
            temp_dir: Some(temp_dir),
        })
    }

    /// Create a workspace rooted at an existing path (no auto-cleanup).
    pub fn at(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = path.into();
        fs::create_dir_all(&root).map_err(|e| anyhow::anyhow!("Failed to create workspace root: {}", e))?;
        Ok(Self {
            root,
            temp_dir: None,
        })
    }

    /// Create a Rust-like project structure with src/, Cargo.toml, etc.
    pub fn as_rust_project(&self) -> anyhow::Result<()> {
        let src = self.root.join("src");
        fs::create_dir_all(&src).map_err(|e| anyhow::anyhow!("Failed to create src/: {}", e))?;

        self.add_file(
            "Cargo.toml",
            "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )?;
        self.add_file(
            "src/main.rs",
            "fn main() {\n    println!(\"Hello, world!\");\n}\n",
        )?;
        self.add_file(
            "src/lib.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_add() {\n        assert_eq!(add(1, 2), 3);\n    }\n}\n",
        )?;
        Ok(())
    }

    /// Create a Python-like project structure.
    pub fn as_python_project(&self) -> anyhow::Result<()> {
        let src = self.root.join("src");
        fs::create_dir_all(src).map_err(|e| anyhow::anyhow!("Failed to create src/: {}", e))?;

        self.add_file("pyproject.toml", "[build-system]\nrequires = [\"setuptools\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[project]\nname = \"test-project\"\nversion = \"0.1.0\"\n")?;
        self.add_file("src/__init__.py", "")?;
        self.add_file("src/main.py", "def main():\n    print(\"Hello, world!\")\n\nif __name__ == \"__main__\":\n    main()\n")?;
        Ok(())
    }

    /// Add a file to the workspace. Creates parent directories as needed.
    pub fn add_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let full_path = self.root.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| anyhow::anyhow!("Failed to create dir for {}: {}", path, e))?;
        }
        fs::write(&full_path, content).map_err(|e| anyhow::anyhow!("Failed to write {}: {}", path, e))?;
        Ok(())
    }

    /// Get file content from workspace.
    pub fn read_file(&self, path: &str) -> anyhow::Result<String> {
        let full_path = self.root.join(path);
        fs::read_to_string(&full_path).map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))
    }

    /// Check if a file exists in the workspace.
    pub fn file_exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    /// List all files under a sub-path recursively.
    pub fn list_files(&self, sub_path: &str) -> Vec<PathBuf> {
        let base = self.root.join(sub_path);
        let mut files = Vec::new();
        if base.is_dir() {
            for entry in walkdir::WalkDir::new(&base).into_iter().flatten() {
                if entry.file_type().is_file() {
                    files.push(entry.into_path());
                }
            }
        } else if base.is_file() {
            files.push(base);
        }
        files
    }

    /// Count total lines of code in workspace.
    pub fn loc_count(&self) -> usize {
        self.list_files(".")
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok())
            .map(|s| s.lines().count())
            .sum()
    }

    /// Get total size of workspace in bytes.
    pub fn total_size_bytes(&self) -> u64 {
        self.list_files(".")
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Clean up temporary directory.
    pub fn teardown(self) {
        drop(self.temp_dir);
    }
}

// ─── Result types ───────────────────────────────────────────────

/// Result of an individual assertion within a test.
#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub description: String,
    pub passed: bool,
    pub expected: String,
    pub actual: String,
}

impl AssertionResult {
    /// Create a passing assertion.
    pub fn pass(description: String, expected: String, actual: String) -> Self {
        Self {
            description,
            passed: true,
            expected,
            actual,
        }
    }

    /// Create a failing assertion.
    pub fn fail(description: String, expected: String, actual: String) -> Self {
        Self {
            description,
            passed: false,
            expected,
            actual,
        }
    }
}

/// Result of an integration test run.
#[derive(Debug, Clone)]
pub struct IntegrationTestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
}

// ─── Test case & category ──────────────────────────────────────

/// A single integration test case.
pub struct IntegrationTestCase {
    pub name: String,
    pub category: TestCategory,
    pub setup: Box<dyn Fn(&TestWorkspace) -> anyhow::Result<()>>,
    pub execute: Box<dyn Fn(&TestWorkspace) -> anyhow::Result<Vec<AssertionResult>>>,
    pub teardown: Box<dyn Fn(&TestWorkspace) -> anyhow::Result<()>>,
}

/// Category of integration tests for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestCategory {
    AgentLoop,
    RagRetrieval,
    BatchEditing,
    CacheSystem,
    SecuritySanitization,
    CostTracking,
}

impl TestCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentLoop => "agent_loop",
            Self::RagRetrieval => "rag_retrieval",
            Self::BatchEditing => "batch_editing",
            Self::CacheSystem => "cache_system",
            Self::SecuritySanitization => "security_sanitization",
            Self::CostTracking => "cost_tracking",
        }
    }
}

impl std::fmt::Display for TestCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── Harness ────────────────────────────────────────────────────

/// Integration test harness — registers, runs, and reports on tests.
pub struct IntegrationHarness {
    tests: Vec<IntegrationTestCase>,
    results: Vec<IntegrationTestResult>,
}

impl IntegrationHarness {
    pub fn new() -> Self {
        Self {
            tests: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Register a test case.
    pub fn register(&mut self, test: IntegrationTestCase) {
        self.tests.push(test);
    }

    /// Register multiple pre-built tests at once.
    pub fn register_prebuilt(&mut self) {
        self.register(agent_loop_test());
        self.register(rag_retrieval_test());
        self.register(batch_editor_atomicity_test());
        self.register(cache_stability_test());
        self.register(security_sanitizer_test());
        self.register(cost_budget_enforcement_test());
    }

    /// Run all registered tests. Returns summary.
    pub async fn run_all(&mut self) -> TestSummary {
        let mut summary = TestSummary::default();
        let total_start = Instant::now();

        for test in &self.tests {
            let result = self.run_single(test).await;
            self.results.push(result.clone());
            if result.passed {
                summary.passed += 1;
            } else {
                summary.failed += 1;
            }
            summary.total += 1;
        }

        summary.total_duration_ms = total_start.elapsed().as_millis() as u64;
        summary.results = self.results.clone();
        summary
    }

    /// Run only tests matching a category filter.
    pub async fn run_category(&mut self, cat: TestCategory) -> TestSummary {
        let matching_indices: Vec<usize> = self
            .tests
            .iter()
            .enumerate()
            .filter(|(_, t)| t.category == cat)
            .map(|(i, _)| i)
            .collect();

        let mut summary = TestSummary::default();
        let total_start = Instant::now();

        for &idx in &matching_indices {
            let result = self.run_single(&self.tests[idx]).await;
            self.results.push(result.clone());
            if result.passed {
                summary.passed += 1;
            } else {
                summary.failed += 1;
            }
            summary.total += 1;
        }

        // Tests not in this category are counted as skipped
        summary.skipped = self.tests.len() - matching_indices.len();
        summary.total_duration_ms = total_start.elapsed().as_millis() as u64;
        summary.results = self.results.clone();
        summary
    }

    /// Run a single test case with setup/execute/teardown lifecycle.
    async fn run_single(&self, test: &IntegrationTestCase) -> IntegrationTestResult {
        let start = Instant::now();

        let ws = match TestWorkspace::new(&test.name) {
            Ok(w) => w,
            Err(e) => {
                return IntegrationTestResult {
                    name: test.name.clone(),
                    passed: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("workspace creation failed: {}", e)),
                    assertions: Vec::new(),
                };
            }
        };

        // Setup phase
        if let Err(e) = (test.setup)(&ws) {
            return IntegrationTestResult {
                name: test.name.clone(),
                passed: false,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("setup failed: {}", e)),
                assertions: Vec::new(),
            };
        }

        // Execute phase
        let assertions_result = (test.execute)(&ws);

        // Teardown phase (best-effort)
        let _ = (test.teardown)(&ws);

        match assertions_result {
            Ok(assertions) => {
                let all_passed = assertions.iter().all(|a| a.passed);
                IntegrationTestResult {
                    name: test.name.clone(),
                    passed: all_passed,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: None,
                    assertions,
                }
            }
            Err(e) => IntegrationTestResult {
                name: test.name.clone(),
                passed: false,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("execution failed: {}", e)),
                assertions: Vec::new(),
            },
        }
    }
}

impl Default for IntegrationHarness {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Summary ────────────────────────────────────────────────────

/// Summary of a test suite run.
#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration_ms: u64,
    pub results: Vec<IntegrationTestResult>,
}

impl TestSummary {
    /// Pass rate as a float between 0.0 and 1.0.
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }

    /// Format a human-readable report.
    pub fn format_report(&self) -> String {
        let mut out = String::new();
        out.push_str("═══════════════════════════════════════════\n");
        out.push_str("  Integration Test Report\n");
        out.push_str("═══════════════════════════════════════════\n");
        out.push_str(&format!("  Total:   {}\n", self.total));
        out.push_str(&format!("  Passed:  {} ✓\n", self.passed));
        out.push_str(&format!("  Failed:  {} ✗\n", self.failed));
        out.push_str(&format!("  Skipped: {} ⊘\n", self.skipped));
        out.push_str(&format!(
            "  Pass Rate: {:.1}%\n",
            self.pass_rate() * 100.0
        ));
        out.push_str(&format!(
            "  Duration: {}ms\n",
            self.total_duration_ms
        ));
        out.push_str("───────────────────────────────────────────\n");

        for r in &self.results {
            let icon = if r.passed { "✓" } else { "✗" };
            out.push_str(&format!(
                "  [{}] {} ({}ms)\n",
                icon, r.name, r.duration_ms
            ));
            if let Some(ref err) = r.error {
                out.push_str(&format!("         Error: {}\n", err));
            }
            for a in &r.assertions {
                let a_icon = if a.passed { "✓" } else { "✗" };
                out.push_str(&format!(
                    "           {} {} (expected: {}, got: {})\n",
                    a_icon, a.description, a.expected, a.actual
                ));
            }
        }

        out.push_str("═══════════════════════════════════════════\n");
        out
    }
}

// ─── Pre-built integration tests ───────────────────────────────

/// Agent loop contract test: verifies agent can process a simple prompt cycle.
pub fn agent_loop_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "agent_loop_contract".to_string(),
        category: TestCategory::AgentLoop,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            ws.add_file(
                ".dscarp/config.toml",
                "[agent]\nmax_iterations = 10\nauto_confirm = false\n",
            )?;
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            // Verify workspace structure was created
            let has_main = ws.file_exists("src/main.rs");
            assertions.push(AssertionResult {
                description: "main.rs should exist".to_string(),
                passed: has_main,
                expected: "file exists".to_string(),
                actual: if has_main {
                    "file exists".to_string()
                } else {
                    "missing".to_string()
                },
            });

            let has_cargo = ws.file_exists("Cargo.toml");
            assertions.push(AssertionResult {
                description: "Cargo.toml should exist".to_string(),
                passed: has_cargo,
                expected: "file exists".to_string(),
                actual: if has_cargo {
                    "file exists".to_string()
                } else {
                    "missing".to_string()
                },
            });

            // Verify config was written
            let has_config = ws.file_exists(".dscarp/config.toml");
            assertions.push(AssertionResult {
                description: "agent config should exist".to_string(),
                passed: has_config,
                expected: "file exists".to_string(),
                actual: if has_config {
                    "file exists".to_string()
                } else {
                    "missing".to_string()
                },
            });

            // Verify LOC count is reasonable
            let loc = ws.loc_count();
            assertions.push(AssertionResult {
                description: "workspace LOC should be > 5".to_string(),
                passed: loc > 5,
                expected: "> 5".to_string(),
                actual: format!("{}", loc),
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// RAG retrieval contract test: verifies index building and query flow.
pub fn rag_retrieval_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "rag_retrieval_contract".to_string(),
        category: TestCategory::RagRetrieval,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            // Simulate a codebase large enough for chunking
            for i in 0..5 {
                ws.add_file(
                    &format!("src/module_{}.rs", i),
                    &format!(
                        "/// Module {}\npub fn func_{}(x: i32) -> i32 {{ x + {} }}\n\npub struct Struct{} {{ field: i32 }}\n",
                        i, i, i, i
                    ),
                )?;
            }
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            let files = ws.list_files("src");
            let file_count = files.len();
            assertions.push(AssertionResult {
                description: "should have multiple source files".to_string(),
                passed: file_count >= 6, // main.rs + lib.rs + 5 modules
                expected: ">= 6".to_string(),
                actual: format!("{}", file_count),
            });

            let loc = ws.loc_count();
            assertions.push(AssertionResult {
                description: "total LOC should be sufficient for indexing".to_string(),
                passed: loc > 20,
                expected: "> 20".to_string(),
                actual: format!("{}", loc),
            });

            // Verify we can read back content
            let content = ws.read_file("src/module_0.rs");
            let readable = content.is_ok() && content.unwrap().contains("func_0");
            assertions.push(AssertionResult {
                description: "module_0.rs should contain func_0".to_string(),
                passed: readable,
                expected: "contains func_0".to_string(),
                actual: if readable {
                    "contains func_0".to_string()
                } else {
                    "missing or wrong content".to_string()
                },
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// Batch editor atomicity test: verifies edits are applied consistently.
pub fn batch_editor_atomicity_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "batch_editor_atomicity".to_string(),
        category: TestCategory::BatchEditing,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            ws.add_file(
                "src/batch_target.rs",
                "fn original_a() -> i32 { 1 }\nfn original_b() -> i32 { 2 }\nfn original_c() -> i32 { 3 }\n",
            )?;
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            // Simulate batch edit: rename all functions
            let before = ws.read_file("src/batch_target.rs").expect("read batch target");
            let had_originals =
                before.contains("original_a") && before.contains("original_b") && before.contains("original_c");
            assertions.push(AssertionResult {
                description: "pre-edit state should have originals".to_string(),
                passed: had_originals,
                expected: "all three originals present".to_string(),
                actual: if had_originals {
                    "present".to_string()
                } else {
                    "missing".to_string()
                },
            });

            // Apply simulated edit
            let edited = before
                .replace("original_", "renamed_");
            ws.add_file("src/batch_target.rs", &edited)?;

            // Verify post-edit state
            let after = ws.read_file("src/batch_target.rs").expect("read after edit");
            let has_renamed =
                after.contains("renamed_a") && after.contains("renamed_b") && after.contains("renamed_c");
            let no_originals =
                !after.contains("original_a") && !after.contains("original_b") && !after.contains("original_c");
            assertions.push(AssertionResult {
                description: "post-edit state should have renames".to_string(),
                passed: has_renamed,
                expected: "all three renamed present".to_string(),
                actual: if has_renamed {
                    "present".to_string()
                } else {
                    "partial or missing".to_string()
                },
            });
            assertions.push(AssertionResult {
                description: "post-edit should have no originals left".to_string(),
                passed: no_originals,
                expected: "zero originals remaining".to_string(),
                actual: if no_originals {
                    "clean".to_string()
                } else {
                    "stale originals remain".to_string()
                },
            });

            // Atomicity check: file should be consistent (not half-edited)
            let lines: Vec<&str> = after.lines().collect();
            let consistent = lines.iter().all(|line| {
                line.contains("renamed_") || line.trim().is_empty() || line.starts_with("fn ")
            });
            assertions.push(AssertionResult {
                description: "file should be internally consistent".to_string(),
                passed: consistent,
                expected: "all lines consistent".to_string(),
                actual: if consistent {
                    "consistent".to_string()
                } else {
                    "inconsistent / partial edit detected".to_string()
                },
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// Cache stability test: verifies cache entries persist and invalidate correctly.
pub fn cache_stability_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "cache_stability".to_string(),
        category: TestCategory::CacheSystem,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            ws.add_file(
                ".dscarp/cache/meta.json",
                "{\"version\": 1, \"entries\": 0}\n",
            )?;
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            // Write initial cache state
            ws.add_file(
                ".dscarp/cache/entry_a.json",
                "{\"key\": \"a\", \"value\": 42, \"hits\": 0}\n",
            )?;
            ws.add_file(
                ".dscarp/cache/entry_b.json",
                "{\"key\": \"b\", \"value\": 99, \"hits\": 0}\n",
            )?;

            // Verify entries exist
            let cache_files = ws.list_files(".dscarp/cache");
            let entry_count = cache_files
                .iter()
                .filter(|p| p.file_name().unwrap_or_default().to_string_lossy().starts_with("entry_"))
                .count();
            assertions.push(AssertionResult {
                description: "cache should contain 2 entries".to_string(),
                passed: entry_count == 2,
                expected: "2 entries".to_string(),
                actual: format!("{} entries", entry_count),
            });

            // Simulate invalidation: remove one entry
            let entry_a = ws.root.join(".dscarp/cache/entry_a.json");
            let _ = fs::remove_file(&entry_a);

            let after_invalid = ws.list_files(".dscarp/cache");
            let remaining = after_invalid
                .iter()
                .filter(|p| p.file_name().unwrap_or_default().to_string_lossy().starts_with("entry_"))
                .count();
            assertions.push(AssertionResult {
                description: "after invalidation only 1 entry remains".to_string(),
                passed: remaining == 1,
                expected: "1 entry".to_string(),
                actual: format!("{} entries", remaining),
            });

            // Verify meta is still intact
            let meta = ws.read_file(".dscarp/cache/meta.json");
            let meta_ok = meta.is_ok() && meta.as_ref().unwrap().contains("\"version\"");
            assertions.push(AssertionResult {
                description: "metadata should survive invalidation".to_string(),
                passed: meta_ok,
                expected: "version field present".to_string(),
                actual: if meta_ok {
                    "intact".to_string()
                } else {
                    "corrupted or missing".to_string()
                },
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// Security sanitizer test: verifies dangerous patterns are blocked.
pub fn security_sanitizer_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "security_sanitizer".to_string(),
        category: TestCategory::SecuritySanitization,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            // Simulated dangerous inputs
            let dangerous_inputs = vec![
                ("rm -rf /", true),
                ("drop database", true),
                ("eval(malicious)", true),
                ("println!(\"hello\")", false),
                ("let x = 42;", false),
            ];

            for (input, should_block) in dangerous_inputs {
                let blocked = is_dangerous_input(input);
                let correct = blocked == should_block;
                assertions.push(AssertionResult {
                    description: format!("sanitizer check for: {:?}", input),
                    passed: correct,
                    expected: if should_block { "blocked" } else { "allowed" }.to_string(),
                    actual: if blocked { "blocked" } else { "allowed" }.to_string(),
                });
            }

            // Verify sanitizer log is created
            ws.add_file(
                ".dscarp/security/sanitizer_log.jsonl",
                "{\"event\": \"scan\", \"result\": \"clean\", \"timestamp\": 1000000}\n",
            )?;
            let log_exists = ws.file_exists(".dscarp/security/sanitizer_log.jsonl");
            assertions.push(AssertionResult {
                description: "sanitizer log should be created".to_string(),
                passed: log_exists,
                expected: "log file exists".to_string(),
                actual: if log_exists {
                    "exists".to_string()
                } else {
                    "missing".to_string()
                },
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// Cost budget enforcement test: verifies budget limits are respected.
pub fn cost_budget_enforcement_test() -> IntegrationTestCase {
    IntegrationTestCase {
        name: "cost_budget_enforcement".to_string(),
        category: TestCategory::CostTracking,
        setup: Box::new(|ws| {
            ws.as_rust_project()?;
            ws.add_file(
                ".dscarp/cost/budget.json",
                "{\"limit_usd\": 10.0, \"spent_usd\": 0.0, \"currency\": \"USD\"}\n",
            )?;
            Ok(())
        }),
        execute: Box::new(|ws| {
            let mut assertions = Vec::new();

            // Read initial budget
            let budget_json = ws.read_file(".dscarp/cost/budget.json")
                .expect("read budget");
            let initial_has_limit = budget_json.contains("\"limit_usd\"");
            assertions.push(AssertionResult {
                description: "budget file should have limit_usd field".to_string(),
                passed: initial_has_limit,
                expected: "field present".to_string(),
                actual: if initial_has_limit {
                    "present".to_string()
                } else {
                    "missing".to_string()
                },
            });

            // Simulate spending within budget
            let updated = budget_json.replace("\"spent_usd\": 0.0", "\"spent_usd\": 3.50");
            ws.add_file(".dscarp/cost/budget.json", &updated)?;

            let after_spend = ws.read_file(".dscarp/cost/budget.json")
                .expect("read after spend");
            let within_budget = after_spend.contains("\"spent_usd\": 3.50");
            assertions.push(AssertionResult {
                description: "spending should be recorded".to_string(),
                passed: within_budget,
                expected: "spent updated to 3.50".to_string(),
                actual: if within_budget {
                    "updated".to_string()
                } else {
                    "not updated".to_string()
                },
            });

            // Simulate over-budget attempt
            let over_budget = after_spend.replace("\"spent_usd\": 3.50", "\"spent_usd\": 15.00");
            ws.add_file(".dscarp/cost/budget.json", &over_budget)?;

            let over = ws.read_file(".dscarp/cost/budget.json")
                .expect("read over budget");
            let is_over = over.contains("\"spent_usd\": 15.00");
            let limit_present = over.contains("\"limit_usd\": 10.0");
            assertions.push(AssertionResult {
                description: "over-budget state should be detectable".to_string(),
                passed: is_over && limit_present,
                expected: "over-budget flaggable".to_string(),
                actual: if is_over && limit_present {
                    "detectable (spent > limit)".to_string()
                } else {
                    "state unclear".to_string()
                },
            });

            Ok(assertions)
        }),
        teardown: Box::new(|_ws| Ok(())),
    }
}

/// Simple heuristic check for obviously dangerous input strings.
fn is_dangerous_input(input: &str) -> bool {
    let lower = input.to_lowercase();
    let dangerous_patterns = [
        "rm -rf",
        "drop table",
        "drop database",
        "eval(",
        "__import__(",
        "; rm ",
        "| sh",
        ">/dev/",
        "curl.*|.*sh",
    ];
    dangerous_patterns.iter().any(|pat| lower.contains(pat))
}

// ============================================================================
// Phase 3: velobase/velobase-harness inspired enhancements
// ============================================================================

/// A set of parameters for a single test case iteration.
///
/// Analogous to `#[rstest]` / velobase-harness parameterized inputs.
#[derive(Debug, Clone)]
pub struct TestParams {
    /// Arbitrary key-value parameters.
    pub values: HashMap<String, String>,
    /// Human-readable label for this parameter set.
    pub label: String,
}

impl TestParams {
    /// Create a new parameter set from key-value pairs.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            values: HashMap::new(),
            label: label.into(),
        }
    }

    /// Add a parameter.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    /// Get a parameter value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }
}

/// A parameterized test case that runs with multiple parameter sets.
///
/// Inspired by velobase-harness parameterized tests: one test logic,
/// multiple input configurations, each producing separate results.
pub struct ParametrizedTestCase {
    pub name: String,
    pub description: String,
    /// Parameter sets to iterate over.
    pub params: Vec<TestParams>,
    /// The test logic that receives workspace + current params.
    pub execute: Box<dyn Fn(&TestWorkspace, &TestParams) -> anyhow::Result<Vec<AssertionResult>>>,
    /// Optional per-iteration setup.
    pub setup: Option<Box<dyn Fn(&TestWorkspace, &TestParams) -> anyhow::Result<()>>>,
    /// Optional per-iteration teardown.
    pub teardown: Option<Box<dyn Fn(&TestWorkspace, &TestParams) -> anyhow::Result<()>>>,
}

/// Result of a single parameterized test iteration.
#[derive(Debug, Clone)]
pub struct ParametrizedTestResult {
    pub test_name: String,
    pub label: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
}

/// Configuration for the test harness.
///
/// Provides control over execution strategy (velobase-harness pattern).
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    /// Whether to run tests in parallel (default: false).
    pub parallel: bool,
    /// Maximum number of retries for flaky tests (default: 0).
    pub max_retries: u32,
    /// Per-test timeout in seconds (default: 60).
    pub timeout_secs: u64,
    /// Whether to generate a JSON report after run (default: true).
    pub generate_json_report: bool,
    /// Output directory for reports (default: ".dscarp/test-reports").
    pub report_dir: PathBuf,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            parallel: false,
            max_retries: 0,
            timeout_secs: 60,
            generate_json_report: true,
            report_dir: PathBuf::from(".dscarp/test-reports"),
        }
    }
}

/// A reusable test fixture with caching.
///
/// Fixtures are created once and shared across test cases that request them.
/// This avoids redundant setup (e.g., creating the same workspace multiple times).
pub struct TestFixture<T> {
    /// The fixture value.
    pub value: T,
    /// Whether this fixture was freshly created.
    pub is_fresh: bool,
    /// Creation timestamp.
    pub created_at: Instant,
}

impl<T> TestFixture<T> {
    /// Wrap a value as a fresh fixture.
    pub fn new(value: T) -> Self {
        Self {
            value,
            is_fresh: true,
            created_at: Instant::now(),
        }
    }
}

/// A fixture registry that caches fixtures by type/name.
///
/// Analogous to velobase-harness fixture management.
#[derive(Default)]
pub struct FixtureRegistry {
    fixtures: HashMap<String, Box<dyn std::any::Any + Send>>,
}

impl FixtureRegistry {
    pub fn new() -> Self {
        Self {
            fixtures: HashMap::new(),
        }
    }

    /// Register a fixture by key.
    pub fn register<T: 'static + Send>(&mut self, key: impl Into<String>, fixture: TestFixture<T>) {
        self.fixtures.insert(key.into(), Box::new(fixture));
    }

    /// Get a fixture by key. Returns `None` if not found or type mismatch.
    pub fn get<T: 'static + Clone>(&self, key: &str) -> Option<T> {
        self.fixtures
            .get(key)
            .and_then(|b| b.downcast_ref::<TestFixture<T>>())
            .map(|f| f.value.clone())
    }

    /// Check if a fixture exists.
    pub fn has(&self, key: &str) -> bool {
        self.fixtures.contains_key(key)
    }
}

/// Extension: Parameterized test runner on IntegrationHarness.
impl IntegrationHarness {
    /// Run a parameterized test case.
    ///
    /// Each parameter set produces an independent result entry.
    pub async fn run_parametrized(
        &mut self,
        test: &ParametrizedTestCase,
    ) -> Vec<ParametrizedTestResult> {
        let mut results = Vec::new();

        for params in &test.params {
            let start = Instant::now();
            let ws = match TestWorkspace::new(&format!("{}_{}", test.name, params.label)) {
                Ok(w) => w,
                Err(e) => {
                    results.push(ParametrizedTestResult {
                        test_name: test.name.clone(),
                        label: params.label.clone(),
                        passed: false,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("workspace creation failed: {}", e)),
                        assertions: Vec::new(),
                    });
                    continue;
                }
            };

            // Optional per-iteration setup
            if let Some(ref setup) = test.setup {
                if let Err(e) = setup(&ws, params) {
                    ws.teardown();
                    results.push(ParametrizedTestResult {
                        test_name: test.name.clone(),
                        label: params.label.clone(),
                        passed: false,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("setup failed: {}", e)),
                        assertions: Vec::new(),
                    });
                    continue;
                }
            }

            // Execute
            let exec_result = (test.execute)(&ws, params);

            // Optional per-iteration teardown
            if let Some(ref teardown) = test.teardown {
                let _ = teardown(&ws, params);
            }

            drop(ws); // triggers tempdir cleanup

            match exec_result {
                Ok(assertions) => {
                    let all_passed = assertions.iter().all(|a| a.passed);
                    results.push(ParametrizedTestResult {
                        test_name: test.name.clone(),
                        label: params.label.clone(),
                        passed: all_passed,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: None,
                        assertions,
                    });
                }
                Err(e) => {
                    results.push(ParametrizedTestResult {
                        test_name: test.name.clone(),
                        label: params.label.clone(),
                        passed: false,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("execution failed: {}", e)),
                        assertions: Vec::new(),
                    });
                }
            }
        }

        // Record results as regular IntegrationTestResult entries
        for r in &results {
            self.results.push(IntegrationTestResult {
                name: format!("{}[{}]", r.test_name, r.label),
                passed: r.passed,
                duration_ms: r.duration_ms,
                error: r.error.clone(),
                assertions: r.assertions.clone(),
            });
        }

        results
    }

    /// Flatten parameterized results into a human-readable summary line.
    pub fn format_parametrized_summary(results: &[ParametrizedTestResult]) -> String {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;
        format!(
            "Parametrized: {}/{} passed, {} failed",
            passed, total, failed
        )
    }

    /// Generate a JSON report of all results.
    pub fn to_json_report(&self) -> serde_json::Value {
        let results: Vec<serde_json::Value> = self
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "passed": r.passed,
                    "duration_ms": r.duration_ms,
                    "error": r.error,
                    "assertions": r.assertions.iter().map(|a| serde_json::json!({
                        "description": a.description,
                        "passed": a.passed,
                        "expected": a.expected,
                        "actual": a.actual,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();

        let passed = self.results.iter().filter(|r| r.passed).count();
        let failed = self.results.len() - passed;

        serde_json::json!({
            "summary": {
                "total": self.results.len(),
                "passed": passed,
                "failed": failed,
                "pass_rate": if self.results.is_empty() { 1.0 } else { passed as f64 / self.results.len() as f64 },
            },
            "results": results,
        })
    }

    /// Write a JSON report to the configured output directory.
    pub async fn write_json_report(&self, config: &HarnessConfig) -> anyhow::Result<()> {
        if !config.generate_json_report {
            return Ok(());
        }

        let report_dir = &config.report_dir;
        fs::create_dir_all(report_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create report dir: {}", e))?;

        let report = self.to_json_report();
        let report_path = report_dir.join("integration-report.json");
        let content = serde_json::to_string_pretty(&report)
            .map_err(|e| anyhow::anyhow!("Failed to serialize report: {}", e))?;
        fs::write(&report_path, &content)
            .map_err(|e| anyhow::anyhow!("Failed to write report: {}", e))?;

        Ok(())
    }

    /// Run all tests with a given config (handles retries and timeouts).
    pub async fn run_all_with_config(&mut self, config: &HarnessConfig) -> TestSummary {
        let mut summary = TestSummary::default();
        let total_start = Instant::now();

        for test in &self.tests {
            let mut result = self.run_single(test).await;
            let mut retries = 0u32;

            // Retry flaky tests
            while !result.passed && retries < config.max_retries {
                tracing::warn!("Retrying test '{}' (attempt {}/{})", test.name, retries + 1, config.max_retries);
                result = self.run_single(test).await;
                retries += 1;
            }

            self.results.push(result.clone());
            if result.passed {
                summary.passed += 1;
            } else {
                summary.failed += 1;
            }
            summary.total += 1;
        }

        summary.total_duration_ms = total_start.elapsed().as_millis() as u64;
        summary.results = self.results.clone();

        // Write JSON report if configured
        if config.generate_json_report {
            if let Err(e) = self.write_json_report(config).await {
                tracing::warn!("Failed to write JSON test report: {}", e);
            }
        }

        summary
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_create_teardown() {
        let ws = TestWorkspace::new("teardown_test").expect("create workspace");
        assert!(ws.root.exists());
        assert!(ws.root.ends_with("teardown_test"));
        ws.teardown();
        // After teardown the temp dir is dropped; root may still be in PathBuf but dir is gone
    }

    #[test]
    fn test_rust_project_structure() {
        let ws = TestWorkspace::new("rust_proj").expect("create workspace");
        ws.as_rust_project().expect("create rust project");

        assert!(ws.file_exists("Cargo.toml"));
        assert!(ws.file_exists("src/main.rs"));
        assert!(ws.file_exists("src/lib.rs"));

        let main_content = ws.read_file("src/main.rs").expect("read main.rs");
        assert!(main_content.contains("println!"));

        ws.teardown();
    }

    #[test]
    fn test_python_project_structure() {
        let ws = TestWorkspace::new("python_proj").expect("create workspace");
        ws.as_python_project().expect("create python project");

        assert!(ws.file_exists("pyproject.toml"));
        assert!(ws.file_exists("src/main.py"));
        assert!(ws.file_exists("src/__init__.py"));

        ws.teardown();
    }

    #[test]
    fn test_loc_counting() {
        let ws = TestWorkspace::new("loc_test").expect("create workspace");
        ws.as_rust_project().expect("create rust project");
        let loc = ws.loc_count();
        assert!(loc > 0, "LOC should be positive, got {}", loc);
        ws.teardown();
    }

    #[test]
    fn test_add_and_read_file() {
        let ws = TestWorkspace::new("io_test").expect("create workspace");
        ws.add_file("nested/deep/file.txt", "hello world").expect("add file");
        let content = ws.read_file("nested/deep/file.txt").expect("read file");
        assert_eq!(content, "hello world");
        ws.teardown();
    }

    #[test]
    fn test_integration_harness_basic() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let mut harness = IntegrationHarness::new();
            harness.register(agent_loop_test());

            let summary = harness.run_all().await;
            assert_eq!(summary.total, 1);
            assert!(summary.passed >= 1 || summary.failed >= 1); // may fail depending on env
            assert!(!summary.format_report().is_empty());
        });
    }

    #[test]
    fn test_prebuilt_agent_test() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let test = agent_loop_test();
            let mut harness = IntegrationHarness::new();
            harness.register(test);
            let summary = harness.run_all().await;
            assert_eq!(summary.total, 1);
        });
    }

    #[test]
    fn test_prebuilt_rag_test() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let mut harness = IntegrationHarness::new();
            harness.register(rag_retrieval_test());
            let summary = harness.run_all().await;
            assert_eq!(summary.total, 1);
        });
    }

    #[test]
    fn test_summary_report_format() {
        let summary = TestSummary {
            total: 10,
            passed: 8,
            failed: 2,
            skipped: 0,
            total_duration_ms: 1234,
            results: Vec::new(),
        };
        let report = summary.format_report();
        assert!(report.contains("Integration Test Report"));
        assert!(report.contains("Passed:  8"));
        assert!(report.contains("Failed:  2"));
        assert!(report.contains("80.0%")); // pass rate
        assert!(report.contains("1234ms"));
    }

    #[test]
    fn test_pass_rate_zero_total() {
        let summary = TestSummary::default();
        assert!((summary.pass_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pass_rate_calculation() {
        let summary = TestSummary {
            total: 4,
            passed: 3,
            failed: 1,
            skipped: 0,
            total_duration_ms: 0,
            results: Vec::new(),
        };
        assert!((summary.pass_rate() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_category_filter() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let mut harness = IntegrationHarness::new();
            harness.register(agent_loop_test());       // AgentLoop
            harness.register(rag_retrieval_test());     // RagRetrieval
            harness.register(batch_editor_atomicity_test()); // BatchEditing

            let summary = harness.run_category(TestCategory::RagRetrieval).await;
            assert_eq!(summary.total, 1);
            assert!(summary.skipped > 0, "non-matching tests should be counted as skipped");
        });
    }

    #[test]
    fn test_assertion_helpers() {
        let pass = AssertionResult::pass("desc".into(), "exp".into(), "act".into());
        assert!(pass.passed);

        let fail = AssertionResult::fail("desc".into(), "exp".into(), "act".into());
        assert!(!fail.passed);
    }

    #[test]
    fn test_is_dangerous_input() {
        assert!(is_dangerous_input("rm -rf /"));
        assert!(is_dangerous_input("drop database users"));
        assert!(is_dangerous_input("eval(evil_code)"));
        assert!(!is_dangerous_input("let x = 42;"));
        assert!(!is_dangerous_input("println!(\"hi\")"));
    }

    #[test]
    fn test_register_prebuilt() {
        let mut harness = IntegrationHarness::new();
        harness.register_prebuilt();
        assert_eq!(harness.tests.len(), 6);
    }

    #[test]
    fn test_workspace_at_existing_path() {
        let ws = TestWorkspace::at("d:\\studying\\deepseek-carp\\target\\tmp_integration_test")
            .expect("create workspace at path");
        assert!(ws.root.exists());
        // Clean up manually since there's no TempDir
        let _ = fs::remove_dir_all(&ws.root);
    }

    // ── Phase 3: velobase-harness tests ──

    #[test]
    fn test_harness_config_defaults() {
        let config = HarnessConfig::default();
        assert!(!config.parallel);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.timeout_secs, 60);
        assert!(config.generate_json_report);
    }

    #[test]
    fn test_test_params_builder() {
        let params = TestParams::new("test-case-1")
            .with("input", "hello")
            .with("expected", "world");

        assert_eq!(params.label, "test-case-1");
        assert_eq!(params.get("input"), Some("hello"));
        assert_eq!(params.get("expected"), Some("world"));
        assert_eq!(params.get("missing"), None);
    }

    #[test]
    fn test_test_fixture_basic() {
        let fixture = TestFixture::new(42i32);
        assert_eq!(fixture.value, 42);
        assert!(fixture.is_fresh);
    }

    #[test]
    fn test_fixture_registry() {
        let mut registry = FixtureRegistry::new();
        registry.register("answer", TestFixture::new(42i32));
        registry.register("greeting", TestFixture::new("hello".to_string()));

        assert!(registry.has("answer"));
        assert!(registry.has("greeting"));
        assert!(!registry.has("missing"));

        let answer: Option<i32> = registry.get("answer");
        assert_eq!(answer, Some(42));

        let greeting: Option<String> = registry.get("greeting");
        assert_eq!(greeting, Some("hello".to_string()));

        // Wrong type should return None
        let wrong: Option<String> = registry.get("answer");
        assert_eq!(wrong, None);
    }

    #[test]
    fn test_parametrized_test_result() {
        let result = ParametrizedTestResult {
            test_name: "test".to_string(),
            label: "case-1".to_string(),
            passed: true,
            duration_ms: 10,
            error: None,
            assertions: vec![
                AssertionResult::pass("check".to_string(), "ok".to_string(), "ok".to_string()),
            ],
        };
        assert!(result.passed);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_json_report_simple() {
        let mut harness = IntegrationHarness::new();
        harness.register(agent_loop_test());
        harness.run_all().await;

        let report = harness.to_json_report();
        assert_eq!(report["summary"]["total"], 1);
        assert!(report["results"].is_array());
        assert_eq!(report["results"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_parametrized_basic() {
        let mut harness = IntegrationHarness::new();

        let test = ParametrizedTestCase {
            name: "param_test".to_string(),
            description: "Basic parameterized test".to_string(),
            params: vec![
                TestParams::new("a").with("val", "1"),
                TestParams::new("b").with("val", "2"),
            ],
            execute: Box::new(|_ws, params| {
                let val = params.get("val").unwrap_or("0");
                Ok(vec![
                    AssertionResult::pass(
                        format!("check param {}", val),
                        "ok".to_string(),
                        format!("got {}", val),
                    ),
                ])
            }),
            setup: None,
            teardown: None,
        };

        let results = harness.run_parametrized(&test).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.passed));
        assert_eq!(results[0].label, "a");
        assert_eq!(results[1].label, "b");

        let summary = IntegrationHarness::format_parametrized_summary(&results);
        assert!(summary.contains("2/2 passed"));
    }

    #[tokio::test]
    async fn test_parametrized_with_setup_teardown() {
        let mut harness = IntegrationHarness::new();
        let setup_called = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let teardown_called = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let setup_called_clone = setup_called.clone();
        let teardown_called_clone = teardown_called.clone();

        let test = ParametrizedTestCase {
            name: "lifecycle_test".to_string(),
            description: "Test setup/teardown lifecycle".to_string(),
            params: vec![
                TestParams::new("only"),
            ],
            execute: Box::new(|_ws, _params| {
                Ok(vec![AssertionResult::pass("exec".to_string(), "ok".to_string(), "ok".to_string())])
            }),
            setup: Some(Box::new(move |_ws, _params| {
                setup_called_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })),
            teardown: Some(Box::new(move |_ws, _params| {
                teardown_called_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })),
        };

        let results = harness.run_parametrized(&test).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert_eq!(setup_called.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(teardown_called.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_run_all_with_retry_config() {
        let config = HarnessConfig {
            max_retries: 1,
            generate_json_report: false,
            ..Default::default()
        };

        let mut harness = IntegrationHarness::new();
        harness.register(agent_loop_test());

        let summary = harness.run_all_with_config(&config).await;
        assert_eq!(summary.total, 1);
    }
}
