use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use itertools::Itertools;
use lapce_rpc::style::Style;
use tree_sitter::{Node, Parser, Point, Tree};
use xi_rope::{
    spans::{Spans, SpansBuilder},
    Interval, Rope, RopeDelta,
};

use crate::{
    language::LapceLanguage,
    lens::{Lens, LensBuilder},
    style::{Highlight, HighlightEvent, Highlighter, SCOPES},
};

thread_local! {
   static PARSER: RefCell<HashMap<LapceLanguage, Parser>> = RefCell::new(HashMap::new());
   static HIGHLIGHTS: RefCell<HashMap<LapceLanguage, crate::style::HighlightConfiguration>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Syntax {
    rev: u64,
    pub language: LapceLanguage,
    pub text: Rope,
    tree: Option<Tree>,
    pub lens: Lens,
    pub normal_lines: Vec<usize>,
    pub line_height: usize,
    pub lens_height: usize,
    pub styles: Option<Arc<Spans<Style>>>,
}

impl std::fmt::Debug for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Syntax")
            .field("rev", &self.rev)
            .field("language", &self.language)
            .field("text", &self.text)
            .field("tree", &self.tree)
            .field("normal_lines", &self.normal_lines)
            .field("line_height", &self.line_height)
            .field("lens_height", &self.lens_height)
            .field("styles", &self.styles)
            .finish()
    }
}

impl Syntax {
    pub fn init(path: &Path) -> Option<Syntax> {
        LapceLanguage::from_path(path).map(|l| Syntax {
            rev: 0,
            language: l,
            text: Rope::from(""),
            tree: None,
            lens: Self::lens_from_normal_lines(0, 0, 0, &Vec::new()),
            line_height: 0,
            lens_height: 0,
            normal_lines: Vec::new(),
            styles: None,
        })
    }

    pub fn from_language(language: LapceLanguage) -> Syntax {
        Syntax {
            rev: 0,
            language,
            text: Rope::from(""),
            tree: None,
            lens: Self::lens_from_normal_lines(0, 0, 0, &Vec::new()),
            line_height: 0,
            lens_height: 0,
            normal_lines: Vec::new(),
            styles: None,
        }
    }

    pub fn parse(
        &self,
        new_rev: u64,
        new_text: Rope,
        delta: Option<RopeDelta>,
    ) -> Syntax {
        let mut old_tree = None;
        if new_rev == self.rev + 1 {
            if let Some(delta) = delta {
                fn point_at_offset(text: &Rope, offset: usize) -> Point {
                    let line = text.line_of_offset(offset);
                    let col = text.offset_of_line(line + 1) - offset;
                    Point::new(line, col)
                }
                let (interval, _) = delta.summary();
                let (start, end) = interval.start_end();
                if let Some(inserted) = delta.as_simple_insert() {
                    fn traverse(point: Point, text: &str) -> Point {
                        let Point {
                            mut row,
                            mut column,
                        } = point;

                        for ch in text.chars() {
                            if ch == '\n' {
                                row += 1;
                                column = 0;
                            } else {
                                column += 1;
                            }
                        }
                        Point { row, column }
                    }

                    let start_position = point_at_offset(&self.text, start);

                    let edit = tree_sitter::InputEdit {
                        start_byte: start,
                        old_end_byte: start,
                        new_end_byte: start + inserted.len(),
                        start_position,
                        old_end_position: start_position,
                        new_end_position: traverse(
                            start_position,
                            &inserted.slice_to_cow(0..inserted.len()),
                        ),
                    };
                    old_tree = self.tree.as_ref().map(|tree| {
                        let mut tree = tree.clone();
                        tree.edit(&edit);
                        tree
                    });
                } else if delta.is_simple_delete() {
                    let start_position = point_at_offset(&self.text, start);
                    let end_position = point_at_offset(&self.text, end);
                    let edit = tree_sitter::InputEdit {
                        start_byte: start,
                        old_end_byte: end,
                        new_end_byte: start,
                        start_position,
                        old_end_position: end_position,
                        new_end_position: start_position,
                    };
                    old_tree = self.tree.as_ref().map(|tree| {
                        let mut tree = tree.clone();
                        tree.edit(&edit);
                        tree
                    });
                };
            }
        }

        let new_tree = PARSER.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            parsers
                .entry(self.language)
                .or_insert_with(|| self.language.new_parser());
            let parser = parsers.get_mut(&self.language).unwrap();

            parser.parse_with(
                &mut |byte, _| {
                    if byte <= new_text.len() {
                        new_text
                            .iter_chunks(byte..)
                            .next()
                            .map(|s| s.as_bytes())
                            .unwrap_or(&[])
                    } else {
                        &[]
                    }
                },
                old_tree.as_ref(),
            )
        });

        let styles = if let Some(tree) = new_tree.as_ref() {
            let styles = HIGHLIGHTS.with(|configs| {
                let mut configs = configs.borrow_mut();
                configs
                    .entry(self.language)
                    .or_insert_with(|| self.language.new_highlight_config());
                let config = configs.get(&self.language).unwrap();
                let mut current_hl: Option<Highlight> = None;
                let mut highlights = SpansBuilder::new(new_text.len());
                let mut highlighter = Highlighter::new();
                for highlight in highlighter
                    .highlight(
                        tree.clone(),
                        config,
                        new_text.slice_to_cow(0..new_text.len()).as_bytes(),
                        None,
                        |_| None,
                    )
                    .flatten()
                {
                    match highlight {
                        HighlightEvent::Source { start, end } => {
                            if let Some(hl) = current_hl {
                                if let Some(hl) = SCOPES.get(hl.0) {
                                    highlights.add_span(
                                        Interval::new(start, end),
                                        Style {
                                            fg_color: Some(hl.to_string()),
                                        },
                                    );
                                }
                            }
                        }
                        HighlightEvent::HighlightStart(hl) => {
                            current_hl = Some(hl);
                        }
                        HighlightEvent::HighlightEnd => current_hl = None,
                    }
                }
                highlights.build()
            });
            Some(Arc::new(styles))
        } else {
            None
        };

        let normal_lines = if let Some(tree) = new_tree.as_ref() {
            let mut cursor = tree.walk();
            let mut normal_lines = HashSet::new();
            self.language.walk_tree(&mut cursor, &mut normal_lines);
            let normal_lines: Vec<usize> =
                normal_lines.into_iter().sorted().collect();
            normal_lines
        } else {
            Vec::new()
        };

        let lens = Self::lens_from_normal_lines(
            new_text.line_of_offset(new_text.len()) + 1,
            self.line_height,
            self.lens_height,
            &normal_lines,
        );
        Syntax {
            rev: new_rev,
            language: self.language,
            tree: new_tree,
            text: new_text,
            lens,
            line_height: self.line_height,
            lens_height: self.lens_height,
            normal_lines,
            styles,
        }
    }

    pub fn update_lens_height(&mut self, line_height: usize, lens_height: usize) {
        self.lens = Self::lens_from_normal_lines(
            self.text.line_of_offset(self.text.len()) + 1,
            line_height,
            lens_height,
            &self.normal_lines,
        );
        self.line_height = line_height;
        self.lens_height = lens_height;
    }

    pub fn lens_from_normal_lines(
        total_lines: usize,
        line_height: usize,
        lens_height: usize,
        normal_lines: &[usize],
    ) -> Lens {
        let mut builder = LensBuilder::new();
        let mut current_line = 0;
        for normal_line in normal_lines.iter() {
            let normal_line = *normal_line;
            if normal_line > current_line {
                builder.add_section(normal_line - current_line, lens_height);
            }
            builder.add_section(1, line_height);
            current_line = normal_line + 1;
        }
        if current_line < total_lines {
            builder.add_section(total_lines - current_line, lens_height);
        }
        builder.build()
    }

    pub fn find_matching_pair(&self, offset: usize) -> Option<usize> {
        let tree = self.tree.as_ref()?;
        let node = tree
            .root_node()
            .descendant_for_byte_range(offset, offset + 1)?;
        let mut chars = node.kind().chars();
        let char = chars.next()?;
        let char = matching_char(char)?;
        let tag = &char.to_string();

        if let Some(offset) = self.find_tag_in_siblings(node, true, tag) {
            return Some(offset);
        }
        if let Some(offset) = self.find_tag_in_siblings(node, false, tag) {
            return Some(offset);
        }
        None
    }

    pub fn find_tag(
        &self,
        offset: usize,
        previous: bool,
        tag: &str,
    ) -> Option<usize> {
        let tree = self.tree.as_ref()?;
        let node = tree
            .root_node()
            .descendant_for_byte_range(offset, offset + 1)?;

        if let Some(offset) = self.find_tag_in_siblings(node, previous, tag) {
            return Some(offset);
        }

        if let Some(offset) = self.find_tag_in_children(node, tag) {
            return Some(offset);
        }

        let mut node = node;
        while let Some(parent) = node.parent() {
            if let Some(offset) = self.find_tag_in_siblings(parent, previous, tag) {
                return Some(offset);
            }
            node = parent;
        }
        None
    }

    fn find_tag_in_siblings(
        &self,
        node: Node,
        previous: bool,
        tag: &str,
    ) -> Option<usize> {
        let mut node = node;
        while let Some(sibling) = if previous {
            node.prev_sibling()
        } else {
            node.next_sibling()
        } {
            if sibling.kind() == tag {
                let offset = sibling.start_byte();
                return Some(offset);
            }
            node = sibling;
        }
        None
    }

    fn find_tag_in_children(&self, node: Node, tag: &str) -> Option<usize> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == tag {
                    let offset = child.start_byte();
                    return Some(offset);
                }
            }
        }
        None
    }
}

pub fn matching_pair_direction(c: char) -> Option<bool> {
    Some(match c {
        '{' => true,
        '}' => false,
        '(' => true,
        ')' => false,
        '[' => true,
        ']' => false,
        _ => return None,
    })
}

pub fn matching_char(c: char) -> Option<char> {
    Some(match c {
        '{' => '}',
        '}' => '{',
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        _ => return None,
    })
}

pub fn has_unmatched_pair(line: &str) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line.chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            pair_first.entry(key).or_insert(left);
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

pub fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lens() {
        let lens = Syntax::lens_from_normal_lines(5, 25, 2, &[4]);
        assert_eq!(5, lens.len());
        assert_eq!(8, lens.height_of_line(4));
        assert_eq!(33, lens.height_of_line(5));

        let lens = Syntax::lens_from_normal_lines(5, 25, 2, &[3]);
        assert_eq!(5, lens.len());
        assert_eq!(6, lens.height_of_line(3));
        assert_eq!(31, lens.height_of_line(4));
        assert_eq!(33, lens.height_of_line(5));
    }

    #[test]
    fn test_lens_iter() {
        let lens = Syntax::lens_from_normal_lines(5, 25, 2, &[0, 2, 4]);
        assert_eq!(5, lens.len());
        let mut iter = lens.iter_chunks(2..5);
        assert_eq!(Some((2, 25)), iter.next());
        assert_eq!(Some((3, 2)), iter.next());
        assert_eq!(Some((4, 25)), iter.next());
        assert_eq!(None, iter.next());

        let lens =
            Syntax::lens_from_normal_lines(91, 25, 2, &[0, 11, 14, 54, 57, 90]);
        assert_eq!(91, lens.len());
        let mut iter = lens.iter_chunks(89..91);
        assert_eq!(Some((89, 2)), iter.next());
        assert_eq!(Some((90, 25)), iter.next());
        assert_eq!(None, iter.next());
    }
}
