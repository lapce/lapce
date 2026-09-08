//! Cost tracking for AI API calls — inspired by Claude Code's cost-tracker.
//!
//! Claude Code tracks per-model pricing and accumulates session cost in USD.
//! This module provides the same for deepseek-carp, logging cost per call
//! and optionally persisting to SQLite for later analysis.

use std::collections::HashMap;

/// Per-million-token pricing for each provider/model (USD).
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

impl ModelPricing {
    /// Known prices for supported providers (approximate, check official docs).
    pub fn for_provider_model(provider: &str, model: &str) -> Self {
        match (provider.to_lowercase().as_str(), model.to_lowercase().as_str()) {
            // DeepSeek
            ("deepseek", m) if m.contains("v4") => Self { input_per_mtok: 0.28, output_per_mtok: 0.28 },
            ("deepseek", _) => Self { input_per_mtok: 0.14, output_per_mtok: 0.28 },

            // Zhipu GLM
            ("glm", m) if m.contains("5.1") => Self { input_per_mtok: 0.50, output_per_mtok: 0.50 },
            ("glm", _) => Self { input_per_mtok: 0.50, output_per_mtok: 0.50 },

            // Moonshot Kimi
            ("kimi", m) if m.contains("2.6") => Self { input_per_mtok: 0.60, output_per_mtok: 0.60 },
            ("kimi", _) => Self { input_per_mtok: 0.60, output_per_mtok: 0.60 },

            // Minimax
            ("minimax", _) => Self { input_per_mtok: 0.50, output_per_mtok: 0.50 },

            // OpenAI
            ("openai", m) if m.contains("gpt-4o") => Self { input_per_mtok: 2.50, output_per_mtok: 10.00 },

            // Anthropic Claude
            ("claude", m) if m.contains("sonnet") => Self { input_per_mtok: 3.00, output_per_mtok: 15.00 },
            ("claude", m) if m.contains("opus") => Self { input_per_mtok: 15.00, output_per_mtok: 75.00 },

            // Copilot (included in subscription)
            ("copilot", _) => Self { input_per_mtok: 0.0, output_per_mtok: 0.0 },

            // Local (free)
            _ => Self { input_per_mtok: 0.0, output_per_mtok: 0.0 },
        }
    }

    /// Calculate USD cost from token counts.
    pub fn cost_usd(&self, prompt_tokens: u32, completion_tokens: u32) -> f64 {
        let input_mtok = prompt_tokens as f64 / 1_000_000.0;
        let output_mtok = completion_tokens as f64 / 1_000_000.0;
        (input_mtok * self.input_per_mtok) + (output_mtok * self.output_per_mtok)
    }
}

/// Session-level cost tracker.
#[derive(Debug, Clone, Default)]
pub struct CostTracker {
    /// Cumulative USD cost by provider.
    by_provider: HashMap<String, f64>,
    /// Cumulative USD cost total.
    total_cost: f64,
    /// Total tokens used.
    total_tokens: u64,
    /// Call count.
    call_count: u64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a provider call and its cost.
    pub fn record(
        &mut self,
        provider: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> f64 {
        let pricing = ModelPricing::for_provider_model(provider, model);
        let cost = pricing.cost_usd(prompt_tokens, completion_tokens);

        *self.by_provider.entry(provider.to_string()).or_insert(0.0) += cost;
        self.total_cost += cost;
        self.total_tokens += (prompt_tokens + completion_tokens) as u64;
        self.call_count += 1;

        // Log cost for observability
        if cost > 0.0 {
            tracing::info!(
                provider = %provider,
                model = %model,
                cost_usd = %format!("${:.6}", cost),
                prompt_tokens,
                completion_tokens,
                "API cost recorded"
            );
        }

        cost
    }

    /// Get total cost in USD.
    pub fn total_cost(&self) -> f64 {
        self.total_cost
    }

    /// Get per-provider breakdown.
    pub fn by_provider(&self) -> &HashMap<String, f64> {
        &self.by_provider
    }

    /// Get usage summary.
    pub fn summary(&self) -> CostSummary {
        CostSummary {
            total_cost: self.total_cost,
            total_tokens: self.total_tokens,
            call_count: self.call_count,
            by_provider: self.by_provider.clone(),
        }
    }
}

/// Summary for display/export.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_tokens: u64,
    pub call_count: u64,
    pub by_provider: HashMap<String, f64>,
}

impl std::fmt::Display for CostSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "═ Cost Summary ═")?;
        for (provider, cost) in &self.by_provider {
            writeln!(f, "  {}: ${:.4}", provider, cost)?;
        }
        writeln!(f, "  Total: ${:.4} | {} tokens | {} calls", self.total_cost, self.total_tokens, self.call_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deepseek_pricing() {
        let p = ModelPricing::for_provider_model("deepseek", "deepseek-chat");
        let cost = p.cost_usd(1000, 500);
        assert!(cost > 0.0);
        assert!(cost < 0.01); // Very small for 1500 tokens
    }

    #[test]
    fn test_local_is_free() {
        let p = ModelPricing::for_provider_model("qwen-local", "qwen2.5-7b");
        assert_eq!(p.cost_usd(1000000, 1000000), 0.0);
    }

    #[test]
    fn test_cost_tracker_accumulates() {
        let mut tracker = CostTracker::new();
        tracker.record("deepseek", "deepseek-chat", 1000, 500);
        tracker.record("deepseek", "deepseek-chat", 2000, 1000);
        assert!(tracker.total_cost() > 0.0);
        assert_eq!(tracker.call_count, 2);
        assert!(tracker.by_provider().contains_key("deepseek"));
    }

    #[test]
    fn test_cost_summary_format() {
        let mut tracker = CostTracker::new();
        tracker.record("deepseek", "deepseek-chat", 1000, 500);
        let summary = tracker.summary();
        let display = format!("{}", summary);
        assert!(display.contains("Cost Summary"));
        assert!(display.contains("deepseek"));
    }
}
