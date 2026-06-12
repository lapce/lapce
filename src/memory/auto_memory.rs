//! Auto-Memory 闁?cross-session persistent project learning.
//!
//! Automatically discovers project conventions and learns from successful
//! Agent sessions to enrich future prompts with contextual knowledge.
//! Inspired by Claude Code's CLAUDE.md auto-memory feature.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Memory taxonomy — 4 types + 2 scopes (Claude Code-inspired).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    /// User profile: role, skill level, preferences. Always private.
    User,
    /// Feedback: user corrected or validated an approach.
    Feedback,
    /// Project facts: who, what, deadlines, ownership.
    Project,
    /// Reference: external links, API endpoints, docs URLs.
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Private,
    Team,
}

/// A single memory entry with scope + why (so edge cases can be judged later).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub kind: MemoryKind,
    pub scope: MemoryScope,
    pub body: String,
    pub why: String,
    pub source: String,
    pub created_at: String,
}

impl MemoryEntry {
    pub fn new(
        kind: MemoryKind,
        scope: MemoryScope,
        body: impl Into<String>,
        why: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            scope,
            body: body.into(),
            why: why.into(),
            source: source.into(),
            created_at: chrono_utc_now(),
        }
    }
}

/// Persistent project memory that survives across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    /// When this memory was first created.
    pub discovered_at: String,
    /// Last updated timestamp.
    pub updated_at: String,
    /// Build command discovered (e.g., "cargo build", "npm run build", "make").
    pub build_cmd: Option<String>,
    /// Test command discovered.
    pub test_cmd: Option<String>,
    /// Linter / format command.
    pub linter: Option<String>,
    /// Package manager used.
    pub package_manager: Option<String>,
    /// Key architectural notes discovered from file structure.
    pub architecture_notes: Vec<String>,
    /// Common error patterns and their fixes learned from sessions.
    pub common_errors: HashMap<String, String>,
    /// Naming conventions observed.
    pub naming_conventions: Vec<String>,
    /// Important config files found.
    pub config_files: Vec<String>,
    /// Total sessions learned from.
    pub session_count: u32,
    /// Most recent user intent (primary + follow-ups), survives compression.
    pub recent_intents: Vec<UserIntent>,
    /// 4-type taxonomy entries (user / feedback / project / reference).
    pub entries: Vec<MemoryEntry>,
}

const MAX_ENTRIES: usize = 120;

/// A single user intent, anchored as first-class memory so compression never eats it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIntent {
    /// Ordinal in this session (1 = primary).
    pub rank: u32,
    /// Raw user text (never summarized).
    pub text: String,
    /// Timestamp when recorded.
    pub recorded_at: String,
    /// Language tag: "zh" or "en".
    pub lang: String,
    /// Whether this was the session-opening primary intent.
    pub is_primary: bool,
}

impl UserIntent {
    pub fn new(rank: u32, text: impl Into<String>, is_primary: bool) -> Self {
        let text = text.into();
        Self {
            rank,
            lang: detect_lang(&text).to_string(),
            text,
            recorded_at: chrono_utc_now(),
            is_primary,
        }
    }
}

/// Heuristic language detector 闁?zh if any CJK ideograph present, else en.
pub fn detect_lang(text: &str) -> &'static str {
    for c in text.chars() {
        let cp = c as u32;
        if (0x4E00..=0x9FFF).contains(&cp)
            || (0x3400..=0x4DBF).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp)
        {
            return "zh";
        }
    }
    "en"
}

const MAX_INTENTS: usize = 12;

impl Default for ProjectMemory {
    fn default() -> Self {
        let now = chrono_utc_now();
        Self {
            discovered_at: now.clone(),
            updated_at: now,
            build_cmd: None,
            test_cmd: None,
            linter: None,
            package_manager: None,
            architecture_notes: Vec::new(),
            common_errors: HashMap::new(),
            naming_conventions: Vec::new(),
            config_files: Vec::new(),
            session_count: 0,
            recent_intents: Vec::new(),
            entries: Vec::new(),
        }
    }
}

/// The main auto-memory engine.
///
/// Scans a project root to discover build tooling, language, and conventions,
/// then persists findings so future sessions start with rich context.
pub struct AutoMemory {
    memory_dir: PathBuf,
    current: ProjectMemory,
    dirty: bool,
}

impl AutoMemory {
    /// Create a new `AutoMemory` instance for the given project root.
    ///
    /// If an existing `project.json` is found under `.dscarp/memory/` it will
    /// be loaded; otherwise a fresh default is created.
    pub fn new(project_root: &Path) -> Self {
        let memory_dir = project_root.join(".dscarp").join("memory");

        if !memory_dir.exists() {
            let _ = fs::create_dir_all(&memory_dir);
        }

        let current = Self::load_or_default(&memory_dir);
        Self {
            memory_dir,
            current,
            dirty: false,
        }
    }

    /// Record a user intent as first-class memory 闁?never summarized, never dropped.
    pub fn record_intent(&mut self, text: impl Into<String>, is_primary: bool) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        let rank = self.current.recent_intents.len() as u32 + 1;
        let intent = UserIntent::new(rank, text, is_primary);
        self.current.recent_intents.push(intent);
        if self.current.recent_intents.len() > MAX_INTENTS {
            let overflow = self.current.recent_intents.len() - MAX_INTENTS;
            self.current.recent_intents.drain(0..overflow);
        }
        self.dirty = true;
    }

    /// Snapshot of currently anchored intents (sorted by rank asc).
    pub fn list_intents(&self) -> &[UserIntent] {
        &self.current.recent_intents
    }

    /// Drop all anchored intents (new session).
    pub fn clear_intents(&mut self) {
        if !self.current.recent_intents.is_empty() {
            self.current.recent_intents.clear();
            self.dirty = true;
        }
    }

    /// Add a 4-type taxonomy memory entry.
    pub fn add_entry(
        &mut self,
        kind: MemoryKind,
        scope: MemoryScope,
        body: impl Into<String>,
        why: impl Into<String>,
        source: impl Into<String>,
    ) {
        let entry = MemoryEntry::new(kind, scope, body, why, source);
        self.current.entries.push(entry);
        if self.current.entries.len() > MAX_ENTRIES {
            let overflow = self.current.entries.len() - MAX_ENTRIES;
            self.current.entries.drain(0..overflow);
        }
        self.dirty = true;
    }

    /// All entries filtered by scope (Private or Team).
    pub fn entries_for_scope(&self, scope: MemoryScope) -> impl Iterator<Item = &MemoryEntry> {
        self.current.entries.iter().filter(move |e| e.scope == scope)
    }

    /// All entries filtered by kind.
    pub fn entries_for_kind(&self, kind: MemoryKind) -> impl Iterator<Item = &MemoryEntry> {
        self.current.entries.iter().filter(move |e| e.kind == kind)
    }

    /// Remove entries whose body matches the given prefix (used to correct).
    pub fn remove_entry_by_prefix(&mut self, prefix: &str) {
        let len_before = self.current.entries.len();
        self.current.entries.retain(|e| !e.body.starts_with(prefix));
        if self.current.entries.len() != len_before {
            self.dirty = true;
        }
    }

    /// Load existing memory from disk, or return a fresh default.
    fn load_or_default(memory_dir: &Path) -> ProjectMemory {
        let path = memory_dir.join("project.json");
        match fs::read_to_string(&path) {
            Ok(data) => match serde_json::from_str::<ProjectMemory>(&data) {
                Ok(mem) => {
                    debug!(path = %path.display(), "loaded existing project memory");
                    mem
                }
                Err(e) => {
                    debug!(error = %e, "failed to parse project memory, using default");
                    ProjectMemory::default()
                }
            },
            Err(_) => ProjectMemory::default(),
        }
    }

    /// Scan the project root and discover tooling / conventions.
    ///
    /// Returns the number of new discoveries made during this scan.
    pub fn discover(&mut self) -> usize {
        let root: PathBuf = self
            .memory_dir
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut count = 0;

        // --- Detect project type from manifest files ---
        if root.join("Cargo.toml").exists() {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("cargo build".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("cargo test".into());
                count += 1;
            }
            if self.current.linter.is_none() {
                self.current.linter = Some("cargo clippy".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("cargo".into());
                count += 1;
            }
            self.current.architecture_notes.push("Rust project with Cargo".into());
        }

        if root.join("package.json").exists() {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("npm run build".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("npm test".into());
                count += 1;
            }
            if self.current.linter.is_none() {
                self.current.linter = Some("eslint".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("npm".into());
                count += 1;
            }
            self.current.architecture_notes.push("Node.js / JavaScript project".into());
        }

        if root.join("Makefile").exists() || root.join("makefile").exists() {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("make".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("make test".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("make".into());
                count += 1;
            }
            self.current.architecture_notes.push("C/C++ or generic Makefile project".into());
        }

        if root.join("go.mod").exists() {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("go build ./...".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("go test -v ./...".into());
                count += 1;
            }
            if self.current.linter.is_none() {
                self.current.linter = Some("golangci-lint run".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("go modules".into());
                count += 1;
            }
            self.current.architecture_notes.push("Go project with modules".into());
        }

        if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("pip install -e .".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("pytest".into());
                count += 1;
            }
            if self.current.linter.is_none() {
                self.current.linter = Some("ruff check".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("pip/poetry".into());
                count += 1;
            }
            self.current.architecture_notes.push("Python project".into());
        }

        // Detect .sln / .csproj for .NET
        let has_sln = fs::read_dir(&root)
            .ok()
            .and_then(|entries| {
                entries.filter_map(Result::ok).find(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "sln")
                        .unwrap_or(false)
                })
            })
            .is_some();
        let has_csproj = root.join("*.csproj").exists()
            || fs::read_dir(&root)
                .ok()
                .and_then(|entries| {
                    entries.filter_map(Result::ok).find(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "csproj")
                            .unwrap_or(false)
                    })
                })
                .is_some();

        if has_sln || has_csproj {
            if self.current.build_cmd.is_none() {
                self.current.build_cmd = Some("dotnet build".into());
                count += 1;
            }
            if self.current.test_cmd.is_none() {
                self.current.test_cmd = Some("dotnet test".into());
                count += 1;
            }
            if self.current.package_manager.is_none() {
                self.current.package_manager = Some("dotnet nuget".into());
                count += 1;
            }
            self.current.architecture_notes.push(".NET / C# project".into());
        }

        // --- Scan for config files ---
        let known_configs = [
            ".editorconfig",
            ".eslintrc.js",
            ".eslintrc.json",
            ".eslintrc.yml",
            ".prettierrc",
            ".prettierrc.json",
            "rustfmt.toml",
            ".rustfmt.toml",
            "pyrightconfig.json",
            "pyproject.toml",
            ".flake8",
            "tsconfig.json",
            ".clang-format",
            ".clang-tidy",
            "Cargo.toml",
            "package.json",
        ];
        for cfg in &known_configs {
            let path = root.join(cfg);
            if path.exists() && !self.current.config_files.contains(&cfg.to_string()) {
                self.current.config_files.push(cfg.to_string());
                count += 1;
            }
        }

        // --- Detect architecture from directory layout ---
        let arch_dirs = [
            ("src/", "Standard src/ source layout"),
            ("lib/", "Library-style lib/ layout"),
            ("app/", "Application app/ layout"),
            ("internal/", "Go-style internal/ package"),
            ("cmd/", "Go-style cmd/ entry points"),
            ("tests/", "Dedicated tests/ directory"),
            ("docs/", "Documentation docs/ directory"),
            ("scripts/", "Build/deploy scripts/ directory"),
        ];
        for (dir, note) in &arch_dirs {
            if root.join(dir).is_dir()
                && !self.current.architecture_notes.iter().any(|n| n.contains(note))
            {
                self.current.architecture_notes.push((*note).to_string());
                count += 1;
            }
        }

        // --- Infer naming conventions from file names ---
        self.detect_naming_conventions(&root);

        self.dirty = count > 0;
        self.current.updated_at = chrono_utc_now();
        info!(discoveries = count, "project discovery complete");
        count
    }

    /// Heuristically detect naming conventions by sampling filenames.
    fn detect_naming_conventions(&mut self, root: &Path) {
        let snake_count = RefCellCount(std::cell::RefCell::new(0));
        let camel_count = RefCellCount(std::cell::RefCell::new(0));
        let kebab_count = RefCellCount(std::cell::RefCell::new(0));
        let pascal_count = RefCellCount(std::cell::RefCell::new(0));

        let _ = fs::read_dir(root).map(|entries| {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains('.') && !name.starts_with('.') {
                    let stem = name.split('.').next().unwrap_or(&name);

                    // Count patterns
                    if is_snake_case(stem) {
                        *snake_count.0.borrow_mut() += 1;
                    }
                    if is_camel_case(stem) {
                        *camel_count.0.borrow_mut() += 1;
                    }
                    if stem.contains('-') {
                        *kebab_count.0.borrow_mut() += 1;
                    }
                    if is_pascal_case(stem) {
                        *pascal_count.0.borrow_mut() += 1;
                    }
                }
            }
        });

        let snake = *snake_count.0.borrow();
        let camel = *camel_count.0.borrow();
        let kebab = *kebab_count.0.borrow();
        let pascal = *pascal_count.0.borrow();
        let total = snake + camel + kebab + pascal;

        if total > 0 {
            let threshold = total / 3; // at least ~33% of files
            if snake >= threshold && snake >= camel && snake >= pascal {
                self.current.naming_conventions.push("snake_case for file/module names".into());
            }
            if camel >= threshold && camel >= snake && camel >= pascal {
                self.current.naming_conventions.push("camelCase for identifiers".into());
            }
            if pascal >= threshold && pascal >= snake && pascal >= camel {
                self.current.naming_conventions.push("PascalCase for type names".into());
            }
            if kebab >= threshold {
                self.current.naming_conventions.push("kebab-case for config/file names".into());
            }
        }
    }

    /// Learn from a completed Agent session output.
    ///
    /// On success, extracts error闁愁偅濮乮x patterns and stores them so that
    /// future sessions can avoid repeating mistakes.
    pub fn learn_from_session(&mut self, session_output: &str, success: bool) {
        self.current.updated_at = chrono_utc_now();

        if !success {
            debug!("session was not successful; skipping learning");
            return;
        }

        self.current.session_count += 1;

        // Extract Rust compiler errors like error[E0XXX]
        let re_error =
            Regex::new(r"error\[E(\w+)\][^\n]*").expect("error regex compiled successfully");
        let re_fix = Regex::new(r"(?i)(?:fixed|resolved|solved)\s+(?:by|with|through)[^\n]{0,200}")
                .expect("fix regex compiled successfully");

        for cap in re_error.captures_iter(session_output) {
            let pattern = cap[0].trim().to_string();

            // Look for a fix description nearby (within next 500 chars)
            let after_start = cap.get(0).expect("missing capture group: auto_memory.rs:393").end();
            let tail = &session_output[after_start..];
            let search_window = &tail[..tail.len().min(500)];

            if let Some(fix_cap) = re_fix.captures(search_window) {
                let fix = fix_cap[0].trim().to_string();
                self.current.common_errors.insert(pattern, fix);
            } else {
                // Store even without a known fix, as a known issue
                self.current
                    .common_errors
                    .entry(pattern)
                    .or_insert_with(|| "(fix pattern not yet extracted)".into());
            }
        }

        // Also look for generic "fixed by 闁? patterns not tied to specific errors
        let re_generic_fix =
            Regex::new(r"(?i)(?:fixed|resolved|solved)\s+(?:by\s+)(?:adding|removing|changing|updating|replacing)[^\n.]{10,200}")
                .expect("generic-fix regex compiled successfully");
        for cap in re_generic_fix.captures_iter(session_output) {
            let desc = cap[0].trim().to_string();
            let key = format!("pattern_{}", self.current.common_errors.len() + 1);
            self.current.common_errors.entry(key).or_insert(desc);
        }

        self.dirty = true;
        debug!(
            errors_learned = self.current.common_errors.len(),
            sessions = self.current.session_count,
            "learning from session complete"
        );
    }

    /// Enrich a user prompt with all learned project context.
    ///
    /// Prepends build commands, architecture notes, naming conventions,
    /// and known error patterns so the Agent starts each session informed.
    pub fn enrich_prompt(&self, prompt: &str) -> String {
        let mut ctx = String::from("[Project Context]\n");

        // Build / Test / Lint line
        let parts: Vec<String> = [
            self.current.build_cmd.as_deref().map(|b| format!("Build: {b}")),
            self.current.test_cmd.as_deref().map(|t| format!("Test: {t}")),
            self.current.linter.as_deref().map(|l| format!("Linter: {l}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !parts.is_empty() {
            ctx.push_str(&parts.join(" | "));
            ctx.push('\n');
        }

        if let Some(ref pm) = self.current.package_manager {
            ctx.push_str(&format!("Package Manager: {pm}\n"));
        }

        // Architecture
        if !self.current.architecture_notes.is_empty() {
            ctx.push_str("\nArchitecture:\n");
            for note in &self.current.architecture_notes {
                ctx.push_str(&format!("  - {note}\n"));
            }
        }

        // Naming conventions
        if !self.current.naming_conventions.is_empty() {
            ctx.push_str("\nConventions:\n");
            for conv in &self.current.naming_conventions {
                ctx.push_str(&format!("  - {conv}\n"));
            }
        }

        // Config files
        if !self.current.config_files.is_empty() {
            ctx.push_str("\nConfig Files:\n");
            for cfg in &self.current.config_files {
                ctx.push_str(&format!("  - {cfg}\n"));
            }
        }

        // Known issues / error patterns
        if !self.current.common_errors.is_empty() {
            ctx.push_str("\nKnown Issues / Fixes:\n");
            for (err, fix) in &self.current.common_errors {
                ctx.push_str(&format!("  - {err} 闁?{fix}\n"));
            }
        }

        // Anchored user intents — never summarized, never dropped by compression
        if !self.current.recent_intents.is_empty() {
            ctx.push_str("\n[User Intents — anchor, do NOT compress these]\n");
            for intent in &self.current.recent_intents {
                let role_label = if intent.is_primary { "PRIMARY" } else { "followup" };
                let lang_label = if intent.lang == "zh" { "zh" } else { "en" };
                let line = format!("  [{}·{}] {}\n", role_label, lang_label, intent.text);
                ctx.push_str(&line);
            }
        }

        // 4-type taxonomy memory entries
        for kind in [MemoryKind::User, MemoryKind::Feedback, MemoryKind::Project, MemoryKind::Reference] {
            let scope_letter = |s: MemoryScope| if s == MemoryScope::Team { "team" } else { "priv" };
            let header = match kind {
                MemoryKind::User => "\n[User Profile]\n",
                MemoryKind::Feedback => "\n[Feedback]\n",
                MemoryKind::Project => "\n[Project Facts]\n",
                MemoryKind::Reference => "\n[References]\n",
            };
            let filtered: Vec<&MemoryEntry> = self
                .current
                .entries
                .iter()
                .filter(|e| e.kind == kind)
                .collect();
            if !filtered.is_empty() {
                ctx.push_str(header);
                for entry in filtered {
                    let why = if entry.why.is_empty() {
                        String::new()
                    } else {
                        format!("  // why: {}\n", entry.why)
                    };
                    let line = format!("  [{}] {}\n", scope_letter(entry.scope), entry.body);
                    ctx.push_str(&line);
                    if !why.is_empty() {
                        ctx.push_str(&why);
                    }
                }
            }
        }

        ctx.push_str(&format!(
            "\n(Sessions learned from: {})\n",
            self.current.session_count
        ));
        ctx.push_str("---\n\n");
        ctx.push_str(prompt);
        ctx
    }

    /// Persist the current state to `{memory_dir}/project.json`.
    pub fn save(&self) -> Result<()> {
        let path = self.memory_dir.join("project.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.current)
            .context("failed to serialize ProjectMemory")?;
        fs::write(&path, json)
            .with_context(|| format!("failed to write {}", path.display()))?;
        debug!(path = %path.display(), "project memory saved");
        Ok(())
    }

    /// Static constructor: load persisted memory from disk.
    ///
    /// Reads `{project_root}/.dscarp/memory/project.json`.
    pub fn load(project_root: &Path) -> Result<Self> {
        let memory_dir = project_root.join(".dscarp").join("memory");
        let path = memory_dir.join("project.json");
        let data = fs::read_to_string(&path)
            .with_context(|| format!("no project memory found at {}", path.display()))?;
        let current: ProjectMemory = serde_json::from_str(&data)
            .context("failed to deserialize ProjectMemory")?;
        Ok(Self {
            memory_dir,
            current,
            dirty: false,
        })
    }

    /// Return a reference to the memory directory path.
    pub fn memory_dir(&self) -> &Path {
        &self.memory_dir
    }

    /// Return a human-readable summary of what has been learned.
    pub fn summary(&self) -> String {
        let m = &self.current;
        let mut s = String::new();
        s.push_str(&format!("Project Memory (since {})\n", m.discovered_at));
        s.push_str(&format!("  Last updated : {}\n", m.updated_at));
        s.push_str(&format!(
            "  Sessions      : {}\n",
            m.session_count
        ));

        if let Some(ref b) = m.build_cmd {
            s.push_str(&format!("  Build cmd     : {b}\n"));
        }
        if let Some(ref t) = m.test_cmd {
            s.push_str(&format!("  Test cmd      : {t}\n"));
        }
        if let Some(ref l) = m.linter {
            s.push_str(&format!("  Linter        : {l}\n"));
        }
        if let Some(ref pm) = m.package_manager {
            s.push_str(&format!("  Pkg manager   : {pm}\n"));
        }

        s.push_str(&format!(
            "  Arch notes    : {}\n",
            m.architecture_notes.len()
        ));
        s.push_str(&format!(
            "  Known errors  : {}\n",
            m.common_errors.len()
        ));
        s.push_str(&format!(
            "  Conventions   : {}\n",
            m.naming_conventions.len()
        ));
        s.push_str(&format!(
            "  Config files  : {}\n",
            m.config_files.len()
        ));

        s
    }

    /// Whether unsaved changes exist.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Immutable reference to the underlying [`ProjectMemory`].
    pub fn memory(&self) -> &ProjectMemory {
        &self.current
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the current UTC timestamp as an ISO-8601 string.
fn chrono_utc_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ---- Naming-convention helpers ----

struct RefCellCount(std::cell::RefCell<usize>);

fn is_snake_case(s: &str) -> bool {
    s.contains('_') && s.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

fn is_camel_case(s: &str) -> bool {
    !s.contains('_')
        && !s.contains('-')
        && s.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
        && s.chars().any(|c| c.is_ascii_uppercase())
}

fn is_pascal_case(s: &str) -> bool {
    !s.contains('_')
        && !s.contains('-')
        && s.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && s.chars().any(|c| c.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_new_creates_memory_dir() {
        let dir = TempDir::new().unwrap();
        let am = AutoMemory::new(dir.path());
        assert!(am.memory_dir().exists());
    }

    #[test]
    fn test_enrich_prompt_adds_context() {
        let mut am = AutoMemory::new(Path::new("/tmp/fake"));
        am.current.build_cmd = Some("cargo build".into());
        am.current.test_cmd = Some("cargo test".into());
        am.current.package_manager = Some("cargo".into());

        let enriched = am.enrich_prompt("fix the bug");
        assert!(enriched.contains("[Project Context]"));
        assert!(enriched.contains("cargo build"));
        assert!(enriched.contains("cargo test"));
        assert!(enriched.ends_with("fix the bug"));
    }

    #[test]
    fn test_learn_from_session_extracts_errors() {
        let mut am = AutoMemory::new(Path::new("/tmp/fake2"));
        let output = r#"
error[E0599]: no method named `foo` found for struct `Bar`
  --> src/main.rs:10:5
   |
10 |     x.foo()
   |        ^^^ method not found in `Bar`

fixed by adding `impl Bar { fn foo(&self) {} }`
"#;
        am.learn_from_session(output, true);
        assert!(!am.current.common_errors.is_empty());
        assert_eq!(am.current.session_count, 1);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut am = AutoMemory::new(dir.path());
        am.current.build_cmd = Some("cargo build".into());
        am.current.architecture_notes.push("Rust project".into());
        am.save().unwrap();

        let loaded = AutoMemory::load(dir.path()).unwrap();
        assert_eq!(loaded.current.build_cmd, Some("cargo build".into()));
        assert_eq!(loaded.current.architecture_notes.len(), 1);
    }

    #[test]
    fn test_summary_includes_fields() {
        let mut am = AutoMemory::new(Path::new("/tmp/fake3"));
        am.current.build_cmd = Some("make".into());
        am.current.session_count = 5;
        let sum = am.summary();
        assert!(sum.contains("make"));
        assert!(sum.contains("5"));
    }

    #[test]
    fn test_discover_rust_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("rustfmt.toml"), "").unwrap();

        let mut am = AutoMemory::new(dir.path());
        let count = am.discover();
        assert!(count > 0);
        assert_eq!(am.current.build_cmd, Some("cargo build".into()));
        assert_eq!(am.current.test_cmd, Some("cargo test".into()));
        assert!(am.current.config_files.contains(&"Cargo.toml".to_string()));
        assert!(am.current.config_files.contains(&"rustfmt.toml".to_string()));
    }

    #[test]
    fn test_discover_node_project() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"test","version":"1.0"}"#,
        )
        .unwrap();

        let mut am = AutoMemory::new(dir.path());
        am.discover();
        assert_eq!(am.current.package_manager, Some("npm".into()));
        assert_eq!(am.current.build_cmd, Some("npm run build".into()));
    }

    #[test]
    fn test_unsuccessful_session_does_not_increment() {
        let mut am = AutoMemory::new(Path::new("/tmp/fake4"));
        am.learn_from_session("something went wrong", false);
        assert_eq!(am.current.session_count, 0);
    }
    #[test]
    fn test_detect_lang_zh() {
        assert_eq!(detect_lang("重构订单服务里的并发bug"), "zh");
        assert_eq!(detect_lang("修一下那个文件里的 bug"), "zh");
    }

    #[test]
    fn test_detect_lang_en() {
        assert_eq!(detect_lang("fix the concurrency bug in order service"), "en");
        assert_eq!(detect_lang("cargo build --release"), "en");
    }

    #[test]
    fn test_record_intent_anchors_user_request() {
        let dir = TempDir::new().unwrap();
        let mut am = AutoMemory::new(dir.path());

        am.record_intent("重构订单服务并发bug，写测试，跑通过", true);
        am.record_intent("还需要加超时", false);

        let intents = am.list_intents();
        assert_eq!(intents.len(), 2);
        assert!(intents[0].is_primary);
        assert!(!intents[1].is_primary);
        assert_eq!(intents[0].lang, "zh");
    }

    #[test]
    fn test_enrich_prompt_includes_anchored_intents() {
        let dir = TempDir::new().unwrap();
        let mut am = AutoMemory::new(dir.path());
        am.record_intent("修复 auth.rs 里的 panic", true);

        let enriched = am.enrich_prompt("开始做吧");
        assert!(enriched.contains("[User Intents"));
        assert!(enriched.contains("do NOT compress"));
        assert!(enriched.contains("修复 auth.rs"));
        assert!(enriched.contains("PRIMARY"));
    }

    #[test]
    fn test_clear_intents() {
        let dir = TempDir::new().unwrap();
        let mut am = AutoMemory::new(dir.path());
        am.record_intent("做计划", true);
        assert!(!am.list_intents().is_empty());
        am.clear_intents();
        assert!(am.list_intents().is_empty());
    }

    #[test]
    fn test_intent_overflow_drops_oldest() {
        let dir = TempDir::new().unwrap();
        let mut am = AutoMemory::new(dir.path());
        for i in 1..=20 {
            am.record_intent(format!("intent-{i}"), i == 1);
        }
        assert_eq!(am.list_intents().len(), 12);
        assert_eq!(am.list_intents()[0].rank, 9);
    }

}

// end auto_memory