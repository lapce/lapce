//! Canvas — structured visualization components for terminal output.
//!
//! Provides table, progress bar, diff view, metric card, and dashboard
//! rendering for both TUI and plain-text modes.

use std::fmt;

// ── Table ────────────────────────────────────────────────────────────────

/// A table widget with headers, rows, sorting, and highlighting.
pub struct CanvasTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub sort_column: Option<usize>,
    pub highlight_fn: Option<Box<dyn Fn(&Vec<&str>) -> bool>>,
}

impl CanvasTable {
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
            sort_column: None,
            highlight_fn: None,
        }
    }

    pub fn add_row(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Calculate column widths based on header and row content.
    fn column_widths(&self) -> Vec<usize> {
        let col_count = self.headers.len();
        let mut widths = vec![0usize; col_count];

        for (i, h) in self.headers.iter().enumerate() {
            widths[i] = display_width(h);
        }
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate().take(col_count) {
                widths[i] = widths[i].max(display_width(cell));
            }
        }
        // Minimum width of 3 per column
        for w in widths.iter_mut() {
            *w = (*w).max(3);
        }
        widths
    }

    pub fn render(&self) -> String {
        if self.headers.is_empty() && self.rows.is_empty() {
            return "(empty table)".to_string();
        }

        let widths = self.column_widths();
        let col_count = self.headers.len();
        let total_width: usize = widths.iter().sum::<usize>() + 3 * col_count + 1; // borders + padding
        let mut out = String::with_capacity(total_width * (self.rows.len() + 4));

        // Top border
        out.push_str(&border_line(&widths, '┌', '┬', '┐'));

        // Header row
        out.push('│');
        for (i, h) in self.headers.iter().enumerate() {
            out.push(' ');
            out.push_str(h);
            out.push_str(&" ".repeat(widths[i].saturating_sub(display_width(h))));
            out.push_str(" │");
        }
        out.push('\n');

        // Separator
        out.push_str(&border_line(&widths, '├', '┼', '┤'));

        // Data rows
        for row in &self.rows {
            out.push('│');
            for (i, cell) in row.iter().enumerate().take(col_count) {
                out.push(' ');
                out.push_str(cell);
                out.push_str(&" ".repeat(widths[i].saturating_sub(display_width(cell))));
                out.push_str(" │");
            }
            out.push('\n');
        }

        // Bottom border
        out.push_str(&border_line(&widths, '└', '┴', '┘'));

        // Summary line
        if !self.rows.is_empty() {
            out.push_str(&format!("Total: {} row{}\n", self.rows.len(),
                if self.rows.len() == 1 { "" } else { "s" }));
        }

        out
    }

    pub fn width(&self) -> usize {
        let widths = self.column_widths();
        widths.iter().sum::<usize>() + 3 * self.headers.len() + 1
    }
}

impl fmt::Display for CanvasTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

// ── Progress Bar ─────────────────────────────────────────────────────────

/// Multi-step progress bar showing per-step status.
pub struct CanvasProgress {
    pub steps: Vec<ProgressStep>,
    pub current: usize,
}

#[derive(Debug, Clone)]
pub struct ProgressStep {
    pub name: String,
    pub status: StepStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl fmt::Display for StepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepStatus::Pending => write!(f, "Pending"),
            StepStatus::Running => write!(f, "Running"),
            StepStatus::Completed => write!(f, "Completed"),
            StepStatus::Failed => write!(f, "Failed"),
            StepStatus::Skipped => write!(f, "Skipped"),
        }
    }
}

impl CanvasProgress {
    pub fn new(steps: Vec<&str>) -> Self {
        Self {
            steps: steps
                .iter()
                .map(|&name| ProgressStep {
                    name: name.to_string(),
                    status: StepStatus::Pending,
                    detail: None,
                })
                .collect(),
            current: 0,
        }
    }

    /// Advance to next step, marking current as Completed.
    pub fn advance(&mut self) {
        if self.current < self.steps.len() {
            if self.steps[self.current].status == StepStatus::Running {
                self.steps[self.current].status = StepStatus::Completed;
            }
            self.current += 1;
            if self.current < self.steps.len() {
                self.steps[self.current].status = StepStatus::Running;
            }
        }
    }

    /// Set a specific step's status and optional detail message.
    pub fn set_status(&mut self, idx: usize, status: StepStatus, detail: Option<&str>) {
        if idx < self.steps.len() {
            self.steps[idx].status = status;
            self.steps[idx].detail = detail.map(|d| d.to_string());
        }
    }

    /// Check whether all steps are terminal (Completed/Failed/Skipped).
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| matches!(
            s.status,
            StepStatus::Completed | StepStatus::Failed | StepStatus::Skipped
        ))
    }

    /// Count of completed steps.
    pub fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count()
    }

    pub fn render(&self) -> String {
        if self.steps.is_empty() {
            return "(no steps)".to_string();
        }

        // Find max step label width for alignment
        let step_num_width = format!("{}", self.steps.len()).len();
        let max_name_len = self
            .steps
            .iter()
            .map(|s| display_width(&s.name))
            .max()
            .unwrap_or(0);

        // Box inner width: prefix + space + number + ": " + name + spaces + detail
        let inner_width = 2 // "│ "
            + step_num_width
            + 2 // ": "
            + max_name_len
            + 3 // "  " before detail
            + 20; // reasonable detail budget

        let box_width = inner_width.max(30);
        let mut out = String::new();

        // Rounded top border
        out.push('╭');
        out.push_str(&"─".repeat(box_width));
        out.push_str("╮\n");

        // Title line: step names joined with arrows
        let title: String = self
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join("→");
        out.push_str("│ ");
        out.push_str(truncate_str(&title, box_width.saturating_sub(2)));
        out.push_str(&" ".repeat(box_width.saturating_sub(display_width(&title)).saturating_sub(0).min(box_width)));
        out.push_str("│\n");

        // Separator
        out.push('├');
        out.push_str(&"─".repeat(box_width));
        out.push_str("┤\n");

        // Each step
        for (idx, step) in self.steps.iter().enumerate() {
            let icon = match step.status {
                StepStatus::Completed => '✓',
                StepStatus::Running => '◌',
                StepStatus::Failed => '✗',
                StepStatus::Skipped => '⊘',
                StepStatus::Pending => '○',
            };
            let num = idx + 1;
            let prefix = format!("{} Step {}: ", icon, num);
            let padded_name =
                format!("{:<width$}", step.name, width = max_name_len);

            let line_content = if let Some(ref detail) = step.detail {
                format!("{}{}  {}", prefix, padded_name, detail)
            } else {
                format!("{}{}", prefix, padded_name)
            };

            out.push_str("│ ");
            out.push_str(truncate_str(&line_content, box_width.saturating_sub(2)));
            let content_display = display_width(&line_content);
            if content_display < box_width.saturating_sub(2) {
                out.push_str(&" ".repeat(box_width.saturating_sub(2) - content_display));
            }
            out.push_str("│\n");
        }

        // Bottom border
        out.push('╰');
        out.push_str(&"─".repeat(box_width));
        out.push_str("╯\n");

        // Status summary
        let done = self.completed_count();
        let total = self.steps.len();
        let failed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Failed)
            .count();
        out.push_str(&format!(
            "Round: {}/{} | Completed: {} | Errors remaining: {}\n",
            self.current, total, done, failed
        ));

        out
    }
}

impl fmt::Display for CanvasProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

// ── Diff View ────────────────────────────────────────────────────────────

/// Diff view showing old→new content changes.
pub struct CanvasDiff {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}

pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

pub struct DiffLine {
    pub prefix: char, // '+', '-', ' '
    pub content: String,
}

impl CanvasDiff {
    /// Create a diff from two string snapshots using line-by-line comparison.
    pub fn from_strings(file_path: &str, old: &str, new: &str) -> Self {
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();

        let hunks = compute_diff_hunks(&old_lines, &new_lines);

        Self {
            file_path: file_path.to_string(),
            hunks,
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();

        // File header
        out.push_str(&format!("--- {}\n", self.file_path));
        out.push_str(&format!("+++ {}\n", self.file_path));

        if self.hunks.is_empty() {
            out.push_str("(no differences)\n");
            return out;
        }

        for hunk in &self.hunks {
            // Hunk header
            let old_count = hunk.lines.len() as u32;
            let new_count = hunk.lines.len() as u32;
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, old_count, hunk.new_start, new_count
            ));

            for line in &hunk.lines {
                out.push(line.prefix);
                out.push_str(&line.content);
                out.push('\n');
            }
        }

        out
    }
}

impl fmt::Display for CanvasDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// Compute diff hunks from two slices of lines using a simple LCS-based approach.
fn compute_diff_hunks(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffHunk> {
    use similar::{Algorithm, TextDiff};

    let old_text = old_lines.join("\n");
    let new_text = new_lines.join("\n");
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(&old_text, &new_text);

    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_idx = 1u32;
    let mut new_idx = 1u32;

    for change in diff.iter_all_changes() {
        let (prefix, is_change): (char, bool) = match change.tag() {
            similar::ChangeTag::Equal => (' ', false),
            similar::ChangeTag::Delete => ('-', true),
            similar::ChangeTag::Insert => ('+', true),
        };

        let content = change.to_string();

        if is_change || current_hunk.is_some() {
            if current_hunk.is_none() {
                current_hunk = Some(DiffHunk {
                    old_start: old_idx,
                    new_start: new_idx,
                    lines: Vec::new(),
                });
            }

            let hunk = current_hunk.as_mut().expect("unwrap failed: canvas.rs:410");
            hunk.lines.push(DiffLine { prefix, content });

            match change.tag() {
                similar::ChangeTag::Delete | similar::ChangeTag::Equal => {
                    old_idx += 1;
                }
                _ => {}
            }
            match change.tag() {
                similar::ChangeTag::Insert | similar::ChangeTag::Equal => {
                    new_idx += 1;
                }
                _ => {}
            }

            // Close hunk after a run of equal lines
            if !is_change {
                // Keep a few context lines then close
                let equal_count = hunk
                    .lines
                    .iter()
                    .rev()
                    .take_while(|l| l.prefix == ' ')
                    .count();
                if equal_count >= 3 {
                    if let Some(h) = current_hunk.take() {
                        hunks.push(h);
                    }
                }
            }
        } else {
            match change.tag() {
                similar::ChangeTag::Equal => {
                    old_idx += 1;
                    new_idx += 1;
                }
                _ => unreachable!(),
            }
        }
    }

    // Flush remaining hunk
    if let Some(h) = current_hunk.take() {
        hunks.push(h);
    }

    hunks
}

// ── Metric Card ──────────────────────────────────────────────────────────

/// Metric card showing title + value + trend indicator.
pub struct CanvasMetric {
    pub title: String,
    pub value: String,
    pub unit: Option<String>,
    pub trend: TrendDirection,
    pub sparkline: Option<Vec<u64>>, // last N values for mini chart
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendDirection {
    Up,
    Down,
    Flat,
    Unknown,
}

impl fmt::Display for TrendDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrendDirection::Up => write!(f, "↑"),
            TrendDirection::Down => write!(f, "↓"),
            TrendDirection::Flat => write!(f, "→"),
            TrendDirection::Unknown => write!(f, "?"),
        }
    }
}

impl CanvasMetric {
    pub fn new(title: &str, value: &str) -> Self {
        Self {
            title: title.to_string(),
            value: value.to_string(),
            unit: None,
            trend: TrendDirection::Unknown,
            sparkline: None,
        }
    }

    /// Set the unit label (e.g., "ms", "%", "req/s").
    pub fn with_unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.to_string());
        self
    }

    /// Set the trend direction.
    pub fn with_trend(mut self, trend: TrendDirection) -> Self {
        self.trend = trend;
        self
    }

    /// Attach sparkline data for mini chart rendering.
    pub fn with_sparkline(mut self, data: Vec<u64>) -> Self {
        self.sparkline = Some(data);
        self
    }

    /// Render a single metric card as a boxed widget.
    pub fn render(&self) -> String {
        let title_display = format!(" {} ", self.title);
        let title_width = display_width(&title_display);
        let inner_width = title_width.max(24);

        let value_with_unit = match &self.unit {
            Some(u) if !u.is_empty() => format!("{}{}", self.value, u),
            _ => self.value.clone(),
        };
        let value_line = format!(
            "         {:>10}     {}       ",
            value_with_unit, self.trend
        );

        let sparkline_str = match &self.sparkline {
            Some(data) if !data.is_empty() => render_sparkline(data),
            _ => String::new(),
        };

        let content_width = display_width(&value_line).max(
            display_width(&sparkline_str),
        );
        let box_inner = inner_width.max(content_width + 2);

        let mut out = String::new();

        // Top border with title
        out.push('┌');
        out.push_str(&"─".repeat(title_width.saturating_sub(2)));
        out.push_str(&title_display);
        if title_width + 2 < box_inner {
            out.push_str(&"─".repeat(box_inner - title_width - 2));
        }
        out.push_str("┐\n");

        // Value line
        out.push('│');
        out.push_str(&value_line);
        let val_w = display_width(&value_line);
        if val_w + 2 < box_inner {
            out.push_str(&" ".repeat(box_inner - val_w - 2));
        }
        out.push_str("│\n");

        // Sparkline (if present)
        if !sparkline_str.is_empty() {
            out.push_str("│ ");
            out.push_str(&sparkline_str);
            let sp_w = display_width(&sparkline_str) + 2;
            if sp_w < box_inner {
                out.push_str(&" ".repeat(box_inner - sp_w));
            }
            out.push_str("│\n");
        }

        // Bottom border
        out.push('└');
        out.push_str(&"─".repeat(box_inner));
        out.push_str("┘\n");

        out
    }
}

impl fmt::Display for CanvasMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// Render sparkline from numeric data using Unicode block characters.
fn render_sparkline(data: &[u64]) -> String {
    if data.is_empty() {
        return String::new();
    }

    let max_val = *data.iter().max().unwrap_or(&1).max(&1);
    // Block characters from low to high: ▁▂▃▄▅▆▇█
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    const N_LEVELS: u64 = BLOCKS.len() as u64;

    data.iter()
        .map(|&v| {
            let level = if max_val == 0 {
                0
            } else {
                (v * (N_LEVELS - 1) / max_val).min(N_LEVELS - 1)
            };
            BLOCKS[level as usize]
        })
        .collect()
}

// ── Dashboard ────────────────────────────────────────────────────────────

/// Dashboard combining multiple widgets into a unified layout.
pub struct CanvasDashboard {
    pub title: String,
    pub widgets: Vec<CanvasWidget>,
}

pub enum CanvasWidget {
    Table(CanvasTable),
    Progress(CanvasProgress),
    Metric(Vec<CanvasMetric>),
    Text(String),
    Diff(CanvasDiff),
}

impl CanvasDashboard {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            widgets: Vec::new(),
        }
    }

    pub fn add_table(&mut self, table: CanvasTable) {
        self.widgets.push(CanvasWidget::Table(table));
    }

    pub fn add_progress(&mut self, progress: CanvasProgress) {
        self.widgets.push(CanvasWidget::Progress(progress));
    }

    pub fn add_metrics(&mut self, metrics: Vec<CanvasMetric>) {
        self.widgets.push(CanvasWidget::Metric(metrics));
    }

    pub fn add_text(&mut self, text: &str) {
        self.widgets.push(CanvasWidget::Text(text.to_string()));
    }

    pub fn add_diff(&mut self, diff: CanvasDiff) {
        self.widgets.push(CanvasWidget::Diff(diff));
    }

    pub fn render(&self) -> String {
        let title_line = format!(
            "═{}═{}══",
            "═".repeat(8),
            self.title
        );
        let separator = "─".repeat(title_line.chars().count().max(40));

        let mut out = String::new();
        out.push_str(&separator);
        out.push('\n');

        for widget in &self.widgets {
            match widget {
                CanvasWidget::Table(t) => out.push_str(&t.render()),
                CanvasWidget::Progress(p) => out.push_str(&p.render()),
                CanvasWidget::Metric(metrics) => {
                    for m in metrics {
                        out.push_str(&m.render());
                    }
                }
                CanvasWidget::Text(s) => {
                    out.push_str(s);
                    if !s.ends_with('\n') {
                        out.push('\n');
                    }
                }
                CanvasWidget::Diff(d) => out.push_str(&d.render()),
            }
            // Widget separator
            out.push('\n');
        }

        out.push_str(&separator);
        out.push('\n');

        out
    }
}

impl fmt::Display for CanvasDashboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Build a horizontal border line with given corner/join characters.
fn border_line(widths: &[usize], left: char, mid: char, right: char) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, &w) in widths.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2)); // cell padding
        if i + 1 < widths.len() {
            s.push(mid);
        }
    }
    s.push(right);
    s.push('\n');
    s
}

/// Compute the display width of a string accounting for CJK / wide characters.
fn display_width(s: &str) -> usize {
    s.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1)).sum()
}

/// Truncate a string to fit within `max_width` display columns.
fn truncate_str(s: &str, max_width: usize) -> &str {
    let mut w = 0usize;
    let mut end = s.len();

    for (i, c) in s.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if w + cw > max_width {
            end = i;
            break;
        }
        w += cw;
    }

    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_basic_render() {
        let mut t = CanvasTable::new(vec![
            "ID".to_string(),
            "Status".to_string(),
            "Errors".to_string(),
            "Duration".to_string(),
        ]);
        t.add_row(vec!["task_001".into(), "SUCCESS".into(), "0".into(), "1.2s".into()]);
        t.add_row(vec!["task_002".into(), "FAILED".into(), "3".into(), "5.8s".into()]);
        let rendered = t.render();
        assert!(rendered.contains("task_001"));
        assert!(rendered.contains("Total: 2 rows"));
    }

    #[test]
    fn test_table_empty() {
        let t = CanvasTable::new(vec!["A".into()]);
        assert!(t.render().contains("(empty table)"));
    }

    #[test]
    fn test_progress_advance() {
        let mut p = CanvasProgress::new(vec!["Step A", "Step B"]);
        assert_eq!(p.current, 0);
        assert_eq!(p.steps[0].status, StepStatus::Pending);

        p.set_status(0, StepStatus::Running, None);
        p.advance();
        assert_eq!(p.current, 1);
        assert_eq!(p.steps[0].status, StepStatus::Completed);
        assert_eq!(p.steps[1].status, StepStatus::Running);
    }

    #[test]
    fn test_progress_complete() {
        let mut p = CanvasProgress::new(vec!["One"]);
        p.set_status(0, StepStatus::Completed, None);
        assert!(p.is_complete());
    }

    #[test]
    fn test_diff_from_strings() {
        let d = CanvasDiff::from_strings("test.txt", "hello\nworld\n", "hello\nthere\n");
        let rendered = d.render();
        assert!(rendered.contains("--- test.txt"));
        assert!(rendered.contains("@@"));
    }

    #[test]
    fn test_metric_basic() {
        let m = CanvasMetric::new("Latency", "142")
            .with_unit("ms")
            .with_trend(TrendDirection::Up)
            .with_sparkline(vec![1, 3, 5, 7, 6, 4, 2, 1]);
        let r = m.render();
        assert!(r.contains("Latency"));
        assert!(r.contains("142ms"));
        assert!(r.contains("↑"));
    }

    #[test]
    fn test_dashboard_render() {
        let mut dash = CanvasDashboard::new("Report");
        dash.add_text("Summary line");
        let mut t = CanvasTable::new(vec!["Key".into(), "Value".into()]);
        t.add_row(vec!["a".into(), "1".into()]);
        dash.add_table(t);
        let r = dash.render();
        assert!(r.contains("Report"));
        assert!(r.contains("Summary line"));
        assert!(r.contains("a"));
    }

    #[test]
    fn test_display_width_cjk() {
        assert_eq!(display_width("你好"), 4); // each CJK char is width 2
        assert_eq!(display_width("abc"), 3);
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn test_sparkline() {
        let sl = render_sparkline(&[0, 2, 5, 8, 5, 2, 0]);
        assert!(!sl.is_empty());
        // Should contain block characters
        assert!(sl.chars().all(|c| "▁▂▃▄▅▆▇█".contains(c)));
    }

    #[test]
    fn test_trend_direction_display() {
        assert_eq!(format!("{}", TrendDirection::Up), "↑");
        assert_eq!(format!("{}", TrendDirection::Down), "↓");
        assert_eq!(format!("{}", TrendDirection::Flat), "→");
        assert_eq!(format!("{}", TrendDirection::Unknown), "?");
    }

    #[test]
    fn test_step_status_display() {
        assert_eq!(format!("{}", StepStatus::Completed), "Completed");
        assert_eq!(format!("{}", StepStatus::Failed), "Failed");
    }
}
