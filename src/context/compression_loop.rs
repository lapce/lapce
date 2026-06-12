//! BudgetTracker + loop integration for the Agent's iterative execution.
//!
//! This file wires `BudgetTracker` (from compression.rs) into an actual
//! decision point that a loop engine can consult every turn.  When the
//! loop crosses 90% of its token budget *and* the last two turn deltas
//! are both below 500 tokens, the tracker returns `Err(DiminishingReturns)`
//! so the caller can break the loop instead of burning more turns.

use crate::context::compression::{BudgetDecision, BudgetTracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStopReason {
    TurnLimit,
    BudgetExhausted,
    DiminishingReturns,
    ExplicitStop,
    Success,
}

impl std::fmt::Display for LoopStopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LoopStopReason::TurnLimit => "turn_limit",
            LoopStopReason::BudgetExhausted => "budget_exhausted",
            LoopStopReason::DiminishingReturns => "diminishing_returns",
            LoopStopReason::ExplicitStop => "explicit_stop",
            LoopStopReason::Success => "success",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone)]
pub struct LoopBudgetConfig {
    pub max_turns: u32,
    pub token_budget: usize,
    pub diminishing_delta: usize,
    pub diminishing_min_continuation: u32,
}

impl Default for LoopBudgetConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            token_budget: 32_000,
            diminishing_delta: 500,
            diminishing_min_continuation: 1,
        }
    }
}

impl LoopBudgetConfig {
    pub fn with_budget(token_budget: usize) -> Self {
        Self { token_budget, ..Self::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoopBudgetState {
    pub turn_count: u32,
    pub tracker: BudgetTracker,
    pub decision: Option<BudgetDecision>,
    pub stop_reason: Option<LoopStopReason>,
}

impl LoopBudgetState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn advance(&mut self, turn_tokens_used: usize, cfg: &LoopBudgetConfig) -> Option<LoopStopReason> {
        self.tracker.continuation_count += 1;
        self.tracker.last_last_global_turn_tokens = self.tracker.last_global_turn_tokens;
        self.tracker.last_global_turn_tokens = turn_tokens_used;
        self.turn_count += 1;

        if self.turn_count >= cfg.max_turns {
            self.stop_reason = Some(LoopStopReason::TurnLimit);
            return self.stop_reason;
        }

        match self.tracker.decide(cfg.token_budget) {
            Ok(d) => {
                if d.pct >= 0.99 {
                    self.stop_reason = Some(LoopStopReason::BudgetExhausted);
                } else {
                    self.decision = Some(d);
                }
            }
            Err(_) => {
                self.stop_reason = Some(LoopStopReason::DiminishingReturns);
            }
        }
        self.stop_reason
    }

    pub fn mark_success(&mut self) {
        self.stop_reason = Some(LoopStopReason::Success);
    }

    pub fn should_continue(&self) -> bool {
        self.stop_reason.is_none()
    }

    pub fn summary(&self) -> String {
        let reason = self.stop_reason.map(|r| r.to_string()).unwrap_or_else(|| "running".into());
        let decision = self.decision.as_ref().map(|d| format!("used={}/{} ({:.0}%)", d.used, d.budget, d.pct * 100.0)).unwrap_or_else(|| "no-decision".into());
        format!("turn={} stop={} {}", self.turn_count, reason, decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advance_within_budget_continues() {
        let cfg = LoopBudgetConfig { token_budget: 32_000, ..Default::default() };
        let mut s = LoopBudgetState::new();
        assert!(s.advance(3_000, &cfg).is_none());
        assert!(s.should_continue());
    }

    #[test]
    fn test_advance_hits_turn_limit() {
        let cfg = LoopBudgetConfig { max_turns: 3, token_budget: 32_000, ..Default::default() };
        let mut s = LoopBudgetState::new();
        s.advance(2_000, &cfg);
        s.advance(2_000, &cfg);
        let reason = s.advance(2_000, &cfg);
        assert_eq!(reason, Some(LoopStopReason::TurnLimit));
        assert!(!s.should_continue());
    }

    #[test]
    fn test_diminishing_returns_kicks_in() {
        let cfg = LoopBudgetConfig {
            token_budget: 32_000,
            diminishing_delta: 500,
            diminishing_min_continuation: 1,
            max_turns: 20,
        };
        let mut s = LoopBudgetState::new();
        s.advance(20_000, &cfg);
        s.advance(100, &cfg);
        let reason = s.advance(100, &cfg);
        assert_eq!(reason, Some(LoopStopReason::DiminishingReturns));
    }

    #[test]
    fn test_mark_success_overrides() {
        let mut s = LoopBudgetState::new();
        s.mark_success();
        assert_eq!(s.stop_reason, Some(LoopStopReason::Success));
    }
}
