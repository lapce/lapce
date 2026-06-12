//! Natural Language Understanding - Parses natural instructions.
//!
//! This module provides:
//! - Intent classification
//! - Entity extraction
//! - Context resolution
//! - Instruction parsing

use std::collections::HashMap;

/// An intent with its confidence.
#[derive(Debug, Clone)]
pub struct Intent {
    pub name: IntentType,
    pub confidence: f32,
    pub parameters: HashMap<String, ParameterValue>,
    pub original_text: String,
}

/// Type of intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentType {
    Generate,
    Refactor,
    Explain,
    Debug,
    Test,
    Document,
    Find,
    Optimize,
    Review,
    Create,
    Modify,
    Delete,
    Unknown,
}

/// A parameter value.
#[derive(Debug, Clone)]
pub enum ParameterValue {
    String(String),
    Number(f64),
    Boolean(bool),
    FilePath(String),
    CodeLanguage(String),
}

/// Extracted entity.
#[derive(Debug, Clone)]
pub struct Entity {
    pub text: String,
    pub entity_type: EntityType,
    pub start: usize,
    pub end: usize,
}

/// Type of entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    FilePath,
    FunctionName,
    ClassName,
    VariableName,
    Language,
    LineNumber,
    CodeSnippet,
    ErrorMessage,
}

/// Parsed instruction.
#[derive(Debug, Clone)]
pub struct ParsedInstruction {
    pub intent: Intent,
    pub entities: Vec<Entity>,
    pub target_files: Vec<String>,
    pub constraints: Vec<String>,
    pub original: String,
    pub cleaned: String,
}

/// Natural language understanding engine.
pub struct NluEngine {
    intent_patterns: HashMap<IntentType, Vec<IntentPattern>>,
}

struct IntentPattern {
    keywords: Vec<String>,
    weight: f32,
}

impl NluEngine {
    pub fn new() -> Self {
        Self {
            intent_patterns: Self::default_intent_patterns(),
        }
    }

    /// Default intent patterns.
    fn default_intent_patterns() -> HashMap<IntentType, Vec<IntentPattern>> {
        let mut patterns = HashMap::new();

        patterns.insert(IntentType::Generate, vec![
            IntentPattern { keywords: vec!["generate".into(), "write".into(), "create".into(), "implement".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["add".into(), "make".into(), "build".into()], weight: 0.7 },
        ]);

        patterns.insert(IntentType::Refactor, vec![
            IntentPattern { keywords: vec!["refactor".into(), "restructure".into(), "reorganize".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["extract".into(), "inline".into(), "rename".into(), "move".into()], weight: 0.8 },
        ]);

        patterns.insert(IntentType::Explain, vec![
            IntentPattern { keywords: vec!["explain".into(), "describe".into(), "what does".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["how does".into(), "what is".into(), "tell me about".into()], weight: 0.8 },
        ]);

        patterns.insert(IntentType::Debug, vec![
            IntentPattern { keywords: vec!["debug".into(), "fix".into(), "error".into(), "bug".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["crash".into(), "fails".into(), "issue".into(), "problem".into()], weight: 0.7 },
        ]);

        patterns.insert(IntentType::Test, vec![
            IntentPattern { keywords: vec!["test".into(), "spec".into(), "specify".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["write tests".into(), "add tests".into()], weight: 0.8 },
        ]);

        patterns.insert(IntentType::Document, vec![
            IntentPattern { keywords: vec!["document".into(), "doc".into(), "comment".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["add comments".into(), "explain".into(), "readme".into()], weight: 0.7 },
        ]);

        patterns.insert(IntentType::Find, vec![
            IntentPattern { keywords: vec!["find".into(), "search".into(), "locate".into(), "where is".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["look for".into(), "get".into(), "show me".into()], weight: 0.7 },
        ]);

        patterns.insert(IntentType::Optimize, vec![
            IntentPattern { keywords: vec!["optimize".into(), "improve".into(), "performance".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["speed up".into(), "faster".into(), "efficient".into()], weight: 0.8 },
        ]);

        patterns.insert(IntentType::Review, vec![
            IntentPattern { keywords: vec!["review".into(), "analyze".into(), "check".into()], weight: 0.9 },
            IntentPattern { keywords: vec!["look at".into(), "examine".into(), "audit".into()], weight: 0.7 },
        ]);

        patterns.insert(IntentType::Create, vec![
            IntentPattern { keywords: vec!["create".into(), "new".into(), "initialize".into()], weight: 0.9 },
        ]);

        patterns.insert(IntentType::Modify, vec![
            IntentPattern { keywords: vec!["modify".into(), "change".into(), "update".into(), "edit".into()], weight: 0.9 },
        ]);

        patterns.insert(IntentType::Delete, vec![
            IntentPattern { keywords: vec!["delete".into(), "remove".into(), "clean up".into()], weight: 0.9 },
        ]);

        patterns
    }

    /// Parse a natural language instruction.
    pub fn parse(&self, text: &str) -> ParsedInstruction {
        let text_lower = text.to_lowercase();
        let intent = self.classify_intent(&text_lower, text);
        let entities = self.extract_entities(text);

        // Extract target files
        let target_files = self.extract_file_paths(text);

        // Extract constraints
        let constraints = self.extract_constraints(&text_lower);

        // Clean the text
        let cleaned = self.clean_instruction(text);

        ParsedInstruction {
            intent,
            entities,
            target_files,
            constraints,
            original: text.to_string(),
            cleaned,
        }
    }

    /// Classify intent from text.
    fn classify_intent(&self, text_lower: &str, original: &str) -> Intent {
        let mut best_intent = IntentType::Unknown;
        let mut best_score = 0.0_f32;
        

        for (intent_type, patterns) in &self.intent_patterns {
            for pattern in patterns {
                let matches = pattern.keywords.iter()
                    .filter(|kw| text_lower.contains(&kw.to_lowercase()))
                    .count();

                if matches > 0 {
                    let score = pattern.weight * (matches as f32 / pattern.keywords.len() as f32);

                    if score > best_score {
                        best_score = score;
                        best_intent = *intent_type;
                    }
                }
            }
        }

        // Extract parameters based on intent
        let parameters = self.extract_parameters(best_intent, original);

        // Normalize confidence
        let confidence = best_score.min(1.0).max(0.0);

        Intent {
            name: best_intent,
            confidence,
            parameters,
            original_text: original.to_string(),
        }
    }

    /// Extract parameters based on intent.
    fn extract_parameters(&self, intent: IntentType, text: &str) -> HashMap<String, ParameterValue> {
        let mut params = HashMap::new();

        match intent {
            IntentType::Generate | IntentType::Modify => {
                // Extract language if mentioned
                let languages = ["rust", "python", "javascript", "typescript", "go", "java", "c++", "cpp"];
                for lang in languages {
                    if text.to_lowercase().contains(lang) {
                        params.insert("language".to_string(), ParameterValue::CodeLanguage(lang.to_string()));
                        break;
                    }
                }
            }
            IntentType::Debug
                // Extract error patterns
                if (text.contains("error") || text.contains("Error") || text.contains("ERROR")) => {
                    params.insert("has_error".to_string(), ParameterValue::Boolean(true));
                }
            _ => {}
        }

        params
    }

    /// Extract entities from text.
    fn extract_entities(&self, text: &str) -> Vec<Entity> {
        let mut entities = Vec::new();

        // Extract file paths
        let file_patterns = [
            r#"\w+\.\w{1,10}"#,  // Basic file.extension
            r#"(?:src|lib|bin)/[\w./]+"#,  // Path with common dirs
        ];

        for pattern in &file_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                for mat in re.find_iter(text) {
                    let text_str = mat.as_str().to_string();
                    if self.looks_like_file_path(&text_str) {
                        entities.push(Entity {
                            text: text_str,
                            entity_type: EntityType::FilePath,
                            start: mat.start(),
                            end: mat.end(),
                        });
                    }
                }
            }
        }

        // Extract function names (snake_case or camelCase followed by parenthesis)
        let func_pattern = regex::Regex::new(r#"\b([a-z][a-z0-9_]*|[A-Z][a-zA-Z0-9]*)\s*\("#).expect("unwrap failed: nlu_engine.rs:278");
        for cap in func_pattern.captures_iter(text) {
            if let Some(name) = cap.get(1) {
                entities.push(Entity {
                    text: name.as_str().to_string(),
                    entity_type: EntityType::FunctionName,
                    start: name.start(),
                    end: name.end(),
                });
            }
        }

        // Extract code snippets (content in backticks)
        let code_pattern = regex::Regex::new(r#"`([^`]+)`"#).expect("unwrap failed: nlu_engine.rs:291");
        for cap in code_pattern.captures_iter(text) {
            if let Some(code) = cap.get(1) {
                entities.push(Entity {
                    text: code.as_str().to_string(),
                    entity_type: EntityType::CodeSnippet,
                    start: code.start(),
                    end: code.end(),
                });
            }
        }

        entities
    }

    /// Check if text looks like a file path.
    fn looks_like_file_path(&self, text: &str) -> bool {
        let extensions = [".rs", ".py", ".js", ".ts", ".tsx", ".go", ".java", ".cpp", ".c", ".h", ".hpp", ".json", ".toml", ".yaml", ".yml", ".md", ".txt"];
        extensions.iter().any(|ext| text.ends_with(ext)) || text.contains('/') || text.contains('\\')
    }

    /// Extract file paths from text.
    fn extract_file_paths(&self, text: &str) -> Vec<String> {
        let entities = self.extract_entities(text);
        entities.iter()
            .filter(|e| e.entity_type == EntityType::FilePath)
            .map(|e| e.text.clone())
            .collect()
    }

    /// Extract constraints from text.
    fn extract_constraints(&self, text_lower: &str) -> Vec<String> {
        let mut constraints = Vec::new();

        let constraint_patterns = [
            ("performance", vec!["fast", "efficient", "optimized"]),
            ("simplicity", vec!["simple", "minimal", "clean"]),
            ("safety", vec!["safe", "secure", "no panic"]),
            ("compatibility", vec!["compatible", "works with", "supports"]),
        ];

        for (name, keywords) in constraint_patterns {
            if keywords.iter().any(|kw| text_lower.contains(kw)) {
                constraints.push(name.to_string());
            }
        }

        constraints
    }

    /// Clean instruction text.
    fn clean_instruction(&self, text: &str) -> String {
        let mut cleaned = text.to_string();

        // Remove common filler words
        let fillers = ["please", "could you", "would you", "can you", "i want to", "i need to"];
        for filler in fillers {
            cleaned = cleaned.replace(filler, "");
        }

        // Normalize whitespace
        cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");

        cleaned.trim().to_string()
    }

    /// Resolve pronouns and references.
    pub fn resolve_references(&self, text: &str, _context: &str) -> String {
        let mut resolved = text.to_string();

        // Simple reference resolution
        let references = [
            ("it", "the previous code"),
            ("that", "the mentioned item"),
            ("this", "the current item"),
        ];

        for (pronoun, replacement) in references {
            resolved = resolved.replace(pronoun, replacement);
        }

        resolved
    }

    /// Expand abbreviations.
    pub fn expand_abbreviations(&self, text: &str) -> String {
        let abbreviations = [
            ("impl", "implement"),
            ("fn", "function"),
            ("cls", "class"),
            ("msg", "message"),
            ("err", "error"),
            ("usr", "user"),
            ("config", "configuration"),
            ("opt", "option"),
            ("spec", "specification"),
        ];

        let mut expanded = text.to_string();
        for (abbr, full) in abbreviations {
            let pattern = regex::Regex::new(&format!(r"\b{}\b", abbr)).expect("unwrap failed: nlu_engine.rs:391");
            expanded = pattern.replace_all(&expanded, full).to_string();
        }

        expanded
    }
}

impl Default for NluEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_generate() {
        let engine = NluEngine::new();
        let result = engine.parse("generate a function to add two numbers in rust");

        assert_eq!(result.intent.name, IntentType::Generate);
        assert!(result.intent.confidence > 0.5);
    }

    #[test]
    fn test_parse_refactor() {
        let engine = NluEngine::new();
        let result = engine.parse("refactor this code to use a better pattern");

        assert_eq!(result.intent.name, IntentType::Refactor);
    }

    #[test]
    fn test_parse_debug() {
        let engine = NluEngine::new();
        let result = engine.parse("debug this error: null pointer exception");

        assert_eq!(result.intent.name, IntentType::Debug);
    }

    #[test]
    fn test_extract_file_paths() {
        let engine = NluEngine::new();
        let result = engine.parse("look at src/main.rs and lib/utils.rs");

        assert_eq!(result.target_files.len(), 2);
        assert!(result.target_files.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn test_clean_instruction() {
        let engine = NluEngine::new();
        let result = engine.parse("please could you generate a function");

        assert!(!result.cleaned.contains("please"));
        assert!(!result.cleaned.contains("could you"));
    }

    #[test]
    fn test_expand_abbreviations() {
        let engine = NluEngine::new();
        let expanded = engine.expand_abbreviations("impl a fn for usr");

        assert!(expanded.contains("implement"));
        assert!(expanded.contains("function"));
        assert!(expanded.contains("user"));
    }
}
