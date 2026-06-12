//! CodeGraph — Knowledge graph & code structure analysis.
//!
//! Inspired by Understand Anything (https://github.com/Lum1104/Understand-Anything)
//! Core patterns absorbed:
//! - Tree-sitter + LLM hybrid analysis
//! - Multi-agent code understanding pipeline
//! - Business domain mapping
//! - Impact analysis (diff ripple effects)

mod domain_mapper;
mod impact_analyzer;

pub use domain_mapper::{DomainMapper, DomainConfig, DomainInfo, DomainLevel};
pub use impact_analyzer::{ImpactAnalyzer, ImpactConfig, ImpactResult, AffectedFile, ImpactReason};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A node in the code knowledge graph.
#[derive(Debug, Clone)]
pub struct CodeNode {
    pub id: String,
    pub name: String,
    pub kind: CodeNodeKind,
    pub file_path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub doc_comment: Option<String>,
    pub visibility: Option<String>, // "pub", "pub(crate)", "private"
}

/// Kinds of code entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeNodeKind {
    Module,    // Rust module
    Struct,
    Enum,
    Trait,
    Impl,
    Function,
    Method,
    Constant,
    TypeAlias,
    Macro,
    Unknown,
}

/// A relationship between two code nodes.
#[derive(Debug, Clone)]
pub struct CodeEdge {
    pub source_id: String,
    pub target_id: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Calls,       // Function calls function
    Implements,  // Struct implements trait
    Extends,     // Trait extends trait
    Contains,    // Module contains function/struct
    Uses,        // Uses as type
    Inherits,    // Enum variant inherits
}

/// The code knowledge graph.
pub struct CodeGraph {
    nodes: HashMap<String, CodeNode>,
    edges: Vec<CodeEdge>,
    /// File→nodes lookup for quick file-based queries
    file_index: HashMap<PathBuf, Vec<String>>,
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            file_index: HashMap::new(),
        }
    }

    /// Add a code node. If a node with the same ID already exists, it is updated.
    pub fn add_node(&mut self, node: CodeNode) {
        let id = node.id.clone();
        let file_path = node.file_path.clone();

        // Update file_index: remove old entry if node existed, then add new one
        if let Some(old_node) = self.nodes.get(&id) {
            if old_node.file_path != file_path {
                if let Some(ids) = self.file_index.get_mut(&old_node.file_path) {
                    ids.retain(|nid| nid != &id);
                    if ids.is_empty() {
                        self.file_index.remove(&old_node.file_path);
                    }
                }
            }
        }

        self.nodes.insert(id.clone(), node);
        self.file_index.entry(file_path).or_default().push(id);
    }

    /// Add a relationship edge.
    pub fn add_edge(&mut self, edge: CodeEdge) {
        self.edges.push(edge);
    }

    /// Get all nodes in a file.
    pub fn nodes_in_file(&self, path: &Path) -> Vec<&CodeNode> {
        self.file_index
            .get(path)
            .map(|ids| ids.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all callers of a function/method (edges where target_id matches node_id).
    pub fn get_callers(&self, node_id: &str) -> Vec<&CodeEdge> {
        self.edges
            .iter()
            .filter(|e| e.target_id == node_id && e.kind == EdgeKind::Calls)
            .collect()
    }

    /// Find nodes matching a name (for quick symbol lookup).
    pub fn find_by_name(&self, name: &str) -> Vec<&CodeNode> {
        self.nodes
            .values()
            .filter(|n| n.name == name)
            .collect()
    }

    /// Generate a DOT graph visualization string.
    pub fn to_dot(&self) -> String {
        let mut dot = String::from("digraph CodeGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=box, style=rounded];\n\n");

        for node in self.nodes.values() {
            let label = match &node.doc_comment {
                Some(doc) => format!("{}\\n{}", node.name, doc.split('\n').next().unwrap_or("")),
                None => node.name.clone(),
            };
            dot.push_str(&format!("  \"{}\" [label=\"{}\"];\n", node.id, label));
        }

        dot.push('\n');
        for edge in &self.edges {
            let label = match edge.kind {
                EdgeKind::Calls => "calls",
                EdgeKind::Implements => "implements",
                EdgeKind::Extends => "extends",
                EdgeKind::Contains => "contains",
                EdgeKind::Uses => "uses",
                EdgeKind::Inherits => "inherits",
            };
            dot.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                edge.source_id, edge.target_id, label
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// Generate a JSON representation for external visualization.
    pub fn to_json(&self) -> serde_json::Value {
        let nodes: Vec<serde_json::Value> = self
            .nodes
            .values()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "kind": format!("{:?}", n.kind),
                    "file_path": n.file_path.to_string_lossy(),
                    "start_line": n.start_line,
                    "end_line": n.end_line,
                    "doc_comment": n.doc_comment,
                    "visibility": n.visibility,
                })
            })
            .collect();

        let edges: Vec<serde_json::Value> = self
            .edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "source_id": e.source_id,
                    "target_id": e.target_id,
                    "kind": format!("{:?}", e.kind),
                })
            })
            .collect();

        serde_json::json!({
            "nodes": nodes,
            "edges": edges,
        })
    }

    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Merge another code graph into this one.
    pub fn merge(&mut self, other: CodeGraph) {
        for (_, node) in other.nodes {
            self.add_node(node);
        }
        for edge in other.edges {
            self.edges.push(edge);
        }
    }

    /// Get all edges (for external use like impact analysis).
    pub fn all_edges(&self) -> &[CodeEdge] {
        &self.edges
    }

    /// Get a node by its ID.
    pub fn get_node(&self, id: &str) -> Option<&CodeNode> {
        self.nodes.get(id)
    }

    /// Get all outgoing edges from a node.
    pub fn outgoing_edges(&self, node_id: &str) -> Vec<&CodeEdge> {
        self.edges.iter().filter(|e| e.source_id == node_id).collect()
    }

    /// Get all incoming edges to a node.
    pub fn incoming_edges(&self, node_id: &str) -> Vec<&CodeEdge> {
        self.edges.iter().filter(|e| e.target_id == node_id).collect()
    }
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

// CodeGraph must support Send + Sync
fn _assert_send_sync()
where
    CodeGraph: Send + Sync,
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_graph_new_empty() {
        let graph = CodeGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert!(graph.to_dot().contains("digraph CodeGraph"));
    }

    #[test]
    fn test_code_graph_add_node() {
        let mut graph = CodeGraph::new();
        let node = CodeNode {
            id: "fn1".into(),
            name: "calculate".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/math.rs"),
            start_line: 10,
            end_line: 25,
            doc_comment: Some("Calculate the result".into()),
            visibility: Some("pub".into()),
        };
        graph.add_node(node);
        assert_eq!(graph.node_count(), 1);

        let nodes = graph.nodes_in_file(Path::new("src/math.rs"));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "calculate");
    }

    #[test]
    fn test_code_graph_add_node_duplicate_id_updates() {
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "fn1".into(),
            name: "original".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/a.rs"),
            start_line: 1,
            end_line: 5,
            doc_comment: None,
            visibility: None,
        });
        graph.add_node(CodeNode {
            id: "fn1".into(),
            name: "updated".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/b.rs"),
            start_line: 10,
            end_line: 20,
            doc_comment: None,
            visibility: None,
        });
        // After update, node_count should still be 1
        assert_eq!(graph.node_count(), 1);
        let nodes = graph.find_by_name("updated");
        assert_eq!(nodes.len(), 1);
        // Old file index entry should be removed
        assert!(graph.nodes_in_file(Path::new("src/a.rs")).is_empty());
        // New file index entry should exist
        assert_eq!(graph.nodes_in_file(Path::new("src/b.rs")).len(), 1);
    }

    #[test]
    fn test_code_graph_add_edge() {
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "caller".into(),
            name: "main".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/main.rs"),
            start_line: 1,
            end_line: 10,
            doc_comment: None,
            visibility: None,
        });
        graph.add_node(CodeNode {
            id: "callee".into(),
            name: "helper".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("src/helper.rs"),
            start_line: 1,
            end_line: 5,
            doc_comment: None,
            visibility: None,
        });
        graph.add_edge(CodeEdge {
            source_id: "caller".into(),
            target_id: "callee".into(),
            kind: EdgeKind::Calls,
        });
        assert_eq!(graph.edge_count(), 1);

        let callers = graph.get_callers("callee");
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].source_id, "caller");
    }

    #[test]
    fn test_code_graph_find_by_name() {
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "id1".into(),
            name: "process".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("lib.rs"),
            start_line: 1,
            end_line: 2,
            doc_comment: None,
            visibility: None,
        });
        graph.add_node(CodeNode {
            id: "id2".into(),
            name: "process".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("mod.rs"),
            start_line: 5,
            end_line: 6,
            doc_comment: None,
            visibility: None,
        });
        graph.add_node(CodeNode {
            id: "id3".into(),
            name: "other".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("other.rs"),
            start_line: 1,
            end_line: 2,
            doc_comment: None,
            visibility: None,
        });

        let found = graph.find_by_name("process");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn test_code_graph_to_dot_basic() {
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "n1".into(),
            name: "Node1".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("f.rs"),
            start_line: 1,
            end_line: 5,
            doc_comment: None,
            visibility: None,
        });
        graph.add_node(CodeNode {
            id: "n2".into(),
            name: "Node2".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("f.rs"),
            start_line: 6,
            end_line: 10,
            doc_comment: None,
            visibility: None,
        });
        graph.add_edge(CodeEdge {
            source_id: "n1".into(),
            target_id: "n2".into(),
            kind: EdgeKind::Calls,
        });

        let dot = graph.to_dot();
        assert!(dot.contains("digraph CodeGraph"));
        assert!(dot.contains("\"n1\" -> \"n2\" [label=\"calls\"]"));
        assert!(dot.contains("\"n1\" [label="));
        assert!(dot.contains("\"n2\" [label="));
    }

    #[test]
    fn test_code_graph_to_json() {
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "n1".into(),
            name: "main".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("main.rs"),
            start_line: 1,
            end_line: 3,
            doc_comment: None,
            visibility: Some("pub".into()),
        });
        graph.add_edge(CodeEdge {
            source_id: "n1".into(),
            target_id: "n2".into(),
            kind: EdgeKind::Calls,
        });

        let json = graph.to_json();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(json["edges"].as_array().unwrap().len(), 1);
        assert_eq!(json["nodes"][0]["name"], "main");
        assert_eq!(json["nodes"][0]["visibility"], "pub");
    }

    #[test]
    fn test_code_graph_merge() {
        let mut graph1 = CodeGraph::new();
        graph1.add_node(CodeNode {
            id: "a".into(),
            name: "alpha".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("a.rs"),
            start_line: 1,
            end_line: 1,
            doc_comment: None,
            visibility: None,
        });

        let mut graph2 = CodeGraph::new();
        graph2.add_node(CodeNode {
            id: "b".into(),
            name: "beta".into(),
            kind: CodeNodeKind::Function,
            file_path: PathBuf::from("b.rs"),
            start_line: 1,
            end_line: 1,
            doc_comment: None,
            visibility: None,
        });
        graph2.add_edge(CodeEdge {
            source_id: "a".into(),
            target_id: "b".into(),
            kind: EdgeKind::Uses,
        });

        graph1.merge(graph2);
        assert_eq!(graph1.node_count(), 2);
        assert_eq!(graph1.edge_count(), 1);
    }
}