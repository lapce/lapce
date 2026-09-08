use serde::{Deserialize, Serialize};

/// A requirement-level change recorded during a loop round.
///
/// Unlike code diffs (which show line-level changes), `SpecDelta` captures
/// the intent: what requirement was added, modified, or removed, and why.
///
/// This enables the Markdown report to include a "Spec Changes" section
/// alongside the compilation status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecDelta {
    /// What kind of change this is.
    pub action: SpecAction,
    /// The domain/module this spec change belongs to
    /// (e.g. "auth", "payments", "review").
    pub domain: String,
    /// Human-readable description of the requirement change.
    pub description: String,
}

/// The type of spec change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpecAction {
    /// A new requirement was introduced.
    Added,
    /// An existing requirement was modified.
    Modified,
    /// An existing requirement was removed.
    Removed,
}

impl SpecAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpecAction::Added => "ADDED",
            SpecAction::Modified => "MODIFIED",
            SpecAction::Removed => "REMOVED",
        }
    }
}

impl SpecDelta {
    pub fn new(action: SpecAction, domain: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            action,
            domain: domain.into(),
            description: description.into(),
        }
    }

    /// Format this delta as a single Markdown list item.
    pub fn to_markdown(&self) -> String {
        format!("- **{}** [{}]: {}", self.action.as_str(), self.domain, self.description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_delta_added() {
        let delta = SpecDelta::new(SpecAction::Added, "auth", "add OAuth2 login flow");
        assert_eq!(delta.action.as_str(), "ADDED");
        assert_eq!(delta.domain, "auth");
        assert!(delta.to_markdown().contains("ADDED"));
    }

    #[test]
    fn test_spec_delta_modified() {
        let delta = SpecDelta::new(SpecAction::Modified, "payments", "increase timeout to 30s");
        assert_eq!(delta.action.as_str(), "MODIFIED");
        assert!(delta.to_markdown().contains("MODIFIED"));
    }

    #[test]
    fn test_spec_delta_removed() {
        let delta = SpecDelta::new(SpecAction::Removed, "legacy", "drop v1 API support");
        assert_eq!(delta.action.as_str(), "REMOVED");
        assert!(delta.to_markdown().contains("REMOVED"));
    }

    #[test]
    fn test_spec_delta_markdown_format() {
        let delta = SpecDelta::new(SpecAction::Added, "review", "incremental scanning");
        let md = delta.to_markdown();
        assert!(md.starts_with("- **"));
        assert!(md.contains("review"));
        assert!(md.contains("incremental scanning"));
    }
}