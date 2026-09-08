//! ImpactAnalyzer — Predict the ripple effects of code changes.
//!
//! Inspired by Understand Anything's Diff Impact Analysis.
//! Uses the code graph to determine which files/nodes are affected by edits.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use super::{CodeGraph, CodeEdge, EdgeKind};

/// Configuration for impact analysis depth.
#[derive(Debug, Clone)]
pub struct ImpactConfig {
    /// How deep to traverse dependencies (default: 3)
    pub max_depth: usize,
    /// Include transitive dependencies
    pub include_transitive: bool,
    /// Include test files that cover affected modules
    pub include_tests: bool,
    /// Minimum confidence to report (0.0-1.0)
    pub min_confidence: f64,
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            include_transitive: true,
            include_tests: true,
            min_confidence: 0.3,
        }
    }
}

/// A file that may be affected by a change.
#[derive(Debug, Clone)]
pub struct AffectedFile {
    pub file_path: PathBuf,
    pub confidence: f64,        // 0.0-1.0
    pub reason: ImpactReason,
    pub depth: usize,
    pub affected_nodes: Vec<String>,  // Node IDs that trigger the impact
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpactReason {
    DirectEdit,        // File being directly edited
    ContainsUsedType,  // Contains type used by edited file
    CalledBy,          // Function called by edited file
    CallsInto,         // Function that calls into edited file
    SameDomain,        // Same business domain
    Implements,        // Trait implementor
    TestCoverage,      // Test file covering module
}

/// Result of impact analysis.
#[derive(Debug, Clone)]
pub struct ImpactResult {
    pub affected_files: Vec<AffectedFile>,
    pub total_files: usize,
    pub max_depth_reached: usize,
    pub analyzed_at: std::time::Instant,
}

/// Analyzes the impact of code changes across the codebase.
pub struct ImpactAnalyzer {
    pub config: ImpactConfig,
}

impl ImpactAnalyzer {
    pub fn new() -> Self {
        Self {
            config: ImpactConfig::default(),
        }
    }

    pub fn with_config(config: ImpactConfig) -> Self {
        Self { config }
    }

    /// Analyze the impact of changes to specific files.
    pub fn analyze(&self, changed_files: &[PathBuf], graph: &CodeGraph) -> ImpactResult {
        let start = std::time::Instant::now();

        // Find the node IDs for each changed file
        let mut start_node_ids: Vec<String> = Vec::new();
        for file in changed_files {
            let nodes = graph.nodes_in_file(file);
            for node in nodes {
                start_node_ids.push(node.id.clone());
            }
        }

        let mut max_depth_reached: usize = 0;

        if start_node_ids.is_empty() {
            // If no nodes are found, report direct edit on the files themselves
            let affected_files: Vec<AffectedFile> = changed_files
                .iter()
                .map(|f| AffectedFile {
                    file_path: f.clone(),
                    confidence: 1.0,
                    reason: ImpactReason::DirectEdit,
                    depth: 0,
                    affected_nodes: vec![],
                })
                .collect();
            return ImpactResult {
                total_files: affected_files.len(),
                affected_files,
                max_depth_reached: 0,
                analyzed_at: start,
            };
        }

        // BFS traversal to find affected nodes
        let affected = self.bfs_impact(&start_node_ids, graph);
        for (_, depth, _) in &affected {
            if *depth > max_depth_reached {
                max_depth_reached = *depth;
            }
        }

        // Convert affected nodes to AffectedFile entries
        let mut file_map: std::collections::HashMap<PathBuf, AffectedFile> =
            std::collections::HashMap::new();

        // First, add direct edit files
        for file in changed_files {
            file_map.insert(
                file.clone(),
                AffectedFile {
                    file_path: file.clone(),
                    confidence: 1.0,
                    reason: ImpactReason::DirectEdit,
                    depth: 0,
                    affected_nodes: start_node_ids.clone(),
                },
            );
        }

        // Then add each affected (transitive) file
        for (node_id, depth, reason) in &affected {
            if let Some(node) = graph.get_node(node_id) {
                let file_path = node.file_path.clone();
                let confidence = self.compute_confidence(*depth);

                if confidence < self.config.min_confidence {
                    continue;
                }

                let entry = file_map.entry(file_path.clone()).or_insert_with(|| {
                    // Check if this is a test file and if we should include tests
                    let is_test = file_path.to_string_lossy().contains("test");
                    if is_test && !self.config.include_tests {
                        // We still create the entry but it may be filtered out
                    }
                    AffectedFile {
                        file_path: file_path.clone(),
                        confidence,
                        reason: reason.clone(),
                        depth: *depth,
                        affected_nodes: vec![],
                    }
                });

                // Update confidence/depth if this path is better (lower depth)
                if *depth < entry.depth {
                    entry.depth = *depth;
                    entry.confidence = confidence;
                    entry.reason = reason.clone();
                }
                entry.affected_nodes.push(node_id.clone());
            }
        }

        let affected_files: Vec<AffectedFile> = file_map.into_values().collect();
        let affected_files = self.deduplicate(affected_files);

        // Filter out test files if not requested
        let affected_files = if !self.config.include_tests {
            affected_files
                .into_iter()
                .filter(|f| !f.file_path.to_string_lossy().contains("test"))
                .collect()
        } else {
            affected_files
        };

        let total_files = affected_files.len();

        ImpactResult {
            affected_files,
            total_files,
            max_depth_reached,
            analyzed_at: start,
        }
    }

    /// BFS traversal to find all affected nodes starting from changed nodes.
    fn bfs_impact(
        &self,
        start_nodes: &[String],
        graph: &CodeGraph,
    ) -> Vec<(String, usize, ImpactReason)> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut results: Vec<(String, usize, ImpactReason)> = Vec::new();

        // Seed the queue with direct edits (depth 0)
        for node_id in start_nodes {
            if visited.insert(node_id.clone()) {
                queue.push_back((node_id.clone(), 0));
            }
        }

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth > 0 {
                // Only add non-start nodes to results
                if let Some(_node) = graph.get_node(&current_id) {
                    // Compute a reason based on how we reached this node
                    let reason = ImpactReason::ContainsUsedType; // default
                    results.push((current_id.clone(), depth, reason));
                }
            }

            if depth >= self.config.max_depth {
                continue;
            }

            // Follow outgoing edges
            for edge in graph.outgoing_edges(&current_id) {
                let reason = self.impact_reason(edge);
                if visited.insert(edge.target_id.clone()) {
                    queue.push_back((edge.target_id.clone(), depth + 1));
                    results.push((edge.target_id.clone(), depth + 1, reason));
                }
            }

            // Follow incoming edges (reverse dependencies)
            if self.config.include_transitive {
                for edge in graph.incoming_edges(&current_id) {
                    let reason = self.impact_reason(edge);
                    if visited.insert(edge.source_id.clone()) {
                        queue.push_back((edge.source_id.clone(), depth + 1));
                        results.push((edge.source_id.clone(), depth + 1, reason));
                    }
                }
            }
        }

        results
    }

    /// Determine the impact reason for an edge traversal.
    fn impact_reason(&self, edge: &CodeEdge) -> ImpactReason {
        match edge.kind {
            EdgeKind::Calls => ImpactReason::CalledBy,
            EdgeKind::Implements => ImpactReason::Implements,
            EdgeKind::Extends => ImpactReason::Implements,
            EdgeKind::Contains => ImpactReason::ContainsUsedType,
            EdgeKind::Uses => ImpactReason::ContainsUsedType,
            EdgeKind::Inherits => ImpactReason::ContainsUsedType,
        }
    }

    /// Compute confidence based on depth in the dependency graph.
    fn compute_confidence(&self, depth: usize) -> f64 {
        match depth {
            0 => 1.0,
            1 => 0.8,
            2 => 0.5,
            _ => 0.3_f64.max(1.0 / (depth as f64 + 1.0)),
        }
    }

    /// Filter and deduplicate affected files.
    fn deduplicate(&self, files: Vec<AffectedFile>) -> Vec<AffectedFile> {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        files
            .into_iter()
            .filter(|f| seen.insert(f.file_path.clone()))
            .collect()
    }

    /// Generate a human-readable report.
    pub fn format_report(&self, result: &ImpactResult) -> String {
        let mut report = String::new();
        report.push_str("=== Impact Analysis Report ===\n");
        report.push_str(&format!("Total affected files: {}\n", result.total_files));
        report.push_str(&format!(
            "Max depth reached: {}\n\n",
            result.max_depth_reached
        ));

        for (i, file) in result.affected_files.iter().enumerate() {
            report.push_str(&format!(
                "{}. {} (confidence: {:.2}, depth: {})\n",
                i + 1,
                file.file_path.display(),
                file.confidence,
                file.depth
            ));
            report.push_str(&format!(
                "   Reason: {:?}\n",
                file.reason
            ));
            if !file.affected_nodes.is_empty() {
                report.push_str(&format!(
                    "   Affected nodes: {}\n",
                    file.affected_nodes.join(", ")
                ));
            }
        }

        report
    }

    /// Generate machine-readable JSON.
    pub fn to_json(&self, result: &ImpactResult) -> serde_json::Value {
        let files: Vec<serde_json::Value> = result
            .affected_files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "file_path": f.file_path.to_string_lossy(),
                    "confidence": f.confidence,
                    "reason": format!("{:?}", f.reason),
                    "depth": f.depth,
                    "affected_nodes": f.affected_nodes,
                })
            })
            .collect();

        serde_json::json!({
            "total_files": result.total_files,
            "max_depth_reached": result.max_depth_reached,
            "affected_files": files,
        })
    }
}

impl Default for ImpactAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::{CodeGraph, CodeNode, CodeNodeKind, CodeEdge, EdgeKind};
    use std::path::PathBuf;

    fn make_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();

        graph.add_node(CodeNode {
            id: "main".into(),
            name: "main".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/main.rs"),
            start_line: 1,
            end_line: 10,
            doc_comment: None,
            visibility: None,
        });

        graph.add_node(CodeNode {
            id: "helper".into(),
            name: "helper_fn".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/helper.rs"),
            start_line: 1,
            end_line: 5,
            doc_comment: None,
            visibility: None,
        });

        graph.add_node(CodeNode {
            id: "utils".into(),
            name: "Utils".into(),
            kind: CodeNodeKind::Struct,
            file_path: PathBuf::from("src/utils.rs"),
            start_line: 1,
            end_line: 20,
            doc_comment: None,
            visibility: None,
        });

        graph.add_node(CodeNode {
            id: "deep".into(),
            name: "deep_func".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/deep.rs"),
            start_line: 1,
            end_line: 3,
            doc_comment: None,
            visibility: None,
        });

        // main -> helper
        graph.add_edge(CodeEdge {
            source_id: "main".into(),
            target_id: "helper".into(),
            kind: EdgeKind::Calls,
        });

        // helper -> utils
        graph.add_edge(CodeEdge {
            source_id: "helper".into(),
            target_id: "utils".into(),
            kind: EdgeKind::Uses,
        });

        // utils -> deep
        graph.add_edge(CodeEdge {
            source_id: "utils".into(),
            target_id: "deep".into(),
            kind: EdgeKind::Uses,
        });

        graph
    }

    #[test]
    fn test_impact_analyzer_empty() {
        let analyzer = ImpactAnalyzer::new();
        let graph = CodeGraph::new();
        let result = analyzer.analyze(&[], &graph);
        assert_eq!(result.total_files, 0);
        assert_eq!(result.max_depth_reached, 0);
    }

    #[test]
    fn test_impact_analyzer_direct_edit() {
        let analyzer = ImpactAnalyzer::new();
        let graph = make_graph();
        let changed = vec![PathBuf::from("src/main.rs")];
        let result = analyzer.analyze(&changed, &graph);

        // Should include direct edit + affected files
        assert!(result.total_files >= 1);
        let has_direct = result
            .affected_files
            .iter()
            .any(|f| f.file_path.ends_with("main.rs") && f.reason == ImpactReason::DirectEdit);
        assert!(has_direct, "Should contain direct edit entry for main.rs");
    }

    #[test]
    fn test_impact_analyzer_bfs_traversal() {
        let analyzer = ImpactAnalyzer::new();
        let graph = make_graph();
        let changed = vec![PathBuf::from("src/main.rs")];
        let result = analyzer.analyze(&changed, &graph);

        // Should find helper.rs and utils.rs (transitive)
        let helper_found = result
            .affected_files
            .iter()
            .any(|f| f.file_path.ends_with("helper.rs"));
        let utils_found = result
            .affected_files
            .iter()
            .any(|f| f.file_path.ends_with("utils.rs"));

        assert!(helper_found, "Should find helper.rs via Calls edge");
        assert!(utils_found, "Should find utils.rs via transitive traversal");
    }

    #[test]
    fn test_impact_analyzer_format_report() {
        let analyzer = ImpactAnalyzer::new();
        let graph = make_graph();
        let changed = vec![PathBuf::from("src/main.rs")];
        let result = analyzer.analyze(&changed, &graph);
        let report = analyzer.format_report(&result);

        assert!(report.contains("Impact Analysis Report"));
        assert!(report.contains("Total affected files"));
        assert!(report.contains("main.rs"));
    }

    #[test]
    fn test_impact_analyzer_to_json() {
        let analyzer = ImpactAnalyzer::new();
        let graph = make_graph();
        let changed = vec![PathBuf::from("src/main.rs")];
        let result = analyzer.analyze(&changed, &graph);
        let json = analyzer.to_json(&result);

        assert!(json["total_files"].as_u64().unwrap_or(0) > 0);
        assert!(json["max_depth_reached"].as_u64().is_some());
        assert!(json["affected_files"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_impact_analyzer_with_config() {
        let config = ImpactConfig {
            max_depth: 1,
            include_transitive: false,
            include_tests: false,
            min_confidence: 0.5,
        };
        let analyzer = ImpactAnalyzer::with_config(config);
        let graph = make_graph();
        let changed = vec![PathBuf::from("src/main.rs")];
        let result = analyzer.analyze(&changed, &graph);

        // With max_depth=1 and no transitive, should only find direct + one hop
        assert!(result.total_files >= 1);
        assert!(result.max_depth_reached <= 1);
    }

    #[test]
    fn test_impact_analyzer_no_changed_files() {
        let analyzer = ImpactAnalyzer::new();
        let graph = make_graph();
        let result = analyzer.analyze(&[], &graph);
        assert_eq!(result.total_files, 0);
    }
}