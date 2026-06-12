//! Localization — Chinese/English UI string translation.
//!
//! Pattern inspired by CodeWhale's localization.rs (7 locales, 300+ MessageId).
//! Simplified for dscarp-lapce: zh-CN and en, covering Slash command descriptions,
//! status messages, error messages, and UI labels.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::localization::{tr, Locale, MsgId};
//!
//! let text = tr(Locale::ZhCN, MsgId::PlanExecuting);
//! ```

use std::sync::OnceLock;

// ── Locale ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    /// English (default)
    En,
    /// Simplified Chinese
    ZhCN,
}

impl Locale {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "zh" | "zh-cn" | "zh_cn" | "chinese" | "中文" => Locale::ZhCN,
            _ => Locale::En,
        }
    }
}

static LOCALE: OnceLock<Locale> = OnceLock::new();

/// Set the global locale.
pub fn set_locale(locale: Locale) {
    let _ = LOCALE.set(locale);
}

/// Get the current global locale.
pub fn locale() -> Locale {
    *LOCALE.get_or_init(|| Locale::En)
}

// ── Message IDs ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MsgId {
    // Slash command descriptions
    CmdClear,
    CmdStats,
    CmdPlan,
    CmdSwarm,
    CmdSwarmRun,
    CmdSwarmStatus,
    CmdExecute,
    CmdListPlans,
    CmdCompile,
    CmdMetrics,
    CmdConstitution,
    CmdPermission,
    CmdSnapshot,
    CmdRestore,
    CmdCheckpoint,
    CmdSeam,
    CmdApply,
    CmdMcp,
    CmdBrowser,

    // Status messages
    StatusStreaming,
    StatusThinking,
    StatusCompiling,
    StatusCompilePass,
    StatusCompileFail,
    StatusAutoFix,
    StatusSnapshotSaved,
    StatusSnapshotRestored,
    StatusCheckpointSaved,
    StatusSeamTokens,
    StatusSessionCleared,
    StatusSending,

    // Placeholder
    PlaceholderInput,

    // Error messages
    ErrUnknownCmd,
    ErrCompileFailed,
    ErrNetworkFail,
    ErrPermissionDenied,

    // Button labels
    BtnAccept,
    BtnReject,
    BtnSend,
}

// ── Translation tables ───────────────────────────────────────────

fn translate_en(id: MsgId) -> &'static str {
    match id {
        MsgId::CmdClear => "Clear session",
        MsgId::CmdStats => "Show session stats",
        MsgId::CmdPlan => "Create execution plan",
        MsgId::CmdSwarm => "Decompose into sub-tasks",
        MsgId::CmdSwarmRun => "Execute swarm in parallel",
        MsgId::CmdSwarmStatus => "Show swarm agent status",
        MsgId::CmdExecute => "Execute a plan by slug",
        MsgId::CmdListPlans => "List saved plans",
        MsgId::CmdCompile => "Run cargo check with auto-fix",
        MsgId::CmdMetrics => "Show AI metrics report",
        MsgId::CmdConstitution => "Show AI Constitution",
        MsgId::CmdPermission => "Set permission mode",
        MsgId::CmdSnapshot => "Create git snapshot",
        MsgId::CmdRestore => "Restore git snapshot",
        MsgId::CmdCheckpoint => "Save file checkpoint",
        MsgId::CmdSeam => "Show seam context status",
        MsgId::CmdApply => "Apply pending code edits",
        MsgId::CmdMcp => "Show MCP status",
        MsgId::CmdBrowser => "Fetch URL content",

        MsgId::StatusStreaming => "Streaming...",
        MsgId::StatusThinking => "Thinking...",
        MsgId::StatusCompiling => "Running cargo check with auto-fix...",
        MsgId::StatusCompilePass => "Compilation passed",
        MsgId::StatusCompileFail => "errors, warnings after auto-fix",
        MsgId::StatusAutoFix => "cargo check (auto-fix)",
        MsgId::StatusSnapshotSaved => "Snapshot saved",
        MsgId::StatusSnapshotRestored => "Restored to turn",
        MsgId::StatusCheckpointSaved => "Checkpoint saved",
        MsgId::StatusSeamTokens => "Seam context: ~{} tokens in layered window",
        MsgId::StatusSessionCleared => "Session cleared.",
        MsgId::StatusSending => "...",

        MsgId::PlaceholderInput => "Ask or /plan /execute /swarm /compile /browser ...",

        MsgId::ErrUnknownCmd => "Unknown command",
        MsgId::ErrCompileFailed => "Compilation failed",
        MsgId::ErrNetworkFail => "Network error",
        MsgId::ErrPermissionDenied => "Permission denied",

        MsgId::BtnAccept => "Accept",
        MsgId::BtnReject => "Reject",
        MsgId::BtnSend => "Send",
    }
}

fn translate_zh(id: MsgId) -> &'static str {
    match id {
        MsgId::CmdClear => "清除会话",
        MsgId::CmdStats => "显示会话统计",
        MsgId::CmdPlan => "创建执行计划",
        MsgId::CmdSwarm => "分解为子任务",
        MsgId::CmdSwarmRun => "并行执行 Swarm",
        MsgId::CmdSwarmStatus => "Swarm 代理状态",
        MsgId::CmdExecute => "按计划执行",
        MsgId::CmdListPlans => "列出已保存计划",
        MsgId::CmdCompile => "编译检查 + 自动修复",
        MsgId::CmdMetrics => "AI 指标报告",
        MsgId::CmdConstitution => "查看 AI 准则",
        MsgId::CmdPermission => "设置权限模式",
        MsgId::CmdSnapshot => "创建 Git 快照",
        MsgId::CmdRestore => "恢复 Git 快照",
        MsgId::CmdCheckpoint => "保存文件检查点",
        MsgId::CmdSeam => "分层上下文状态",
        MsgId::CmdApply => "应用待处理编辑",
        MsgId::CmdMcp => "MCP 连接状态",
        MsgId::CmdBrowser => "获取网页内容",

        MsgId::StatusStreaming => "流式输出中...",
        MsgId::StatusThinking => "思考中...",
        MsgId::StatusCompiling => "正在编译检查 + 自动修复...",
        MsgId::StatusCompilePass => "编译通过",
        MsgId::StatusCompileFail => "个错误，自动修复后仍有",
        MsgId::StatusAutoFix => "编译检查（自动修复）",
        MsgId::StatusSnapshotSaved => "快照已保存",
        MsgId::StatusSnapshotRestored => "已恢复到回合",
        MsgId::StatusCheckpointSaved => "检查点已保存",
        MsgId::StatusSeamTokens => "分层上下文：约 {} token",
        MsgId::StatusSessionCleared => "会话已清除。",
        MsgId::StatusSending => "发送中...",

        MsgId::PlaceholderInput => "输入或 /plan /execute /swarm /compile /browser ...",

        MsgId::ErrUnknownCmd => "未知命令",
        MsgId::ErrCompileFailed => "编译失败",
        MsgId::ErrNetworkFail => "网络错误",
        MsgId::ErrPermissionDenied => "权限不足",

        MsgId::BtnAccept => "接受",
        MsgId::BtnReject => "拒绝",
        MsgId::BtnSend => "发送",
    }
}

// ── Public API ───────────────────────────────────────────────────

/// Translate a message ID to the given locale.
pub fn tr(locale: Locale, id: MsgId) -> &'static str {
    match locale {
        Locale::En => translate_en(id),
        Locale::ZhCN => translate_zh(id),
    }
}

/// Translate using the current global locale.
pub fn t(id: MsgId) -> &'static str {
    tr(locale(), id)
}

/// Format a translatable string with one argument.
pub fn tr1(locale: Locale, id: MsgId, arg: impl std::fmt::Display) -> String {
    tr(locale, id).replace("{}", &arg.to_string())
}

/// Format with the current locale.
pub fn t1(id: MsgId, arg: impl std::fmt::Display) -> String {
    tr1(locale(), id, arg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zh_basics() {
        assert_eq!(tr(Locale::ZhCN, MsgId::BtnAccept), "接受");
        assert_eq!(tr(Locale::ZhCN, MsgId::BtnReject), "拒绝");
    }

    #[test]
    fn test_en_basics() {
        assert_eq!(tr(Locale::En, MsgId::BtnAccept), "Accept");
        assert_eq!(tr(Locale::En, MsgId::BtnReject), "Reject");
    }

    #[test]
    fn test_format() {
        let s = tr1(Locale::En, MsgId::StatusSeamTokens, 42);
        assert_eq!(s, "Seam context: ~42 tokens in layered window");
    }
}
