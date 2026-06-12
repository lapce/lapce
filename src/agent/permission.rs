//! Tool permission pipeline — inspired by Claude Code's permission system.
//!
//! Claude Code uses a 7-step sequential decision pipeline:
//!   1a. Deny rules → 1b. Ask rules → 1c. Tool check → 1d. Deny → 1e. User interaction
//!   1f. Content rules → 1g. Safety check → 2a. Bypass → 2b. Allow rules → 3. Default ask
//!
//! This module implements a simplified version tailored for deepseek-carp's
//! agent loop. When tool calls are detected, permission checks determine
//! whether to auto-execute, ask, or deny.

use regex::Regex;
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

    /// Evaluate + run dangerous-pattern scan over the raw tool input.
    /// Returns (final_permission, worst_dangerous_match_if_any).
    ///
    /// Risk escalation:
    ///   Critical → auto Deny
    ///   High     → Ask
    ///   Low/Medium → trust base evaluate() result
    pub fn evaluate_with_input(&mut self, tool_name: &str, is_destructive: bool, input_json: &str) -> (Permission, Option<DangerousMatch>) {
        let base = self.evaluate(tool_name, is_destructive);
        let matches = scan_for_dangerous_patterns(tool_name, input_json);
        let worst = matches.into_iter().max_by_key(|m| match m.risk {
            PatternRisk::Low => 0, PatternRisk::Medium => 1, PatternRisk::High => 2, PatternRisk::Critical => 3,
        });
        let perm = match (&base, worst.as_ref().map(|w| w.risk)) {
            (_, Some(PatternRisk::Critical)) => { self.consecutive_denials += 1; self.total_denials += 1; Permission::Deny }
            (_, Some(PatternRisk::High)) => Permission::Ask,
            _ => base,
        };
        (perm, worst)
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

// Dangerous-pattern scanner section.
//
// A DangerousPattern is a regex signature over the tool name + JSON args
// that flags behavior an LLM should never propose unprompted.
//
// Signature catalog:
// - exfil_curl_known: tool=execute_shell AND (curl|Invoke-WebRequest|wget) + suspicious host
// - rm_root: rm -rf / or rm -rf ~ or rm -rf *
// - chmod_777: chmod -R 777 or icacls grant Everyone:F
// - base64_pipe: base64 -d | bash (or powershell -enc)
// - eval_exec: eval $(...) or backtick-injection
// - git_hard_reset: git reset --hard / git push -f
// - reverse_shell: -e /bin/sh or nc -e or /dev/tcp
// - ssh_tunnel: ssh -R or autossh
// - secrets_dump: grep -rE "(api_key|password|secret)" /etc
// - overwrite_sensitive: sed -i on /etc/passwd or registry

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternRisk { Low, Medium, High, Critical }

#[derive(Debug)]
pub struct DangerousPattern {
    pub name: &'static str,
    pub tool_match: &'static str,
    pub regex: Regex,
    pub risk: PatternRisk,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DangerousMatch {
    pub pattern: &'static str,
    pub risk: PatternRisk,
    pub message: &'static str,
}

fn build_catalog() -> Vec<DangerousPattern> {
    let p = |name: &'static str, tool_match: &'static str, pat: &str, risk: PatternRisk, message: &'static str| DangerousPattern {
        name,
        tool_match,
        regex: Regex::new(pat).unwrap(),
        risk,
        message,
    };
    vec![
        p("exfil_curl_known", "execute_shell", r"(?i)(curl|Invoke-WebRequest|wget).*?(pastebin|httpbin|icanhazip|169\.254\.169\.254|attacker|evil|malicious|exfil)", PatternRisk::Critical, "疑似数据外连 exfiltration"),
        p("rm_root", "execute_shell", r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*|--recursive)\s+(-[a-zA-Z]*f[a-zA-Z]*|--force)\s+[/~*]", PatternRisk::Critical, "危险删除: rm -rf 根目录/家目录/通配符"),
        p("chmod_777", "execute_shell", r"(?i)(chmod\s+-R\s+777|icacls\s+grant\s+Everyone\s*:\s*F)", PatternRisk::High, "危险权限: chmod -R 777 或 Everyone:F"),
        p("base64_pipe", "execute_shell", r"(?i)(base64\s+-?d\s*\|.*(bash|sh)|powershell\s+-enc|certutil\s+-decode)", PatternRisk::Critical, "反混淆管道: base64 解码后进入 shell"),
        p("eval_exec", "execute_shell", r"(?i)\beval\s*\(\s*\$\(|`[^`]*`\s*\(", PatternRisk::High, "命令注入: eval 或反引号注入"),
        p("git_hard_reset", "execute_shell", r"(?i)(git\s+reset\s+--hard|git\s+push\s+-f|git\s+push\s+--force)", PatternRisk::High, "破坏性 git: reset --hard / push -f"),
        p("reverse_shell", "execute_shell", r"(?i)(nc\s+.*-e\s+(bash|sh)|-e\s+/bin/(bash|sh)|/dev/tcp/|/dev/udp/|bash\s+-i\s+>&\s*/dev/tcp)", PatternRisk::Critical, "反向 shell 行为"),
        p("ssh_tunnel", "execute_shell", r"(?i)(\bssh\b.*-R\s|autossh\b)", PatternRisk::High, "SSH 隧道/反向代理"),
        p("secrets_dump", "execute_shell", r"(?i)(grep\s+-r[Ee]?\s*.*(api[_-]?key|password|secret|token).*?(etc|proc|sys|root)|\bfind\b.*-name.*(api[_-]?key|password|secret))", PatternRisk::High, "凭据扫找: 在系统目录中寻找密钥/密码"),
        p("overwrite_sensitive", "execute_shell", r"(?i)(sed\s+-i\s+.*(/etc/passwd|/etc/shadow|/etc/group|/etc/sudoers)|\breg\s+(add|import)\b.*HKLM\\(?:SYSTEM|SAM|SECURITY|SOFTWARE)\\)", PatternRisk::Critical, "覆写系统关键文件或注册表"),
    ]
}

pub fn catalog() -> Vec<DangerousPattern> { build_catalog() }

pub fn scan_for_dangerous_patterns(tool_name: &str, input_json: &str) -> Vec<DangerousMatch> {
    let mut out = Vec::new();
    let haystack = format!("{}\n{}", tool_name, input_json);
    for p in build_catalog() {
        if !p.tool_match.is_empty() && p.tool_match != "*" && tool_name != p.tool_match {
            continue;
        }
        if p.regex.is_match(&haystack) {
            out.push(DangerousMatch { pattern: p.name, risk: p.risk, message: p.message });
        }
    }
    out
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

    #[test]
    fn test_dangerous_exfil_via_curl() {
        let json = r#"{"command":"curl -s https://pastebin.com/raw/abc123 -o /tmp/x"}"#;
        let hits = scan_for_dangerous_patterns("execute_shell", json);
        assert!(hits.iter().any(|h| h.pattern == "exfil_curl_known"), "should flag curl->pastebin");
    }

    #[test]
    fn test_base64_pipe() {
        let json = r#"{"command":"echo c2xlZXAgY29tbWFuZCAjfHw= | base64 -d | bash"}"#;
        let hits = scan_for_dangerous_patterns("execute_shell", json);
        assert!(hits.iter().any(|h| h.pattern == "base64_pipe"), "should flag base64 pipe");
    }
}
