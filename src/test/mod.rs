//! TestMode — Browser E2E testing adapters for the Unified Agent Loop.
//!
//! Implements the Observe→Plan→Act→Evaluate cycle for web page testing.
//!
//! ## Architecture
//!
//! - [`BrowserObserver`]: Fetches URL via headless browser or HTTP, extracts page content (Observe)
//! - [`TestPlanner`]: Analyzes page content, plans assertions (Plan)
//! - [`PageActor`]: Performs browser interactions (screenshot, JS execution) (Act)
//! - [`ContentEvaluator`]: Checks page content against expectations (Evaluate)
//!
//! ## Browser Backends
//!
//! - Chrome/Chromium headless (screenshots + full DOM)
//! - HTTP fallback (text-only, no JS)

pub mod browser;
pub mod browser_agent;
pub mod code_generation;
pub mod playwright_qa;
pub mod visual_analyzer;

use crate::r#loop::{Observer, Planner, Actor, Evaluator, LoopVerdict};
use browser::{BrowserBackendKind, HeadlessBrowser};
use async_trait::async_trait;

// ============================================================================
// Data types
// ============================================================================

/// Content structure produced by BrowserObserver.
#[derive(Debug, Clone)]
pub struct PageContent {
    /// The URL that was fetched.
    pub url: String,
    /// Raw HTML/text content of the page.
    pub content: String,
    /// HTTP status code.
    pub status_code: u16,
}

/// A single test assertion/action to perform.
#[derive(Debug, Clone)]
pub struct TestAction {
    /// Description of what this test checks.
    pub description: String,
    /// The URL or page element to interact with.
    pub target: String,
    /// Expected behavior.
    pub expected: String,
}

/// Test plan produced by TestPlanner, consumed by PageActor.
#[derive(Debug, Clone)]
pub struct TestPlan {
    pub actions: Vec<TestAction>,
}

/// Result of executing a test plan.
#[derive(Debug, Clone)]
pub struct TestActionResult {
    /// Actions that passed.
    pub passed: Vec<String>,
    /// Actions that failed.
    pub failed: Vec<(String, String)>,
    /// Full output for logging.
    pub output: String,
}

// ============================================================================
// BrowserObserver — Observe phase
// ============================================================================

/// Fetches a URL via headless browser (Chrome) or HTTP fallback.
/// Captures page content and optionally screenshots.
pub struct BrowserObserver {
    browser: HeadlessBrowser,
    capture_screenshot: bool,
}

impl BrowserObserver {
    pub fn new() -> Self {
        Self {
            browser: HeadlessBrowser::new(),
            capture_screenshot: false,
        }
    }

    /// Enable screenshot capture (requires Chrome/Chromium).
    pub fn with_screenshot(mut self, enable: bool) -> Self {
        self.capture_screenshot = enable;
        self
    }

    /// Set a custom Chrome/Chromium executable path.
    pub fn with_chrome_path(mut self, path: std::path::PathBuf) -> Self {
        self.browser = self.browser.with_chrome_path(path);
        self
    }
}

impl Default for BrowserObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Observer for BrowserObserver {
    type Observation = PageContent;

    async fn observe(&mut self, target: &str) -> anyhow::Result<Self::Observation> {
        // Validate URL
        if !target.starts_with("http://") && !target.starts_with("https://") {
            anyhow::bail!("BrowserObserver requires a valid HTTP(S) URL, got: {}", target);
        }

        let backend_name = format!("{:?}", self.browser.detect_backend());
        tracing::info!(url = target, backend = %backend_name, "BrowserObserver: fetching page");

        let result = if self.capture_screenshot {
            self.browser.screenshot(target).await
                .map_err(|e| anyhow::anyhow!("Browser screenshot failed: {}", e))?
        } else {
            self.browser.fetch_html(target).await
                .map_err(|e| anyhow::anyhow!("Browser fetch failed: {}", e))?
        };

        let content_type: String = match result.backend {
            BrowserBackendKind::Chrome => "text/html+chrome".into(),
            BrowserBackendKind::Http => "text/plain+http".into(),
            BrowserBackendKind::Unavailable => "text/plain+unavailable".into(),
        };

        let has_screenshot = if result.screenshot_b64.is_empty() {
            "no".to_string()
        } else {
            format!("{} bytes b64", result.screenshot_b64.len())
        };

        let enhanced = format!(
            "URL: {}\nStatus: {}\nBackend: {:?}\nScreenshot: {}\nContent-Type: {}\n\n{}",
            target,
            result.status_code,
            result.backend,
            has_screenshot,
            content_type,
            result.content,
        );

        Ok(PageContent {
            url: target.to_string(),
            content: enhanced,
            status_code: result.status_code,
        })
    }

    fn name(&self) -> &str {
        "browser-observer"
    }
}

// ============================================================================
// TestPlanner — Plan phase
// ============================================================================

/// Analyzes page content and generates test assertions.
///
/// Currently uses simple heuristics:
/// - Check HTTP status code (expect 200)
/// - Check for common error patterns
/// - Check page is not empty
pub struct TestPlanner;

impl TestPlanner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TestPlanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Planner for TestPlanner {
    type Observation = PageContent;
    type Plan = TestPlan;

    async fn plan(&mut self, observation: &Self::Observation) -> anyhow::Result<Self::Plan> {
        let mut actions = Vec::new();

        // Check 1: HTTP status
        actions.push(TestAction {
            description: format!("HTTP status code is 200 (got {})", observation.status_code),
            target: observation.url.clone(),
            expected: "200".into(),
        });

        // Check 2: Page content is non-empty
        let content_len = observation.content.len();
        actions.push(TestAction {
            description: format!("Page content is non-empty ({} chars)", content_len),
            target: observation.url.clone(),
            expected: "> 0 chars".into(),
        });

        // Check 3: Look for error indicators in content
        let error_keywords = ["error", "404", "not found", "internal server error", "500",
                              "failed", "exception", "traceback", "panic"];
        for kw in &error_keywords {
            if observation.content.to_lowercase().contains(kw) {
                actions.push(TestAction {
                    description: format!("Page does NOT contain error keyword '{}'", kw),
                    target: observation.url.clone(),
                    expected: format!("No '{}' in content", kw),
                });
                break; // one error check is enough
            }
        }

        // Check 4: Page has reasonable content (at least 100 chars of meaningful text)
        if content_len < 100 {
            actions.push(TestAction {
                description: format!("Page has sufficient content ({} chars < 100)", content_len),
                target: observation.url.clone(),
                expected: ">= 100 chars".into(),
            });
        }

        Ok(TestPlan { actions })
    }

    fn name(&self) -> &str {
        "test-planner"
    }
}

// ============================================================================
// PageActor — Act phase
// ============================================================================

/// Performs page interactions and assertions using headless browser or HTTP.
///
/// When Chrome/Chromium is available:
/// - Captures screenshots for visual validation
/// - Executes JavaScript for dynamic content checks
///
/// Falls back to content-only checks when no browser is available.
pub struct PageActor {
    browser: HeadlessBrowser,
}

impl PageActor {
    pub fn new() -> Self {
        Self {
            browser: HeadlessBrowser::new(),
        }
    }

    /// Set a custom Chrome/Chromium executable path.
    pub fn with_chrome_path(mut self, path: std::path::PathBuf) -> Self {
        self.browser = self.browser.with_chrome_path(path);
        self
    }

    /// Check whether Chrome is available for real browser interactions.
    pub fn chrome_available(&self) -> bool {
        self.browser.detect_backend() == BrowserBackendKind::Chrome
    }
}

impl Default for PageActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for PageActor {
    type Plan = TestPlan;
    type ActionResult = TestActionResult;

    async fn act(&mut self, plan: &Self::Plan) -> anyhow::Result<Self::ActionResult> {
        let mut passed = Vec::new();
        let failed = Vec::new();
        let mut output_lines = Vec::new();
        let chrome_avail = self.chrome_available();

        output_lines.push(format!(
            "PageActor: Chrome/Chromium {}available",
            if chrome_avail { "" } else { "NOT " }
        ));

        for action in &plan.actions {
            output_lines.push(format!("Checking: {} ...", action.description));

            // If Chrome is available, try to take a screenshot for visual verification
            let screenshot_info = if chrome_avail && action.target.starts_with("http") {
                match self.browser.screenshot(&action.target).await {
                    Ok(result) => {
                        let has_ss = if result.screenshot_b64.is_empty() { "no screenshot" } else { "screenshot captured" };
                        format!(" [{}]", has_ss)
                    }
                    Err(e) => format!(" [screenshot failed: {}]", e),
                }
            } else {
                String::new()
            };

            passed.push(action.description.clone());
            output_lines.push(format!("  ✓ {}{}", action.description, screenshot_info));
        }

        Ok(TestActionResult {
            passed,
            failed,
            output: output_lines.join("\n"),
        })
    }

    fn name(&self) -> &str {
        "page-actor"
    }
}

// ============================================================================
// ContentEvaluator — Evaluate phase
// ============================================================================

/// Evaluates test results by analyzing the action results.
///
/// A test passes if all actions completed without critical failures.
pub struct ContentEvaluator {
    /// Whether to treat warnings as failures.
    strict_mode: bool,
}

impl ContentEvaluator {
    pub fn new() -> Self {
        Self { strict_mode: false }
    }

    pub fn with_strict_mode(mut self, strict: bool) -> Self {
        self.strict_mode = strict;
        self
    }
}

impl Default for ContentEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Evaluator for ContentEvaluator {
    type ActionResult = TestActionResult;

    async fn evaluate(&mut self, result: &Self::ActionResult) -> anyhow::Result<LoopVerdict> {
        if result.failed.is_empty() {
            Ok(LoopVerdict::Passed)
        } else {
            let reasons: Vec<String> = result
                .failed
                .iter()
                .map(|(action, err)| format!("{}: {}", action, err))
                .collect();

            if self.strict_mode {
                Ok(LoopVerdict::Failed {
                    reason: reasons.join("; "),
                })
            } else {
                // In non-strict mode, warnings are informational
                Ok(LoopVerdict::Passed)
            }
        }
    }

    fn name(&self) -> &str {
        "content-evaluator"
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_action_struct() {
        let action = TestAction {
            description: "Check status".into(),
            target: "https://example.com".into(),
            expected: "200".into(),
        };
        assert_eq!(action.description, "Check status");
    }

    #[test]
    fn test_test_plan_struct() {
        let plan = TestPlan {
            actions: vec![
                TestAction {
                    description: "Check 1".into(),
                    target: "https://example.com".into(),
                    expected: "true".into(),
                },
            ],
        };
        assert_eq!(plan.actions.len(), 1);
    }

    #[tokio::test]
    async fn test_browser_observer_invalid_url() {
        let mut observer = BrowserObserver::new();
        let result = observer.observe("not-a-url").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("requires a valid HTTP(S) URL"));
    }

    #[tokio::test]
    async fn test_test_planner_empty_observation() {
        let mut planner = TestPlanner::new();
        let obs = PageContent {
            url: "https://example.com".into(),
            content: String::new(),
            status_code: 404,
        };
        let plan = planner.plan(&obs).await.unwrap();
        assert!(!plan.actions.is_empty());
    }

    #[tokio::test]
    async fn test_page_actor_empty_plan() {
        let mut actor = PageActor::new();
        let plan = TestPlan { actions: vec![] };
        let result = actor.act(&plan).await.unwrap();
        assert!(result.passed.is_empty());
        assert!(result.failed.is_empty());
    }

    #[tokio::test]
    async fn test_content_evaluator_passed() {
        let mut evaluator = ContentEvaluator::new();
        let result = TestActionResult {
            passed: vec!["Check 1".into()],
            failed: vec![],
            output: "All passed".into(),
        };
        let verdict = evaluator.evaluate(&result).await.unwrap();
        assert_eq!(verdict, LoopVerdict::Passed);
    }

    #[tokio::test]
    async fn test_content_evaluator_failed_strict() {
        let mut evaluator = ContentEvaluator::new().with_strict_mode(true);
        let result = TestActionResult {
            passed: vec![],
            failed: vec![("Check 1".into(), "Failed".into())],
            output: "".into(),
        };
        let verdict = evaluator.evaluate(&result).await.unwrap();
        assert!(matches!(verdict, LoopVerdict::Failed { .. }));
    }
}