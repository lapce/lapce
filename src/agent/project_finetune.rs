//! Project-specific Fine-tuning - LoRA fine-tuning for project-specific code.
//!
//! This module provides:
//! - Code sample collection
//! - Dataset generation
//! - LoRA config management
//! - Training pipeline setup

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A code sample for fine-tuning.
#[derive(Debug, Clone)]
pub struct CodeSample {
    pub id: String,
    pub input: String,
    pub output: String,
    pub language: String,
    pub file_path: String,
    pub category: SampleCategory,
    pub quality_score: f32,
}

/// Category of code sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleCategory {
    Function,
    Class,
    Test,
    Refactor,
    BugFix,
    Documentation,
    Config,
}

/// Fine-tuning configuration.
#[derive(Debug, Clone)]
pub struct FineTuningConfig {
    /// LoRA rank.
    pub lora_rank: usize,
    /// LoRA alpha.
    pub lora_alpha: usize,
    /// Dropout.
    pub dropout: f32,
    /// Learning rate.
    pub learning_rate: f32,
    /// Epochs.
    pub epochs: usize,
    /// Batch size.
    pub batch_size: usize,
    /// Max sequence length.
    pub max_seq_len: usize,
    /// Target modules.
    pub target_modules: Vec<String>,
}

impl Default for FineTuningConfig {
    fn default() -> Self {
        Self {
            lora_rank: 16,
            lora_alpha: 32,
            dropout: 0.05,
            learning_rate: 3e-4,
            epochs: 3,
            batch_size: 4,
            max_seq_len: 2048,
            target_modules: vec![
                "q_proj".to_string(),
                "k_proj".to_string(),
                "v_proj".to_string(),
                "o_proj".to_string(),
            ],
        }
    }
}

/// Dataset for fine-tuning.
#[derive(Debug, Clone)]
pub struct FineTuningDataset {
    pub samples: Vec<CodeSample>,
    pub metadata: DatasetMetadata,
}

#[derive(Debug, Clone)]
pub struct DatasetMetadata {
    pub total_samples: usize,
    pub language_distribution: HashMap<String, usize>,
    pub category_distribution: HashMap<String, usize>,
    pub avg_input_tokens: usize,
    pub avg_output_tokens: usize,
}

/// Project fine-tuning manager.
pub struct ProjectFineTuner {
    config: FineTuningConfig,
    samples: Vec<CodeSample>,
    collected_paths: HashSet<String>,
}

impl ProjectFineTuner {
    pub fn new(config: FineTuningConfig) -> Self {
        Self {
            config,
            samples: Vec::new(),
            collected_paths: HashSet::new(),
        }
    }

    /// Collect code samples from a project.
    pub fn collect_from_project(&mut self, project_path: &Path) -> usize {
        let mut count = 0;

        if let Ok(entries) = std::fs::read_dir(project_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip common non-code directories
                    let name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");

                    if !name.starts_with('.') && name != "target" && name != "node_modules" && name != "__pycache__" {
                        count += self.collect_from_project(&path);
                    }
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lang) = self.extension_to_language(ext) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let samples = self.extract_samples(&content, &path.to_string_lossy(), &lang);
                            for sample in samples {
                                if !self.collected_paths.contains(&sample.file_path) {
                                    self.samples.push(sample);
                                    self.collected_paths.insert(path.to_string_lossy().to_string());
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        count
    }

    /// Extract samples from code content.
    fn extract_samples(&self, content: &str, file_path: &str, language: &str) -> Vec<CodeSample> {
        let mut samples = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        // Extract functions
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Function detection
            if self.is_function_def(trimmed, language) {
                let func_content = self.extract_block(&lines, i);
                if func_content.len() > 10 {
                    samples.push(CodeSample {
                        id: format!("{}_func_{}", file_path, i),
                        input: format!("Write a function that does: {}", self.summarize_function(trimmed)),
                        output: func_content,
                        language: language.to_string(),
                        file_path: file_path.to_string(),
                        category: SampleCategory::Function,
                        quality_score: 0.8,
                    });
                }
            }

            // Class detection
            if self.is_class_def(trimmed, language) {
                let class_content = self.extract_block(&lines, i);
                if class_content.len() > 20 {
                    samples.push(CodeSample {
                        id: format!("{}_class_{}", file_path, i),
                        input: format!("Write a class that does: {}", trimmed),
                        output: class_content,
                        language: language.to_string(),
                        file_path: file_path.to_string(),
                        category: SampleCategory::Class,
                        quality_score: 0.8,
                    });
                }
            }
        }

        samples
    }

    /// Check if line is a function definition.
    fn is_function_def(&self, line: &str, language: &str) -> bool {
        match language {
            "rust" => line.starts_with("fn "),
            "python" => line.starts_with("def "),
            "javascript" | "typescript" => line.contains("function ") || line.contains("=>"),
            "go" => line.starts_with("func "),
            "java" => line.contains("public ") && line.contains("void ") && line.contains("("),
            _ => false,
        }
    }

    /// Check if line is a class definition.
    fn is_class_def(&self, line: &str, language: &str) -> bool {
        match language {
            "rust" => line.starts_with("struct ") || line.starts_with("impl "),
            "python" => line.starts_with("class "),
            "javascript" | "typescript" => line.starts_with("class "),
            "go" => false,
            "java" => line.starts_with("class ") || line.starts_with("interface "),
            _ => false,
        }
    }

    /// Extract a block of code (indented section).
    fn extract_block(&self, lines: &[&str], start: usize) -> String {
        if start >= lines.len() {
            return String::new();
        }

        let base_indent = Self::get_indent(lines[start]);
        let mut end = start + 1;

        while end < lines.len() {
            let line = lines[end].trim();
            if line.is_empty() {
                end += 1;
                continue;
            }

            let indent = Self::get_indent(lines[end]);
            if indent <= base_indent && !lines[end].trim().is_empty() {
                break;
            }
            end += 1;
        }

        lines[start..end].join("\n")
    }

    /// Get indentation level.
    fn get_indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Summarize function purpose.
    fn summarize_function(&self, line: &str) -> String {
        // Remove function keyword and extract signature
        let line = line.trim();
        let signature = if let Some(idx) = line.find('(') {
            &line[..idx]
        } else {
            line
        };
        signature.to_string()
    }

    /// Map file extension to language.
    fn extension_to_language(&self, ext: &str) -> Option<String> {
        match ext {
            "rs" => Some("rust".to_string()),
            "py" => Some("python".to_string()),
            "js" => Some("javascript".to_string()),
            "ts" | "tsx" => Some("typescript".to_string()),
            "go" => Some("go".to_string()),
            "java" => Some("java".to_string()),
            "cpp" | "cc" | "cxx" => Some("cpp".to_string()),
            "c" => Some("c".to_string()),
            "rb" => Some("ruby".to_string()),
            "php" => Some("php".to_string()),
            _ => None,
        }
    }

    /// Build fine-tuning dataset.
    pub fn build_dataset(&self) -> FineTuningDataset {
        let mut lang_dist = HashMap::new();
        let mut cat_dist = HashMap::new();
        let mut total_input = 0;
        let mut total_output = 0;

        for sample in &self.samples {
            *lang_dist.entry(sample.language.clone()).or_insert(0) += 1;
            *cat_dist.entry(format!("{:?}", sample.category)).or_insert(0) += 1;
            total_input += sample.input.len() / 4;
            total_output += sample.output.len() / 4;
        }

        FineTuningDataset {
            samples: self.samples.clone(),
            metadata: DatasetMetadata {
                total_samples: self.samples.len(),
                language_distribution: lang_dist,
                category_distribution: cat_dist,
                avg_input_tokens: if self.samples.is_empty() { 0 } else { total_input / self.samples.len() },
                avg_output_tokens: if self.samples.is_empty() { 0 } else { total_output / self.samples.len() },
            },
        }
    }

    /// Export dataset to JSONL format.
    pub fn export_jsonl(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;

        let file = std::fs::File::create(path)?;
        let mut buf_writer = std::io::BufWriter::new(file);

        for sample in &self.samples {
            let record = serde_json::json!({
                "id": sample.id,
                "input": sample.input,
                "output": sample.output,
                "language": sample.language,
                "category": format!("{:?}", sample.category),
            });
            writeln!(buf_writer, "{}", record)?;
        }

        Ok(())
    }

    /// Export dataset to ChatML format.
    pub fn export_chatml(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;

        let file = std::fs::File::create(path)?;
        let mut buf_writer = std::io::BufWriter::new(file);

        for sample in &self.samples {
            let record = serde_json::json!({
                "messages": [
                    {"role": "user", "content": sample.input},
                    {"role": "assistant", "content": sample.output}
                ]
            });
            writeln!(buf_writer, "{}", record)?;
        }

        Ok(())
    }

    /// Get LoRA config for training.
    pub fn get_lora_config(&self) -> LoRAConfig {
        LoRAConfig {
            rank: self.config.lora_rank,
            alpha: self.config.lora_alpha,
            dropout: self.config.dropout,
            target_modules: self.config.target_modules.clone(),
        }
    }

    /// Get statistics.
    pub fn stats(&self) -> FineTuningStats {
        FineTuningStats {
            total_samples: self.samples.len(),
            unique_files: self.collected_paths.len(),
            language_count: self.samples.iter().map(|s| &s.language).collect::<HashSet<_>>().len(),
            avg_quality: if self.samples.is_empty() {
                0.0
            } else {
                self.samples.iter().map(|s| s.quality_score).sum::<f32>() / self.samples.len() as f32
            },
        }
    }
}

impl Default for ProjectFineTuner {
    fn default() -> Self {
        Self::new(FineTuningConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct LoRAConfig {
    pub rank: usize,
    pub alpha: usize,
    pub dropout: f32,
    pub target_modules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FineTuningStats {
    pub total_samples: usize,
    pub unique_files: usize,
    pub language_count: usize,
    pub avg_quality: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_language_detection() {
        let tuner = ProjectFineTuner::default();

        assert_eq!(tuner.extension_to_language("rs"), Some("rust".to_string()));
        assert_eq!(tuner.extension_to_language("py"), Some("python".to_string()));
        assert_eq!(tuner.extension_to_language("xyz"), None);
    }

    #[test]
    fn test_function_detection() {
        let tuner = ProjectFineTuner::default();

        assert!(tuner.is_function_def("fn main()", "rust"));
        assert!(tuner.is_function_def("def hello():", "python"));
        assert!(!tuner.is_function_def("let x = 1;", "rust"));
    }

    #[test]
    fn test_stats() {
        let mut tuner = ProjectFineTuner::default();
        tuner.samples.push(CodeSample {
            id: "1".to_string(),
            input: "test".to_string(),
            output: "output".to_string(),
            language: "rust".to_string(),
            file_path: "test.rs".to_string(),
            category: SampleCategory::Function,
            quality_score: 0.9,
        });

        let stats = tuner.stats();
        assert_eq!(stats.total_samples, 1);
        assert_eq!(stats.avg_quality, 0.9);
    }
}
