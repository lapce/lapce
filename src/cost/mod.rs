//! Cost budget management — track, limit, and alert on LLM API spending.
//!
//! Features:
//! - Per-session, hourly, and daily budget limits with auto-enforcement
//! - Real-time cost tracking per model/agent/task
//! - Budget alerts at configurable thresholds (50%, 75%, 90%)
//! - Cost projection: estimate remaining budget based on usage rate
//! - Persistent cost history across sessions (.dscarp/costs.json)

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

pub mod tokenizer;

/// Pricing info for a model (per 1M tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub model_name: String,
    /// Input price per 1M tokens (USD).
    pub input_per_1m: f64,
    /// Output price per 1M tokens (USD).
    pub output_per_1m: f64,
    /// Cache read price per 1M tokens (USD) — for DeepSeek prefix cache.
    pub cache_read_per_1m: f64,
}

impl ModelPricing {
    /// DeepSeek V3 pricing (as of 2025).
    pub fn deepseek_v3() -> Self {
        Self {
            model_name: "deepseek-v3".into(),
            input_per_1m: 0.27,
            output_per_1m: 1.10,
            cache_read_per_1m: 0.07,
        }
    }

    /// DeepSeek R1 pricing (reasoning model).
    pub fn deepseek_r1() -> Self {
        Self {
            model_name: "deepseek-r1".into(),
            input_per_1m: 0.55,
            output_per_1m: 2.19,
            cache_read_per_1m: 0.14,
        }
    }

    /// Calculate cost for given token counts.
    pub fn calculate(&self, input_tokens: u64, output_tokens: u64, cache_hit_tokens: u64) -> CostBreakdown {
        let input_cost = input_tokens as f64 / 1_000_000.0 * self.input_per_1m;
        let output_cost = output_tokens as f64 / 1_000_000.0 * self.output_per_1m;
        let cache_cost = cache_hit_tokens as f64 / 1_000_000.0 * self.cache_read_per_1m;
        let total = input_cost + output_cost + cache_cost;
        CostBreakdown {
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            input_cost,
            output_cost,
            cache_cost,
            total,
            model: self.model_name.clone(),
            timestamp: now_ts(),
        }
    }
}

/// Detailed cost breakdown for a single API call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_hit_tokens: u64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_cost: f64,
    pub total: f64,
    pub model: String,
    pub timestamp: u64,
}

impl CostBreakdown {
    pub fn new(model: &str, input: u64, output: u64, cache: u64, pricing: &ModelPricing) -> Self {
        let mut breakdown = pricing.calculate(input, output, cache);
        breakdown.model = model.to_string();
        breakdown.timestamp = now_ts();
        breakdown
    }
}

/// Budget limits configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Maximum spend per session (USD). None = unlimited.
    pub session_limit: Option<f64>,
    /// Maximum spend per hour (USD).
    pub hourly_limit: Option<f64>,
    /// Maximum spend per day (USD).
    pub daily_limit: Option<f64>,
    /// Maximum spend per month (USD).
    pub monthly_budget: f64,
    /// Alert thresholds (e.g., [0.50, 0.75, 0.90]).
    pub alert_thresholds: Vec<f64>,
    /// Action when budget exceeded: "warn" | "block" | "switch_model".
    pub exceed_action: ExceedAction,
    /// Fallback cheaper model when switching.
    pub fallback_model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceedAction {
    /// Just warn, allow continue.
    Warn,
    /// Block further requests.
    Block,
    /// Switch to cheaper fallback model.
    SwitchModel,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            session_limit: Some(5.0),
            hourly_limit: Some(20.0),
            daily_limit: Some(100.0),
            monthly_budget: 2000.0,
            alert_thresholds: vec![0.5, 0.75, 0.9],
            exceed_action: ExceedAction::Warn,
            fallback_model: Some("deepseek-v3".into()),
        }
    }
}

/// Budget state — tracks actual spending against limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetState {
    /// Session start time.
    pub session_start: u64,
    /// Current hour window start.
    pub hour_start: u64,
    /// Current day window start (Unix day).
    pub day_start: u64,
    /// Total spent this session.
    pub session_spent: f64,
    /// Spent this hour.
    pub hourly_spent: f64,
    /// Spent today.
    pub daily_spent: f64,
    /// All-time historical spending (persisted).
    pub lifetime_spent: f64,
    /// Number of alerts fired (per threshold).
    pub alerts_fired: HashMap<String, u32>,
    /// Whether budget was exceeded this session.
    pub exceeded: bool,
    /// Last alert level that was triggered.
    pub last_alert_level: Option<f64>,
}

/// Alert information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlert {
    pub alert_type: AlertType,
    pub level: f64,
    pub limit_type: LimitType,
    pub spent: f64,
    pub limit: f64,
    pub message: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertType {
    ThresholdReached,
    LimitExceeded,
    ProjectionWarning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitType {
    Session,
    Hourly,
    Daily,
}

/// Main cost manager.
pub struct CostManager {
    config: BudgetConfig,
    state: Arc<RwLock<BudgetState>>,
    /// Known model pricing table.
    pricing: HashMap<String, ModelPricing>,
    /// Usage per model.
    usage: std::sync::RwLock<HashMap<String, UsageRecord>>,
    /// Call history for this session.
    call_history: Arc<RwLock<Vec<CostBreakdown>>>,
    /// Persistent storage path.
    store_path: PathBuf,
}

impl CostManager {
    pub fn new(config: BudgetConfig, workspace: impl Into<PathBuf>) -> Self {
        let ws = workspace.into();
        let store_path = ws.join(".dscarp").join("costs.json");

        let mut pricing = HashMap::new();
        let dv3 = ModelPricing::deepseek_v3();
        let dr1 = ModelPricing::deepseek_r1();
        pricing.insert(dv3.model_name.clone(), dv3);
        pricing.insert(dr1.model_name.clone(), dr1);

        // Load persisted state if exists
        let state = Self::load_state(&store_path).unwrap_or_else(|| BudgetState {
            session_start: now_ts(),
            hour_start: now_ts(),
            day_start: now_ts() - (now_ts() % 86400),
            session_spent: 0.0,
            hourly_spent: 0.0,
            daily_spent: 0.0,
            lifetime_spent: 0.0,
            alerts_fired: HashMap::new(),
            exceeded: false,
            last_alert_level: None,
        });

        Self {
            config,
            state: Arc::new(RwLock::new(state)),
            pricing,
            usage: std::sync::RwLock::new(HashMap::new()),
            call_history: Arc::new(RwLock::new(Vec::new())),
            store_path,
        }
    }

    /// Record a cost from an API call. Returns alerts if thresholds crossed.
    pub async fn record_cost(&self, breakdown: &CostBreakdown) -> Vec<BudgetAlert> {
        let mut alerts = Vec::new();

        {
            let mut state = self.state.write().await;
            state.session_spent += breakdown.total;
            state.hourly_spent += breakdown.total;
            state.daily_spent += breakdown.total;
            state.lifetime_spent += breakdown.total;

            // Check session limit
            if let Some(limit) = self.config.session_limit {
                let spent = state.session_spent;
                let ratio = spent / limit;
                alerts.extend(self.check_thresholds(
                    &mut state,
                    ratio,
                    LimitType::Session,
                    limit,
                    spent,
                ));
                if spent > limit && !state.exceeded {
                    state.exceeded = true;
                    alerts.push(self.build_exceeded_alert(
                        LimitType::Session,
                        limit,
                        spent,
                    ));
                }
            }

            // Check hourly limit
            if let Some(limit) = self.config.hourly_limit {
                let spent = state.hourly_spent;
                let ratio = spent / limit;
                alerts.extend(self.check_thresholds(
                    &mut state,
                    ratio,
                    LimitType::Hourly,
                    limit,
                    spent,
                ));
                if spent > limit && !state.exceeded {
                    state.exceeded = true;
                    alerts.push(self.build_exceeded_alert(
                        LimitType::Hourly,
                        limit,
                        spent,
                    ));
                }
            }

            // Check daily limit
            if let Some(limit) = self.config.daily_limit {
                let spent = state.daily_spent;
                let ratio = spent / limit;
                alerts.extend(self.check_thresholds(
                    &mut state,
                    ratio,
                    LimitType::Daily,
                    limit,
                    spent,
                ));
                if spent > limit && !state.exceeded {
                    state.exceeded = true;
                    alerts.push(self.build_exceeded_alert(
                        LimitType::Daily,
                        limit,
                        spent,
                    ));
                }
            }

            // Projection warning: if projected daily spend exceeds daily limit
            if let Some(daily_limit) = self.config.daily_limit {
                let projected = Self::project_daily(&state);
                if projected > daily_limit && state.daily_spent < daily_limit {
                    let key = "projection_warning".to_string();
                    let prev = state.alerts_fired.entry(key).or_insert(0);
                    if *prev == 0 {
                        *prev += 1;
                        alerts.push(BudgetAlert {
                            alert_type: AlertType::ProjectionWarning,
                            level: state.daily_spent / daily_limit,
                            limit_type: LimitType::Daily,
                            spent: state.daily_spent,
                            limit: daily_limit,
                            message: format!(
                                "Projected daily spend ${:.2} exceeds limit ${:.2}",
                                projected, daily_limit
                            ),
                            suggested_action: "Reduce usage or increase budget".into(),
                        });
                    }
                }
            }
        }

        // Store in call history
        self.call_history.write().await.push(breakdown.clone());

        // Update per-model usage
        {
            let mut usage = self.usage.write().unwrap();
            let record = usage.entry(breakdown.model.clone()).or_insert(UsageRecord {
                input_tokens: 0,
                output_tokens: 0,
                last_used: 0,
            });
            record.input_tokens += breakdown.input_tokens;
            record.output_tokens += breakdown.output_tokens;
            record.last_used = now_ts();
        }

        // Persist state
        if let Err(e) = self.save_state().await {
            tracing::warn!(error = %e, "Failed to persist cost state");
        }

        alerts
    }

    /// Check if a new request should be allowed based on budget.
    pub async fn check_budget(&self, estimated_cost: f64) -> BudgetDecision {
        let state = self.state.read().await;

        // Check session limit
        if let Some(limit) = self.config.session_limit {
            if state.session_spent + estimated_cost > limit {
                return BudgetDecision {
                    allowed: false,
                    reason: Some(format!(
                        "Would exceed session budget ${:.2} (current ${:.2} + est ${:.2})",
                        limit, state.session_spent, estimated_cost
                    )),
                    action: self.config.exceed_action,
                };
            }
        }

        // Check hourly limit
        if let Some(limit) = self.config.hourly_limit {
            if state.hourly_spent + estimated_cost > limit {
                return BudgetDecision {
                    allowed: false,
                    reason: Some(format!(
                        "Would exceed hourly budget ${:.2} (current ${:.2} + est ${:.2})",
                        limit, state.hourly_spent, estimated_cost
                    )),
                    action: self.config.exceed_action,
                };
            }
        }

        // Check daily limit
        if let Some(limit) = self.config.daily_limit {
            if state.daily_spent + estimated_cost > limit {
                return BudgetDecision {
                    allowed: false,
                    reason: Some(format!(
                        "Would exceed daily budget ${:.2} (current ${:.2} + est ${:.2})",
                        limit, state.daily_spent, estimated_cost
                    )),
                    action: self.config.exceed_action,
                };
            }
        }

        BudgetDecision {
            allowed: true,
            reason: None,
            action: ExceedAction::Warn,
        }
    }

    /// Get current budget status report.
    pub async fn status(&self) -> BudgetStatus {
        let state = self.state.read().await;
        let history = self.call_history.read().await;

        let call_count = history.len();
        let avg_cost_per_call = if call_count == 0 {
            0.0
        } else {
            state.session_spent / call_count as f64
        };

        BudgetStatus {
            session_spent: state.session_spent,
            session_limit: self.config.session_limit,
            session_pct: self.config.session_limit.map(|l| state.session_spent / l),
            hourly_spent: state.hourly_spent,
            hourly_limit: self.config.hourly_limit,
            daily_spent: state.daily_spent,
            daily_limit: self.config.daily_limit,
            lifetime_spent: state.lifetime_spent,
            call_count,
            avg_cost_per_call,
            projected_daily: Self::project_daily(&state),
            exceeded: state.exceeded,
        }
    }

    /// Get pricing info for a model.
    pub fn get_pricing(&self, model: &str) -> Option<&ModelPricing> {
        self.pricing.get(model)
    }

    /// Register custom pricing for a model.
    pub fn register_pricing(&mut self, pricing: ModelPricing) {
        let name = pricing.model_name.clone();
        self.pricing.insert(name, pricing);
    }

    /// Monthly billing report.
    pub fn monthly_report(&self, year: u32, month: u32) -> CostReport {
        let mut total_tokens = 0u64;
        let mut total_cost = 0.0;
        let mut by_model = HashMap::new();

        let usage = self.usage.read().unwrap();
        for (model, u) in usage.iter() {
            let cost = self.calculate_cost(model, u.input_tokens, u.output_tokens);
            total_tokens += u.input_tokens + u.output_tokens;
            total_cost += cost;
            by_model.insert(model.clone(), (u.input_tokens, u.output_tokens, cost));
        }

        CostReport {
            year,
            month,
            total_tokens,
            total_cost,
            by_model,
            budget_exceeded: total_cost > self.config.monthly_budget,
        }
    }

    /// Export cost data to CSV format (manual formatting, no external crate).
    pub fn export_csv(&self, path: &PathBuf) -> anyhow::Result<()> {
        let mut csv_content = String::from("Model,InputTokens,OutputTokens,Cost,Timestamp\n");
        let usage = self.usage.read().unwrap();
        for (model, u) in usage.iter() {
            let cost = self.calculate_cost(model, u.input_tokens, u.output_tokens);
            csv_content.push_str(&format!(
                "{},{},{},{:.6},{}\n",
                model, u.input_tokens, u.output_tokens, cost, u.last_used
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, csv_content)?;
        Ok(())
    }

    /// Calculate cost for a model given token counts.
    fn calculate_cost(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        let fallback = ModelPricing {
            model_name: model.to_string(),
            input_per_1m: 0.14,
            output_per_1m: 0.28,
            cache_read_per_1m: 0.07,
        };
        let pricing = self.pricing.get(model).unwrap_or(&fallback);
        (input_tokens as f64 / 1_000_000.0) * pricing.input_per_1m
            + (output_tokens as f64 / 1_000_000.0) * pricing.output_per_1m
    }

    /// Budget forecast: predict remaining budget.
    pub fn forecast(&self, expected_daily_use: f64) -> Forecast {
        let spent_today = self.daily_usage();
        let daily_budget = self.config.daily_limit.unwrap_or(100.0);
        let monthly_budget = self.config.monthly_budget;
        let spent_this_month = self.monthly_usage();

        Forecast {
            daily_remaining: daily_budget - spent_today,
            monthly_remaining: monthly_budget - spent_this_month,
            estimated_days_remaining: if expected_daily_use > 0.0 {
                (monthly_budget - spent_this_month) / expected_daily_use
            } else {
                f64::INFINITY
            },
            daily_budget_depletion_rate: if daily_budget > 0.0 {
                spent_today / daily_budget
            } else {
                0.0
            },
            monthly_depletion_rate: if monthly_budget > 0.0 {
                spent_this_month / monthly_budget
            } else {
                0.0
            },
        }
    }

    /// Daily usage total.
    fn daily_usage(&self) -> f64 {
        let usage = self.usage.read().unwrap();
        usage
            .values()
            .map(|u| self.calculate_cost("", u.input_tokens, u.output_tokens))
            .sum()
    }

    /// Monthly usage total (approximation from historical data).
    fn monthly_usage(&self) -> f64 {
        self.daily_usage() * 30.0
    }

    /// Project expected daily spend based on current rate.
    fn project_daily(state: &BudgetState) -> f64 {
        let elapsed = now_ts().saturating_sub(state.day_start);
        if elapsed == 0 {
            return state.daily_spent;
        }
        let secs_in_day = 86464u64;
        state.daily_spent * (secs_in_day as f64 / elapsed as f64)
    }

    /// Reset hourly counter (call every hour).
    pub async fn reset_hourly(&self) {
        let mut state = self.state.write().await;
        state.hourly_spent = 0.0;
        state.hour_start = now_ts();
    }

    /// Reset daily counter.
    pub async fn reset_daily(&self) {
        let mut state = self.state.write().await;
        state.daily_spent = 0.0;
        state.day_start = now_ts() - (now_ts() % 86400);
    }

    /// Save state to persistent storage.
    async fn save_state(&self) -> anyhow::Result<()> {
        let state = self.state.read().await;
        let dir = self.store_path.parent().expect("store path must have parent");
        tokio::fs::create_dir_all(dir).await?;
        let json = serde_json::to_string_pretty(&*state)?;
        tokio::fs::write(&self.store_path, json).await?;
        Ok(())
    }

    /// Load state from persistent storage.
    fn load_state(path: &PathBuf) -> Option<BudgetState> {
        if path.exists() {
            let json = std::fs::read_to_string(path).ok()?;
            serde_json::from_str(&json).ok()
        } else {
            None
        }
    }

    /// Build a limit-exceeded alert.
    fn build_exceeded_alert(
        &self,
        limit_type: LimitType,
        limit: f64,
        spent: f64,
    ) -> BudgetAlert {
        let limit_name = match limit_type {
            LimitType::Session => "session",
            LimitType::Hourly => "hourly",
            LimitType::Daily => "daily",
        };
        BudgetAlert {
            alert_type: AlertType::LimitExceeded,
            level: 1.0,
            limit_type,
            spent,
            limit,
            message: format!(
                "{} budget ${:.2} exceeded (spent ${:.2})",
                limit_name, limit, spent
            ),
            suggested_action: match self.config.exceed_action {
                ExceedAction::Block => "Blocking further requests".into(),
                ExceedAction::Warn => "Consider reducing usage".into(),
                ExceedAction::SwitchModel => format!(
                    "Switching to {}",
                    self.config
                        .fallback_model
                        .as_deref()
                        .unwrap_or("cheaper model")
                ),
            },
        }
    }

    /// Internal: check alert thresholds and fire alerts.
    fn check_thresholds(
        &self,
        state: &mut BudgetState,
        ratio: f64,
        limit_type: LimitType,
        limit: f64,
        spent: f64,
    ) -> Vec<BudgetAlert> {
        let mut alerts = Vec::new();
        let limit_name = match limit_type {
            LimitType::Session => "session",
            LimitType::Hourly => "hourly",
            LimitType::Daily => "daily",
        };

        for &threshold in &self.config.alert_thresholds {
            if ratio >= threshold {
                let key = format!("{:?}_{}", limit_type, threshold);
                let prev = state.alerts_fired.entry(key).or_insert(0);
                if *prev == 0 {
                    // Fire each threshold only once
                    *prev += 1;
                    state.last_alert_level =
                        Some(threshold.max(state.last_alert_level.unwrap_or(0.0)));
                    alerts.push(BudgetAlert {
                        alert_type: AlertType::ThresholdReached,
                        level: threshold,
                        limit_type,
                        spent,
                        limit,
                        message: format!(
                            "{} budget at {:.0}% (${:.2}/${:.2})",
                            limit_name,
                            threshold * 100.0,
                            spent,
                            limit
                        ),
                        suggested_action: "Monitor spending closely".into(),
                    });
                }
            }
        }
        alerts
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetDecision {
    pub allowed: bool,
    pub reason: Option<String>,
    pub action: ExceedAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub session_spent: f64,
    pub session_limit: Option<f64>,
    pub session_pct: Option<f64>,
    pub hourly_spent: f64,
    pub hourly_limit: Option<f64>,
    pub daily_spent: f64,
    pub daily_limit: Option<f64>,
    pub lifetime_spent: f64,
    pub call_count: usize,
    pub avg_cost_per_call: f64,
    pub projected_daily: f64,
    pub exceeded: bool,
}

/// Track usage per model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_used: u64,
}

/// Monthly cost report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostReport {
    pub year: u32,
    pub month: u32,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub by_model: HashMap<String, (u64, u64, f64)>,
    pub budget_exceeded: bool,
}

/// Budget forecast information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub daily_remaining: f64,
    pub monthly_remaining: f64,
    pub estimated_days_remaining: f64,
    pub daily_budget_depletion_rate: f64,
    pub monthly_depletion_rate: f64,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time must be after UNIX_EPOCH")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_pricing_calculate() {
        let pricing = ModelPricing::deepseek_v3();
        let breakdown = pricing.calculate(1_000_000, 500_000, 200_000);

        assert!((breakdown.input_cost - 0.27).abs() < 0.001);
        assert!((breakdown.output_cost - 0.55).abs() < 0.001);
        assert!((breakdown.cache_cost - 0.014).abs() < 0.001);
        assert!((breakdown.total - 0.834).abs() < 0.001);
        assert_eq!(breakdown.input_tokens, 1_000_000);
        assert_eq!(breakdown.output_tokens, 500_000);
    }

    #[test]
    fn test_deepseek_r1_pricing() {
        let pricing = ModelPricing::deepseek_r1();
        assert_eq!(pricing.model_name, "deepseek-r1");
        assert!(pricing.input_per_1m > pricing.cache_read_per_1m);
        let breakdown = pricing.calculate(1_000_000, 1_000_000, 0);
        assert!(breakdown.total > 0.0);
    }

    #[test]
    fn test_budget_config_defaults() {
        let config = BudgetConfig::default();
        assert_eq!(config.session_limit, Some(5.0));
        assert_eq!(config.hourly_limit, Some(20.0));
        assert_eq!(config.daily_limit, Some(100.0));
        assert_eq!(config.alert_thresholds, vec![0.5, 0.75, 0.9]);
        assert_eq!(config.exceed_action, ExceedAction::Warn);
        assert_eq!(config.fallback_model, Some("deepseek-v3".into()));
    }

    #[tokio::test]
    async fn test_record_cost_basic() {
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let breakdown = CostBreakdown::new("deepseek-v3", 1000, 500, 100, &pricing);

        let alerts = manager.record_cost(&breakdown).await;
        // Small cost shouldn't trigger any alerts at default thresholds
        assert!(alerts.is_empty());

        let status = manager.status().await;
        assert!(status.session_spent > 0.0);
        assert_eq!(status.call_count, 1);
        assert!((status.avg_cost_per_call - status.session_spent).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_session_limit_enforcement() {
        let config = BudgetConfig {
            session_limit: Some(0.01), // Very low limit
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        // A call that will exceed the tiny session budget
        let breakdown = CostBreakdown::new("deepseek-v3", 1_000_000, 1_000_000, 0, &pricing);

        let alerts = manager.record_cost(&breakdown).await;
        // Should trigger exceed alert
        assert!(!alerts.is_empty());
        assert!(alerts.iter().any(|a| a.alert_type == AlertType::LimitExceeded));

        let status = manager.status().await;
        assert!(status.exceeded);
    }

    #[tokio::test]
    async fn test_alert_threshold_firing() {
        let config = BudgetConfig {
            session_limit: Some(1.0),
            alert_thresholds: vec![0.5],
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();

        // First call: small, under 50%
        let b1 = CostBreakdown::new("deepseek-v3", 100_000, 100_000, 0, &pricing);
        let alerts1 = manager.record_cost(&b1).await;
        assert!(alerts1.is_empty()); // Under 50%

        // Second call: push over 50%
        let b2 = CostBreakdown::new("deepseek-v3", 2_000_000, 2_000_000, 0, &pricing);
        let alerts2 = manager.record_cost(&b2).await;
        assert!(!alerts2.is_empty());
        assert!(alerts2.iter().any(|a| {
            a.alert_type == AlertType::ThresholdReached && (a.level - 0.5).abs() < 0.01
        }));

        // Third call: same threshold should NOT fire again
        let b3 = CostBreakdown::new("deepseek-v3", 100_000, 100_000, 0, &pricing);
        let alerts3 = manager.record_cost(&b3).await;
        assert!(!alerts3.iter().any(|a| {
            a.alert_type == AlertType::ThresholdReached && (a.level - 0.5).abs() < 0.01
        }));
    }

    #[tokio::test]
    async fn test_status_report() {
        let config = BudgetConfig {
            session_limit: Some(10.0),
            hourly_limit: Some(50.0),
            daily_limit: Some(200.0),
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let b1 = CostBreakdown::new("deepseek-v3", 500_000, 250_000, 0, &pricing);
        let b2 = CostBreakdown::new("deepseek-r1", 200_000, 100_000, 0, &pricing);
        manager.record_cost(&b1).await;
        manager.record_cost(&b2).await;

        let status = manager.status().await;
        assert_eq!(status.session_limit, Some(10.0));
        assert_eq!(status.hourly_limit, Some(50.0));
        assert_eq!(status.daily_limit, Some(200.0));
        assert_eq!(status.call_count, 2);
        assert!(status.session_spent > 0.0);
        assert!(status.session_pct.unwrap() < 1.0); // Should be well under 100%
        assert!(status.projected_daily >= status.daily_spent); // Projected >= actual
        assert!(!status.exceeded);
    }

    #[test]
    fn test_projection_calculation() {
        // Test that projection scales based on elapsed time
        let state = BudgetState {
            session_start: now_ts(),
            hour_start: now_ts(),
            day_start: now_ts(),
            session_spent: 0.0,
            hourly_spent: 0.0,
            daily_spent: 10.0,
            lifetime_spent: 0.0,
            alerts_fired: HashMap::new(),
            exceeded: false,
            last_alert_level: None,
        };

        // With very recent day_start, projection should be much higher than spent
        let projected = CostManager::project_daily(&state);
        assert!(projected >= state.daily_spent);
    }

    #[tokio::test]
    async fn test_persistence_save_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, dir.path());

        let pricing = ModelPricing::deepseek_v3();
        let breakdown = CostBreakdown::new("deepseek-v3", 1_000_000, 500_000, 100_000, &pricing);
        manager.record_cost(&breakdown).await;

        // Force save by dropping and recreating
        drop(manager);

        // Create a new manager that loads persisted state
        let manager2 = CostManager::new(BudgetConfig::default(), dir.path());
        let status = manager2.status().await;

        // Lifetime spent should carry over from persistence
        assert!(status.lifetime_spent > 0.0);
    }

    #[tokio::test]
    async fn test_check_budget_allowed() {
        let config = BudgetConfig {
            session_limit: Some(100.0), // Generous limit
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let decision = manager.check_budget(0.01).await;
        assert!(decision.allowed);
        assert!(decision.reason.is_none());
    }

    #[tokio::test]
    async fn test_check_budget_blocked() {
        let config = BudgetConfig {
            session_limit: Some(0.001), // Tiny limit
            exceed_action: ExceedAction::Block,
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        // Request more than the limit allows
        let decision = manager.check_budget(1.0).await;
        assert!(!decision.allowed);
        assert!(decision.reason.is_some());
        assert_eq!(decision.action, ExceedAction::Block);
        assert!(decision.reason.unwrap().contains("session budget"));
    }

    #[tokio::test]
    async fn test_reset_hourly_and_daily() {
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let breakdown = CostBreakdown::new("deepseek-v3", 1_000_000, 1_000_000, 0, &pricing);
        manager.record_cost(&breakdown).await;

        let before = manager.status().await;
        assert!(before.hourly_spent > 0.0);
        assert!(before.daily_spent > 0.0);

        manager.reset_hourly().await;
        let after_hourly = manager.status().await;
        assert!((after_hourly.hourly_spent).abs() < 0.0001); // Reset to ~0
        assert!(after_hourly.daily_spent > 0.0); // Daily unaffected

        manager.reset_daily().await;
        let after_daily = manager.status().await;
        assert!((after_daily.daily_spent).abs() < 0.0001); // Reset to ~0
    }

    #[test]
    fn test_cost_breakdown_new() {
        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("test-model", 100, 200, 50, &pricing);
        assert_eq!(bd.model, "test-model");
        assert_eq!(bd.input_tokens, 100);
        assert_eq!(bd.output_tokens, 200);
        assert_eq!(bd.cache_hit_tokens, 50);
        assert!(bd.total > 0.0);
        assert!(bd.timestamp > 0);
    }

    // -- New tests: monthly_report, export_csv, calculate_cost, forecast --

    #[tokio::test]
    async fn test_monthly_report_basic() {
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("deepseek-v3", 1_000_000, 500_000, 0, &pricing);
        manager.record_cost(&bd).await;

        let report = manager.monthly_report(2025, 6);
        assert_eq!(report.year, 2025);
        assert_eq!(report.month, 6);
        assert!(report.total_tokens > 0);
        assert!(report.total_cost > 0.0);
        assert!(report.by_model.contains_key("deepseek-v3"));
        assert!(!report.budget_exceeded);
    }

    #[tokio::test]
    async fn test_export_csv_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("deepseek-v3", 1000, 500, 0, &pricing);
        manager.record_cost(&bd).await;

        let csv_path = dir.path().join("test_costs.csv");
        manager.export_csv(&csv_path).expect("CSV export failed");

        let content = std::fs::read_to_string(&csv_path).expect("Read CSV");
        assert!(content.contains("Model,InputTokens,OutputTokens,Cost,Timestamp"));
        assert!(content.contains("deepseek-v3"));
        assert!(content.contains("1000"));
        assert!(content.contains("500"));
    }

    #[test]
    fn test_calculate_cost_deepseek() {
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        // deepseek-v3: $0.27/1M input, $1.10/1M output
        let cost = manager.calculate_cost("deepseek-v3", 1_000_000, 500_000);
        let expected = 0.27 + 0.55; // 0.27 input + 0.55 output
        assert!((cost - expected).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_forecast_basic() {
        let config = BudgetConfig::default();
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("deepseek-v3", 1_000_000, 500_000, 0, &pricing);
        manager.record_cost(&bd).await;

        let forecast = manager.forecast(10.0);
        assert!(forecast.daily_remaining >= 0.0);
        assert!(forecast.monthly_remaining > 0.0);
        assert!(forecast.estimated_days_remaining.is_finite());
        assert!(forecast.daily_budget_depletion_rate >= 0.0);
        assert!(forecast.monthly_depletion_rate >= 0.0);
    }

    #[test]
    fn test_cost_report_serialization() {
        let mut by_model = HashMap::new();
        by_model.insert("test-model".to_string(), (1000u64, 500u64, 0.42));

        let report = CostReport {
            year: 2025,
            month: 6,
            total_tokens: 1500,
            total_cost: 0.42,
            by_model,
            budget_exceeded: false,
        };

        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains("\"year\":2025"));
        assert!(json.contains("\"month\":6"));
        assert!(json.contains("\"total_tokens\":1500"));
        assert!(json.contains("\"total_cost\":0.42"));

        let deserialized: CostReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.year, 2025);
        assert_eq!(deserialized.total_tokens, 1500);
    }

    #[tokio::test]
    async fn test_budget_exceeded_detection() {
        let config = BudgetConfig {
            monthly_budget: 0.001, // Very small budget
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("deepseek-v3", 1_000_000, 500_000, 0, &pricing);
        manager.record_cost(&bd).await;

        let report = manager.monthly_report(2025, 6);
        assert!(report.budget_exceeded);
    }

    #[tokio::test]
    async fn test_forecast_depletion_rate() {
        let config = BudgetConfig {
            daily_limit: Some(10.0),
            monthly_budget: 200.0,
            ..BudgetConfig::default()
        };
        let manager = CostManager::new(config, tempfile::tempdir().expect("tempdir").path());

        // Record significant cost
        let pricing = ModelPricing::deepseek_v3();
        let bd = CostBreakdown::new("deepseek-v3", 10_000_000, 5_000_000, 0, &pricing);
        manager.record_cost(&bd).await;

        let forecast = manager.forecast(5.0);
        assert!(forecast.daily_budget_depletion_rate > 0.0);
        assert!(forecast.monthly_depletion_rate > 0.0);
    }
}
