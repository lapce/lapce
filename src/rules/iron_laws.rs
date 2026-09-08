//! Iron Laws + Red Flags enforcement for the LoopEngine.
//!
//! Inspired by Superpowers' Iron Laws pattern — declarative, capital-letter rules
//! paired with explicit "red flag" rationalizations that the LLM might use to
//! bypass each rule. The enforcer generates system-prompt snippets for the
//! Clarify phase so the model cannot "reason its way out" of following rules.
//!
//! ## Usage
//!
//! ```rust
//! use crate::rules::iron_laws::IronLawEnforcer;
//!
//! let enforcer = IronLawEnforcer::default();
//! let prompt = enforcer.to_system_prompt();
//! // Inject `prompt` into the Clarify phase system message.
//! ```

use serde::{Deserialize, Serialize};

/// A single Iron Law: a non-negotiable rule + its Red Flag table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronLaw {
    /// Short name (e.g. "TDD Iron Law").
    pub name: String,
    /// The capitalized rule text (e.g. "NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST").
    pub rule: String,
    /// Common rationalizations the LLM uses to skip this rule.
    pub red_flags: Vec<RedFlag>,
}

/// A single Red Flag entry: what the model says → why it's wrong.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedFlag {
    /// The LLM's rationalization excuse.
    pub excuse: String,
    /// Why this excuse is invalid.
    pub rebuttal: String,
}

impl IronLawEnforcer {
    /// Format the full rule set as a system-prompt snippet.
    pub fn to_system_prompt(&self) -> String {
        if self.laws.is_empty() {
            return String::new();
        }

        let mut out = String::from("\n## Iron Laws (NON-NEGOTIABLE)\n");
        out.push_str("The following rules MUST be followed. The 'Red Flags' list below each rule\n");
        out.push_str("shows rationalizations you might be tempted to use to skip the rule.\n");
        out.push_str("DO NOT use any of these excuses. The rules are absolute.\n\n");

        for (i, law) in self.laws.iter().enumerate() {
            out.push_str(&format!("### Iron Law {}: {}\n", i + 1, law.name));
            out.push_str(&format!("**{}**\n\n", law.rule));
            if !law.red_flags.is_empty() {
                out.push_str("| Rationalization | Why It's Wrong |\n");
                out.push_str("|----------------|----------------|\n");
                for rf in &law.red_flags {
                    out.push_str(&format!("| \"{}\" | {} |\n", rf.excuse, rf.rebuttal));
                }
                out.push('\n');
            }
        }

        out
    }
}

impl Default for IronLawEnforcer {
    fn default() -> Self {
        Self {
            laws: vec![
                // === 1. TDD Iron Law ===
                IronLaw {
                    name: "TDD Iron Law".into(),
                    rule: "NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST".into(),
                    red_flags: vec![
                        RedFlag {
                            excuse: "This change is too simple to need a test.".into(),
                            rebuttal: "Simple code breaks in production. A test takes 30 seconds.".into(),
                        },
                        RedFlag {
                            excuse: "I'll write tests after the implementation.".into(),
                            rebuttal: "Post-hoc tests confirm what the code does, not what it should do. Write the test first.".into(),
                        },
                    ],
                },
                // === 2. Evidence Iron Law ===
                IronLaw {
                    name: "Evidence Iron Law".into(),
                    rule: "NO COMPLETION CLAIM WITHOUT EVIDENCE".into(),
                    red_flags: vec![
                        RedFlag {
                            excuse: "The code compiles, so it must work.".into(),
                            rebuttal: "Compilation ≠ correctness. Run the test suite to verify.".into(),
                        },
                        RedFlag {
                            excuse: "I manually verified the logic is correct.".into(),
                            rebuttal: "Manual verification is not systematic. Run automated verification.".into(),
                        },
                    ],
                },
                // === 3. Systematic Iron Law ===
                IronLaw {
                    name: "Systematic First Iron Law".into(),
                    rule: "SYSTEMATIC APPROACH OVER AD-HOC FIXES".into(),
                    red_flags: vec![
                        RedFlag {
                            excuse: "Fixing the symptom is faster than finding the root cause.".into(),
                            rebuttal: "Symptom fixes cause the same bug to reappear. Find and fix the root cause.".into(),
                        },
                        RedFlag {
                            excuse: "This is a one-off issue that won't repeat.".into(),
                            rebuttal: "If you can't explain why it happened, you can't prevent it from recurring.".into(),
                        },
                    ],
                },
                // === 4. Review Iron Law ===
                IronLaw {
                    name: "Review Iron Law".into(),
                    rule: "NO MERGE WITHOUT REVIEW".into(),
                    red_flags: vec![
                        RedFlag {
                            excuse: "I reviewed the code myself as I wrote it.".into(),
                            rebuttal: "Self-review misses structural issues. Use the formal review process.".into(),
                        },
                        RedFlag {
                            excuse: "The tests pass, so review is unnecessary.".into(),
                            rebuttal: "Tests verify correctness, not quality. Architecture, security, and style need a fresh perspective.".into(),
                        },
                    ],
                },
                // === 5. 1% Principle ===
                IronLaw {
                    name: "1% Principle".into(),
                    rule: "EVEN WITH 1% PROBABILITY, INVOKE THE SKILL".into(),
                    red_flags: vec![
                        RedFlag {
                            excuse: "This doesn't require a formal skill.".into(),
                            rebuttal: "If a skill exists for the task, invoke it. Don't decide it's not needed.".into(),
                        },
                        RedFlag {
                            excuse: "I remember what the skill says, no need to re-read.".into(),
                            rebuttal: "Skills may have been updated. Read the current version every time.".into(),
                        },
                    ],
                },
            ],
        }
    }
}

/// Holds the set of Iron Laws and generates enforcement prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronLawEnforcer {
    /// The list of Iron Laws to enforce.
    pub laws: Vec<IronLaw>,
}

impl IronLawEnforcer {
    /// Create a new enforcer with the given laws.
    pub fn new(laws: Vec<IronLaw>) -> Self {
        Self { laws }
    }

    /// Check if all laws are present and return an enforcement summary.
    pub fn summary(&self) -> String {
        format!(
            "Iron Law Enforcer: {} laws active\n{}",
            self.laws.len(),
            self.to_system_prompt()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iron_law_system_prompt() {
        let enforcer = IronLawEnforcer::default();
        let prompt = enforcer.to_system_prompt();
        assert!(prompt.contains("TDD Iron Law"));
        assert!(prompt.contains("NO PRODUCTION CODE"));
        assert!(prompt.contains("Rationalization"));
        assert!(prompt.contains("Evidence Iron Law"));
    }

    #[test]
    fn test_iron_law_custom() {
        let law = IronLaw {
            name: "Custom Law".into(),
            rule: "CUSTOM RULE".into(),
            red_flags: vec![RedFlag {
                excuse: "excuse".into(),
                rebuttal: "rebuttal".into(),
            }],
        };
        let enforcer = IronLawEnforcer::new(vec![law]);
        let prompt = enforcer.to_system_prompt();
        assert!(prompt.contains("Custom Law"));
        assert!(prompt.contains("CUSTOM RULE"));
        assert!(prompt.contains("excuse"));
    }

    #[test]
    fn test_iron_law_nolaws() {
        let enforcer = IronLawEnforcer::new(vec![]);
        assert!(enforcer.to_system_prompt().is_empty());
    }

    #[test]
    fn test_summary() {
        let enforcer = IronLawEnforcer::default();
        let s = enforcer.summary();
        assert!(s.contains("5 laws active"));
    }
}