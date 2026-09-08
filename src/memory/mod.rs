//! Memory System - Cross-session persistent memory
//!
//! Based on Claude Code's memory approach, this module provides:
//! - Episodic memory (session events)
//! - Semantic memory (learned facts)
//! - Working memory (current context)
//! - Memory retrieval and relevance scoring
//! - Auto-Memory: cross-session project learning and prompt enrichment

pub mod auto_memory;

pub use auto_memory::{AutoMemory, ProjectMemory, UserIntent, MemoryKind, MemoryScope, MemoryEntry, detect_lang};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Memory types
#[derive(Debug, Clone)]
pub enum MemoryType {
    Episodic,
    Semantic,
    Working,
    Procedural,
}

/// Memory item
#[derive(Debug, Clone)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub memory_type: MemoryType,
    pub importance: f32,
    pub access_count: u32,
    pub last_accessed: Instant,
    pub created_at: Instant,
    pub expires_at: Option<Instant>,
    pub tags: Vec<String>,
    pub source: MemorySource,
}

#[derive(Debug, Clone)]
pub enum MemorySource {
    Conversation,
    CodeAnalysis,
    UserProvided,
    TaskExecution,
    SystemGenerated,
}

impl Memory {
    pub fn new(content: String, memory_type: MemoryType, source: MemorySource) -> Self {
        Self {
            id: uuid_v4(),
            content,
            memory_type,
            importance: 0.5,
            access_count: 0,
            last_accessed: Instant::now(),
            created_at: Instant::now(),
            expires_at: None,
            tags: Vec::new(),
            source,
        }
    }

    pub fn with_importance(mut self, importance: f32) -> Self {
        self.importance = importance.clamp(0.0, 1.0);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.expires_at = Some(Instant::now() + ttl);
        self
    }

    pub fn access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            Instant::now() > expires
        } else {
            false
        }
    }

    pub fn relevance_score(&self, query: &str) -> f32 {
        let mut score = 0.0;
        let query_lower = query.to_lowercase();
        let content_lower = self.content.to_lowercase();

        for tag in &self.tags {
            if query_lower.contains(&tag.to_lowercase()) {
                score += 0.3;
            }
        }

        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let content_words: Vec<&str> = content_lower.split_whitespace().collect();
        
        for word in &query_words {
            if content_words.contains(word) {
                score += 0.1;
            }
        }

        let age_hours = self.last_accessed.elapsed().as_secs_f32() / 3600.0;
        if age_hours < 1.0 {
            score += 0.2;
        } else if age_hours < 24.0 {
            score += 0.1;
        }

        if self.access_count > 10 {
            score += 0.2;
        }

        score += self.importance * 0.2;
        score.min(1.0)
    }
}

/// Working memory (short-term)
pub struct WorkingMemory {
    items: HashMap<String, Memory>,
    capacity: usize,
    current_context: Vec<String>,
}

impl WorkingMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: HashMap::new(),
            capacity,
            current_context: Vec::new(),
        }
    }

    pub fn store(&mut self, key: &str, content: String) {
        let memory = Memory::new(content, MemoryType::Working, MemorySource::Conversation);
        
        if self.items.len() >= self.capacity {
            if let Some(oldest) = self.items.keys().next().cloned() {
                self.items.remove(&oldest);
            }
        }
        
        self.items.insert(key.to_string(), memory);
        self.current_context.push(key.to_string());
    }

    pub fn retrieve(&mut self, key: &str) -> Option<&Memory> {
        if let Some(memory) = self.items.get_mut(key) {
            memory.access();
        }
        self.items.get(key)
    }

    pub fn get_context(&self) -> Vec<&Memory> {
        self.current_context.iter()
            .filter_map(|k| self.items.get(k))
            .collect()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current_context.clear();
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Semantic memory (long-term)
pub struct SemanticMemory {
    memories: Arc<RwLock<HashMap<String, Memory>>>,
    index: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl SemanticMemory {
    pub fn new() -> Self {
        Self {
            memories: Arc::new(RwLock::new(HashMap::new())),
            index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn store(&self, memory: Memory) {
        let id = memory.id.clone();
        
        self.memories.write().await.insert(id.clone(), memory);
        
        let mut index = self.index.write().await;
        if let Some(stored) = self.memories.read().await.get(&id) {
            for tag in &stored.tags {
                index.entry(tag.clone())
                    .or_insert_with(Vec::new)
                    .push(id.clone());
            }
        }
    }

    pub async fn retrieve(&self, query: &str, limit: usize) -> Vec<Memory> {
        let memories = self.memories.read().await;
        let mut scored: Vec<_> = memories.values()
            .filter(|m| !m.is_expired())
            .map(|m| {
                let mut memory = m.clone();
                memory.access();
                (m.relevance_score(query), memory)
            })
            .collect();
        
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter()
            .take(limit)
            .map(|(_, m)| m)
            .collect()
    }

    pub async fn retrieve_by_tag(&self, tag: &str) -> Vec<Memory> {
        let index = self.index.read().await;
        let memories = self.memories.read().await;
        
        index.get(tag)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| memories.get(id).cloned())
                    .filter(|m| !m.is_expired())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn get_all(&self) -> Vec<Memory> {
        self.memories.read().await.values()
            .filter(|m| !m.is_expired())
            .cloned()
            .collect()
    }

    pub async fn forget(&self, id: &str) -> bool {
        self.memories.write().await.remove(id).is_some()
    }

    pub async fn count(&self) -> usize {
        self.memories.read().await.len()
    }
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Episodic memory (events)
pub struct EpisodicMemory {
    events: Arc<RwLock<VecDeque<Memory>>>,
    max_events: usize,
}

impl EpisodicMemory {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::new())),
            max_events,
        }
    }

    pub async fn add_event(&self, content: String, importance: f32) {
        let mut memory = Memory::new(content, MemoryType::Episodic, MemorySource::Conversation)
            .with_importance(importance);
        memory.expires_at = Some(Instant::now() + Duration::from_secs(7 * 24 * 3600));
        
        let mut events = self.events.write().await;
        
        if events.len() >= self.max_events {
            events.pop_front();
        }
        
        events.push_back(memory);
    }

    pub async fn get_recent(&self, count: usize) -> Vec<Memory> {
        let events = self.events.read().await;
        events.iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    pub async fn search(&self, query: &str, limit: usize) -> Vec<Memory> {
        let events = self.events.read().await;
        let mut scored: Vec<_> = events.iter()
            .map(|m| (m.relevance_score(query), m.clone()))
            .collect();
        
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter()
            .take(limit)
            .map(|(_, m)| m)
            .collect()
    }

    pub async fn count(&self) -> usize {
        self.events.read().await.len()
    }
}

/// Unified memory manager
pub struct MemoryManager {
    pub working: Arc<RwLock<WorkingMemory>>,
    pub semantic: SemanticMemory,
    pub episodic: EpisodicMemory,
}

impl MemoryManager {
    pub fn new() -> Self {
        Self {
            working: Arc::new(RwLock::new(WorkingMemory::new(100))),
            semantic: SemanticMemory::new(),
            episodic: EpisodicMemory::new(1000),
        }
    }

    pub async fn remember(&self, content: String, memory_type: MemoryType, importance: f32) {
        match memory_type {
            MemoryType::Working => {
                self.working.write().await.store(&uuid_v4(), content);
            }
            MemoryType::Semantic => {
                let memory = Memory::new(content.clone(), MemoryType::Semantic, MemorySource::Conversation)
                    .with_importance(importance);
                self.semantic.store(memory).await;
            }
            MemoryType::Episodic => {
                self.episodic.add_event(content, importance).await;
            }
            MemoryType::Procedural => {
                let memory = Memory::new(content.clone(), MemoryType::Procedural, MemorySource::Conversation)
                    .with_importance(importance);
                self.semantic.store(memory).await;
            }
        }
    }

    pub async fn remember_important(&self, content: String, tags: Vec<String>) {
        let memory = Memory::new(content, MemoryType::Semantic, MemorySource::UserProvided)
            .with_importance(0.9)
            .with_tags(tags)
            .with_ttl(Duration::from_secs(365 * 24 * 3600));
        self.semantic.store(memory).await;
    }

    pub async fn recall(&self, query: &str, limit: usize) -> Vec<Memory> {
        self.semantic.retrieve(query, limit).await
    }

    pub async fn get_current_context(&self) -> Vec<Memory> {
        self.working.read().await.get_context()
            .into_iter()
            .cloned()
            .collect()
    }

    pub async fn update_context(&self, context: Vec<String>) {
        let mut working = self.working.write().await;
        working.clear();
        for (i, item) in context.into_iter().enumerate() {
            working.store(&format!("ctx_{}", i), item);
        }
    }

    pub async fn stats(&self) -> MemoryStats {
        MemoryStats {
            working_items: self.working.read().await.len(),
            semantic_memories: self.semantic.count().await,
            episodic_events: self.episodic.count().await,
        }
    }

    /// Save memory (placeholder for persistence)
    pub async fn save(&self) -> Result<(), String> {
        Ok(())
    }

    /// Add a message to memory
    pub async fn add_message(&self, role: &str, content: &str) {
        // Note: This is a placeholder - real implementation would need interior mutability
        let _ = (role, content);
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub working_items: usize,
    pub semantic_memories: usize,
    pub episodic_events: usize,
}

fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        (bytes[6] & 0x0f) | 0x40, bytes[7],
        (bytes[8] & 0x3f) | 0x80, bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}
