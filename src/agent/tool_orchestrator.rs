//! Tool Chain Orchestration - Complex task tool composition.
//!
//! This module provides:
//! - Tool registry and discovery
//! - Tool chain planning
//! - Sequential and parallel execution
//! - Error handling and recovery

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A tool in the registry.
#[derive(Debug, Clone)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub capabilities: Vec<String>,
    pub timeout_secs: u64,
}

/// A tool execution result.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_id: String,
    pub success: bool,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// A tool chain for complex tasks.
#[derive(Debug, Clone)]
pub struct ToolChain {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<ToolStep>,
    pub parallel: bool,
}

#[derive(Debug, Clone)]
pub struct ToolStep {
    pub step_order: usize,
    pub tool_id: String,
    pub input_mapping: HashMap<String, String>,
    pub condition: Option<String>,
    pub retry_on_failure: bool,
    pub max_retries: usize,
}

/// Tool registry.
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a tool.
    pub async fn register(&self, tool: Tool) {
        self.tools.write().await.insert(tool.id.clone(), tool);
    }

    /// Get a tool by ID.
    pub async fn get(&self, id: &str) -> Option<Tool> {
        self.tools.read().await.get(id).cloned()
    }

    /// Find tools by capability.
    pub async fn find_by_capability(&self, capability: &str) -> Vec<Tool> {
        self.tools.read().await
            .values()
            .filter(|t| t.capabilities.iter().any(|c| c == capability))
            .cloned()
            .collect()
    }

    /// List all tools.
    pub async fn list(&self) -> Vec<Tool> {
        self.tools.read().await.values().cloned().collect()
    }

    /// Register default tools.
    pub async fn register_defaults(&self) {
        let default_tools = vec![
            Tool {
                id: "code_generator".to_string(),
                name: "Code Generator".to_string(),
                description: "Generates code from specifications".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "spec": {"type": "string"},
                        "language": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"}
                    }
                }),
                capabilities: vec!["code_generation".to_string()],
                timeout_secs: 60,
            },
            Tool {
                id: "test_generator".to_string(),
                name: "Test Generator".to_string(),
                description: "Generates test cases".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tests": {"type": "string"}
                    }
                }),
                capabilities: vec!["test_generation".to_string()],
                timeout_secs: 30,
            },
            Tool {
                id: "linter".to_string(),
                name: "Linter".to_string(),
                description: "Checks code for issues".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "issues": {"type": "array"}
                    }
                }),
                capabilities: vec!["code_analysis".to_string(), "linting".to_string()],
                timeout_secs: 20,
            },
            Tool {
                id: "test_runner".to_string(),
                name: "Test Runner".to_string(),
                description: "Runs tests".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tests": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "passed": {"type": "boolean"},
                        "output": {"type": "string"}
                    }
                }),
                capabilities: vec!["test_execution".to_string()],
                timeout_secs: 60,
            },
            Tool {
                id: "code_analyzer".to_string(),
                name: "Code Analyzer".to_string(),
                description: "Analyzes code structure".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "structure": {"type": "object"}
                    }
                }),
                capabilities: vec!["code_analysis".to_string()],
                timeout_secs: 30,
            },
            Tool {
                id: "refactor_planner".to_string(),
                name: "Refactor Planner".to_string(),
                description: "Plans refactoring changes".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"},
                        "goal": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "plan": {"type": "array"}
                    }
                }),
                capabilities: vec!["refactoring".to_string()],
                timeout_secs: 45,
            },
            Tool {
                id: "error_analyzer".to_string(),
                name: "Error Analyzer".to_string(),
                description: "Analyzes errors and suggests fixes".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "error": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "causes": {"type": "array"},
                        "fixes": {"type": "array"}
                    }
                }),
                capabilities: vec!["debugging".to_string(), "error_analysis".to_string()],
                timeout_secs: 30,
            },
            Tool {
                id: "fix_generator".to_string(),
                name: "Fix Generator".to_string(),
                description: "Generates code fixes".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "error": {"type": "string"},
                        "code": {"type": "string"}
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "fixed_code": {"type": "string"}
                    }
                }),
                capabilities: vec!["code_generation".to_string(), "bug_fixing".to_string()],
                timeout_secs: 45,
            },
        ];

        for tool in default_tools {
            self.register(tool).await;
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool chain orchestrator.
pub struct ToolOrchestrator {
    tools: Arc<RwLock<HashMap<String, Tool>>>,
    execution_log: Arc<RwLock<Vec<ExecutionLogEntry>>>,
}

impl Default for ToolOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            execution_log: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create from registry.
    pub fn from_registry(registry: ToolRegistry) -> Self {
        Self {
            tools: registry.tools,
            execution_log: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Execute a tool chain.
    pub async fn execute_chain(&self, chain: &ToolChain, initial_input: serde_json::Value) -> ChainResult {
        let mut context: HashMap<String, serde_json::Value> = HashMap::new();
        context.insert("input".to_string(), initial_input);

        let mut results: Vec<ToolResult> = Vec::new();

        for step in &chain.steps {
            // Check condition if present
            if let Some(condition) = &step.condition {
                if !self.evaluate_condition(condition, &context) {
                    continue;
                }
            }

            // Get tool from registry
            let tools = self.tools.read().await;
            let tool = match tools.get(&step.tool_id).cloned() {
                Some(t) => t,
                None => {
                    results.push(ToolResult {
                        tool_id: step.tool_id.clone(),
                        success: false,
                        output: None,
                        error: Some(format!("Tool {} not found", step.tool_id)),
                        duration_ms: 0,
                    });
                    continue;
                }
            };

            // Build input from mapping
            let input = self.build_input(&step.input_mapping, &context);

            // Execute with retries
            let mut attempt = 0;
            let mut success = false;
            let mut result: Option<ToolResult> = None;

            while attempt <= step.max_retries && !success {
                if attempt > 0 {
                    // Wait before retry
                    tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempt as u64)).await;
                }

                result = Some(self.execute_tool(&tool, input.clone()).await);
                success = result.as_ref().map(|r| r.success).unwrap_or(false);
                attempt += 1;

                if success || !step.retry_on_failure {
                    break;
                }
            }

            if let Some(r) = result {
                // Store output in context for next steps
                if r.success {
                    if let Some(output) = &r.output {
                        context.insert(step.tool_id.clone(), output.clone());
                    }
                }
                results.push(r);
            }
        }

        // Determine overall success
        let all_success = results.iter().all(|r| r.success);
        let final_output = context.get("output").cloned();

        // Log execution
        let log_entry = ExecutionLogEntry {
            chain_id: chain.id.clone(),
            timestamp: current_timestamp(),
            steps_executed: results.len(),
            success: all_success,
        };
        let _log_str = log_entry.format_log_entry();
        self.execution_log.write().await.push(log_entry);

        ChainResult {
            success: all_success,
            results,
            output: final_output,
        }
    }

    /// Execute a single tool (simulated).
    async fn execute_tool(&self, tool: &Tool, input: serde_json::Value) -> ToolResult {
        let start = std::time::Instant::now();

        // Simulate tool execution
        // In real implementation, would actually execute the tool
        let output = serde_json::json!({
            "executed": tool.id,
            "input_received": input
        });

        ToolResult {
            tool_id: tool.id.clone(),
            success: true,
            output: Some(output),
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Evaluate a condition.
    fn evaluate_condition(&self, condition: &str, context: &HashMap<String, serde_json::Value>) -> bool {
        // Simple condition evaluation
        // In real implementation, would use proper expression evaluation
        if condition == "has_errors" {
            return context.contains_key("errors");
        }
        if condition == "has_tests" {
            return context.contains_key("tests");
        }
        true
    }

    /// Build input from mapping.
    fn build_input(&self, mapping: &HashMap<String, String>, context: &HashMap<String, serde_json::Value>) -> serde_json::Value {
        let mut input = serde_json::json!({});

        for (key, source) in mapping {
            if let Some(value) = context.get(source) {
                input[key] = value.clone();
            }
        }

        input
    }

    /// Plan a tool chain for a task.
    pub async fn plan_chain(&self, task: &str) -> Option<ToolChain> {
        let task_lower = task.to_lowercase();

        if task_lower.contains("generate") && task_lower.contains("test") {
            Some(ToolChain {
                id: "gen_test_chain".to_string(),
                name: "Generate and Test".to_string(),
                description: "Generate code and create tests".to_string(),
                steps: vec![
                    ToolStep {
                        step_order: 1,
                        tool_id: "code_analyzer".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 2,
                    },
                    ToolStep {
                        step_order: 2,
                        tool_id: "code_generator".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 2,
                    },
                    ToolStep {
                        step_order: 3,
                        tool_id: "test_generator".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 2,
                    },
                    ToolStep {
                        step_order: 4,
                        tool_id: "linter".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 1,
                    },
                ],
                parallel: false,
            })
        } else if task_lower.contains("refactor") {
            Some(ToolChain {
                id: "refactor_chain".to_string(),
                name: "Refactor".to_string(),
                description: "Plan and execute refactoring".to_string(),
                steps: vec![
                    ToolStep {
                        step_order: 1,
                        tool_id: "code_analyzer".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 2,
                    },
                    ToolStep {
                        step_order: 2,
                        tool_id: "refactor_planner".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: false,
                        max_retries: 1,
                    },
                ],
                parallel: false,
            })
        } else if task_lower.contains("debug") || task_lower.contains("fix") {
            Some(ToolChain {
                id: "debug_fix_chain".to_string(),
                name: "Debug and Fix".to_string(),
                description: "Analyze error and generate fix".to_string(),
                steps: vec![
                    ToolStep {
                        step_order: 1,
                        tool_id: "error_analyzer".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: false,
                        max_retries: 1,
                    },
                    ToolStep {
                        step_order: 2,
                        tool_id: "fix_generator".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 2,
                    },
                    ToolStep {
                        step_order: 3,
                        tool_id: "linter".to_string(),
                        input_mapping: HashMap::new(),
                        condition: None,
                        retry_on_failure: true,
                        max_retries: 1,
                    },
                ],
                parallel: false,
            })
        } else {
            None
        }
    }

    /// Get execution statistics.
    pub async fn stats(&self) -> OrchestratorStats {
        let log = self.execution_log.read().await;
        let total = log.len();
        let successful = log.iter().filter(|e| e.success).count();

        OrchestratorStats {
            total_executions: total,
            successful_executions: successful,
            failed_executions: total - successful,
            success_rate: if total > 0 { successful as f32 / total as f32 } else { 0.0 },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChainResult {
    pub success: bool,
    pub results: Vec<ToolResult>,
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct ExecutionLogEntry {
    chain_id: String,
    timestamp: u64,
    steps_executed: usize,
    success: bool,
}

impl ExecutionLogEntry {
    /// Get the unique chain identifier for this execution.
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    /// Get the Unix timestamp when this execution started.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get the number of steps executed in this chain.
    pub fn steps_executed(&self) -> usize {
        self.steps_executed
    }

    /// Format a log entry string using all fields.
    pub fn format_log_entry(&self) -> String {
        format!(
            "ExecutionLogEntry(chain={}, ts={}, steps={}, success={})",
            self.chain_id(),
            self.timestamp(),
            self.steps_executed(),
            self.success,
        )
    }
}

#[derive(Debug, Clone)]
pub struct OrchestratorStats {
    pub total_executions: usize,
    pub successful_executions: usize,
    pub failed_executions: usize,
    pub success_rate: f32,
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: tool_orchestrator.rs:597")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_registry() {
        let registry = ToolRegistry::new();
        registry.register_defaults().await;

        let tools = registry.list().await;
        assert!(!tools.is_empty());

        let code_gen = registry.get("code_generator").await;
        assert!(code_gen.is_some());
    }

    #[tokio::test]
    async fn test_plan_chain() {
        let registry = ToolRegistry::new();
        registry.register_defaults().await;
        let orchestrator = ToolOrchestrator::from_registry(registry);

        let chain = orchestrator.plan_chain("generate and test code").await;
        assert!(chain.is_some());
        assert_eq!(chain.unwrap().steps.len(), 4);
    }

    #[tokio::test]
    async fn test_execute_chain() {
        let registry = ToolRegistry::new();
        registry.register_defaults().await;
        let orchestrator = ToolOrchestrator::from_registry(registry);

        let chain = ToolChain {
            id: "test_chain".to_string(),
            name: "Test Chain".to_string(),
            description: "A simple test chain".to_string(),
            steps: vec![
                ToolStep {
                    step_order: 1,
                    tool_id: "code_analyzer".to_string(),
                    input_mapping: HashMap::new(),
                    condition: None,
                    retry_on_failure: false,
                    max_retries: 0,
                },
            ],
            parallel: false,
        };

        let result = orchestrator.execute_chain(&chain, serde_json::json!({})).await;
        assert!(result.success);
    }
}
