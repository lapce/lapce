//! Execution Path Analysis — Predict bug locations and analyze code flow.
//!
//! This module provides:
//! - Control flow analysis
//! - Data flow tracking
//! - Bug prediction
//! - Path visualization

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A node in the control flow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CFGNode {
    pub id: usize,
    pub node_type: CFGNodeType,
    pub statements: Vec<String>,
    pub successors: Vec<usize>,
    pub predecessors: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CFGNodeType {
    Entry,
    Exit,
    Assignment,
    Branch,
    Loop,
    Call,
    Return,
}

/// Control flow graph.
#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    pub nodes: HashMap<usize, CFGNode>,
    pub entry_id: usize,
    pub exit_id: usize,
}

/// An execution path through the code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPath {
    pub path_id: usize,
    pub nodes: Vec<usize>,
    pub probability: f64, // Estimated execution probability
    pub is_testable: bool,
    pub is_critical: bool,
}

/// Bug prediction result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugPrediction {
    pub location: BugLocation,
    pub bug_type: BugType,
    pub confidence: f64,
    pub explanation: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugLocation {
    pub file: PathBuf,
    pub line: usize,
    pub function: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BugType {
    NullPointer,
    DivisionByZero,
    OffByOne,
    RaceCondition,
    MemoryLeak,
    UnhandledError,
    InfiniteLoop,
    DeadCode,
}

/// Execution path analyzer.
pub struct PathAnalyzer {
    cfg: Option<ControlFlowGraph>,
}

impl PathAnalyzer {
    pub fn new() -> Self {
        Self { cfg: None }
    }

    /// Build control flow graph from source.
    pub fn build_cfg(&mut self, source: &str, _language: &str) -> ControlFlowGraph {
        let lines: Vec<&str> = source.lines().collect();
        let mut nodes = HashMap::new();
        let mut node_id = 0;
        let entry_id = node_id;
        
        nodes.insert(entry_id, CFGNode {
            id: entry_id,
            node_type: CFGNodeType::Entry,
            statements: vec!["Entry".to_string()],
            successors: vec![],
            predecessors: vec![],
        });
        
        let mut current_id = entry_id;
        let mut in_branch = false;
        let mut branch_stack: Vec<(usize, usize)> = Vec::new(); // (condition_id, else_target)

        for line in lines.iter() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            
            node_id += 1;
            let new_node_id = node_id;
            
            // Determine node type
            let node_type = if trimmed.starts_with("if") || trimmed.starts_with("match") {
                in_branch = true;
                CFGNodeType::Branch
            } else if trimmed.starts_with("for") || trimmed.starts_with("while") || trimmed.starts_with("loop") {
                CFGNodeType::Loop
            } else if trimmed.starts_with("return") {
                CFGNodeType::Return
            } else if trimmed.contains('(') && trimmed.contains(')') {
                CFGNodeType::Call
            } else {
                CFGNodeType::Assignment
            };
            
            nodes.insert(new_node_id, CFGNode {
                id: new_node_id,
                node_type,
                statements: vec![trimmed.to_string()],
                successors: vec![],
                predecessors: vec![current_id],
            });
            
            // Link to previous node
            if let Some(prev) = nodes.get_mut(&current_id) {
                prev.successors.push(new_node_id);
            }
            
            // Handle branch structures
            if in_branch
                && (trimmed.ends_with('{') || trimmed.ends_with(':')) {
                    branch_stack.push((new_node_id, new_node_id + 1));
                    in_branch = false;
                }
            
            current_id = new_node_id;
        }
        
        let exit_id = node_id + 1;
        nodes.insert(exit_id, CFGNode {
            id: exit_id,
            node_type: CFGNodeType::Exit,
            statements: vec!["Exit".to_string()],
            successors: vec![],
            predecessors: vec![current_id],
        });
        
        if let Some(prev) = nodes.get_mut(&current_id) {
            prev.successors.push(exit_id);
        }
        
        let cfg = ControlFlowGraph {
            nodes,
            entry_id,
            exit_id,
        };
        
        self.cfg = Some(cfg.clone());
        cfg
    }

    /// Analyze paths and predict potential bugs.
    pub fn predict_bugs(&self, source: &str) -> Vec<BugPrediction> {
        let mut predictions = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            // Check for null/none access
            if trimmed.contains(".") && !trimmed.contains("?")
                && (trimmed.contains("unwrap") || trimmed.contains("expect")) {
                    predictions.push(BugPrediction {
                        location: BugLocation {
                            file: PathBuf::from("."),
                            line: i + 1,
                            function: None,
                        },
                        bug_type: BugType::NullPointer,
                        confidence: 0.7,
                        explanation: "Potential null pointer access without proper handling".to_string(),
                        suggestions: vec![
                            "Use optional chaining (?.)".to_string(),
                            "Add null check before access".to_string(),
                            "Consider using expect with better message".to_string(),
                        ],
                    });
                }
            
            // Check for division
            if trimmed.contains('/') && !trimmed.contains("//") {
                if let Some(expr) = trimmed.split('/').nth(1) {
                    let divisor = expr.trim().split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
                    if divisor == "0" || divisor.is_empty() {
                        predictions.push(BugPrediction {
                            location: BugLocation {
                                file: PathBuf::from("."),
                                line: i + 1,
                                function: None,
                            },
                            bug_type: BugType::DivisionByZero,
                            confidence: 0.8,
                            explanation: "Division by zero or unvalidated divisor".to_string(),
                            suggestions: vec![
                                "Add divisor validation".to_string(),
                                "Use checked_div or safe division".to_string(),
                            ],
                        });
                    }
                }
            }
            
            // Check for loop conditions
            if trimmed.starts_with("for ") && trimmed.contains("<=") {
                predictions.push(BugPrediction {
                    location: BugLocation {
                        file: PathBuf::from("."),
                        line: i + 1,
                        function: None,
                    },
                    bug_type: BugType::OffByOne,
                    confidence: 0.5,
                    explanation: "Loop with <= might have off-by-one issue".to_string(),
                    suggestions: vec![
                        "Verify loop bounds".to_string(),
                        "Consider using .enumerate() or .zip()".to_string(),
                    ],
                });
            }
            
            // Check for unreachable code
            if trimmed.starts_with("return") && i < lines.len() - 1 {
                let remaining: Vec<_> = lines.iter().skip(i + 1).collect();
                let has_meaningful = remaining.iter().any(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with("//")
                });
                
                if has_meaningful {
                    predictions.push(BugPrediction {
                        location: BugLocation {
                            file: PathBuf::from("."),
                            line: i + 1,
                            function: None,
                        },
                        bug_type: BugType::DeadCode,
                        confidence: 0.9,
                        explanation: "Code after return statement is unreachable".to_string(),
                        suggestions: vec![
                            "Remove unreachable code".to_string(),
                            "Verify logic flow".to_string(),
                        ],
                    });
                }
            }
        }
        
        predictions
    }

    /// Get all execution paths.
    pub fn get_paths(&self) -> Vec<ExecutionPath> {
        if let Some(ref cfg) = self.cfg {
            let mut paths = Vec::new();
            self.find_paths(
                cfg.entry_id,
                cfg.exit_id,
                &cfg.nodes,
                &mut Vec::new(),
                &mut paths,
                0,
            );
            
            // Assign probabilities
            let total = paths.len() as f64;
            for (i, path) in paths.iter_mut().enumerate() {
                path.path_id = i;
                path.probability = 1.0 / total;
                path.is_testable = path.nodes.len() < 10;
                path.is_critical = path.nodes.iter().any(|n| {
                    cfg.nodes.get(n).map(|node| {
                        matches!(node.node_type, CFGNodeType::Branch | CFGNodeType::Loop)
                    }).unwrap_or(false)
                });
            }
            
            paths
        } else {
            Vec::new()
        }
    }

    /// Recursively find all paths.
    fn find_paths(
        &self,
        current: usize,
        exit: usize,
        nodes: &HashMap<usize, CFGNode>,
        current_path: &mut Vec<usize>,
        all_paths: &mut Vec<ExecutionPath>,
        path_id: usize,
    ) {
        current_path.push(current);
        
        if current == exit {
            all_paths.push(ExecutionPath {
                path_id,
                nodes: current_path.clone(),
                probability: 0.0,
                is_testable: false,
                is_critical: false,
            });
        } else if let Some(node) = nodes.get(&current) {
            for &successor in &node.successors {
                self.find_paths(successor, exit, nodes, current_path, all_paths, path_id,);
            }
        }
        
        current_path.pop();
    }

    /// Format predictions as markdown report.
    pub fn format_report(&self, predictions: &[BugPrediction]) -> String {
        let mut md = String::new();
        
        md.push_str("# Bug Prediction Report\n\n");
        md.push_str(&format!("**Predictions:** {} potential issues\n\n", predictions.len()));
        
        for (i, pred) in predictions.iter().enumerate() {
            md.push_str(&format!("## {}. {:?} (Confidence: {:.0}%)\n\n", i + 1, pred.bug_type, pred.confidence * 100.0));
            md.push_str(&format!("**Location:** {}:{}\n\n", pred.location.file.display(), pred.location.line));
            md.push_str(&format!("**Explanation:** {}\n\n", pred.explanation));
            
            md.push_str("**Suggestions:**\n");
            for suggestion in &pred.suggestions {
                md.push_str(&format!("- {}\n", suggestion));
            }
            md.push_str("\n---\n\n");
        }
        
        md
    }
}

impl Default for PathAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfg_build() {
        let mut analyzer = PathAnalyzer::new();
        let source = "fn test() {\n    let x = 1;\n    if x > 0 {\n        return;\n    }\n}\n";
        
        let cfg = analyzer.build_cfg(source, "rust");
        assert!(!cfg.nodes.is_empty());
    }

    #[test]
    fn test_bug_prediction() {
        let analyzer = PathAnalyzer::new();
        let source = "let x = 1;\nlet y = x.unwrap();\n";
        
        let predictions = analyzer.predict_bugs(source);
        assert!(!predictions.is_empty());
    }
}
