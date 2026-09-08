//! Auto Model + Reasoning Router — Enhanced for local/cloud hybrid routing.
//!
//! Uses task classification to route to optimal model, with heavy preference
//! for local models when appropriate.
//!
//! ## How it works
//!
//! 1. User sends message → router classifies complexity
//! 2. Complexity ≤ low    → Local model (Qwen 7B / DeepSeek-R1)
//! 3. Complexity = medium → Smart choice: Local if capable, else cloud (GLM-5.1 / Kimi-2.6)
//! 4. Complexity ≥ high   → Premium cloud model (DeepSeek V4 / Claude)
//! 5. Reasoning needed    → enable reasoning_content
//!
//! ## Routing Strategy
//! - **Local-first principle**: 60-70% of tasks should stay local
//! - **Gradual upgrade**: Only use cloud when really needed
//! - **Cost-aware**: Balance between quality and API costs

/// Task complexity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskComplexity {
    /// Simple query: "what is X", "show me Y", translate, chat
    Low = 0,
    /// Moderate task: explain code, review, debug, write simple function
    Medium = 1,
    /// Complex task: architecture, refactor, multi-file edit, algorithm design
    High = 2,
}

/// Recommended model for each complexity tier.
#[derive(Debug, Clone)]
pub struct ModelRecommendation {
    pub complexity: TaskComplexity,
    /// Provider name to use.
    pub provider: String,
    /// Model name to use.
    pub model: String,
    /// Whether to request reasoning/thinking.
    pub enable_reasoning: bool,
    /// Estimated cost tier ($/MTok).
    pub cost_tier: f64,
}

/// Auto-router: classifies intent and recommends optimal model.
pub struct AutoRouter {
    /// Keywords indicating complex tasks (multiple languages).
    high_complexity_keywords: Vec<String>,
    /// Keywords indicating medium complexity.
    medium_complexity_keywords: Vec<String>,
    /// Whether to prefer local models whenever possible.
    prefer_local: bool,
    /// Confidence threshold for local model capability (0.0-1.0).
    local_confidence_threshold: f64,
}

/// Local model capability assessment.
#[derive(Debug, Clone, PartialEq)]
pub enum LocalCapability {
    /// Definitely local model capable
    Capable,
    /// Potentially capable, but might need cloud backup
    Maybe,
    /// Definitely needs cloud model
    NeedsCloud,
}

impl Default for AutoRouter {
    fn default() -> Self {
        Self {
            high_complexity_keywords: vec![
                "architecture".into(), "architectural".into(), "重构".into(), "架构".into(),
                "refactor".into(), "refactoring".into(), "redesign".into(), "design pattern".into(),
                "distributed".into(), "分布式".into(), "concurrency".into(), "并发".into(),
                "optimize".into(), "性能优化".into(), "performance".into(),
                "machine learning".into(), "深度学习".into(), "algorithm".into(), "算法".into(),
                "multi-file".into(), "cross-module".into(), "breaking change".into(),
                "security".into(), "安全".into(), "vulnerability".into(),
                "microservice".into(), "系统设计".into(), "system design".into(),
            ],
            medium_complexity_keywords: vec![
                "debug".into(), "调试".into(), "explain".into(), "解释".into(),
                "review".into(), "审查".into(), "test".into(), "测试".into(),
                "write".into(), "implement".into(), "function".into(), "函数".into(),
                "error".into(), "fix".into(), "修复".into(), "comment".into(),
                "document".into(), "文档".into(), "api".into(),
            ],
            prefer_local: true,
            local_confidence_threshold: 0.7,
        }
    }
}

impl AutoRouter {
    /// Create a new router with local-first strategy.
    pub fn new_local_first() -> Self {
        Self::default()
    }

    /// Create a new router with custom settings.
    pub fn with_preferences(prefer_local: bool, confidence_threshold: f64) -> Self {
        Self {
            prefer_local,
            local_confidence_threshold: confidence_threshold,
            ..Self::default()
        }
    }

    /// Assess whether a task can be handled by a local model.
    pub fn assess_local_capability(&self, user_input: &str) -> LocalCapability {
        let complexity = self.classify(user_input);
        
        match complexity {
            TaskComplexity::Low => LocalCapability::Capable,
            TaskComplexity::Medium => {
                // Check for indicators that even medium tasks might need cloud
                let lower = user_input.to_lowercase();
                let cloud_indicators = self.high_complexity_keywords.iter()
                    .any(|kw| lower.contains(kw));
                
                if cloud_indicators {
                    LocalCapability::Maybe
                } else {
                    LocalCapability::Capable
                }
            }
            TaskComplexity::High => LocalCapability::NeedsCloud,
        }
    }

    /// Get recommendation with strong local preference.
    pub fn recommend_local_preferred(&self, user_input: &str) -> ModelRecommendation {
        let capability = self.assess_local_capability(user_input);
        
        match capability {
            LocalCapability::Capable => {
                // Use local Qwen for capable tasks
                ModelRecommendation {
                    complexity: self.classify(user_input),
                    provider: "qwen-local".into(),
                    model: "qwen2.5-7b-instruct-1m-q4_k_m.gguf".into(),
                    enable_reasoning: self.classify(user_input) >= TaskComplexity::Medium,
                    cost_tier: 0.0,
                }
            }
            LocalCapability::Maybe => {
                // Hybrid approach: Try local DeepSeek-R1, fall back to cloud
                ModelRecommendation {
                    complexity: TaskComplexity::Medium,
                    provider: "deepseek-local".into(),
                    model: "DeepSeek-R1-14B-Q4_K_M.gguf".into(),
                    enable_reasoning: true,
                    cost_tier: 0.0,
                }
            }
            LocalCapability::NeedsCloud => {
                // Use cloud premium model for high complexity
                self.recommend(TaskComplexity::High)
            }
        }
    }
}

impl AutoRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Classify task complexity from user input.
    pub fn classify(&self, user_input: &str) -> TaskComplexity {
        let lower = user_input.to_lowercase();

        // Check high complexity first
        let high_count = self.high_complexity_keywords.iter()
            .filter(|kw| lower.contains(kw.as_str()))
            .count();

        if high_count >= 2 {
            return TaskComplexity::High;
        }

        // Check medium complexity
        let medium_count = self.medium_complexity_keywords.iter()
            .filter(|kw| lower.contains(kw.as_str()))
            .count();

        if medium_count >= 1 || user_input.len() > 500 {
            return TaskComplexity::Medium;
        }

        // Default: low
        TaskComplexity::Low
    }

    /// Get model recommendation based on complexity.
    pub fn recommend(&self, complexity: TaskComplexity) -> ModelRecommendation {
        match complexity {
            TaskComplexity::High => ModelRecommendation {
                complexity,
                provider: "deepseek".into(),
                model: "deepseek-v4-pro".into(),
                enable_reasoning: true,
                cost_tier: 0.28,
            },
            TaskComplexity::Medium => ModelRecommendation {
                complexity,
                provider: "glm".into(),
                model: "GLM-5.1".into(),
                enable_reasoning: false,
                cost_tier: 0.50,
            },
            TaskComplexity::Low => ModelRecommendation {
                complexity,
                provider: "qwen-local".into(),
                model: "qwen2.5-7b-instruct-1m-q4_k_m.gguf".into(),
                enable_reasoning: false,
                cost_tier: 0.0,
            },
        }
    }

    /// Estimate API cost savings from auto-routing.
    pub fn estimate_savings(&self, input_chars: usize) -> f64 {
        let est_tokens = input_chars / 4; // Rough estimate
        let premium_cost = est_tokens as f64 / 1_000_000.0 * 0.28; // All on V4 @ $0.28
        let avg_cost = est_tokens as f64 / 1_000_000.0 * 0.15;     // Mixed routing ~$0.15 avg
        premium_cost - avg_cost
    }

    /// Whether the router prefers local models when possible.
    pub fn prefers_local(&self) -> bool {
        self.prefer_local
    }

    /// The confidence threshold for local model capability assessment.
    pub fn local_confidence_threshold(&self) -> f64 {
        self.local_confidence_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_high() {
        let router = AutoRouter::new();
        assert_eq!(router.classify("Refactor the distributed architecture for better performance"), TaskComplexity::High);
    }

    #[test]
    fn test_classify_medium() {
        let router = AutoRouter::new();
        assert_eq!(router.classify("Debug this function and write tests for it"), TaskComplexity::Medium);
    }

    #[test]
    fn test_classify_low() {
        let router = AutoRouter::new();
        assert_eq!(router.classify("What is Rust's ownership model?"), TaskComplexity::Low);
    }

    #[test]
    fn test_recommend_premium() {
        let router = AutoRouter::new();
        let rec = router.recommend(TaskComplexity::High);
        assert_eq!(rec.provider, "deepseek");
        assert!(rec.enable_reasoning);
    }

    #[test]
    fn test_recommend_budget() {
        let router = AutoRouter::new();
        let rec = router.recommend(TaskComplexity::Low);
        assert_eq!(rec.provider, "qwen-local");
        assert!(!rec.enable_reasoning);
        assert_eq!(rec.cost_tier, 0.0);
    }
}
