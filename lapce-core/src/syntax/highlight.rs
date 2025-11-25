/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Much of the code in this file is modified from [helix](https://github.com/helix-editor/helix)'s implementation of their syntax highlighting, which is under the MPL.
 */

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use arc_swap::ArcSwap;
use lapce_xi_rope::Rope;
use once_cell::sync::Lazy;
use regex::Regex;
use tree_sitter::{
    Language, Point, Query, QueryCaptures, QueryCursor, QueryMatch, Tree,
};

use super::{PARSER, util::RopeProvider};
use crate::{language::LapceLanguage, style::SCOPES};

thread_local! {
    static HIGHLIGHT_CONFIGS: RefCell<HashMap<LapceLanguage, Result<Arc<HighlightConfiguration>, HighlightIssue>>> = Default::default();
}

pub fn reset_highlight_configs() {
    HIGHLIGHT_CONFIGS.with_borrow_mut(|configs| {
        configs.clear();
    });
}

pub(crate) fn get_highlight_config(
    lang: LapceLanguage,
) -> Result<Arc<HighlightConfiguration>, HighlightIssue> {
    HIGHLIGHT_CONFIGS.with(|configs| {
        let mut configs = configs.borrow_mut();
        let config = configs
            .entry(lang)
            .or_insert_with(|| lang.new_highlight_config().map(Arc::new));
        config.clone()
    })
}

/// Indicates which highlight should be applied to a region of source code.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Highlight(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightIssue {
    Error(String),
    NotAvailable,
}

/// Represents a single step in rendering a syntax-highlighted document.
#[derive(Copy, Clone, Debug)]
pub enum HighlightEvent {
    Source { start: usize, end: usize },
    HighlightStart(Highlight),
    HighlightEnd,
}

#[derive(Debug)]
pub(crate) struct LocalDef<'a> {
    name: Cow<'a, str>,
    value_range: std::ops::Range<usize>,
    highlight: Option<Highlight>,
}

#[derive(Debug)]
pub(crate) struct LocalScope<'a> {
    pub(crate) inherits: bool,
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) local_defs: Vec<LocalDef<'a>>,
}

#[derive(Debug, Clone)]
pub enum InjectionLanguageMarker<'a> {
    Name(Cow<'a, str>),
    Filename(Cow<'a, Path>),
    Shebang(String),
}

const SHEBANG: &str = r"#!\s*(?:\S*[/\\](?:env\s+(?:\-\S+\s+)*)?)?([^\s\.\d]+)";

const CANCELLATION_CHECK_INTERVAL: usize = 100;

/// Contains the data needed to highlight code written in a particular language.
///
/// This struct is immutable and can be shared between threads.
#[derive(Debug)]
pub struct HighlightConfiguration {
    pub language: Language,
    pub query: Query,
    pub injections_query: Query,
    pub combined_injections_patterns: Vec<usize>,
    pub highlights_pattern_index: usize,
    pub highlight_indices: ArcSwap<Vec<Option<Highlight>>>,
    pub non_local_variable_patterns: Vec<bool>,
    pub injection_content_capture_index: Option<u32>,
    pub injection_language_capture_index: Option<u32>,
    pub injection_filename_capture_index: Option<u32>,
    pub injection_shebang_capture_index: Option<u32>,
    pub local_scope_capture_index: Option<u32>,
    pub local_def_capture_index: Option<u32>,
    pub local_def_value_capture_index: Option<u32>,
    pub local_ref_capture_index: Option<u32>,
}

impl HighlightConfiguration {
    /// Creates a `HighlightConfiguration` for a given `Language` and set of highlighting
    /// queries.
    ///
    /// # Parameters
    ///
    /// * `language`  - The Tree-sitter `Language` that should be used for parsing.
    /// * `highlights_query` - A string containing tree patterns for syntax highlighting. This
    ///   should be non-empty, otherwise no syntax highlights will be added.
    /// * `injections_query` -  A string containing tree patterns for injecting other languages
    ///   into the document. This can be empty if no injections are desired.
    /// * `locals_query` - A string containing tree patterns for tracking local variable
    ///   definitions and references. This can be empty if local variable tracking is not needed.
    ///
    /// Returns a `HighlightConfiguration` that can then be used with the `highlight` method.
    pub fn new(
        language: Language,
        highlights_query: &str,
        injection_query: &str,
        locals_query: &str,
    ) -> Result<Self, tree_sitter::QueryError> {
        // Concatenate the query strings, keeping track of the start offset of each section.
        let mut query_source = String::new();
        query_source.push_str(locals_query);
        let highlights_query_offset = query_source.len();
        query_source.push_str(highlights_query);

        // Construct a single query by concatenating the three query strings, but record the
        // range of pattern indices that belong to each individual string.
        let query = Query::new(&language, &query_source)?;
        let mut highlights_pattern_index = 0;
        for i in 0..(query.pattern_count()) {
            let pattern_offset = query.start_byte_for_pattern(i);
            if pattern_offset < highlights_query_offset {
                highlights_pattern_index += 1;
            }
        }

        let injections_query = Query::new(&language, injection_query)?;
        let combined_injections_patterns = (0..injections_query.pattern_count())
            .filter(|&i| {
                injections_query
                    .property_settings(i)
                    .iter()
                    .any(|s| &*s.key == "injection.combined")
            })
            .collect();

        // Find all of the highlighting patterns that are disabled for nodes that
        // have been identified as local variables.
        let non_local_variable_patterns = (0..query.pattern_count())
            .map(|i| {
                query.property_predicates(i).iter().any(|(prop, positive)| {
                    !*positive && prop.key.as_ref() == "local"
                })
            })
            .collect();

        // Store the numeric ids for all of the special captures.
        let mut injection_content_capture_index = None;
        let mut injection_language_capture_index = None;
        let mut injection_filename_capture_index = None;
        let mut injection_shebang_capture_index = None;
        let mut local_def_capture_index = None;
        let mut local_def_value_capture_index = None;
        let mut local_ref_capture_index = None;
        let mut local_scope_capture_index = None;
        for (i, name) in query.capture_names().iter().enumerate() {
            let i = Some(i as u32);
            match *name {
                "local.definition" => local_def_capture_index = i,
                "local.definition-value" => local_def_value_capture_index = i,
                "local.reference" => local_ref_capture_index = i,
                "local.scope" => local_scope_capture_index = i,
                _ => {}
            }
        }

        for (i, name) in injections_query.capture_names().iter().enumerate() {
            let i = Some(i as u32);
            match *name {
                "injection.content" => injection_content_capture_index = i,
                "injection.language" => injection_language_capture_index = i,
                "injection.filename" => injection_filename_capture_index = i,
                "injection.shebang" => injection_shebang_capture_index = i,
                _ => {}
            }
        }

        let highlight_indices: ArcSwap<Vec<_>> =
            ArcSwap::from_pointee(vec![None; query.capture_names().len()]);
        let conf = Self {
            language,
            query,
            injections_query,
            combined_injections_patterns,
            highlights_pattern_index,
            highlight_indices,
            non_local_variable_patterns,
            injection_content_capture_index,
            injection_language_capture_index,
            injection_shebang_capture_index,
            injection_filename_capture_index,
            local_scope_capture_index,
            local_def_capture_index,
            local_def_value_capture_index,
            local_ref_capture_index,
        };
        conf.configure(SCOPES);
        Ok(conf)
    }

    /// Get a slice containing all of the highlight names used in the configuration.
    pub fn names(&self) -> &[&str] {
        self.query.capture_names()
    }

    /// Set the list of recognized highlight names.
    ///
    /// Tree-sitter syntax-highlighting queries specify highlights in the form of dot-separated
    /// highlight names like `punctuation.bracket` and `function.method.builtin`. Consumers of
    /// these queries can choose to recognize highlights with different levels of specificity.
    /// For example, the string `function.builtin` will match against `function.builtin.constructor`
    /// but will not match `function.method.builtin` and `function.method`.
    ///
    /// When highlighting, results are returned as `Highlight` values, which contain the index
    /// of the matched highlight this list of highlight names.
    pub fn configure(&self, recognized_names: &[&str]) {
        let mut capture_parts = Vec::new();
        let indices: Vec<_> = self
            .query
            .capture_names()
            .iter()
            .map(move |capture_name| {
                capture_parts.clear();
                capture_parts.extend(capture_name.split('.'));

                let mut best_index = None;
                let mut best_match_len = 0;
                for (i, recognized_name) in recognized_names.iter().enumerate() {
                    let mut len = 0;
                    let mut matches = true;
                    for (i, part) in recognized_name.split('.').enumerate() {
                        match capture_parts.get(i) {
                            Some(capture_part) if *capture_part == part => len += 1,
                            _ => {
                                matches = false;
                                break;
                            }
                        }
                    }
                    if matches && len > best_match_len {
                        best_index = Some(i);
                        best_match_len = len;
                    }
                }
                best_index.map(Highlight)
            })
            .collect();

        self.highlight_indices.store(Arc::new(indices));
    }

    pub fn injection_pair<'a>(
        &self,
        query_match: &QueryMatch<'a, 'a>,
        source: &'a Rope,
    ) -> (
        Option<InjectionLanguageMarker<'a>>,
        Option<tree_sitter::Node<'a>>,
    ) {
        let mut injection_capture = None;
        let mut content_node = None;

        for capture in query_match.captures {
            let index = Some(capture.index);
            if index == self.injection_language_capture_index {
                let name = source.slice_to_cow(capture.node.byte_range());
                injection_capture = Some(InjectionLanguageMarker::Name(name));
            } else if index == self.injection_filename_capture_index {
                let name = source.slice_to_cow(capture.node.byte_range());
                let path = Path::new(name.as_ref()).to_path_buf();
                injection_capture =
                    Some(InjectionLanguageMarker::Filename(path.into()));
            } else if index == self.injection_shebang_capture_index {
                let node_slice = source.slice(capture.node.byte_range());
                let max_line = node_slice.line_of_offset(node_slice.len());
                let node_slice = if max_line > 0 {
                    node_slice.slice(..node_slice.offset_of_line(1))
                } else {
                    node_slice
                };

                static SHEBANG_REGEX: Lazy<Regex> =
                    Lazy::new(|| Regex::new(SHEBANG).unwrap());

                injection_capture = SHEBANG_REGEX
                    .captures_iter(&node_slice.slice_to_cow(..))
                    .map(|cap| InjectionLanguageMarker::Shebang(cap[1].to_string()))
                    .next()
            } else if index == self.injection_content_capture_index {
                content_node = Some(capture.node);
            }
        }
        (injection_capture, content_node)
    }

    pub(crate) fn injection_for_match<'a>(
        &self,
        query: &'a Query,
        query_match: &QueryMatch<'a, 'a>,
        source: &'a Rope,
    ) -> (
        Option<InjectionLanguageMarker<'a>>,
        Option<tree_sitter::Node<'a>>,
        IncludedChildren,
    ) {
        let (mut injection_capture, content_node) =
            self.injection_pair(query_match, source);

        let mut included_children = IncludedChildren::default();
        for prop in query.property_settings(query_match.pattern_index) {
            match prop.key.as_ref() {
                // In addition to specifying the language name via the text of a
                // captured node, it can also be hard-coded via a `#set!` predicate
                // that sets the injection.language key.
                "injection.language" if injection_capture.is_none() => {
                    injection_capture = prop
                        .value
                        .as_ref()
                        .map(|s| InjectionLanguageMarker::Name(s.as_ref().into()));
                }

                // By default, injections do not include the *children* of an
                // `injection.content` node - only the ranges that belong to the
                // node itself. This can be changed using a `#set!` predicate that
                // sets the `injection.include-children` key.
                "injection.include-children" => {
                    included_children = IncludedChildren::All
                }

                // Some queries might only exclude named children but include unnamed
                // children in their `injection.content` node. This can be enabled using
                // a `#set!` predicate that sets the `injection.include-unnamed-children` key.
                "injection.include-unnamed-children" => {
                    included_children = IncludedChildren::Unnamed
                }
                _ => {}
            }
        }

        (injection_capture, content_node, included_children)
    }
}

#[derive(Debug)]
pub(crate) struct HighlightIter<'a> {
    pub(crate) source: &'a Rope,
    pub(crate) byte_offset: usize,
    pub(crate) cancellation_flag: Option<&'a AtomicUsize>,
    pub(crate) layers: Vec<HighlightIterLayer<'a>>,
    pub(crate) iter_count: usize,
    pub(crate) next_event: Option<HighlightEvent>,
    pub(crate) last_highlight_range: Option<(usize, usize, usize)>,
}

pub(crate) struct HighlightIterLayer<'a> {
    pub(crate) _tree: Option<Tree>,
    pub(crate) cursor: QueryCursor,
    pub(crate) captures: RefCell<
        std::iter::Peekable<QueryCaptures<'a, 'a, RopeProvider<'a>, &'a [u8]>>,
    >,
    pub(crate) config: &'a HighlightConfiguration,
    pub(crate) highlight_end_stack: Vec<usize>,
    pub(crate) scope_stack: Vec<LocalScope<'a>>,
    pub(crate) depth: usize,
}

impl std::fmt::Debug for HighlightIterLayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightIterLayer").finish()
    }
}

impl HighlightIterLayer<'_> {
    // First, sort scope boundaries by their byte offset in the document. At a
    // given position, emit scope endings before scope beginnings. Finally, emit
    // scope boundaries from deeper layers first.
    pub fn sort_key(&self) -> Option<(usize, bool, isize)> {
        let depth = -(self.depth as isize);
        let next_start = self
            .captures
            .borrow_mut()
            .peek()
            .map(|(m, i)| m.captures[*i].node.start_byte());
        let next_end = self.highlight_end_stack.last().cloned();
        match (next_start, next_end) {
            (Some(start), Some(end)) => {
                if start < end {
                    Some((start, true, depth))
                } else {
                    Some((end, false, depth))
                }
            }
            (Some(i), None) => Some((i, true, depth)),
            (None, Some(j)) => Some((j, false, depth)),
            _ => None,
        }
    }
}

impl HighlightIter<'_> {
    fn emit_event(
        &mut self,
        offset: usize,
        event: Option<HighlightEvent>,
    ) -> Option<Result<HighlightEvent, super::Error>> {
        let result;
        if self.byte_offset < offset {
            result = Some(Ok(HighlightEvent::Source {
                start: self.byte_offset,
                end: offset,
            }));
            self.byte_offset = offset;
            self.next_event = event;
        } else {
            result = event.map(Ok);
        }
        self.sort_layers();
        result
    }

    pub(crate) fn sort_layers(&mut self) {
        while !self.layers.is_empty() {
            if let Some(sort_key) = self.layers[0].sort_key() {
                let mut i = 0;
                while i + 1 < self.layers.len() {
                    if let Some(next_offset) = self.layers[i + 1].sort_key() {
                        if next_offset < sort_key {
                            i += 1;
                            continue;
                        }
                    } else {
                        let layer = self.layers.remove(i + 1);
                        PARSER.with(|ts_parser| {
                            let highlighter = &mut ts_parser.borrow_mut();
                            highlighter.cursors.push(layer.cursor);
                        });
                    }
                    break;
                }
                if i > 0 {
                    self.layers[0..(i + 1)].rotate_left(1);
                }
                break;
            } else {
                let layer = self.layers.remove(0);
                PARSER.with(|ts_parser| {
                    let highlighter = &mut ts_parser.borrow_mut();
                    highlighter.cursors.push(layer.cursor);
                });
            }
        }
    }
}

impl Iterator for HighlightIter<'_> {
    type Item = Result<HighlightEvent, super::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        'main: loop {
            // If we've already determined the next highlight boundary, just return it.
            if let Some(e) = self.next_event.take() {
                return Some(Ok(e));
            }

            // Periodically check for cancellation, returning `Cancelled` error if the
            // cancellation flag was flipped.
            if let Some(cancellation_flag) = self.cancellation_flag {
                self.iter_count += 1;
                if self.iter_count >= CANCELLATION_CHECK_INTERVAL {
                    self.iter_count = 0;
                    if cancellation_flag.load(Ordering::Relaxed) != 0 {
                        return Some(Err(super::Error::Cancelled));
                    }
                }
            }

            // If none of the layers have any more highlight boundaries, terminate.
            if self.layers.is_empty() {
                let len = self.source.len();
                return if self.byte_offset < len {
                    let result = Some(Ok(HighlightEvent::Source {
                        start: self.byte_offset,
                        end: len,
                    }));
                    self.byte_offset = len;
                    result
                } else {
                    None
                };
            }

            // Get the next capture from whichever layer has the earliest highlight boundary.
            let range;
            let layer = &mut self.layers[0];
            let captures = layer.captures.get_mut();
            if let Some((next_match, capture_index)) = captures.peek() {
                let next_capture = next_match.captures[*capture_index];
                range = next_capture.node.byte_range();

                // If any previous highlight ends before this node starts, then before
                // processing this capture, emit the source code up until the end of the
                // previous highlight, and an end event for that highlight.
                if let Some(end_byte) = layer.highlight_end_stack.last().cloned() {
                    if end_byte <= range.start {
                        layer.highlight_end_stack.pop();
                        return self.emit_event(
                            end_byte,
                            Some(HighlightEvent::HighlightEnd),
                        );
                    }
                }
            }
            // If there are no more captures, then emit any remaining highlight end events.
            // And if there are none of those, then just advance to the end of the document.
            else if let Some(end_byte) = layer.highlight_end_stack.last().cloned()
            {
                layer.highlight_end_stack.pop();
                return self
                    .emit_event(end_byte, Some(HighlightEvent::HighlightEnd));
            } else {
                return self.emit_event(self.source.len(), None);
            };

            let (mut match_, capture_index) = captures.next().unwrap();
            let mut capture = match_.captures[capture_index];

            // Remove from the local scope stack any local scopes that have already ended.
            while range.start > layer.scope_stack.last().unwrap().range.end {
                layer.scope_stack.pop();
            }

            // If this capture is for tracking local variables, then process the
            // local variable info.
            let mut reference_highlight = None;
            let mut definition_highlight = None;
            while match_.pattern_index < layer.config.highlights_pattern_index {
                // If the node represents a local scope, push a new local scope onto
                // the scope stack.
                if Some(capture.index) == layer.config.local_scope_capture_index {
                    definition_highlight = None;
                    let mut scope = LocalScope {
                        inherits: true,
                        range: range.clone(),
                        local_defs: Vec::new(),
                    };
                    for prop in
                        layer.config.query.property_settings(match_.pattern_index)
                    {
                        if let "local.scope-inherits" = prop.key.as_ref() {
                            scope.inherits = prop
                                .value
                                .as_ref()
                                .is_none_or(|r| r.as_ref() == "true");
                        }
                    }
                    layer.scope_stack.push(scope);
                }
                // If the node represents a definition, add a new definition to the
                // local scope at the top of the scope stack.
                else if Some(capture.index) == layer.config.local_def_capture_index
                {
                    reference_highlight = None;
                    let scope = layer.scope_stack.last_mut().unwrap();

                    let mut value_range = 0..0;
                    for capture in match_.captures {
                        if Some(capture.index)
                            == layer.config.local_def_value_capture_index
                        {
                            value_range = capture.node.byte_range();
                        }
                    }

                    let name = self.source.slice_to_cow(range.clone());
                    scope.local_defs.push(LocalDef {
                        name,
                        value_range,
                        highlight: None,
                    });
                    definition_highlight =
                        scope.local_defs.last_mut().map(|s| &mut s.highlight);
                }
                // If the node represents a reference, then try to find the corresponding
                // definition in the scope stack.
                else if Some(capture.index) == layer.config.local_ref_capture_index
                    && definition_highlight.is_none()
                {
                    definition_highlight = None;
                    let name = self.source.slice_to_cow(range.clone());
                    for scope in layer.scope_stack.iter().rev() {
                        if let Some(highlight) =
                            scope.local_defs.iter().rev().find_map(|def| {
                                if def.name == name
                                    && range.start >= def.value_range.end
                                {
                                    Some(def.highlight)
                                } else {
                                    None
                                }
                            })
                        {
                            reference_highlight = highlight;
                            break;
                        }
                        if !scope.inherits {
                            break;
                        }
                    }
                }

                // Continue processing any additional matches for the same node.
                if let Some((next_match, next_capture_index)) = captures.peek() {
                    let next_capture = next_match.captures[*next_capture_index];
                    if next_capture.node == capture.node {
                        capture = next_capture;
                        match_ = captures.next().unwrap().0;
                        continue;
                    }
                }

                self.sort_layers();
                continue 'main;
            }

            // Otherwise, this capture must represent a highlight.
            // If this exact range has already been highlighted by an earlier pattern, or by
            // a different layer, then skip over this one.
            if let Some((last_start, last_end, last_depth)) =
                self.last_highlight_range
            {
                if range.start == last_start
                    && range.end == last_end
                    && layer.depth < last_depth
                {
                    self.sort_layers();
                    continue 'main;
                }
            }

            // If the current node was found to be a local variable, then skip over any
            // highlighting patterns that are disabled for local variables.
            if definition_highlight.is_some() || reference_highlight.is_some() {
                while layer.config.non_local_variable_patterns[match_.pattern_index]
                {
                    if let Some((next_match, next_capture_index)) = captures.peek() {
                        let next_capture = next_match.captures[*next_capture_index];
                        if next_capture.node == capture.node {
                            capture = next_capture;
                            match_ = captures.next().unwrap().0;
                            continue;
                        }
                    }

                    self.sort_layers();
                    continue 'main;
                }
            }

            // Once a highlighting pattern is found for the current node, skip over
            // any later highlighting patterns that also match this node. Captures
            // for a given node are ordered by pattern index, so these subsequent
            // captures are guaranteed to be for highlighting, not injections or
            // local variables.
            while let Some((next_match, next_capture_index)) = captures.peek() {
                let next_capture = next_match.captures[*next_capture_index];
                if next_capture.node == capture.node {
                    captures.next();
                } else {
                    break;
                }
            }

            let current_highlight =
                layer.config.highlight_indices.load()[capture.index as usize];

            // If this node represents a local definition, then store the current
            // highlight value on the local scope entry representing this node.
            if let Some(definition_highlight) = definition_highlight {
                *definition_highlight = current_highlight;
            }

            // Emit a scope start event and push the node's end position to the stack.
            if let Some(highlight) = reference_highlight.or(current_highlight) {
                self.last_highlight_range =
                    Some((range.start, range.end, layer.depth));
                layer.highlight_end_stack.push(range.end);
                return self.emit_event(
                    range.start,
                    Some(HighlightEvent::HighlightStart(highlight)),
                );
            }

            self.sort_layers();
        }
    }
}

#[derive(Clone, Default)]
pub(crate) enum IncludedChildren {
    #[default]
    None,
    All,
    Unnamed,
}

// Compute the ranges that should be included when parsing an injection.
// This takes into account three things:
// * `parent_ranges` - The ranges must all fall within the *current* layer's ranges.
// * `nodes` - Every injection takes place within a set of nodes. The injection ranges
//   are the ranges of those nodes.
// * `includes_children` - For some injections, the content nodes' children should be
//   excluded from the nested document, so that only the content nodes' *own* content
//   is reparsed. For other injections, the content nodes' entire ranges should be
//   reparsed, including the ranges of their children.
pub(crate) fn intersect_ranges(
    parent_ranges: &[tree_sitter::Range],
    nodes: &[tree_sitter::Node],
    included_children: IncludedChildren,
) -> Vec<tree_sitter::Range> {
    let mut cursor = nodes[0].walk();
    let mut result = Vec::new();
    let mut parent_range_iter = parent_ranges.iter();
    let mut parent_range = parent_range_iter
        .next()
        .expect("Layers should only be constructed with non-empty ranges vectors");
    for node in nodes.iter() {
        let mut preceding_range = tree_sitter::Range {
            start_byte: 0,
            start_point: Point::new(0, 0),
            end_byte: node.start_byte(),
            end_point: node.start_position(),
        };
        let following_range = tree_sitter::Range {
            start_byte: node.end_byte(),
            start_point: node.end_position(),
            end_byte: usize::MAX,
            end_point: Point::new(usize::MAX, usize::MAX),
        };

        for excluded_range in node
            .children(&mut cursor)
            .filter_map(|child| match included_children {
                IncludedChildren::None => Some(child.range()),
                IncludedChildren::All => None,
                IncludedChildren::Unnamed => {
                    if child.is_named() {
                        Some(child.range())
                    } else {
                        None
                    }
                }
            })
            .chain([following_range].iter().cloned())
        {
            let mut range = tree_sitter::Range {
                start_byte: preceding_range.end_byte,
                start_point: preceding_range.end_point,
                end_byte: excluded_range.start_byte,
                end_point: excluded_range.start_point,
            };
            preceding_range = excluded_range;

            if range.end_byte < parent_range.start_byte {
                continue;
            }

            while parent_range.start_byte <= range.end_byte {
                if parent_range.end_byte > range.start_byte {
                    if range.start_byte < parent_range.start_byte {
                        range.start_byte = parent_range.start_byte;
                        range.start_point = parent_range.start_point;
                    }

                    if parent_range.end_byte < range.end_byte {
                        if range.start_byte < parent_range.end_byte {
                            result.push(tree_sitter::Range {
                                start_byte: range.start_byte,
                                start_point: range.start_point,
                                end_byte: parent_range.end_byte,
                                end_point: parent_range.end_point,
                            });
                        }
                        range.start_byte = parent_range.end_byte;
                        range.start_point = parent_range.end_point;
                    } else {
                        if range.start_byte < range.end_byte {
                            result.push(range);
                        }
                        break;
                    }
                }

                if let Some(next_range) = parent_range_iter.next() {
                    parent_range = next_range;
                } else {
                    return result;
                }
            }
        }
    }
    result
}
