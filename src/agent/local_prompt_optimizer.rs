//! Local Model Prompt Optimizer — Enhanced prompts for local models (Qwen/DeepSeek-R1).
//!
//! This module provides specialized prompt templates optimized for local models,
//! with:
//! - Claude Code inspired thinking patterns
//! - Code-specific instruction enhancements
//! - Chain-of-thought prompting for complex tasks
//! - Structured output guidelines

use crate::providers::TaskCategory;

/// Configuration for prompt optimization.
#[derive(Debug, Clone)]
pub struct PromptOptimizerConfig {
    /// Enable chain-of-thought prompting for complex tasks.
    pub enable_chain_of_thought: bool,
    /// Enable structured output guidelines.
    pub enable_structured_output: bool,
    /// Maximum reasoning tokens to allocate.
    pub max_reasoning_tokens: u32,
    /// Optimize for code generation.
    pub optimize_for_code: bool,
}

impl Default for PromptOptimizerConfig {
    fn default() -> Self {
        Self {
            enable_chain_of_thought: true,
            enable_structured_output: true,
            max_reasoning_tokens: 2048,
            optimize_for_code: true,
        }
    }
}

/// The main prompt optimizer for local models.
pub struct LocalPromptOptimizer {
    config: PromptOptimizerConfig,
}

impl LocalPromptOptimizer {
    /// Create a new optimizer with default configuration.
    pub fn new() -> Self {
        Self::with_config(PromptOptimizerConfig::default())
    }

    /// Create a new optimizer with custom configuration.
    pub fn with_config(config: PromptOptimizerConfig) -> Self {
        Self { config }
    }

    /// Optimize a system prompt for local model performance.
    pub fn optimize_system_prompt(&self, base_prompt: &str, category: &TaskCategory) -> String {
        let mut optimized = String::new();

        // Base prompt
        optimized.push_str(base_prompt);
        optimized.push_str("\n\n");

        // Add specialized instructions based on task category
        match category {
            TaskCategory::CodeGeneration => {
                optimized.push_str(&self.code_generation_instructions());
            }
            TaskCategory::CodeReview => {
                optimized.push_str(&self.code_review_instructions());
            }
            TaskCategory::ComplexReasoning => {
                optimized.push_str(&self.complex_reasoning_instructions());
            }
            _ => {
                optimized.push_str(&self.general_instructions());
            }
        }

        // Add chain-of-thought if enabled
        if self.config.enable_chain_of_thought {
            optimized.push_str("\n\n");
            optimized.push_str(&self.chain_of_thought_guidelines());
        }

        // Add structured output guidelines if enabled
        if self.config.enable_structured_output {
            optimized.push_str("\n\n");
            optimized.push_str(&self.structured_output_guidelines());
        }

        optimized
    }

    /// Enhance a user prompt with specific guidance for local models.
    pub fn enhance_user_prompt(&self, user_prompt: &str, category: &TaskCategory) -> String {
        let mut enhanced = user_prompt.to_string();

        // Add task-specific context hints
        match category {
            TaskCategory::CodeGeneration => {
                if !user_prompt.to_lowercase().contains("test") && 
                   !user_prompt.to_lowercase().contains("测试") {
                    enhanced.push_str("\n\n---\n");
                    enhanced.push_str("Please include comments explaining your code.");
                }
            }
            TaskCategory::ComplexReasoning => {
                enhanced.push_str("\n\n---\n");
                enhanced.push_str("Please explain your reasoning step by step.");
            }
            _ => {}
        }

        enhanced
    }

    // --- Specialized instruction templates ---

    fn code_generation_instructions(&self) -> String {
        r#"### Code Generation Guidelines

1. **Think before coding**: Briefly plan your approach before writing code.
2. **Code quality**:
   - Write clean, idiomatic code
   - Include appropriate error handling
   - Add comments for non-obvious logic
   - Follow language-specific conventions
3. **Output format**:
   - Use fenced code blocks with language specification
   - Explain key decisions after the code
   - If multiple approaches exist, discuss tradeoffs
4. **Precision**:
   - Prefer search-and-replace edits for existing files
   - Show exact line numbers when referencing code
   - Provide complete, runnable examples when possible"#.to_string()
    }

    fn code_review_instructions(&self) -> String {
        r#"### Code Review Guidelines

1. **Structure your review**:
   - Start with a high-level summary
   - Break down into specific sections (bugs, style, performance, etc.)
   - Provide concrete suggestions, not just criticisms
2. **Checklist**:
   - Logic correctness and edge cases
   - Code style and readability
   - Performance considerations
   - Security implications
   - Documentation and comments
3. **Constructive feedback**:
   - Explain why something should change
   - Provide improved code examples when possible
   - Prioritize issues by severity"#.to_string()
    }

    fn complex_reasoning_instructions(&self) -> String {
        r#"### Complex Reasoning Guidelines

1. **Break it down**:
   - Decompose complex problems into smaller parts
   - Address each part systematically
2. **Show your work**:
   - Explain your reasoning process clearly
   - State assumptions explicitly
   - Show intermediate steps
3. **Verify conclusions**:
   - Check for logical consistency
   - Consider alternative approaches
   - Validate with examples when possible"#.to_string()
    }

    fn general_instructions(&self) -> String {
        r#"### General Response Guidelines

1. **Be clear and concise**: Get to the point while being thorough.
2. **Structure your response**:
   - Use headings and bullet points for readability
   - Organize information logically
3. **Ask for clarification**: If the request is ambiguous, seek more information before proceeding."#.to_string()
    }

    fn chain_of_thought_guidelines(&self) -> String {
        format!(
            r#"### Thinking Process (Chain-of-Thought)

Before giving your final answer, take a moment to think through the problem:

1. **Understand**: What is being asked? What are the constraints?
2. **Plan**: What approach will you take? What steps are needed?
3. **Execute**: Work through the problem step by step.
4. **Verify**: Does your solution make sense? Can you improve it?

Format your thinking clearly, then provide your final answer.
You may use up to {} tokens for reasoning."#,
            self.config.max_reasoning_tokens
        )
    }

    fn structured_output_guidelines(&self) -> String {
        r#"### Structured Output Format

When responding, use these formatting conventions:

- **Headings**: Use `### Heading` for section titles
- **Lists**: Use `- ` for bullet points
- **Code**: Use ```language ... ``` for code blocks
- **Important**: Use **bold** for key points
- **Examples**: Provide concrete examples when helpful

This makes your response easier to read and understand."#.to_string()
    }
}

impl Default for LocalPromptOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a specialized local model system prompt.
pub fn local_model_system_prompt(category: &TaskCategory) -> String {
    let base = r#"You are DeepSeek Carp, a specialized AI coding assistant optimized for local execution.
You are helpful, precise, and focused on practical solutions.

Core Principles:
- Prioritize correctness and clarity
- Provide actionable, practical advice
- Be honest about limitations
- Keep responses concise but thorough"#;

    let optimizer = LocalPromptOptimizer::new();
    optimizer.optimize_system_prompt(base, category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimize_system_prompt() {
        let optimizer = LocalPromptOptimizer::new();
        let result = optimizer.optimize_system_prompt("Test prompt", &TaskCategory::CodeGeneration);
        assert!(result.contains("Code Generation Guidelines"));
        assert!(result.len() > "Test prompt".len());
    }

    #[test]
    fn test_enhance_user_prompt() {
        let optimizer = LocalPromptOptimizer::new();
        let result = optimizer.enhance_user_prompt("Write a function", &TaskCategory::CodeGeneration);
        assert!(result.contains("Write a function"));
        assert!(result.contains("comments"));
    }

    #[test]
    fn test_local_model_system_prompt() {
        let prompt = local_model_system_prompt(&TaskCategory::General);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("DeepSeek Carp"));
    }
}
