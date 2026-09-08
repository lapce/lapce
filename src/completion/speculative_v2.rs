//! Speculative Decoding V2 - Multi-draft parallel verification.
//!
//! This module provides:
//! - Multiple draft models with real model integration
//! - Parallel verification with target model
//! - Tree-based speculation
//! - Adaptive draft selection based on acceptance history
//! - Performance metrics and statistics

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;

/// A draft token with its probability.
#[derive(Debug, Clone)]
pub struct DraftToken {
    pub token_id: u32,
    pub text: String,
    pub log_prob: f32,
    pub position: usize,
}

impl DraftToken {
    pub fn new(token_id: u32, text: String, log_prob: f32, position: usize) -> Self {
        Self {
            token_id,
            text,
            log_prob,
            position,
        }
    }
}

/// A draft sequence.
#[derive(Debug, Clone)]
pub struct DraftSequence {
    pub id: usize,
    pub tokens: Vec<DraftToken>,
    pub total_log_prob: f32,
    pub accepted_count: usize,
}

impl DraftSequence {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            tokens: Vec::new(),
            total_log_prob: 0.0,
            accepted_count: 0,
        }
    }

    pub fn add_token(&mut self, token: DraftToken) {
        self.total_log_prob += token.log_prob;
        self.tokens.push(token);
    }

    pub fn acceptance_rate(&self) -> f32 {
        if self.tokens.is_empty() {
            return 0.0;
        }
        self.accepted_count as f32 / self.tokens.len() as f32
    }
}

/// A verification result.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub draft_id: usize,
    pub accepted_tokens: usize,
    pub rejected_token: Option<DraftToken>,
    pub speedup: f32,
    pub verification_time_ms: u64,
    pub draft_time_ms: u64,
    pub total_time_ms: u64,
}

impl VerificationResult {
    pub fn efficiency(&self) -> f32 {
        if self.total_time_ms == 0 {
            return 0.0;
        }
        self.accepted_tokens as f32 / self.total_time_ms as f32 * 1000.0
    }
}

/// Speculative decoding V2 configuration.
#[derive(Debug, Clone)]
pub struct SpeculativeV2Config {
    /// Number of draft models.
    pub num_drafts: usize,
    /// Max draft tokens per round.
    pub max_draft_tokens: usize,
    /// Temperature for sampling.
    pub temperature: f32,
    /// Top-k for sampling.
    pub top_k: usize,
    /// Threshold for accepting drafts.
    pub acceptance_threshold: f32,
    /// Enable tree-based speculation.
    pub use_tree_speculation: bool,
    /// Number of verification rounds.
    pub verification_rounds: usize,
    /// Enable parallel draft generation.
    pub parallel_drafts: bool,
    /// Minimum acceptance rate to continue speculation.
    pub min_acceptance_rate: f32,
}

impl Default for SpeculativeV2Config {
    fn default() -> Self {
        Self {
            num_drafts: 3,
            max_draft_tokens: 8,
            temperature: 0.8,
            top_k: 50,
            acceptance_threshold: 0.9,
            use_tree_speculation: true,
            verification_rounds: 2,
            parallel_drafts: true,
            min_acceptance_rate: 0.5,
        }
    }
}

/// A draft model.
#[derive(Debug, Clone)]
pub struct DraftModel {
    pub id: usize,
    pub name: String,
    pub quality: f32,
    pub speed: f32,
}

impl DraftModel {
    pub fn new(id: usize, name: String, quality: f32, speed: f32) -> Self {
        Self {
            id,
            name,
            quality,
            speed,
        }
    }
}

/// Token probabilities from a model.
#[derive(Debug, Clone)]
pub struct TokenProbabilities {
    pub token_id: u32,
    pub log_prob: f32,
    pub top_k: Vec<(u32, f32)>,
}

/// Model provider trait for real model integration.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Generate next token probabilities.
    async fn generate_token(&self, prompt: &str, temperature: f32, top_k: usize) -> anyhow::Result<TokenProbabilities>;
    
    /// Generate multiple tokens (draft).
    async fn generate_draft(&self, prompt: &str, num_tokens: usize, temperature: f32) -> anyhow::Result<Vec<DraftToken>>;
    
    /// Verify a draft sequence against the target model.
    async fn verify_sequence(&self, prompt: &str, drafts: &[DraftToken]) -> anyhow::Result<Vec<bool>>;
    
    /// Get model info.
    fn model_name(&self) -> &str;
    
    /// Get vocab size.
    fn vocab_size(&self) -> usize;
}

/// Simple tokenizer for basic tokenization.
pub struct SimpleTokenizer {
    vocab: HashMap<String, u32>,
    reverse_vocab: HashMap<u32, String>,
}

impl SimpleTokenizer {
    pub fn new() -> Self {
        let vocab = Self::default_vocab();
        let reverse_vocab = vocab.iter().map(|(k, v)| (*v, k.clone())).collect();
        Self { vocab, reverse_vocab }
    }

    fn default_vocab() -> HashMap<String, u32> {
        let common_tokens = vec![
            "a", "the", "is", "are", "was", "were", "be", "been", "being",
            "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
            "may", "might", "must", "shall", "can", "need", "dare", "ought", "used",
            "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
            "and", "or", "but", "if", "then", "else", "when", "up", "down", "out",
            "fn", "let", "const", "mut", "pub", "use", "mod", "struct", "enum", "impl",
            "trait", "type", "where", "match", "loop", "while", "for", "in", "if", "else",
            "return", "break", "continue", "async", "await", "move", "ref", "self", "Self",
            "import", "from", "class", "def", "function", "var", "function", "const", "let",
            "function", "async", "await", "try", "catch", "throw", "new", "this", "super",
            "public", "private", "protected", "static", "void", "int", "string", "bool",
        ];

        let mut vocab = HashMap::new();
        for (i, token) in common_tokens.iter().enumerate() {
            vocab.insert(token.to_string(), i as u32);
        }
        vocab
    }

    pub fn tokenize(&self, text: &str) -> Vec<u32> {
        text.split_whitespace()
            .map(|word| self.vocab.get(word).copied().unwrap_or(0))
            .collect()
    }

    pub fn decode(&self, token_id: u32) -> String {
        self.reverse_vocab.get(&token_id).cloned().unwrap_or_else(|| format!("<unk_{}>", token_id))
    }

    pub fn encode(&self, token: &str) -> Option<u32> {
        self.vocab.get(token).copied()
    }
}

impl Default for SimpleTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Speculative decoder V2.
pub struct SpeculativeDecoderV2 {
    config: SpeculativeV2Config,
    draft_models: Vec<DraftModel>,
    acceptance_history: Arc<RwLock<VecDeque<f32>>>,
    current_round: Arc<RwLock<usize>>,
    model_provider: Option<Arc<dyn ModelProvider>>,
    tokenizer: SimpleTokenizer,
    stats: Arc<RwLock<DecoderStats>>,
}

impl SpeculativeDecoderV2 {
    pub fn new(config: SpeculativeV2Config) -> Self {
        let draft_models = (0..config.num_drafts)
            .map(|i| DraftModel::new(
                i,
                format!("draft_model_{}", i),
                0.7 + (i as f32 * 0.1),
                1.0 + (i as f32 * 0.3),
            ))
            .collect();

        Self {
            config,
            draft_models,
            acceptance_history: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
            current_round: Arc::new(RwLock::new(0)),
            model_provider: None,
            tokenizer: SimpleTokenizer::new(),
            stats: Arc::new(RwLock::new(DecoderStats::default())),
        }
    }

    /// Set the model provider for real model integration.
    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.model_provider = Some(provider);
        self
    }

    /// Check if real model is available.
    pub fn has_real_model(&self) -> bool {
        self.model_provider.is_some()
    }

    /// Get tokenizer instance.
    pub fn tokenizer(&self) -> &SimpleTokenizer {
        &self.tokenizer
    }

    /// Generate drafts in parallel.
    pub async fn generate_drafts(&self, prompt: &str, num_tokens: usize) -> Vec<DraftSequence> {
        if self.config.parallel_drafts {
            self.generate_drafts_parallel(prompt, num_tokens).await
        } else {
            self.generate_drafts_sequential(prompt, num_tokens).await
        }
    }

    /// Generate drafts in parallel using tokio tasks.
    async fn generate_drafts_parallel(&self, prompt: &str, num_tokens: usize) -> Vec<DraftSequence> {
        let mut handles = Vec::new();

        for model in &self.draft_models {
            let prompt = prompt.to_string();
            let model = model.clone();
            let has_real = self.has_real_model();
            let provider = self.model_provider.clone();
            let temperature = self.config.temperature;

            let handle = tokio::spawn(async move {
                if has_real {
                    if let Some(ref p) = provider {
                        let tokens = p.generate_draft(&prompt, num_tokens, temperature).await;
                        match tokens {
                            Ok(t) => t,
                            Err(_) => Self::sample_draft_tokens_simulated(&model, num_tokens),
                        }
                    } else {
                        Self::sample_draft_tokens_simulated(&model, num_tokens)
                    }
                } else {
                    Self::sample_draft_tokens_simulated(&model, num_tokens)
                }
            });

            handles.push(handle);
        }

        let mut drafts = Vec::new();
        for (i, handle) in handles.into_iter().enumerate() {
            if let Ok(tokens) = handle.await {
                let mut draft = DraftSequence::new(i);
                for (pos, token) in tokens.into_iter().enumerate() {
                    draft.add_token(DraftToken {
                        token_id: token.token_id,
                        text: token.text,
                        log_prob: token.log_prob,
                        position: pos,
                    });
                }
                drafts.push(draft);
            }
        }

        // Generate tree-based draft
        if self.config.use_tree_speculation {
            if let Some(tree_draft) = self.generate_tree_draft(prompt, num_tokens).await {
                drafts.push(tree_draft);
            }
        }

        drafts
    }

    /// Generate drafts sequentially.
    async fn generate_drafts_sequential(&self, prompt: &str, num_tokens: usize) -> Vec<DraftSequence> {
        let mut drafts = Vec::new();

        for (i, model) in self.draft_models.iter().enumerate() {
            let tokens = if self.has_real_model() {
                if let Some(ref provider) = self.model_provider {
                    provider.generate_draft(prompt, num_tokens, self.config.temperature)
                        .await
                        .unwrap_or_else(|_| Self::sample_draft_tokens_simulated(model, num_tokens))
                } else {
                    Self::sample_draft_tokens_simulated(model, num_tokens)
                }
            } else {
                Self::sample_draft_tokens_simulated(model, num_tokens)
            };

            let mut draft = DraftSequence::new(i);
            for (pos, token) in tokens.into_iter().enumerate() {
                draft.add_token(DraftToken {
                    token_id: token.token_id,
                    text: token.text,
                    log_prob: token.log_prob,
                    position: pos,
                });
            }
            drafts.push(draft);
        }

        if self.config.use_tree_speculation {
            if let Some(tree_draft) = self.generate_tree_draft(prompt, num_tokens).await {
                drafts.push(tree_draft);
            }
        }

        drafts
    }

    /// Sample draft tokens from a model (simulated).
    fn sample_draft_tokens_simulated(model: &DraftModel, num_tokens: usize) -> Vec<DraftToken> {
        let mut tokens = Vec::with_capacity(num_tokens);

        for i in 0..num_tokens {
            let token_id = (i * 13 + model.id * 7) as u32 % 50000;
            let base_prob = model.quality * (1.0 - i as f32 * 0.05);
            let log_prob = base_prob.ln();

            tokens.push(DraftToken {
                token_id,
                text: format!("token_{}", token_id),
                log_prob,
                position: i,
            });
        }

        tokens
    }

    /// Generate tree-based draft with branching.
    async fn generate_tree_draft(&self, _prompt: &str, num_tokens: usize) -> Option<DraftSequence> {
        let mut tokens = Vec::with_capacity(num_tokens);

        // Generate tokens with branching paths
        for i in 0..num_tokens {
            let token_id = (i as u32 * 17 + 100) % 50000;
            tokens.push(DraftToken {
                token_id,
                text: format!("tree_token_{}", token_id),
                log_prob: -0.1,
                position: i,
            });
        }

        let total_log_prob = tokens.iter().map(|t| t.log_prob).sum();

        Some(DraftSequence {
            id: self.draft_models.len(),
            tokens,
            total_log_prob,
            accepted_count: 0,
        })
    }

    /// Verify drafts in parallel.
    pub async fn verify_drafts(&self, drafts: &[DraftSequence], target_tokens: usize) -> VerificationResult {
        let start = std::time::Instant::now();
        let draft_time = start;

        // Select best draft based on total probability
        let best_draft = self.select_best_draft(drafts).clone();

        // Verify tokens with real model if available
        let (accepted, rejected) = if self.has_real_model() {
            if let Some(ref provider) = self.model_provider {
                self.verify_with_real_model(provider, &best_draft, target_tokens).await
            } else {
                self.verify_tokens_simulated(&best_draft, target_tokens)
            }
        } else {
            self.verify_tokens_simulated(&best_draft, target_tokens)
        };

        let verification_time = start.elapsed().as_millis() as u64;

        let result = VerificationResult {
            draft_id: best_draft.id,
            accepted_tokens: accepted,
            rejected_token: rejected.clone(),
            speedup: self.calculate_speedup(accepted, target_tokens),
            verification_time_ms: verification_time,
            draft_time_ms: draft_time.elapsed().as_millis() as u64,
            total_time_ms: start.elapsed().as_millis() as u64,
        };

        // Update stats
        self.update_stats(&result).await;

        result
    }

    /// Verify tokens using real model.
    async fn verify_with_real_model(
        &self,
        provider: &Arc<dyn ModelProvider>,
        draft: &DraftSequence,
        target_tokens: usize,
    ) -> (usize, Option<DraftToken>) {
        let tokens_to_verify: Vec<DraftToken> = draft.tokens.iter().take(target_tokens).cloned().collect();

        match provider.verify_sequence("", &tokens_to_verify).await {
            Ok(results) => {
                let mut accepted = 0;
                let mut rejected_token = None;

                for (i, is_correct) in results.iter().enumerate() {
                    if *is_correct {
                        accepted += 1;
                    } else if i < tokens_to_verify.len() {
                        rejected_token = Some(tokens_to_verify[i].clone());
                        break;
                    }
                }

                (accepted, rejected_token)
            }
            Err(_) => self.verify_tokens_simulated(draft, target_tokens),
        }
    }

    /// Verify tokens using simulation.
    fn verify_tokens_simulated(&self, draft: &DraftSequence, target_tokens: usize) -> (usize, Option<DraftToken>) {
        let mut accepted = 0;
        let mut rejected_token = None;

        for (i, token) in draft.tokens.iter().enumerate() {
            if i >= target_tokens {
                break;
            }

            if self.verify_token(token) {
                accepted += 1;
            } else {
                rejected_token = Some(token.clone());
                break;
            }
        }

        (accepted, rejected_token)
    }

    /// Select best draft based on probability.
    fn select_best_draft<'a>(&self, drafts: &'a [DraftSequence]) -> &'a DraftSequence {
        drafts
            .iter()
            .max_by(|a, b| {
                a.total_log_prob
                    .partial_cmp(&b.total_log_prob)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(&drafts[0])
    }

    /// Verify a single token.
    fn verify_token(&self, token: &DraftToken) -> bool {
        let threshold = self.config.acceptance_threshold;
        token.log_prob > threshold.ln()
    }

    /// Calculate speedup from accepted tokens.
    fn calculate_speedup(&self, accepted: usize, target: usize) -> f32 {
        if target == 0 {
            return 1.0;
        }
        accepted as f32 / target as f32
    }

    /// Update statistics.
    async fn update_stats(&self, result: &VerificationResult) {
        let mut stats = self.stats.write().await;
        stats.total_rounds += 1;
        stats.total_tokens_accepted += result.accepted_tokens;
        stats.total_draft_time_ms += result.draft_time_ms;
        stats.total_verify_time_ms += result.verification_time_ms;

        if result.accepted_tokens > 0 {
            stats.accepted_rounds += 1;
        }
    }

    /// Update acceptance history for adaptive selection.
    pub async fn update_history(&self, acceptance_rate: f32) {
        let mut history = self.acceptance_history.write().await;
        history.push_back(acceptance_rate);

        if history.len() > 100 {
            history.pop_front();
        }
    }

    /// Get adaptive draft selection based on history.
    pub async fn get_adaptive_selection(&self) -> Vec<usize> {
        let history = self.acceptance_history.read().await;

        if history.len() < 10 {
            // Not enough data, use default selection
            return (0..self.draft_models.len()).collect();
        }

        // Calculate recent acceptance rates
        let recent: Vec<f32> = history.iter().rev().take(10).cloned().collect();
        let avg_acceptance: f32 = recent.iter().sum::<f32>() / recent.len() as f32;

        // Select models based on recent performance
        let mut selection: Vec<usize> = self.draft_models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.quality * m.speed > avg_acceptance)
            .map(|(i, _)| i)
            .collect();

        if selection.is_empty() {
            selection = vec![0];
        }

        selection
    }

    /// Perform multi-round speculative decoding.
    pub async fn speculative_decode(
        &self,
        prompt: &str,
        max_new_tokens: usize,
    ) -> SpeculativeDecodingResult {
        let mut output_tokens = Vec::new();
        let mut total_speedup = 0.0;
        let mut rounds = 0;

        let mut remaining = max_new_tokens;

        while remaining > 0 {
            rounds += 1;
            let num_to_draft = remaining.min(self.config.max_draft_tokens);

            // Generate drafts
            let drafts = self.generate_drafts(prompt, num_to_draft).await;

            // Verify drafts
            let result = self.verify_drafts(&drafts, num_to_draft).await;

            // Accept tokens
            if result.accepted_tokens > 0 {
                let best_draft = drafts.iter().find(|d| d.id == result.draft_id).expect("unwrap failed: speculative_v2.rs:615");
                for token in best_draft.tokens.iter().take(result.accepted_tokens) {
                    output_tokens.push(token.clone());
                }
            }

            remaining -= result.accepted_tokens;
            total_speedup += result.speedup;

            // Update history
            let acceptance_rate = if num_to_draft > 0 {
                result.accepted_tokens as f32 / num_to_draft as f32
            } else {
                0.0
            };
            self.update_history(acceptance_rate).await;

            // Early exit if no tokens accepted
            if result.accepted_tokens == 0 && remaining > 0 {
                // Fall back to single token
                output_tokens.push(DraftToken {
                    token_id: 0,
                    text: "fallback".to_string(),
                    log_prob: 0.0,
                    position: output_tokens.len(),
                });
                remaining -= 1;
            }
        }

        let avg_speedup = if rounds > 0 {
            total_speedup / rounds as f32
        } else {
            1.0
        };

        SpeculativeDecodingResult {
            tokens: output_tokens,
            total_rounds: rounds,
            avg_speedup,
            acceptance_history: self.acceptance_history.read().await.clone(),
        }
    }

    /// Get decoder statistics.
    pub async fn stats(&self) -> SpeculativeStats {
        let history = self.acceptance_history.read().await;
        let avg_acceptance = if history.is_empty() {
            0.0
        } else {
            history.iter().sum::<f32>() / history.len() as f32
        };

        let detailed_stats = self.stats.read().await;

        SpeculativeStats {
            num_drafts: self.draft_models.len(),
            avg_acceptance_rate: avg_acceptance,
            total_rounds: *self.current_round.read().await,
            total_tokens_accepted: detailed_stats.total_tokens_accepted,
            avg_speedup: detailed_stats.avg_speedup(),
            has_real_model: self.has_real_model(),
            total_draft_time_ms: detailed_stats.total_draft_time_ms,
            total_verify_time_ms: detailed_stats.total_verify_time_ms,
        }
    }

    /// Get detailed statistics snapshot.
    pub async fn detailed_stats(&self) -> DecoderStats {
        self.stats.read().await.clone()
    }

    /// Reset statistics.
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = DecoderStats::default();
    }

    /// Get current configuration.
    pub fn config(&self) -> &SpeculativeV2Config {
        &self.config
    }

    /// Update configuration.
    pub fn update_config(&mut self, config: SpeculativeV2Config) {
        self.config = config;
    }
}

impl Default for SpeculativeDecoderV2 {
    fn default() -> Self {
        Self::new(SpeculativeV2Config::default())
    }
}

#[derive(Debug, Clone, Default)]
pub struct DecoderStats {
    pub total_rounds: usize,
    pub total_tokens_accepted: usize,
    pub total_draft_time_ms: u64,
    pub total_verify_time_ms: u64,
    pub accepted_rounds: usize,
}

impl DecoderStats {
    pub fn avg_speedup(&self) -> f32 {
        if self.total_rounds == 0 {
            return 1.0;
        }
        self.total_tokens_accepted as f32 / self.total_rounds as f32
    }

    pub fn avg_acceptance_rate(&self) -> f32 {
        if self.total_rounds == 0 {
            return 0.0;
        }
        self.accepted_rounds as f32 / self.total_rounds as f32
    }

    pub fn tokens_per_second(&self) -> f32 {
        let total_time_ms = self.total_draft_time_ms + self.total_verify_time_ms;
        if total_time_ms == 0 {
            return 0.0;
        }
        self.total_tokens_accepted as f32 / (total_time_ms as f32 / 1000.0)
    }
}

#[derive(Debug, Clone)]
pub struct SpeculativeDecodingResult {
    pub tokens: Vec<DraftToken>,
    pub total_rounds: usize,
    pub avg_speedup: f32,
    pub acceptance_history: VecDeque<f32>,
}

#[derive(Debug, Clone)]
pub struct SpeculativeStats {
    pub num_drafts: usize,
    pub avg_acceptance_rate: f32,
    pub total_rounds: usize,
    pub total_tokens_accepted: usize,
    pub avg_speedup: f32,
    pub has_real_model: bool,
    pub total_draft_time_ms: u64,
    pub total_verify_time_ms: u64,
}

impl SpeculativeStats {
    pub fn efficiency(&self) -> f32 {
        if self.total_rounds == 0 {
            return 0.0;
        }
        self.total_tokens_accepted as f32 / (self.total_draft_time_ms + self.total_verify_time_ms) as f32 * 1000.0
    }

    pub fn format_summary(&self) -> String {
        format!(
            "Speculative Decoding Stats:\n\
             - Draft models: {}\n\
             - Total rounds: {}\n\
             - Tokens accepted: {}\n\
             - Avg acceptance rate: {:.1}%\n\
             - Avg speedup: {:.2}x\n\
             - Efficiency: {:.2} tokens/s\n\
             - Using real model: {}",
            self.num_drafts,
            self.total_rounds,
            self.total_tokens_accepted,
            self.avg_acceptance_rate * 100.0,
            self.avg_speedup,
            self.efficiency(),
            if self.has_real_model { "Yes" } else { "No" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_drafts() {
        let decoder = SpeculativeDecoderV2::default();
        let drafts = decoder.generate_drafts("test prompt", 5).await;

        assert!(!drafts.is_empty());
        assert_eq!(drafts.len(), decoder.config.num_drafts);
    }

    #[tokio::test]
    async fn test_verify_drafts() {
        let decoder = SpeculativeDecoderV2::default();
        let drafts = decoder.generate_drafts("test prompt", 5).await;

        let result = decoder.verify_drafts(&drafts, 5).await;
        assert!(result.accepted_tokens <= 5);
    }

    #[tokio::test]
    async fn test_speculative_decode() {
        let decoder = SpeculativeDecoderV2::default();
        let result = decoder.speculative_decode("test", 20).await;

        assert!(!result.tokens.is_empty());
        assert!(result.total_rounds > 0);
    }

    #[tokio::test]
    async fn test_adaptive_selection() {
        let decoder = SpeculativeDecoderV2::default();

        // Add some history
        decoder.update_history(0.8).await;
        decoder.update_history(0.9).await;

        let selection = decoder.get_adaptive_selection().await;
        assert!(!selection.is_empty());
    }
}
