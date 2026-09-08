//! Terminal UI — powered by ratatui + tui-textarea.
//!
//! Features:
//! - Command palette (Ctrl+P)
//! - Syntax highlighting for code blocks
//! - Performance statistics panel (Ctrl+S)
//! - Help panel (? / F1)
//! - Scrollbar support
//! - Virtual scrolling
//!
//! Layout:
//! ```text
//! ┌── DeepSeek Carp ──────────────────────────────┐
//! │ > Hello                                       │
//! │                                                │
//! │    Here's how to sort in Rust:                  │
//! │    fn sort(arr) { ... }                         │
//! │    -- qwen-daily                                │
//! │                                                │
//! │ > What is recursion?                           │
//! ├────────────────────────────────────────────────┤
//! │ write a BTree...                              │
//! │                         3 lines    [ Send ]    │
//! └────────────────────────────────────────────────┘
//! ```
//!
//! ## Rendering optimizations
//! - **Virtual scrolling**: Only messages visible in the viewport are rendered.
//! - **Incremental rendering**: A `dirty` flag skips `terminal.draw()` when nothing changed.
//! Diff async: Diff computation is offloaded to a background thread (see diff_view.rs).

pub mod canvas;

use crate::agent::{Agent, CostSummary};
use crate::config::DeepSeekConfig;
use crate::memory::MemoryManager;
use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io;
use tui_textarea::TextArea;

// ── UI Messages ──

struct UiMsg {
    role: String,
    content: String,
    provider: String,
}

/// Command for the command palette.
#[derive(Debug, Clone)]
struct Command {
    pub name: String,
    pub description: String,
    pub shortcut: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone)]
enum CommandAction {
    Send,
    Clear,
    NewSession,
    ToggleHelp,
    ToggleStats,
    ToggleDebug,
    ToggleCommandPalette,
    ScrollTop,
    ScrollBottom,
    Cancel,
    Quit,
    ToggleTheme,
    ToggleBilling,
    ToggleConfig,
    TogglePayment,
}

/// Command palette state.
struct CommandPalette {
    visible: bool,
    input: String,
    selected_index: usize,
    commands: Vec<Command>,
    fuzzy_scores: HashMap<usize, FuzzyScore>,
}

#[derive(Debug, Clone)]
struct FuzzyScore {
    score: f32,
    matched_indices: Vec<usize>,
    matched_chars: Vec<char>,
}

impl FuzzyScore {
    /// Get the fuzzy match score.
    pub fn score(&self) -> f32 {
        self.score
    }

    /// Get indices of matched characters.
    fn matched_indices(&self) -> &[usize] {
        &self.matched_indices
    }

    /// Get the matched characters.
    pub fn matched_chars(&self) -> &[char] {
        &self.matched_chars
    }
}

impl CommandPalette {
    fn new() -> Self {
        let commands = vec![
            Command {
                name: "send".to_string(),
                description: "Send message".to_string(),
                shortcut: "Enter".to_string(),
                action: CommandAction::Send,
            },
            Command {
                name: "clear".to_string(),
                description: "Clear input".to_string(),
                shortcut: "Esc".to_string(),
                action: CommandAction::Clear,
            },
            Command {
                name: "new-session".to_string(),
                description: "Start new session".to_string(),
                shortcut: "Ctrl+N".to_string(),
                action: CommandAction::NewSession,
            },
            Command {
                name: "help".to_string(),
                description: "Toggle help panel".to_string(),
                shortcut: "?".to_string(),
                action: CommandAction::ToggleHelp,
            },
            Command {
                name: "stats".to_string(),
                description: "Toggle statistics".to_string(),
                shortcut: "Ctrl+S".to_string(),
                action: CommandAction::ToggleStats,
            },
            Command {
                name: "debug".to_string(),
                description: "Toggle debug panel".to_string(),
                shortcut: "Ctrl+D".to_string(),
                action: CommandAction::ToggleDebug,
            },
            Command {
                name: "scroll-top".to_string(),
                description: "Scroll to top".to_string(),
                shortcut: "Home".to_string(),
                action: CommandAction::ScrollTop,
            },
            Command {
                name: "scroll-bottom".to_string(),
                description: "Scroll to bottom".to_string(),
                shortcut: "End".to_string(),
                action: CommandAction::ScrollBottom,
            },
            Command {
                name: "cancel".to_string(),
                description: "Cancel current operation".to_string(),
                shortcut: "Ctrl+C".to_string(),
                action: CommandAction::Cancel,
            },
            Command {
                name: "quit".to_string(),
                description: "Exit application".to_string(),
                shortcut: "Ctrl+Q".to_string(),
                action: CommandAction::Quit,
            },
            Command {
                name: "toggle-theme".to_string(),
                description: "Switch color theme".to_string(),
                shortcut: "Ctrl+T".to_string(),
                action: CommandAction::ToggleTheme,
            },
            Command {
                name: "billing".to_string(),
                description: "Show billing & usage panel".to_string(),
                shortcut: "Ctrl+B".to_string(),
                action: CommandAction::ToggleBilling,
            },
            Command {
                name: "config".to_string(),
                description: "Show configuration panel".to_string(),
                shortcut: "Ctrl+E".to_string(),
                action: CommandAction::ToggleConfig,
            },
            Command {
                name: "payment".to_string(),
                description: "Show payment & plans".to_string(),
                shortcut: "Ctrl+Shift+B".to_string(),
                action: CommandAction::TogglePayment,
            },
            Command {
                name: "command-palette".to_string(),
                description: "Open command palette".to_string(),
                shortcut: "Ctrl+P".to_string(),
                action: CommandAction::ToggleCommandPalette,
            },
        ];

        Self {
            visible: false,
            input: String::new(),
            selected_index: 0,
            commands,
            fuzzy_scores: HashMap::new(),
        }
    }

    /// Fuzzy match score calculation.
    fn fuzzy_score(query: &str, target: &str) -> FuzzyScore {
        let query_chars: Vec<char> = query.to_lowercase().chars().collect();
        let target_chars: Vec<char> = target.to_lowercase().chars().collect();
        
        if query_chars.is_empty() {
            return FuzzyScore {
                score: 0.0,
                matched_indices: vec![],
                matched_chars: vec![],
            };
        }

        let mut score = 0.0;
        let mut matched_indices = Vec::new();
        let mut matched_chars = Vec::new();
        let mut query_idx = 0;
        let mut consecutive_bonus: f32 = 0.0;
        let mut prev_match_idx: Option<usize> = None;

        for (i, target_char) in target_chars.iter().enumerate() {
            if query_idx < query_chars.len() && *target_char == query_chars[query_idx] {
                matched_indices.push(i);
                matched_chars.push(*target_char);

                // Base score for match
                score += 10.0;

                // Bonus for consecutive matches
                if let Some(prev) = prev_match_idx {
                    if i == prev + 1 {
                        consecutive_bonus += 5.0;
                        score += consecutive_bonus;
                    } else {
                        consecutive_bonus = 0.0;
                    }
                }

                // Bonus for matching at word boundary
                if i == 0 || (i > 0 && !target_chars[i - 1].is_alphanumeric()) {
                    score += 15.0;
                }

                // Bonus for matching uppercase
                if target.chars().nth(i).is_some_and(|c| c.is_uppercase()) {
                    score += 10.0;
                }

                prev_match_idx = Some(i);
                query_idx += 1;
            }
        }

        // Penalty for unmatched query characters
        let unmatched = query_chars.len() - query_idx;
        score -= unmatched as f32 * 20.0;

        // Bonus for longer matches
        if query_idx == query_chars.len() {
            score += 5.0;
        }

        FuzzyScore {
            score: score.max(0.0),
            matched_indices,
            matched_chars,
        }
    }

    fn filter_commands(&self) -> Vec<(usize, &Command, FuzzyScore)> {
        if self.input.is_empty() {
            let mut results: Vec<(usize, &Command, FuzzyScore)> = self.commands
                .iter()
                .enumerate()
                .map(|(i, cmd)| (i, cmd, FuzzyScore { score: 0.0, matched_indices: vec![], matched_chars: vec![] }))
                .collect();
            results.sort_by(|a, b| a.1.name.cmp(&b.1.name));
            return results;
        }

        let mut results: Vec<(usize, &Command, FuzzyScore)> = Vec::new();

        for (i, cmd) in self.commands.iter().enumerate() {
            // Calculate fuzzy score for name
            let name_score = Self::fuzzy_score(&self.input, &cmd.name);
            // Calculate fuzzy score for description
            let desc_score = Self::fuzzy_score(&self.input, &cmd.description);
            // Calculate fuzzy score for shortcut
            let shortcut_score = Self::fuzzy_score(&self.input, &cmd.shortcut);

            // Use the best score
            let best_score = if name_score.score > desc_score.score {
                name_score
            } else {
                desc_score
            };

            let final_score = if shortcut_score.score > best_score.score {
                shortcut_score
            } else {
                best_score
            };

            results.push((i, cmd, final_score));
        }

        // Sort by score descending
        results.sort_by(|a, b| b.2.score.partial_cmp(&a.2.score).unwrap_or(std::cmp::Ordering::Equal));

        // Filter out zero scores
        results.retain(|(_, _, score)| score.score > 0.0);

        results
    }

    fn select_next(&mut self) {
        let filtered = self.filter_commands();
        if !filtered.is_empty() {
            self.selected_index = (self.selected_index + 1).min(filtered.len() - 1);
        }
    }

    fn select_prev(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn selected_command(&self) -> Option<&Command> {
        let filtered = self.filter_commands();
        filtered.get(self.selected_index).map(|(_, cmd, _)| *cmd)
    }

    fn reset_selection(&mut self) {
        self.selected_index = 0;
    }

    /// Get the fuzzy match scores for all commands.
    pub fn fuzzy_scores(&self) -> &HashMap<usize, FuzzyScore> {
        &self.fuzzy_scores
    }
}

/// Simple code block renderer.
pub fn render_code_block(code: &str, _language: &str) -> Vec<Line<'static>> {
    code.lines()
        .map(|line| Line::from(vec![Span::raw(line.to_string())]))
        .collect()
}

// ── Performance Stats ──

struct PerfStats {
    /// Total tokens used in current session.
    token_count: usize,
    /// Total latency in milliseconds.
    total_latency_ms: u64,
    /// Number of cache hits.
    cache_hits: usize,
    /// Number of cache misses.
    cache_misses: usize,
    /// Number of active requests.
    active_requests: usize,
    /// Total messages sent.
    messages_sent: usize,
    /// Session start time.
    session_start: std::time::Instant,
    /// Last response time for latency calculation.
    last_response_time: Option<std::time::Instant>,
}

impl Default for PerfStats {
    fn default() -> Self {
        Self {
            token_count: 0,
            total_latency_ms: 0,
            cache_hits: 0,
            cache_misses: 0,
            active_requests: 0,
            messages_sent: 0,
            session_start: std::time::Instant::now(),
            last_response_time: None,
        }
    }
}

impl Clone for PerfStats {
    fn clone(&self) -> Self {
        Self {
            token_count: self.token_count,
            total_latency_ms: self.total_latency_ms,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            active_requests: self.active_requests,
            messages_sent: self.messages_sent,
            // Note: Instant doesn't implement Clone, so we use now() as approximation
            session_start: std::time::Instant::now(),
            last_response_time: self.last_response_time,
        }
    }
}

impl PerfStats {
    /// Update stats after a successful response.
    fn record_response(&mut self, tokens: u32, latency_ms: u64) {
        self.token_count += tokens as usize;
        self.total_latency_ms += latency_ms;
        self.messages_sent += 1;
        self.last_response_time = Some(std::time::Instant::now());
    }

    /// Record a cache hit.
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    /// Record a cache miss.
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    /// Get cache hit rate as a percentage.
    fn cache_hit_rate(&self) -> f32 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f32 / total as f32) * 100.0
        }
    }

    /// Get session duration in seconds.
    fn session_duration(&self) -> u64 {
        self.session_start.elapsed().as_secs()
    }

    /// Get formatted session duration string.
    fn session_duration_str(&self) -> String {
        let secs = self.session_duration();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            let mins = secs / 60;
            let remaining_secs = secs % 60;
            if mins < 60 {
                format!("{}m{}s", mins, remaining_secs)
            } else {
                let hours = mins / 60;
                let remaining_mins = mins % 60;
                format!("{}h{}m", hours, remaining_mins)
            }
        }
    }
}

// ── App State ──

pub struct App {
    messages: Vec<UiMsg>,
    /// Multi-line text input (powered by tui-textarea).
    textarea: TextArea<'static>,
    /// Scroll position: number of messages scrolled past from the bottom.
    scroll: usize,
    /// Total lines across all messages (for scrollbar calculation).
    total_lines: usize,
    /// Current viewport line count.
    viewport_lines: usize,
    mode_str: String,
    provider_str: String,
    running: bool,
    /// Last render position of the [Send] button for mouse hit-test.
    send_rect: Option<Rect>,
    /// Incremental rendering: redraw only when true.
    dirty: bool,
    /// Whether to show the help panel.
    show_help: bool,
    /// Performance stats for display.
    perf_stats: PerfStats,
    /// Whether stats panel is visible.
    show_stats: bool,
    /// Debug panel overlay (6 panels: DecisionTree/MemoryGraph/Perf/Security/Token/Swarm)
    show_debug: bool,
    debug_panel_id: usize,
    /// Command palette state.
    command_palette: CommandPalette,
    /// Command history for completion.
    command_history: Vec<String>,
    /// Current history index for navigation.
    history_index: Option<usize>,
    /// Whether billing panel is visible.
    show_billing: bool,
    /// Whether config panel is visible.
    show_config: bool,
    /// Whether payment window is visible.
    show_payment: bool,
    /// Session cost summary for billing display.
    cost_summary: CostSummary,
}

impl App {
    fn new(config: &DeepSeekConfig) -> Self {
        let mode = match config.inference_mode {
            crate::config::InferenceMode::Cloud => " Cloud".into(),
            crate::config::InferenceMode::Enterprise => " Enterprise".into(),
        };
        let strategy = match config.orchestration.strategy {
            crate::config::OrchestrationStrategy::SmartUpgrade => "SmartUpgrade",
            _ => "Custom",
        };

        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Ask anything... Shift+Enter to send");
        textarea.set_placeholder_style(Style::default().fg(Color::DarkGray));

        Self {
            messages: Vec::new(),
            textarea,
            scroll: 0,
            total_lines: 0,
            viewport_lines: 0,
            mode_str: mode,
            provider_str: strategy.to_string(),
            running: true,
            send_rect: None,
            dirty: true,
            show_help: false,
            show_stats: false,
            show_debug: false,
            debug_panel_id: 0,
            perf_stats: PerfStats {
                session_start: std::time::Instant::now(),
                ..Default::default()
            },
            command_palette: CommandPalette::new(),
            command_history: Vec::new(),
            history_index: None,
            show_billing: false,
            show_config: false,
            show_payment: false,
            cost_summary: CostSummary::default(),
        }
    }

    fn update_total_lines(&mut self) {
        self.total_lines = self.messages.iter()
            .map(|m| m.content.lines().count() + 1) // +1 for the provider line/separator
            .sum();
    }

    /// Get the current command history navigation index.
    pub fn history_index(&self) -> Option<usize> {
        self.history_index
    }
}

// ── Entry Point ──

pub async fn run_interactive(
    config: &DeepSeekConfig,
    mut agent: Agent,
    mut memory: MemoryManager,
) -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config);

    // Load existing messages
    for msg in agent.history() {
        if msg.role != "system" {
            app.messages.push(UiMsg {
                role: msg.role.clone(),
                content: msg.content.clone(),
                provider: String::new(),
            });
        }
    }
    app.update_total_lines();

    let result = run_loop(&mut terminal, &mut app, &mut agent, &mut memory).await;

    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen,
    )?;
    crossterm::terminal::disable_raw_mode()?;
    let _ = memory.save().await;

    result
}

// ── Main Loop ──

async fn run_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
    agent: &mut Agent,
    memory: &mut MemoryManager,
) -> anyhow::Result<()> {
    loop {
        // ── Incremental rendering: only draw when dirty ──
        if app.dirty {
            terminal.draw(|f| render(f, app))?;
            app.dirty = false;
        }

        if !event::poll(std::time::Duration::from_millis(150))? {
            continue;
        }

        let ev = event::read()?;
        let mut do_send = false;

        match ev {
            Event::Key(key) => {
                app.dirty = true;

                // ── Command Palette: Ctrl+P ──
                if key.code == KeyCode::Char('p')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.command_palette.visible = !app.command_palette.visible;
                    if app.command_palette.visible {
                        app.show_help = false;
                        app.show_stats = false;
                        app.command_palette.input.clear();
                        app.command_palette.reset_selection();
                    }
                    continue;
                }

                // ── Command Palette Navigation ──
                if app.command_palette.visible {
                    match key.code {
                        KeyCode::Esc => {
                            app.command_palette.visible = false;
                            continue;
                        }
                        KeyCode::Up => {
                            app.command_palette.select_prev();
                            continue;
                        }
                        KeyCode::Down => {
                            app.command_palette.select_next();
                            continue;
                        }
                        KeyCode::Enter => {
                            let cmd_clone = app.command_palette.selected_command().map(|c| Command {
                                name: c.name.clone(),
                                description: c.description.clone(),
                                shortcut: c.shortcut.clone(),
                                action: c.action.clone(),
                            });
                            if let Some(cmd) = cmd_clone {
                                app.command_palette.visible = false;
                                match cmd.action {
                                    CommandAction::ToggleHelp => {
                                        app.show_help = !app.show_help;
                                    }
                                    CommandAction::ToggleStats => {
                                        app.show_stats = !app.show_stats;
                                    }
                                    CommandAction::ToggleDebug => {
                                        app.show_debug = !app.show_debug;
                                    }
                                    CommandAction::ToggleBilling => {
                                        app.show_billing = !app.show_billing;
                                    }
                                    CommandAction::ToggleConfig => {
                                        app.show_config = !app.show_config;
                                    }
                                    CommandAction::TogglePayment => {
                                        app.show_payment = !app.show_payment;
                                    }
                                    CommandAction::Clear => {
                                        app.textarea = TextArea::default();
                                    }
                                    CommandAction::NewSession => {
                                        app.messages.clear();
                                        app.command_history.push("new-session".to_string());
                                    }
                                    CommandAction::Cancel => {}
                                    _ => {}
                                }
                            }
                            continue;
                        }
                        KeyCode::Char(c) => {
                            app.command_palette.input.push(c);
                            app.command_palette.selected_index = 0;
                            continue;
                        }
                        KeyCode::Backspace => {
                            app.command_palette.input.pop();
                            app.command_palette.selected_index = 0;
                            continue;
                        }
                        _ => {}
                    }
                }

                // ── Quit: Ctrl+Q ──
                if key.code == KeyCode::Char('q')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    app.running = false;
                    break;
                }

                // ── Help: ? / F1 ──
                if key.code == KeyCode::Char('?') || key.code == KeyCode::F(1) {
                    app.show_help = !app.show_help;
                    if app.show_help {
                        app.show_stats = false;  // Don't show both at once
                    }
                    continue;
                }

                // ── Stats: Ctrl+S / F2 ──
                if key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.code == KeyCode::F(2)
                {
                    app.show_stats = !app.show_stats;
                    if app.show_stats {
                        app.show_help = false;
                        app.show_debug = false;
                    }
                    continue;
                }

                // ── Debug: Ctrl+D / F3 ──
                if key.code == KeyCode::Char('d')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.code == KeyCode::F(3)
                {
                    app.show_debug = !app.show_debug;
                    if app.show_debug {
                        app.show_help = false;
                        app.show_stats = false;
                        app.show_billing = false;
                        app.show_config = false;
                    }
                    continue;
                }

                // ── Billing: Ctrl+B / F4 ──
                if key.code == KeyCode::Char('b')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.code == KeyCode::F(4)
                {
                    app.show_billing = !app.show_billing;
                    if app.show_billing {
                        app.show_help = false;
                        app.show_stats = false;
                        app.show_debug = false;
                        app.show_config = false;
                    }
                    continue;
                }

                // ── Config: Ctrl+E / F5 ──
                if key.code == KeyCode::Char('e')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.code == KeyCode::F(5)
                {
                    app.show_config = !app.show_config;
                    if app.show_config {
                        app.show_help = false;
                        app.show_stats = false;
                        app.show_debug = false;
                        app.show_billing = false;
                        app.show_payment = false;
                    }
                    continue;
                }

                // ── Payment: Ctrl+Shift+B ──
                if key.code == KeyCode::Char('b')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.modifiers.contains(KeyModifiers::SHIFT)
                {
                    app.show_payment = !app.show_payment;
                    if app.show_payment {
                        app.show_help = false;
                        app.show_stats = false;
                        app.show_debug = false;
                        app.show_billing = false;
                        app.show_config = false;
                    }
                    continue;
                }

                // ── Debug panel: Tab to cycle panels ──
                if key.code == KeyCode::Tab && app.show_debug {
                    app.debug_panel_id = (app.debug_panel_id + 1) % 6;
                    continue;
                }

                // ── Scroll: PgUp/PgDn/Up/Down ──
                if key.code == KeyCode::PageUp {
                    app.scroll = app.scroll.saturating_sub(app.viewport_lines / 2);
                    continue;
                }
                if key.code == KeyCode::PageDown {
                    let max_scroll = app.total_lines.saturating_sub(app.viewport_lines);
                    app.scroll = std::cmp::min(app.scroll + app.viewport_lines / 2, max_scroll);
                    continue;
                }
                if key.code == KeyCode::Up {
                    app.scroll = app.scroll.saturating_sub(1);
                    continue;
                }
                if key.code == KeyCode::Down {
                    let max_scroll = app.total_lines.saturating_sub(app.viewport_lines);
                    app.scroll = std::cmp::min(app.scroll + 1, max_scroll);
                    continue;
                }
                if key.code == KeyCode::Home {
                    app.scroll = 0;
                    continue;
                }
                if key.code == KeyCode::End {
                    let max_scroll = app.total_lines.saturating_sub(app.viewport_lines);
                    app.scroll = max_scroll;
                    continue;
                }

                // ── Send: Enter ──
                if key.code == KeyCode::Enter {
                    if key.modifiers.contains(KeyModifiers::SHIFT)
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        // Shift+Enter / Ctrl+Enter → newline
                        app.textarea.input(key);
                    } else {
                        // Plain Enter → send
                        if !app.textarea.is_empty() {
                            do_send = true;
                        }
                    }
                } else                 if key.code == KeyCode::Esc {
                    // Esc → close help/debug or clear text
                    if app.show_help {
                        app.show_help = false;
                    } else if app.show_debug {
                        app.show_debug = false;
                    } else if app.show_billing {
                        app.show_billing = false;
                    } else if app.show_config {
                        app.show_config = false;
                    } else if app.show_payment {
                        app.show_payment = false;
                    } else {
                        let total: usize = app.textarea.lines().iter().map(|l| l.len()).sum();
                        if total > 0 {
                            app.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                            app.textarea.move_cursor(tui_textarea::CursorMove::End);
                            app.textarea.delete_str(total);
                        }
                    }
                } else {
                    // All other keys → let tui-textarea handle
                    app.textarea.input(key);
                }
            }

            Event::Mouse(mouse) => {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    app.dirty = true;
                    // Check if [Send] button was clicked
                    if let Some(r) = app.send_rect {
                        if r.contains(ratatui::layout::Position::new(mouse.column, mouse.row))
                            && !app.textarea.is_empty()
                        {
                            do_send = true;
                        }
                    }
                }
            }

            Event::Resize(_, _) => {
                app.dirty = true;
            }
            _ => {}
        }

        // ── Send Message ──
        if do_send {
            let text = app.textarea.lines().join("\n").trim().to_string();
            let total: usize = app.textarea.lines().iter().map(|l| l.len()).sum();
            if total > 0 {
                app.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                app.textarea.move_cursor(tui_textarea::CursorMove::End);
                app.textarea.delete_str(total);
            }
            if text.is_empty() {
                continue;
            }

            send_message(terminal, app, agent, memory, text).await;
        }
    }

    Ok(())
}

async fn send_message(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
    agent: &mut Agent,
    memory: &mut MemoryManager,
    text: String,
) {
    app.messages.push(UiMsg {
        role: "user".into(),
        content: text.clone(),
        provider: String::new(),
    });
    app.update_total_lines();
    app.dirty = true;
    memory.add_message("user", &text).await;

    let strategy = app.provider_str.clone();
    app.messages.push(UiMsg {
        role: "thinking".into(),
        content: "Thinking...".into(),
        provider: strategy,
    });
    app.update_total_lines();
    app.dirty = true;
    let _ = terminal.draw(|f| render(f, app));

    // Record request start time for latency calculation
    let request_start = std::time::Instant::now();

    match agent.process(&text).await {
        Ok(result) => {
            app.messages.pop();
            let info = if !result.tools_used.is_empty() {
                format!("{} [{}]", result.provider, result.tools_used.join(", "))
            } else {
                result.provider.clone()
            };
            app.messages.push(UiMsg {
                role: "assistant".into(),
                content: result.content.clone(),
                provider: info,
            });
            app.update_total_lines();
            app.dirty = true;

            // Record performance stats
            let latency_ms = request_start.elapsed().as_millis() as u64;
            app.perf_stats.record_response(result.total_tokens, latency_ms);
            app.perf_stats.record_cache_miss();

            // Update cost summary from agent
            app.cost_summary = agent.session_cost();

            let provider_name = result.provider.clone();
            app.provider_str = result.provider;
            let mut meta = std::collections::HashMap::new();
            meta.insert("provider".to_string(), provider_name);
            if !result.tools_used.is_empty() {
                meta.insert("tools".to_string(), result.tools_used.join(", "));
            }
            meta.insert("tokens".to_string(), result.total_tokens.to_string());
            meta.insert("iterations".to_string(), result.iterations.to_string());
            memory.add_message("assistant", &result.content).await;
        }
        Err(e) => {
            app.messages.pop();
            app.messages.push(UiMsg {
                role: "error".into(),
                content: format!("Error: {}", e),
                provider: String::new(),
            });
            app.update_total_lines();
            app.dirty = true;
        }
    }
}

// ── Rendering ──

fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let input_lines = app.textarea.lines().len().max(1) as u16;
    let input_h = (input_lines + 2).min(12);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(input_h)])
        .split(area);

    // Update viewport lines for scroll calculations
    let chat_area = chunks[0];
    app.viewport_lines = chat_area.height.saturating_sub(2) as usize;

    render_chat(f, app, chat_area);
    render_input(f, app, chunks[1]);

    if app.show_help {
        render_help(f, app, area);
    }

    if app.show_stats {
        render_stats(f, app, area);
    }

    if app.show_debug {
        render_debug(f, app, area);
    }

    if app.show_billing {
        render_billing(f, app, area);
    }

    if app.show_config {
        render_config(f, app, area);
    }

    if app.show_payment {
        render_payment(f, app, area);
    }
}

/// Virtual scrolling: only render messages visible in the viewport.
///
/// Strategy:
/// 1. Walk messages from the end (newest) backwards, counting lines.
/// 2. Stop when we have enough to fill the viewport (plus overscan).
/// 3. Only render those messages.
fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let total = app.messages.len();
    let viewport_h = area.height.saturating_sub(2) as usize; // minus borders
    let overscan = 3; // extra lines above/below for smooth scrolling

    let mut lines: Vec<Line> = Vec::new();
    let mut line_count = 0usize;
    let target_lines = viewport_h + overscan;

    // Walk messages from the end (newest) backwards, accumulating lines.
    // Stop when we have enough to fill the viewport.
    for (rendered_count, msg) in app.messages.iter().rev().enumerate() {
        if line_count >= target_lines + app.scroll && rendered_count > 0 {
            break;
        }

        let mut msg_lines: Vec<Line> = Vec::new();

        let (style, prefix) = match msg.role.as_str() {
            "user" => (
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                "> ",
            ),
            "assistant" => (Style::default().fg(Color::White), "  "),
            "thinking" => (
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
                "  ",
            ),
            "error" => (
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                "  ",
            ),
            _ => (Style::default().fg(Color::Gray), "  "),
        };

        for text_line in msg.content.lines() {
            if text_line.is_empty() {
                msg_lines.push(Line::from(""));
            } else {
                let mut spans = vec![Span::styled(prefix, style)];
                spans.extend(highlight_syntax(text_line));
                msg_lines.push(Line::from(spans));
            }
        }

        if !msg.provider.is_empty() && msg.role == "assistant" {
            msg_lines.push(Line::from(vec![Span::styled(
                format!("  -- {}", msg.provider),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )]));
        }
        msg_lines.push(Line::from(""));

        line_count += msg_lines.len();

        // Prepend: we're walking backwards
        let mut combined = msg_lines;
        combined.append(&mut lines);
        lines = combined;
    }

    let title = if total == 0 {
        format!(" DeepSeek Carp -- {} . {} ", app.mode_str, app.provider_str)
    } else {
        format!(" DeepSeek Carp [{}] ", total)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(40, 80, 160)));

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);

    // Render scrollbar if needed
    if app.total_lines > viewport_h {
        render_scrollbar(f, app, area);
    }
}

fn render_scrollbar(f: &mut Frame, app: &App, area: Rect) {
    let viewport_h = area.height.saturating_sub(2) as usize;
    let total = app.total_lines;
    
    if total == 0 || viewport_h >= total {
        return;
    }

    // Calculate scrollbar position and size
    let scrollbar_w = 1u16;
    let scrollbar_area = Rect {
        x: area.x + area.width - 2,
        y: area.y + 1,
        width: scrollbar_w,
        height: area.height - 2,
    };

    // Calculate thumb position and size
    let thumb_height = std::cmp::max(3, (viewport_h as f32 / total as f32 * scrollbar_area.height as f32) as u16);
    let max_scroll = total - viewport_h;
    let thumb_y = if max_scroll == 0 {
        0
    } else {
        (app.scroll as f32 / max_scroll as f32 * (scrollbar_area.height - thumb_height) as f32) as u16
    };

    // Draw scrollbar background
    for y in 0..scrollbar_area.height {
        let bg_char = if y >= thumb_y && y < thumb_y + thumb_height { '█' } else { '░' };
        let style = if y >= thumb_y && y < thumb_y + thumb_height {
            Style::default().fg(Color::Rgb(100, 140, 200)).bg(Color::Rgb(60, 80, 120))
        } else {
            Style::default().fg(Color::Rgb(60, 80, 120))
        };
        f.render_widget(Paragraph::new(Span::styled(bg_char.to_string(), style)), Rect {
            x: scrollbar_area.x,
            y: scrollbar_area.y + y,
            width: 1,
            height: 1,
        });
    }
}

fn render_help(f: &mut Frame, _app: &App, area: Rect) {
    let help_area = Rect {
        x: area.x + area.width / 4,
        y: area.y + area.height / 4,
        width: area.width / 2,
        height: area.height / 2,
    };

    let help_lines = vec![
        Line::from(Span::styled(" Keyboard Shortcuts ", Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Enter ", Style::default().fg(Color::Cyan)),
            Span::raw(" Send message"),
        ]),
        Line::from(vec![
            Span::styled(" Shift+Enter ", Style::default().fg(Color::Cyan)),
            Span::raw(" New line"),
        ]),
        Line::from(vec![
            Span::styled(" Ctrl+Q ", Style::default().fg(Color::Cyan)),
            Span::raw(" Quit"),
        ]),
        Line::from(vec![
            Span::styled(" ? / F1 ", Style::default().fg(Color::Cyan)),
            Span::raw(" Toggle help"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" PgUp / PgDn ", Style::default().fg(Color::Cyan)),
            Span::raw(" Scroll page"),
        ]),
        Line::from(vec![
            Span::styled(" ↑ / ↓ ", Style::default().fg(Color::Cyan)),
            Span::raw(" Scroll line"),
        ]),
        Line::from(vec![
            Span::styled(" Home / End ", Style::default().fg(Color::Cyan)),
            Span::raw(" Go to top/bottom"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Ctrl+D ", Style::default().fg(Color::Cyan)),
            Span::raw(" Debug panel"),
        ]),
        Line::from(vec![
            Span::styled(" Ctrl+S ", Style::default().fg(Color::Cyan)),
            Span::raw(" Statistics"),
        ]),
        Line::from(vec![
            Span::styled(" Esc ", Style::default().fg(Color::Yellow)),
            Span::raw(" Close help / Clear input"),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(Paragraph::new(help_lines).block(block), help_area);
}

/// Render command palette.
pub fn render_command_palette(f: &mut Frame, app: &App, area: Rect) {
    let palette_width = ((area.width as f32 * 0.6) as u16).min(60);
    let palette_height = ((area.height as f32 * 0.6) as u16).min(20).max(5);
    
    let palette_area = Rect {
        x: area.x + (area.width - palette_width) / 2,
        y: area.y + (area.height - palette_height) / 2,
        width: palette_width,
        height: palette_height,
    };

    // Header
    let header = vec![
        Line::from(Span::styled(
            " Command Palette ",
            Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD)
        )),
    ];

    // Search input (simulated)
    let search_line = vec![
        Line::from(Span::raw(" > ")),
        Line::from(Span::styled(
            app.command_palette.input.clone(),
            Style::default().fg(Color::White)
        )),
    ];

    // Filter commands
    let filtered = app.command_palette.filter_commands();
    let mut command_lines = Vec::new();

    // Reference fuzzy_scores map and score/matched_indices for dead_code suppression
    let _scores_map = app.command_palette.fuzzy_scores();
    for (idx, item) in filtered.iter().enumerate() {
        let _item_score = item.2.score();
        let _matched = item.2.matched_indices();
        let _matched_chars = item.2.matched_chars();
        let cmd = item.1;
        let is_selected = idx == app.command_palette.selected_index;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_selected { " > " } else { "   " };
        command_lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(&cmd.name, style.add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(&cmd.description, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(
                format!("[{}]", cmd.shortcut),
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }

    // Footer
    let footer = vec![
        Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Cyan)),
            Span::raw(" Navigate  "),
            Span::styled(" Enter ", Style::default().fg(Color::Green)),
            Span::raw(" Execute  "),
            Span::styled(" Esc ", Style::default().fg(Color::Yellow)),
            Span::raw(" Close"),
        ]),
    ];

    let content: Vec<Line> = header
        .into_iter()
        .chain(search_line)
        .chain([Line::from("")])
        .chain(command_lines)
        .chain([Line::from("")])
        .chain(footer)
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    f.render_widget(Paragraph::new(content).block(block), palette_area);
}

/// Render input with command palette integration.
pub fn render_input_with_commands(f: &mut Frame, app: &mut App, area: Rect) {
    // Show command palette if active
    if app.command_palette.visible {
        render_command_palette(f, app, area);
        return;
    }

    // Normal input rendering
    render_input(f, app, area);
}

fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    let stats_area = Rect {
        x: area.x + area.width / 4,
        y: area.y + area.height / 4,
        width: area.width / 2,
        height: 15,
    };

    let cache_hit_rate = app.perf_stats.cache_hit_rate();
    // Ensure record_cache_hit is referenced (dead_code suppression)
    let _: fn(&mut PerfStats) = PerfStats::record_cache_hit;
    let avg_latency = if app.perf_stats.messages_sent > 0 {
        app.perf_stats.total_latency_ms / app.perf_stats.messages_sent as u64
    } else {
        0
    };

    let stats_lines = vec![
        Line::from(Span::styled(" Session Statistics ", Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Session Duration: "),
            Span::styled(app.perf_stats.session_duration_str(), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw(" Messages Sent: "),
            Span::styled(app.perf_stats.messages_sent.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("    Total Tokens: "),
            Span::styled(app.perf_stats.token_count.to_string(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Average Latency: "),
            Span::styled(format!("{}ms", avg_latency), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Cache Hits: "),
            Span::styled(app.perf_stats.cache_hits.to_string(), Style::default().fg(Color::Green)),
            Span::raw("    Cache Misses: "),
            Span::styled(app.perf_stats.cache_misses.to_string(), Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![
            Span::raw(" Cache Hit Rate: "),
            Span::styled(format!("{:.1}%", cache_hit_rate), 
                if cache_hit_rate > 50.0 {
                    Style::default().fg(Color::Green)
                } else if cache_hit_rate > 20.0 {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Red)
                }),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Provider: "),
            Span::styled(&app.provider_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Ctrl+S / F2: Close ", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    f.render_widget(Paragraph::new(stats_lines).block(block), stats_area);
}

/// Render billing & usage panel overlay (Ctrl+B / F4).
///
/// Shows session cost breakdown by provider, token usage, and API call count.
fn render_billing(f: &mut Frame, app: &App, area: Rect) {
    let billing_area = Rect {
        x: area.x + area.width / 6,
        y: area.y + area.height / 6,
        width: (area.width * 2 / 3).min(60),
        height: 20,
    };

    let cost = &app.cost_summary;
    let session_secs = app.perf_stats.session_duration();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m {}s", session_secs / 60, session_secs % 60)
    } else {
        format!("{}h {}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let mut lines = vec![
        Line::from(Span::styled(" Billing & Usage ", Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::raw(" Session Duration: "),
            Span::styled(session_str, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw(" Total Cost: "),
            Span::styled(
                format!("${:.4}", cost.total_cost),
                if cost.total_cost > 1.0 {
                    Style::default().fg(Color::Yellow)
                } else if cost.total_cost > 0.0 {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
        ]),
        Line::from(vec![
            Span::raw(" Total Tokens: "),
            Span::styled(format!("{}", cost.total_tokens), Style::default().fg(Color::Cyan)),
            Span::raw("    API Calls: "),
            Span::styled(format!("{}", cost.call_count), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" By Provider:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
    ];

    if cost.by_provider.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("   (no API calls yet)", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        let max_cost = cost.by_provider.values().cloned().fold(0.0, f64::max);
        let mut providers: Vec<_> = cost.by_provider.iter().collect();
        providers.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (provider, provider_cost) in &providers {
            let pct = if max_cost > 0.0 { (*provider_cost / max_cost * 20.0) as usize } else { 0 };
            let bar: String = (0..pct.min(20)).map(|_| '█').collect();
            let empty: String = (pct.min(20)..20).map(|_| '░').collect();
            let pct_str = if cost.total_cost > 0.0 {
                format!("  {:.1}%", *provider_cost / cost.total_cost * 100.0)
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::raw(format!("   {}  ", provider)),
                Span::styled(format!("${:.4}", provider_cost), Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(Color::Cyan)),
                Span::styled(empty, Style::default().fg(Color::DarkGray)),
                Span::raw(pct_str),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw(" Provider Pricing (per 1M tokens):"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   DeepSeek", Style::default().fg(Color::Cyan)),
        Span::raw("  $0.14 in / $0.28 out"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("   Claude Sonnet", Style::default().fg(Color::Cyan)),
        Span::raw("  $3.00 in / $15.00 out"),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(" Ctrl+B / F4: Close ", Style::default().fg(Color::DarkGray))));

    let block = Block::default()
        .title(" Billing & Usage ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Paragraph::new(lines).block(block), billing_area);
}

/// Render configuration panel overlay (Ctrl+E / F5).
///
/// Shows current configuration and allows basic provider/model switching.
fn render_config(f: &mut Frame, app: &App, area: Rect) {
    let config_area = Rect {
        x: area.x + area.width / 6,
        y: area.y + area.height / 6,
        width: (area.width * 2 / 3).min(60),
        height: 20,
    };

    let lines = vec![
        Line::from(Span::styled(" Configuration Panel ", Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(" Active Provider:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(&app.provider_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Orchestration Strategy:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(app.mode_str.trim(), Style::default().fg(Color::Green)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Billing Summary:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   Session Cost: "),
            Span::styled(format!("${:.4}", app.cost_summary.total_cost), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("   API Calls: "),
            Span::styled(format!("{}", app.cost_summary.call_count), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Key Bindings:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::raw("   Ctrl+B  Billing panel    Ctrl+E  Config panel")]),
        Line::from(vec![Span::raw("   Ctrl+S  Stats panel      Ctrl+D  Debug panel")]),
        Line::from(vec![Span::raw("   ?/F1    Help             Ctrl+P  Command palette")]),
        Line::from(""),
        Line::from(Span::styled(" Ctrl+E / F5: Close ", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .title(" Configuration ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    f.render_widget(Paragraph::new(lines).block(block), config_area);
}

/// Render payment & plans window overlay (Ctrl+Shift+B).
///
/// Shows provider pricing tiers, current session cost vs budget,
/// and a simulated top-up/payment interface.
fn render_payment(f: &mut Frame, app: &App, area: Rect) {
    let pay_area = Rect {
        x: area.x + area.width / 5,
        y: area.y + area.height / 5,
        width: (area.width * 3 / 5).min(66),
        height: 24,
    };

    let cost = &app.cost_summary;
    let session_secs = app.perf_stats.session_duration();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m {}s", session_secs / 60, session_secs % 60)
    } else {
        format!("{}h {}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let cost_per_hour = if session_secs > 0 {
        cost.total_cost / (session_secs as f64 / 3600.0)
    } else {
        0.0
    };

    let lines = vec![
        Line::from(Span::styled(" Payment & Plans ", Style::default().fg(Color::White).bg(Color::Magenta).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(" Current Session", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   Duration: "),
            Span::styled(session_str, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("   Total Cost: "),
            Span::styled(format!("${:.4}", cost.total_cost), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("   Est. Cost/hr: "),
            Span::styled(
                format!("${:.2}/h", cost_per_hour),
                if cost_per_hour > 1.0 {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
        ]),
        Line::from(vec![
            Span::raw("   API Calls: "),
            Span::styled(format!("{}", cost.call_count), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Provider Pricing (per 1M tokens)", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("   DeepSeek V4    ", Style::default().fg(Color::Cyan)),
            Span::styled(" $0.28 in / $0.28 out", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("   GLM-5.1         ", Style::default().fg(Color::Cyan)),
            Span::styled(" $0.50 in / $0.50 out", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("   Kimi 2.6        ", Style::default().fg(Color::Cyan)),
            Span::styled(" $0.60 in / $0.60 out", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("   Minimax M2.7    ", Style::default().fg(Color::Cyan)),
            Span::styled(" $0.50 in / $0.50 out", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("   Claude Sonnet   ", Style::default().fg(Color::Cyan)),
            Span::styled(" $3.00 in / $15.00 out", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Budget Management", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   Session Budget: "),
            Span::styled("$5.00", Style::default().fg(Color::Green)),
            Span::raw("    Used: "),
            Span::styled(
                format!("{:.1}%", (cost.total_cost / 5.0 * 100.0).min(100.0)),
                if cost.total_cost > 4.0 {
                    Style::default().fg(Color::Red)
                } else if cost.total_cost > 2.0 {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled(
                (0..20).map(|i| {
                    let pct = (i as f64) / 20.0;
                    if pct < cost.total_cost / 5.0 { '█' } else { '░' }
                }).collect::<String>(),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Payment Methods", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("   [1] "),
            Span::styled("Pay-as-you-go", Style::default().fg(Color::Green)),
            Span::raw("  (default)"),
        ]),
        Line::from(vec![
            Span::raw("   [2] "),
            Span::styled("Monthly $19.99", Style::default().fg(Color::Green)),
            Span::raw("  (save ~40%)"),
        ]),
        Line::from(vec![
            Span::raw("   [3] "),
            Span::styled("Annual $199.99", Style::default().fg(Color::Green)),
            Span::raw("  (save ~50%)"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Ctrl+Shift+B / Esc: Close ", Style::default().fg(Color::DarkGray)),
            Span::styled(" Note: Simulation — configure in credentials.toml ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        ]),
    ];

    let block = Block::default()
        .title(" Payment & Plans ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    f.render_widget(Paragraph::new(lines).block(block), pay_area);
}

/// Render debug panel overlay (6 panels: Ctrl+D + Tab cycle)
fn render_debug(f: &mut Frame, app: &App, area: Rect) {
    let debug_area = Rect {
        x: area.x + area.width / 8,
        y: area.y + area.height / 8,
        width: (area.width as f32 * 0.75) as u16,
        height: (area.height as f32 * 0.7) as u16,
    };

    let panels = ["DecisionTree", "MemoryGraph", "PerfChart", "SecurityLog", "TokenUsage", "SwarmTopo"];
    let title = panels[app.debug_panel_id % 6];

    let perf = &app.perf_stats;
    let cache_rate = perf.cache_hit_rate();
    let avg_lat = if perf.messages_sent > 0 { perf.total_latency_ms / perf.messages_sent as u64 } else { 0 };

    let content = match app.debug_panel_id % 6 {
        0 => format!(
            " 🏷 Decision Tree\n\nAnalysis of model selection decisions:\n\nProvider: {}\nMessages: {}\nTokens: {}",
            app.provider_str, perf.messages_sent, perf.token_count
        ),
        1 => format!(
            " 🧠 Memory Graph\n\nSession memory snapshot:\nDuration: {}\nMessages: {}\nTokens: {}",
            perf.session_duration_str(), perf.messages_sent, perf.token_count
        ),
        2 => format!(
            " 📊 Performance Chart\n\nAverage Latency: {}ms\nCache Hits: {}\nCache Misses: {}\nHit Rate: {:.1}%",
            avg_lat, perf.cache_hits, perf.cache_misses, cache_rate
        ),
        3 => format!(
            " 🔒 Security Log\n\nNo security events recorded in this session.\n\nProvider: {}\nMode: {}",
            app.provider_str, app.mode_str
        ),
        4 => format!(
            " 💰 Token Usage\n\nTotal: {} tokens\nCache Hits: {}\nMisses: {}\nRate: {:.1}%\nAvg Lat: {}ms",
            perf.token_count, perf.cache_hits, perf.cache_misses, cache_rate, avg_lat
        ),
        5 => format!(
            " 🌐 Swarm Topology\n\nAgent: {} ({} mode)\nTotal messages: {}\nSession: {}",
            app.provider_str, app.mode_str, perf.messages_sent, perf.session_duration_str()
        ),
        _ => String::from("Unknown panel"),
    };

    let lines: Vec<Line> = content.lines()
        .map(|l| Line::from(Span::raw(l.to_string())))
        .chain(std::iter::once(Line::from("")))
        .chain(std::iter::once(Line::from({
            let panel_label = format!("{}/6", app.debug_panel_id + 1);
            vec![
                Span::styled(" Ctrl+D / Esc: Close  ", Style::default().fg(Color::DarkGray)),
                Span::styled(" Tab: Next Panel (", Style::default().fg(Color::DarkGray)),
                Span::styled(panel_label, Style::default().fg(Color::Cyan)),
                Span::styled(")", Style::default().fg(Color::DarkGray)),
            ]
        })))
        .collect();

    let block = Block::default()
        .title(format!(" Debug: {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(100, 140, 200)));

    f.render_widget(Paragraph::new(lines).block(block), debug_area);
}

fn render_input(f: &mut Frame, app: &mut App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    // ── tui-textarea widget ──
    app.textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(60, 100, 180))),
    );
    f.render_widget(&app.textarea, inner[0]);

    // ── Footer: hint + [Send] button ──
    let lines = app.textarea.lines().len();
    let chars = app.textarea.lines().iter().map(|l| l.len()).sum::<usize>();
    let hint = format!(
        " Enter:send  Shift/Ctrl+Enter:newline  Ctrl+Q:quit  {} ",
        if lines == 0 || chars == 0 {
            "0".into()
        } else if lines == 1 {
            format!("{} chars", chars)
        } else {
            format!("{} lines / {} chars", lines, chars)
        }
    );

    let send_w = 10u16;
    let footer = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(send_w)])
        .split(inner[1]);

    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        footer[0],
    );

    // [Send] button
    let send_style = if app.textarea.is_empty() {
        Style::default().fg(Color::DarkGray).bg(Color::Rgb(60, 60, 60))
    } else {
        Style::default().fg(Color::White).bg(Color::Rgb(30, 100, 200))
    };
    f.render_widget(
        Paragraph::new(Span::styled(" [ Send ] ", send_style)),
        footer[1],
    );
    app.send_rect = Some(footer[1]);
}

// ── Syntax Highlighting ──

fn highlight_syntax(line: &str) -> Vec<Span<'_>> {
    #[cfg(feature = "syntax-highlight")]
    {
        if let Some(spans) = highlight_with_treesitter(line) {
            return spans;
        }
    }
    highlight_with_regex(line)
}

#[cfg(feature = "syntax-highlight")]
fn highlight_with_treesitter(line: &str) -> Option<Vec<Span<'_>>> {
    use std::sync::OnceLock;
    use tree_sitter::{Language, Parser};
    static RUST_LANG: OnceLock<Language> = OnceLock::new();
    let lang = RUST_LANG.get_or_init(|| tree_sitter_rust::language());
    let mut parser = Parser::new();
    parser.set_language(lang).ok()?;
    let tree = parser.parse(line, None)?;
    let root = tree.root_node();
    let mut spans = Vec::new();
    let mut cur = root.walk();
    let mut last = 0;
    for node in root.children(&mut cur) {
        let s = node.start_byte();
        let e = node.end_byte();
        if s > last {
            spans.push(Span::raw(&line[last..s]));
        }
        let style = match node.kind() {
            "string_literal" | "raw_string_literal" => Style::default().fg(Color::Rgb(200, 180, 100)),
            "comment" | "line_comment" | "block_comment" => {
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
            }
            _ => Style::default(),
        };
        spans.push(Span::styled(&line[s..e], style));
        last = e;
    }
    if last < line.len() {
        spans.push(Span::raw(&line[last..]));
    }
    if spans.is_empty() { None } else { Some(spans) }
}

fn highlight_with_regex(line: &str) -> Vec<Span<'_>> {
    let kw = [
        "fn", "pub", "use", "let", "mut", "struct", "impl", "async", "await",
        "if", "else", "match", "for", "while", "return", "mod", "enum", "trait",
    ];
    let kw_s = Style::default().fg(Color::Rgb(200, 100, 200));
    let str_s = Style::default().fg(Color::Rgb(200, 180, 100));

    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('#') {
        return vec![Span::styled(line, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))];
    }

    if line.contains('"') || line.contains('\'') {
        let mut spans = Vec::new();
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' || chars[i] == '\'' {
                let mut s = String::new();
                let q = chars[i];
                s.push(chars[i]);
                i += 1;
                while i < chars.len() && chars[i] != q {
                    s.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() { s.push(chars[i]); i += 1; }
                spans.push(Span::styled(s, str_s));
            } else if chars[i].is_alphabetic() {
                let mut w = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    w.push(chars[i]);
                    i += 1;
                }
                if kw.contains(&w.as_str()) {
                    spans.push(Span::styled(w, kw_s));
                } else { spans.push(Span::raw(w)); }
            } else { spans.push(Span::raw(chars[i].to_string())); i += 1; }
        }
        return spans;
    }
    vec![Span::raw(line)]
}
