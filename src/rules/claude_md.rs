//! CLAUDE.md upwards discovery + merge — inspired by Claude Code.
//!
//! Walks from a target directory upward toward the filesystem root,
//! collecting every `CLAUDE.md`, `AGENTS.md`, `.cursorrules`, `.windsurfrules`,
//! `.clinerules`, and `.github/copilot-instructions.md` file found.
//! Inner (closer to target) files override outer files on a per-section
//! basis. Auto-merges everything into a single ordered prompt block for
//! the AI system prompt.

use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_DEPTH_UP: usize = 20;

const KNOWN_FILENAMES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
];

const SKIP_DIRS: &[&str] = &["node_modules", ".git", "target", ".venv", "venv", "__pycache__"];

const MERGE_SECTIONS: &[&str] = &["Role", "Constraints", "Rules", "Memory", "MCP", "Workflow"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredEntry {
    pub path: PathBuf,
    pub filename: String,
    pub is_inner: bool,
}

#[derive(Debug, Clone)]
pub struct ClaudeMdStack {
    pub entries: Vec<DiscoveredEntry>,
}

impl ClaudeMdStack {
    pub fn discover(target_dir: &Path) -> Self {
        let entries = discover_all(target_dir);
        Self { entries }
    }

    pub fn discover_with_depth(target_dir: &Path, max_depth_up: usize) -> Self {
        let entries = discover_all_with_depth(target_dir, max_depth_up);
        Self { entries }
    }

    pub fn sources(&self) -> Vec<&DiscoveredEntry> {
        self.entries.iter().collect()
    }

    pub fn render_merged(&self) -> String {
        render_merged(&self.entries)
    }
}

pub fn discover_all(target_dir: &Path) -> Vec<DiscoveredEntry> {
    discover_all_with_depth(target_dir, MAX_DEPTH_UP)
}

pub fn discover_all_with_depth(target_dir: &Path, max_depth_up: usize) -> Vec<DiscoveredEntry> {
    let mut result: Vec<DiscoveredEntry> = Vec::new();

    let mut current: Option<&Path> = Some(target_dir);
    let mut depth: usize = 0;

    while let Some(dir) = current {
        if depth > max_depth_up {
            break;
        }

        let dir_name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if SKIP_DIRS.contains(&dir_name.as_str()) {
            current = dir.parent();
            depth += 1;
            continue;
        }

        let mut found_any = false;

        for name in KNOWN_FILENAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                result.push(DiscoveredEntry {
                    path: candidate,
                    filename: name.to_string(),
                    is_inner: false,
                });
                found_any = true;
            }
        }

        let copilot_path = dir.join(".github").join("copilot-instructions.md");
        if copilot_path.is_file() {
            result.push(DiscoveredEntry {
                path: copilot_path,
                filename: "copilot-instructions.md".to_string(),
                is_inner: false,
            });
            found_any = true;
        }

        let _ = found_any;

        match dir.parent() {
            Some(parent) if parent != dir => {
                current = Some(parent);
            }
            _ => break,
        }
        depth += 1;
    }

    result.reverse();

    let total = result.len();
    result.iter_mut().enumerate().for_each(|(i, e)| {
        e.is_inner = i == total.saturating_sub(1);
    });

    result
}

fn relpath_for_render(entry_path: &Path, target_root: &Path) -> String {
    if let Ok(rel) = entry_path.strip_prefix(target_root) {
        rel.display().to_string()
    } else {
        entry_path.display().to_string()
    }
}

struct SectionBlock {
    name: String,
    body: String,
    #[allow(dead_code)]
    source_relpath: String,
    source_idx: usize,
}

struct FileSections {
    file_block: String,
    sections: Vec<SectionBlock>,
}

fn parse_sections(content: &str, is_sectional: bool, source_relpath: String, source_idx: usize) -> FileSections {
    let file_block = format!("# File: {}\n\n{}\n", source_relpath.trim_end_matches('/'), content.trim());

    let mut sections: Vec<SectionBlock> = Vec::new();

    if !is_sectional {
        sections.push(SectionBlock {
            name: "General rules".to_string(),
            body: content.trim().to_string(),
            source_relpath,
            source_idx,
        });
        return FileSections { file_block, sections };
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut current_name = "Role".to_string();
    let mut current_body = String::new();
    let mut got_any = false;

    for line in lines {
        if let Some(rest) = line.strip_prefix("# ") {
            if got_any {
                let body = current_body.trim().to_string();
                if !body.is_empty() {
                    sections.push(SectionBlock {
                        name: std::mem::take(&mut current_name),
                        body,
                        source_relpath: source_relpath.clone(),
                        source_idx,
                    });
                }
            }
            let header = rest.trim();
            current_name = normalize_section_name(header);
            current_body.clear();
            got_any = true;
        } else if !line.is_empty() || !current_body.is_empty() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line);
        }
    }
    if got_any {
        let body = current_body.trim().to_string();
        if !body.is_empty() {
            sections.push(SectionBlock {
                name: std::mem::take(&mut current_name),
                body,
                source_relpath: source_relpath.clone(),
                source_idx,
            });
        }
    } else {
        let body = content.trim().to_string();
        if !body.is_empty() {
            sections.push(SectionBlock {
                name: "General rules".to_string(),
                body,
                source_relpath: source_relpath.clone(),
                source_idx,
            });
        }
    }

    FileSections { file_block, sections }
}

fn normalize_section_name(raw: &str) -> String {
    let trimmed = raw.trim();
    for known in MERGE_SECTIONS {
        if trimmed.eq_ignore_ascii_case(known) {
            return (*known).to_string();
        }
    }
    trimmed.to_string()
}

fn is_sectional_file(filename: &str) -> bool {
    !matches!(filename, ".cursorrules" | ".windsurfrules")
}

pub fn render_merged(entries: &[DiscoveredEntry]) -> String {
    let mut all_file_blocks: Vec<String> = Vec::new();
    let mut merged_sections: Vec<SectionBlock> = Vec::new();
    let mut sources_line = String::new();

    let mut target_root: Option<PathBuf> = None;
    for e in entries.iter() {
        if let Some(dir) = parent_dir(&e.path) {
            if !SKIP_DIRS.contains(&dir.file_name().and_then(|s| s.to_str()).unwrap_or("")) {
                match &target_root {
                    None => target_root = Some(dir.to_path_buf()),
                    Some(existing) => {
                        // take the deepest common parent as target root
                        let longer = if existing.components().count() > dir.components().count() {
                            existing.clone()
                        } else {
                            dir.to_path_buf()
                        };
                        target_root = Some(longer);
                    }
                }
            }
        }
    }

    let target_root = match target_root {
        Some(r) => r,
        None => PathBuf::from("."),
    };

    for (idx, entry) in entries.iter().enumerate() {
        let raw = match fs::read_to_string(&entry.path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = relpath_for_render(&entry.path, &target_root);
        if sources_line.is_empty() {
            sources_line = format!("Sources (outer → inner): {}", rel);
        } else {
            sources_line.push_str(" ← ");
            sources_line.push_str(&rel);
        }

        let sectional = is_sectional_file(&entry.filename);
        let parsed = parse_sections(&raw, sectional, rel, idx);

        all_file_blocks.push(parsed.file_block);

        for sec in parsed.sections {
            let existing = merged_sections.iter().position(|s| s.name == sec.name);
            match existing {
                Some(pos) => {
                    let keep = merged_sections[pos].source_idx;
                    if sec.source_idx > keep {
                        merged_sections[pos] = sec;
                    }
                }
                None => merged_sections.push(sec),
            }
        }
    }

    let mut out = String::new();
    out.push_str("# === CLAUDE/AGENTS Rules ===\n\n");
    if !sources_line.is_empty() {
        out.push_str(&sources_line);
        out.push('\n');
    }
    out.push_str("--- Per-file blocks ---\n");
    for block in &all_file_blocks {
        out.push_str(block);
        out.push('\n');
    }
    out.push_str("--- Merged (inner wins per section) ---\n");
    for sec in &merged_sections {
        out.push_str(&format!("# {}\n\n{}\n\n", sec.name, sec.body));
    }
    out.push_str("# === End CLAUDE/AGENTS Rules ===\n");

    out
}

fn parent_dir(p: &Path) -> Option<&Path> {
    let c = p.components().last()?;
    if let Component::Normal(_) = c {
        p.parent()
    } else {
        Some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_dir_chain() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tmp");
        let a = root.path().join("proj");
        let b = a.join("sub");
        fs::create_dir_all(&b).expect("mkdir");
        (root, a, b)
    }

    #[test]
    fn test_discover_fake_chain() {
        let (root, a, b) = create_dir_chain();

        let outer = a.join("CLAUDE.md");
        let inner = b.join("CLAUDE.md");
        fs::write(&outer, "# Role\nouter role\n").expect("write outer");
        fs::write(&inner, "# Role\ninner role\n").expect("write inner");

        let stack = ClaudeMdStack::discover(&b);
        let sources = stack.sources();
        assert!(sources.len() >= 2, "expected >=2 entries, got {}", sources.len());

        let names: Vec<&str> = sources.iter().map(|e| e.filename.as_str()).collect();
        assert!(names.contains(&"CLAUDE.md"));

        let _ = root;
    }

    #[test]
    fn test_render_merged_produces_header() {
        let (root, a, b) = create_dir_chain();

        let outer = a.join("CLAUDE.md");
        fs::write(&outer, "# Role\nouter role\n# Rules\nouter rules\n").expect("write");

        let inner = b.join("CLAUDE.md");
        fs::write(&inner, "# Role\ninner role\n").expect("write");

        let stack = ClaudeMdStack::discover(&b);
        let rendered = stack.render_merged();

        assert!(
            rendered.contains("CLAUDE/AGENTS Rules"),
            "rendered = {}",
            rendered
        );
        assert!(
            rendered.contains("inner role"),
            "inner role should win, rendered = {}",
            rendered
        );
        assert!(
            rendered.contains("outer rules"),
            "outer Rules section should survive, rendered = {}",
            rendered
        );

        let _ = root;
    }

    #[test]
    fn test_cursorrules_wrapped_as_general() {
        let root = tempfile::tempdir().unwrap();
        let proj = root.path().join("p");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join(".cursorrules"), "no headers just text").unwrap();

        let stack = ClaudeMdStack::discover(&proj);
        let rendered = stack.render_merged();
        assert!(rendered.contains("General rules"), "rendered = {}", rendered);
    }
}
