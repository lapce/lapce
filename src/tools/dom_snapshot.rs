//! DOM Snapshot — intelligent filtering of interactive elements from HTML.
//!
//! Extracts only interactive elements (buttons, links, inputs, selects, textareas,
//! forms, and elements with role="button" or onclick) from a full HTML page.
//! Reduces LLM token consumption by discarding non-interactive content.
//! Inspired by the browser-use project.

use std::collections::HashMap;
use std::time::Duration;

use regex::Regex;

/// A bounding box for an element (populated by headless browser;
/// always [`None`] in HTML-only static mode).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A single interactive element extracted from HTML.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InteractiveElement {
    pub tag: String,
    pub id: Option<String>,
    pub class: Option<String>,
    pub text: Option<String>,
    pub attributes: HashMap<String, String>,
    pub selector: String,
    pub bounding_box: Option<BoundingBox>,
    pub is_visible: bool,
}

impl InteractiveElement {
    /// Compact single-line text representation for LLM prompts.
    ///
    /// Example: `<button#submit.btn-primary> Submit [visible]`
    pub fn to_text_summary(&self) -> String {
        let vis = if self.is_visible { "visible" } else { "hidden" };
        let txt = self
            .text
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| format!(" {:?}", t))
            .unwrap_or_default();
        format!("<{}>{} [{}]", self.selector, txt, vis)
    }

    /// Structured JSON representation of this element.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Filtered DOM snapshot containing only interactive elements.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DomSnapshot {
    pub url: Option<String>,
    pub title: Option<String>,
    pub elements: Vec<InteractiveElement>,
    pub total_elements: usize,
    pub filtered_count: usize,
}

impl DomSnapshot {
    /// Compact text summary — ideal for LLM context injection.
    ///
    /// Format:
    /// ```text
    /// Page: [title]
    /// URL: [url]
    /// Interactive Elements (N):
    /// [1] <button#submit> "Submit" [visible]
    /// ```
    pub fn to_text_summary(&self) -> String {
        let mut out = String::new();
        if let Some(ref title) = self.title {
            out.push_str(&format!("Page: {}\n", title));
        }
        if let Some(ref url) = self.url {
            out.push_str(&format!("URL: {}\n", url));
        }
        out.push_str(&format!(
            "Interactive Elements ({}):\n",
            self.elements.len()
        ));
        for (i, el) in self.elements.iter().enumerate() {
            out.push_str(&format!("[{}] {}\n", i + 1, el.to_text_summary()));
        }
        out
    }

    /// Structured JSON representation of the full snapshot.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Configuration for DOM filtering.
///
/// # Defaults
/// - `max_elements`: 50
/// - `include_attributes`: true
/// - `include_styles`: false
/// - `min_text_length`: 0
#[derive(Debug, Clone)]
pub struct DomFilter {
    /// Maximum number of interactive elements to include in the snapshot.
    pub max_elements: usize,
    /// Whether to include element attributes in the output.
    pub include_attributes: bool,
    /// Whether to include inline styles in the output.
    pub include_styles: bool,
    /// Minimum text length for text-bearing elements (a, button, option).
    /// Elements below this threshold are excluded.
    pub min_text_length: usize,
}

impl Default for DomFilter {
    fn default() -> Self {
        Self {
            max_elements: 50,
            include_attributes: true,
            include_styles: false,
            min_text_length: 0,
        }
    }
}

// ─── Public extraction API ────────────────────────────────────────────────

/// Extract interactive elements from a raw HTML string.
///
/// Uses regex-based tag parsing (no external HTML parser dependency).
/// Returns a [`DomSnapshot`] containing only elements deemed interactive.
pub fn extract_snapshot(html: &str, config: &DomFilter) -> DomSnapshot {
    // Strip <script> / <style> blocks first so we don't accidentally treat
    // code as interactive content.
    let cleaned = strip_script_style(html);
    let title = extract_title(&cleaned);

    let tag_re =
        Regex::new(r#"<\s*(\w+)((?:\s+(?:[^>"\x27]|"[^"]*"|\x27[^\x27]*\x27)*))?\s*(/?)>"#)
            .expect("valid tag regex");
    let mut elements: Vec<InteractiveElement> = Vec::new();
    // Track already-processed byte positions to avoid duplicates when the
    // same tag matches multiple criteria.
    let mut seen_positions = std::collections::HashSet::new();

    for cap in tag_re.captures_iter(&cleaned) {
        if elements.len() >= config.max_elements {
            break;
        }

        let full_match = cap.get(0).expect("full match");
        let start = full_match.start();

        if !seen_positions.insert(start) {
            continue;
        }

        let tag_name = cap[1].to_lowercase();
        let attr_raw = cap.get(2).map_or("", |m| m.as_str());
        let _is_self_closing = cap.get(3).is_some_and(|m| m.as_str() == "/");

        let mut attributes = parse_attributes(attr_raw);

        // Strip styles from attributes if not requested
        if !config.include_styles {
            attributes.remove("style");
        }

        if !is_interactive(&tag_name, &attributes) {
            continue;
        }

        let is_visible = check_visibility(&attributes, &tag_name);

        let text = extract_element_text(&cleaned, full_match.end(), &tag_name, &attributes);

        // Apply min_text_length filter (skip only for purely widget-like tags)
        if let Some(ref t) = text {
            if t.trim().len() < config.min_text_length
                && !matches!(tag_name.as_str(), "input" | "select" | "textarea")
            {
                continue;
            }
        }

        let selector = build_selector(&tag_name, &attributes);

        let attrs_out = if config.include_attributes {
            attributes
        } else {
            HashMap::new()
        };

        elements.push(InteractiveElement {
            tag: tag_name,
            id: attrs_out.get("id").cloned(),
            class: attrs_out.get("class").cloned(),
            text,
            attributes: attrs_out,
            selector,
            bounding_box: None,
            is_visible,
        });
    }

    let total = elements.len();
    DomSnapshot {
        url: None,
        title,
        elements,
        total_elements: total,
        filtered_count: total,
    }
}

/// Fetch a URL and extract interactive elements from it.
///
/// This is a synchronous convenience wrapper that creates a one-shot tokio
/// runtime internally.
pub fn extract_snapshot_from_url(url: &str) -> anyhow::Result<DomSnapshot> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("deepseek-carp/0.1 (AI coding assistant)")
            .build()?;

        let response = client.get(url).send().await?;
        let html = response.text().await?;

        let config = DomFilter::default();
        let mut snapshot = extract_snapshot(&html, &config);
        snapshot.url = Some(url.to_string());
        Ok(snapshot)
    })
}

// ─── Internal helpers ─────────────────────────────────────────────────────

/// Remove `<script>…</script>` and `<style>…</style>` blocks (and their
/// content) from HTML.
fn strip_script_style(html: &str) -> String {
    let re =
        Regex::new(r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>")
            .expect("valid strip regex");
    re.replace_all(html, "").to_string()
}

/// Extract the page `<title>` text.
fn extract_title(html: &str) -> Option<String> {
    let re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .expect("valid title regex");
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse key="value" and key='value' attributes from an attribute string.
fn parse_attributes(attr_str: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();

    // double-quoted:  key="value"
    let dq = Regex::new(r#"(\w+)\s*=\s*"([^"]*?)""#).expect("valid dq regex");
    for cap in dq.captures_iter(attr_str) {
        attrs.insert(cap[1].to_lowercase(), cap[2].to_string());
    }

    // single-quoted:  key='value'
    let sq = Regex::new(r#"(\w+)\s*=\s*'([^']*?)'"#).expect("valid sq regex");
    for cap in sq.captures_iter(attr_str) {
        attrs.insert(cap[1].to_lowercase(), cap[2].to_string());
    }

    // boolean / valueless attributes (hidden, disabled, required, readonly,
    // checked, selected, multiple, autofocus, novalidate, formnovalidate)
    let bool_attrs = [
        "hidden",
        "disabled",
        "required",
        "readonly",
        "checked",
        "selected",
        "multiple",
        "autofocus",
        "novalidate",
        "formnovalidate",
    ];
    for name in &bool_attrs {
        let pattern = format!(r"\b{}\b", regex::escape(name));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(attr_str) {
                attrs.entry(name.to_string()).or_insert_with(String::new);
            }
        }
    }

    attrs
}

/// Determine whether a tag (with its attributes) is an interactive element.
fn is_interactive(tag: &str, attrs: &HashMap<String, String>) -> bool {
    match tag {
        "a" => attrs.contains_key("href"),
        "button" | "select" | "textarea" | "form" => true,
        "input" => {
            let input_type = attrs
                .get("type")
                .map(|s| s.to_lowercase());
            !matches!(input_type.as_deref(), Some("hidden"))
        }
        _ => {
            // Elements with onclick or role="button"
            if attrs.contains_key("onclick") {
                return true;
            }
            if attrs
                .get("role")
                .is_some_and(|r| r.to_lowercase() == "button")
            {
                return true;
            }
            false
        }
    }
}

/// Check whether an element is visible (not hidden, not aria-hidden).
fn check_visibility(attrs: &HashMap<String, String>, tag: &str) -> bool {
    // <input type="hidden"> is not visible
    if tag == "input"
        && attrs
            .get("type")
            .is_some_and(|t| t.to_lowercase() == "hidden")
    {
        return false;
    }

    // hidden attribute
    if attrs.contains_key("hidden") {
        return false;
    }

    // Inline style checks
    if let Some(style) = attrs.get("style") {
        let compact = style.to_lowercase().replace(' ', "");
        if compact.contains("display:none") || compact.contains("visibility:hidden") {
            return false;
        }
    }

    // aria-hidden="true"
    if attrs
        .get("aria-hidden")
        .is_some_and(|v| v == "true")
    {
        return false;
    }

    // Decorative roles mean the element is not meaningful
    if let Some(role) = attrs.get("role") {
        let r = role.to_lowercase();
        if r == "presentation" || r == "none" {
            return false;
        }
    }

    true
}

/// Build a CSS-like selector string for an element.
///
/// Prefers `#id`, falls back to `tag.class1.class2[type="…"]`.
fn build_selector(tag: &str, attrs: &HashMap<String, String>) -> String {
    if let Some(id) = attrs.get("id") {
        if !id.is_empty() {
            return format!("#{}", id);
        }
    }

    let mut sel = tag.to_string();
    if let Some(class) = attrs.get("class") {
        for cls in class.split_whitespace() {
            sel.push_str(&format!(".{}", cls));
        }
    }

    // Append [type="…"] for input/button elements
    if let Some(typ) = attrs.get("type") {
        if matches!(tag, "input" | "button") {
            sel.push_str(&format!("[type=\"{}\"]", typ));
        }
    }

    sel
}

/// Extract text content from an interactive element.
///
/// For container elements (a, button, etc.) this grabs the inner text with
/// HTML tags stripped. For void/widget elements (input, select, textarea) it
/// uses attributes like `value`, `placeholder`, or `aria-label`.
fn extract_element_text(
    html: &str,
    start_pos: usize,
    tag: &str,
    attrs: &HashMap<String, String>,
) -> Option<String> {
    match tag {
        "input" => attrs
            .get("value")
            .or_else(|| attrs.get("placeholder"))
            .or_else(|| attrs.get("aria-label"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        "select" | "textarea" => attrs
            .get("placeholder")
            .or_else(|| attrs.get("aria-label"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        // Container elements: extract inner text
        _ => extract_inner_text(html, start_pos, tag),
    }
}

/// Extract and clean inner text between an opening tag and its `</tag>`.
fn extract_inner_text(html: &str, start_pos: usize, tag: &str) -> Option<String> {
    let closing = format!("</{}", tag);
    let remaining = &html[start_pos..];
    let end = remaining.find(&closing)?;
    let inner = &remaining[..end];
    let text = strip_html_tags(inner);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Remove all HTML tags from a string, preserving text content.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => {
                // Normalize whitespace: collapse runs of spaces/tabs to one space
                if ch.is_whitespace() && ch != '\n' {
                    if !result.ends_with(' ') && !result.ends_with('\n') {
                        result.push(' ');
                    }
                } else {
                    result.push(ch);
                }
            }
            _ => {}
        }
    }
    result
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_snapshot ──────────────────────────────────────────────

    #[test]
    fn test_extract_links() {
        let html = r#"<html><body><a href="/page1">Link 1</a><a href="/page2">Link 2</a></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 2);
        assert_eq!(snap.elements[0].tag, "a");
        assert_eq!(snap.elements[0].text.as_deref(), Some("Link 1"));
        assert!(snap.elements[0].is_visible);
    }

    #[test]
    fn test_extract_buttons() {
        let html = r#"<html><body><button id="submit">Submit</button><button disabled>No</button></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 2);
        assert_eq!(snap.elements[0].selector, "#submit");
        assert_eq!(snap.elements[0].text.as_deref(), Some("Submit"));
    }

    #[test]
    fn test_extract_inputs() {
        let html = r#"<html><body>
            <input type="text" id="name" placeholder="Your name">
            <input type="email" id="email">
            <input type="hidden" id="token" value="abc">
            <input type="checkbox" id="agree">
            <input type="radio" id="opt1">
        </body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        // hidden input should be excluded
        let hidden = snap.elements.iter().any(|e| e.id.as_deref() == Some("token"));
        assert!(!hidden, "hidden input should be filtered out");
        // All other inputs should be present
        assert!(snap.elements.iter().any(|e| e.id.as_deref() == Some("name")));
        assert!(snap.elements.iter().any(|e| e.id.as_deref() == Some("email")));
        assert!(snap.elements.iter().any(|e| e.id.as_deref() == Some("agree")));
        assert!(snap.elements.iter().any(|e| e.id.as_deref() == Some("opt1")));
    }

    #[test]
    fn test_extract_select_textarea() {
        let html = r#"<html><body>
            <select id="country"><option>US</option></select>
            <textarea id="bio">Hello</textarea>
        </body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 2);
        assert_eq!(snap.elements[0].tag, "select");
        assert_eq!(snap.elements[1].tag, "textarea");
    }

    #[test]
    fn test_extract_role_button() {
        let html = r#"<html><body><div role="button" id="btn">Click</div></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].tag, "div");
        assert_eq!(snap.elements[0].selector, "#btn");
    }

    #[test]
    fn test_extract_onclick() {
        let html = r#"<html><body><span onclick="alert(1)" id="alert">Click</span></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].tag, "span");
    }

    #[test]
    fn test_extract_form() {
        let html = r#"<html><body><form id="login"><input type="text"></form></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert!(snap.elements.iter().any(|e| e.tag == "form"));
        assert!(snap.elements.iter().any(|e| e.tag == "input"));
    }

    // ── Visibility filtering ──────────────────────────────────────────

    #[test]
    fn test_filter_hidden_elements() {
        let html = r#"<html><body>
            <a href="/x" hidden>Hidden</a>
            <button style="display:none">Hidden</button>
            <div role="button" aria-hidden="true">Hidden</div>
            <a href="/y">Visible</a>
        </body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].text.as_deref(), Some("Visible"));
        assert!(snap.elements[0].is_visible);
    }

    #[test]
    fn test_filter_decorative() {
        let html = r#"<html><body>
            <a href="/real">Real</a>
            <div role="presentation" onclick="f()">Decorative</div>
            <div role="none" onclick="g()">Decorative</div>
        </body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].text.as_deref(), Some("Real"));
    }

    // ── Script/style stripping ────────────────────────────────────────

    #[test]
    fn test_script_style_stripped() {
        let html = r#"<html><head>
            <script>alert('x')</script>
            <style>body{color:red}</style>
        </head><body><a href="/ok">OK</a></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].text.as_deref(), Some("OK"));
    }

    // ── Title extraction ──────────────────────────────────────────────

    #[test]
    fn test_title_extraction() {
        let html = "<html><head><title>My Page</title></head><body><a href=\"/\">Home</a></body></html>";
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.title.as_deref(), Some("My Page"));
    }

    #[test]
    fn test_no_title() {
        let html = "<html><body><a href=\"/\">Home</a></body></html>";
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert!(snap.title.is_none());
    }

    // ── max_elements limit ─────────────────────────────────────────────

    #[test]
    fn test_max_elements() {
        let html = r#"<html><body>
            <a href="/1">1</a><a href="/2">2</a><a href="/3">3</a>
            <a href="/4">4</a><a href="/5">5</a><a href="/6">6</a>
        </body></html>"#;
        let config = DomFilter {
            max_elements: 3,
            ..Default::default()
        };
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 3);
    }

    // ── min_text_length ───────────────────────────────────────────────

    #[test]
    fn test_min_text_length() {
        let html = r#"<html><body>
            <a href="/a">A</a>
            <a href="/long">Longer text</a>
        </body></html>"#;
        let config = DomFilter {
            min_text_length: 3,
            ..Default::default()
        };
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 1);
        assert_eq!(snap.elements[0].text.as_deref(), Some("Longer text"));
    }

    // ── Empty / edge cases ────────────────────────────────────────────

    #[test]
    fn test_empty_html() {
        let config = DomFilter::default();
        let snap = extract_snapshot("", &config);
        assert_eq!(snap.elements.len(), 0);
    }

    #[test]
    fn test_no_interactive_elements() {
        let html = "<html><body><p>Just a paragraph.</p><div>Content</div></body></html>";
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements.len(), 0);
    }

    // ── to_text_summary / to_json ─────────────────────────────────────

    #[test]
    fn test_interactive_element_summary() {
        let el = InteractiveElement {
            tag: "button".into(),
            id: Some("submit".into()),
            class: Some("btn primary".into()),
            text: Some("Submit".into()),
            attributes: HashMap::new(),
            selector: "#submit".into(),
            bounding_box: None,
            is_visible: true,
        };
        let summary = el.to_text_summary();
        assert!(summary.contains("#submit"));
        assert!(summary.contains("Submit"));
        assert!(summary.contains("visible"));
    }

    #[test]
    fn test_snapshot_summary() {
        let snap = DomSnapshot {
            url: Some("https://example.com".into()),
            title: Some("Example".into()),
            elements: vec![InteractiveElement {
                tag: "a".into(),
                id: None,
                class: None,
                text: Some("Click".into()),
                attributes: HashMap::new(),
                selector: "a".into(),
                bounding_box: None,
                is_visible: true,
            }],
            total_elements: 1,
            filtered_count: 1,
        };
        let summary = snap.to_text_summary();
        assert!(summary.contains("Example"));
        assert!(summary.contains("https://example.com"));
        assert!(summary.contains("Click"));
    }

    #[test]
    fn test_to_json() {
        let el = InteractiveElement {
            tag: "input".into(),
            id: Some("email".into()),
            class: None,
            text: None,
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".into(), "email".into());
                m
            },
            selector: "#email".into(),
            bounding_box: None,
            is_visible: true,
        };
        let json = el.to_json();
        assert_eq!(json["tag"], "input");
        assert_eq!(json["id"], "email");
        assert_eq!(json["selector"], "#email");
        assert_eq!(json["is_visible"], true);
    }

    // ── Nested interactive elements ───────────────────────────────────

    #[test]
    fn test_nested_elements() {
        let html = r#"<html><body>
            <a href="/">
                <button>Inner button</button>
                Click here
            </a>
        </body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        // Both the <a> and inner <button> should be found
        assert!(snap.elements.iter().any(|e| e.tag == "a"));
        assert!(snap.elements.iter().any(|e| e.tag == "button"));
    }

    // ── Selector generation ───────────────────────────────────────────

    #[test]
    fn test_selector_with_id() {
        let html = r#"<html><body><a id="my-link" href="/x">Link</a></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements[0].selector, "#my-link");
    }

    #[test]
    fn test_selector_with_class() {
        let html = r#"<html><body><a class="nav active" href="/x">Link</a></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements[0].selector, "a.nav.active");
    }

    #[test]
    fn test_selector_with_type() {
        let html = r#"<html><body><input type="email" id="e"></body></html>"#;
        let config = DomFilter::default();
        let snap = extract_snapshot(html, &config);
        assert_eq!(snap.elements[0].selector, "#e");
    }

    // ── extract_snapshot_from_url is tested implicitly via unit tests on
    //     extract_snapshot. The HTTP path is left for integration testing.
}