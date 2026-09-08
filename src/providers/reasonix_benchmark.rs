//! Real cache-hit-rate benchmark against the DeepSeek API.
//!
//! Runs multiple rounds of identical-prefix requests to measure how effectively
//! the ReasonIX three-zone prefix cache achieves cache hits on DeepSeek's
//! built-in prompt caching.
//!
//! ## Usage
//!
//! ```ignore
//! use deepseek_carp::providers::reasonix_benchmark::ReasonixBenchmark;
//!
//! let bench = ReasonixBenchmark::new("sk-...");
//! let result = bench.run_benchmark(5).await;
//! println!("{}", ReasonixBenchmark::format_report(&result));
//! ```

use std::time::Instant;

use crate::providers::reasonix_cache::{
    ApiUsage, CacheMetrics, ReasonixCache, ReasonixConfig,
};

// ============================================================================
// Benchmark Result
// ============================================================================

/// Outcome of a benchmark run.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Total number of rounds requested.
    pub rounds_total: u32,
    /// Number of rounds that completed successfully (without fatal error).
    pub rounds_completed: u32,
    /// Accumulated cache metrics across all rounds.
    pub metrics: CacheMetrics,
    /// Average round latency in milliseconds.
    pub avg_latency_ms: f64,
    /// Non-fatal errors collected during the run.
    pub errors: Vec<String>,
}

// ============================================================================
// DeepSeek Pricing Constants
// ============================================================================

/// DeepSeek V3 input price: $0.14 per 1M tokens.
const DEEPSEEK_INPUT_PRICE_PER_1M: f64 = 0.14;

/// DeepSeek V3 output price: $0.28 per 1M tokens.
const DEEPSEEK_OUTPUT_PRICE_PER_1M: f64 = 0.28;

/// Compute estimated cost in USD for a given usage.
pub fn compute_cost(prompt_tokens: u64, completion_tokens: u64) -> f64 {
    (prompt_tokens as f64) * DEEPSEEK_INPUT_PRICE_PER_1M / 1e6
        + (completion_tokens as f64) * DEEPSEEK_OUTPUT_PRICE_PER_1M / 1e6
}

// ============================================================================
// ReasonixBenchmark
// ============================================================================

/// Runs a real cache-hit-rate benchmark against the DeepSeek API.
///
/// Each round sends a chat-completion request with an identical system-prompt
/// prefix but a different user question, so that after Round 0 every subsequent
/// request should benefit from DeepSeek's prefix cache.
pub struct ReasonixBenchmark {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
    cache: ReasonixCache,
}

impl ReasonixBenchmark {
    /// Create a new benchmark instance.
    ///
    /// * `api_key` — DeepSeek API key (can be empty for dry-run testing).
    pub fn new(api_key: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        let config = ReasonixConfig {
            session_id: format!("bench-{}", uuid::Uuid::new_v4()),
            ..Default::default()
        };

        Self {
            api_key: api_key.to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            client,
            cache: ReasonixCache::new(config),
        }
    }

    /// Override the model name (default: `"deepseek-chat"`).
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Override the base URL (default: `"https://api.deepseek.com"`).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_string();
        self
    }

    // -----------------------------------------------------------------------
    // Core benchmark loop
    // -----------------------------------------------------------------------

    /// Run `rounds` rounds of identical-prefix requests to measure cache behaviour.
    ///
    /// **Round structure:**
    /// - **Round 0:** Initialise prefix (system prompt + tool defs) → always MISS.
    /// - **Round 1..N:** Same prefix + different user question → should HIT on prefix tokens.
    pub async fn run_benchmark(&self, rounds: u32) -> BenchmarkResult {
        // Initialise the immutable prefix once
        let _fp = self.cache.initialize_prefix(
            "You are a helpful coding assistant. Answer concisely.",
            "",
            "",
        );

        let mut completed = 0u32;
        let mut total_latency_ms = 0f64;
        let mut errors = Vec::new();
        let mut local_metrics = CacheMetrics {
            session_id: self.cache.metrics().session_id.clone(),
            start_time: Some(Instant::now()),
            ..Default::default()
        };

        for round in 0..rounds {
            let question = match round {
                0 => "Say hello in one word.".to_string(),
                n => format!("What is {} + {}? Reply with only the number.", n, n + 1),
            };

            // Build messages via the cache's deterministic JSON builder.
            // For round > 0 we also append previous turns to the log so the
            // prefix grows monotonically — this is what makes the cache fire.
            if round > 0 {
                let prev_q = format!("What is {} + {}?", round - 1, round);
                let prev_a = format!("{}", (round - 1) + round);
                // Append previous turn to log (append-only)
                let _ = self.cache.append("user", &prev_q, round - 1);
                let _ = self.cache.append("assistant", &prev_a, round - 1);
            }

            let _json_payload = self.cache.build_request_json(&self.model, &question);

            // Build messages array from the JSON payload for the API call
            let messages = self.extract_messages_from_json(&_json_payload);

            let start = Instant::now();
            match self.call_deepseek(&messages).await {
                Ok(usage) => {
                    let elapsed = start.elapsed().as_millis() as f64;
                    total_latency_ms += elapsed;
                    completed += 1;
                    local_metrics.record_api_response(&usage);

                    tracing::info!(
                        round = round,
                        prompt_tokens = usage.prompt_tokens,
                        cache_hit = usage.prompt_cache_hit_tokens,
                        cache_miss = usage.prompt_cache_miss_tokens,
                        latency_ms = elapsed,
                        "Round completed"
                    );
                }
                Err(e) => {
                    let msg = format!("Round {}: {}", round, e);
                    tracing::warn!(round = round, error = %msg, "Round failed");
                    errors.push(msg);
                }
            }
        }

        BenchmarkResult {
            rounds_total: rounds,
            rounds_completed: completed,
            metrics: local_metrics,
            avg_latency_ms: if completed > 0 {
                total_latency_ms / completed as f64
            } else {
                0.0
            },
            errors,
        }
    }

    // -----------------------------------------------------------------------
    // API call
    // -----------------------------------------------------------------------

    /// Make one chat completion call to the DeepSeek API.
    async fn call_deepseek(
        &self,
        messages: &[serde_json::Value],
    ) -> anyhow::Result<ApiUsage> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "API returned HTTP {}: {}",
                status,
                &text[..text.len().min(500)]
            ));
        }

        Ok(self.parse_usage(&text))
    }

    // -----------------------------------------------------------------------
    // Usage parsing (compatible with both v1 and v2 formats)
    // -----------------------------------------------------------------------

    /// Parse usage from a DeepSeek response body.
    ///
    /// Supports two formats:
    /// - **v1 (standard):** `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
    /// - **v2 (details):** `usage.prompt_tokens_details.cache_creation_input_tokens` /
    ///   `cache_read_input_tokens`
    fn parse_usage(&self, resp_body: &str) -> ApiUsage {
        let json: serde_json::Value =
            match serde_json::from_str(resp_body) {
                Ok(v) => v,
                Err(_) => return ApiUsage::empty(),
            };

        let usage = &json["usage"];

        let prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
        let completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);

        // Try v1 format first
        let (hit, miss) = if let (Some(h), Some(m)) = (
            usage["prompt_cache_hit_tokens"].as_u64(),
            usage["prompt_cache_miss_tokens"].as_u64(),
        ) {
            (h, m)
        } else {
            // Fallback to v2 prompt_tokens_details format
            let details = &usage["prompt_tokens_details"];
            let creation = details["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0);
            let read = details["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0);
            // cache_read_input_tokens = hit tokens, cache_creation = miss tokens
            (read, creation)
        };

        let total_cost = compute_cost(prompt_tokens, completion_tokens);

        ApiUsage {
            prompt_tokens,
            completion_tokens,
            prompt_cache_hit_tokens: hit,
            prompt_cache_miss_tokens: miss,
            total_cost_usd: total_cost,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Extract the `messages` array from a JSON string produced by
    /// [`ReasonixCache::build_request_json()`].
    fn extract_messages_from_json(&self, json_str: &str) -> Vec<serde_json::Value> {
        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        parsed["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Report formatting
    // -----------------------------------------------------------------------

    /// Format results as an ASCII art report.
    pub fn format_report(result: &BenchmarkResult) -> String {
        let hit_pct = result.metrics.hit_rate() * 100.0;
        let token_hit_pct = result.metrics.token_cache_hit_rate() * 100.0;
        let success_rate = if result.rounds_total > 0 {
            result.rounds_completed as f64 / result.rounds_total as f64 * 100.0
        } else {
            0.0
        };

        let mut report = String::new();
        report.push_str(&format!(
            r#"╔══════════════════════════════════════════════════════════╗
║           ReasonIX Cache Hit-Rate Benchmark              ║
╠══════════════════════════════════════════════════════════╣
║ Rounds          : {rt}/{completed} ({sr:.1}% success)     ║
║ Avg Latency     : {al:.1} ms                              ║
╠══════════════════════════════════════════════════════════╣
║ Request-Level Hit Rate                                   ║
║   Prefix Hits   : {ph} ({hpp:.1}%)                          ║
║   Partial Hits  : {pa} ({pap:.1}%)                          ║
║   Misses        : {ms} ({msp:.1}%)                          ║
║   Overall       : {hp:.1}%                                  ║
╠══════════════════════════════════════════════════════════╣
║ Token-Level Cache Efficiency                             ║
║   Input Tokens  : {it}                                   ║
║   Cached Tokens : {ct} ({thp:.1}%)                         ║
║   New Tokens    : {nt}                                   ║
╠══════════════════════════════════════════════════════════╣
║ Cost                                                   ║
║   Estimated     : ${cost:.6}                            ║
║   Savings       : ${sav:.6}                            ║
╚══════════════════════════════════════════════════════════╝"#,
            rt = result.rounds_total,
            completed = result.rounds_completed,
            sr = success_rate,
            al = result.avg_latency_ms,
            ph = result.metrics.prefix_hits,
            hpp = if result.metrics.total_requests > 0 {
                result.metrics.prefix_hits as f64 / result.metrics.total_requests as f64 * 100.0
            } else {
                0.0
            },
            pa = result.metrics.partial_hits,
            pap = if result.metrics.total_requests > 0 {
                result.metrics.partial_hits as f64 / result.metrics.total_requests as f64 * 100.0
            } else {
                0.0
            },
            ms = result.metrics.misses,
            msp = if result.metrics.total_requests > 0 {
                result.metrics.misses as f64 / result.metrics.total_requests as f64 * 100.0
            } else {
                0.0
            },
            hp = hit_pct,
            it = result.metrics.total_input_tokens,
            ct = result.metrics.cached_tokens,
            thp = token_hit_pct,
            nt = result.metrics.new_tokens,
            cost = result.metrics.estimated_cost_usd,
            sav = result.metrics.estimated_savings_usd,
        ));

        if !result.errors.is_empty() {
            report.push_str("\n\n⚠ Errors:\n");
            for e in &result.errors {
                report.push_str(&format!("  • {}\n", e));
            }
        }

        report
    }
}

// ============================================================================
// ApiUsage helper
// ============================================================================

impl ApiUsage {
    /// Create an empty/zeroed usage (for parse failures).
    fn empty() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: 0,
            total_cost_usd: 0.0,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // test_benchmark_creation
    // -----------------------------------------------------------------

    #[test]
    fn test_benchmark_creation() {
        // Should succeed even with empty key (no network call at construction)
        let bench = ReasonixBenchmark::new("");
        assert_eq!(bench.api_key, "");
        assert_eq!(bench.base_url, "https://api.deepseek.com");
        assert_eq!(bench.model, "deepseek-chat");
    }

    #[test]
    fn test_benchmark_with_custom_settings() {
        let bench = ReasonixBenchmark::new("sk-test")
            .with_model("deepseek-reasoner")
            .with_base_url("https://api.example.com/v1");

        assert_eq!(bench.model, "deepseek-reasoner");
        assert_eq!(bench.base_url, "https://api.example.com/v1");
    }

    // -----------------------------------------------------------------
    // test_parse_usage_standard
    // -----------------------------------------------------------------

    #[test]
    fn test_parse_usage_standard() {
        let bench = ReasonixBenchmark::new("");

        let body = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 50,
                "total_tokens": 1050,
                "prompt_cache_hit_tokens": 800,
                "prompt_cache_miss_tokens": 200
            }
        }"#;

        let usage = bench.parse_usage(body);
        assert_eq!(usage.prompt_tokens, 1000);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.prompt_cache_hit_tokens, 800);
        assert_eq!(usage.prompt_cache_miss_tokens, 200);
        // Cost: 1000 * 0.14/1M + 50 * 0.28/1M
        assert!((usage.total_cost_usd - 0.000154).abs() < 1e-8);
    }

    // -----------------------------------------------------------------
    // test_parse_usage_v2_format
    // -----------------------------------------------------------------

    #[test]
    fn test_parse_usage_v2_format() {
        let bench = ReasonixBenchmark::new("");

        // v2 format uses prompt_tokens_details instead of flat fields
        let body = r#"{
            "id": "chatcmpl-456",
            "object": "chat.completion",
            "usage": {
                "prompt_tokens": 2000,
                "completion_tokens": 120,
                "total_tokens": 2120,
                "prompt_tokens_details": {
                    "cached_tokens": 0,
                    "cache_creation_input_tokens": 300,
                    "cache_read_input_tokens": 1700
                }
            }
        }"#;

        let usage = bench.parse_usage(body);
        assert_eq!(usage.prompt_tokens, 2000);
        assert_eq!(usage.completion_tokens, 120);
        // v2: cache_read = hit, cache_creation = miss
        assert_eq!(usage.prompt_cache_hit_tokens, 1700);
        assert_eq!(usage.prompt_cache_miss_tokens, 300);
    }

    #[test]
    fn test_parse_usage_empty_body() {
        let bench = ReasonixBenchmark::new("");
        let usage = bench.parse_usage("not valid json");
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
    }

    #[test]
    fn test_parse_usage_missing_fields() {
        let bench = ReasonixBenchmark::new("");
        let body = r#"{"id":"x","usage":{"prompt_tokens":500}}"#;
        let usage = bench.parse_usage(body);
        assert_eq!(usage.prompt_tokens, 500);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.prompt_cache_hit_tokens, 0);
        assert_eq!(usage.prompt_cache_miss_tokens, 0);
    }

    // -----------------------------------------------------------------
    // test_call_without_key_fails
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_call_without_key_fails() {
        let bench = ReasonixBenchmark::new("");
        let messages = vec![serde_json::json!({"role": "user", "content": "hello"})];

        let result = bench.call_deepseek(&messages).await;
        assert!(result.is_err(), "Expected error when no API key is provided");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("401") || err_msg.contains("HTTP"),
            "Error should mention auth failure or HTTP status, got: {}",
            err_msg
        );
    }

    // -----------------------------------------------------------------
    // test_report_formatting
    // -----------------------------------------------------------------

    #[test]
    fn test_report_formatting() {
        let result = BenchmarkResult {
            rounds_total: 5,
            rounds_completed: 4,
            metrics: CacheMetrics {
                session_id: "bench-test-001".to_string(),
                start_time: Some(Instant::now()),
                total_requests: 4,
                prefix_hits: 3,
                partial_hits: 1,
                misses: 0,
                total_input_tokens: 4000,
                cached_tokens: 3600,
                new_tokens: 600,
                estimated_cost_usd: 0.001,
                estimated_savings_usd: 0.0003,
            },
            avg_latency_ms: 1250.5,
            errors: vec!["Round 3: timeout".to_string()],
        };

        let report = ReasonixBenchmark::format_report(&result);

        // Key indicators must be present
        assert!(report.contains("ReasonIX"), "Report header missing");
        assert!(report.contains("5/4"), "Round counts missing");
        assert!(report.contains("1250.5"), "Latency missing");
        assert!(report.contains("Prefix Hits"), "Prefix hits label missing");
        assert!(report.contains("Cached Tokens"), "Cached tokens label missing");
        assert!(report.contains("75.0%"), "Hit rate percentage missing"); // 3/4 = 75%
        assert!(report.contains("90.0%"), "Token hit rate missing"); // 3600/4000 = 90%
        assert!(report.contains("Errors"), "Errors section should appear");
        assert!(report.contains("timeout"), "Error message should be included");
    }

    #[test]
    fn test_report_no_errors_when_clean() {
        let result = BenchmarkResult {
            rounds_total: 3,
            rounds_completed: 3,
            metrics: CacheMetrics {
                session_id: "clean-run".to_string(),
                start_time: Some(Instant::now()),
                ..Default::default()
            },
            avg_latency_ms: 500.0,
            errors: vec![],
        };

        let report = ReasonixBenchmark::format_report(&result);
        assert!(!report.contains("Error"), "Clean run should not show Errors section");
    }

    // -----------------------------------------------------------------
    // test_api_usage_cost_calculation
    // -----------------------------------------------------------------

    #[test]
    fn test_api_usage_cost_calculation() {
        // DeepSeek pricing: input $0.14/1M, output $0.28/1M

        // 1M input tokens + 0 output → $0.14
        let c1 = compute_cost(1_000_000, 0);
        assert!((c1 - 0.14).abs() < 1e-10, "1M input should cost $0.14, got {}", c1);

        // 0 input + 1M output → $0.28
        let c2 = compute_cost(0, 1_000_000);
        assert!((c2 - 0.28).abs() < 1e-10, "1M output should cost $0.28, got {}", c2);

        // 500k input + 200k output
        let c3 = compute_cost(500_000, 200_000);
        let expected = 500_000.0 * 0.14 / 1e6 + 200_000.0 * 0.28 / 1e6;
        assert!((c3 - expected).abs() < 1e-10, "Mixed cost mismatch: got {}", c3);

        // Zero tokens → zero cost
        let c4 = compute_cost(0, 0);
        assert_eq!(c4, 0.0);
    }

    // -----------------------------------------------------------------
    // test_benchmark_integration_round_trip
    // -----------------------------------------------------------------

    #[test]
    fn test_extract_messages_from_json() {
        let bench = ReasonixBenchmark::new("");
        let cache = ReasonixCache::new(ReasonixConfig {
            session_id: "extract-test".into(),
            ..Default::default()
        });
        cache.initialize_prefix("You are a bot.", "", "");
        cache.append("user", "Hello", 0).unwrap();

        let json = cache.build_request_json("deepseek-chat", "New msg");
        let msgs = bench.extract_messages_from_json(&json);

        // Should have: system(prefix) + user(log entry) + user(new message) = 3
        assert_eq!(msgs.len(), 3, "Expected 3 messages, got {}", msgs.len());
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "New msg");
    }

    // -----------------------------------------------------------------
    // test_compute_cost_via_api_usage
    // -----------------------------------------------------------------

    #[test]
    fn test_api_usage_empty() {
        let empty = ApiUsage::empty();
        assert_eq!(empty.prompt_tokens, 0);
        assert_eq!(empty.completion_tokens, 0);
        assert_eq!(empty.prompt_cache_hit_tokens, 0);
        assert_eq!(empty.prompt_cache_miss_tokens, 0);
        assert_eq!(empty.total_cost_usd, 0.0);
    }

    #[tokio::test]
    async fn test_run_benchmark_zero_rounds() {
        let bench = ReasonixBenchmark::new("");
        let result = bench.run_benchmark(0).await;

        assert_eq!(result.rounds_total, 0);
        assert_eq!(result.rounds_completed, 0);
        assert_eq!(result.errors.len(), 0);
        assert_eq!(result.avg_latency_ms, 0.0);
    }
}
