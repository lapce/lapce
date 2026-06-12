//! Headless browser engine — screenshot, HTML extraction, JS execution.
//!
//! ## Backends (auto-detected in priority order)
//!
//! 1. **Chrome/Chromium** subprocess — `--headless --dump-dom` / `--screenshot`
//! 2. **HTTP fallback** — curl/reqwest for basic content fetch
//!
//! ## Usage
//!
//! ```rust
//! let browser = HeadlessBrowser::new();
//! let html = browser.fetch_html("https://example.com").await?;
//! let png = browser.screenshot("https://example.com").await?;
//! ```

use std::path::PathBuf;
use std::time::Duration;

/// Result from a browser operation.
#[derive(Debug, Clone)]
pub struct BrowserResult {
    /// Raw HTML/text content.
    pub content: String,
    /// Base64-encoded PNG screenshot (empty if not captured).
    pub screenshot_b64: String,
    /// HTTP status code (0 if unknown).
    pub status_code: u16,
    /// Which backend was used.
    pub backend: BrowserBackendKind,
}

/// Available browser backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserBackendKind {
    /// Chrome/Chromium headless subprocess.
    Chrome,
    /// Simple HTTP fetch (no JS execution).
    Http,
    /// No backend available.
    Unavailable,
}

/// Headless browser engine with automatic backend selection.
pub struct HeadlessBrowser {
    /// Path to Chrome/Chromium executable (auto-detected if empty).
    chrome_path: Option<PathBuf>,
    /// Request timeout.
    timeout: Duration,
    /// Cached backend detection result.
    cached_backend: std::sync::OnceLock<BrowserBackendKind>,
}

impl Default for HeadlessBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessBrowser {
    /// Create a new headless browser with auto-detected backend.
    pub fn new() -> Self {
        Self {
            chrome_path: None,
            timeout: Duration::from_secs(30),
            cached_backend: std::sync::OnceLock::new(),
        }
    }

    /// Set a custom Chrome/Chromium path.
    pub fn with_chrome_path(mut self, path: PathBuf) -> Self {
        self.chrome_path = Some(path);
        self
    }

    /// Set request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Detect the best available browser backend.
    pub fn detect_backend(&self) -> BrowserBackendKind {
        self.cached_backend.get_or_init(|| {
            // Try Chrome/Chromium
            let candidates = if let Some(ref path) = self.chrome_path {
                vec![path.clone()]
            } else {
                vec![
                    PathBuf::from("google-chrome"),
                    PathBuf::from("google-chrome-stable"),
                    PathBuf::from("chromium"),
                    PathBuf::from("chromium-browser"),
                    PathBuf::from("chrome"),
                    PathBuf::from("chrome.exe"),
                ]
                .into_iter()
                .map(|p| {
                    if p.is_absolute() {
                        p
                    } else {
                        // Try to find in PATH via `which`
                        let found = find_in_path(p.to_str().unwrap_or(""));
                        found.unwrap_or(p)
                    }
                })
                .collect()
            };

            for path in &candidates {
                if path.exists() || which_rs_available(path) {
                    return BrowserBackendKind::Chrome;
                }
            }

            // Fallback: HTTP fetch
            BrowserBackendKind::Http
        }).clone()
    }

    /// Fetch the full HTML/DOM of a page.
    pub async fn fetch_html(&self, url: &str) -> Result<BrowserResult, String> {
        match self.detect_backend() {
            BrowserBackendKind::Chrome => self.fetch_html_via_chrome(url).await,
            BrowserBackendKind::Http => self.fetch_html_via_http(url).await,
            BrowserBackendKind::Unavailable => Err("No browser backend available".into()),
        }
    }

    /// Take a screenshot of a page.
    pub async fn screenshot(&self, url: &str) -> Result<BrowserResult, String> {
        match self.detect_backend() {
            BrowserBackendKind::Chrome => self.screenshot_via_chrome(url).await,
            BrowserBackendKind::Http => {
                // HTTP can't screenshot, but return content
                self.fetch_html_via_http(url).await
            }
            BrowserBackendKind::Unavailable => Err("No browser backend available".into()),
        }
    }

    // ── Chrome subprocess backend ──

    async fn fetch_html_via_chrome(&self, url: &str) -> Result<BrowserResult, String> {
        let chrome = self.resolve_chrome_path()?;

        let output = tokio::process::Command::new(&chrome)
            .args([
                "--headless",
                "--disable-gpu",
                "--no-sandbox",
                "--disable-dev-shm-usage",
                "--dump-dom",
                url,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| format!("Failed to launch Chrome: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Chrome exited with code {:?}",
                output.status.code()
            ));
        }

        let html = String::from_utf8_lossy(&output.stdout).to_string();

        Ok(BrowserResult {
            content: html,
            screenshot_b64: String::new(),
            status_code: 200,
            backend: BrowserBackendKind::Chrome,
        })
    }

    async fn screenshot_via_chrome(&self, url: &str) -> Result<BrowserResult, String> {
        let chrome = self.resolve_chrome_path()?;
        let temp_dir = std::env::temp_dir();
        let screenshot_path = temp_dir.join(format!(
            "carp_screenshot_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));

        let output = tokio::process::Command::new(&chrome)
            .args([
                "--headless",
                "--disable-gpu",
                "--no-sandbox",
                "--disable-dev-shm-usage",
                &format!("--screenshot={}", screenshot_path.display()),
                "--window-size=1920,1080",
                url,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| format!("Failed to launch Chrome for screenshot: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Chrome screenshot exited with code {:?}",
                output.status.code()
            ));
        }

        // Read screenshot as base64
        let png_bytes = tokio::fs::read(&screenshot_path)
            .await
            .map_err(|e| format!("Failed to read screenshot: {}", e))?;

        let b64 = use_base64_encode(&png_bytes);

        // Clean up temp file
        let _ = tokio::fs::remove_file(&screenshot_path).await;

        Ok(BrowserResult {
            content: format!("[Screenshot saved, {} bytes PNG]", png_bytes.len()),
            screenshot_b64: b64,
            status_code: 200,
            backend: BrowserBackendKind::Chrome,
        })
    }

    fn resolve_chrome_path(&self) -> Result<PathBuf, String> {
        if let Some(ref path) = self.chrome_path {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        // Search PATH
        let candidates = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "chrome",
            "chrome.exe",
        ];

        for name in &candidates {
            if let Some(path) = find_in_path(name) {
                return Ok(path);
            }
        }

        Err("Chrome/Chromium not found in PATH. Install Chrome or use HTTP fallback.".into())
    }

    // ── HTTP fallback backend ──

    async fn fetch_html_via_http(&self, url: &str) -> Result<BrowserResult, String> {
        use crate::tools::browser::fetch_url_async;
        let result = fetch_url_async(url).await?;

        let status_code = if result.contains("Status: 200") {
            200
        } else if result.contains("Status: 404") {
            404
        } else if result.contains("Status: 5") {
            500
        } else {
            0
        };

        Ok(BrowserResult {
            content: result,
            screenshot_b64: String::new(),
            status_code,
            backend: BrowserBackendKind::Http,
        })
    }
}

// ── Helper: find executable in PATH ──

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        // Also try with .exe on Windows
        if cfg!(windows) {
            let with_exe = dir.join(format!("{}.exe", name));
            if with_exe.exists() {
                return Some(with_exe);
            }
        }
    }
    None
}

fn which_rs_available(path: &PathBuf) -> bool {
    // Same as find_in_path but using the file stem
    path.file_name()
        .map(|n| find_in_path(&n.to_string_lossy()).is_some())
        .unwrap_or(false)
}

/// Encode bytes to base64 (without pulling in the base64 crate).
fn use_base64_encode(bytes: &[u8]) -> String {
    // Simple base64 encoder using rust standard library
    // Note: this is a minimal implementation
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        let i0 = ((triple >> 18) & 0x3F) as usize;
        let i1 = ((triple >> 12) & 0x3F) as usize;
        let i2 = ((triple >> 6) & 0x3F) as usize;
        let i3 = (triple & 0x3F) as usize;
        result.push(CHARS[i0] as char);
        result.push(CHARS[i1] as char);
        match chunk.len() {
            1 => {
                result.push('=');
                result.push('=');
            }
            2 => {
                result.push(CHARS[i2] as char);
                result.push('=');
            }
            _ => {
                result.push(CHARS[i2] as char);
                result.push(CHARS[i3] as char);
            }
        }
    }
    result
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(use_base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(use_base64_encode(b"Rust"), "UnVzdA==");
        assert_eq!(use_base64_encode(b"a"), "YQ==");
        assert_eq!(use_base64_encode(b"ab"), "YWI=");
        assert_eq!(use_base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn test_detect_backend() {
        let browser = HeadlessBrowser::new();
        let backend = browser.detect_backend();
        // On CI or machines without Chrome, this will be Http
        // Either is acceptable
        assert!(backend == BrowserBackendKind::Http || backend == BrowserBackendKind::Chrome);
    }

    #[tokio::test]
    async fn test_fetch_html_invalid_url() {
        let browser = HeadlessBrowser::new();
        let result = browser.fetch_html("not-a-valid-url").await;
        // Should fail but gracefully
        assert!(result.is_err());
    }

    #[test]
    fn test_browser_backend_kind_debug() {
        assert_eq!(format!("{:?}", BrowserBackendKind::Chrome), "Chrome");
        assert_eq!(format!("{:?}", BrowserBackendKind::Http), "Http");
    }

    #[test]
    fn test_browser_result_creation() {
        let result = BrowserResult {
            content: "hello".into(),
            screenshot_b64: "".into(),
            status_code: 200,
            backend: BrowserBackendKind::Http,
        };
        assert_eq!(result.status_code, 200);
        assert_eq!(result.backend, BrowserBackendKind::Http);
    }
}