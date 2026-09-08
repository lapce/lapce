//! Plan execution submodule — closed-loop plan→execute→compile→fix engine.
//!
//! 增强: Plan 模式除了传统 markdown task list，还产出 OpenSpec 风格的 4 种
//! 可视化 artifact (架构 Mermaid flowchart, API OpenAPI JSON, 时序 Mermaid,
//! UI HTML wireframe)，由 `artifacts` 模块承载。

pub mod artifacts;
pub mod execute_loop;
pub mod plan_mode;

pub use artifacts::{
    ArtifactKind, HtmlPrototype, MermaidFlowchart, MermaidSequence,
    OpenApiSchema, PlanArtifact, PlanArtifactBundle,
    openspec_plan_prompt,
};
pub use plan_mode::{Plan, PlanManager, PlanStatus, plan_mode_prompt, execute_mode_prompt};
pub use execute_loop::{
    ExecuteLoop, ExecuteLoopConfig, ExecuteLoopResult, RoundRecord, RoundStatus,
    format_fix_prompt,
};
