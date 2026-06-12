use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineChoice {
    Local,
    Cloud,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    pub score: f32,
    pub reason: String,
    pub recommended_engine: EngineChoice,
    nesting_depth: f32,
    file_references: usize,
}

const ARCH_KEYWORDS: &[&str] = &[
    "refactor", "migration", "architecture", "redesign",
    "rewrite", "restructure", "reorganize", "pattern",
];

fn count_file_references(prompt: &str) -> usize {
    let mut count = 0;
    for ext in &["rs", "ts", "tsx", "js", "py", "go", "java", "cpp", "hpp"] {
        let pattern = format!(".{}", ext);
        if prompt.contains(&pattern) {
            count += prompt.matches(&pattern).count();
        }
    }
    count
}

fn estimate_nesting_depth(prompt: &str) -> f32 {
    let open_braces = prompt.matches('{').count();
    let open_parens = prompt.matches('(').count();
    let open_brackets = prompt.matches('[').count();
    (open_braces + open_parens / 2 + open_brackets) as f32 / 3.0
}

fn arch_keyword_weight(prompt: &str) -> f32 {
    let lower = prompt.to_lowercase();
    ARCH_KEYWORDS.iter()
        .filter(|kw| lower.contains(*kw))
        .count() as f32 * 0.12
}

pub fn estimate_complexity(prompt: &str, _context_size: usize) -> ComplexityScore {
    let char_count = prompt.chars().count().max(1);
    let token_estimate = char_count / 4;

    let token_factor = (token_estimate as f32 / 4096.0).min(1.0) * 0.3;
    let nesting_factor = (estimate_nesting_depth(prompt) / 8.0).min(1.0) * 0.25;
    let file_factor = (count_file_references(prompt) as f32 / 5.0).min(1.0) * 0.2;
    let arch_factor = arch_keyword_weight(prompt).min(0.25);

    let raw_score = token_factor + nesting_factor + file_factor + arch_factor;
    let score = raw_score.min(1.0).max(0.0);

    let (reason, recommended_engine) = if score < 0.4 {
        let parts = vec![
            if token_factor > 0.05 { Some(format!("{} tokens est.", token_estimate)) } else { None },
            if file_factor > 0.02 { Some(format!("{} files ref.", count_file_references(prompt))) } else { None },
        ].into_iter().flatten().collect::<Vec<_>>();
        let r = if parts.is_empty() { "simple".into() } else { parts.join(", ") };
        (r, EngineChoice::Local)
    } else if score < 0.7 {
        let parts = vec![
            if token_factor > 0.1 { Some(format!("{} tokens est.", token_estimate)) } else { None },
            if nesting_factor > 0.1 { Some(format!("nesting depth {:.0}", estimate_nesting_depth(prompt))) } else { None },
            if file_factor > 0.05 { Some(format!("{} files ref.", count_file_references(prompt))) } else { None },
        ].into_iter().flatten().collect::<Vec<_>>();
        (parts.join(", "), EngineChoice::Hybrid)
    } else {
        let parts = vec![
            if token_factor > 0.15 { Some("large context".into()) } else { None },
            if nesting_factor > 0.15 { Some("deep nesting".into()) } else { None },
            if file_factor > 0.1 { Some(format!("multi-file ({})", count_file_references(prompt))) } else { None },
            if arch_factor > 0.05 { Some("architectural".into()) } else { None },
        ].into_iter().flatten().collect::<Vec<_>>();
        (if parts.is_empty() { "complex".into() } else { parts.join(", ") }, EngineChoice::Hybrid)
    };

    ComplexityScore { score, reason, recommended_engine, nesting_depth: estimate_nesting_depth(prompt), file_references: count_file_references(prompt) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_prompt() {
        let cs = estimate_complexity("hello world", 100);
        assert!(cs.score < 0.4);
        assert_eq!(cs.recommended_engine, EngineChoice::Local);
    }

    #[test]
    fn test_architectural_refactor() {
        let cs = estimate_complexity("refactor the auth module to use a new architecture pattern with migration support", 5000);
        assert!(cs.score >= 0.5);
        assert!(cs.reason.contains("architectural") || cs.score > 0.4);
    }

    #[test]
    fn test_deeply_nested_code() {
        let code = "{fn main() { if x { for i in 0..10 { match v { _ => { let y = || { 1 }; }}}}}}";
        let cs = estimate_complexity(code, 2000);
        assert!(cs.nesting_depth_contribution() > 0.0);
    }

    #[test]
    fn test_multi_file_reference() {
        let prompt = "check src/main.rs and src/lib.rs and src/config.rs for errors";
        let cs = estimate_complexity(prompt, 3000);
        assert!(cs.file_count() >= 3);
    }

    impl ComplexityScore {
        pub fn nesting_depth_contribution(&self) -> f32 {
            self.nesting_depth
        }
        pub fn file_count(&self) -> usize {
            self.file_references
        }
    }
}
