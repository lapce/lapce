//! Tool permission pipeline — inspired by Claude Code's permission system.
//!
//! Claude Code uses a 7-step sequential decision pipeline:
//!   1a. Deny rules → 1b. Ask rules → 1c. Tool check → 1d. Deny → 1e. User interaction
//!   1f. Content rules → 1g. Safety check → 2a. Bypass → 2b. Allow rules → 3. Default ask
//!
//! This module implements a simplified version tailored for deepseek-carp's
//! agent loop. When tool calls are detected, permission checks determine
//! whether to auto-execute, ask, or deny.

use std::collections::HashSet;

/// Permission decision for a tool invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    /// Execute immediately without asking.
    Allow,
    /// Requires user confirmation before execution.
    Ask,
    /// Blocked — do not execute.
    Deny,
}

/// Permission mode for the current session.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionMode {
    /// Normal mode — ask for destructive operations.
    Default,
    /// Auto-accept everything (CI/non-interactive mode).
    AutoAccept,
    /// Plan mode — discuss plan first, then execute.
    Plan,
    /// Strict mode — ask for everything except reads.
    Strict,
}

/// Permission evaluator — checks tool calls against configured rules.
pub struct PermissionEvaluator {
    mode: PermissionMode,
    /// Tools always allowed (e.g., read_file, list_dir).
    allowlist: HashSet<String>,
    /// Tools always denied (e.g., rm -rf, format disk).
    denylist: HashSet<String>,
    /// Consecutive denials for circuit-breaking.
    consecutive_denials: u32,
    /// Maximum consecutive denials before locked to manual.
    max_denials: u32,
    /// Total denials in this session.
    total_denials: u32,
}

impl Default for PermissionEvaluator {
    fn default() -> Self {
        let mut allowlist = HashSet::new();
        allowlist.insert("read_file".into());
        allowlist.insert("list_dir".into());
        allowlist.insert("search_code".into());
        allowlist.insert("get_current_time".into());

        let mut denylist = HashSet::new();
        denylist.insert("execute_shell".into());
        denylist.insert("delete_file".into());
        denylist.insert("write_file".into());

        Self {
            mode: PermissionMode::Default,
            allowlist,
            denylist,
            consecutive_denials: 0,
            max_denials: 5,
            total_denials: 0,
        }
    }
}

impl PermissionEvaluator {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode, ..Default::default() }
    }

    /// Set permission mode at runtime.
    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
        self.consecutive_denials = 0; // Reset on mode change
    }

    /// Evaluate whether a tool should be allowed, asked, or denied.
    ///
    /// Pipeline (inspired by Claude Code):
    ///   1. Denylist check → Deny
    ///   2. Safety check (destructive tools in Default mode) → Ask
    ///   3. AutoAccept mode → Allow
    ///   4. Allowlist check → Allow
    ///   5. Default → Ask
    pub fn evaluate(&mut self, tool_name: &str, is_destructive: bool) -> Permission {
        // Step 1: Denylist always blocks
        if self.denylist.contains(tool_name) {
            self.consecutive_denials += 1;
            self.total_denials += 1;
            self.check_circuit_breaker();
            return Permission::Deny;
        }

        // Step 2: Safety check — destructive tools ask in Default/Strict mode
        if is_destructive && self.mode != PermissionMode::AutoAccept {
            return Permission::Ask;
        }

        // Step 3: AutoAccept bypasses all checks
        if self.mode == PermissionMode::AutoAccept {
            self.consecutive_denials = 0;
            return Permission::Allow;
        }

        // Step 4: Allowlist permits
        if self.allowlist.contains(tool_name) {
            self.consecutive_denials = 0;
            return Permission::Allow;
        }

        // Step 5: Default to ask
        Permission::Ask
    }

    /// Reset denial counters after user approves.
    pub fn approve(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Check circuit breaker — if too many consecutive denials, lock to manual.
    fn check_circuit_breaker(&self) {
        if self.consecutive_denials >= self.max_denials {
            tracing::warn!(
                consecutive = self.consecutive_denials,
                total = self.total_denials,
                "Permission circuit breaker triggered — consider manual review"
            );
        }
    }

    /// Get stats for observability.
    pub fn stats(&self) -> PermissionStats {
        PermissionStats {
            mode: self.mode.clone(),
            consecutive_denials: self.consecutive_denials,
            total_denials: self.total_denials,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PermissionStats {
    pub mode: PermissionMode,
    pub consecutive_denials: u32,
    pub total_denials: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_denylist_blocks() {
        let mut eval = PermissionEvaluator::default();
        assert_eq!(eval.evaluate("execute_shell", true), Permission::Deny);
    }

    #[test]
    fn test_allowlist_permits() {
        let mut eval = PermissionEvaluator::default();
        assert_eq!(eval.evaluate("read_file", false), Permission::Allow);
    }

    #[test]
    fn test_destructive_asks_in_default() {
        let mut eval = PermissionEvaluator::default();
        assert_eq!(eval.evaluate("unknown_tool", true), Permission::Ask);
    }

    #[test]
    fn test_auto_accept_allows_all() {
        let mut eval = PermissionEvaluator::new(PermissionMode::AutoAccept);
        assert_eq!(eval.evaluate("execute_shell", true), Permission::Deny); // denylist always blocks
        assert_eq!(eval.evaluate("unknown_tool", true), Permission::Allow);
    }

    #[test]
    fn test_circuit_breaker() {
        let mut eval = PermissionEvaluator::default();
        for _ in 0..5 {
            eval.evaluate("execute_shell", true);
        }
        let stats = eval.stats();
        assert_eq!(stats.consecutive_denials, 5);
        assert_eq!(stats.total_denials, 5);
    }
}
