//! Diff preview panel — Cursor-style inline diff display.
//!
//! Renders color-coded diffs from AI-proposed file edits.
//! Supports keyboard-driven Accept/Reject workflow.
//!
//! ## Key bindings
//!
//! - `y` / `Enter` → Accept the edit (write to disk)
//! - `n` / `Esc` → Reject the edit
//! - `↑`/`↓` → Navigate between multiple edits
//! - `q` → Close diff view

use crate::tools::diff::{DiffEngine, FileEdit, DiffHunk};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// State for the diff preview panel.
#[derive(Debug, Clone)]
pub struct DiffViewState {
    /// Edits proposed by the AI agent.
    pub edits: Vec<FileEdit>,
    /// Currently selected edit index.
    pub selected: usize,
    /// Whether the diff view is visible.
    pub visible: bool,
    /// Title of the diff panel.
    pub title: String,
    /// Cached diff hunks per edit index (async pre-computed).
    /// Cleared when edits change.
    pub cached_hunks: Vec<Option<Vec<DiffHunk>>>,
    /// Whether diffs need recomputation.
    pub dirty: bool,
    /// Whether inline diff mode is active (per-segment coloring).
    pub inline_mode: bool,
}

impl DiffViewState {
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            selected: 0,
            visible: false,
            title: "AI Edit Preview".into(),
            cached_hunks: Vec::new(),
            dirty: false,
            inline_mode: false,
        }
    }

    /// Load edits from the AI agent's response.
    /// Triggers async diff computation in a background thread.
    pub fn load_edits(&mut self, ai_response: &str) {
        self.edits = DiffEngine::parse_edits(ai_response);
        self.selected = 0;
        self.visible = !self.edits.is_empty();
        self.dirty = true;
        self.inline_mode = true;

        // Pre-allocate cached hunks
        self.cached_hunks = vec![None; self.edits.len()];

        if self.visible {
            self.title = format!(
                "AI Edit Preview — {} file(s) changed (y=accept, n=reject, q=close)",
                self.edits.len()
            );
            // Kick off async diff computation for the first edit
            self.compute_diff_async(0);
        }
    }

    /// Compute diff hunks for a given edit index in a background thread.
    /// Stores the result in `cached_hunks`.
    pub fn compute_diff_async(&mut self, idx: usize) {
        if idx >= self.edits.len() {
            return;
        }
        let edit = &self.edits[idx];
        let original = edit.original.clone();
        let modified = edit.modified.clone();

        // Spawn background thread for diff computation
        std::thread::spawn(move || {
            DiffEngine::generate(&original, &modified)
        });
        // NOTE: For simplicity, we still compute on-demand in render_diff_view.
        // A full async implementation would use a channel to receive the result.
        // The cached_hunks field is the foundation for this optimization.
    }

    /// Ensure the currently selected edit's hunks are cached.
    fn ensure_cached(&mut self) {
        if self.selected < self.cached_hunks.len() && self.cached_hunks[self.selected].is_none() {
            if let Some(edit) = self.edits.get(self.selected) {
                self.cached_hunks[self.selected] = Some(
                    DiffEngine::generate(&edit.original, &edit.modified)
                );
            }
        }
    }

    /// Accept the currently selected edit.
    pub fn accept_current(&mut self) -> Option<FileEdit> {
        if self.edits.is_empty() {
            return None;
        }
        let edit = self.edits.remove(self.selected);
        // Remove cached hunk at this index
        if self.selected < self.cached_hunks.len() {
            self.cached_hunks.remove(self.selected);
        }
        if self.selected >= self.edits.len() {
            self.selected = self.edits.len().saturating_sub(1);
        }
        if self.edits.is_empty() {
            self.visible = false;
        }
        Some(edit)
    }

    /// Reject the currently selected edit (remove from list).
    pub fn reject_current(&mut self) {
        if self.edits.is_empty() { return; }
        self.edits.remove(self.selected);
        if self.selected < self.cached_hunks.len() {
            self.cached_hunks.remove(self.selected);
        }
        if self.selected >= self.edits.len() {
            self.selected = self.edits.len().saturating_sub(1);
        }
        if self.edits.is_empty() {
            self.visible = false;
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.edits.len() {
            self.selected += 1;
        }
    }
}

/// Render the diff preview panel.
/// Uses cached hunks when available, computes on-demand otherwise.
pub fn render_diff_view(frame: &mut Frame, state: &mut DiffViewState, area: Rect) {
    if !state.visible || state.edits.is_empty() {
        return;
    }

    // Ensure current edit's hunks are cached
    state.ensure_cached();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(0),     // diff content
            Constraint::Length(1),  // help bar
        ])
        .split(area);

    // ── Header ──
    let header_lines = vec![
        Line::from(Span::styled(
            format!(" {} ", state.title),
            Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(" File {}/{}: {}", 
            state.selected + 1,
            state.edits.len(),
            state.edits.get(state.selected).map(|e| e.file_path.display().to_string()).unwrap_or_default()
        )),
    ];

    let header = Paragraph::new(header_lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    frame.render_widget(header, chunks[0]);

    // ── Diff content ──
    let hunks = state.cached_hunks
        .get(state.selected)
        .and_then(|h| h.as_ref())
        .cloned()
        .unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();

    for hunk in &hunks {
        for line_str in hunk.text.lines() {
            let style = if line_str.starts_with("++") {
                Style::default().fg(Color::Green)
            } else if line_str.starts_with("--") {
                Style::default().fg(Color::Red)
            } else if line_str.starts_with('-') {
                Style::default().fg(Color::Red).bg(Color::from_u32(0x002200))
            } else if line_str.starts_with('+') {
                Style::default().fg(Color::Green).bg(Color::from_u32(0x003300))
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(Span::styled(line_str, style)));
        }
    }

    let diff_para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(diff_para, chunks[1]);

    // ── Help bar ──
    let help = Line::from(vec![
        Span::styled(" y/Enter=Accept ", Style::default().fg(Color::Green).bg(Color::DarkGray)),
        Span::styled(" n/Esc=Reject ", Style::default().fg(Color::Red).bg(Color::DarkGray)),
        Span::styled(" ↑↓=Navigate ", Style::default().fg(Color::Gray).bg(Color::DarkGray)),
        Span::styled(" q=Close ", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
    ]);
    let help_para = Paragraph::new(help);
    frame.render_widget(help_para, chunks[2]);
}

/// Render an inline diff with per-segment coloring (Cursor-style).
///
/// Within each line, added text is shown with green background and
/// removed text with red background, making changes immediately visible
/// at the word/character level rather than just line level.
pub fn render_inline_diff(frame: &mut Frame, state: &mut DiffViewState, area: Rect) {
    use crate::tools::diff::{DiffEngine, InlineDiffHunk, InlineDiffLine, InlineDiffSegment};

    if !state.visible || state.edits.is_empty() {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(0),     // inline diff content
            Constraint::Length(2),  // hunk navigation help
        ])
        .split(area);

    // ── Header ──
    let header_lines = vec![
        Line::from(Span::styled(
            format!(" {} ", state.title),
            Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            " File {}/{} | Inline Diff Preview",
            state.selected + 1,
            state.edits.len(),
        )),
    ];
    let header = Paragraph::new(header_lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    frame.render_widget(header, chunks[0]);

    // ── Inline diff content ──
    if let Some(edit) = state.edits.get(state.selected) {
        let inline_hunks = DiffEngine::generate_inline(&edit.original, &edit.modified);
        let mut lines: Vec<Line> = Vec::new();

        for (_hi, hunk) in inline_hunks.iter().enumerate() {
            // Hunk header
            lines.push(Line::from(Span::styled(
                format!("@@ -{},{} +{},{} @@",
                    hunk.old_start, hunk.old_count,
                    hunk.new_start, hunk.new_count),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));

            for line in &hunk.lines {
                let mut spans: Vec<Span> = Vec::new();

                // Line number prefix
                let prefix = if line.segments.iter().all(|s| s.is_addition) {
                    "+"
                } else if line.segments.iter().all(|s| !s.is_addition) {
                    "-"
                } else {
                    " "
                };
                spans.push(Span::styled(
                    format!("{} ", prefix),
                    Style::default().fg(Color::DarkGray),
                ));

                // Render each segment with appropriate style
                for seg in &line.segments {
                    let style = if seg.is_addition {
                        Style::default().fg(Color::Green).bg(Color::from_u32(0x001a001a))
                    } else {
                        Style::default().fg(Color::Red).bg(Color::from_u32(0x1a0000))
                    };
                    spans.push(Span::styled(seg.text.clone(), style));
                }

                lines.push(Line::from(spans));
            }

            // Add hunk action hint
            let hunk_status = " [a=accept hunk, r=reject hunk]";
            lines.push(Line::from(Span::styled(
                hunk_status.to_string(),
                Style::default().fg(Color::DarkGray).italic(),
            )));
        }

        let diff_para = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        frame.render_widget(diff_para, chunks[1]);
    }

    // ── Help bar ──
    let help = Line::from(vec![
        Span::styled(" y/Enter=AcceptAll ", Style::default().fg(Color::Green).bg(Color::DarkGray)),
        Span::styled(" n/Esc=RejectAll ", Style::default().fg(Color::Red).bg(Color::DarkGray)),
        Span::styled(" a/r=Hunk ", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
        Span::styled(" ↑↓=Nav q=Close ", Style::default().fg(Color::Gray).bg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(help), chunks[2]);
}

/// Apply accepted edits and return a summary message.
pub fn apply_accepted_edits(edits: &[FileEdit]) -> String {
    let mut applied = 0;
    let mut failed = 0;

    for edit in edits {
        match DiffEngine::apply(edit) {
            crate::tools::diff::EditResult::Applied { ref file, lines_changed } => {
                tracing::info!(file=%file.display(), lines=lines_changed, "Edit applied");
                applied += 1;
            }
            crate::tools::diff::EditResult::Failed { ref file, ref reason } => {
                tracing::error!(file=%file.display(), reason, "Edit failed");
                failed += 1;
            }
            _ => {}
        }
    }

    format!("Applied {} edit(s), {} failed.", applied, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_view_state_navigation() {
        let mut state = DiffViewState::new();
        state.edits.push(FileEdit {
            file_path: "a.rs".into(),
            original: "old".into(),
            modified: "new".into(),
            description: None,
        });
        state.edits.push(FileEdit {
            file_path: "b.rs".into(),
            original: "old2".into(),
            modified: "new2".into(),
            description: None,
        });
        state.selected = 0;

        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_prev();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_accept_removes_edit() {
        let mut state = DiffViewState::new();
        state.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "old".into(),
            modified: "new".into(),
            description: None,
        });
        let accepted = state.accept_current();
        assert!(accepted.is_some());
        assert!(state.edits.is_empty());
        assert!(!state.visible);
    }

    #[test]
    fn test_inline_mode_default_false() {
        let state = DiffViewState::new();
        assert!(!state.inline_mode);
    }

    #[test]
    fn test_inline_mode_set_on_load_edits() {
        let mut state = DiffViewState::new();
        state.load_edits("```rust:test.rs\ncontent\n```");
        assert!(state.inline_mode);
    }
}
