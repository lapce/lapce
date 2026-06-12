//! Refactoring History — Learn from user decisions to improve suggestions.
//!
//! This module tracks:
//! - User accept/reject patterns
//! - Refactoring preferences
//! - Success/failure rates
//! - Learning from feedback

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A refactoring decision record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorDecision {
    pub timestamp: u64,
    pub refactor_type: String,
    pub target_name: String,
    pub was_accepted: bool,
    pub was_modified: bool,
    pub user_feedback: Option<String>,
    pub context: RefactorContext,
}

/// Context where refactoring was applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorContext {
    pub file_type: String,
    pub file_size: usize,
    pub project_type: String,
    pub language: String,
}

/// Learned patterns from refactoring history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPatterns {
    pub preferred_refactors: HashMap<String, f64>, // type -> success rate
    pub avoid_patterns: Vec<String>,
    pub typical_modifications: HashMap<String, String>, // type -> common modification
    pub success_by_language: HashMap<String, f64>,
    pub success_by_project: HashMap<String, f64>,
}

/// Refactoring history tracker.
pub struct RefactorHistory {
    decisions: Vec<RefactorDecision>,
    patterns: LearnedPatterns,
}

impl RefactorHistory {
    pub fn new() -> Self {
        Self {
            decisions: Vec::new(),
            patterns: LearnedPatterns {
                preferred_refactors: HashMap::new(),
                avoid_patterns: Vec::new(),
                typical_modifications: HashMap::new(),
                success_by_language: HashMap::new(),
                success_by_project: HashMap::new(),
            },
        }
    }

    /// Record a refactoring decision.
    pub fn record(&mut self, decision: RefactorDecision) {
        self.decisions.push(decision.clone());
        self.update_patterns(&decision);
    }

    /// Update learned patterns based on decision.
    fn update_patterns(&mut self, decision: &RefactorDecision) {
        // Update success rate for refactor type
        let entry = self.patterns.preferred_refactors
            .entry(decision.refactor_type.clone())
            .or_insert(0.0);
        
        if decision.was_accepted && !decision.was_modified {
            *entry = (*entry * 0.9) + 0.1; // Exponential moving average
        } else {
            *entry *= 0.9; // Decrease on rejection
        }
        
        // Track modifications
        if decision.was_modified {
            self.patterns.typical_modifications
                .insert(decision.refactor_type.clone(), "user_modified".to_string());
        }
        
        // Track by language
        let lang_entry = self.patterns.success_by_language
            .entry(decision.context.language.clone())
            .or_insert(0.5);
        
        if decision.was_accepted {
            *lang_entry = (*lang_entry * 0.9) + 0.1;
        } else {
            *lang_entry *= 0.9;
        }
        
        // Track by project type
        let proj_entry = self.patterns.success_by_project
            .entry(decision.context.project_type.clone())
            .or_insert(0.5);
        
        if decision.was_accepted {
            *proj_entry = (*proj_entry * 0.9) + 0.1;
        } else {
            *proj_entry *= 0.9;
        }
    }

    /// Get success rate for a refactor type.
    pub fn get_success_rate(&self, refactor_type: &str) -> f64 {
        self.patterns.preferred_refactors
            .get(refactor_type)
            .copied()
            .unwrap_or(0.5)
    }

    /// Get best refactor type for language.
    pub fn get_best_for_language(&self, language: &str) -> Option<String> {
        self.patterns.success_by_language
            .get(language)
            .and_then(|&rate| {
                if rate > 0.6 {
                    self.patterns.preferred_refactors
                        .iter()
                        .max_by(|a, b| a.1.partial_cmp(b.1).expect("unwrap failed: refactor_history.rs:128"))
                        .map(|(k, _)| k.clone())
                } else {
                    None
                }
            })
    }

    /// Get suggestions based on history.
    pub fn get_suggestions(&self, _context: &RefactorContext) -> Vec<String> {
        let mut suggestions = Vec::new();
        
        // Suggest high-success refactors for this language
        for (refactor_type, &rate) in &self.patterns.preferred_refactors {
            if rate > 0.7 {
                suggestions.push(format!("{} (success rate: {:.0}%)", refactor_type, rate * 100.0));
            }
        }
        
        suggestions
    }

    /// Export patterns as JSON.
    pub fn export_patterns(&self) -> String {
        serde_json::to_string_pretty(&self.patterns).unwrap_or_default()
    }

    /// Import patterns from JSON.
    pub fn import_patterns(&mut self, json: &str) {
        if let Ok(patterns) = serde_json::from_str::<LearnedPatterns>(json) {
            self.patterns = patterns;
        }
    }

    /// Get recent decisions.
    pub fn get_recent(&self, limit: usize) -> Vec<&RefactorDecision> {
        self.decisions.iter().rev().take(limit).collect()
    }

    /// Get overall statistics.
    pub fn get_stats(&self) -> RefactorStats {
        let total = self.decisions.len();
        let accepted = self.decisions.iter().filter(|d| d.was_accepted).count();
        let modified = self.decisions.iter().filter(|d| d.was_modified).count();
        
        RefactorStats {
            total_refactors: total,
            acceptance_rate: if total > 0 { accepted as f64 / total as f64 } else { 0.0 },
            modification_rate: if total > 0 { modified as f64 / total as f64 } else { 0.0 },
            top_refactor: self.patterns.preferred_refactors
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).expect("unwrap failed: refactor_history.rs:179"))
                .map(|(k, _)| k.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorStats {
    pub total_refactors: usize,
    pub acceptance_rate: f64,
    pub modification_rate: f64,
    pub top_refactor: Option<String>,
}

impl Default for RefactorHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_learn() {
        let mut history = RefactorHistory::new();
        
        let decision = RefactorDecision {
            timestamp: current_timestamp(),
            refactor_type: "ExtractMethod".to_string(),
            target_name: "process_data".to_string(),
            was_accepted: true,
            was_modified: false,
            user_feedback: None,
            context: RefactorContext {
                file_type: "rs".to_string(),
                file_size: 1000,
                project_type: "library".to_string(),
                language: "rust".to_string(),
            },
        };
        
        history.record(decision);
        
        let rate = history.get_success_rate("ExtractMethod");
        assert!(rate > 0.5);
    }
    
    #[test]
    fn test_stats() {
        let history = RefactorHistory::new();
        let stats = history.get_stats();
        
        assert_eq!(stats.total_refactors, 0);
        assert_eq!(stats.acceptance_rate, 0.0);
    }
}

/// Get the current Unix timestamp in seconds.
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
