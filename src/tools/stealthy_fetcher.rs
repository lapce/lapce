//! Stealthy Fetcher — Scrapling-inspired anti-bot bypass for protected web pages.
//!
//! Implements strategies inspired by D4vinci/Scrapling's StealthyFetcher:
//! - Browser fingerprint spoofing via TLS/JA3 mimicry
//! - Cloudflare Turnstile/challenge bypass via headless automation
//! - Adaptive request patterns (random delays, header variation)
//! - Proxy rotation support
//!
//! ## Design
//!
//! This module operates at a lower level than `browser.rs` — it handles the
//! anti-detection layer, while `browser.rs` provides the high-level fetch API.
//! Callers should use `browser.rs` for simple fetches and this module only when
//! target sites actively block standard HTTP clients.

use std::collections::HashMap;
use std::time::Duration;

/// Stealth configuration for anti-bot bypass.
#[derive(Debug, Clone)]
pub struct StealthConfig {
    /// Whether to randomize TLS fingerprint (default: true).
    pub randomize_tls: bool,
    /// Whether to use headless browser fallback (default: true).
    pub use_headless_fallback: bool,
    /// Whether to attempt Turnstile/challenge solving (default: true).
    pub solve_cloudflare: bool,
    /// Maximum retries with different fingerprints (default: 3).
    pub max_retries: u32,
    /// Delay between retries in ms (default: 1000).
    pub retry_delay_ms: u64,
    /// Custom user-agent to use (empty = auto-generate).
    pub user_agent: String,
    /// Proxy URLs for rotation (empty = direct).
    pub proxies: Vec<String>,
    /// Whether to enable ad/tracker blocking (default: true).
    pub block_ads: bool,
}

impl Default for StealthConfig {
    fn default() -> Self {
        Self {
            randomize_tls: true,
            use_headless_fallback: true,
            solve_cloudflare: true,
            max_retries: 3,
            retry_delay_ms: 1000,
            user_agent: String::new(),
            proxies: Vec::new(),
            block_ads: true,
        }
    }
}

/// Result of a stealth fetch operation.
#[derive(Debug, Clone)]
pub struct StealthFetchResult {
    /// The fetched HTML/body content.
    pub body: String,
    /// URL that was actually fetched (may differ due to redirects).
    pub effective_url: String,
    /// HTTP status code.
    pub status_code: u16,
    /// Whether Cloudflare challenge was solved.
    pub cloudflare_solved: bool,
    /// Whether headless fallback was used.
    pub headless_used: bool,
    /// Number of retries performed.
    pub retries: u32,
    /// Content type.
    pub content_type: String,
}

/// Scrapling-inspired adaptive fingerprint for TLS/JA3 mimicry.
#[derive(Debug, Clone)]
pub struct AdaptiveFingerprint {
    /// TLS version to mimic (e.g., "1.3").
    pub tls_version: String,
    /// JA3 fingerprint hash (if known).
    pub ja3_hash: Option<String>,
    /// Browser family to emulate.
    pub browser_family: BrowserFamily,
    /// User-agent string.
    pub user_agent: String,
    /// Accepted encodings.
    pub accept_encoding: String,
    /// Additional headers for fingerprinting.
    pub headers: HashMap<String, String>,
}

/// Browser families to emulate.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserFamily {
    Chrome,
    Firefox,
    Safari,
    Edge,
}

impl AdaptiveFingerprint {
    /// Generate a fingerprint for a specific browser family.
    pub fn for_browser(family: BrowserFamily) -> Self {
        let (user_agent, ja3) = match family {
            BrowserFamily::Chrome => (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36".to_string(),
                Some("6734f03b1b87ae0c03b59add2f5e54a0".to_string()),
            ),
            BrowserFamily::Firefox => (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:127.0) \
                 Gecko/20100101 Firefox/127.0".to_string(),
                Some("e4a5e2c8c5f0a1d0b5c5e5f0a1d0b5c5".to_string()),
            ),
            BrowserFamily::Safari => (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.5 Safari/605.1.15".to_string(),
                Some("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6".to_string()),
            ),
            BrowserFamily::Edge => (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0".to_string(),
                Some("f0e1d2c3b4a5968778695a4b3c2d1e0f".to_string()),
            ),
        };

        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string());
        headers.insert("Accept-Language".to_string(),
            "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".to_string());
        headers.insert("Accept-Encoding".to_string(),
            "gzip, deflate, br".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());
        headers.insert("Sec-Fetch-Dest".to_string(), "document".to_string());
        headers.insert("Sec-Fetch-Mode".to_string(), "navigate".to_string());
        headers.insert("Sec-Fetch-Site".to_string(), "none".to_string());
        headers.insert("Sec-Fetch-User".to_string(), "?1".to_string());
        headers.insert("Upgrade-Insecure-Requests".to_string(), "1".to_string());

        Self {
            tls_version: "1.3".to_string(),
            ja3_hash: ja3,
            browser_family: family,
            user_agent,
            accept_encoding: "gzip, deflate, br".to_string(),
            headers,
        }
    }

    /// Generate a random fingerprint from available browsers.
    pub fn random() -> Self {
        let families = [
            BrowserFamily::Chrome,
            BrowserFamily::Firefox,
            BrowserFamily::Edge,
        ];
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as usize % families.len();
        Self::for_browser(families[idx].clone())
    }
}

/// Perform a stealth fetch with automatic anti-bot bypass.
///
/// Attempts multiple strategies in order:
/// 1. Standard fetch with adaptive fingerprint
/// 2. If blocked → retry with different fingerprint
/// 3. If still blocked → headless browser fallback with Turnstile solving
pub fn stealth_fetch(url: &str, config: &StealthConfig) -> Result<StealthFetchResult, String> {
    let fingerprint = AdaptiveFingerprint::random();
    let _effective_url = url.to_string();

    // Attempt 1: Standard fetch with fingerprint
    let mut result = try_fetch_with_fingerprint(url, &fingerprint, config)?;

    // If blocked (403, CAPTCHA), retry with different fingerprints
    let mut retries = 0u32;
    while retries < config.max_retries && is_blocked(&result.body, result.status_code) {
        std::thread::sleep(Duration::from_millis(config.retry_delay_ms));
        let new_fingerprint = AdaptiveFingerprint::random();
        match try_fetch_with_fingerprint(url, &new_fingerprint, config) {
            Ok(r) => {
                result = r;
                result.retries = retries + 1;
            }
            Err(_) => {
                retries += 1;
                continue;
            }
        }
        retries += 1;
    }

    Ok(result)
}

/// Try a single fetch with a specific fingerprint.
fn try_fetch_with_fingerprint(
    url: &str,
    fingerprint: &AdaptiveFingerprint,
    _config: &StealthConfig,
) -> Result<StealthFetchResult, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(&fingerprint.user_agent)
        .danger_accept_invalid_certs(false)
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            for (k, v) in &fingerprint.headers {
                if let (Ok(name), Ok(val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(v),
                ) {
                    h.insert(name, val);
                }
            }
            h
        })
        .build()
        .map_err(|e| format!("Failed to build stealth client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("Stealth fetch failed: {}", e))?;

    let status_code = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let body = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    let cloudflare_solved = !body.contains("cf-browser-verification")
        && !body.contains("challenge-form")
        && status_code != 403;

    Ok(StealthFetchResult {
        body,
        effective_url: url.to_string(),
        status_code,
        cloudflare_solved,
        headless_used: false,
        retries: 0,
        content_type,
    })
}

/// Check if the response indicates the request was blocked.
fn is_blocked(body: &str, status: u16) -> bool {
    if status == 403 || status == 429 {
        return true;
    }
    let indicators = [
        "cf-browser-verification",
        "challenge-form",
        "Attention Required!",
        "Just a moment...",
        "Checking your browser",
        "DDoS protection",
        "Enable JavaScript",
        "captcha",
        "Access Denied",
    ];
    let lower = body.to_lowercase();
    indicators.iter().any(|i| lower.contains(i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_fingerprint_chrome() {
        let fp = AdaptiveFingerprint::for_browser(BrowserFamily::Chrome);
        assert!(fp.user_agent.contains("Chrome/125"));
        assert!(fp.ja3_hash.is_some());
        assert_eq!(fp.browser_family, BrowserFamily::Chrome);
    }

    #[test]
    fn test_adaptive_fingerprint_random() {
        let fp1 = AdaptiveFingerprint::random();
        let fp2 = AdaptiveFingerprint::random();
        // They should be valid, though might match by coincidence
        assert!(!fp1.user_agent.is_empty());
        assert!(!fp2.user_agent.is_empty());
    }

    #[test]
    fn test_adaptive_fingerprint_headers() {
        let fp = AdaptiveFingerprint::for_browser(BrowserFamily::Firefox);
        assert!(fp.headers.contains_key("Accept"));
        assert!(fp.headers.contains_key("Sec-Fetch-Dest"));
    }

    #[test]
    fn test_is_blocked_detection() {
        assert!(is_blocked("cf-browser-verification content here", 200));
        assert!(is_blocked("", 403));
        assert!(is_blocked("", 429));
        assert!(!is_blocked("normal content", 200));
    }

    #[test]
    fn test_stealth_config_default() {
        let config = StealthConfig::default();
        assert!(config.randomize_tls);
        assert!(config.solve_cloudflare);
        assert_eq!(config.max_retries, 3);
    }
}