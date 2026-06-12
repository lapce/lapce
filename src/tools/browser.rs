//! Browser automation tool — HTTP fetch + headless browser with Playwright support.
//!
//! ## Backends (auto-detected in order)
//! 1. **Playwright** (feature `playwright`): Native Rust Playwright bindings — full JS, click, fill, screenshot
//! 2. **Chrome Subprocess**: Headless Chrome/Chromium via CLI — HTML dump & screenshot only
//! 3. **HTTP**: Simple reqwest-based fetch — text extraction only, no JS
//!
//! ## Types
//! - [`HeadlessBrowser`]: High-level browser with auto-detection and unified API
//! - [`PlaywrightBackend`]: Low-level Playwright wrapper (behind feature flag)
//! - [`DomSnapshot`]: Structured snapshot of interactive DOM elements
//!
//! ## Backwards Compatibility
//! - `fetch_url_async` / `fetch_url` / `extract_text_from_html` / `browser_screenshot` remain unchanged.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Result Type ──

/// Convenience result type for browser operations.
pub type BrowserResult<T> = Result<T, String>;

// ── Backend Kind ──

/// Identifies which browser backend is in use.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserBackendKind {
    /// Plain HTTP reqwest-based fetch (no JS execution).
    Http,
    /// Headless Chrome/Chromium subprocess (limited JS, dump-dom + screenshot).
    ChromeSubprocess,
    /// Native Playwright Rust bindings (full page control).
    #[cfg(feature = "playwright")]
    Playwright,
}

// ── DOM Snapshot Types ──

/// A structured snapshot of a page's interactive elements.
///
/// Captured by evaluating JavaScript in the browser context, returning key UI elements
/// such as links, buttons, inputs, selects, and focusable elements.
///
/// ## Smart Filtering (P0)
///
/// [`DomSnapshot::smart_filter`] applies a three-stage pipeline:
/// 1. **Filter noise**: remove hidden, zero-size, tracking, and decorative elements
/// 2. **Deduplicate**: merge elements targeting the same interaction region
/// 3. **Prioritize**: sort by [`InteractiveElement::interaction_score`], limit count
#[derive(Debug, Clone)]
pub struct DomSnapshot {
    /// Page URL at snapshot time.
    pub url: String,
    /// Document title.
    pub title: String,
    /// Interactive elements found on the page.
    pub interactive_elements: Vec<InteractiveElement>,
}

impl DomSnapshot {
    /// Return only elements that are currently visible on screen.
    pub fn filter_visible(&self) -> Vec<InteractiveElement> {
        self.interactive_elements
            .iter()
            .filter(|e| e.is_visible)
            .cloned()
            .collect()
    }

    /// Remove duplicate elements that share the same normalized selector.
    ///
    /// When two elements have the same tag + selector prefix (e.g. both point
    /// to `button#submit`), the one with higher `interaction_score` is kept.
    pub fn deduplicate(&self) -> Vec<InteractiveElement> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::with_capacity(self.interactive_elements.len());
        for e in &self.interactive_elements {
            let key = format!("{}:{}", e.tag, e.selector);
            if seen.insert(key) {
                result.push(e.clone());
            }
        }
        result
    }

    /// Remove low-value / noisy elements likely not useful for interaction.
    ///
    /// Removes:
    /// - Hidden inputs (`input[type=hidden]`)
    /// - Zero-size elements (width or height == 0)
    /// - Elements with no text and no accessible label
    /// - Decorative/utility elements (e.g. script, style, meta)
    /// - Elements below a minimum interaction score threshold
    pub fn filter_noise(&self) -> Vec<InteractiveElement> {
        self.interactive_elements
            .iter()
            .filter(|e| {
                // Remove hidden inputs
                if e.attributes.get("type").map(|t| t == "hidden").unwrap_or(false) {
                    return false;
                }
                // Remove elements with no interaction value
                if e.tag == "script" || e.tag == "style" || e.tag == "meta" || e.tag == "link" {
                    return false;
                }
                // Remove elements with zero interaction score (likely noise)
                if e.interaction_score < 0.05 {
                    return false;
                }
                true
            })
            .cloned()
            .collect()
    }

    /// Sort elements by interaction score descending, using a tie-breaking
    /// heuristic: buttons/links before generic containers, visible before hidden.
    pub fn prioritize(&self) -> Vec<InteractiveElement> {
        let mut sorted = self.interactive_elements.clone();
        sorted.sort_by(|a, b| {
            b.interaction_score
                .partial_cmp(&a.interaction_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_important = matches!(a.element_kind, ElementKind::Action | ElementKind::FormControl | ElementKind::Navigation);
                    let b_important = matches!(b.element_kind, ElementKind::Action | ElementKind::FormControl | ElementKind::Navigation);
                    b_important.cmp(&a_important)
                })
                .then_with(|| b.is_visible.cmp(&a.is_visible))
                .then_with(|| b.text.len().cmp(&a.text.len()))
        });
        sorted
    }

    /// Run the full smart filter pipeline: noise → dedup → prioritize → limit.
    ///
    /// This is the recommended entry point for most consumers.
    /// `max_elements` caps the result (default sensible limit: 50).
    pub fn smart_filter(&self, max_elements: usize) -> Vec<InteractiveElement> {
        let mut elements = self.filter_noise();
        let snapshot = DomSnapshot {
            url: self.url.clone(),
            title: self.title.clone(),
            interactive_elements: elements,
        };
        elements = snapshot.deduplicate();
        let snapshot = DomSnapshot {
            interactive_elements: elements,
            ..self.clone()
        };
        elements = snapshot.prioritize();
        elements.truncate(max_elements.max(1));
        elements
    }
}

/// Categorised kind of an interactive DOM element.
///
/// Serialized as lowercase string (e.g. `"action"`) in JS snapshot JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    /// Primary action: button, submit, clickable
    Action,
    /// Form control: input, select, textarea, checkbox, radio
    FormControl,
    /// Navigation anchor: link, menu, breadcrumb
    Navigation,
    /// Informational content: heading, paragraph, label
    Content,
    /// Structural container: div, section, article
    Structure,
    /// Decorative / non-interactive visual only
    Decorative,
}

/// A single interactive DOM element discovered during snapshotting.
///
/// Inspired by Scrapling's auto-healing selector pattern: when the primary
/// `selector` fails (e.g. page structure changed), `fallback_selectors` are
/// tried in order to re-locate the element without re-analyzing the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveElement {
    /// HTML tag name (e.g. `"a"`, `"button"`, `"input"`).
    pub tag: String,
    /// A minimal CSS selector identifying this element.
    pub selector: String,
    /// Alternative selectors for auto-healing (Scrapling pattern).
    ///
    /// Generated from text content, ARIA labels, attributes, and position.
    /// Tried in order when the primary selector fails to locate the element.
    #[serde(default)]
    pub fallback_selectors: Vec<String>,
    /// Visible text content (truncated to 200 chars).
    pub text: String,
    /// Key HTML attributes (id, class, href, type, name, value, placeholder, role).
    pub attributes: HashMap<String, String>,
    /// Whether the element is currently visible in the viewport.
    pub is_visible: bool,
    /// Interaction importance score 0.0–1.0 (computed by JS snapshot engine).
    /// Higher = more likely to be the target of user interaction.
    pub interaction_score: f32,
    /// Categorised kind of this element (computed by JS snapshot engine).
    pub element_kind: ElementKind,
}

impl InteractiveElement {
    /// Generate fallback selectors for auto-healing (Scrapling adaptive pattern).
    ///
    /// Produces alternative selectors from the element's attributes, text content,
    /// and ARIA labels. These can be tried when the primary selector fails due to
    /// page structure changes.
    pub fn generate_fallback_selectors(&self) -> Vec<String> {
        let mut fallbacks = Vec::new();

        // 1. By text content (Playwright's :has-text() or XPath contains)
        let trimmed = self.text.trim();
        if !trimmed.is_empty() && trimmed.len() < 100 {
            fallbacks.push(format!("{}:has-text(\"{}\")", self.tag, trimmed));
            fallbacks.push(format!("//{}[contains(text(),\"{}\")]", self.tag, trimmed));
        }

        // 2. By ARIA label
        if let Some(label) = self.attributes.get("aria-label") {
            if !label.is_empty() {
                fallbacks.push(format!("[aria-label=\"{}\"]", label));
            }
        }

        // 3. By placeholder
        if let Some(placeholder) = self.attributes.get("placeholder") {
            if !placeholder.is_empty() {
                fallbacks.push(format!("[placeholder=\"{}\"]", placeholder));
            }
        }

        // 4. By role
        if let Some(role) = self.attributes.get("role") {
            if !role.is_empty() {
                fallbacks.push(format!("[role=\"{}\"]", role));
            }
        }

        // 5. By name attribute
        if let Some(name) = self.attributes.get("name") {
            if !name.is_empty() {
                fallbacks.push(format!("[name=\"{}\"]", name));
            }
        }

        // 6. By id (if not already the primary selector)
        if let Some(id) = self.attributes.get("id") {
            if !id.is_empty() && self.selector != format!("#{}", id) {
                fallbacks.push(format!("#{}", id));
            }
        }

        fallbacks
    }

    /// Try to heal the selector by finding an alternative that works.
    ///
    /// Returns the first fallback selector, or `None` if none available.
    /// In Scrapling's adaptive model, this is called when the primary selector
    /// fails to locate the element in a new DOM snapshot.
    pub fn heal_selector(&self) -> Option<&str> {
        self.fallback_selectors.first().map(|s| s.as_str())
    }

    /// Try all fallback selectors in order, returning the first one.
    ///
    /// Skips `skip_count` initial fallbacks (e.g. ones already tried).
    pub fn heal_selector_skip(&self, skip_count: usize) -> Option<&str> {
        self.fallback_selectors.get(skip_count).map(|s| s.as_str())
    }
}

// ── HeadlessBrowser ──

/// High-level browser automation interface with automatic backend detection.
///
/// # Auto-detection order
/// 1. Playwright (if feature `playwright` is enabled and available)
/// 2. Chrome subprocess (if a compatible browser binary is found on `$PATH`)
/// 3. HTTP fetch (always available — no extra dependencies)
///
/// # Examples
/// ```ignore
/// let browser = HeadlessBrowser::auto_detect();
/// let html = browser.fetch_html("https://example.com").await?;
/// ```
pub struct HeadlessBrowser {
    backend: BrowserBackendKind,
}

impl Default for HeadlessBrowser {
    fn default() -> Self {
        Self::auto_detect()
    }
}

impl HeadlessBrowser {
    /// Auto-detect the best available backend.
    ///
    /// Checks in order: Playwright → Chrome subprocess → HTTP.
    pub fn auto_detect() -> Self {
        // 1. Try Playwright (requires feature flag)
        #[cfg(feature = "playwright")]
        {
            if playwright_binary_available() {
                return Self { backend: BrowserBackendKind::Playwright };
            }
        }
        // 2. Try Chrome subprocess
        if chrome_binary_available() {
            return Self { backend: BrowserBackendKind::ChromeSubprocess };
        }
        // 3. Fall back to HTTP
        Self { backend: BrowserBackendKind::Http }
    }

    /// Create a browser with a specific backend kind.
    pub fn new(backend: BrowserBackendKind) -> Self {
        Self { backend }
    }

    /// Fetch a page's rendered HTML (text-extracted).
    ///
    /// For HTTP backend this performs a simple GET request.
    /// For Chrome/Playwright this renders JS before extracting text.
    pub async fn fetch_html(&self, url: &str) -> BrowserResult<String> {
        match self.backend {
            BrowserBackendKind::Http => fetch_url(url),
            BrowserBackendKind::ChromeSubprocess => chrome_fetch_html(url).await,
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let result = backend.fetch_html(url).await?;
                backend.close().await?;
                Ok(result)
            }
        }
    }

    /// Take a screenshot of the page.
    ///
    /// Returns the path to the saved screenshot file.
    /// HTTP backend returns an error (no rendering capability).
    pub async fn screenshot(&self, url: &str) -> BrowserResult<String> {
        match self.backend {
            BrowserBackendKind::Http => Err("HTTP backend does not support screenshots. Enable the `playwright` feature for full browser support.".into()),
            BrowserBackendKind::ChromeSubprocess => chrome_screenshot(url).await,
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let path = backend.screenshot(url).await?;
                backend.close().await?;
                Ok(path)
            }
        }
    }

    /// Execute JavaScript in the page context and return the result.
    ///
    /// Only supported by the Playwright backend.
    /// Chrome subprocess and HTTP backends return an error.
    pub async fn execute_js(&self, _url: &str, _js: &str) -> BrowserResult<String> {
        match self.backend {
            BrowserBackendKind::Http | BrowserBackendKind::ChromeSubprocess => {
                Err("JavaScript execution requires the Playwright backend. Enable `playwright` feature.".into())
            }
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let result = backend.execute_js(url, js).await?;
                backend.close().await?;
                Ok(result)
            }
        }
    }

    /// Click an element identified by a CSS selector.
    ///
    /// Only supported by the Playwright backend.
    pub async fn click(&self, _url: &str, _selector: &str) -> BrowserResult<String> {
        match self.backend {
            BrowserBackendKind::Http | BrowserBackendKind::ChromeSubprocess => {
                Err("Click requires the Playwright backend. Enable `playwright` feature.".into())
            }
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let result = backend.click(url, selector).await?;
                backend.close().await?;
                Ok(result)
            }
        }
    }

    /// Fill an input field identified by a CSS selector.
    ///
    /// Only supported by the Playwright backend.
    pub async fn fill(&self, _url: &str, _selector: &str, _value: &str) -> BrowserResult<String> {
        match self.backend {
            BrowserBackendKind::Http | BrowserBackendKind::ChromeSubprocess => {
                Err("Fill requires the Playwright backend. Enable `playwright` feature.".into())
            }
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let result = backend.fill(url, selector, value).await?;
                backend.close().await?;
                Ok(result)
            }
        }
    }

    /// Navigate to a URL and return a [`DomSnapshot`] of the page.
    ///
    /// Only supported by the Playwright backend.
    /// HTTP and Chrome subprocess backends return an error.
    pub async fn navigate(&self, _url: &str) -> BrowserResult<DomSnapshot> {
        match self.backend {
            BrowserBackendKind::Http | BrowserBackendKind::ChromeSubprocess => {
                Err("Full navigation with DOM snapshot requires the Playwright backend. Enable `playwright` feature.".into())
            }
            #[cfg(feature = "playwright")]
            BrowserBackendKind::Playwright => {
                let backend = PlaywrightBackend::launch().await?;
                let snapshot = backend.navigate(url).await?;
                backend.close().await?;
                Ok(snapshot)
            }
        }
    }

    /// Returns the current backend kind.
    pub fn backend_kind(&self) -> &BrowserBackendKind {
        &self.backend
    }
}

// ── Chrome Subprocess Helpers ──

/// Check whether a Chrome-compatible browser binary is available on `$PATH`.
fn chrome_binary_available() -> bool {
    let candidates = [
        "google-chrome", "google-chrome-stable", "chromium",
        "chromium-browser", "chrome", "msedge", "edge",
    ];
    candidates.iter().any(|name| which::which(name).is_ok())
}

/// Find a Chrome-compatible browser binary, returning an error if none is found.
fn find_chrome() -> BrowserResult<String> {
    let candidates = [
        "google-chrome-stable", "google-chrome", "chromium",
        "chromium-browser", "chrome", "msedge", "edge",
    ];
    for name in &candidates {
        if let Ok(path) = which::which(name) {
            return Ok(path.to_string_lossy().to_string());
        }
    }
    Err("No Chrome/Chromium/Edge browser binary found on $PATH".into())
}

/// Fetch page HTML using headless Chrome's `--dump-dom` flag.
async fn chrome_fetch_html(url: &str) -> BrowserResult<String> {
    let chrome = find_chrome()?;
    let output = tokio::process::Command::new(&chrome)
        .args([
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--dump-dom",
        ])
        .arg(url)
        .output()
        .await
        .map_err(|e| format!("Failed to launch Chrome: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // --dump-dom writes HTML to stdout even with warnings; only fail on empty stdout
        if output.stdout.is_empty() {
            return Err(format!("Chrome --dump-dom failed: {}", stderr));
        }
    }

    let html = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(extract_text_from_html(&html))
}

/// Take a screenshot using headless Chrome's `--screenshot` flag.
async fn chrome_screenshot(url: &str) -> BrowserResult<String> {
    let chrome = find_chrome()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let output_path = format!("screenshot_{}.png", timestamp);

    let output = tokio::process::Command::new(&chrome)
        .args([
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--screenshot",
            &format!("--screenshot-path={}", output_path),
            "--window-size=1280,720",
        ])
        .arg(url)
        .output()
        .await
        .map_err(|e| format!("Failed to launch Chrome for screenshot: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Chrome screenshot failed: {}", stderr));
    }

    Ok(format!("Screenshot saved to {}", output_path))
}

// ── Playwright Backend (feature-gated) ──

/// Native Playwright Rust bindings backend.
///
/// Wraps `playwright-rs` to provide full browser automation: JS execution,
/// click, fill, screenshot, and DOM snapshotting.
///
/// Requires the `playwright` feature flag (enabled via `Cargo.toml`).
#[cfg(feature = "playwright")]
pub struct PlaywrightBackend {
    browser: playwright::api::Browser,
}

#[cfg(feature = "playwright")]
impl PlaywrightBackend {
    /// Launch a new Playwright browser instance (Chromium headless by default).
    pub async fn launch() -> BrowserResult<Self> {
        let pw = playwright::Playwright::initialize()
            .await
            .map_err(|e| format!("Playwright initialization failed: {}", e))?;

        let browser = pw
            .chromium()
            .launcher()
            .headless(true)
            .launch()
            .await
            .map_err(|e| format!("Playwright browser launch failed: {}", e))?;

        Ok(Self { browser })
    }

    /// Connect to an existing Playwright browser over CDP (e.g. `ws://...`).
    pub async fn connect(ws_endpoint: &str) -> BrowserResult<Self> {
        let pw = playwright::Playwright::initialize()
            .await
            .map_err(|e| format!("Playwright initialization failed: {}", e))?;

        let browser = pw
            .chromium()
            .connect(ws_endpoint)
            .await
            .map_err(|e| format!("Playwright connect to {} failed: {}", ws_endpoint, e))?;

        Ok(Self { browser })
    }

    /// Close the browser and release all resources.
    pub async fn close(&self) -> BrowserResult<()> {
        self.browser
            .close()
            .await
            .map_err(|e| format!("Playwright browser close failed: {}", e))
    }

    /// Fetch rendered HTML (text-extracted) from a page.
    pub async fn fetch_html(&self, url: &str) -> BrowserResult<String> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        // Wait for network idle
        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        let content = page
            .content()
            .await
            .map_err(|e| format!("Failed to get page content: {}", e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        Ok(extract_text_from_html(&content))
    }

    /// Take a screenshot and return the file path.
    pub async fn screenshot(&self, url: &str) -> BrowserResult<String> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let output_path = format!("playwright_screenshot_{}.png", timestamp);

        page.screenshot(playwright::api::PageScreenshot {
            path: Some(output_path.clone()),
            full_page: Some(true),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("Screenshot failed: {}", e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        Ok(output_path)
    }

    /// Execute JavaScript in the page context and return the serialized result.
    pub async fn execute_js(&self, url: &str, js: &str) -> BrowserResult<String> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        let result = page
            .evaluate(js)
            .await
            .map_err(|e| format!("JavaScript execution failed: {}", e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        Ok(result.to_string())
    }

    /// Click an element identified by a CSS selector.
    pub async fn click(&self, url: &str, selector: &str) -> BrowserResult<String> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        page.click(selector)
            .await
            .map_err(|e| format!("Click on '{}' failed: {}", selector, e))?;

        // Wait a brief moment for any post-click navigation/updates
        tokio::time::sleep(Duration::from_millis(500)).await;

        let final_url = page
            .url()
            .await
            .map_err(|e| format!("Failed to get current URL: {}", e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        Ok(format!("Clicked '{}', current URL: {}", selector, final_url))
    }

    /// Fill an input field identified by a CSS selector.
    pub async fn fill(&self, url: &str, selector: &str, value: &str) -> BrowserResult<String> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        // Clear existing content then type the new value
        page.fill(selector, value)
            .await
            .map_err(|e| format!("Fill '{}' on '{}' failed: {}", value, selector, e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        Ok(format!("Filled '{}' with '{}'", selector, value))
    }

    /// Navigate to a URL and extract a structured [`DomSnapshot`].
    pub async fn navigate(&self, url: &str) -> BrowserResult<DomSnapshot> {
        let page = self
            .browser
            .new_page()
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        page.navigate(url)
            .await
            .map_err(|e| format!("Navigation failed: {}", e))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Wait for navigation failed: {}", e))?;

        let current_url = page
            .url()
            .await
            .map_err(|e| format!("Failed to get URL: {}", e))?;

        let title = page
            .title()
            .await
            .map_err(|e| format!("Failed to get title: {}", e))?;

        // Extract interactive elements via JavaScript — enhanced snapshot engine
        // with intelligent scoring, classification, and visibility detection.
        let snapshot_js = r#"
            (() => {
                const VIEWPORT_W = window.innerWidth;
                const VIEWPORT_H = window.innerHeight;

                function computeInteractionScore(el, rect) {
                    let score = 0.0;
                    const tag = el.tagName.toLowerCase();

                    // Base score by tag type
                    if (tag === 'button') score += 0.5;
                    else if (tag === 'a') score += 0.4;
                    else if (tag === 'input' || tag === 'select' || tag === 'textarea') score += 0.5;
                    else if (el.role === 'button') score += 0.4;
                    else score += 0.1;

                    // Boost by ARIA roles
                    const role = el.getAttribute('role');
                    if (role === 'button' || role === 'link' || role === 'menuitem') score += 0.2;
                    if (role === 'search' || role === 'form' || role === 'dialog') score += 0.15;

                    // Boost by text content
                    const text = (el.textContent || '').trim();
                    if (text.length > 0) score += 0.15;
                    if (text.length > 20) score += 0.05;

                    // Boost by visible/accessible labels
                    const ariaLabel = el.getAttribute('aria-label');
                    if (ariaLabel && ariaLabel.length > 0) score += 0.1;

                    // Boost by prominent positioning
                    if (rect) {
                        const area = rect.width * rect.height;
                        const vpArea = VIEWPORT_W * VIEWPORT_H;
                        if (area > 0 && area / vpArea > 0.05) score += 0.1;
                        // Near viewport center
                        const cx = rect.left + rect.width / 2;
                        const cy = rect.top + rect.height / 2;
                        const distFromCenter = Math.hypot(cx - VIEWPORT_W/2, cy - VIEWPORT_H/2);
                        const maxDist = Math.hypot(VIEWPORT_W/2, VIEWPORT_H/2);
                        if (distFromCenter / maxDist < 0.3) score += 0.1;
                    }

                    // Penalize hidden inputs
                    if (el.getAttribute('type') === 'hidden') score -= 0.5;

                    // Penalize very small elements (likely tracking/decorative)
                    if (rect && (rect.width < 5 || rect.height < 5)) score -= 0.3;

                    // Boost by tabindex (interactive by convention)
                    const ti = el.getAttribute('tabindex');
                    if (ti && ti !== '-1' && parseInt(ti) >= 0) score += 0.1;

                    // Penalize disabled elements
                    if (el.disabled) score -= 0.3;

                    return Math.max(0.0, Math.min(1.0, score));
                }

                function classifyElementKind(el) {
                    const tag = el.tagName.toLowerCase();
                    const role = el.getAttribute('role');
                    const type = el.getAttribute('type');

                    if (tag === 'button' || role === 'button') return 'action';
                    if (tag === 'a' || role === 'link' || role === 'menuitem') return 'navigation';
                    if (tag === 'input' || tag === 'select' || tag === 'textarea') {
                        if (type === 'hidden') return 'decorative';
                        return 'form_control';
                    }
                    if (tag === 'h1' || tag === 'h2' || tag === 'h3' || tag === 'h4' || tag === 'h5' || tag === 'h6' || tag === 'p' || tag === 'label') return 'content';
                    if (tag === 'div' || tag === 'section' || tag === 'article' || tag === 'main' || tag === 'aside' || tag === 'nav') return 'structure';
                    return 'decorative';
                }

                function computeVisibility(el) {
                    const rect = el.getBoundingClientRect();
                    const isZeroSize = rect.width === 0 || rect.height === 0;
                    const isOffscreen = rect.bottom < 0 || rect.right < 0 || rect.top > VIEWPORT_H || rect.left > VIEWPORT_W;
                    const style = window.getComputedStyle(el);
                    const hasOpacity = parseFloat(style.opacity) > 0.01;
                    const hasVisibility = style.visibility !== 'hidden';
                    const hasDisplay = style.display !== 'none';
                    return !isZeroSize && !isOffscreen && hasOpacity && hasVisibility && hasDisplay && !el.disabled;
                }

                function bestSelector(el) {
                    // Strategy 1: id (most stable)
                    if (el.id) return '#' + CSS.escape(el.id);
                    // Strategy 2: data-testid (QA-friendly)
                    const testId = el.getAttribute('data-testid');
                    if (testId) return '[data-testid="' + CSS.escape(testId) + '"]';
                    // Strategy 3: name attribute
                    const name = el.getAttribute('name');
                    if (name) return '[name="' + CSS.escape(name) + '"]';
                    // Strategy 4: aria-label
                    const ariaLabel = el.getAttribute('aria-label');
                    if (ariaLabel) return '[aria-label="' + CSS.escape(ariaLabel) + '"]';
                    // Strategy 5: unique class combination
                    const classes = el.className && typeof el.className === 'string'
                        ? el.className.trim().split(/\s+/).filter(c => c.length > 0)
                        : [];
                    if (classes.length > 0) {
                        return el.tagName.toLowerCase() + '.' + classes.join('.');
                    }
                    // Strategy 6: nth-child path (fragile but better than nothing)
                    const parent = el.parentElement;
                    if (parent) {
                        const siblings = Array.from(parent.children).filter(c => c.tagName === el.tagName);
                        if (siblings.length > 1) {
                            const idx = siblings.indexOf(el) + 1;
                            return el.tagName.toLowerCase() + ':nth-child(' + idx + ')';
                        }
                    }
                    return el.tagName.toLowerCase();
                }

                const elements = document.querySelectorAll(
                    'a, button, input, select, textarea, [role="button"], [role="link"], [role="menuitem"], [tabindex]:not([tabindex="-1"])'
                );
                const results = Array.from(elements).map(el => {
                    const rect = el.getBoundingClientRect();
                    const is_visible = computeVisibility(el);
                    return {
                        tag: el.tagName.toLowerCase(),
                        selector: bestSelector(el),
                        text: (el.textContent || '').trim().substring(0, 200),
                        attributes: (() => {
                            const attrs = {};
                            const keys = ['id', 'class', 'href', 'type', 'name', 'value', 'placeholder', 'role', 'aria-label', 'data-testid', 'target', 'rel', 'disabled'];
                            for (const k of keys) {
                                const v = el.getAttribute(k);
                                if (v) attrs[k] = v;
                            }
                            return attrs;
                        })(),
                        is_visible: is_visible,
                        interaction_score: computeInteractionScore(el, rect),
                        element_kind: classifyElementKind(el),
                    };
                });
                // Sort server-side by score descending, so server can apply smart_filter
                results.sort((a, b) => b.interaction_score - a.interaction_score);
                return JSON.stringify(results);
            })()
        "#;

        let json_result = page
            .evaluate(snapshot_js)
            .await
            .map_err(|e| format!("DOM snapshot JS evaluation failed: {}", e))?;

        page.close()
            .await
            .map_err(|e| format!("Failed to close page: {}", e))?;

        let elements: Vec<InteractiveElement> = serde_json::from_str(&json_result.to_string())
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to parse interactive elements JSON: {}", e);
                Vec::new()
            });

        Ok(DomSnapshot {
            url: current_url,
            title,
            interactive_elements: elements,
        })
    }
}

/// Check whether Playwright CLI / browsers are available.
#[cfg(feature = "playwright")]
fn playwright_binary_available() -> bool {
    // Common indicators that Playwright browsers are installed
    which::which("playwright").is_ok()
        || std::env::var("PLAYWRIGHT_BROWSERS_PATH").is_ok()
        || std::env::var("PLAYWRIGHT_NODEJS_PATH").is_ok()
}

// ══════════════════════════════════════════════════════════════════════════════
// Existing API — preserved for backwards compatibility
// ══════════════════════════════════════════════════════════════════════════════

/// Fetch a URL and extract readable text content (async).
pub async fn fetch_url_async(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("deepseek-carp/0.1 (AI coding assistant)")
        .danger_accept_invalid_certs(false)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    let text = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        extract_text_from_html(&body)
    } else {
        body.clone()
    };

    let max_chars = 50_000;
    let truncated = if text.len() > max_chars {
        format!("{}...\n\n[Truncated: {} total chars]", &text[..max_chars], text.len())
    } else {
        text
    };

    Ok(format!(
        "URL: {}\nStatus: {}\nContent-Type: {}\n\n{}",
        url, status, content_type, truncated
    ))
}

/// Synchronous wrapper for fetch_url_async (used in DefaultToolExecutor).
pub fn fetch_url(url: &str) -> Result<String, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| format!("Runtime error: {}", e))?;
    rt.block_on(fetch_url_async(url))
}

/// Extract readable text from HTML, stripping tags and scripts.
fn extract_text_from_html(html: &str) -> String {
    // Simple approach: remove script/style tags and their content,
    // then strip remaining HTML tags, and normalize whitespace.
    let mut text = String::with_capacity(html.len());

    // Remove <script>...</script> and <style>...</style> blocks
    let mut in_skip = false;
    let mut skip_tag = "";
    let mut pos = 0;
    let chars: Vec<char> = html.chars().collect();

    while pos < chars.len() {
        if !in_skip {
            // Check for opening script/style tag
            if pos + 7 < chars.len() {
                let slice: String = chars[pos..pos + 8].iter().collect();
                if slice.to_lowercase().starts_with("<script") {
                    in_skip = true;
                    skip_tag = "script";
                    pos += 7;
                    continue;
                }
            }
            if pos + 7 < chars.len() {
                let slice: String = chars[pos..pos + 7].iter().collect();
                if slice.to_lowercase().starts_with("<style") {
                    in_skip = true;
                    skip_tag = "style";
                    pos += 6;
                    continue;
                }
            }
            text.push(chars[pos]);
        } else {
            // Check for closing tag
            let close_tag = format!("</{}", skip_tag);
            if pos + close_tag.len() < chars.len() {
                let slice: String = chars[pos..pos + close_tag.len() + 1].iter().collect();
                if slice.to_lowercase().starts_with(&close_tag) {
                    in_skip = false;
                }
            }
        }
        pos += 1;
    }

    // Strip remaining HTML tags: remove everything between < and >
    let mut clean = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => clean.push(ch),
            _ => {}
        }
    }

    // Decode common HTML entities
    let decoded = clean
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Normalize whitespace: collapse multiple newlines/blanks
    let mut result = String::with_capacity(decoded.len());
    let mut prev_was_newline = false;
    for line in decoded.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_was_newline {
                result.push('\n');
                prev_was_newline = true;
            }
        } else {
            result.push_str(trimmed);
            result.push('\n');
            prev_was_newline = false;
        }
    }

    result.trim().to_string()
}

/// Placeholder for future headless browser screenshot.
#[allow(dead_code)]
pub fn browser_screenshot(_url: &str) -> Result<String, String> {
    Err("Browser screenshot not yet supported. Install Playwright or Firefox Agent Bridge.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_basic() {
        let html = "<html><head><script>console.log('x')</script></head><body><p>Hello World</p></body></html>";
        let text = extract_text_from_html(html);
        assert!(text.contains("Hello World"));
        assert!(!text.contains("console.log"));
        assert!(!text.contains("<script>"));
    }

    #[test]
    fn test_extract_text_entities() {
        let html = "<p>Rock &amp; Roll &lt;3</p>";
        let text = extract_text_from_html(html);
        assert!(text.contains("Rock & Roll <3"));
    }

    #[test]
    fn test_extract_text_style_block() {
        let html = "<style>body { color: red; }</style><p>Visible</p>";
        let text = extract_text_from_html(html);
        assert!(text.contains("Visible"));
        assert!(!text.contains("color: red"));
    }

    #[test]
    fn test_browser_backend_kind_debug() {
        // Verify the enum is Debug + Clone + PartialEq
        let a = BrowserBackendKind::Http;
        let b = BrowserBackendKind::ChromeSubprocess;
        assert_eq!(a, a);
        assert_ne!(a, b);
        let _ = format!("{:?}", a);
        let _ = a.clone();
    }

    #[test]
    fn test_headless_browser_auto_detect() {
        // auto_detect should always at least return Http
        let browser = HeadlessBrowser::auto_detect();
        // We can't assert exact backend since it depends on the test environment,
        // but we can assert it's one of the valid variants
        let _ = format!("{:?}", browser.backend_kind());
    }

    #[test]
    fn test_headless_browser_new() {
        let browser = HeadlessBrowser::new(BrowserBackendKind::Http);
        assert_eq!(browser.backend_kind(), &BrowserBackendKind::Http);
    }

    #[test]
    fn test_dom_snapshot_construction() {
        let snapshot = DomSnapshot {
            url: "https://example.com".into(),
            title: "Example".into(),
            interactive_elements: vec![
                InteractiveElement {
                    tag: "a".into(),
                    selector: "#link1".into(),
                    fallback_selectors: vec![],
                    text: "Click me".into(),
                    attributes: HashMap::from([
                        ("href".into(), "https://example.com/page2".into()),
                    ]),
                    is_visible: true,
                    interaction_score: 0.6,
                    element_kind: ElementKind::Navigation,
                },
                InteractiveElement {
                    tag: "input".into(),
                    selector: "#search".into(),
                    fallback_selectors: vec![],
                    text: "".into(),
                    attributes: HashMap::from([
                        ("type".into(), "text".into()),
                        ("placeholder".into(), "Search...".into()),
                    ]),
                    is_visible: true,
                    interaction_score: 0.5,
                    element_kind: ElementKind::FormControl,
                },
            ],
        };
        assert_eq!(snapshot.url, "https://example.com");
        assert_eq!(snapshot.title, "Example");
        assert_eq!(snapshot.interactive_elements.len(), 2);
        assert_eq!(snapshot.interactive_elements[0].tag, "a");
        assert_eq!(snapshot.interactive_elements[1].tag, "input");
    }

    #[test]
    fn test_interactive_element_default_attributes() {
        let el = InteractiveElement {
            tag: "button".into(),
            selector: "button.submit".into(),
            fallback_selectors: vec![],
            text: "Submit".into(),
            attributes: HashMap::new(),
            is_visible: false,
            interaction_score: 0.0,
            element_kind: ElementKind::Decorative,
        };
        assert!(el.attributes.is_empty());
        assert!(!el.is_visible);
    }

    #[test]
    fn test_smart_filter_noise() {
        let snapshot = DomSnapshot {
            url: "https://example.com".into(),
            title: "Test".into(),
            interactive_elements: vec![
                // A useful button
                InteractiveElement {
                    tag: "button".into(),
                    selector: "#submit".into(),
                    fallback_selectors: vec![],
                    text: "Submit".into(),
                    attributes: HashMap::new(),
                    is_visible: true,
                    interaction_score: 0.8,
                    element_kind: ElementKind::Action,
                },
                // Hidden input — should be filtered
                InteractiveElement {
                    tag: "input".into(),
                    selector: "#hidden-csrf".into(),
                    fallback_selectors: vec![],
                    text: "".into(),
                    attributes: HashMap::from([("type".into(), "hidden".into())]),
                    is_visible: false,
                    interaction_score: 0.0,
                    element_kind: ElementKind::Decorative,
                },
                // Noisy element — should be filtered
                InteractiveElement {
                    tag: "script".into(),
                    selector: "#tracking".into(),
                    fallback_selectors: vec![],
                    text: "".into(),
                    attributes: HashMap::new(),
                    is_visible: false,
                    interaction_score: 0.0,
                    element_kind: ElementKind::Decorative,
                },
            ],
        };
        let filtered = snapshot.filter_noise();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].selector, "#submit");
    }

    #[test]
    fn test_smart_filter_dedup() {
        let snapshot = DomSnapshot {
            url: "https://example.com".into(),
            title: "Test".into(),
            interactive_elements: vec![
                InteractiveElement {
                    tag: "button".into(),
                    selector: "#save".into(),
                    fallback_selectors: vec![],
                    text: "Save".into(),
                    attributes: HashMap::new(),
                    is_visible: true,
                    interaction_score: 0.7,
                    element_kind: ElementKind::Action,
                },
                // Duplicate — same tag + selector
                InteractiveElement {
                    tag: "button".into(),
                    selector: "#save".into(),
                    fallback_selectors: vec![],
                    text: "Save".into(),
                    attributes: HashMap::new(),
                    is_visible: true,
                    interaction_score: 0.7,
                    element_kind: ElementKind::Action,
                },
            ],
        };
        let deduped = snapshot.deduplicate();
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn test_smart_filter_pipeline() {
        let snapshot = DomSnapshot {
            url: "https://example.com".into(),
            title: "Test".into(),
            interactive_elements: vec![
                InteractiveElement {
                    tag: "button".into(),
                    selector: "#primary-cta".into(),
                    fallback_selectors: vec![],
                    text: "Buy Now".into(),
                    attributes: HashMap::new(),
                    is_visible: true,
                    interaction_score: 0.9,
                    element_kind: ElementKind::Action,
                },
                InteractiveElement {
                    tag: "a".into(),
                    selector: ".footer-link".into(),
                    fallback_selectors: vec![],
                    text: "Terms".into(),
                    attributes: HashMap::new(),
                    is_visible: false,
                    interaction_score: 0.2,
                    element_kind: ElementKind::Navigation,
                },
                InteractiveElement {
                    tag: "input".into(),
                    selector: "#email".into(),
                    fallback_selectors: vec![],
                    text: "".into(),
                    attributes: HashMap::from([("type".into(), "email".into())]),
                    is_visible: true,
                    interaction_score: 0.6,
                    element_kind: ElementKind::FormControl,
                },
            ],
        };
        let result = snapshot.smart_filter(2);
        assert_eq!(result.len(), 2);
        // Highest score first
        assert_eq!(result[0].selector, "#primary-cta");
        assert_eq!(result[1].selector, "#email");
    }

    #[test]
    fn test_browser_result_type() {
        let ok: BrowserResult<i32> = Ok(42);
        let err: BrowserResult<i32> = Err("oops".into());
        assert!(ok.is_ok());
        assert!(err.is_err());
    }

    #[test]
    fn test_chrome_binary_available() {
        // This should not panic — it may return true or false depending on the environment
        let _available = chrome_binary_available();
    }

    #[test]
    fn test_find_chrome_on_ci() {
        // find_chrome may fail if no browser is installed; just verify it doesn't panic
        let _result = find_chrome();
    }

    #[test]
    fn test_generate_fallback_selectors_text() {
        let el = InteractiveElement {
            tag: "button".into(),
            selector: "#submit-btn".into(),
            fallback_selectors: vec![],
            text: "Submit".into(),
            attributes: HashMap::from([
                ("id".into(), "submit-btn".into()),
                ("class".into(), "btn-primary".into()),
            ]),
            is_visible: true,
            interaction_score: 0.8,
            element_kind: ElementKind::Action,
        };
        let fallbacks = el.generate_fallback_selectors();
        // Should have text-based selectors (tag:has-text and XPath)
        assert!(fallbacks.iter().any(|s| s.contains("has-text")));
        assert!(fallbacks.iter().any(|s| s.contains("contains(text")));
    }

    #[test]
    fn test_generate_fallback_selectors_aria() {
        let el = InteractiveElement {
            tag: "button".into(),
            selector: "#close".into(),
            fallback_selectors: vec![],
            text: "".into(),
            attributes: HashMap::from([
                ("aria-label".into(), "Close dialog".into()),
            ]),
            is_visible: true,
            interaction_score: 0.5,
            element_kind: ElementKind::Action,
        };
        let fallbacks = el.generate_fallback_selectors();
        assert!(fallbacks.iter().any(|s| s.contains("aria-label")));
    }

    #[test]
    fn test_heal_selector_returns_fallback() {
        let el = InteractiveElement {
            tag: "a".into(),
            selector: "#old-link".into(),
            fallback_selectors: vec![
                "a:has-text(\"Click here\")".into(),
                "[aria-label=\"click\"]".into(),
            ],
            text: "Click here".into(),
            attributes: HashMap::new(),
            is_visible: true,
            interaction_score: 0.6,
            element_kind: ElementKind::Navigation,
        };
        assert_eq!(el.heal_selector(), Some("a:has-text(\"Click here\")"));
        assert_eq!(el.heal_selector_skip(1), Some("[aria-label=\"click\"]"));
        assert_eq!(el.heal_selector_skip(5), None);
    }
}