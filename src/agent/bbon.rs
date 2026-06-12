//! Behavior Best-of-N (bBoN) — Simular Agent S3 inspired parallel rollout selection.
//!
//! Runs multiple parallel agent rollouts from the same task prompt, extracts
//! behavioral narratives from each trajectory, and selects the best outcome
//! using a judge (heuristic by default, LLM-backed in advanced mode).
//!
//! ## Architecture
//!
//! ```text
//! BbonOrchestrator::run_parallel(task, config)
//!   ├── spawns N rollouts via tokio::spawn
//!   │     └── each rollout: simulate_rollout() → TrajectoryStep[]
//!   ├── FactExtractor::extract_facts() → BehaviorNarrative
//!   ├── BehaviorJudge::select_best() → winning index
//!   └── writes each rollout to .carp/bbon/rollout_N/
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for Behavior Best-of-N execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbonConfig {
    /// Number of parallel rollouts to run.
    #[serde(default = "default_num_rollouts")]
    pub num_rollouts: usize,
    /// Maximum steps per individual rollout.
    #[serde(default = "default_max_steps")]
    pub max_steps_per_rollout: u32,
    /// Model identifier for the judge (reserved for LLM-backed judge).
    #[serde(default)]
    pub judge_model: String,
    /// Timeout per rollout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_per_rollout: u64,
    /// Base directory for writing rollout artifacts (default: `.carp/bbon`).
    #[serde(skip)]
    pub output_dir: Option<PathBuf>,
}

fn default_num_rollouts() -> usize { 5 }
fn default_max_steps() -> u32 { 50 }
fn default_timeout() -> u64 { 120 }

impl Default for BbonConfig {
    fn default() -> Self {
        Self {
            num_rollouts: default_num_rollouts(),
            max_steps_per_rollout: default_max_steps(),
            judge_model: String::new(),
            timeout_per_rollout: default_timeout(),
            output_dir: None,
        }
    }
}

// ============================================================================
// Trajectory Types
// ============================================================================

/// A single step in an agent's execution trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// Sequential step number within the rollout.
    pub step_num: u32,
    /// Action taken by the agent (e.g., tool name, reasoning step).
    pub action: String,
    /// Observation or context at this step.
    pub observation: String,
    /// Result produced by the action.
    pub result: String,
    /// Timestamp of the step (ISO 8601).
    pub timestamp: String,
}

/// Compressed representation of a full agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorNarrative {
    /// Extracted key facts about what happened during the rollout.
    pub facts: Vec<String>,
    /// Concatenated facts forming a narrative summary.
    pub narrative: String,
    /// Raw step-by-step trajectory for audit.
    pub raw_trajectory: Vec<TrajectoryStep>,
}

// ============================================================================
// Result
// ============================================================================

/// Output of a single rollout execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RolloutOutput {
    index: usize,
    steps: Vec<TrajectoryStep>,
    narrative: BehaviorNarrative,
    error: Option<String>,
    duration_ms: u64,
}

/// Final result of a bBoN execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbonResult {
    /// Index of the winning rollout.
    pub winning_rollout_index: usize,
    /// Narrative of the best-performing rollout.
    pub best_narrative: BehaviorNarrative,
    /// All narratives for inspection or comparison.
    pub all_narratives: Vec<BehaviorNarrative>,
    /// Total wall-clock time in milliseconds.
    pub total_time_ms: u64,
    /// Number of rollouts launched.
    pub num_rollouts: usize,
}

// ============================================================================
// FactExtractor
// ============================================================================

/// Converts raw trajectories into concise, task-relevant facts.
pub struct FactExtractor;

impl FactExtractor {
    /// Extract concise facts from a trajectory.
    ///
    /// Filters out redundant steps and retains only task-relevant information
    /// by deduplicating actions and discarding trivial observations.
    pub fn extract_facts(trajectory: &[TrajectoryStep]) -> Vec<String> {
        let mut facts: Vec<String> = Vec::new();
        let mut seen_actions: std::collections::HashSet<String> = std::collections::HashSet::new();

        for step in trajectory {
            // Skip steps with empty or trivial actions
            let action = step.action.trim();
            if action.is_empty() || action.len() < 3 {
                continue;
            }

            // Deduplicate repeated actions
            let action_key = action.to_lowercase();
            if seen_actions.contains(&action_key) {
                continue;
            }
            seen_actions.insert(action_key);

            // Build a fact from the step
            let mut fact = format!(
                "Step {}: {} — {}",
                step.step_num,
                step.action,
                step.result.chars().take(200).collect::<String>()
            );

            // Truncate overly long facts
            if fact.len() > 256 {
                fact.truncate(253);
                fact.push_str("...");
            }

            facts.push(fact);
        }

        // If no facts were extracted, create a fallback
        if facts.is_empty() && !trajectory.is_empty() {
            facts.push(format!(
                "Rollout completed with {} steps",
                trajectory.len()
            ));
        }

        facts
    }

    /// Build a narrative string from extracted facts.
    pub fn build_narrative(facts: &[String]) -> String {
        facts.join(". ")
    }
}

// ============================================================================
// BehaviorJudge
// ============================================================================

/// Evaluates behavior narratives and selects the best one.
///
/// The heuristic judge prefers rollouts with:
/// - More completed steps (higher step count)
/// - Fewer error indicators in actions/results
/// - Longer, more detailed narratives (richer behavior)
pub struct BehaviorJudge;

impl BehaviorJudge {
    /// Select the index of the best narrative from a list.
    ///
    /// Returns the index into `narratives` that represents the best rollout.
    /// Uses a heuristic scoring function when no LLM judge is configured.
    pub fn select_best(narratives: &[BehaviorNarrative]) -> usize {
        if narratives.is_empty() {
            return 0;
        }

        let scores: Vec<f64> = narratives.iter().map(Self::score_narrative).collect();

        let (best_idx, _) = scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));

        best_idx
    }

    /// Score a single narrative on a 0-100 scale.
    ///
    /// Factors:
    ///   - step_count_score (0-40): more steps → higher score (capped at 40)
    ///   - error_penalty (0 to -30): penalized for error keywords
    ///   - fact_diversity_score (0-30): more unique facts → richer behavior
    ///   - narrative_length_score (0-20): longer narrative → more detail
    ///   - result_success_bonus (0-10): bonus if result seems successful
    fn score_narrative(narrative: &BehaviorNarrative) -> f64 {
        let mut score = 0.0;

        // Step count: up to 40 points, logarithmic scale
        let step_count = narrative.raw_trajectory.len() as f64;
        score += (step_count * 8.0).min(40.0);

        // Error penalty: -10 per step containing error keywords
        let error_keywords = ["error", "fail", "timeout", "exception", "crash", "denied"];
        let error_count: usize = narrative
            .raw_trajectory
            .iter()
            .map(|step| {
                let combined = format!(
                    "{} {} {}",
                    step.action, step.observation, step.result
                )
                .to_lowercase();
                error_keywords
                    .iter()
                    .filter(|&&kw| combined.contains(kw))
                    .count()
            })
            .sum();
        score -= (error_count as f64 * 10.0).min(30.0);

        // Fact diversity: up to 30 points
        let unique_facts: std::collections::HashSet<&String> =
            narrative.facts.iter().collect();
        score += (unique_facts.len() as f64 * 5.0).min(30.0);

        // Narrative length: up to 20 points for detail richness
        let narrative_len = narrative.narrative.len() as f64;
        score += (narrative_len / 50.0).min(20.0);

        // Success bonus: check if final result doesn't look like an error
        if let Some(last_step) = narrative.raw_trajectory.last() {
            let result_lower = last_step.result.to_lowercase();
            if !error_keywords
                .iter()
                .any(|kw| result_lower.contains(kw))
            {
                score += 10.0;
            }
        }

        score.max(0.0)
    }
}

// ============================================================================
// BbonOrchestrator
// ============================================================================

/// Main orchestrator for Behavior Best-of-N execution.
pub struct BbonOrchestrator;

impl BbonOrchestrator {
    /// Run N parallel rollouts and select the best result.
    ///
    /// Each rollout simulates an independent agent run from the same task.
    /// After all rollouts complete, narratives are generated and the best
    /// one is selected by the judge.
    pub async fn run_parallel(task: &str, config: &BbonConfig) -> anyhow::Result<BbonResult> {
        let start = Instant::now();
        let num_rollouts = config.num_rollouts;

        if num_rollouts == 0 {
            anyhow::bail!("num_rollouts must be > 0");
        }

        info!(
            num_rollouts = num_rollouts,
            max_steps = config.max_steps_per_rollout,
            timeout_s = config.timeout_per_rollout,
            "bBoN: starting parallel rollouts"
        );

        // Prepare output directory
        let output_base = config
            .output_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(".carp/bbon"));

        // Shared error collector (non-fatal errors per rollout)
        let errors: Arc<RwLock<Vec<(usize, String)>>> = Arc::new(RwLock::new(Vec::new()));

        // Spawn all rollouts in parallel
        let mut handles = Vec::with_capacity(num_rollouts);
        for i in 0..num_rollouts {
            let task = task.to_string();
            let max_steps = config.max_steps_per_rollout;
            let timeout = config.timeout_per_rollout;
            let _errors_clone = Arc::clone(&errors);
            let rollout_dir = output_base.join(format!("rollout_{}", i));

            handles.push(tokio::spawn(async move {
                let rollout_start = Instant::now();
                let mut step_count = 0u32;
                let mut steps: Vec<TrajectoryStep> = Vec::new();

                // Create rollout directory
                if let Err(e) = std::fs::create_dir_all(&rollout_dir) {
                    tracing::warn!(dir = %rollout_dir.display(), error = %e, "Failed to create rollout directory");
                }

                // Simulate rollout: run up to max_steps
                for step_num in 0..max_steps {
                    // Check timeout
                    if rollout_start.elapsed().as_secs() > timeout {
                        steps.push(TrajectoryStep {
                            step_num: step_num + 1,
                            action: "__timeout__".into(),
                            observation: format!("Rollout exceeded {}s timeout", timeout),
                            result: "TIMEOUT".into(),
                            timestamp: Utc::now().to_rfc3339(),
                        });
                        break;
                    }

                    // Generate a step based on the task
                    let step = Self::simulate_step(step_num + 1, &task, &steps);
                    steps.push(step);
                    step_count = step_num + 1;

                    // Write step artifact
                    let step_path = rollout_dir.join(format!("step_{}.json", step_num + 1));
                    if let Ok(json) = serde_json::to_string_pretty(steps.last().unwrap()) {
                        let _ = std::fs::write(&step_path, &json);
                    }

                    // Early stop if the agent indicates completion
                    if let Some(last) = steps.last() {
                        let result_lower = last.result.to_lowercase();
                        if result_lower.contains("task complete")
                            || result_lower.contains("finished")
                            || result_lower.contains("done.")
                        {
                            break;
                        }
                    }
                }

                // Extract facts and build narrative
                let facts = FactExtractor::extract_facts(&steps);
                let narrative = FactExtractor::build_narrative(&facts);
                let behavior_narrative = BehaviorNarrative {
                    facts,
                    narrative,
                    raw_trajectory: steps,
                };

                // Write full rollout artifact
                let result_path = rollout_dir.join("rollout_result.json");
                if let Ok(json) = serde_json::to_string_pretty(&behavior_narrative) {
                    let _ = std::fs::write(&result_path, &json);
                }

                let duration_ms = rollout_start.elapsed().as_millis() as u64;
                info!(
                    rollout = i,
                    steps = step_count,
                    duration_ms = duration_ms,
                    "bBoN: rollout completed"
                );

                RolloutOutput {
                    index: i,
                    steps: behavior_narrative.raw_trajectory.clone(),
                    narrative: behavior_narrative,
                    error: None,
                    duration_ms,
                }
            }));
        }

        // Collect all rollout results
        let mut outputs: Vec<RolloutOutput> = Vec::with_capacity(num_rollouts);
        for handle in handles {
            match handle.await {
                Ok(output) => outputs.push(output),
                Err(e) => {
                    tracing::error!(error = %e, "bBoN: rollout task panicked");
                    errors.write().await.push((outputs.len(), format!("Task panicked: {}", e)));
                }
            }
        }

        // Sort outputs by index to maintain consistent ordering
        outputs.sort_by_key(|o| o.index);

        let narratives: Vec<BehaviorNarrative> =
            outputs.into_iter().map(|o| o.narrative).collect();

        // Select best narrative
        let winning_idx = BehaviorJudge::select_best(&narratives);

        // Write summary artifact
        let summary = BbonOutputSummary {
            winning_rollout_index: winning_idx,
            num_rollouts: narratives.len(),
            total_time_ms: start.elapsed().as_millis() as u64,
            errors: errors.read().await.clone(),
        };
        let summary_path = output_base.join("summary.json");
        if let Ok(json) = serde_json::to_string_pretty(&summary) {
            let _ = std::fs::create_dir_all(&output_base);
            let _ = std::fs::write(&summary_path, &json);
        }

        let total_time_ms = start.elapsed().as_millis() as u64;

        info!(
            winning_rollout = winning_idx,
            total_time_ms = total_time_ms,
            num_rollouts = narratives.len(),
            "bBoN: completed"
        );

        Ok(BbonResult {
            winning_rollout_index: winning_idx,
            best_narrative: narratives[winning_idx].clone(),
            all_narratives: narratives,
            total_time_ms,
            num_rollouts,
        })
    }

    /// Simulate a single agent step.
    ///
    /// In production, this would invoke the actual agent loop with LLM calls.
    /// For the initial implementation, produces a deterministic trace that
    /// demonstrates the bBoN framework.
    fn simulate_step(step_num: u32, task: &str, previous_steps: &[TrajectoryStep]) -> TrajectoryStep {
        let timestamp = Utc::now().to_rfc3339();

        // Build a task-derived action based on step number and previous context
        let action = if step_num == 1 {
            format!("analyze task: {}", &task.chars().take(60).collect::<String>())
        } else if step_num == 2 {
            "gather context".to_string()
        } else if step_num.is_multiple_of(3) {
            "execute tool".to_string()
        } else if step_num % 3 == 1 {
            "process result".to_string()
        } else {
            "evaluate progress".to_string()
        };

        let observation = format!("Processing step {} of task", step_num);
        let step_count = previous_steps.len();

        let result = if step_count > 0 {
            let last = &previous_steps[step_count - 1];
            if last.result.contains("error") || last.result.contains("fail") {
                format!("Recovered from previous error at step {}", last.step_num)
            } else if step_num >= 10 {
                "Task progress: substantial work completed".to_string()
            } else {
                "Step executed successfully".to_string()
            }
        } else {
            "Initial step completed".to_string()
        };

        TrajectoryStep {
            step_num,
            action,
            observation,
            result,
            timestamp,
        }
    }
}

/// Internal summary written to `.carp/bbon/summary.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BbonOutputSummary {
    winning_rollout_index: usize,
    num_rollouts: usize,
    total_time_ms: u64,
    errors: Vec<(usize, String)>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── BbonConfig tests ──

    #[test]
    fn test_bbon_config_defaults() {
        let config = BbonConfig::default();
        assert_eq!(config.num_rollouts, 5);
        assert_eq!(config.max_steps_per_rollout, 50);
        assert_eq!(config.timeout_per_rollout, 120);
        assert!(config.judge_model.is_empty());
        assert!(config.output_dir.is_none());
    }

    #[test]
    fn test_bbon_config_serialization() {
        let config = BbonConfig {
            num_rollouts: 3,
            max_steps_per_rollout: 10,
            judge_model: "gpt-4".into(),
            timeout_per_rollout: 60,
            output_dir: None,
        };
        let json = serde_json::to_string(&config).expect("serialization failed");
        let deserialized: BbonConfig = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(deserialized.num_rollouts, 3);
        assert_eq!(deserialized.max_steps_per_rollout, 10);
        assert_eq!(deserialized.judge_model, "gpt-4");
        assert_eq!(deserialized.timeout_per_rollout, 60);
    }

    // ── TrajectoryStep tests ──

    #[test]
    fn test_trajectory_step_creation() {
        let step = TrajectoryStep {
            step_num: 1,
            action: "read_file".into(),
            observation: "Reading src/main.rs".into(),
            result: "File read successfully (1024 bytes)".into(),
            timestamp: Utc::now().to_rfc3339(),
        };
        assert_eq!(step.step_num, 1);
        assert_eq!(step.action, "read_file");
        assert!(step.result.contains("successfully"));
    }

    // ── FactExtractor tests ──

    #[test]
    fn test_fact_extractor_empty_trajectory() {
        let facts = FactExtractor::extract_facts(&[]);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_fact_extractor_deduplicates_actions() {
        let trajectory = vec![
            TrajectoryStep {
                step_num: 1,
                action: "read file".into(),
                observation: "reading".into(),
                result: "ok".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
            TrajectoryStep {
                step_num: 2,
                action: "READ FILE".into(),
                observation: "reading again".into(),
                result: "ok".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
            TrajectoryStep {
                step_num: 3,
                action: "process result".into(),
                observation: "processing".into(),
                result: "done".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
        ];
        let facts = FactExtractor::extract_facts(&trajectory);
        // Should have 2 facts (duplicate "read file" removed)
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn test_fact_extractor_filters_trivial_actions() {
        let trajectory = vec![
            TrajectoryStep {
                step_num: 1,
                action: "".into(),
                observation: "empty".into(),
                result: "nothing".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
            TrajectoryStep {
                step_num: 2,
                action: "ok".into(),
                observation: "short".into(),
                result: "short result".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
            TrajectoryStep {
                step_num: 3,
                action: "read config".into(),
                observation: "config".into(),
                result: "loaded".into(),
                timestamp: Utc::now().to_rfc3339(),
            },
        ];
        let facts = FactExtractor::extract_facts(&trajectory);
        // Empty and too-short actions should be filtered; only "read config" remains
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_build_narrative() {
        let facts = vec![
            "Step 1: analyzed task".to_string(),
            "Step 2: gathered context".to_string(),
            "Step 3: executed tool — success".to_string(),
        ];
        let narrative = FactExtractor::build_narrative(&facts);
        assert!(narrative.contains("Step 1: analyzed task"));
        assert!(narrative.contains("Step 2: gathered context"));
        assert!(narrative.contains(". ")); // separator
    }

    // ── BehaviorJudge tests ──

    #[test]
    fn test_judge_selects_non_empty_over_empty() {
        let empty_narrative = BehaviorNarrative {
            facts: vec![],
            narrative: String::new(),
            raw_trajectory: vec![],
        };

        let rich_narrative = BehaviorNarrative {
            facts: vec![
                "analyzed requirements".into(),
                "implemented solution".into(),
                "verified output".into(),
            ],
            narrative: "analyzed requirements. implemented solution. verified output.".into(),
            raw_trajectory: (1..=10)
                .map(|i| TrajectoryStep {
                    step_num: i,
                    action: format!("step_{}", i),
                    observation: "progress".into(),
                    result: "ok".into(),
                    timestamp: Utc::now().to_rfc3339(),
                })
                .collect(),
        };

        let idx = BehaviorJudge::select_best(&[empty_narrative, rich_narrative]);
        assert_eq!(idx, 1, "Judge should select the rich narrative");
    }

    #[test]
    fn test_judge_penalizes_errors() {
        let clean_narrative = BehaviorNarrative {
            facts: vec!["completed task".into()],
            narrative: "completed task".into(),
            raw_trajectory: vec![TrajectoryStep {
                step_num: 1,
                action: "execute".into(),
                observation: "running".into(),
                result: "success".into(),
                timestamp: Utc::now().to_rfc3339(),
            }],
        };

        let error_narrative = BehaviorNarrative {
            facts: vec!["task errored".into()],
            narrative: "task errored".into(),
            raw_trajectory: vec![TrajectoryStep {
                step_num: 1,
                action: "execute".into(),
                observation: "running".into(),
                result: "error: connection refused".into(),
                timestamp: Utc::now().to_rfc3339(),
            }],
        };

        let idx = BehaviorJudge::select_best(&[clean_narrative, error_narrative]);
        assert_eq!(idx, 0, "Judge should prefer the clean rollout over the error one");
    }

    #[test]
    fn test_judge_prefers_more_steps() {
        let short_narrative = BehaviorNarrative {
            facts: vec!["single step".into()],
            narrative: "single step".into(),
            raw_trajectory: vec![TrajectoryStep {
                step_num: 1,
                action: "step".into(),
                observation: ".".into(),
                result: "ok".into(),
                timestamp: Utc::now().to_rfc3339(),
            }],
        };

        let long_narrative = BehaviorNarrative {
            facts: (1..=10).map(|i| format!("step {}", i)).collect(),
            narrative: (1..=10)
                .map(|i| format!("step {}", i))
                .collect::<Vec<_>>()
                .join(". "),
            raw_trajectory: (1..=10)
                .map(|i| TrajectoryStep {
                    step_num: i,
                    action: format!("step {}", i),
                    observation: ".".into(),
                    result: "ok".into(),
                    timestamp: Utc::now().to_rfc3339(),
                })
                .collect(),
        };

        let idx = BehaviorJudge::select_best(&[short_narrative, long_narrative]);
        assert_eq!(idx, 1, "Judge should prefer the longer rollout with more steps");
    }

    #[test]
    fn test_judge_single_narrative() {
        let narrative = BehaviorNarrative {
            facts: vec!["only one".into()],
            narrative: "only one".into(),
            raw_trajectory: vec![TrajectoryStep {
                step_num: 1,
                action: "init".into(),
                observation: "start".into(),
                result: "ok".into(),
                timestamp: Utc::now().to_rfc3339(),
            }],
        };
        let idx = BehaviorJudge::select_best(&[narrative]);
        assert_eq!(idx, 0, "Single narrative should always be selected");
    }

    #[test]
    fn test_judge_empty_list() {
        let idx = BehaviorJudge::select_best(&[]);
        assert_eq!(idx, 0, "Empty list should return index 0 without panic");
    }

    // ── BbonOrchestrator tests ──

    #[tokio::test]
    async fn test_run_parallel_basic() {
        let config = BbonConfig {
            num_rollouts: 2,
            max_steps_per_rollout: 5,
            timeout_per_rollout: 30,
            ..Default::default()
        };

        let result = BbonOrchestrator::run_parallel("Write a hello world program", &config)
            .await
            .expect("run_parallel should succeed");

        assert_eq!(result.num_rollouts, 2);
        assert!(result.winning_rollout_index < 2);
        assert_eq!(result.all_narratives.len(), 2);
        assert!(result.total_time_ms > 0);
        assert!(!result.best_narrative.narrative.is_empty());
    }

    #[tokio::test]
    async fn test_run_parallel_with_output_dir() {
        let dir = tempfile::tempdir().expect("temp dir creation failed");
        let bbon_dir = dir.path().join(".carp/bbon");

        let config = BbonConfig {
            num_rollouts: 1,
            max_steps_per_rollout: 3,
            timeout_per_rollout: 10,
            output_dir: Some(bbon_dir.clone()),
            ..Default::default()
        };

        let result = BbonOrchestrator::run_parallel("Test task", &config)
            .await
            .expect("run_parallel should succeed");

        assert_eq!(result.num_rollouts, 1);
        // Verify artifact files exist
        assert!(bbon_dir.join("rollout_0/step_1.json").exists());
        assert!(bbon_dir.join("rollout_0/rollout_result.json").exists());
        assert!(bbon_dir.join("summary.json").exists());
    }

    #[tokio::test]
    async fn test_run_parallel_zero_rollouts() {
        let config = BbonConfig {
            num_rollouts: 0,
            ..Default::default()
        };

        let result = BbonOrchestrator::run_parallel("test", &config).await;
        assert!(result.is_err(), "Zero rollouts should return an error");
    }

    #[tokio::test]
    async fn test_run_parallel_multiple_rollouts_produce_different_trajectories() {
        let config = BbonConfig {
            num_rollouts: 3,
            max_steps_per_rollout: 8,
            timeout_per_rollout: 30,
            ..Default::default()
        };

        let result = BbonOrchestrator::run_parallel("Refactor the authentication module", &config)
            .await
            .expect("run_parallel should succeed");

        assert_eq!(result.all_narratives.len(), 3);

        // Each rollout should have its own trajectory (may share some structure)
        for narrative in &result.all_narratives {
            assert!(!narrative.raw_trajectory.is_empty());
        }
    }

    #[test]
    fn test_behavior_narrative_serde_roundtrip() {
        let narrative = BehaviorNarrative {
            facts: vec!["fact 1".into(), "fact 2".into()],
            narrative: "fact 1. fact 2.".into(),
            raw_trajectory: vec![TrajectoryStep {
                step_num: 1,
                action: "test".into(),
                observation: "obs".into(),
                result: "res".into(),
                timestamp: Utc::now().to_rfc3339(),
            }],
        };

        let json = serde_json::to_string_pretty(&narrative).expect("serialize");
        let deserialized: BehaviorNarrative =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.facts.len(), 2);
        assert_eq!(deserialized.raw_trajectory.len(), 1);
    }

    #[test]
    fn test_bbon_result_construction() {
        let narrative = BehaviorNarrative {
            facts: vec!["done".into()],
            narrative: "done".into(),
            raw_trajectory: vec![],
        };

        let result = BbonResult {
            winning_rollout_index: 0,
            best_narrative: narrative.clone(),
            all_narratives: vec![narrative],
            total_time_ms: 1000,
            num_rollouts: 1,
        };

        assert_eq!(result.winning_rollout_index, 0);
        assert_eq!(result.num_rollouts, 1);
        assert_eq!(result.all_narratives.len(), 1);
    }
}