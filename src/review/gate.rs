//! ReviewGate — simple pass/fail gating for a review session.
//! Rules: critical > 0 → block, high > 2 → block, else → pass.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewGateReport {
    pub total_findings: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub pass: bool,
    pub blocked_reason: Option<String>,
}

impl ReviewGateReport {
    pub fn assess(critical: usize, high: usize, medium: usize, low: usize) -> Self {
        let pass = critical == 0 && high <= 2;
        let blocked_reason = if critical > 0 { Some(format!("{critical} critical findings must be fixed")) }
            else if high > 2 { Some(format!("{high} high findings exceeds limit (2)")) }
            else { None };
        Self { total_findings: critical + high + medium + low, critical, high, medium, low, pass, blocked_reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_gate_passes_when_clean() { let r = ReviewGateReport::assess(0, 0, 2, 3); assert!(r.pass); assert!(r.blocked_reason.is_none()); }
    #[test] fn test_gate_blocks_on_critical() { let r = ReviewGateReport::assess(1, 0, 0, 0); assert!(!r.pass); }
    #[test] fn test_gate_blocks_on_too_many_high() { let r = ReviewGateReport::assess(0, 3, 0, 0); assert!(!r.pass); }
}
