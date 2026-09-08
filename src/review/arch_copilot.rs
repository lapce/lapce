//! Architecture Copilot — inspired by study8677/awesome-architecture's guided design pattern.
//!
//! Provides architecture knowledge base + structured design templates for
//! AI-assisted architecture decisions.
//!
//! ## Architecture
//!
//! ```text
//! ArchCopilot
//!   ├── KnowledgeBase  — architecture patterns & best practices
//!   ├── DesignTemplate — system design templates (microservices, monolith, etc.)
//!   ├── ReviewEngine   — architecture review & guidance
//!   └── SkillBridge    — integration with the skill system
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Architecture pattern category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArchPattern {
    Microservices,
    Monolith,
    EventDriven,
    Hexagonal,
    CQRS,
    EventSourcing,
    Layered,
    Pipeline,
    PeerToPeer,
    Serverless,
}

impl ArchPattern {
    /// All known patterns.
    pub fn all() -> Vec<Self> {
        vec![
            Self::Microservices,
            Self::Monolith,
            Self::EventDriven,
            Self::Hexagonal,
            Self::CQRS,
            Self::EventSourcing,
            Self::Layered,
            Self::Pipeline,
            Self::PeerToPeer,
            Self::Serverless,
        ]
    }

    /// Short description of the pattern.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Microservices => "Independent services communicating via APIs",
            Self::Monolith => "Single deployable unit with all functionality",
            Self::EventDriven => "Components communicate via events/messages",
            Self::Hexagonal => "Ports & adapters pattern for clean boundaries",
            Self::CQRS => "Separate read and write data models",
            Self::EventSourcing => "Store state as a sequence of events",
            Self::Layered => "Hierarchical layers with strict dependencies",
            Self::Pipeline => "Sequential processing stages",
            Self::PeerToPeer => "Decentralized nodes with equal roles",
            Self::Serverless => "Function-as-a-Service, auto-scaling",
        }
    }
}

/// A design decision template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignTemplate {
    /// Template name.
    pub name: String,
    /// Architecture pattern.
    pub pattern: ArchPattern,
    /// When to use this template.
    pub when_to_use: String,
    /// Key considerations.
    pub considerations: Vec<String>,
    /// Suggested ADR tags.
    pub adr_tags: Vec<String>,
    /// Quality attributes affected.
    pub quality_attributes: HashMap<String, String>,
}

impl DesignTemplate {
    /// Get built-in design templates.
    pub fn builtin_templates() -> Vec<Self> {
        vec![
            Self::microservice_template(),
            Self::monolith_template(),
            Self::event_driven_template(),
            Self::hexagonal_template(),
        ]
    }

    fn microservice_template() -> Self {
        let mut qa = HashMap::new();
        qa.insert("scalability".into(), "High (horizontal scaling)".into());
        qa.insert("deployability".into(), "High (independent deploys)".into());
        qa.insert("complexity".into(), "High (network, data consistency)".into());
        qa.insert("testability".into(), "Medium (contract testing needed)".into());

        Self {
            name: "microservice-arch".into(),
            pattern: ArchPattern::Microservices,
            when_to_use: "Multiple teams, independent scaling needs, polyglot tech stack".into(),
            considerations: vec![
                "Service boundaries must align with business domains".into(),
                "Inter-service communication: sync (REST/gRPC) vs async (events)".into(),
                "Data consistency: saga pattern vs distributed transactions".into(),
                "Observability: centralized logging, tracing, metrics".into(),
                "Deployment: container orchestration (K8s)".into(),
            ],
            adr_tags: vec!["architecture".into(), "microservices".into(), "api".into()],
            quality_attributes: qa,
        }
    }

    fn monolith_template() -> Self {
        let mut qa = HashMap::new();
        qa.insert("simplicity".into(), "High".into());
        qa.insert("performance".into(), "High (no network overhead)".into());
        qa.insert("scalability".into(), "Low (vertical scaling only)".into());
        qa.insert("maintainability".into(), "Medium (grows with codebase)".into());

        Self {
            name: "monolith-arch".into(),
            pattern: ArchPattern::Monolith,
            when_to_use: "Small team, early stage, simple domain, rapid prototyping".into(),
            considerations: vec![
                "Modular monolith: keep internal boundaries clean".into(),
                "Extract services gradually as domain complexity grows".into(),
                "Database remains a single point of contention".into(),
            ],
            adr_tags: vec!["architecture".into(), "monolith".into(), "modular".into()],
            quality_attributes: qa,
        }
    }

    fn event_driven_template() -> Self {
        let mut qa = HashMap::new();
        qa.insert("loose_coupling".into(), "Very High".into());
        qa.insert("scalability".into(), "High (async processing)".into());
        qa.insert("consistency".into(), "Eventual".into());
        qa.insert("debugging".into(), "Harder (async flow)".into());

        Self {
            name: "event-driven-arch".into(),
            pattern: ArchPattern::EventDriven,
            when_to_use: "Real-time processing, loose coupling, multiple consumers".into(),
            considerations: vec![
                "Choose event broker: Kafka vs RabbitMQ vs cloud-native".into(),
                "Event schema evolution: Avro, Protobuf, or JSON Schema".into(),
                "Exactly-once vs at-least-once delivery semantics".into(),
                "Dead letter queues for failed event processing".into(),
            ],
            adr_tags: vec!["architecture".into(), "events".into(), "async".into()],
            quality_attributes: qa,
        }
    }

    fn hexagonal_template() -> Self {
        let mut qa = HashMap::new();
        qa.insert("testability".into(), "Very High".into());
        qa.insert("maintainability".into(), "High".into());
        qa.insert("complexity".into(), "Medium (more interfaces)".into());
        qa.insert("flexibility".into(), "High (swap adapters)".into());

        Self {
            name: "hexagonal-arch".into(),
            pattern: ArchPattern::Hexagonal,
            when_to_use: "Complex business logic, high test requirements, multiple I/O adapters".into(),
            considerations: vec![
                "Core domain should have zero external dependencies".into(),
                "Ports = interfaces, adapters = implementations".into(),
                "Use DI for adapter injection".into(),
                "Keep domain model pure — no framework annotations".into(),
            ],
            adr_tags: vec!["architecture".into(), "hexagonal".into(), "ports-and-adapters".into()],
            quality_attributes: qa,
        }
    }
}

/// Architecture knowledge entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    /// Title.
    pub title: String,
    /// Brief description.
    pub summary: String,
    /// Detail content (markdown).
    pub detail: String,
    /// Related tags.
    pub tags: Vec<String>,
}

/// Built-in architecture knowledge base.
pub fn builtin_knowledge_base() -> Vec<KnowledgeEntry> {
    vec![
        KnowledgeEntry {
            title: "CAP Theorem".into(),
            summary: "A distributed system can only guarantee two of: Consistency, Availability, Partition Tolerance".into(),
            detail: "In distributed systems, you must choose between CP (consistent but unavailable during partitions) \
                     and AP (available but eventually consistent). CA is not realistic in distributed systems.".into(),
            tags: vec!["distributed-systems".into(), "theory".into(), "cap".into()],
        },
        KnowledgeEntry {
            title: "SOLID Principles".into(),
            summary: "Five design principles for maintainable OOP: SRP, OCP, LSP, ISP, DIP".into(),
            detail: "Single Responsibility, Open/Closed, Liskov Substitution, Interface Segregation, Dependency Inversion.".into(),
            tags: vec!["design".into(), "oop".into(), "principles".into()],
        },
        KnowledgeEntry {
            title: "CQRS Pattern".into(),
            summary: "Separate read and write data models for different optimization strategies".into(),
            detail: "Command Query Responsibility Segregation allows read and write sides to evolve independently. \
                     Writes use a write-optimized model, reads use a read-optimized (denormalized) model.".into(),
            tags: vec!["pattern".into(), "cqrs".into(), "database".into()],
        },
        KnowledgeEntry {
            title: "Strangler Fig Pattern".into(),
            summary: "Gradually replace legacy systems by routing functionality to new implementations incrementally".into(),
            detail: "Add a facade that routes requests to either legacy or new implementation. \
                     Gradually migrate features until legacy can be decommissioned.".into(),
            tags: vec!["pattern".into(), "migration".into(), "legacy".into()],
        },
        KnowledgeEntry {
            title: "12-Factor App".into(),
            summary: "Methodology for building SaaS applications with declarative setup and minimal divergence".into(),
            detail: "Key factors: codebase, dependencies, config, backing services, build/release/run, \
                     processes, port binding, concurrency, disposability, dev/prod parity, logs, admin processes.".into(),
            tags: vec!["methodology".into(), "saas".into(), "best-practices".into()],
        },
        KnowledgeEntry {
            title: "ADR Best Practices".into(),
            summary: "Keep ADRs short, focus on context and consequences, use status field for lifecycle".into(),
            detail: "Good ADRs: (1) State the context/problem clearly, (2) List alternatives considered, \
                     (3) Explain the decision rationale, (4) Document consequences (positive/negative/neutral). \
                     Use status: proposed → accepted → deprecated/superseded.".into(),
            tags: vec!["adr".into(), "documentation".into(), "architecture".into()],
        },
    ]
}

/// Architecture Copilot — guides architecture decisions with knowledge + templates.
pub struct ArchCopilot {
    /// Templates registry.
    templates: Vec<DesignTemplate>,
    /// Knowledge base.
    knowledge: Vec<KnowledgeEntry>,
}

impl ArchCopilot {
    /// Create a new ArchCopilot with built-in knowledge.
    pub fn new() -> Self {
        Self {
            templates: DesignTemplate::builtin_templates(),
            knowledge: builtin_knowledge_base(),
        }
    }

    /// Create with custom templates and knowledge.
    pub fn with_custom(
        templates: Vec<DesignTemplate>,
        knowledge: Vec<KnowledgeEntry>,
    ) -> Self {
        Self { templates, knowledge }
    }

    /// Get all available design templates.
    pub fn templates(&self) -> &[DesignTemplate] {
        &self.templates
    }

    /// Get a specific template by name.
    pub fn get_template(&self, name: &str) -> Option<&DesignTemplate> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// Get templates for a specific pattern.
    pub fn templates_for_pattern(&self, pattern: ArchPattern) -> Vec<&DesignTemplate> {
        self.templates.iter().filter(|t| t.pattern == pattern).collect()
    }

    /// Search knowledge base.
    pub fn search_knowledge(&self, query: &str) -> Vec<&KnowledgeEntry> {
        let lower = query.to_lowercase();
        self.knowledge
            .iter()
            .filter(|e| {
                e.title.to_lowercase().contains(&lower)
                    || e.summary.to_lowercase().contains(&lower)
                    || e.tags.iter().any(|t| t.contains(&lower))
            })
            .collect()
    }

    /// Get all knowledge entries.
    pub fn knowledge(&self) -> &[KnowledgeEntry] {
        &self.knowledge
    }

    /// Generate architecture review suggestions for a given context.
    pub fn review_suggestions(&self, context: &str) -> Vec<&'static str> {
        let lower = context.to_lowercase();
        let mut suggestions = Vec::new();

        if lower.contains("microservice") || lower.contains("service") {
            suggestions.push("Consider starting with a modular monolith and extract services as boundaries emerge");
            suggestions.push("If microservices are required, define service boundaries by business capability, not technical function");
        }
        if lower.contains("database") || lower.contains("data") {
            suggestions.push("Document data consistency requirements: strong vs eventual consistency");
            suggestions.push("Consider CQRS if read and write patterns differ significantly");
        }
        if lower.contains("scale") || lower.contains("perform") {
            suggestions.push("Define scalability requirements upfront: expected load, growth rate, peak patterns");
            suggestions.push("Consider event-driven architecture for async, scalable processing");
        }
        if lower.contains("legacy") || lower.contains("migrate") {
            suggestions.push("Use the Strangler Fig pattern for incremental migration");
            suggestions.push("Create ADRs for each migration step to track decisions");
        }

        suggestions
    }

    /// Template count.
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// Knowledge entry count.
    pub fn knowledge_count(&self) -> usize {
        self.knowledge.len()
    }
}

impl Default for ArchCopilot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_patterns() {
        let patterns = ArchPattern::all();
        assert_eq!(patterns.len(), 10);
        for p in &patterns {
            assert!(!p.description().is_empty());
        }
    }

    #[test]
    fn test_design_templates() {
        let templates = DesignTemplate::builtin_templates();
        assert_eq!(templates.len(), 4);
        assert!(templates.iter().any(|t| t.name == "microservice-arch"));
        assert!(templates.iter().any(|t| t.name == "hexagonal-arch"));
    }

    #[test]
    fn test_knowledge_base() {
        let kb = builtin_knowledge_base();
        assert!(!kb.is_empty());
        assert!(kb.iter().any(|e| e.title == "CAP Theorem"));
        assert!(kb.iter().any(|e| e.title == "SOLID Principles"));
    }

    #[test]
    fn test_arch_copilot_builtin() {
        let copilot = ArchCopilot::new();
        assert!(copilot.template_count() >= 4);
        assert!(copilot.knowledge_count() >= 6);
    }

    #[test]
    fn test_search_knowledge() {
        let copilot = ArchCopilot::new();
        let results = copilot.search_knowledge("cap");
        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.title == "CAP Theorem"));

        let no_results = copilot.search_knowledge("zzzznonexistent");
        assert!(no_results.is_empty());
    }

    #[test]
    fn test_get_template() {
        let copilot = ArchCopilot::new();
        let tmpl = copilot.get_template("microservice-arch");
        assert!(tmpl.is_some());
        assert_eq!(tmpl.unwrap().pattern, ArchPattern::Microservices);
    }

    #[test]
    fn test_templates_for_pattern() {
        let copilot = ArchCopilot::new();
        let micro = copilot.templates_for_pattern(ArchPattern::Microservices);
        assert_eq!(micro.len(), 1);
    }

    #[test]
    fn test_review_suggestions() {
        let copilot = ArchCopilot::new();
        let suggestions = copilot.review_suggestions("need microservices for scaling");
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains(&"modular monolith")));
    }

    #[test]
    fn test_arch_copilot_custom() {
        let templates = vec![DesignTemplate::microservice_template()];
        let knowledge = vec![KnowledgeEntry {
            title: "Custom".into(),
            summary: "Custom knowledge".into(),
            detail: "Details".into(),
            tags: vec!["custom".into()],
        }];

        let copilot = ArchCopilot::with_custom(templates, knowledge);
        assert_eq!(copilot.template_count(), 1);
        assert_eq!(copilot.knowledge_count(), 1);
    }
}