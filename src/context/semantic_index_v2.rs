//! Semantic Index V2 - Enhanced Code Understanding
//!
//! Based on Claude Code's code indexing implementation, this module provides:
//! - Real-time symbol indexing
//! - Cross-reference tracking
//! - Dependency graph
//! - Semantic search with ranking
//! - Code context extraction

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use regex::Regex;

/// Symbol information
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: CodeRange,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub type_info: Option<String>,
    pub references: Vec<Reference>,
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Class,
    Function,
    Method,
    Property,
    Field,
    Variable,
    Constant,
    Enum,
    Interface,
    Trait,
    Struct,
    Module,
    Type,
    Import,
    Parameter,
    Keyword,
    Impl,
}

impl SymbolKind {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "class" | "classdecl" => SymbolKind::Class,
            "function" | "func" => SymbolKind::Function,
            "method" | "instmethod" | "staticmethod" => SymbolKind::Method,
            "property" | "prop" => SymbolKind::Property,
            "field" => SymbolKind::Field,
            "variable" | "var" | "localvar" | "param" => SymbolKind::Variable,
            "constant" | "const" => SymbolKind::Constant,
            "enum" | "enumitem" => SymbolKind::Enum,
            "interface" => SymbolKind::Interface,
            "trait" => SymbolKind::Trait,
            "struct" | "structfield" => SymbolKind::Struct,
            "module" | "namespace" => SymbolKind::Module,
            "type" | "typedef" => SymbolKind::Type,
            "import" | "use" | "require" => SymbolKind::Import,
            _ => SymbolKind::Variable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodeRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl CodeRange {
    pub fn contains(&self, line: usize, col: usize) -> bool {
        if line < self.start_line || line > self.end_line {
            return false;
        }
        if line == self.start_line && col < self.start_col {
            return false;
        }
        if line == self.end_line && col > self.end_col {
            return false;
        }
        true
    }

    pub fn overlaps(&self, other: &CodeRange) -> bool {
        if self.end_line < other.start_line || self.start_line > other.end_line {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct Reference {
    pub uri: String,
    pub range: CodeRange,
    pub context: String,
    pub is_definition: bool,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub uri: String,
    pub range: CodeRange,
    pub symbol_id: String,
}

/// Dependency information
#[derive(Debug, Clone)]
pub struct Dependency {
    pub source: String,
    pub target: String,
    pub kind: DependencyKind,
    pub is_external: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum DependencyKind {
    Import,
    Use,
    Require,
    Include,
    Extend,
    Implement,
    Call,
    Reference,
}

impl DependencyKind {
    pub fn from_import_style(lang: &str) -> Self {
        match lang {
            "rust" => DependencyKind::Use,
            "python" => DependencyKind::Import,
            "javascript" | "typescript" => DependencyKind::Require,
            "go" => DependencyKind::Import,
            _ => DependencyKind::Import,
        }
    }
}

/// Code context for AI understanding
#[derive(Debug, Clone)]
pub struct CodeContext {
    pub current_file: String,
    pub current_symbol: Option<SymbolInfo>,
    pub related_symbols: Vec<SymbolInfo>,
    pub dependencies: Vec<Dependency>,
    pub call_chain: Vec<SymbolInfo>,
    pub imports: Vec<String>,
    pub exports: Vec<String>,
}

/// Enhanced semantic index
pub struct SemanticIndexV2 {
    symbols: Arc<RwLock<HashMap<String, SymbolInfo>>>,
    uri_to_symbols: Arc<RwLock<HashMap<String, Vec<String>>>>,
    name_to_symbols: Arc<RwLock<HashMap<String, Vec<String>>>>,
    dependencies: Arc<RwLock<Vec<Dependency>>>,
    uri_to_dependencies: Arc<RwLock<HashMap<String, Vec<String>>>>,
    recent_files: Arc<RwLock<VecDeque<String>>>,
    config: IndexConfig,
}

#[derive(Debug, Clone)]
pub struct IndexConfig {
    pub max_recent_files: usize,
    pub enable_dependency_tracking: bool,
    pub enable_cross_reference: bool,
    pub enable_semantic_search: bool,
    pub indexing_workers: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_recent_files: 100,
            enable_dependency_tracking: true,
            enable_cross_reference: true,
            enable_semantic_search: true,
            indexing_workers: 4,
        }
    }
}

impl SemanticIndexV2 {
    pub fn new(config: IndexConfig) -> Self {
        Self {
            symbols: Arc::new(RwLock::new(HashMap::new())),
            uri_to_symbols: Arc::new(RwLock::new(HashMap::new())),
            name_to_symbols: Arc::new(RwLock::new(HashMap::new())),
            dependencies: Arc::new(RwLock::new(Vec::new())),
            uri_to_dependencies: Arc::new(RwLock::new(HashMap::new())),
            recent_files: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
            config,
        }
    }

    /// Index a file and extract symbols
    pub async fn index_file(&self, uri: &str, content: &str, language: &str) -> Vec<SymbolInfo> {
        let symbols = self.extract_symbols(uri, content, language);
        
        // Store symbols
        {
            let mut symbol_map = self.symbols.write().await;
            let mut uri_map = self.uri_to_symbols.write().await;
            let mut name_map = self.name_to_symbols.write().await;
            
            // Remove old symbols for this URI
            if let Some(old_ids) = uri_map.remove(uri) {
                for id in &old_ids {
                    if let Some(old_sym) = symbol_map.remove(id) {
                        // Remove from name index
                        if let Some(name_refs) = name_map.get_mut(&old_sym.name) {
                            name_refs.retain(|r| r != id);
                        }
                    }
                }
            }

            // Add new symbols
            let mut symbol_ids = Vec::new();
            for symbol in &symbols {
                symbol_ids.push(symbol.id.clone());
                symbol_map.insert(symbol.id.clone(), symbol.clone());
                
                // Index by name
                name_map
                    .entry(symbol.name.clone())
                    .or_insert_with(Vec::new)
                    .push(symbol.id.clone());
            }
            uri_map.insert(uri.to_string(), symbol_ids);
        }

        // Update recent files
        {
            let mut recent = self.recent_files.write().await;
            recent.retain(|r| r != uri);
            recent.push_front(uri.to_string());
            while recent.len() > self.config.max_recent_files {
                recent.pop_back();
            }
        }

        // Track dependencies if enabled
        if self.config.enable_dependency_tracking {
            self.index_dependencies(uri, content, language).await;
        }

        symbols
    }

    /// Extract symbols from code
    fn extract_symbols(&self, uri: &str, content: &str, language: &str) -> Vec<SymbolInfo> {
        let _symbols: Vec<SymbolInfo> = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        match language {
            "rust" => self.extract_rust_symbols(uri, &lines),
            "typescript" | "javascript" => self.extract_typescript_symbols(uri, &lines),
            "python" => self.extract_python_symbols(uri, &lines),
            "go" => self.extract_go_symbols(uri, &lines),
            _ => self.extract_generic_symbols(uri, &lines),
        }
    }

    /// Extract Rust symbols
    fn extract_rust_symbols(&self, uri: &str, lines: &[&str]) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        let mut in_comment = false;
        let mut _brace_depth = 0;
        let mut _current_struct: Option<String> = None;

        let patterns: Vec<(Regex, SymbolKind, bool)> = vec![
            (Regex::new(r"^\s*pub\s+struct\s+(\w+)").expect("invalid rust struct regex"), SymbolKind::Struct, false),
            (Regex::new(r"^\s*pub\s+enum\s+(\w+)").expect("invalid rust enum regex"), SymbolKind::Enum, false),
            (Regex::new(r"^\s*pub\s+trait\s+(\w+)").expect("invalid rust trait regex"), SymbolKind::Trait, false),
            (Regex::new(r"^\s*impl(?:\s+[\w\s,&]*)?\s+for\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:288"), SymbolKind::Impl, false),
            (Regex::new(r"^\s*pub\s+fn\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:289"), SymbolKind::Function, false),
            (Regex::new(r"^\s*fn\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:290"), SymbolKind::Function, false),
            (Regex::new(r"^\s*pub\s+const\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:291"), SymbolKind::Constant, false),
            (Regex::new(r"^\s*const\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:292"), SymbolKind::Constant, false),
            (Regex::new(r"^\s*pub\s+type\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:293"), SymbolKind::Type, false),
            (Regex::new(r"^\s*type\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:294"), SymbolKind::Type, false),
            (Regex::new(r"^\s*pub\s+mod\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:295"), SymbolKind::Module, false),
            (Regex::new(r"^\s*mod\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:296"), SymbolKind::Module, false),
            (Regex::new(r"^\s*use\s+([\w:]+)").expect("unwrap failed: semantic_index_v2.rs:297"), SymbolKind::Import, false),
        ];

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Handle comments
            if trimmed.starts_with("/*") {
                in_comment = true;
            }
            if in_comment {
                if trimmed.contains("*/") {
                    in_comment = false;
                }
                continue;
            }
            if trimmed.starts_with("//") {
                continue;
            }

            // Track brace depth (reserved for nested scope analysis)
            _brace_depth += line.matches('{').count() as i32;
            _brace_depth -= line.matches('}').count() as i32;

            // Extract symbols
            for (pattern, kind, _) in &patterns {
                if let Some(caps) = pattern.captures(trimmed) {
                    if let Some(name) = caps.get(1) {
                        let id = format!("{}::{}:{}", uri, name.as_str(), line_num);
                        let range = CodeRange {
                            start_line: line_num,
                            start_col: line.len() - line.trim_start().len(),
                            end_line: line_num,
                            end_col: line.len(),
                        };

                        let symbol = SymbolInfo {
                            id,
                            name: name.as_str().to_string(),
                            kind: *kind,
                            uri: uri.to_string(),
                            range,
                            signature: Some(self.extract_signature(trimmed)),
                            documentation: self.extract_doc_comment(lines, line_num),
                            type_info: self.extract_type_info(trimmed, kind),
                            references: vec![],
                            definitions: vec![],
                        };
                        symbols.push(symbol);

                        // Track current struct for method association (reserved for future use)
                        _current_struct = Some(name.as_str().to_string());
                    }
                }
            }
        }

        symbols
    }

    /// Extract TypeScript/JavaScript symbols
    fn extract_typescript_symbols(&self, uri: &str, lines: &[&str]) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();

        let patterns: Vec<(Regex, SymbolKind)> = vec![
            (Regex::new(r"export\s+class\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:362"), SymbolKind::Class),
            (Regex::new(r"class\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:363"), SymbolKind::Class),
            (Regex::new(r"export\s+interface\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:364"), SymbolKind::Interface),
            (Regex::new(r"interface\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:365"), SymbolKind::Interface),
            (Regex::new(r"export\s+function\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:366"), SymbolKind::Function),
            (Regex::new(r"function\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:367"), SymbolKind::Function),
            (Regex::new(r"export\s+const\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:368"), SymbolKind::Constant),
            (Regex::new(r"const\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:369"), SymbolKind::Constant),
            (Regex::new(r"export\s+let\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:370"), SymbolKind::Variable),
            (Regex::new(r"let\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:371"), SymbolKind::Variable),
            (Regex::new(r"export\s+enum\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:372"), SymbolKind::Enum),
            (Regex::new(r"enum\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:373"), SymbolKind::Enum),
            (Regex::new(r#"import\s+.*from\s+['"]([^'"]+)['"]"#).expect("unwrap failed: semantic_index_v2.rs:374"), SymbolKind::Import),
        ];

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }

            for (pattern, kind) in &patterns {
                if let Some(caps) = pattern.captures(trimmed) {
                    if let Some(name) = caps.get(1) {
                        let id = format!("{}::{}:{}", uri, name.as_str(), line_num);
                        let range = CodeRange {
                            start_line: line_num,
                            start_col: line.len() - line.trim_start().len(),
                            end_line: line_num,
                            end_col: line.len(),
                        };

                        symbols.push(SymbolInfo {
                            id,
                            name: name.as_str().to_string(),
                            kind: *kind,
                            uri: uri.to_string(),
                            range,
                            signature: Some(self.extract_signature(trimmed)),
                            documentation: self.extract_doc_comment(lines, line_num),
                            type_info: None,
                            references: vec![],
                            definitions: vec![],
                        });
                    }
                }
            }
        }

        symbols
    }

    /// Extract Python symbols
    fn extract_python_symbols(&self, uri: &str, lines: &[&str]) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        let mut indent_stack: Vec<usize> = vec![0];

        let patterns: Vec<(Regex, SymbolKind)> = vec![
            (Regex::new(r"^class\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:421"), SymbolKind::Class),
            (Regex::new(r"^def\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:422"), SymbolKind::Function),
            (Regex::new(r"^async\s+def\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:423"), SymbolKind::Function),
            (Regex::new(r"^(\w+)\s*=").expect("unwrap failed: semantic_index_v2.rs:424"), SymbolKind::Constant),
            (Regex::new(r"^import\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:425"), SymbolKind::Import),
            (Regex::new(r"^from\s+(\S+)").expect("unwrap failed: semantic_index_v2.rs:426"), SymbolKind::Import),
        ];

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            if trimmed.starts_with("#") || trimmed.starts_with("\"\"\"") {
                continue;
            }

            // Track indentation
            let indent = line.len() - line.trim_start().len();
            while !indent_stack.is_empty() && indent <= indent_stack[indent_stack.len() - 1] {
                indent_stack.pop();
            }

            for (pattern, kind) in &patterns {
                if let Some(caps) = pattern.captures(trimmed) {
                    if let Some(name) = caps.get(1) {
                        let id = format!("{}::{}:{}", uri, name.as_str(), line_num);
                        let range = CodeRange {
                            start_line: line_num,
                            start_col: indent,
                            end_line: line_num,
                            end_col: line.len(),
                        };

                        symbols.push(SymbolInfo {
                            id,
                            name: name.as_str().to_string(),
                            kind: *kind,
                            uri: uri.to_string(),
                            range,
                            signature: Some(self.extract_signature(trimmed)),
                            documentation: self.extract_doc_comment(lines, line_num),
                            type_info: None,
                            references: vec![],
                            definitions: vec![],
                        });

                        // Track for nested symbols
                        if matches!(kind, SymbolKind::Class | SymbolKind::Function) {
                            indent_stack.push(indent + 4);
                        }
                    }
                }
            }
        }

        symbols
    }

    /// Extract Go symbols
    fn extract_go_symbols(&self, uri: &str, lines: &[&str]) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();

        let patterns: Vec<(Regex, SymbolKind)> = vec![
            (Regex::new(r"^type\s+(\w+)\s+struct").expect("unwrap failed: semantic_index_v2.rs:483"), SymbolKind::Struct),
            (Regex::new(r"^type\s+(\w+)\s+interface").expect("unwrap failed: semantic_index_v2.rs:484"), SymbolKind::Interface),
            (Regex::new(r"^type\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:485"), SymbolKind::Type),
            (Regex::new(r"^func\s+(\w+)\s*\(").expect("unwrap failed: semantic_index_v2.rs:486"), SymbolKind::Function),
            (Regex::new(r"^func\s+\((\w+)\s+\*?\w+\)\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:487"), SymbolKind::Method),
            (Regex::new(r"^const\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:488"), SymbolKind::Constant),
            (Regex::new(r"^import\s+\(").expect("invalid regex: semantic_index_v2.rs:489"), SymbolKind::Import),
            (Regex::new(r#"^\s+"([^"]+)""#).expect("unwrap failed: semantic_index_v2.rs:490"), SymbolKind::Import),
        ];

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }

            for (pattern, kind) in &patterns {
                if let Some(caps) = pattern.captures(trimmed) {
                    let name_idx = if matches!(kind, SymbolKind::Method) { 2 } else { 1 };
                    if let Some(name) = caps.get(name_idx) {
                        let id = format!("{}::{}:{}", uri, name.as_str(), line_num);
                        let range = CodeRange {
                            start_line: line_num,
                            start_col: line.len() - line.trim_start().len(),
                            end_line: line_num,
                            end_col: line.len(),
                        };

                        symbols.push(SymbolInfo {
                            id,
                            name: name.as_str().to_string(),
                            kind: *kind,
                            uri: uri.to_string(),
                            range,
                            signature: Some(self.extract_signature(trimmed)),
                            documentation: self.extract_doc_comment(lines, line_num),
                            type_info: None,
                            references: vec![],
                            definitions: vec![],
                        });
                    }
                }
            }
        }

        symbols
    }

    /// Generic symbol extraction
    fn extract_generic_symbols(&self, uri: &str, lines: &[&str]) -> Vec<SymbolInfo> {
        lines
            .iter()
            .enumerate()
            .filter_map(|(line_num, line)| {
                let trimmed = line.trim();
                if trimmed.starts_with("//") || trimmed.starts_with("#") {
                    return None;
                }

                // Try to find function-like patterns
                if let Some(re) = Regex::new(r"\b(\w+)\s*\([^)]*\)\s*[{:=]").expect("unwrap failed: semantic_index_v2.rs:544").captures(trimmed) {
                    if let Some(name) = re.get(1) {
                        let kind = if name.as_str().chars().next().is_some_and(|c| c.is_uppercase()) {
                            SymbolKind::Class
                        } else {
                            SymbolKind::Function
                        };

                        Some(SymbolInfo {
                            id: format!("{}::{}:{}", uri, name.as_str(), line_num),
                            name: name.as_str().to_string(),
                            kind,
                            uri: uri.to_string(),
                            range: CodeRange {
                                start_line: line_num,
                                start_col: line.len() - line.trim_start().len(),
                                end_line: line_num,
                                end_col: line.len(),
                            },
                            signature: Some(trimmed.to_string()),
                            documentation: None,
                            type_info: None,
                            references: vec![],
                            definitions: vec![],
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    /// Extract function/class signature
    fn extract_signature(&self, line: &str) -> String {
        line.to_string()
    }

    /// Extract doc comment
    fn extract_doc_comment(&self, lines: &[&str], line_num: usize) -> Option<String> {
        if line_num == 0 {
            return None;
        }

        let prev_line = lines.get(line_num - 1)?;
        let trimmed = prev_line.trim();

        if trimmed.starts_with("///") || trimmed.starts_with("/**") || trimmed.starts_with("\"\"\"") {
            Some(trimmed.trim_start_matches(['/', '"', '*']).to_string())
        } else {
            None
        }
    }

    /// Extract type information
    fn extract_type_info(&self, line: &str, _kind: &SymbolKind) -> Option<String> {
        let patterns: Vec<&str> = vec![
            r"->\s*([A-Za-z]\w*)",
            r":\s*([A-Za-z]\w*)",
            r"type\s+\w+\s*=\s*([A-Za-z]\w*)",
        ];

        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(line) {
                    if let Some(typ) = caps.get(1) {
                        return Some(typ.as_str().to_string());
                    }
                }
            }
        }

        None
    }

    /// Index dependencies
    async fn index_dependencies(&self, uri: &str, content: &str, language: &str) {
        let imports = self.extract_imports(content, language);
        
        let mut deps = Vec::new();
        for import in &imports {
            deps.push(Dependency {
                source: uri.to_string(),
                target: import.clone(),
                kind: DependencyKind::from_import_style(language),
                is_external: self.is_external_dependency(import),
            });
        }

        // Update dependency index
        {
            let mut dep_list = self.dependencies.write().await;
            dep_list.extend(deps.clone());

            let mut uri_deps = self.uri_to_dependencies.write().await;
            uri_deps.insert(uri.to_string(), imports);
        }
    }

    /// Extract imports
    fn extract_imports(&self, content: &str, language: &str) -> Vec<String> {
        let mut imports = Vec::new();
        let patterns: Vec<Regex> = match language {
            "rust" => vec![
                Regex::new(r"use\s+([\w:]+)").expect("unwrap failed: semantic_index_v2.rs:650"),
                Regex::new(r"extern\s+crate\s+(\w+)").expect("unwrap failed: semantic_index_v2.rs:651"),
            ],
            "typescript" | "javascript" => vec![
                Regex::new(r#"import\s+.*from\s+['"]([^'"]+)['"]"#).expect("unwrap failed: semantic_index_v2.rs:654"),
                Regex::new(r#"require\(['"]([^'"]+)['"]\)"#).expect("unwrap failed: semantic_index_v2.rs:655"),
            ],
            "python" => vec![
                Regex::new(r"^from\s+([\w.]+)").expect("unwrap failed: semantic_index_v2.rs:658"),
                Regex::new(r"^import\s+([\w.]+)").expect("unwrap failed: semantic_index_v2.rs:659"),
            ],
            "go" => vec![
                Regex::new(r#"import\s+"([^"]+)""#).expect("unwrap failed: semantic_index_v2.rs:662"),
            ],
            _ => vec![],
        };

        for line in content.lines() {
            for pattern in &patterns {
                if let Some(caps) = pattern.captures(line) {
                    if let Some(m) = caps.get(1) {
                        imports.push(m.as_str().to_string());
                    }
                }
            }
        }

        imports
    }

    /// Check if dependency is external
    fn is_external_dependency(&self, import: &str) -> bool {
        let external_prefixes = ["npm:", "pip:", "cargo:", "go:", "@", "crate::"];
        external_prefixes.iter().any(|p| import.starts_with(p))
    }

    /// Search symbols by name with ranking
    pub async fn search_symbols(&self, query: &str) -> Vec<(SymbolInfo, f32)> {
        let symbols = self.symbols.read().await;
        let query_lower = query.to_lowercase();
        
        let mut results: Vec<(SymbolInfo, f32)> = symbols
            .values()
            .filter_map(|symbol| {
                let score = self.calculate_match_score(&symbol.name, &query_lower);
                if score > 0.0 {
                    Some((symbol.clone(), score))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        results
    }

    /// Calculate match score for fuzzy search
    fn calculate_match_score(&self, name: &str, query: &str) -> f32 {
        let name_lower = name.to_lowercase();
        
        if name_lower == query {
            return 100.0;  // Exact match
        }
        
        if name_lower.starts_with(query) {
            return 80.0 + (query.len() as f32 / name_lower.len() as f32) * 10.0;
        }
        
        if name_lower.contains(query) {
            return 50.0 + (query.len() as f32 / name_lower.len() as f32) * 20.0;
        }

        // Fuzzy matching
        let mut score = 0.0;
        let mut query_idx = 0;
        let mut consecutive = 0;

        for c in name_lower.chars() {
            if query_idx < query.len() && Some(c) == query.chars().nth(query_idx) {
                score += 10.0;
                consecutive += 1;
                score += consecutive as f32 * 2.0;
                query_idx += 1;
            } else {
                consecutive = 0;
            }
        }

        if query_idx < query.len() {
            return 0.0;  // Not all query chars matched
        }

        score
    }

    /// Get symbol at position
    pub async fn get_symbol_at(&self, uri: &str, line: usize, col: usize) -> Option<SymbolInfo> {
        let symbols = self.symbols.read().await;
        let uri_symbols = self.uri_to_symbols.read().await;
        
        if let Some(ids) = uri_symbols.get(uri) {
            for id in ids {
                if let Some(symbol) = symbols.get(id) {
                    if symbol.range.contains(line, col) {
                        return Some(symbol.clone());
                    }
                }
            }
        }
        
        None
    }

    /// Get related symbols
    pub async fn get_related_symbols(&self, symbol_id: &str) -> Vec<SymbolInfo> {
        let symbols = self.symbols.read().await;
        
        if let Some(symbol) = symbols.get(symbol_id) {
            let mut related = Vec::new();
            
            // Find symbols with same name
            if let Some(name_symbols) = self.name_to_symbols.read().await.get(&symbol.name) {
                for id in name_symbols {
                    if let Some(s) = symbols.get(id) {
                        if s.id != symbol_id {
                            related.push(s.clone());
                        }
                    }
                }
            }
            
            related
        } else {
            vec![]
        }
    }

    /// Get code context for AI
    pub async fn get_code_context(&self, uri: &str, line: usize, col: usize) -> CodeContext {
        let current_symbol = self.get_symbol_at(uri, line, col).await;
        let related_symbols = if let Some(ref sym) = current_symbol {
            self.get_related_symbols(&sym.id).await
        } else {
            vec![]
        };
        
        let dependencies = {
            let uri_deps = self.uri_to_dependencies.read().await;
            uri_deps.get(uri).cloned().unwrap_or_default()
        };

        CodeContext {
            current_file: uri.to_string(),
            current_symbol,
            related_symbols,
            dependencies: vec![],
            call_chain: vec![],
            imports: dependencies,
            exports: vec![],
        }
    }

    /// Clear index for a file
    pub async fn clear_file(&self, uri: &str) {
        let mut symbol_map = self.symbols.write().await;
        let mut uri_map = self.uri_to_symbols.write().await;
        
        if let Some(ids) = uri_map.remove(uri) {
            for id in ids {
                if let Some(sym) = symbol_map.remove(&id) {
                    let mut name_map = self.name_to_symbols.write().await;
                    if let Some(name_refs) = name_map.get_mut(&sym.name) {
                        name_refs.retain(|r| r != &id);
                    }
                }
            }
        }
        
        self.uri_to_dependencies.write().await.remove(uri);
    }
}

// Note: SymbolKind::Impl should be added in the enum definition itself
