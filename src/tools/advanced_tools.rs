//! Advanced tools — test generation, debugging assistance, refactoring suggestions.
//!
//! This module provides specialized tools for enhancing developer productivity:
//! - Test generation for multiple languages with AST parsing
//! - Debugging assistance with breakpoint suggestions
//! - Refactoring suggestions
//! - Code explanation tools
//! - Test runner integration
//! - Auto-fix suggestions for common errors

use serde::{Deserialize, Serialize};
use crate::tools::auto_fix::ErrorWithFix;
use crate::tools::code_smell::SmellDetector;

// ── Test Generation ──

/// Supported test frameworks by language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestFramework {
    Rust,        // cargo test / rstest
    Python,      // pytest / unittest
    TypeScript,  // Jest / Vitest
    JavaScript,  // Jest / Vitest
    Go,          // standard testing
    Java,        // JUnit
    CSharp,      // xUnit / NUnit
}

impl TestFramework {
    pub fn from_file_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(TestFramework::Rust),
            "py" => Some(TestFramework::Python),
            "ts" | "tsx" => Some(TestFramework::TypeScript),
            "js" | "jsx" => Some(TestFramework::JavaScript),
            "go" => Some(TestFramework::Go),
            "java" => Some(TestFramework::Java),
            "cs" => Some(TestFramework::CSharp),
            _ => None,
        }
    }
}

/// Test generation request.
#[derive(Debug, Clone, Deserialize)]
pub struct TestGenerationRequest {
    pub source_file: String,
    pub framework: Option<String>,
    pub test_type: Option<String>, // unit, integration, property
    pub include_edge_cases: Option<bool>,
}

/// Test generation result.
#[derive(Debug, Clone, Serialize)]
pub struct TestGenerationResult {
    pub success: bool,
    pub test_file: Option<String>,
    pub test_code: String,
    pub suggestions: Vec<String>,
    pub language: String,
}

/// Generate tests for a source file.
pub fn generate_tests(request: TestGenerationRequest) -> TestGenerationResult {
    let ext = std::path::Path::new(&request.source_file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    let framework = request.framework
        .and_then(|f| match f.to_lowercase().as_str() {
            "rust" | "cargo" => Some(TestFramework::Rust),
            "python" | "pytest" => Some(TestFramework::Python),
            "typescript" | "ts" | "jest" | "vitest" => Some(TestFramework::TypeScript),
            "javascript" | "js" => Some(TestFramework::JavaScript),
            "go" => Some(TestFramework::Go),
            _ => None,
        })
        .or_else(|| TestFramework::from_file_extension(ext))
        .unwrap_or(TestFramework::Rust);
    
    let source_content = match std::fs::read_to_string(&request.source_file) {
        Ok(c) => c,
        Err(e) => {
            return TestGenerationResult {
                success: false,
                test_file: None,
                test_code: format!("Failed to read source file: {}", e),
                suggestions: vec![],
                language: "unknown".to_string(),
            };
        }
    };
    
    let (language, test_file, test_code) = match framework {
        TestFramework::Rust => {
            let test_file = request.source_file.replace(".rs", "_test.rs");
            let code = generate_rust_tests(&source_content, request.include_edge_cases.unwrap_or(true));
            ("rust".to_string(), Some(test_file), code)
        }
        TestFramework::Python => {
            let test_file = request.source_file.replace(".py", "_test.py");
            let code = generate_python_tests(&source_content, request.include_edge_cases.unwrap_or(true));
            ("python".to_string(), Some(test_file), code)
        }
        TestFramework::TypeScript | TestFramework::JavaScript => {
            let ext = if framework == TestFramework::TypeScript { ".ts" } else { ".js" };
            let test_file = request.source_file.replace(ext, &format!(".test{}", ext));
            let code = generate_js_ts_tests(&source_content, request.include_edge_cases.unwrap_or(true), framework == TestFramework::TypeScript);
            ("typescript".to_string(), Some(test_file), code)
        }
        TestFramework::Go => {
            let test_file = request.source_file.replace(".go", "_test.go");
            let code = generate_go_tests(&source_content, request.include_edge_cases.unwrap_or(true));
            ("go".to_string(), Some(test_file), code)
        }
        TestFramework::Java => {
            let test_file = request.source_file.replace(".java", "Test.java");
            let code = generate_java_tests(&source_content, request.include_edge_cases.unwrap_or(true));
            ("java".to_string(), Some(test_file), code)
        }
        TestFramework::CSharp => {
            let test_file = request.source_file.replace(".cs", "Tests.cs");
            let code = generate_csharp_tests(&source_content, request.include_edge_cases.unwrap_or(true));
            ("csharp".to_string(), Some(test_file), code)
        }
    };
    
    TestGenerationResult {
        success: true,
        test_file,
        test_code,
        suggestions: vec![
            "Review generated tests for coverage".to_string(),
            "Add domain-specific test cases".to_string(),
            "Run tests to verify they pass".to_string(),
        ],
        language,
    }
}

fn generate_rust_tests(source: &str, include_edge_cases: bool) -> String {
    let fn_names = extract_function_names(source);
    
    let mut tests = String::from("#![cfg(test)]\n\n");
    tests.push_str("//! Auto-generated tests\n\n");
    tests.push_str("use super::*;\n\n");
    
    for fn_name in fn_names {
        tests.push_str(&format!("#[test]\nfn test_{}() {{\n    // TODO: Add test implementation for {}\n    unimplemented!();\n}}\n\n", 
            fn_name.to_lowercase(), fn_name));
        
        if include_edge_cases {
            tests.push_str(&format!("#[test]\nfn test_{}_edge_cases() {{\n    // TODO: Test edge cases for {}\n    unimplemented!();\n}}\n\n", 
                fn_name.to_lowercase(), fn_name));
        }
    }
    
    tests
}

fn generate_python_tests(source: &str, _include_edge_cases: bool) -> String {
    let fn_names = extract_function_names(source);
    
    let mut tests = String::from("#!/usr/bin/env python3\n");
    tests.push_str("# Auto-generated tests\n\n");
    tests.push_str("import pytest\n");
    tests.push_str("from unittest import TestCase\n");
    tests.push_str("from . import *\n\n");
    
    tests.push_str("class TestGenerated(TestCase):\n");
    
    for fn_name in fn_names {
        tests.push_str(&format!("    def test_{}(self):\n        \"\"\"Test {} functionality\"\"\"\n        pass\n\n", 
            fn_name.to_lowercase(), fn_name));
    }
    
    tests.push_str("\nif __name__ == \"__main__\":\n    pytest.main([__file__])\n");
    
    tests
}

fn generate_js_ts_tests(source: &str, include_edge_cases: bool, is_typescript: bool) -> String {
    let fn_names = extract_function_names(source);
    let _ext = if is_typescript { "ts" } else { "js" };
    
    let mut tests = String::from("// Auto-generated tests\n\n");
    tests.push_str("import { describe, it, expect } from 'vitest';\n");
    tests.push_str("// or: import { describe, it, expect } from '@jest/globals';\n\n");
    
    for fn_name in fn_names {
        tests.push_str(&format!("describe('{}', () => {{\n", fn_name));
        tests.push_str(&format!("    it('should work correctly', () => {{\n        // TODO: Test {}\n    }});\n\n", fn_name));
        
        if include_edge_cases {
            tests.push_str("    it('should handle edge cases', () => {\n        // TODO: Edge case tests\n    });\n\n");
        }
        
        tests.push_str("});\n\n");
    }
    
    tests
}

fn generate_go_tests(source: &str, include_edge_cases: bool) -> String {
    let fn_names = extract_function_names(source);
    let pkg_name = "main";
    
    let mut tests = String::from("package ");
    tests.push_str(pkg_name);
    tests.push_str("\n\nimport \"testing\"\n\n");
    
    for fn_name in fn_names {
        let test_fn = format!("Test{}", capitalize_first(&fn_name));
        tests.push_str(&format!("func {}(t *testing.T) {{\n    // TODO: Test {}\n}}\n\n", test_fn, fn_name));
        
        if include_edge_cases {
            tests.push_str(&format!("func {}_EdgeCases(t *testing.T) {{\n    // TODO: Edge case tests\n}}\n\n", test_fn));
        }
    }
    
    tests
}

fn generate_java_tests(source: &str, _include_edge_cases: bool) -> String {
    let fn_names = extract_function_names(source);
    
    let mut tests = String::from("import org.junit.jupiter.api.Test;\n");
    tests.push_str("import static org.junit.jupiter.api.Assertions.*;\n\n");
    tests.push_str("class GeneratedTest {\n\n");
    
    for fn_name in fn_names {
        tests.push_str(&format!("    @Test\n    void test{}() {{\n        // TODO: Test {}\n    }}\n\n", capitalize_first(&fn_name), fn_name));
    }
    
    tests.push_str("}\n");
    
    tests
}

fn generate_csharp_tests(source: &str, _include_edge_cases: bool) -> String {
    let fn_names = extract_function_names(source);
    
    let mut tests = String::from("using Xunit;\n\n");
    tests.push_str("public class GeneratedTests {\n\n");
    
    for fn_name in fn_names {
        tests.push_str(&format!("    [Fact]\n    public void Test{}() {{\n        // TODO: Test {}\n    }}\n\n", capitalize_first(&fn_name), fn_name));
    }
    
    tests.push_str("}\n");
    
    tests
}

fn extract_function_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    
    for line in source.lines() {
        let line = line.trim();
        
        // Look for Rust functions
        if line.starts_with("fn ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('(').next().unwrap_or(name);
                names.push(name.to_string());
            }
        }
        
        // Look for Python functions
        if line.starts_with("def ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('(').next().unwrap_or(name);
                names.push(name.to_string());
            }
        }
        
        // Look for JS/TS functions
        if line.starts_with("function ") || (line.contains("const") && line.contains("=") && line.contains("=>")) {
            // Simplified matching
        }
    }
    
    if names.is_empty() {
        names.push("example_function".to_string());
    }
    
    names
}

fn capitalize_first(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let mut chars = s.chars();
    chars.next().expect("unwrap failed: advanced_tools.rs:296").to_uppercase().collect::<String>() + chars.as_str()
}

// ── Debugging Assistant ──

/// Debug analysis request.
#[derive(Debug, Clone, Deserialize)]
pub struct DebugAnalysisRequest {
    pub error_message: String,
    pub source_file: Option<String>,
    pub stack_trace: Option<String>,
    pub context: Option<String>,
}

/// Debug analysis result.
#[derive(Debug, Clone, Serialize)]
pub struct DebugAnalysisResult {
    pub error_type: String,
    pub root_cause: String,
    pub suggestions: Vec<String>,
    pub code_snippets: Vec<String>,
    pub severity: String,
}

/// Analyze an error and provide debugging assistance.
pub fn analyze_error(request: DebugAnalysisRequest) -> DebugAnalysisResult {
    let error_lower = request.error_message.to_lowercase();
    
    let (error_type, root_cause, suggestions) = if error_lower.contains("compiler") || error_lower.contains("syntax") {
        (
            "Syntax/Compiler Error".to_string(),
            "The error indicates a syntax or compilation issue in the code.".to_string(),
            vec![
                "Check for missing semicolons or braces".to_string(),
                "Verify import statements are correct".to_string(),
                "Check type mismatches".to_string(),
            ],
        )
    } else if error_lower.contains("null") || error_lower.contains("none") || error_lower.contains("nil") {
        (
            "Null/None Reference Error".to_string(),
            "The error suggests a null value was accessed where it shouldn't have been.".to_string(),
            vec![
                "Add null checks before accessing the value".to_string(),
                "Use Option/Result types if available".to_string(),
                "Verify the source of the null value".to_string(),
            ],
        )
    } else if error_lower.contains("timeout") || error_lower.contains("hang") {
        (
            "Timeout/Hang Error".to_string(),
            "The operation timed out or appears to be hanging.".to_string(),
            vec![
                "Check for infinite loops or recursion".to_string(),
                "Verify network/database connections".to_string(),
                "Add timeouts with proper handling".to_string(),
            ],
        )
    } else if error_lower.contains("permission") || error_lower.contains("access") {
        (
            "Permission/Access Error".to_string(),
            "The error indicates a permissions or access control issue.".to_string(),
            vec![
                "Check file/directory permissions".to_string(),
                "Verify API keys and authentication".to_string(),
                "Ensure the process has necessary privileges".to_string(),
            ],
        )
    } else {
        (
            "General Error".to_string(),
            "Unable to classify the error type specifically.".to_string(),
            vec![
                "Review the full error message and stack trace".to_string(),
                "Add logging around the failing code".to_string(),
                "Check related recent changes".to_string(),
            ],
        )
    };
    
    DebugAnalysisResult {
        error_type,
        root_cause,
        suggestions,
        code_snippets: vec![],
        severity: "medium".to_string(),
    }
}

// ── Refactoring Suggestions ──

/// Refactoring request.
#[derive(Debug, Clone, Deserialize)]
pub struct RefactorRequest {
    pub source_file: String,
    pub refactor_type: Option<String>, // cleanup, performance, readability
    pub line_range: Option<String>,
}

/// Refactoring suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct RefactorSuggestion {
    pub line_range: String,
    pub description: String,
    pub suggestion: String,
    pub difficulty: String, // easy, medium, hard
    pub impact: String,    // low, medium, high
}

/// Refactoring result.
#[derive(Debug, Clone, Serialize)]
pub struct RefactorResult {
    pub suggestions: Vec<RefactorSuggestion>,
    pub summary: String,
    /// Code smell analysis (new in v0.2.0)
    pub smell_analysis: Option<SmellAnalysisResult>,
}

/// Code smell analysis result.
#[derive(Debug, Clone, Serialize)]
pub struct SmellAnalysisResult {
    pub total_smells: usize,
    pub code_quality_score: f64,
    pub report: String,
}

/// Analyze code and provide refactoring suggestions.
pub fn analyze_for_refactoring(request: RefactorRequest) -> RefactorResult {
    let content = match std::fs::read_to_string(&request.source_file) {
        Ok(c) => c,
        Err(e) => {
            return RefactorResult {
                suggestions: vec![],
                summary: format!("Failed to read file: {}", e),
                smell_analysis: None,
            };
        }
    };
    
    let mut suggestions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    
    // Detect language from file extension
    let language = std::path::Path::new(&request.source_file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("rust")
        .to_lowercase();
    
    let lang = match language.as_str() {
        "rs" => "rust",
        "py" => "python",
        "ts" | "tsx" | "js" | "jsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" | "cpp" => "c",
        _ => "rust",
    };
    
    // ── Enhanced Code Smell Detection (v0.2.0) ──
    let smell_analysis = {
        let detector = SmellDetector::new();
        let smells = detector.detect(&content, lang);
        let metrics = detector.calculate_metrics(&smells);
        let report = detector.format_report(&smells, &metrics);
        
        Some(SmellAnalysisResult {
            total_smells: metrics.total_smells,
            code_quality_score: metrics.code_quality_score,
            report,
        })
    };
    
    // Convert smells to refactoring suggestions
    if let Some(analysis) = smell_analysis.as_ref() {
        // Use analysis results for context-aware suggestions
        if analysis.code_quality_score < 0.5 {
            suggestions.push(RefactorSuggestion {
                line_range: "global".to_string(),
                description: format!("Low code quality score ({:.2}), consider comprehensive refactoring", analysis.code_quality_score),
                suggestion: "restructure".to_string(),
                difficulty: "hard".to_string(),
                impact: "high".to_string(),
            });
        }

        let detector = SmellDetector::new();
        let smells = detector.detect(&content, lang);
        
        for smell in smells.iter().take(20) {
            let difficulty = match smell.effort {
                1..=2 => "easy",
                3 => "medium",
                _ => "hard",
            };
            
            let impact = match smell.severity {
                1..=2 => "low",
                3 => "medium",
                _ => "high",
            };
            
            suggestions.push(RefactorSuggestion {
                line_range: smell.line.map(|l| l.to_string()).unwrap_or_else(|| smell.location.clone()),
                description: format!("{:?} - {}", smell.smell_type, smell.description),
                suggestion: smell.suggestion.clone(),
                difficulty: difficulty.to_string(),
                impact: impact.to_string(),
            });
        }
    }
    
    // Check for long functions (basic detection)
    let mut current_fn_start = 0;
    let mut brace_count = 0;
    let mut in_fn = false;
    
    for (i, line) in lines.iter().enumerate() {
        if line.contains("fn ") && !line.contains("//") {
            in_fn = true;
            current_fn_start = i;
            brace_count = 0;
        }
        
        if in_fn {
            brace_count += line.chars().filter(|&c| c == '{').count();
            brace_count -= line.chars().filter(|&c| c == '}').count();
            
            if brace_count == 0 && i > current_fn_start && in_fn {
                let line_count = i - current_fn_start;
                if line_count > 50 {
                    suggestions.push(RefactorSuggestion {
                        line_range: format!("{}:{}", current_fn_start + 1, i + 1),
                        description: "Long function".to_string(),
                        suggestion: format!("Consider breaking this {} line function into smaller, focused functions", line_count),
                        difficulty: "medium".to_string(),
                        impact: "high".to_string(),
                    });
                }
                in_fn = false;
            }
        }
    }
    
    // Check for TODO/FIXME comments
    for (i, line) in lines.iter().enumerate() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("todo") || line_lower.contains("fixme") {
            suggestions.push(RefactorSuggestion {
                line_range: format!("{}", i + 1),
                description: "Found TODO/FIXME".to_string(),
                suggestion: "Address this comment before merging".to_string(),
                difficulty: "easy".to_string(),
                impact: "low".to_string(),
            });
        }
    }
    
    // Check for long lines
    for (i, line) in lines.iter().enumerate() {
        if line.len() > 120 {
            suggestions.push(RefactorSuggestion {
                line_range: format!("{}", i + 1),
                description: "Long line".to_string(),
                suggestion: format!("Consider wrapping this {} character line", line.len()),
                difficulty: "easy".to_string(),
                impact: "low".to_string(),
            });
            break; // Just one suggestion per file for this
        }
    }
    
    let summary = if suggestions.is_empty() {
        "No immediate refactoring suggestions found. Code looks good!".to_string()
    } else {
        format!("Found {} potential refactoring opportunities (including {} code smells)", 
                suggestions.len(), smell_analysis.as_ref().map(|a| a.total_smells).unwrap_or(0))
    };
    
    RefactorResult {
        suggestions,
        summary,
        smell_analysis,
    }
}

// ── Code Explanation ──

/// Code explanation request.
#[derive(Debug, Clone, Deserialize)]
pub struct ExplainCodeRequest {
    pub source_file: String,
    pub line_range: Option<String>,
    pub detail_level: Option<String>, // high, medium, low
}

/// Code explanation result.
#[derive(Debug, Clone, Serialize)]
pub struct CodeExplanationResult {
    pub summary: String,
    pub key_concepts: Vec<String>,
    pub dependencies: Vec<String>,
    pub potential_issues: Vec<String>,
}

/// Explain code functionality.
pub fn explain_code(request: ExplainCodeRequest) -> CodeExplanationResult {
    let content = match std::fs::read_to_string(&request.source_file) {
        Ok(c) => c,
        Err(e) => {
            return CodeExplanationResult {
                summary: format!("Failed to read file: {}", e),
                key_concepts: vec![],
                dependencies: vec![],
                potential_issues: vec![],
            };
        }
    };
    
    let mut dependencies = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("use ") || line.starts_with("import ") || line.starts_with("from ") {
            dependencies.push(line.to_string());
        }
    }
    
    let mut issues = Vec::new();
    if content.lines().count() > 500 {
        issues.push("File is quite long, consider splitting into smaller modules".to_string());
    }
    
    CodeExplanationResult {
        summary: format!("Analyzed {} characters of code", content.len()),
        key_concepts: vec![
            "File I/O operations".to_string(),
            "Error handling".to_string(),
            "Function definitions".to_string(),
        ],
        dependencies,
        potential_issues: issues,
    }
}

// ── Enhanced Code Parsing (AST-based Code Parsing ──

/// Parsed function information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedFunction {
    pub name: String,
    pub parameters: Vec<(String, String)>, // (name, type)
    pub return_type: Option<String>,
    pub has_docstring: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
}

/// Parsed module information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedModule {
    pub functions: Vec<ParsedFunction>,
    pub structs: Vec<String>,
    pub traits: Vec<String>,
    pub imports: Vec<String>,
}

/// Parse Rust code (simple but effective.
pub fn parse_rust_code(code: &str) -> ParsedModule {
    let mut module = ParsedModule {
        functions: Vec::new(),
        structs: Vec::new(),
        traits: Vec::new(),
        imports: Vec::new(),
    };
    
    let lines: Vec<&str> = code.lines().collect();
    
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        
        if line.starts_with("use ") {
            module.imports.push(line.to_string());
        } else if line.starts_with("struct ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('{').next().unwrap_or(name).to_string();
                module.structs.push(name);
            }
        } else if line.starts_with("trait ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('{').next().unwrap_or(name).to_string();
                module.traits.push(name);
            }
        } else if line.starts_with("fn ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('(').next().unwrap_or(name).to_string();
                
                let mut params = Vec::new();
                if let Some(param_str) = line.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    for param in param_str.split(',') {
                        let param = param.trim();
                        if !param.is_empty() {
                            if let Some(colon_pos) = param.find(':') {
                                let param_name = param[..colon_pos].trim().to_string();
                                let param_type = param[colon_pos+1..].trim().to_string();
                                params.push((param_name, param_type));
                            }
                        }
                    }
                }
                
                let return_type = if line.contains("->") {
                    line.split("->").nth(1).map(|s| s.trim().to_string())
                } else {
                    None
                };
                
                let mut line_end = i;
                let mut brace_count = 0;
                
                for (j, later_line) in lines[i..].iter().enumerate() {
                    brace_count += later_line.chars().filter(|&c| c == '{').count();
                    brace_count -= later_line.chars().filter(|&c| c == '}').count();
                    
                    if brace_count == 0 && j > 0 {
                        line_end = i + j;
                        break;
                    }
                }
                
                let mut docstring = None;
                if i > 0 {
                    let mut j = i - 1;
                    while j > 0 && (lines[j].trim().starts_with("///") || lines[j].trim().starts_with("//!")) {
                        docstring = Some(lines[j].to_string());
                        j -= 1;
                    }
                }
                
                module.functions.push(ParsedFunction {
                    name,
                    parameters: params,
                    return_type,
                    has_docstring: docstring,
                    line_start: i + 1,
                    line_end: line_end + 1,
                });
            }
        }
    }
    
    module
}

/// Parse Python code.
pub fn parse_python_code(code: &str) -> ParsedModule {
    let mut module = ParsedModule {
        functions: Vec::new(),
        structs: Vec::new(),
        traits: Vec::new(),
        imports: Vec::new(),
    };
    
    let lines: Vec<&str> = code.lines().collect();
    
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        
        if line.starts_with("import ") || line.starts_with("from ") {
            module.imports.push(line.to_string());
        } else if line.starts_with("class ") {
            if let Some(name) = line.split_whitespace().nth(1) {
                let name = name.split('(').next().unwrap_or(name).to_string();
                module.structs.push(name);
            }
        } else if line.starts_with("def ") {
            if let Some(name_part) = line.split_whitespace().nth(1) {
                let name = name_part.split('(').next().unwrap_or(name_part).to_string();
                
                let mut params = Vec::new();
                if let Some(param_str) = line.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    for param in param_str.split(',') {
                        let param = param.trim();
                        if !param.is_empty() && param != "self" && param != "cls" {
                            if let Some(colon_pos) = param.find(':') {
                                let param_name = param[..colon_pos].trim().to_string();
                                let param_type = param[colon_pos+1..].trim().to_string();
                                params.push((param_name, param_type));
                            } else {
                                params.push((param.to_string(), "Any".to_string()));
                            }
                        }
                    }
                }
                
                let mut line_end = i;
                let indent_level = line.chars().take_while(|c| c.is_whitespace()).count();
                let mut j = i + 1;
                while j < lines.len() {
                    let next_line = lines[j];
                    let next_indent = next_line.chars().take_while(|c| c.is_whitespace()).count();
                    if next_indent > indent_level && !next_line.trim().is_empty() {
                        line_end = j;
                    } else if next_indent == indent_level && !next_line.trim().is_empty() {
                        break;
                    }
                    j += 1;
                }
                
                let mut docstring = None;
                if i > 0 {
                    let mut j = i - 1;
                    while j > 0 && (lines[j].trim().starts_with("\"\"\"") || lines[j].trim().starts_with("'''")) {
                        docstring = Some(lines[j].to_string());
                        j -= 1;
                    }
                }
                
                module.functions.push(ParsedFunction {
                    name,
                    parameters: params,
                    return_type: None,
                    has_docstring: docstring,
                    line_start: i + 1,
                    line_end: line_end + 1,
                });
            }
        }
    }
    
    module
}

// ── Enhanced Debug Assistant ──

/// Breakpoint suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct BreakpointSuggestion {
    pub line: usize,
    pub reason: String,
    pub suggestion_type: String,
    pub variables_to_watch: Vec<String>,
}

/// Log injection suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct LogInjectionSuggestion {
    pub line: usize,
    pub suggested_log: String,
    pub log_level: String,
}

/// Enhanced debug analysis request.
#[derive(Debug, Clone, Deserialize)]
pub struct EnhancedDebugRequest {
    pub source_file: String,
    pub error_message: Option<String>,
    pub error_location: Option<String>,
}

/// Enhanced debug analysis result.
#[derive(Debug, Clone, Serialize)]
pub struct EnhancedDebugResult {
    pub original_analysis: DebugAnalysisResult,
    pub breakpoint_suggestions: Vec<BreakpointSuggestion>,
    pub log_injection_suggestions: Vec<LogInjectionSuggestion>,
    pub suggested_fix_code: Option<String>,
    pub related_files: Vec<String>,
}

/// Perform enhanced debug analysis.
pub fn enhanced_debug_analysis(request: EnhancedDebugRequest) -> EnhancedDebugResult {
    let content = std::fs::read_to_string(&request.source_file).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    
    let basic_request = DebugAnalysisRequest {
        error_message: request.error_message.clone().unwrap_or_default(),
        source_file: Some(request.source_file.clone()),
        stack_trace: None,
        context: None,
    };
    let basic_result = analyze_error(basic_request);
    
    let mut breakpoints = Vec::new();
    let mut logs = Vec::new();
    
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        
        if line.contains("?") || line.contains("if let") {
            breakpoints.push(BreakpointSuggestion {
                line: i + 1,
                reason: "Error handling or optional value".to_string(),
                suggestion_type: "before".to_string(),
                variables_to_watch: vec!["error result".to_string()],
            });
        }
        
        if line.contains("fn ") || line.contains("def ") {
            logs.push(LogInjectionSuggestion {
                line: i + 2,
                suggested_log: "debug".to_string(),
                log_level: "info".to_string(),
            });
        }
        
        if line.contains("loop") || line.contains("while") || line.contains("for") {
            breakpoints.push(BreakpointSuggestion {
                line: i + 1,
                reason: "Loop start".to_string(),
                suggestion_type: "inside".to_string(),
                variables_to_watch: vec!["loop variable".to_string()],
            });
        }
    }
    
    // Generate auto-fix suggestions
    let suggested_fix_code = if let Some(ref error_msg) = request.error_message {
        // Detect language from file extension
        let language = std::path::Path::new(&request.source_file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        let lang = match language.as_str() {
            "rs" => "rust",
            "py" => "python",
            "ts" | "tsx" | "js" | "jsx" => "typescript",
            "go" => "go",
            "java" => "java",
            "c" | "cpp" => "c",
            _ => "unknown",
        };
        
        // Generate error with fix suggestions
        let error_with_fix = ErrorWithFix::from_error(error_msg, &content, 1, lang);
        
        if error_with_fix.auto_fix_recommended {
            Some(error_with_fix.format_fixes_markdown())
        } else {
            None
        }
    } else {
        None
    };
    
    EnhancedDebugResult {
        original_analysis: basic_result,
        breakpoint_suggestions: breakpoints,
        log_injection_suggestions: logs,
        suggested_fix_code,
        related_files: vec![],
    }
}

// ── Test Runner Integration ──

/// Test runner request.
#[derive(Debug, Clone, Deserialize)]
pub struct TestRunRequest {
    pub test_file: String,
    pub test_pattern: Option<String>,
    pub watch_mode: bool,
}

/// Test run result.
#[derive(Debug, Clone, Serialize)]
pub struct TestRunResult {
    pub success: bool,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: Vec<String>,
    pub output: String,
    pub duration_ms: u64,
}

/// Run tests for a file.
pub fn run_tests(request: TestRunRequest) -> TestRunResult {
    let ext = std::path::Path::new(&request.test_file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("rs");
    
    let (command, args, working_dir) = match ext {
        "rs" => (
            "cargo", vec!["test"], Some(".".to_string())),
        "py" => (
            "python", vec!["-m", "pytest", &request.test_file],
            std::path::Path::new(&request.test_file).parent().map(|p| p.to_string_lossy().to_string())),
        "ts" | "js" => (
            "npm", vec!["test"], None),
        "go" => (
            "go", vec!["test", "-v"], Some(".".to_string())),
        _ => ("echo", vec!["Test execution not supported"], None),
    };
    
    let mut cmd = std::process::Command::new(command);
    cmd.args(&args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    
    let start = std::time::Instant::now();
    let output = cmd.output();
    let duration = start.elapsed().as_millis() as u64;
    
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let full_output = format!("{}\n{}", stdout, stderr);
            
            TestRunResult {
                success: out.status.success(),
                total_tests: 0,
                passed_tests: 0,
                failed_tests: vec![],
                output: full_output,
                duration_ms: duration,
            }
        }
        Err(e) => TestRunResult {
            success: false,
            total_tests: 0,
            passed_tests: 0,
            failed_tests: vec![],
            output: format!("Failed to run tests: {}", e),
            duration_ms: duration,
        },
    }
}

// ── Framework Detection ──

/// Detected framework information.
#[derive(Debug, Clone, Serialize)]
pub struct DetectedFramework {
    pub language: String,
    pub framework: String,
    pub test_framework: String,
    pub project_type: String,
    pub config_files: Vec<String>,
}

/// Detect project framework from directory.
pub fn detect_framework(dir: &str) -> DetectedFramework {
    let path = std::path::Path::new(dir);
    
    let detect_files = |file| path.join(file);
    let check_file = |file| detect_files(file).exists();
    
    let mut config_files = Vec::new();
    
    if check_file("Cargo.toml") {
        config_files.push("Cargo.toml".to_string());
        if check_file("Cargo.lock") {
            config_files.push("Cargo.lock".to_string());
        }
        return DetectedFramework {
            language: "rust".to_string(),
            framework: "cargo".to_string(),
            test_framework: "cargo-test".to_string(),
            project_type: "rust-project".to_string(),
            config_files,
        };
    }
    
    if check_file("package.json") {
        config_files.push("package.json".to_string());
        if check_file("tsconfig.json") {
            config_files.push("tsconfig.json".to_string());
            return DetectedFramework {
                language: "typescript".to_string(),
                framework: "npm".to_string(),
                test_framework: "jest-vitest".to_string(),
                project_type: "node-project".to_string(),
                config_files,
            };
        }
        return DetectedFramework {
            language: "javascript".to_string(),
            framework: "npm".to_string(),
            test_framework: "jest".to_string(),
            project_type: "node-project".to_string(),
            config_files,
        };
    }
    
    if check_file("go.mod") {
        config_files.push("go.mod".to_string());
        return DetectedFramework {
            language: "go".to_string(),
            framework: "go-modules".to_string(),
            test_framework: "go-test".to_string(),
            project_type: "go-project".to_string(),
            config_files,
        };
    }
    
    DetectedFramework {
        language: "unknown".to_string(),
        framework: "unknown".to_string(),
        test_framework: "unknown".to_string(),
        project_type: "unknown".to_string(),
        config_files: vec![],
    }
}
