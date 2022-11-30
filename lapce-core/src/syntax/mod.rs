/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Much of the code in this file is modified from [helix](https://github.com/helix-editor/helix)'s implementation of their syntax highlighting, which is under the MPL.
 */

use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    mem,
    path::Path,
    sync::{atomic::AtomicUsize, Arc},
};

use itertools::Itertools;
use lapce_rpc::style::Style;
use lapce_xi_rope::{
    spans::{Spans, SpansBuilder},
    Interval, Rope,
};
use slotmap::{DefaultKey as LayerId, HopSlotMap};
use thiserror::Error;
use tree_sitter::{Node, Parser, Point, QueryCursor, Tree};

use self::{
    edit::SyntaxEdit,
    highlight::{
        get_highlight_config, injection_for_match, intersect_ranges, Highlight,
        HighlightConfiguration, HighlightEvent, HighlightIter, HighlightIterLayer,
        IncludedChildren, LocalScope,
    },
    util::{matching_char, matching_pair_direction, RopeProvider},
};
use crate::{
    language::LapceLanguage,
    lens::{Lens, LensBuilder},
    style::SCOPES,
};

pub mod edit;
pub mod highlight;
pub mod util;

// Uses significant portions Helix's implementation, and on tree-sitter's highlighter implementation

pub struct TsParser {
    parser: tree_sitter::Parser,
    pub cursors: Vec<QueryCursor>,
}

thread_local! {
    pub static PARSER: RefCell<TsParser> = RefCell::new(TsParser {
        parser: Parser::new(),
        cursors: Vec::new(),
    });
}

/// Represents the reason why syntax highlighting failed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Cancelled")]
    Cancelled,
    #[error("Invalid language")]
    InvalidLanguage,
    #[error("Unknown error")]
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LanguageLayer {
    // mode
    // grammar
    pub config: Arc<HighlightConfiguration>,
    pub(crate) tree: Option<Tree>,
    pub ranges: Vec<tree_sitter::Range>,
    pub depth: usize,
    rev: u64,
}

impl LanguageLayer {
    pub fn tree(&self) -> &Tree {
        self.tree.as_ref().unwrap()
    }

    pub fn try_tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    fn parse(
        &mut self,
        parser: &mut Parser,
        source: &Rope,
        had_edits: bool,
    ) -> Result<(), Error> {
        parser.set_included_ranges(&self.ranges).unwrap();

        parser
            .set_language(self.config.language)
            .map_err(|_| Error::InvalidLanguage)?;

        // unsafe { syntax.parser.set_cancellation_flag(cancellation_flag) };
        let tree = parser
            .parse_with(
                &mut |byte, _| {
                    if byte <= source.len() {
                        source
                            .iter_chunks(byte..)
                            .next()
                            .map(|s| s.as_bytes())
                            .unwrap_or(&[])
                    } else {
                        &[]
                    }
                },
                had_edits.then_some(()).and(self.tree.as_ref()),
            )
            .ok_or(Error::Cancelled)?;
        // unsafe { ts_parser.parser.set_cancellation_flag(None) };
        self.tree = Some(tree);
        Ok(())
    }
}

#[derive(Clone)]
pub struct SyntaxLayers {
    layers: HopSlotMap<LayerId, LanguageLayer>,
    root: LayerId,
}
impl SyntaxLayers {
    pub fn new_empty(config: Arc<HighlightConfiguration>) -> SyntaxLayers {
        Self::new(None, config)
    }

    pub fn new(
        source: Option<&Rope>,
        config: Arc<HighlightConfiguration>,
    ) -> SyntaxLayers {
        let root_layer = LanguageLayer {
            tree: None,
            config,
            depth: 0,
            ranges: vec![tree_sitter::Range {
                start_byte: 0,
                end_byte: usize::MAX,
                start_point: Point::new(0, 0),
                end_point: Point::new(usize::MAX, usize::MAX),
            }],
            rev: 0,
        };

        let mut layers = HopSlotMap::default();
        let root = layers.insert(root_layer);

        let mut syntax = SyntaxLayers { root, layers };

        if let Some(source) = source {
            let _ = syntax.update(0, 0, source, None);
        }

        syntax
    }

    pub fn update(
        &mut self,
        current_rev: u64,
        new_rev: u64,
        source: &Rope,
        syntax_edits: Option<&[SyntaxEdit]>,
    ) -> Result<(), Error> {
        let mut queue = VecDeque::new();
        queue.push_back(self.root);

        let injection_callback = |language: &str| {
            LapceLanguage::from_name(language).map(get_highlight_config)
        };

        let mut edits = Vec::new();
        if let Some(syntax_edits) = syntax_edits {
            for edit in syntax_edits {
                for edit in &edit.0 {
                    edits.push(edit);
                }
            }
        }

        // Use the edits to update all layers markers
        if !edits.is_empty() {
            fn point_add(a: Point, b: Point) -> Point {
                if b.row > 0 {
                    Point::new(a.row.saturating_add(b.row), b.column)
                } else {
                    Point::new(0, a.column.saturating_add(b.column))
                }
            }
            fn point_sub(a: Point, b: Point) -> Point {
                if a.row > b.row {
                    Point::new(a.row.saturating_sub(b.row), a.column)
                } else {
                    Point::new(0, a.column.saturating_sub(b.column))
                }
            }

            for layer in &mut self.layers.values_mut() {
                // The root layer always covers the whole range (0..usize::MAX)
                if layer.depth == 0 {
                    continue;
                }

                for range in &mut layer.ranges {
                    // Roughly based on https://github.com/tree-sitter/tree-sitter/blob/ddeaa0c7f534268b35b4f6cb39b52df082754413/lib/src/subtree.c#L691-L720
                    for edit in edits.iter().rev() {
                        let is_pure_insertion = edit.old_end_byte == edit.start_byte;

                        // if edit is after range, skip
                        if edit.start_byte > range.end_byte {
                            // TODO: || (is_noop && edit.start_byte == range.end_byte)
                            continue;
                        }

                        // if edit is before range, shift entire range by len
                        if edit.old_end_byte < range.start_byte {
                            range.start_byte = edit.new_end_byte
                                + (range.start_byte - edit.old_end_byte);
                            range.start_point = point_add(
                                edit.new_end_position,
                                point_sub(range.start_point, edit.old_end_position),
                            );

                            range.end_byte = edit
                                .new_end_byte
                                .saturating_add(range.end_byte - edit.old_end_byte);
                            range.end_point = point_add(
                                edit.new_end_position,
                                point_sub(range.end_point, edit.old_end_position),
                            );
                        }
                        // if the edit starts in the space before and extends into the range
                        else if edit.start_byte < range.start_byte {
                            range.start_byte = edit.new_end_byte;
                            range.start_point = edit.new_end_position;

                            range.end_byte = range
                                .end_byte
                                .saturating_sub(edit.old_end_byte)
                                .saturating_add(edit.new_end_byte);
                            range.end_point = point_add(
                                edit.new_end_position,
                                point_sub(range.end_point, edit.old_end_position),
                            );
                        }
                        // If the edit is an insertion at the start of the tree, shift
                        else if edit.start_byte == range.start_byte
                            && is_pure_insertion
                        {
                            range.start_byte = edit.new_end_byte;
                            range.start_point = edit.new_end_position;
                        } else {
                            range.end_byte = range
                                .end_byte
                                .saturating_sub(edit.old_end_byte)
                                .saturating_add(edit.new_end_byte);
                            range.end_point = point_add(
                                edit.new_end_position,
                                point_sub(range.end_point, edit.old_end_position),
                            );
                        }
                    }
                }
            }
        }

        PARSER.with(|ts_parser| {
            let ts_parser = &mut ts_parser.borrow_mut();
            let mut cursor = ts_parser.cursors.pop().unwrap_or_else(QueryCursor::new);
            // TODO: might need to set cursor range
            cursor.set_byte_range(0..usize::MAX);

            let mut touched = HashSet::new();

            // TODO: we should be able to avoid editing & parsing layers with ranges earlier in the document before the edit

            while let Some(layer_id) = queue.pop_front() {
                // Mark the layer as touched
                touched.insert(layer_id);

                let layer = &mut self.layers[layer_id];

                let had_edits = layer.rev == current_rev && syntax_edits.is_some();
                // If a tree already exists, notify it of changes.
                if had_edits {
                    if let Some(tree) = &mut layer.tree {
                        for edit in edits.iter() {
                            tree.edit(edit);
                        }
                    }
                }

                // Re-parse the tree.
                layer.parse(&mut ts_parser.parser, source, had_edits)?;
                layer.rev = new_rev;

                // Switch to an immutable borrow.
                let layer = &self.layers[layer_id];

                // Process injections.
                let matches = cursor.matches(
                    &layer.config.injections_query,
                    layer.tree().root_node(),
                    RopeProvider(source),
                );
                let mut injections = Vec::new();
                for mat in matches {
                    let (language_name, content_node, included_children) = injection_for_match(
                        &layer.config,
                        &layer.config.injections_query,
                        &mat,
                        source,
                    );

                    // Explicitly remove this match so that none of its other captures will remain
                    // in the stream of captures.
                    mat.remove();

                    // If a language is found with the given name, then add a new language layer
                    // to the highlighted document.
                    if let (Some(language_name), Some(content_node)) = (language_name, content_node)
                    {
                        if let Some(config) = (injection_callback)(&language_name) {
                            let ranges =
                                intersect_ranges(&layer.ranges, &[content_node], included_children);

                            if !ranges.is_empty() {
                                injections.push((config, ranges));
                            }
                        }
                    }
                }

                // Process combined injections.
                if let Some(combined_injections_query) = &layer.config.combined_injections_query {
                    let mut injections_by_pattern_index =
                        vec![
                            (None, Vec::new(), IncludedChildren::default());
                            combined_injections_query.pattern_count()
                        ];
                    let matches = cursor.matches(
                        combined_injections_query,
                        layer.tree().root_node(),
                        RopeProvider(source),
                    );
                    for mat in matches {
                        let entry = &mut injections_by_pattern_index[mat.pattern_index];
                        let (language_name, content_node, included_children) = injection_for_match(
                            &layer.config,
                            combined_injections_query,
                            &mat,
                            source,
                        );
                        if language_name.is_some() {
                            entry.0 = language_name;
                        }
                        if let Some(content_node) = content_node {
                            entry.1.push(content_node);
                        }
                        entry.2 = included_children;
                    }
                    for (lang_name, content_nodes, included_children) in injections_by_pattern_index
                    {
                        if let (Some(lang_name), false) = (lang_name, content_nodes.is_empty()) {
                            if let Some(config) = (injection_callback)(&lang_name) {
                                let ranges = intersect_ranges(
                                    &layer.ranges,
                                    &content_nodes,
                                    included_children,
                                );
                                if !ranges.is_empty() {
                                    injections.push((config, ranges));
                                }
                            }
                        }
                    }
                }

                let depth = layer.depth + 1;
                // TODO: can't inline this since matches borrows self.layers
                for (config, ranges) in injections {
                    // Find an existing layer
                    let layer = self
                        .layers
                        .iter_mut()
                        .find(|(_, layer)| {
                            layer.depth == depth && // TODO: track parent id instead
                            layer.config.language == config.language && layer.ranges == ranges
                        })
                        .map(|(id, _layer)| id);

                    // ...or insert a new one.
                    let layer_id = layer.unwrap_or_else(|| {
                        self.layers.insert(LanguageLayer {
                            tree: None,
                            config,
                            depth,
                            ranges,
                            rev: 0,
                        })
                    });

                    queue.push_back(layer_id);
                }

                // TODO: pre-process local scopes at this time, rather than highlight?
                // would solve problems with locals not working across boundaries
            }

            // Return the cursor back in the pool.
            ts_parser.cursors.push(cursor);

            // Remove all untouched layers
            self.layers.retain(|id, _| touched.contains(&id));

            Ok(())
        })
    }

    pub fn tree(&self) -> &Tree {
        self.layers[self.root].tree()
    }

    pub fn try_tree(&self) -> Option<&Tree> {
        self.layers[self.root].try_tree()
    }

    /// Iterate over the highlighted regions for a given slice of source code.
    pub fn highlight_iter<'a>(
        &'a self,
        source: &'a Rope,
        range: Option<std::ops::Range<usize>>,
        cancellation_flag: Option<&'a AtomicUsize>,
    ) -> impl Iterator<Item = Result<HighlightEvent, Error>> + 'a {
        let mut layers = self
            .layers
            .iter()
            .filter_map(|(_, layer)| {
                // TODO: if range doesn't overlap layer range, skip it

                // Reuse a cursor from the pool if available.
                let mut cursor = PARSER.with(|ts_parser| {
                    let highlighter = &mut ts_parser.borrow_mut();
                    highlighter.cursors.pop().unwrap_or_else(QueryCursor::new)
                });

                // The `captures` iterator borrows the `Tree` and the `QueryCursor`, which
                // prevents them from being moved. But both of these values are really just
                // pointers, so it's actually ok to move them.
                let cursor_ref = unsafe {
                    mem::transmute::<_, &'static mut QueryCursor>(&mut cursor)
                };

                // if reusing cursors & no range this resets to whole range
                cursor_ref.set_byte_range(range.clone().unwrap_or(0..usize::MAX));

                let mut captures = cursor_ref
                    .captures(
                        &layer.config.query,
                        layer.tree().root_node(),
                        RopeProvider(source),
                    )
                    .peekable();

                // If there's no captures, skip the layer
                captures.peek()?;

                Some(HighlightIterLayer {
                    highlight_end_stack: Vec::new(),
                    scope_stack: vec![LocalScope {
                        inherits: false,
                        range: 0..usize::MAX,
                        local_defs: Vec::new(),
                    }],
                    cursor,
                    _tree: None,
                    captures,
                    config: layer.config.as_ref(), // TODO: just reuse `layer`
                    depth: layer.depth,            // TODO: just reuse `layer`
                    ranges: &layer.ranges,         // TODO: temp
                })
            })
            .collect::<Vec<_>>();

        // HAXX: arrange layers by byte range, with deeper layers positioned first
        layers.sort_by_key(|layer| {
            (
                layer.ranges.first().cloned(),
                std::cmp::Reverse(layer.depth),
            )
        });

        let mut result = HighlightIter {
            source,
            byte_offset: range.map_or(0, |r| r.start),
            cancellation_flag,
            iter_count: 0,
            layers,
            next_event: None,
            last_highlight_range: None,
        };
        result.sort_layers();
        result
    }

    // Commenting
    // comment_strings_for_pos
    // is_commented

    // Indentation
    // suggested_indent_for_line_at_buffer_row
    // suggested_indent_for_buffer_row
    // indent_level_for_line

    // TODO: Folding
}

#[derive(Clone)]
pub struct Syntax {
    pub rev: u64,
    pub language: LapceLanguage,
    pub text: Rope,
    pub layers: SyntaxLayers,
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
            .field("normal_lines", &self.normal_lines)
            .field("line_height", &self.line_height)
            .field("lens_height", &self.lens_height)
            .field("styles", &self.styles)
            .finish()
    }
}

impl Syntax {
    pub fn init(path: &Path) -> Option<Syntax> {
        LapceLanguage::from_path(path).map(Syntax::from_language)
    }

    pub fn from_language(language: LapceLanguage) -> Syntax {
        Syntax {
            rev: 0,
            language,
            text: Rope::from(""),
            layers: SyntaxLayers::new_empty(get_highlight_config(language)),
            lens: Self::lens_from_normal_lines(0, 0, 0, &Vec::new()),
            line_height: 0,
            lens_height: 0,
            normal_lines: Vec::new(),
            styles: None,
        }
    }

    pub fn parse(
        &mut self,
        new_rev: u64,
        new_text: Rope,
        edits: Option<&[SyntaxEdit]>,
    ) {
        let edits = if let Some(edits) = edits {
            if new_rev == self.rev + edits.len() as u64 {
                Some(edits)
            } else {
                None
            }
        } else {
            None
        };
        let _ = self.layers.update(self.rev, new_rev, &new_text, edits);
        let tree = self.layers.try_tree();

        let styles = if tree.is_some() {
            let mut current_hl: Option<Highlight> = None;
            let mut highlights: SpansBuilder<Style> =
                SpansBuilder::new(new_text.len());

            // TODO: Should we be ignoring highlight errors via flattening them?
            for highlight in self
                .layers
                .highlight_iter(&new_text, Some(0..new_text.len()), None)
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

            Some(Arc::new(highlights.build()))
        } else {
            None
        };

        let normal_lines = if let Some(tree) = tree {
            let mut cursor = tree.walk();
            let mut normal_lines = HashSet::new();
            self.language.walk_tree(&mut cursor, &mut normal_lines);
            normal_lines.into_iter().sorted().collect::<Vec<usize>>()
        } else {
            Vec::new()
        };

        let lens = Self::lens_from_normal_lines(
            new_text.line_of_offset(new_text.len()) + 1,
            self.line_height,
            self.lens_height,
            &normal_lines,
        );

        self.rev = new_rev;
        self.lens = lens;
        self.normal_lines = normal_lines;
        self.styles = styles;
        self.text = new_text
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
        let tree = self.layers.try_tree()?;
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

    pub fn parent_offset(&self, offset: usize) -> Option<usize> {
        let tree = self.layers.try_tree()?;
        let node = tree
            .root_node()
            .descendant_for_byte_range(offset, offset + 1)?;
        let parent = node.parent()?;
        Some(parent.start_byte())
    }

    pub fn find_tag(
        &self,
        offset: usize,
        previous: bool,
        tag: &str,
    ) -> Option<usize> {
        let tree = self.layers.try_tree()?;
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

    pub fn sticky_headers(&self, offset: usize) -> Option<Vec<usize>> {
        let tree = self.layers.try_tree()?;
        let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
        let mut offsets = Vec::new();
        let sticky_header_tags = self.language.sticky_header_tags();
        loop {
            if sticky_header_tags.iter().any(|t| *t == node.kind()) {
                offsets.push(node.start_byte());
            }
            if let Some(p) = node.parent() {
                node = p;
            } else {
                break;
            }
        }
        Some(offsets)
    }

    pub fn find_enclosing_parentheses(
        &self,
        offset: usize,
    ) -> Option<(usize, usize)> {
        let tree = self.layers.try_tree()?;
        let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
        // If there is no text then the document can't have any bytes
        if self.text.is_empty() {
            return None;
        }

        loop {
            let start = node.start_byte();
            let c = self.text.byte_at(start) as char;
            if c == '(' {
                let end = self.find_matching_pair(start)?;
                if end >= offset && start < offset {
                    return Some((start, end));
                }
            }
            if let Some(sibling) = node.prev_sibling() {
                node = sibling;
            } else if let Some(parent) = node.parent() {
                node = parent;
            } else {
                return None;
            }
        }
    }

    pub fn find_enclosing_pair(&self, offset: usize) -> Option<(usize, usize)> {
        let tree = self.layers.try_tree()?;
        let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
        // If there is no text then the document can't have any bytes
        if self.text.is_empty() {
            return None;
        }

        loop {
            let start = node.start_byte();
            let c = self.text.byte_at(start) as char;
            if matching_pair_direction(c) == Some(true) {
                let end = self.find_matching_pair(start)?;
                if end >= offset {
                    return Some((start, end));
                }
            }
            if let Some(sibling) = node.prev_sibling() {
                node = sibling;
            } else if let Some(parent) = node.parent() {
                node = parent;
            } else {
                return None;
            }
        }
    }
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
