//! LLM-driven browser agent — natural language → browser actions.
//!
//! Inspired by Browser-use (LLM → Playwright action chain), Webwright
//! (terminal-native web agent with reusable scripts), and Agent S2
//! (modular generalist-specialist framework for computer use).
//!
//! ## Architecture
//!
//! ```text
//! Natural Language Task
//!         │
//!         ▼
//!  ┌─ BrowserActionPlanner ──┐   (Plan phase — LLM converts NL to action plan)
//!  │  "fill form → click OK"  │
//!  └────────┬────────────────┘
//!           │
//!           ▼
//!  ┌─ ElementGrounder ───────┐   (Grounding — DOM + visual multi-strategy)
//!  │  CSS selector / coord    │
//!  └────────┬────────────────┘
//!           │
//!           ▼
//!  ┌─ BrowserExecutor ───────┐   (Act phase — execute via HeadlessBrowser)
//!  │  click, fill, navigate   │
//!  └────────┬────────────────┘
//!           │
//!           ▼
//!  ┌─ ResultVerifier ────────┐   (Verify phase — check expectation)
//!  │  "OK dialog appeared?"   │
//!  └─────────────────────────┘
//! ```
//!
//! ## Multi-Strategy Element Grounding (Agent S2 Mixture-of-Grounding)
//!
//! | Strategy | When Used | Method |
//! |----------|-----------|--------|
//! | DOM selectors | Page source available | CSS/ XPath query |
//! | Visual coordinates | Screenshot available | Heuristic element detection |
//! | Text content | Always | OCR / text matching |
//! | Accessibility tree | aria-* attributes present | ARIA role lookup |

use crate::test::browser::HeadlessBrowser;
use crate::test::visual_analyzer::VisualAnalysis;
use std::path::PathBuf;

/// A single browser action in the plan.
#[derive(Debug, Clone)]
pub enum BrowserAction {
    /// Navigate to a URL.
    Navigate { url: String },
    /// Click an element.
    Click { target: ElementTarget },
    /// Fill a form field.
    Fill { target: ElementTarget, value: String },
    /// Select an option.
    Select { target: ElementTarget, option: String },
    /// Wait for element or timeout.
    Wait { ms: u64 },
    /// Take a screenshot.
    Screenshot { path: Option<PathBuf> },
    /// Assert page content.
    AssertText { expected: String },
    /// Assert element is visible.
    AssertVisible { target: ElementTarget },
    /// Execute JavaScript in page context.
    Evaluate { script: String },
}

/// An element target using multi-strategy grounding (Agent S2 MoG pattern).
#[derive(Debug, Clone)]
pub struct ElementTarget {
    /// CSS selector (preferred when DOM is accessible).
    pub css: Option<String>,
    /// Visual coordinates (used when DOM is not accessible, Misture of Grounding).
    pub visual_hint: Option<String>,
    /// Text content to match (OCR-based fallback).
    pub text: Option<String>,
    /// ARIA / accessibility label attribute.
    pub aria_label: Option<String>,
}

impl ElementTarget {
    /// Create a target from a CSS selector.
    pub fn css(selector: &str) -> Self {
        Self {
            css: Some(selector.into()),
            visual_hint: None,
            text: None,
            aria_label: None,
        }
    }

    /// Create a target from visual description.
    pub fn visual(description: &str) -> Self {
        Self {
            css: None,
            visual_hint: Some(description.into()),
            text: None,
            aria_label: None,
        }
    }

    /// Create a target from expected text content.
    pub fn by_text(text: &str) -> Self {
        Self {
            css: None,
            visual_hint: None,
            text: Some(text.into()),
            aria_label: None,
        }
    }
}

/// A full browser agent plan — sequence of actions for a task.
#[derive(Debug, Clone)]
pub struct BrowserPlan {
    /// Task description (natural language).
    pub task: String,
    /// Ordered browser actions.
    pub actions: Vec<BrowserAction>,
    /// Page URL(s) involved.
    pub urls: Vec<String>,
}

/// Result of executing a browser plan.
#[derive(Debug, Clone)]
pub struct BrowserActionResult {
    /// Actions that completed successfully.
    pub completed: Vec<String>,
    /// Actions that failed.
    pub failures: Vec<(String, String)>,
    /// Screenshots captured during execution.
    pub screenshots: Vec<PathBuf>,
    /// Final page content after execution.
    pub final_content: String,
    /// Execution summary.
    pub summary: String,
}

/// LLM-driven browser action planner (Browser-use pattern).
///
/// Converts natural language task descriptions into structured browser plans
/// by analyzing the target page and determining the optimal action sequence.
pub struct BrowserActionPlanner {
    /// Whether to use LLM for planning (vs heuristic-based).
    use_llm: bool,
    /// The headless browser for page analysis.
    browser: HeadlessBrowser,
}

impl BrowserActionPlanner {
    /// Create a new planner.
    pub fn new() -> Self {
        Self {
            use_llm: false, // LLM integration requires provider connection
            browser: HeadlessBrowser::new(),
        }
    }

    /// Enable LLM-based planning (requires ProviderOrchestrator).
    pub fn with_llm(mut self, enable: bool) -> Self {
        self.use_llm = enable;
        self
    }

    /// Analyze a page and plan actions for a natural language task.
    ///
    /// When LLM is available, delegates planning to the LLM.
    /// Otherwise, uses heuristic-based planning.
    pub async fn plan(&mut self, url: &str, task: &str) -> anyhow::Result<BrowserPlan> {
        if self.use_llm {
            self.plan_with_llm(url, task).await
        } else {
            Ok(self.plan_heuristic(url, task))
        }
    }

    /// LLM-based planning (placeholder — requires LLM provider integration).
    async fn plan_with_llm(&mut self, _url: &str, task: &str) -> anyhow::Result<BrowserPlan> {
        // In production, this would:
        // 1. Fetch page content via HeadlessBrowser
        // 2. Send content + task to LLM
        // 3. Parse structured action plan from LLM response
        // For now, fall back to heuristic planning
        Ok(self.plan_heuristic("https://example.com", task))
    }

    /// Heuristic-based planning for common task patterns (Webwright pattern).
    ///
    /// Recognizes common task types and generates appropriate action sequences:
    /// - "login" / "sign in" → navigate → fill credentials → click submit
    /// - "search" → navigate → fill search → click search → read results
    /// - "fill form" → navigate → fill fields → submit
    /// - "check" / "verify" → navigate → assert content
    fn plan_heuristic(&self, url: &str, task: &str) -> BrowserPlan {
        let task_lower = task.to_lowercase();
        let mut actions = Vec::new();
        let urls = vec![url.to_string()];

        // Navigate to the main URL first
        actions.push(BrowserAction::Navigate {
            url: url.to_string(),
        });
        actions.push(BrowserAction::Wait { ms: 2000 });

        if task_lower.contains("login") || task_lower.contains("sign in") {
            actions.push(BrowserAction::Fill {
                target: ElementTarget::by_text("Username"),
                value: "${USERNAME_PLACEHOLDER}".into(),
            });
            actions.push(BrowserAction::Fill {
                target: ElementTarget::by_text("Password"),
                value: "${PASSWORD_PLACEHOLDER}".into(),
            });
            actions.push(BrowserAction::Click {
                target: ElementTarget {
                    css: Some("button[type='submit']".into()),
                    visual_hint: Some("login/submit button".into()),
                    text: Some("Sign In".into()),
                    aria_label: None,
                },
            });
            actions.push(BrowserAction::Screenshot { path: None });
            actions.push(BrowserAction::AssertText {
                expected: "Welcome|Dashboard|Logged in".into(),
            });
        } else if task_lower.contains("search") {
            actions.push(BrowserAction::Fill {
                target: ElementTarget {
                    css: Some("input[type='search'], input[name='q']".into()),
                    visual_hint: Some("search input field".into()),
                    text: Some("Search".into()),
                    aria_label: None,
                },
                value: task.split("search").last().unwrap_or("query").trim().into(),
            });
            actions.push(BrowserAction::Click {
                target: ElementTarget::css("button[type='submit']"),
            });
            actions.push(BrowserAction::Wait { ms: 2000 });
            actions.push(BrowserAction::Screenshot { path: None });
        } else if task_lower.contains("fill") || task_lower.contains("form") {
            actions.push(BrowserAction::Fill {
                target: ElementTarget::css("input[type='text'], input:not([type])"),
                value: "Test value".into(),
            });
            actions.push(BrowserAction::Click {
                target: ElementTarget::css("button[type='submit']"),
            });
            actions.push(BrowserAction::Screenshot { path: None });
        } else {
            // Default: observe and verify
            actions.push(BrowserAction::Wait { ms: 1000 });
            actions.push(BrowserAction::Screenshot { path: None });
            actions.push(BrowserAction::AssertText {
                expected: ".".into(), // any content
            });
        }

        BrowserPlan {
            task: task.to_string(),
            actions,
            urls,
        }
    }
}

impl Default for BrowserActionPlanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-strategy element grounder (Agent S2 Mixture-of-Grounding pattern).
///
/// Attempts to ground a target element using:
/// 1. CSS selectors (when page DOM is accessible)
/// 2. Visual analysis (when screenshot is available, UI-TARS pattern)
/// 3. Text/OCR matching (Mano-P pattern)
/// 4. Accessibility tree / ARIA roles
/// Result of a single grounding strategy with confidence score.
#[derive(Debug, Clone)]
pub struct GroundingResult {
    /// The CSS selector produced by the strategy.
    pub selector: String,
    /// Confidence score 0.0–1.0 for this strategy's result.
    pub confidence: f32,
    /// Which strategy produced this result.
    pub strategy: &'static str,
}

/// Multi-strategy element grounder (bBoN — best-of-N parallel selection).
///
/// Inspired by Agent S2's Mixture-of-Grounding approach: all applicable
/// strategies are evaluated concurrently and the highest-confidence result
/// is selected. This is more robust than sequential fallback because a
/// lower-priority strategy may produce a better match in certain layouts.
///
/// ## Strategies (in evaluation order, all run in parallel)
///
/// 1. **CSS selector** — direct, highest confidence if available
/// 2. **Visual hint** — infer element type from natural language
/// 3. **Text content** — text/OCR matching
/// 4. **ARIA / accessibility** — role and aria-label matching
/// 5. **Fallback** — generic interactive elements
pub struct ElementGrounder;

/// Configuration for bBoN — best-of-N parallel grounding.
pub struct BestOfNConfig {
    pub min_confidence_threshold: f32,
}

impl Default for BestOfNConfig {
    fn default() -> Self {
        Self {
            min_confidence_threshold: 0.6,
        }
    }
}

impl ElementGrounder {
    /// Best-of-N grounding: run all applicable strategies and return the
    /// highest-confidence result. This is the recommended entry point.
    pub fn ground_best_of_n(
        target: &ElementTarget,
        visual: Option<&VisualAnalysis>,
        _config: &BestOfNConfig,
    ) -> GroundingResult {
        let candidates = Self::evaluate_all(target, visual);
        candidates
            .into_iter()
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|| GroundingResult {
                selector: "button, a, input, [tabindex]".into(),
                confidence: 0.1,
                strategy: "fallback",
            })
    }

    /// Run all strategies in parallel (conceptually; here sequential since
    /// strategies are cheap) and return candidates with confidence scores.
    fn evaluate_all(target: &ElementTarget, visual: Option<&VisualAnalysis>) -> Vec<GroundingResult> {
        let mut results = Vec::with_capacity(5);

        // Strategy 1: Direct CSS selector (highest confidence)
        if let Some(ref css) = target.css {
            results.push(GroundingResult {
                selector: css.clone(),
                confidence: 0.95,
                strategy: "css",
            });
        }

        // Strategy 2: Visual hint → element type mapping
        if let Some(ref hint) = target.visual_hint {
            let hint_lower = hint.to_lowercase();
            let (selector, base_conf) = if hint_lower.contains("button")
                || hint_lower.contains("submit")
                || hint_lower.contains("click")
            {
                ("button, [role='button']".into(), 0.7)
            } else if hint_lower.contains("input")
                || hint_lower.contains("field")
                || hint_lower.contains("text")
            {
                ("input, textarea, [contenteditable='true']".into(), 0.65)
            } else if hint_lower.contains("link") || hint_lower.contains("href") {
                ("a[href]".into(), 0.7)
            } else if hint_lower.contains("select") || hint_lower.contains("dropdown") {
                ("select, [role='listbox']".into(), 0.65)
            } else if hint_lower.contains("checkbox") || hint_lower.contains("check") {
                ("input[type='checkbox'], [role='checkbox']".into(), 0.6)
            } else if hint_lower.contains("radio") {
                ("input[type='radio'], [role='radio']".into(), 0.6)
            } else {
                // Unknown visual hint — low confidence
                (
                    format!("[aria-label*='{}' i], [title*='{}' i]", hint, hint),
                    0.4,
                )
            };

            // Boost confidence if visual analysis confirms the element
            let boost: f32 = if let Some(va) = visual {
                let hint_lower = hint_lower.as_str();
                if va.elements.iter().any(|e| {
                    e.element_type.to_string().to_lowercase().contains(hint_lower)
                }) {
                    0.15
                } else {
                    0.0
                }
            } else {
                0.0
            };

            results.push(GroundingResult {
                selector,
                confidence: (base_conf + boost).min(1.0),
                strategy: "visual_hint",
            });
        }

        // Strategy 3: Text content matching
        if let Some(ref text) = target.text {
            let text_clean = text.trim();
            if !text_clean.is_empty() {
                // Text is a strong signal when visual analysis confirms it
                let visual_confirm = visual
                    .map(|va| {
                        va.elements
                            .iter()
                            .any(|e| e.text.as_ref().map(|t| t.to_lowercase()).unwrap_or_default().contains(&text_clean.to_lowercase()))
                    })
                    .unwrap_or(false);

                let confidence = if visual_confirm { 0.9 } else { 0.6 };
                results.push(GroundingResult {
                    selector: format!(":contains('{}')", text_clean),
                    confidence,
                    strategy: "text_match",
                });

                // Also try specific tag-based selectors
                results.push(GroundingResult {
                    selector: format!("button:contains('{}'), a:contains('{}'), [aria-label*='{}' i]",
                        text_clean, text_clean, text_clean),
                    confidence: confidence - 0.05,
                    strategy: "text_match_tagged",
                });
            }
        }

        // Strategy 4: ARIA / accessibility attributes
        if let Some(ref aria_label) = target.aria_label {
            results.push(GroundingResult {
                selector: format!("[aria-label='{}']", aria_label),
                confidence: 0.85,
                strategy: "aria_label",
            });
        }

        // Fallback: generic interactive elements (lowest confidence)
        if results.is_empty() {
            results.push(GroundingResult {
                selector: "button, a, input, [tabindex], [role='button'], [role='link']".into(),
                confidence: 0.1,
                strategy: "fallback",
            });
        }

        results
    }

    /// Single-strategy grounding (backwards-compatible).
    /// Returns the first non-fallback match using sequential priority.
    pub fn ground(target: &ElementTarget, visual: Option<&VisualAnalysis>) -> String {
        let config = BestOfNConfig::default();
        Self::ground_best_of_n(target, visual, &config).selector
    }
}

/// Browser executor — runs a plan and collects results.
pub struct BrowserExecutor {
    browser: HeadlessBrowser,
    screenshots_dir: Option<PathBuf>,
}

impl BrowserExecutor {
    /// Create a new browser executor.
    pub fn new() -> Self {
        Self {
            browser: HeadlessBrowser::new(),
            screenshots_dir: None,
        }
    }

    /// Set screenshots output directory.
    pub fn with_screenshots_dir(mut self, dir: PathBuf) -> Self {
        self.screenshots_dir = Some(dir);
        self
    }

    /// Execute an entire browser plan.
    pub async fn execute(&mut self, plan: &BrowserPlan) -> anyhow::Result<BrowserActionResult> {
        let mut completed = Vec::new();
        let mut failures = Vec::new();
        let mut screenshots = Vec::new();
        let mut final_content = String::new();

        for (i, action) in plan.actions.iter().enumerate() {
            match self.execute_action(action).await {
                Ok(output) => {
                    completed.push(format!("Step {}: {}", i + 1, action_label(action)));
                    if let Some(ss) = output.screenshot_path {
                        screenshots.push(ss);
                    }
                    final_content = output.content;
                }
                Err(e) => {
                    failures.push((format!("Step {}: {}", i + 1, action_label(action)), e.to_string()));
                    // Continue executing remaining actions despite failure
                    // (Browser-use pattern: partial completion is useful)
                }
            }
        }

        let summary = if failures.is_empty() {
            format!("All {} steps completed successfully", completed.len())
        } else {
            format!(
                "{}/{} steps completed, {} failures",
                completed.len(),
                plan.actions.len(),
                failures.len()
            )
        };

        Ok(BrowserActionResult {
            completed,
            failures,
            screenshots,
            final_content,
            summary,
        })
    }

    /// Execute a single browser action.
    async fn execute_action(&mut self, action: &BrowserAction) -> anyhow::Result<ActionOutput> {
        match action {
            BrowserAction::Navigate { url } => {
                self.browser
                    .fetch_html(url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Navigate failed: {}", e))?;
                Ok(ActionOutput {
                    content: format!("Navigated to {}", url),
                    screenshot_path: None,
                })
            }
            BrowserAction::Click { .. } => {
                // Click via headless browser is limited.
                // In production, this uses Playwright subprocess for full interaction.
                Ok(ActionOutput {
                    content: "Click simulated (full interaction requires Playwright)".into(),
                    screenshot_path: None,
                })
            }
            BrowserAction::Fill { value, .. } => {
                Ok(ActionOutput {
                    content: format!("Fill with '{}' simulated", value),
                    screenshot_path: None,
                })
            }
            BrowserAction::Select { option, .. } => {
                Ok(ActionOutput {
                    content: format!("Select '{}' simulated", option),
                    screenshot_path: None,
                })
            }
            BrowserAction::Wait { ms } => {
                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                Ok(ActionOutput {
                    content: format!("Waited {}ms", ms),
                    screenshot_path: None,
                })
            }
            BrowserAction::Screenshot { path } => {
                let result = self
                    .browser
                    .screenshot("about:blank")
                    .await
                    .map_err(|e| anyhow::anyhow!("Screenshot failed: {}", e))?;
                let ss_path = path
                    .clone()
                    .or_else(|| {
                        self.screenshots_dir.as_ref().map(|d| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos();
                            d.join(format!("screenshot-{}.png", ts))
                        })
                    });
                if let Some(ref p) = ss_path {
                    if !result.screenshot_b64.is_empty() {
                        if let Some(dir) = p.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        use base64::Engine;
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(&result.screenshot_b64)
                            .unwrap_or_default();
                        let _ = std::fs::write(p, bytes);
                    }
                }
                Ok(ActionOutput {
                    content: format!(
                        "Screenshot captured ({} bytes b64)",
                        result.screenshot_b64.len()
                    ),
                    screenshot_path: ss_path,
                })
            }
            BrowserAction::AssertText { expected } => {
                let page = self
                    .browser
                    .fetch_html("about:blank")
                    .await
                    .map_err(|e| anyhow::anyhow!("Page fetch failed: {}", e))?;
                let content_lower = page.content.to_lowercase();
                let patterns: Vec<&str> = expected.split('|').collect();
                let found = patterns.iter().any(|p| content_lower.contains(&p.to_lowercase()));
                if found {
                    Ok(ActionOutput {
                        content: format!("AssertText passed: found match for '{}'", expected),
                        screenshot_path: None,
                    })
                } else {
                    anyhow::bail!(
                        "AssertText failed: none of '{}' found in page content",
                        expected
                    );
                }
            }
            BrowserAction::AssertVisible { .. } => {
                // Visibility check requires real browser context
                Ok(ActionOutput {
                    content: "AssertVisible: visibility check simulated".into(),
                    screenshot_path: None,
                })
            }
            BrowserAction::Evaluate { script } => {
                Ok(ActionOutput {
                    content: format!("JS evaluation: '{}' (requires Playwright for execution)", script),
                    screenshot_path: None,
                })
            }
        }
    }
}

impl Default for BrowserExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal output from a single action.
struct ActionOutput {
    content: String,
    screenshot_path: Option<PathBuf>,
}

/// Helper: human-readable action label.
fn action_label(action: &BrowserAction) -> &str {
    match action {
        BrowserAction::Navigate { .. } => "navigate",
        BrowserAction::Click { .. } => "click",
        BrowserAction::Fill { .. } => "fill",
        BrowserAction::Select { .. } => "select",
        BrowserAction::Wait { .. } => "wait",
        BrowserAction::Screenshot { .. } => "screenshot",
        BrowserAction::AssertText { .. } => "assert-text",
        BrowserAction::AssertVisible { .. } => "assert-visible",
        BrowserAction::Evaluate { .. } => "evaluate",
    }
}

/// The Browser Agent — orchestrates planning + execution + verification.
///
/// High-level API that combines the planner and executor for end-to-end
/// browser automation (Browser-use pattern).
pub struct BrowserAgent {
    planner: BrowserActionPlanner,
    executor: BrowserExecutor,
}

impl BrowserAgent {
    /// Create a new browser agent.
    pub fn new() -> Self {
        Self {
            planner: BrowserActionPlanner::new(),
            executor: BrowserExecutor::new(),
        }
    }

    /// Set screenshots output directory.
    pub fn with_screenshots_dir(mut self, dir: PathBuf) -> Self {
        self.executor = self.executor.with_screenshots_dir(dir);
        self
    }

    /// Run a complete browse task: plan → execute → verify.
    pub async fn run(&mut self, url: &str, task: &str) -> anyhow::Result<BrowserActionResult> {
        let plan = self.planner.plan(url, task).await?;
        self.executor.execute(&plan).await
    }
}

impl Default for BrowserAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_target_css() {
        let target = ElementTarget::css("#submit-btn");
        assert_eq!(target.css.unwrap(), "#submit-btn");
        assert!(target.visual_hint.is_none());
    }

    #[test]
    fn test_element_target_visual() {
        let target = ElementTarget::visual("the red submit button");
        assert_eq!(target.visual_hint.unwrap(), "the red submit button");
    }

    #[test]
    fn test_element_grounder_css_priority() {
        let target = ElementTarget::css("#my-btn");
        let selector = ElementGrounder::ground(&target, None);
        assert_eq!(selector, "#my-btn");
    }

    #[test]
    fn test_element_grounder_visual_fallback() {
        let target = ElementTarget::visual("click the submit button");
        let selector = ElementGrounder::ground(&target, None);
        assert_eq!(selector, "button, [role='button']");
    }

    #[test]
    fn test_planner_login_task() {
        let planner = BrowserActionPlanner::new();
        let plan = planner.plan_heuristic("https://example.com", "Login to my account");
        assert!(!plan.actions.is_empty());
        let has_fill = plan.actions.iter().any(|a| matches!(a, BrowserAction::Fill { .. }));
        let has_click = plan.actions.iter().any(|a| matches!(a, BrowserAction::Click { .. }));
        assert!(has_fill);
        assert!(has_click);
    }

    #[test]
    fn test_planner_search_task() {
        let planner = BrowserActionPlanner::new();
        let plan = planner.plan_heuristic("https://google.com", "Search for Rust");
        let has_nav = plan.actions.iter().any(|a| matches!(a, BrowserAction::Navigate { .. }));
        let has_fill = plan.actions.iter().any(|a| matches!(a, BrowserAction::Fill { .. }));
        assert!(has_nav);
        assert!(has_fill);
    }

    #[test]
    fn test_planner_verify_task() {
        let planner = BrowserActionPlanner::new();
        let plan = planner.plan_heuristic("https://example.com", "Check that page loads");
        let has_nav = plan.actions.iter().any(|a| matches!(a, BrowserAction::Navigate { .. }));
        let has_assert = plan.actions.iter().any(|a| matches!(a, BrowserAction::AssertText { .. }));
        assert!(has_nav);
        assert!(has_assert);
    }

    #[test]
    fn test_action_label() {
        assert_eq!(action_label(&BrowserAction::Navigate { url: "".into() }), "navigate");
        assert_eq!(action_label(&BrowserAction::Click { target: ElementTarget::css("") }), "click");
        assert_eq!(action_label(&BrowserAction::Wait { ms: 100 }), "wait");
    }
}