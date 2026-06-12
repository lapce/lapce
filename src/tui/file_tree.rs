//! File Tree Navigator - Project file browsing
//!
//! Provides an interactive file tree view with:
//! - Directory expansion/collapse
//! - File filtering
//! - Selection and navigation
//! - Icons for different file types

use std::collections::HashMap;
use std::path::PathBuf;

/// File or directory entry
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub is_selected: bool,
    pub depth: usize,
    pub children: Vec<FileEntry>,
    pub extension: Option<String>,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
}

impl FileEntry {
    pub fn new(path: PathBuf, is_dir: bool, depth: usize) -> Self {
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let extension = if is_dir {
            None
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
        };

        let metadata = std::fs::metadata(&path).ok();

        Self {
            name,
            path,
            is_dir,
            is_expanded: false,
            is_selected: false,
            depth,
            children: Vec::new(),
            extension,
            size: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: metadata.and_then(|m| m.modified()).ok(),
        }
    }

    pub fn icon(&self) -> &'static str {
        if self.is_dir {
            if self.is_expanded {
                "📂" // 📂 expanded folder
            } else {
                "📁" // 📁 closed folder
            }
        } else {
            match self.extension.as_deref() {
                Some("rs") => "🦀",      // Rust
                Some("ts") | Some("tsx") => "🔷", // TypeScript
                Some("js") | Some("jsx") => "🟨", // JavaScript
                Some("py") => "🐍",      // Python
                Some("go") => "🐹",      // Go
                Some("java") => "☕",     // Java
                Some("c") | Some("h") => "🔧", // C
                Some("cpp") | Some("hpp") | Some("cc") => "⚙️", // C++
                Some("md") | Some("mdx") => "📝", // Markdown
                Some("json") => "📋",    // JSON
                Some("yaml") | Some("yml") => "📄", // YAML
                Some("toml") => "⚙️",    // TOML
                Some("css") => "🎨",      // CSS
                Some("html") => "🌐",    // HTML
                Some("svg") => "🖼️",     // SVG
                Some("png") | Some("jpg") | Some("jpeg") | Some("gif") => "🖼️", // Images
                Some("txt") => "📃",     // Text
                Some("lock") => "🔒",    // Lock file
                Some("gitignore") | Some("dockerignore") => "👁️", // Ignore files
                Some("env") => "🔐",     // Environment
                _ => "📄",               // Default
            }
        }
    }

    pub fn formatted_size(&self) -> String {
        if self.is_dir {
            String::new()
        } else {
            format_size(self.size)
        }
    }
}

/// Format file size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// File tree state
pub struct FileTree {
    pub root: Vec<FileEntry>,
    pub root_path: PathBuf,
    pub expanded_paths: HashMap<String, bool>,
    pub selected_index: usize,
    pub visible_entries: Vec<(usize, usize)>, // (parent_idx, child_idx)
    pub filter: Option<String>,
    pub show_hidden: bool,
    pub excluded_dirs: Vec<String>,
}

impl FileTree {
    pub fn new(root_path: PathBuf) -> Self {
        Self {
            root: Vec::new(),
            root_path,
            expanded_paths: HashMap::new(),
            selected_index: 0,
            visible_entries: Vec::new(),
            filter: None,
            show_hidden: false,
            excluded_dirs: vec![
                "node_modules".to_string(),
                "target".to_string(),
                ".git".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
                "dist".to_string(),
                "build".to_string(),
                ".idea".to_string(),
                ".vscode".to_string(),
                "vendor".to_string(),
            ],
        }
    }

    /// Load directory tree
    pub fn load(&mut self) -> std::io::Result<()> {
        self.root.clear();
        self.build_tree(&self.root_path, 0)
    }

    fn build_tree(&mut self, path: &std::path::Path, depth: usize) -> std::io::Result<()> {
        if !path.is_dir() {
            return Ok(());
        }

        let entries = std::fs::read_dir(path)?;
        
        let mut items: Vec<FileEntry> = entries
            .filter_map(|e| e.ok())
            .map(|e| FileEntry::new(e.path(), e.path().is_dir(), depth))
            .collect();

        // Sort: directories first, then alphabetically
        items.sort_by(|a, b| {
            if a.is_dir == b.is_dir {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            } else if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });

        for item in items {
            // Skip hidden files if not showing
            if !self.show_hidden && item.name.starts_with('.') {
                continue;
            }

            // Skip excluded directories
            if item.is_dir && self.excluded_dirs.contains(&item.name) {
                continue;
            }

            // Apply filter
            if let Some(ref filter) = self.filter {
                if !item.name.to_lowercase().contains(&filter.to_lowercase()) {
                    continue;
                }
            }

            // Recursively load directory contents
            let mut entry = item;
            if entry.is_dir {
                let children = self.load_children(&entry.path, depth + 1)?;
                entry.children = children;
            }

            self.root.push(entry);
        }

        Ok(())
    }

    fn load_children(&mut self, path: &std::path::Path, depth: usize) -> std::io::Result<Vec<FileEntry>> {
        if !path.is_dir() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(path)?;
        let mut items: Vec<FileEntry> = entries
            .filter_map(|e| e.ok())
            .map(|e| FileEntry::new(e.path(), e.path().is_dir(), depth))
            .collect();

        items.sort_by(|a, b| {
            if a.is_dir == b.is_dir {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            } else if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });

        let mut result = Vec::new();
        for item in items {
            if !self.show_hidden && item.name.starts_with('.') {
                continue;
            }

            if item.is_dir && self.excluded_dirs.contains(&item.name) {
                continue;
            }

            if let Some(ref filter) = self.filter {
                if !item.name.to_lowercase().contains(&filter.to_lowercase()) {
                    continue;
                }
            }

            let mut entry = item;
            if entry.is_dir {
                let children = self.load_children(&entry.path, depth + 1)?;
                entry.children = children;
            }

            result.push(entry);
        }

        Ok(result)
    }

    /// Build visible entries list
    pub fn build_visible(&mut self) {
        self.visible_entries.clear();
        self.selected_index = 0;

        for (parent_idx, entry) in self.root.iter().enumerate() {
            self.add_visible_entry(parent_idx, 0, &entry);
        }
    }

    fn add_visible_entry(&mut self, parent_idx: usize, child_idx: usize, entry: &FileEntry) {
        // Add this entry
        if !entry.is_dir || entry.is_expanded {
            let idx = self.visible_entries.len();
            self.visible_entries.push((parent_idx, child_idx));
        }

        // Add children if expanded
        if entry.is_dir && entry.is_expanded {
            for (i, child) in entry.children.iter().enumerate() {
                self.add_visible_entry(parent_idx, child_idx + 1 + i, child);
            }
        }
    }

    /// Toggle expansion
    pub fn toggle_expand(&mut self, visible_idx: usize) {
        if visible_idx >= self.visible_entries.len() {
            return;
        }

        let (parent_idx, _) = self.visible_entries[visible_idx];
        if let Some(entry) = self.root.get_mut(parent_idx) {
            entry.is_expanded = !entry.is_expanded;
            self.expanded_paths.insert(
                entry.path.to_string_lossy().to_string(),
                entry.is_expanded,
            );
            self.build_visible();
        }
    }

    /// Select entry
    pub fn select(&mut self, visible_idx: usize) {
        // Clear previous selection
        for entry in &mut self.root {
            entry.is_selected = false;
        }

        if visible_idx < self.visible_entries.len() {
            let (parent_idx, _) = self.visible_entries[visible_idx];
            if let Some(entry) = self.root.get_mut(parent_idx) {
                entry.is_selected = true;
            }
            self.selected_index = visible_idx;
        }
    }

    /// Move selection
    pub fn move_selection(&mut self, direction: SelectionDirection) {
        let new_idx = match direction {
            SelectionDirection::Up => self.selected_index.saturating_sub(1),
            SelectionDirection::Down => (self.selected_index + 1).min(self.visible_entries.len().saturating_sub(1)),
            SelectionDirection::PageUp => self.selected_index.saturating_sub(10),
            SelectionDirection::PageDown => (self.selected_index + 10).min(self.visible_entries.len().saturating_sub(1)),
            SelectionDirection::Top => 0,
            SelectionDirection::Bottom => self.visible_entries.len().saturating_sub(1),
        };

        self.select(new_idx);
    }

    /// Get selected entry
    pub fn selected_entry(&self) -> Option<&FileEntry> {
        if self.selected_index < self.visible_entries.len() {
            let (parent_idx, _) = self.visible_entries[self.selected_index];
            self.root.get(parent_idx)
        } else {
            None
        }
    }

    /// Set filter
    pub fn set_filter(&mut self, filter: Option<String>) {
        self.filter = filter;
        self.build_visible();
    }

    /// Toggle hidden files
    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        let _ = self.load();
        self.build_visible();
    }

    /// Get total entries count
    pub fn total_entries(&self) -> usize {
        self.visible_entries.len()
    }

    /// Get selected path
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|e| e.path.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SelectionDirection {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// Render file tree for TUI
pub fn render_file_tree(tree: &FileTree, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for (idx, &(parent_idx, depth)) in tree.visible_entries.iter().enumerate() {
        if let Some(entry) = tree.root.get(parent_idx) {
            let indent = "  ".repeat(depth);
            let icon = entry.icon();
            let selection = if entry.is_selected { " > " } else { "   " };
            let name = if entry.is_selected {
                format!("\x1b[1;37m{}\x1b[0m", entry.name) // Bold white
            } else {
                entry.name.clone()
            };

            let line = format!("{}{}{} {}", indent, selection, icon, name);
            lines.push(line);
        }
    }

    lines
}

/// File tree with tab support
pub struct FileTreeTab {
    pub tabs: Vec<FileTree>,
    pub active_tab: usize,
    pub show_tree: bool,
}

impl FileTreeTab {
    pub fn new(root_path: PathBuf) -> Self {
        let mut tree = FileTree::new(root_path);
        let _ = tree.load();
        tree.build_visible();

        Self {
            tabs: vec![tree],
            active_tab: 0,
            show_tree: true,
        }
    }

    pub fn active_tree(&mut self) -> &mut FileTree {
        &mut self.tabs[self.active_tab]
    }

    pub fn add_tab(&mut self, path: PathBuf) {
        let mut tree = FileTree::new(path);
        let _ = tree.load();
        tree.build_visible();
        self.tabs.push(tree);
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() > 1 {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
    }

    pub fn prev_tab(&mut self) {
        if self.active_tab == 0 {
            self.active_tab = self.tabs.len() - 1;
        } else {
            self.active_tab -= 1;
        }
    }
}
