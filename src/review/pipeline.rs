//! AuditPipeline — helper struct + timestamp for audit-coordinated review.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPipelineResult {
    pub session_id: String,
    pub review_started: bool,
    pub apply_count: u32,
    pub verify_passed: bool,
    pub audit_compare_ok: bool,
}

impl AuditPipelineResult {
    pub fn summary_line(&self) -> String {
        format!("[{}] session={} apply={} verify={} compare={}",
            if self.audit_compare_ok { "OK" } else { "--" },
            self.session_id, self.apply_count, self.verify_passed, self.audit_compare_ok)
    }
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_pipeline_result_fields() { let r = AuditPipelineResult { session_id: "p-1".into(), review_started: true, apply_count: 3, verify_passed: true, audit_compare_ok: true }; assert!(r.verify_passed); }
    #[test] fn test_now_unix_ms_nonzero() { let t = now_unix_ms(); assert!(t > 1_700_000_000_000); }
}
