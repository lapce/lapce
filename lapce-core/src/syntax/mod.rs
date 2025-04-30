/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Much of the code in this file is modified from [helix](https://github.com/helix-editor/helix)'s implementation of their syntax highlighting, which is under the MPL.
 */

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    hash::{Hash, Hasher},
    mem,
    path::Path,
    sync::{Arc, atomic::AtomicUsize},
};

use ahash::RandomState;
use floem_editor_core::util::{matching_bracket_general, matching_pair_direction};
use hashbrown::raw::RawTable;
use itertools::Itertools;
use lapce_rpc::style::{LineStyle, Style};
use lapce_xi_rope::{
    Interval, Rope,
    spans::{Spans, SpansBuilder},
};
use slotmap::{DefaultKey as LayerId, HopSlotMap};
use thiserror::Error;
use tree_sitter::{Node, Parser, Point, QueryCursor, Tree};

use self::{
    edit::SyntaxEdit,
    highlight::{
        Highlight, HighlightConfiguration, HighlightEvent, HighlightIter,
        HighlightIterLayer, IncludedChildren, LocalScope, get_highlight_config,
        intersect_ranges,
    },
    util::RopeProvider,
};
use crate::{
    buffer::{Buffer, rope_text::RopeText},
    language::{self, LapceLanguage},
    lens::{Lens, LensBuilder},
    style::SCOPES,
    syntax::highlight::InjectionLanguageMarker,
};
pub mod edit;
pub mod highlight;
pub mod util;

const TREE_SITTER_MATCH_LIMIT: u32 = 256;

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
    #[error("Invalid ranges")]
    InvalidRanges,
    #[error("Invalid language")]
    InvalidLanguage,
    #[error("Unknown error")]
    Unknown,
}

#[derive(Clone, Debug)]
pub enum NodeType {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftCurly,
    RightCurly,
    Pair,
    Code,
    Dummy,
}

#[derive(Clone, Debug)]
pub enum BracketParserMode {
    Parsing,
    NoParsing,
}

#[derive(Clone, Debug)]
pub struct ASTNode {
    pub tt: NodeType,
    pub len: usize,
    pub children: Vec<ASTNode>,
    pub level: usize,
}

impl ASTNode {
    pub fn new() -> Self {
        Self {
            tt: NodeType::Dummy,
            len: 0,
            children: vec![],
            level: 0,
        }
    }

    pub fn new_with_type(tt: NodeType, len: usize) -> Self {
        Self {
            tt,
            len,
            children: vec![],
            level: 0,
        }
    }
}

impl Default for ASTNode {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct BracketParser {
    pub code: Vec<char>,
    pub cur: usize,
    pub ast: ASTNode,
    bracket_set: HashMap<char, ASTNode>,
    pub bracket_pos: HashMap<usize, Vec<LineStyle>>,
    mode: BracketParserMode,
    noparsing_token: Vec<char>,
    pub active: bool,
    pub limit: u64,
}

impl BracketParser {
    pub fn new(code: String, active: bool, limit: u64) -> Self {
        Self {
            code: code.chars().collect(),
            cur: 0,
            ast: ASTNode::new(),
            bracket_set: HashMap::from([
                ('(', ASTNode::new_with_type(NodeType::LeftParen, 1)),
                (')', ASTNode::new_with_type(NodeType::RightParen, 1)),
                ('{', ASTNode::new_with_type(NodeType::LeftCurly, 1)),
                ('}', ASTNode::new_with_type(NodeType::RightCurly, 1)),
                ('[', ASTNode::new_with_type(NodeType::LeftBracket, 1)),
                (']', ASTNode::new_with_type(NodeType::RightBracket, 1)),
            ]),
            bracket_pos: HashMap::new(),
            mode: BracketParserMode::Parsing,
            noparsing_token: vec!['\'', '"', '`'],
            active,
            limit,
        }
    }

    /*pub fn enable(&self) {
        *(self.active.borrow_mut()) = true;
    }

    pub fn disable(&self) {
        *(self.active.borrow_mut()) = false;
    }*/

    pub fn update_code(
        &mut self,
        code: String,
        buffer: &Buffer,
        syntax: Option<&Syntax>,
    ) {
        let palette = vec![
            "bracket.color.1".to_string(),
            "bracket.color.2".to_string(),
            "bracket.color.3".to_string(),
        ];
        if self.active
            && code
                .chars()
                .fold(0, |i, c| if c == '\n' { i + 1 } else { i })
                < self.limit as usize
        {
            self.bracket_pos = HashMap::new();
            if let Some(syntax) = syntax {
                if let Some(layers) = &syntax.layers {
                    if let Some(tree) = layers.try_tree() {
                        let mut walk_cursor = tree.walk();
                        let mut bracket_pos: HashMap<usize, Vec<LineStyle>> =
                            HashMap::new();
                        language::walk_tree_bracket_ast(
                            &mut walk_cursor,
                            &mut 0,
                            &mut 0,
                            &mut bracket_pos,
                            &palette,
                        );
                        self.bracket_pos = bracket_pos;
                    }
                }
            } else {
                self.code = code.chars().collect();
                self.cur = 0;
                self.parse();
                let mut pos_vec = vec![];
                Self::highlight_pos(
                    &self.ast,
                    &mut pos_vec,
                    &mut 0usize,
                    &mut 0usize,
                    &palette,
                );
                if buffer.is_empty() {
                    return;
                }
                for (offset, color) in pos_vec.iter() {
                    let (line, col) = buffer.offset_to_line_col(*offset);
                    let line_style = LineStyle {
                        start: col,
                        end: col + 1,
                        style: Style {
                            fg_color: Some(color.clone()),
                        },
                    };
                    match self.bracket_pos.entry(line) {
                        Entry::Vacant(v) => _ = v.insert(vec![line_style.clone()]),
                        Entry::Occupied(mut o) => {
                            o.get_mut().push(line_style.clone())
                        }
                    }
                }
            }
        } else {
            self.bracket_pos = HashMap::new();
        }
    }

    fn is_left(c: &char) -> bool {
        if *c == '(' || *c == '{' || *c == '[' {
            return true;
        }
        false
    }

    fn parse(&mut self) {
        let new_ast = &mut ASTNode::new();
        self.parse_bracket(0, new_ast);
        self.ast = new_ast.clone();
        Self::patch_len(&mut self.ast);
        self.cur = 0;
    }

    fn parse_bracket(&mut self, level: usize, parent_node: &mut ASTNode) {
        let mut counter = 0usize;
        while self.cur < self.code.len() {
            if self.noparsing_token.contains(&self.code[self.cur]) {
                if matches!(self.mode, BracketParserMode::Parsing) {
                    self.mode = BracketParserMode::NoParsing;
                } else {
                    self.mode = BracketParserMode::Parsing;
                }
            }
            if self.bracket_set.contains_key(&self.code[self.cur])
                && matches!(self.mode, BracketParserMode::Parsing)
            {
                if Self::is_left(&self.code[self.cur]) {
                    let code_node = ASTNode::new_with_type(NodeType::Code, counter);
                    let left_node =
                        self.bracket_set.get(&self.code[self.cur]).unwrap().clone();
                    let mut pair_node =
                        ASTNode::new_with_type(NodeType::Pair, counter + 1);
                    pair_node.level = level;
                    pair_node.children.push(code_node);
                    pair_node.children.push(left_node);
                    self.cur += 1;
                    self.parse_bracket(level + 1, &mut pair_node);
                    parent_node.children.push(pair_node.clone());
                    counter = 0;
                } else if level <= parent_node.level {
                    let code_node = ASTNode::new_with_type(NodeType::Code, counter);
                    let right_node =
                        self.bracket_set.get(&self.code[self.cur]).unwrap().clone();
                    parent_node.children.push(code_node);
                    parent_node.children.push(right_node);
                    let parent_len = parent_node.len;
                    parent_node.len = parent_len + counter + 1;
                    counter = 0;
                    self.cur += 1;
                } else {
                    let code_node = ASTNode::new_with_type(NodeType::Code, counter);
                    let right_node =
                        self.bracket_set.get(&self.code[self.cur]).unwrap().clone();
                    parent_node.children.push(code_node);
                    parent_node.children.push(right_node);
                    let parent_len = parent_node.len;
                    parent_node.len = parent_len + counter + 1;
                    self.cur += 1;
                    return;
                }
            } else {
                counter += self.code[self.cur].len_utf8();
                self.cur += 1;
            }
        }
    }

    fn patch_len(ast: &mut ASTNode) {
        if !ast.children.is_empty() {
            let mut len = 0usize;
            for n in ast.children.iter_mut() {
                match n.tt {
                    NodeType::LeftCurly => len += 1,
                    NodeType::RightCurly => len += 1,
                    NodeType::LeftParen => len += 1,
                    NodeType::RightParen => len += 1,
                    NodeType::LeftBracket => len += 1,
                    NodeType::RightBracket => len += 1,
                    NodeType::Code => len += n.len,
                    NodeType::Pair => {
                        Self::patch_len(n);
                        len += n.len;
                    }
                    _ => break,
                }
            }
            ast.len = len;
        }
    }

    fn highlight_pos(
        ast: &ASTNode,
        pos_vec: &mut Vec<(usize, String)>,
        index: &mut usize,
        level: &mut usize,
        palette: &Vec<String>,
    ) {
        if !ast.children.is_empty() {
            for n in ast.children.iter() {
                match n.tt {
                    NodeType::LeftCurly
                    | NodeType::LeftParen
                    | NodeType::LeftBracket => {
                        pos_vec
                            .push((*index, palette[*level % palette.len()].clone()));
                        *level += 1;
                        *index += 1;
                    }
                    NodeType::RightCurly
                    | NodeType::RightParen
                    | NodeType::RightBracket => {
                        let (new_level, overflow) = (*level).overflowing_sub(1);
                        if overflow {
                            pos_vec.push((*index, "bracket.unpaired".to_string()));
                        } else {
                            *level = new_level;
                            pos_vec.push((
                                *index,
                                palette[*level % palette.len()].clone(),
                            ));
                        }
                        *index += 1;
                    }
                    NodeType::Code => *index += n.len,
                    NodeType::Pair => {
                        Self::highlight_pos(n, pos_vec, index, level, palette)
                    }
                    _ => break,
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LanguageLayer {
    // mode
    // grammar
    pub config: Arc<HighlightConfiguration>,
    pub(crate) tree: Option<Tree>,
    pub ranges: Vec<tree_sitter::Range>,
    pub depth: usize,
    _parent: Option<LayerId>,
    rev: u64,
}

/// This PartialEq implementation only checks if that
/// two layers are theoretically identical (meaning they highlight the same text range with the same language).
/// It does not check whether the layers have the same internal treesitter
/// state.
impl PartialEq for LanguageLayer {
    fn eq(&self, other: &Self) -> bool {
        self.depth == other.depth
            && self.config.language == other.config.language
            && self.ranges == other.ranges
    }
}

/// Hash implementation belongs to PartialEq implementation above.
/// See its documentation for details.
impl Hash for LanguageLayer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.depth.hash(state);
        self.config.language.hash(state);
        self.ranges.hash(state);
    }
}

impl LanguageLayer {
    pub fn try_tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    fn parse(
        &mut self,
        parser: &mut Parser,
        source: &Rope,
        had_edits: bool,
        cancellation_flag: &AtomicUsize,
    ) -> Result<(), Error> {
        parser
            .set_included_ranges(&self.ranges)
            .map_err(|_| Error::InvalidRanges)?;

        parser
            .set_language(&self.config.language)
            .map_err(|_| Error::InvalidLanguage)?;

        unsafe { parser.set_cancellation_flag(Some(cancellation_flag)) };
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
            _parent: None,
            rev: 0,
        };

        let mut layers = HopSlotMap::default();
        let root = layers.insert(root_layer);

        let mut syntax = SyntaxLayers { root, layers };

        let cancel_flag = AtomicUsize::new(0);
        if let Some(source) = source {
            if let Err(err) = syntax.update(0, 0, source, None, &cancel_flag) {
                tracing::error!("{:?}", err);
            }
        }

        syntax
    }

    pub fn update(
        &mut self,
        current_rev: u64,
        new_rev: u64,
        source: &Rope,
        syntax_edits: Option<&[SyntaxEdit]>,
        cancellation_flag: &AtomicUsize,
    ) -> Result<(), Error> {
        let mut queue = VecDeque::new();
        queue.push_back(self.root);

        let injection_callback = |language: &InjectionLanguageMarker| {
            let language = match language {
                InjectionLanguageMarker::Name(name) => {
                    LapceLanguage::from_name(name)
                }
                InjectionLanguageMarker::Filename(path) => {
                    LapceLanguage::from_path_raw(path)
                }
                InjectionLanguageMarker::Shebang(id) => LapceLanguage::from_name(id),
            };
            language
                .map(get_highlight_config)
                .unwrap_or(Err(highlight::HighlightIssue::NotAvailable))
        };

        let mut edits = Vec::new();
        if let Some(syntax_edits) = syntax_edits {
            for edit in syntax_edits {
                for edit in &edit.0 {
                    edits.push(edit);
                }
            }
        }

        // This table allows inverse indexing of `layers`.
        // That is by hashing a `Layer` you can find
        // the `LayerId` of an existing equivalent `Layer` in `layers`.
        //
        // It is used to determine if a new layer exists for an injection
        // or if an existing layer needs to be updated.
        let mut layers_table = RawTable::with_capacity(self.layers.len());
        let layers_hasher = RandomState::new();
        // Use the edits to update all layers markers
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

        for (layer_id, layer) in self.layers.iter_mut() {
            // The root layer always covers the whole range (0..usize::MAX)
            if layer.depth == 0 {
                continue;
            }

            if !edits.is_empty() {
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

            let hash = layers_hasher.hash_one(layer);
            // Safety: insert_no_grow is unsafe because it assumes that the table
            // has enough capacity to hold additional elements.
            // This is always the case as we reserved enough capacity above.
            unsafe { layers_table.insert_no_grow(hash, layer_id) };
        }

        PARSER.with(|ts_parser| {
            let ts_parser = &mut ts_parser.borrow_mut();
            ts_parser.parser.set_timeout_micros(1000 * 500); // half a second is pretty generours
            let mut cursor = ts_parser.cursors.pop().unwrap_or_default();
            // TODO: might need to set cursor range
            cursor.set_byte_range(0..usize::MAX);
            cursor.set_match_limit(TREE_SITTER_MATCH_LIMIT);

            let mut touched = HashSet::new();

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
                layer.parse(
                    &mut ts_parser.parser,
                    source,
                    had_edits,
                    cancellation_flag,
                )?;
                layer.rev = new_rev;

                // Switch to an immutable borrow.
                let layer = &self.layers[layer_id];

                // Process injections.
                if let Some(tree) = layer.try_tree() {
                    let matches = cursor.matches(
                        &layer.config.injections_query,
                        tree.root_node(),
                        RopeProvider(source),
                    );
                    let mut combined_injections =
                        vec![
                            (None, Vec::new(), IncludedChildren::default());
                            layer.config.combined_injections_patterns.len()
                        ];
                    let mut injections = Vec::new();
                    let mut last_injection_end = 0;
                    for mat in matches {
                        let (injection_capture, content_node, included_children) =
                            layer.config.injection_for_match(
                                &layer.config.injections_query,
                                &mat,
                                source,
                            );

                        // in case this is a combined injection save it for more processing later
                        if let Some(combined_injection_idx) = layer
                            .config
                            .combined_injections_patterns
                            .iter()
                            .position(|&pattern| pattern == mat.pattern_index)
                        {
                            let entry =
                                &mut combined_injections[combined_injection_idx];
                            if injection_capture.is_some() {
                                entry.0 = injection_capture;
                            }
                            if let Some(content_node) = content_node {
                                if content_node.start_byte() >= last_injection_end {
                                    entry.1.push(content_node);
                                    last_injection_end = content_node.end_byte();
                                }
                            }
                            entry.2 = included_children;
                            continue;
                        }

                        // Explicitly remove this match so that none of its other captures will remain
                        // in the stream of captures.
                        mat.remove();

                        // If a language is found with the given name, then add a new language layer
                        // to the highlighted document.
                        if let (Some(injection_capture), Some(content_node)) =
                            (injection_capture, content_node)
                        {
                            match (injection_callback)(&injection_capture) {
                                Ok(config) => {
                                    let ranges = intersect_ranges(
                                        &layer.ranges,
                                        &[content_node],
                                        included_children,
                                    );

                                    if !ranges.is_empty() {
                                        if content_node.start_byte()
                                            < last_injection_end
                                        {
                                            continue;
                                        }
                                        last_injection_end = content_node.end_byte();
                                        injections.push((config, ranges));
                                    }
                                }
                                Err(err) => {
                                    tracing::error!("{:?}", err);
                                }
                            }
                        }
                    }

                    for (lang_name, content_nodes, included_children) in
                        combined_injections
                    {
                        if let (Some(lang_name), false) =
                            (lang_name, content_nodes.is_empty())
                        {
                            match (injection_callback)(&lang_name) {
                                Ok(config) => {
                                    let ranges = intersect_ranges(
                                        &layer.ranges,
                                        &content_nodes,
                                        included_children,
                                    );
                                    if !ranges.is_empty() {
                                        injections.push((config, ranges));
                                    }
                                }
                                Err(err) => {
                                    tracing::error!("{:?}", err);
                                }
                            }
                        }
                    }

                    let depth = layer.depth + 1;
                    // TODO: can't inline this since matches borrows self.layers
                    for (config, ranges) in injections {
                        let new_layer = LanguageLayer {
                            tree: None,
                            config,
                            depth,
                            ranges,
                            _parent: Some(layer_id),
                            rev: 0,
                        };

                        // Find an identical existing layer
                        let layer = layers_table
                            .get(layers_hasher.hash_one(&new_layer), |&it| {
                                self.layers[it] == new_layer
                            })
                            .copied();

                        // ...or insert a new one.
                        let layer_id =
                            layer.unwrap_or_else(|| self.layers.insert(new_layer));

                        queue.push_back(layer_id);
                    }
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
                    highlighter.cursors.pop().unwrap_or_default()
                });

                // The `captures` iterator borrows the `Tree` and the `QueryCursor`, which
                // prevents them from being moved. But both of these values are really just
                // pointers, so it's actually ok to move them.
                let cursor_ref = unsafe {
                    mem::transmute::<
                        &mut tree_sitter::QueryCursor,
                        &mut tree_sitter::QueryCursor,
                    >(&mut cursor)
                };

                // if reusing cursors & no range this resets to whole range
                cursor_ref.set_byte_range(range.clone().unwrap_or(0..usize::MAX));
                cursor_ref.set_match_limit(TREE_SITTER_MATCH_LIMIT);

                let mut captures = cursor_ref
                    .captures(
                        &layer.config.query,
                        layer.try_tree()?.root_node(),
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
                    captures: RefCell::new(captures),
                    config: layer.config.as_ref(), // TODO: just reuse `layer`
                    depth: layer.depth,            // TODO: just reuse `layer`
                })
            })
            .collect::<Vec<_>>();

        layers.sort_unstable_by_key(|layer| layer.sort_key());

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
}

#[derive(Clone)]
pub struct Syntax {
    pub rev: u64,
    pub language: LapceLanguage,
    pub text: Rope,
    pub layers: Option<SyntaxLayers>,
    pub lens: Lens,
    pub normal_lines: Vec<usize>,
    pub line_height: usize,
    pub lens_height: usize,
    pub styles: Option<Spans<Style>>,
    pub cancel_flag: Arc<AtomicUsize>,
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
    pub fn init(path: &Path) -> Syntax {
        let language = LapceLanguage::from_path(path);
        Syntax::from_language(language)
    }

    pub fn plaintext() -> Syntax {
        Self::from_language(LapceLanguage::PlainText)
    }

    pub fn from_language(language: LapceLanguage) -> Syntax {
        let highlight = get_highlight_config(language).ok();
        Syntax {
            rev: 0,
            language,
            text: Rope::from(""),
            layers: highlight.map(SyntaxLayers::new_empty),
            lens: Self::lens_from_normal_lines(0, 0, 0, &Vec::new()),
            line_height: 0,
            lens_height: 0,
            normal_lines: Vec::new(),
            styles: None,
            cancel_flag: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn parse(
        &mut self,
        new_rev: u64,
        new_text: Rope,
        edits: Option<&[SyntaxEdit]>,
    ) {
        let layers = match &mut self.layers {
            Some(layers) => layers,
            None => return,
        };
        let edits = edits.filter(|edits| new_rev == self.rev + edits.len() as u64);
        if let Err(err) =
            layers.update(self.rev, new_rev, &new_text, edits, &self.cancel_flag)
        {
            tracing::error!("{:?}", err);
        }
        let tree = layers.try_tree();

        let styles = if tree.is_some() {
            let mut current_hl: Option<Highlight> = None;
            let mut highlights: SpansBuilder<Style> =
                SpansBuilder::new(new_text.len());

            // TODO: Should we be ignoring highlight errors via flattening them?
            for highlight in layers
                .highlight_iter(
                    &new_text,
                    Some(0..new_text.len()),
                    Some(&self.cancel_flag),
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

            Some(highlights.build())
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
        let tree = self.layers.as_ref()?.try_tree()?;
        let node = tree
            .root_node()
            .descendant_for_byte_range(offset, offset + 1)?;
        let char = node.kind().chars().next()?;
        let tag: &'static str = matching_bracket_general(char)?;

        if let Some(offset) = self.find_tag_in_siblings(node, true, tag) {
            return Some(offset);
        }
        if let Some(offset) = self.find_tag_in_siblings(node, false, tag) {
            return Some(offset);
        }
        None
    }

    pub fn parent_offset(&self, offset: usize) -> Option<usize> {
        let tree = self.layers.as_ref()?.try_tree()?;
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
        let tree = self.layers.as_ref()?.try_tree()?;
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
        let tree = self.layers.as_ref()?.try_tree()?;
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
        if offset >= self.text.len() {
            return None;
        }

        let tree = self.layers.as_ref()?.try_tree()?;
        let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;

        loop {
            let start = node.start_byte();
            if start >= self.text.len() {
                return None;
            }
            if start < offset {
                let c = self.text.byte_at(start) as char;
                if c == '(' {
                    let end = self.find_matching_pair(start)?;
                    if end >= offset && start < offset {
                        return Some((start, end));
                    }
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
        if self.language == LapceLanguage::Markdown {
            // TODO: fix the issue that sometimes node.prev_sibling can stuck for markdown
            return None;
        }

        if offset >= self.text.len() {
            return None;
        }

        let tree = self.layers.as_ref()?.try_tree()?;
        let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;

        loop {
            let start = node.start_byte();
            if start >= self.text.len() {
                return None;
            }
            if start < offset {
                let c = self.text.byte_at(start) as char;
                if matching_pair_direction(c) == Some(true) {
                    let end = self.find_matching_pair(start)?;
                    if end >= offset {
                        return Some((start, end));
                    }
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
