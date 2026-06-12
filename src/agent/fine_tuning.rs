//! Fine-tuning Data Collection - Gather training data for LoRA fine-tuning.
//!
//! This module collects high-quality training data from user interactions:
//! - Code generation pairs (prompt → code)
//! - Refactoring examples
//! - Code review comments
//! - Bug fixes with explanations
//!
//! Data is collected in a format ready for LoRA training pipelines.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// A training example for fine-tuning.
#[derive(Debug, Clone)]
pub struct TrainingExample {
    /// Unique identifier.
    pub id: String,
    /// Type of example.
    pub example_type: ExampleType,
    /// Input prompt.
    pub instruction: String,
    /// Expected output.
    pub output: String,
    /// Metadata.
    pub metadata: ExampleMetadata,
    /// Quality score (0.0 - 1.0).
    pub quality_score: f32,
    /// When this example was created.
    pub timestamp: u64,
}

/// Type of training example.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExampleType {
    /// Code generation.
    CodeGeneration,
    /// Code refactoring.
    Refactoring,
    /// Bug fix.
    BugFix,
    /// Code review.
    CodeReview,
    /// Test generation.
    TestGeneration,
    /// Documentation.
    Documentation,
    /// Chat response.
    Chat,
}

/// Metadata for a training example.
#[derive(Debug, Clone)]
pub struct ExampleMetadata {
    /// Programming language.
    pub language: Option<String>,
    /// Source file path.
    pub file_path: Option<PathBuf>,
    /// Number of tokens in input.
    pub input_tokens: usize,
    /// Number of tokens in output.
    pub output_tokens: usize,
    /// User rating if available.
    pub user_rating: Option<u8>,
    /// Was the output accepted?
    pub was_accepted: bool,
    /// Number of edits made.
    pub edit_count: u32,
    /// Context files used.
    pub context_files: Vec<PathBuf>,
}

/// Quality filter configuration.
#[derive(Debug, Clone)]
pub struct QualityFilter {
    /// Minimum quality score.
    pub min_quality: f32,
    /// Minimum user rating.
    pub min_user_rating: Option<u8>,
    /// Only include accepted examples.
    pub only_accepted: bool,
    /// Minimum token count.
    pub min_output_tokens: usize,
    /// Maximum token count.
    pub max_output_tokens: usize,
}

impl Default for QualityFilter {
    fn default() -> Self {
        Self {
            min_quality: 0.7,
            min_user_rating: None,
            only_accepted: true,
            min_output_tokens: 10,
            max_output_tokens: 4096,
        }
    }
}

/// Fine-tuning data collector.
pub struct FineTuningCollector {
    /// Collected examples.
    examples: Arc<RwLock<Vec<TrainingExample>>>,
    /// Examples by type.
    by_type: Arc<RwLock<HashMap<ExampleType, Vec<String>>>>,
    /// Statistics.
    stats: Arc<RwLock<CollectorStats>>,
    /// Output directory.
    output_dir: PathBuf,
}

/// Statistics for the collector.
#[derive(Debug, Clone, Default)]
pub struct CollectorStats {
    pub total_collected: u64,
    pub by_type: HashMap<String, u64>,
    pub total_tokens: u64,
    pub avg_quality: f32,
}

impl FineTuningCollector {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            examples: Arc::new(RwLock::new(Vec::new())),
            by_type: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CollectorStats::default())),
            output_dir,
        }
    }

    /// Collect a new training example.
    pub async fn collect(&self, example: TrainingExample) -> String {
        let id = example.id.clone();

        // Add to examples
        let mut examples = self.examples.write().await;
        examples.push(example.clone());

        // Add to type index
        let mut by_type = self.by_type.write().await;
        by_type.entry(example.example_type).or_insert_with(Vec::new).push(id.clone());

        // Update stats
        let mut stats = self.stats.write().await;
        stats.total_collected += 1;
        *stats.by_type.entry(format!("{:?}", example.example_type)).or_insert(0) += 1;
        stats.total_tokens += example.metadata.input_tokens as u64 + example.metadata.output_tokens as u64;

        id
    }

    /// Collect a code generation example.
    pub async fn collect_code_generation(
        &self,
        instruction: &str,
        code: &str,
        language: &str,
        file_path: Option<&str>,
        was_accepted: bool,
    ) -> String {
        let id = format!("code_{}", self.generate_id());

        let example = TrainingExample {
            id: id.clone(),
            example_type: ExampleType::CodeGeneration,
            instruction: instruction.to_string(),
            output: code.to_string(),
            metadata: ExampleMetadata {
                language: Some(language.to_string()),
                file_path: file_path.map(PathBuf::from),
                input_tokens: estimate_tokens(instruction),
                output_tokens: estimate_tokens(code),
                user_rating: None,
                was_accepted,
                edit_count: 0,
                context_files: Vec::new(),
            },
            quality_score: if was_accepted { 0.9 } else { 0.5 },
            timestamp: current_timestamp(),
        };

        self.collect(example).await
    }

    /// Collect a refactoring example.
    pub async fn collect_refactoring(
        &self,
        original: &str,
        refactored: &str,
        language: &str,
        refactor_type: &str,
        was_accepted: bool,
    ) -> String {
        let id = format!("refactor_{}", self.generate_id());
        let instruction = format!("Refactor this {} code: {}", refactor_type, original);

        let example = TrainingExample {
            id: id.clone(),
            example_type: ExampleType::Refactoring,
            instruction,
            output: refactored.to_string(),
            metadata: ExampleMetadata {
                language: Some(language.to_string()),
                file_path: None,
                input_tokens: estimate_tokens(original),
                output_tokens: estimate_tokens(refactored),
                user_rating: None,
                was_accepted,
                edit_count: 0,
                context_files: Vec::new(),
            },
            quality_score: if was_accepted { 0.85 } else { 0.4 },
            timestamp: current_timestamp(),
        };

        self.collect(example).await
    }

    /// Collect a bug fix example.
    pub async fn collect_bug_fix(
        &self,
        buggy_code: &str,
        fixed_code: &str,
        explanation: &str,
        language: &str,
        was_accepted: bool,
    ) -> String {
        let id = format!("bugfix_{}", self.generate_id());
        let instruction = format!("Fix this bug. Error: {}\n\nCode:\n{}", explanation, buggy_code);

        let example = TrainingExample {
            id: id.clone(),
            example_type: ExampleType::BugFix,
            instruction,
            output: fixed_code.to_string(),
            metadata: ExampleMetadata {
                language: Some(language.to_string()),
                file_path: None,
                input_tokens: estimate_tokens(buggy_code) + estimate_tokens(explanation),
                output_tokens: estimate_tokens(fixed_code),
                user_rating: None,
                was_accepted,
                edit_count: 0,
                context_files: Vec::new(),
            },
            quality_score: if was_accepted { 0.95 } else { 0.5 },
            timestamp: current_timestamp(),
        };

        self.collect(example).await
    }

    /// Collect a test generation example.
    pub async fn collect_test_generation(
        &self,
        code: &str,
        tests: &str,
        language: &str,
        was_accepted: bool,
    ) -> String {
        let id = format!("test_{}", self.generate_id());
        let instruction = format!("Generate tests for:\n{}", code);

        let example = TrainingExample {
            id: id.clone(),
            example_type: ExampleType::TestGeneration,
            instruction,
            output: tests.to_string(),
            metadata: ExampleMetadata {
                language: Some(language.to_string()),
                file_path: None,
                input_tokens: estimate_tokens(code),
                output_tokens: estimate_tokens(tests),
                user_rating: None,
                was_accepted,
                edit_count: 0,
                context_files: Vec::new(),
            },
            quality_score: if was_accepted { 0.9 } else { 0.5 },
            timestamp: current_timestamp(),
        };

        self.collect(example).await
    }

    /// Filter and get high-quality examples.
    pub async fn get_filtered(&self, filter: &QualityFilter) -> Vec<TrainingExample> {
        let examples = self.examples.read().await;

        examples.iter()
            .filter(|e| {
                e.quality_score >= filter.min_quality
                && e.metadata.output_tokens >= filter.min_output_tokens
                && e.metadata.output_tokens <= filter.max_output_tokens
                && (!filter.only_accepted || e.metadata.was_accepted)
                && filter.min_user_rating.is_none_or(|min| {
                    e.metadata.user_rating.is_some_and(|r| r >= min)
                })
            })
            .cloned()
            .collect()
    }

    /// Export examples to JSONL format for training.
    pub async fn export_jsonl(&self, path: &PathBuf) -> std::io::Result<usize> {
        use tokio::io::AsyncWriteExt;

        let examples = self.examples.read().await;
        let mut file = tokio::fs::File::create(path).await?;
        let mut count = 0;

        for example in examples.iter() {
            // Format for training: instruction-based
            let json = serde_json::json!({
                "id": example.id,
                "instruction": example.instruction,
                "output": example.output,
                "type": format!("{:?}", example.example_type),
                "language": example.metadata.language,
                "quality": example.quality_score,
            });

            let line = serde_json::to_string(&json).expect("json serialize failed: fine_tuning.rs:326") + "\n";
            file.write_all(line.as_bytes()).await?;
            count += 1;
        }

        Ok(count)
    }

    /// Export in ChatML format for instruction tuning.
    pub async fn export_chatml(&self, path: &PathBuf) -> std::io::Result<usize> {
        use tokio::io::AsyncWriteExt;

        let examples = self.examples.read().await;
        let mut file = tokio::fs::File::create(path).await?;
        let mut count = 0;

        for example in examples.iter() {
            let chatml = format!(
                "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n{}<|im_end|>\n",
                example.instruction,
                example.output
            );

            file.write_all(chatml.as_bytes()).await?;
            count += 1;
        }

        Ok(count)
    }

    /// Get statistics.
    pub async fn stats(&self) -> CollectorStats {
        self.stats.read().await.clone()
    }

    /// Get count by type.
    pub async fn count_by_type(&self) -> HashMap<String, u64> {
        let by_type = self.by_type.read().await;
        by_type.iter()
            .map(|(k, v)| (format!("{:?}", k), v.len() as u64))
            .collect()
    }

    fn generate_id(&self) -> String {
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unwrap failed: fine_tuning.rs:373")
            .as_nanos();
        format!("{:x}", now)
    }

    /// Get the output directory for collected training data.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }
}

/// Estimate token count.
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unwrap failed: fine_tuning.rs:393")
        .as_secs()
}

/// Project-specific fine-tuning configurator.
pub struct ProjectFineTuner {
    collector: FineTuningCollector,
    project_path: PathBuf,
}

impl ProjectFineTuner {
    pub fn new(project_path: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            collector: FineTuningCollector::new(output_dir),
            project_path,
        }
    }

    /// Collect examples from project git history.
    pub async fn collect_from_git(&self) -> std::io::Result<u64> {
        // Placeholder for git-based collection
        // In real implementation, would parse git history for commits
        Ok(0)
    }

    /// Generate training dataset config.
    pub async fn generate_dataset_config(&self) -> serde_json::Value {
        let stats = self.collector.stats().await;
        let by_type = self.collector.count_by_type().await;

        serde_json::json!({
            "project_path": self.project_path,
            "total_examples": stats.total_collected,
            "by_type": by_type,
            "total_tokens": stats.total_tokens,
            "avg_quality": stats.avg_quality,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_collect_code_generation() {
        let collector = FineTuningCollector::new(PathBuf::from("/tmp/fine_tuning"));

        let id = collector.collect_code_generation(
            "Write a function to add two numbers",
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            "rust",
            Some("math.rs"),
            true,
        ).await;

        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn test_filter_examples() {
        let collector = FineTuningCollector::new(PathBuf::from("/tmp/fine_tuning"));

        collector.collect_code_generation(
            "Write hello",
            "println!(\"hello\")",
            "rust",
            None,
            true,
        ).await;

        let filter = QualityFilter {
            min_quality: 0.8,
            only_accepted: true,
            ..Default::default()
        };

        let filtered = collector.get_filtered(&filter).await;
        assert!(!filtered.is_empty());
    }
}
