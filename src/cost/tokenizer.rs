//! Token counting — precise when possible, heuristic otherwise.
//!
//! ## Local (no network) fallback
//! Cl100k-style heuristic: `ceil(chars / 4)` for ASCII, weighted for CJK.
//! Chinese chars ≈ 1.5 tokens each. Very cheap, good enough for estimates.
//!
//! ## DeepSeek precise API
//! When an API key is available, POST to:
//!   https://api.deepseek.com/v1/tokenize
//! body: {"model":"deepseek-v3","content":"<text>"}
//! headers: Authorization: Bearer $DEEPSEEK_API_KEY
//! Response: {"tokens": [...], "num_tokens": N}
//!
//! Cache results in-memory keyed by sha2::Sha256(text). No disk cache needed
//! (keeps tokens transient).

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

pub trait TokenCounter: Send + Sync {
    fn count_sync(&self, text: &str) -> usize;
    fn name(&self) -> &'static str;
}

pub trait AsyncTokenCounter: TokenCounter {
    fn count<'a>(&'a self, text: String) -> std::pin::Pin<Box<dyn std::future::Future<Output = usize> + Send + 'a>> {
        let n = self.count_sync(&text);
        Box::pin(async move { n })
    }
}

pub struct HeuristicCounter;

impl HeuristicCounter {
    pub fn new() -> Self {
        Self
    }

    pub fn count(text: &str) -> usize {
        let mut ascii_chars = 0usize;
        let mut cjk_chars = 0usize;
        for ch in text.chars() {
            if ch.is_ascii() {
                ascii_chars += 1;
            } else {
                cjk_chars += 1;
            }
        }
        let ascii_tokens = (ascii_chars as f64 / 4.0).ceil() as usize;
        let cjk_tokens = (cjk_chars as f64 * 1.5).ceil() as usize;
        (ascii_tokens + cjk_tokens).max(1)
    }
}

impl Default for HeuristicCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter for HeuristicCounter {
    fn count_sync(&self, text: &str) -> usize {
        Self::count(text)
    }
    fn name(&self) -> &'static str {
        "heuristic"
    }
}

impl AsyncTokenCounter for HeuristicCounter {}

fn hash_prefix_u64(text: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    let digest = h.finalize();
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(buf)
}

pub struct DeepSeekApiCounter {
    api_key: String,
    cache: Arc<RwLock<HashMap<u64, usize>>>,
}

impl DeepSeekApiCounter {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn count_remote(&self, text: &str) -> Option<usize> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .ok()?;

        let body = serde_json::json!({
            "model": "deepseek-v3",
            "content": text,
        });

        let resp = client
            .post("https://api.deepseek.com/v1/tokenize")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .ok()?;

        let json: serde_json::Value = resp.json().await.ok()?;
        let n = json.get("num_tokens")?.as_u64()? as usize;
        Some(n)
    }
}

impl TokenCounter for DeepSeekApiCounter {
    fn count_sync(&self, text: &str) -> usize {
        HeuristicCounter::count(text)
    }
    fn name(&self) -> &'static str {
        "deepseek-api"
    }
}

impl AsyncTokenCounter for DeepSeekApiCounter {
    fn count<'a>(&'a self, text: String) -> std::pin::Pin<Box<dyn std::future::Future<Output = usize> + Send + 'a>> {
        Box::pin(async move {
            let key = hash_prefix_u64(&text);
            {
                let cache = self.cache.read().await;
                if let Some(&n) = cache.get(&key) {
                    return n;
                }
            }

            let n = match self.count_remote(&text).await {
                Some(n) => n,
                None => HeuristicCounter::count(&text),
            };

            {
                let mut cache = self.cache.write().await;
                cache.insert(key, n);
            }
            n
        })
    }
}

pub fn counter_from_env() -> Box<dyn TokenCounter + Send + Sync> {
    match std::env::var("DEEPSEEK_API_KEY") {
        Ok(key) if !key.is_empty() => Box::new(DeepSeekApiCounter::new(key)),
        _ => Box::new(HeuristicCounter::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_english() {
        let c = HeuristicCounter::new();
        let n = c.count_sync("Hello world this is a test");
        assert!((6..=8).contains(&n), "expected ~6-8 tokens, got {}", n);
    }

    #[test]
    fn test_heuristic_chinese() {
        let c = HeuristicCounter::new();
        let n = c.count_sync("你好世界这是一个测试");
        assert!((14..=16).contains(&n), "expected ~15 tokens (10 hanzi * 1.5), got {}", n);
    }

    #[test]
    fn test_counter_factory_no_key() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let c = counter_from_env();
        assert_eq!(c.name(), "heuristic");
    }
}
