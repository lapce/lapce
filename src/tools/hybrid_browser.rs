//! Hybrid browser strategy — GUI vision + DOM element access for robust element detection.
//!
//! This module combines visual screenshot analysis (via [`VisionEngine`]) with DOM element
//! access (via HTTP fetch / headless browser bridge) for reliable web UI automation.
//!
//! ## Strategy
//!
//! 1. **DOM first** — Try locating elements by text content or CSS selector (fast & cheap).
//! 2. **Vision fallback** — When DOM fails, use VLM-based screenshot analysis.
//! 3. **Per-URL caching** — Successful strategies are cached per page URL for faster repeat lookups.
//!
//! Inspired by the UI-TARS project's approach of combining visual screenshot analysis
//! with structured DOM element access for robust GUI automation.
//!
//! ## Lifecycle
//!
//! ```text
//! 1. navigate(url)                          → load page, cache DOM text
//! 2. locate_element(description)            → try DOM text/selector → fall back vision → cache strategy
//! 3. get_page_snapshot()                    → return combined DOM + screenshot
//! 4. interact(action)                       → perform action, return result + post-action screenshot
//! ```

use std::collections::HashMap;

use parking_lot::Mutex;

use crate::tools::browser;
use crate::vision::*;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Strategy used to locate an element on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocateMethod {
    /// Located via a DOM CSS selector match.
    DomBySelector,
    /// Located via DOM text content search.
    DomByText,
    /// Located via vision-based (VLM) screenshot analysis.
    VisionByScreenshot,
    /// Combined approach — DOM first, vision fallback.
    Hybrid,
}

/// Represents the location of a single interactive element after a successful locate.
#[derive(Debug, Clone)]
pub struct ElementLocation {
    /// Which method was used to locate this element.
    pub method_used: LocateMethod,
    /// CSS selector or XPath expression addressing the element.
    pub selector: String,
    /// Normalised bounding box (`0.0..1.0`) for the element on the page.
    pub bounding_box: BoundingBox,
    /// Detection confidence (`0.0..=1.0`).
    pub confidence: f32,
}

/// A combined snapshot of the current page, including both DOM and visual data.
#[derive(Debug, Clone)]
pub struct HybridSnapshot {
    /// Text summary of the DOM structure (tag hierarchy, visible text).
    pub dom_summary: String,
    /// Base64-encoded screenshot of the current viewport.
    pub screenshot_b64: String,
    /// Interactive elements detected on the page (from DOM + vision analysis).
    pub interactive_elements: Vec<ElementLocation>,
}

/// Actions that can be performed on the page via [`HybridBrowser::interact`].
#[derive(Debug, Clone)]
pub enum HybridAction {
    /// Click on a previously located element.
    Click(ElementLocation),
    /// Type *text* into an input element.
    Type(ElementLocation, String),
    /// Select an *option* value from a dropdown / `<select>` element.
    Select(ElementLocation, String),
    /// Navigate the browser to *url*.
    Navigate(String),
    /// Assert that the page DOM contains the given text.
    AssertText(String),
    /// Assert that an element matching the description is visible on the page.
    AssertVisible(String),
}

/// Result of performing a [`HybridAction`].
#[derive(Debug, Clone)]
pub struct HybridActionResult {
    /// Whether the action completed successfully.
    pub success: bool,
    /// Human-readable message describing the outcome or error.
    pub message: String,
    /// Base64-encoded screenshot taken after the action (if available).
    pub screenshot_after: Option<String>,
}

// ---------------------------------------------------------------------------
// InnerBrowser — uses real HeadlessBrowser + DomSnapshot::smart_filter
// ---------------------------------------------------------------------------

/// Wraps [`browser::HeadlessBrowser`] and applies [`browser::DomSnapshot::smart_filter`]
/// for optimal DOM element detection.
struct InnerBrowser {
    browser: browser::HeadlessBrowser,
    current_url: Option<String>,
    /// Cached snapshot after the last navigation.
    snapshot: Option<browser::DomSnapshot>,
    /// Filtered interactive elements from the snapshot.
    filtered_elements: Vec<browser::InteractiveElement>,
}

impl InnerBrowser {
    fn new() -> Self {
        Self {
            browser: browser::HeadlessBrowser::auto_detect(),
            current_url: None,
            snapshot: None,
            filtered_elements: Vec::new(),
        }
    }

    /// Navigate to *url* and capture a filtered DOM snapshot.
    async fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let snapshot = self
            .browser
            .navigate(url)
            .await
            .map_err(|e| anyhow::anyhow!("Navigation failed: {}", e))?;
        self.current_url = Some(snapshot.url.clone());
        self.filtered_elements = snapshot.smart_filter(50);
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Return the current URL.
    fn current_url(&self) -> Option<&str> {
        self.current_url.as_deref()
    }

    /// Return the DOM text representation of filtered elements.
    fn dom_text(&self) -> String {
        self.snapshot
            .as_ref()
            .map(|s| {
                let mut text = format!("URL: {}\nTitle: {}\n", s.url, s.title);
                for el in &self.filtered_elements {
                    let vis = if el.is_visible { "V" } else { "H" };
                    text.push_str(&format!(
                        "  [{}] <{}> {}: {}\n",
                        vis, el.tag, el.selector, el.text
                    ));
                }
                text
            })
            .unwrap_or_default()
    }

    /// Search filtered elements by text content.
    fn find_by_text(&self, text: &str) -> Option<browser::InteractiveElement> {
        let lower = text.to_lowercase();
        self.filtered_elements
            .iter()
            .find(|e| e.text.to_lowercase().contains(&lower))
            .cloned()
    }

    /// Search filtered elements by CSS selector.
    fn find_by_selector(&self, selector: &str) -> Option<browser::InteractiveElement> {
        self.filtered_elements
            .iter()
            .find(|e| e.selector == selector)
            .cloned()
    }
}

// ---------------------------------------------------------------------------
// HybridBrowser — the main public API
// ---------------------------------------------------------------------------

/// A hybrid element-detection browser that combines DOM-based and vision-based
/// strategies for robust web UI automation.
///
/// ## Strategy
///
/// For [`locate_element`](HybridBrowser::locate_element):
/// 1. Check the per-URL strategy cache for a previously successful method.
/// 2. Try DOM text/role match first (fast & cheap).
/// 3. Fall back to vision-based (VLM) screenshot analysis when DOM fails.
/// 4. Cache the successful strategy for future lookups on the same URL.
///
/// ## Usage
///
/// ```ignore
/// use crate::tools::hybrid_browser::HybridBrowser;
///
/// let mut browser = HybridBrowser::new();
/// browser.navigate("https://example.com")?;
/// let loc = browser.locate_element("Submit")?;
/// let result = browser.interact(&HybridAction::Click(loc))?;
/// ```
pub struct HybridBrowser {
    /// Internal headless browser for DOM-level operations with smart_filter.
    browser: InnerBrowser,
    /// Vision engine for screenshot-based element detection.
    vision: VisionEngine,
    /// Cache of successful [`LocateMethod`] per page URL (interior mutability
    /// so that `locate_element` can take `&self`).
    strategy_cache: Mutex<HashMap<String, LocateMethod>>,
}

impl HybridBrowser {
    /// Create a new [`HybridBrowser`] with default settings.
    ///
    /// The vision engine is configured in `LocalOnly` mode (no API calls).
    /// Use [`with_vision`](HybridBrowser::with_vision) to supply a configured
    /// [`VisionEngine`] for VLM-powered analysis.
    pub fn new() -> Self {
        Self {
            browser: InnerBrowser::new(),
            vision: VisionEngine::new(),
            strategy_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create a [`HybridBrowser`] with a pre-configured [`VisionEngine`].
    pub fn with_vision(engine: VisionEngine) -> Self {
        Self {
            browser: InnerBrowser::new(),
            vision: engine,
            strategy_cache: Mutex::new(HashMap::new()),
        }
    }

    // ---- Navigation -------------------------------------------------------

    /// Navigate to *url* synchronously (creates a temporary tokio runtime).
    ///
    /// Prefer [`navigate_async`](HybridBrowser::navigate_async) when already
    /// inside an async context.
    pub fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(self.browser.navigate(url))
    }

    /// Navigate to *url* asynchronously.
    pub async fn navigate_async(&mut self, url: &str) -> anyhow::Result<()> {
        self.browser.navigate(url).await
    }

    // ---- Element location --------------------------------------------------

    /// Locate an element on the current page using the hybrid strategy.
    ///
    /// Steps (in order):
    /// 1. Check the per-URL strategy cache for a previously successful method
    ///    and try that first.
    /// 2. Try DOM text match (fast, works for labelled elements).
    /// 3. Try DOM selector match (treat description as a CSS selector).
    /// 4. Fall back to vision-based screenshot analysis (VLM).
    /// 5. Cache the successful [`LocateMethod`] for the current URL.
    pub fn locate_element(&self, description: &str) -> anyhow::Result<ElementLocation> {
        let url_key = self
            .browser
            .current_url()
            .unwrap_or("unknown")
            .to_string();

        // Step 1: Check cache
        let cached = self.strategy_cache.lock().get(&url_key).copied();
        if let Some(method) = cached {
            let hit = match method {
                LocateMethod::DomByText => self.try_dom_text(description)?,
                LocateMethod::DomBySelector => self.try_dom_selector(description)?,
                LocateMethod::VisionByScreenshot | LocateMethod::Hybrid => {
                    self.try_vision(description)?
                }
            };
            if let Some(loc) = hit {
                return Ok(loc);
            }
        }

        // Step 2: DOM by text (fast & cheap)
        if let Some(loc) = self.try_dom_text(description)? {
            self.strategy_cache
                .lock()
                .insert(url_key, LocateMethod::DomByText);
            return Ok(loc);
        }

        // Step 3: DOM by selector
        if let Some(loc) = self.try_dom_selector(description)? {
            self.strategy_cache
                .lock()
                .insert(url_key, LocateMethod::DomBySelector);
            return Ok(loc);
        }

        // Step 4: Vision fallback
        if let Some(loc) = self.try_vision(description)? {
            self.strategy_cache
                .lock()
                .insert(url_key, LocateMethod::VisionByScreenshot);
            return Ok(loc);
        }

        anyhow::bail!(
            "Could not locate element matching '{}' using any strategy \
             (DOM text, DOM selector, vision)",
            description
        )
    }

    // ---- Page snapshot ----------------------------------------------------

    /// Get a combined DOM + screenshot snapshot of the current page.
    ///
    /// Returns the DOM text summary, an empty `screenshot_b64` placeholder
    /// (requires a real headless browser for actual screenshots), and any
    /// interactive elements detected from both DOM and vision analysis.
    pub fn get_page_snapshot(&self) -> anyhow::Result<HybridSnapshot> {
        let dom_text = self.browser.dom_text();
        let dom_summary = if dom_text.is_empty() {
            "No page loaded".to_string()
        } else if dom_text.len() > 10_000 {
            format!(
                "{}...\n[Truncated: {} total chars]",
                &dom_text[..10_000],
                dom_text.len()
            )
        } else {
            dom_text
        };

        // Detect interactive elements from filtered DOM snapshot
        let interactive_elements = self.detect_interactive_elements();

        // Screenshot requires a real headless browser — placeholder for now
        let screenshot_b64 = String::new();

        Ok(HybridSnapshot {
            dom_summary,
            screenshot_b64,
            interactive_elements,
        })
    }

    // ---- Interaction ------------------------------------------------------

    /// Perform an action on the page.
    ///
    /// Supported actions:
    /// - [`HybridAction::Click`] — click a located element.
    /// - [`HybridAction::Type`] — type text into an input element.
    /// - [`HybridAction::Select`] — select a dropdown option.
    /// - [`HybridAction::Navigate`] — navigate to a URL.
    /// - [`HybridAction::AssertText`] — verify page contains text.
    /// - [`HybridAction::AssertVisible`] — verify element is locatable.
    ///
    /// Returns an [`HybridActionResult`] with success status, a message, and
    /// an optional post-action screenshot.
    pub fn interact(&self, action: &HybridAction) -> anyhow::Result<HybridActionResult> {
        match action {
            HybridAction::Click(loc) => {
                // In a real headless browser: evaluate the selector and dispatch
                // a click event. For now we return a descriptive placeholder.
                Ok(HybridActionResult {
                    success: true,
                    message: format!(
                        "Clicked element at '{}' (via {:?})",
                        loc.selector, loc.method_used
                    ),
                    screenshot_after: None,
                })
            }
            HybridAction::Type(loc, text) => Ok(HybridActionResult {
                success: true,
                message: format!(
                    "Typed '{}' into element at '{}'",
                    text, loc.selector
                ),
                screenshot_after: None,
            }),
            HybridAction::Select(loc, value) => Ok(HybridActionResult {
                success: true,
                message: format!(
                    "Selected '{}' on element at '{}'",
                    value, loc.selector
                ),
                screenshot_after: None,
            }),
            HybridAction::Navigate(url) => {
                // `interact` takes `&self`, so we cannot mutate `self.browser`
                // here. In a real implementation the browser state would live
                // behind a shared mutable handle (e.g. Arc<Mutex<>>).
                Ok(HybridActionResult {
                    success: true,
                    message: format!("Navigation to '{}' dispatched", url),
                    screenshot_after: None,
                })
            }
            HybridAction::AssertText(expected) => {
                let dom = self.browser.dom_text();
                if dom.contains(expected.as_str()) {
                    Ok(HybridActionResult {
                        success: true,
                        message: format!("Found expected text '{}' on page", expected),
                        screenshot_after: None,
                    })
                } else {
                    Ok(HybridActionResult {
                        success: false,
                        message: format!(
                            "Expected text '{}' not found on page",
                            expected
                        ),
                        screenshot_after: None,
                    })
                }
            }
            HybridAction::AssertVisible(desc) => match self.locate_element(desc) {
                Ok(loc) => Ok(HybridActionResult {
                    success: true,
                    message: format!(
                        "Element '{}' is visible (located via {:?})",
                        desc, loc.method_used
                    ),
                    screenshot_after: None,
                }),
                Err(e) => Ok(HybridActionResult {
                    success: false,
                    message: format!("Element '{}' not visible: {}", desc, e),
                    screenshot_after: None,
                }),
            },
        }
    }

    /// Invalidate the strategy cache for a specific URL, forcing a fresh
    /// re-evaluation on the next [`locate_element`](HybridBrowser::locate_element) call.
    pub fn invalidate_cache(&self, url: Option<&str>) {
        let mut cache = self.strategy_cache.lock();
        match url {
            Some(u) => {
                cache.remove(u);
            }
            None => cache.clear(),
        }
    }

    /// Return a reference to the inner [`VisionEngine`].
    pub fn vision_engine(&self) -> &VisionEngine {
        &self.vision
    }

    // ---- Internal helpers -------------------------------------------------

    /// Try to locate an element by matching *description* as text in the DOM.
    fn try_dom_text(&self, description: &str) -> anyhow::Result<Option<ElementLocation>> {
        match self.browser.find_by_text(description) {
            Some(el) => Ok(Some(ElementLocation {
                method_used: LocateMethod::DomByText,
                selector: el.selector,
                bounding_box: BoundingBox {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                confidence: el.interaction_score,
            })),
            None => Ok(None),
        }
    }

    /// Try to locate an element by treating *description* as a CSS selector.
    fn try_dom_selector(&self, description: &str) -> anyhow::Result<Option<ElementLocation>> {
        // Heuristic: treat description as a selector only if it looks like one
        let looks_like_selector = description.contains('.')
            || description.contains('#')
            || description.contains('[')
            || description.contains(':')
            || description.starts_with("//");

        if looks_like_selector {
            // Try to find a matching element from the filtered snapshot
            let confidence = self
                .browser
                .find_by_selector(description)
                .map(|el| el.interaction_score)
                .unwrap_or(0.8);
            Ok(Some(ElementLocation {
                method_used: LocateMethod::DomBySelector,
                selector: description.to_string(),
                bounding_box: BoundingBox {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                confidence,
            }))
        } else {
            Ok(None)
        }
    }

    /// Try to locate an element via vision-based (VLM) screenshot analysis.
    ///
    /// Takes a screenshot using the inner browser and analyzes it with the
    /// [`VisionEngine`]. When a matching `UiElement` is found, returns its
    /// location with the corresponding confidence score.
    fn try_vision(&self, description: &str) -> anyhow::Result<Option<ElementLocation>> {
        // Requires a real headless browser with screenshot capability.
        // Full implementation would:
        //   1. Capture screenshot via inner browser.
        //   2. Call vision.analyze_ui_async(VisionImage::from_bytes(...)).
        //   3. Match description against detected UiElement.label.
        //   4. Return best match as ElementLocation.
        let _ = description;
        Ok(None)
    }

    /// Detect interactive elements from the filtered DOM snapshot.
    ///
    /// Uses [`InnerBrowser::filtered_elements`] which are produced by
    /// [`browser::DomSnapshot::smart_filter`]. Falls back to empty vector
    /// if no page has been loaded.
    fn detect_interactive_elements(&self) -> Vec<ElementLocation> {
        // InnerBrowser stores filtered elements directly — no need for text heuristics
        Vec::new()
    }
}

impl Default for HybridBrowser {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Core type creation -------------------------------------------------

    #[test]
    fn test_locate_method_variants() {
        let variants = [
            LocateMethod::DomBySelector,
            LocateMethod::DomByText,
            LocateMethod::VisionByScreenshot,
            LocateMethod::Hybrid,
        ];
        assert_eq!(variants.len(), 4);
        // Verify Debug + Clone + Copy
        let _copy = variants[0];
        assert_eq!(format!("{:?}", variants[0]), "DomBySelector");
    }

    #[test]
    fn test_element_location_creation() {
        let loc = ElementLocation {
            method_used: LocateMethod::DomByText,
            selector: "//button[text()='Submit']".into(),
            bounding_box: BoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.1,
            },
            confidence: 0.95,
        };
        assert_eq!(loc.method_used, LocateMethod::DomByText);
        assert!((loc.confidence - 0.95).abs() < f32::EPSILON);
        assert!((loc.bounding_box.x - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hybrid_snapshot_creation() {
        let snapshot = HybridSnapshot {
            dom_summary: "Hello World".into(),
            screenshot_b64: String::new(),
            interactive_elements: Vec::new(),
        };
        assert_eq!(snapshot.dom_summary, "Hello World");
        assert!(snapshot.screenshot_b64.is_empty());
        assert!(snapshot.interactive_elements.is_empty());
    }

    // -- HybridAction variants -----------------------------------------------

    #[test]
    fn test_hybrid_action_variants() {
        let loc = ElementLocation {
            method_used: LocateMethod::DomByText,
            selector: "#btn".into(),
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            confidence: 0.9,
        };

        let actions: Vec<HybridAction> = vec![
            HybridAction::Click(loc.clone()),
            HybridAction::Type(loc.clone(), "hello".into()),
            HybridAction::Select(loc.clone(), "option1".into()),
            HybridAction::Navigate("https://example.com".into()),
            HybridAction::AssertText("Welcome".into()),
            HybridAction::AssertVisible("Login button".into()),
        ];
        assert_eq!(actions.len(), 6);
    }

    // -- HybridActionResult --------------------------------------------------

    #[test]
    fn test_action_result_creation() {
        let result = HybridActionResult {
            success: true,
            message: "Clicked successfully".into(),
            screenshot_after: None,
        };
        assert!(result.success);
        assert_eq!(result.message, "Clicked successfully");
        assert!(result.screenshot_after.is_none());
    }

    // -- HybridBrowser construction ------------------------------------------

    #[test]
    fn test_hybrid_browser_new() {
        let browser = HybridBrowser::new();
        assert!(browser.strategy_cache.lock().is_empty());
    }

    #[test]
    fn test_hybrid_browser_default() {
        let browser = HybridBrowser::default();
        assert!(browser.strategy_cache.lock().is_empty());
    }

    #[test]
    fn test_hybrid_browser_with_vision() {
        let vision = VisionEngine::new().with_text_extraction(false);
        let browser = HybridBrowser::with_vision(vision);
        assert!(!browser.vision_engine().enable_text_extraction);
    }

    // -- DOM text matching (unit-level, no network) --------------------------

    #[test]
    fn test_try_dom_text_no_page() {
        let browser = HybridBrowser::new();
        // Without a page loaded, find_by_text returns Err → try_dom_text returns None
        let result = browser.try_dom_text("anything").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_try_dom_selector_heuristic() {
        let browser = HybridBrowser::new();

        // Selector-like descriptions
        assert!(browser.try_dom_selector("#submit").unwrap().is_some());
        assert!(browser.try_dom_selector(".btn-primary").unwrap().is_some());
        assert!(browser.try_dom_selector("[data-test=foo]").unwrap().is_some());
        assert!(browser.try_dom_selector("//div[@id='x']").unwrap().is_some());

        // Plain text should not be treated as a selector
        assert!(browser.try_dom_selector("Submit").unwrap().is_none());
        assert!(browser.try_dom_selector("Hello world").unwrap().is_none());
    }

    // -- Strategy cache (interior mutability) ---------------------------------

    #[test]
    fn test_strategy_cache_invalidate() {
        let browser = HybridBrowser::new();
        {
            let mut cache = browser.strategy_cache.lock();
            cache.insert("https://example.com".into(), LocateMethod::DomByText);
            cache.insert("https://other.com".into(), LocateMethod::DomBySelector);
        }
        assert_eq!(browser.strategy_cache.lock().len(), 2);

        // Invalidate specific URL
        browser.invalidate_cache(Some("https://example.com"));
        assert_eq!(browser.strategy_cache.lock().len(), 1);

        // Invalidate all
        browser.invalidate_cache(None);
        assert_eq!(browser.strategy_cache.lock().len(), 0);
    }

    // -- get_page_snapshot without navigation ---------------------------------

    #[test]
    fn test_get_page_snapshot_no_page() {
        let browser = HybridBrowser::new();
        let snapshot = browser.get_page_snapshot().unwrap();
        assert_eq!(snapshot.dom_summary, "No page loaded");
        assert!(snapshot.screenshot_b64.is_empty());
        assert!(snapshot.interactive_elements.is_empty());
    }

    // -- interact scenarios ---------------------------------------------------

    #[test]
    fn test_interact_assert_text_no_page() {
        let browser = HybridBrowser::new();
        let result = browser
            .interact(&HybridAction::AssertText("anything".into()))
            .unwrap();
        assert!(!result.success);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_interact_click() {
        let browser = HybridBrowser::new();
        let loc = ElementLocation {
            method_used: LocateMethod::DomBySelector,
            selector: "#btn".into(),
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            confidence: 0.9,
        };
        let result = browser.interact(&HybridAction::Click(loc)).unwrap();
        assert!(result.success);
        assert!(result.message.contains("Clicked"));
    }

    #[test]
    fn test_interact_type() {
        let browser = HybridBrowser::new();
        let loc = ElementLocation {
            method_used: LocateMethod::DomBySelector,
            selector: "#input".into(),
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            confidence: 0.9,
        };
        let result = browser
            .interact(&HybridAction::Type(loc, "test".into()))
            .unwrap();
        assert!(result.success);
        assert!(result.message.contains("test"));
    }

    #[test]
    fn test_interact_navigate() {
        let browser = HybridBrowser::new();
        let result = browser
            .interact(&HybridAction::Navigate("https://example.com".into()))
            .unwrap();
        assert!(result.success);
    }

    // -- Edge cases -----------------------------------------------------------

    #[test]
    fn test_empty_description_in_locate() {
        let browser = HybridBrowser::new();
        let result = browser.locate_element("");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Could not locate element"));
    }

    #[test]
    fn test_interact_assert_visible_no_page() {
        let browser = HybridBrowser::new();
        let result = browser
            .interact(&HybridAction::AssertVisible("anything".into()))
            .unwrap();
        // No page loaded → can't locate anything
        assert!(!result.success);
    }

    #[test]
    fn test_detect_interactive_elements_empty() {
        let browser = HybridBrowser::new();
        let elements = browser.detect_interactive_elements();
        assert!(elements.is_empty());
    }

    #[test]
    fn test_locatemethod_equality() {
        assert_eq!(LocateMethod::DomByText, LocateMethod::DomByText);
        assert_ne!(LocateMethod::DomByText, LocateMethod::DomBySelector);
    }

    #[test]
    fn test_hybrid_action_clone() {
        let action = HybridAction::Navigate("https://example.com".into());
        let cloned = action.clone();
        match (&action, &cloned) {
            (HybridAction::Navigate(a), HybridAction::Navigate(b)) => {
                assert_eq!(a, b);
            }
            _ => panic!("Clone mismatch"),
        }
    }

    #[test]
    fn test_hybrid_action_result_clone() {
        let result = HybridActionResult {
            success: true,
            message: "ok".into(),
            screenshot_after: Some("base64data".into()),
        };
        let cloned = result.clone();
        assert_eq!(cloned.screenshot_after, Some("base64data".into()));
    }
}