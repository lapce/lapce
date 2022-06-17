use std::{
    iter, mem, ops, str,
    sync::atomic::{AtomicUsize, Ordering},
};

use lapce_rpc::style::{LineStyle, Style};
use thiserror::Error;
use tree_sitter::{
    Language, LossyUtf8, Node, Point, Query, QueryCaptures, QueryCursor, QueryError,
    QueryMatch, Range, Tree,
};
use xi_rope::{spans::Spans, Rope};

const CANCELLATION_CHECK_INTERVAL: usize = 100;
const BUFFER_HTML_RESERVE_CAPACITY: usize = 10 * 1024;
const BUFFER_LINES_RESERVE_CAPACITY: usize = 1000;
pub const SCOPES: &[&str] = &[
    "constant",
    "type",
    "type.builtin",
    "property",
    "comment",
    "constructor",
    "function",
    "label",
    "keyword",
    "string",
    "variable",
    "variable.other.member",
    "operator",
    "attribute",
    "escape",
    "embedded",
];

/// Indicates which highlight should be applied to a region of source code.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Highlight(pub usize);

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

/// Represents a single step in rendering a syntax-highlighted document.
#[derive(Copy, Clone, Debug)]
pub enum HighlightEvent {
    Source { start: usize, end: usize },
    HighlightStart(Highlight),
    HighlightEnd,
}

/// Contains the data needed to highlight code written in a particular language.
///
/// This struct is immutable and can be shared between threads.
pub struct HighlightConfiguration {
    pub language: Language,
    pub query: Query,
    combined_injections_query: Option<Query>,
    locals_pattern_index: usize,
    highlights_pattern_index: usize,
    highlight_indices: Vec<Option<Highlight>>,
    non_local_variable_patterns: Vec<bool>,
    injection_content_capture_index: Option<u32>,
    injection_language_capture_index: Option<u32>,
    local_scope_capture_index: Option<u32>,
    local_def_capture_index: Option<u32>,
    local_def_value_capture_index: Option<u32>,
    local_ref_capture_index: Option<u32>,
}

/// Performs syntax highlighting, recognizing a given list of highlight names.
///
/// For the best performance `Highlighter` values should be reused between
/// syntax highlighting calls. A separate highlighter is needed for each thread that
/// is performing highlighting.
pub struct Highlighter {
    cursors: Vec<QueryCursor>,
}

/// Converts a general-purpose syntax highlighting iterator into a sequence of lines of HTML.
pub struct HtmlRenderer {
    pub html: Vec<u8>,
    pub line_offsets: Vec<u32>,
    carriage_return_highlight: Option<Highlight>,
}

#[derive(Debug)]
struct LocalDef<'a> {
    name: &'a str,
    value_range: ops::Range<usize>,
    highlight: Option<Highlight>,
}

#[derive(Debug)]
struct LocalScope<'a> {
    inherits: bool,
    range: ops::Range<usize>,
    local_defs: Vec<LocalDef<'a>>,
}

struct HighlightIter<'a, F>
where
    F: FnMut(&str) -> Option<&'a HighlightConfiguration> + 'a,
{
    tree: Tree,
    source: &'a [u8],
    byte_offset: usize,
    highlighter: &'a mut Highlighter,
    injection_callback: F,
    cancellation_flag: Option<&'a AtomicUsize>,
    layers: Vec<HighlightIterLayer<'a>>,
    iter_count: usize,
    next_event: Option<HighlightEvent>,
    last_highlight_range: Option<(usize, usize, usize)>,
}

struct HighlightIterLayer<'a> {
    _tree: Tree,
    cursor: QueryCursor,
    captures: iter::Peekable<QueryCaptures<'a, 'a, &'a [u8]>>,
    config: &'a HighlightConfiguration,
    highlight_end_stack: Vec<usize>,
    scope_stack: Vec<LocalScope<'a>>,
    ranges: Vec<Range>,
    depth: usize,
}

impl Highlight {
    pub fn str(&self) -> &str {
        SCOPES[self.0]
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            cursors: Vec::new(),
        }
    }

    /// Iterate over the highlighted regions for a given slice of source code.
    pub fn highlight<'a>(
        &'a mut self,
        tree: Tree,
        config: &'a HighlightConfiguration,
        source: &'a [u8],
        cancellation_flag: Option<&'a AtomicUsize>,
        mut injection_callback: impl FnMut(&str) -> Option<&'a HighlightConfiguration>
            + 'a,
    ) -> impl Iterator<Item = Result<HighlightEvent, Error>> + 'a {
        let layers = HighlightIterLayer::new(
            tree.clone(),
            source,
            self,
            &mut injection_callback,
            config,
            0,
            vec![Range {
                start_byte: 0,
                end_byte: usize::MAX,
                start_point: Point::new(0, 0),
                end_point: Point::new(usize::MAX, usize::MAX),
            }],
        );
        assert_ne!(layers.len(), 0);
        let mut result = HighlightIter {
            tree,
            source,
            byte_offset: 0,
            injection_callback,
            cancellation_flag,
            highlighter: self,
            iter_count: 0,
            layers,
            next_event: None,
            last_highlight_range: None,
        };
        result.sort_layers();
        result
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
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
    ) -> Result<Self, QueryError> {
        // Concatenate the query strings, keeping track of the start offset of each section.
        let mut query_source = String::new();
        query_source.push_str(injection_query);
        let locals_query_offset = query_source.len();
        query_source.push_str(locals_query);
        let highlights_query_offset = query_source.len();
        query_source.push_str(highlights_query);

        // Construct a single query by concatenating the three query strings, but record the
        // range of pattern indices that belong to each individual string.
        let mut query = Query::new(language, &query_source)?;
        let mut locals_pattern_index = 0;
        let mut highlights_pattern_index = 0;
        for i in 0..(query.pattern_count()) {
            let pattern_offset = query.start_byte_for_pattern(i);
            if pattern_offset < highlights_query_offset {
                if pattern_offset < highlights_query_offset {
                    highlights_pattern_index += 1;
                }
                if pattern_offset < locals_query_offset {
                    locals_pattern_index += 1;
                }
            }
        }

        // Construct a separate query just for dealing with the 'combined injections'.
        // Disable the combined injection patterns in the main query.
        let mut combined_injections_query = Query::new(language, injection_query)?;
        let mut has_combined_queries = false;
        for pattern_index in 0..locals_pattern_index {
            let settings = query.property_settings(pattern_index);
            if settings.iter().any(|s| &*s.key == "injection.combined") {
                has_combined_queries = true;
                query.disable_pattern(pattern_index);
            } else {
                combined_injections_query.disable_pattern(pattern_index);
            }
        }
        let combined_injections_query = if has_combined_queries {
            Some(combined_injections_query)
        } else {
            None
        };

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
        let mut local_def_capture_index = None;
        let mut local_def_value_capture_index = None;
        let mut local_ref_capture_index = None;
        let mut local_scope_capture_index = None;
        for (i, name) in query.capture_names().iter().enumerate() {
            let i = Some(i as u32);
            match name.as_str() {
                "injection.content" => injection_content_capture_index = i,
                "injection.language" => injection_language_capture_index = i,
                "local.definition" => local_def_capture_index = i,
                "local.definition-value" => local_def_value_capture_index = i,
                "local.reference" => local_ref_capture_index = i,
                "local.scope" => local_scope_capture_index = i,
                _ => {}
            }
        }

        let highlight_indices = vec![None; query.capture_names().len()];
        let mut conf = HighlightConfiguration {
            language,
            query,
            combined_injections_query,
            locals_pattern_index,
            highlights_pattern_index,
            highlight_indices,
            non_local_variable_patterns,
            injection_content_capture_index,
            injection_language_capture_index,
            local_def_capture_index,
            local_def_value_capture_index,
            local_ref_capture_index,
            local_scope_capture_index,
        };
        conf.configure(SCOPES);
        Ok(conf)
    }

    /// Get a slice containing all of the highlight names used in the configuration.
    pub fn names(&self) -> &[String] {
        self.query.capture_names()
    }

    /// Set the list of recognized highlight names.
    ///
    /// Tree-sitter syntax-highlighting queries specify highlights in the form of dot-separated
    /// highlight names like `punctuation.bracket` and `function.method.builtin`. Consumers of
    /// these queries can choose to recognize highlights with different levels of specificity.
    /// For example, the string `function.builtin` will match against `function.method.builtin`
    /// and `function.builtin.constructor`, but will not match `function.method`.
    ///
    /// When highlighting, results are returned as `Highlight` values, which contain the index
    /// of the matched highlight this list of highlight names.
    fn configure(&mut self, recognized_names: &[impl AsRef<str>]) {
        let mut capture_parts = Vec::new();
        self.highlight_indices.clear();
        self.highlight_indices
            .extend(self.query.capture_names().iter().map(move |capture_name| {
                capture_parts.clear();
                capture_parts.extend(capture_name.split('.'));

                let mut best_index = None;
                let mut best_match_len = 0;
                for (i, recognized_name) in recognized_names.iter().enumerate() {
                    let mut len = 0;
                    let mut matches = true;
                    for part in recognized_name.as_ref().split('.') {
                        len += 1;
                        if !capture_parts.contains(&part) {
                            matches = false;
                            break;
                        }
                    }
                    if matches && len > best_match_len {
                        best_index = Some(i);
                        best_match_len = len;
                    }
                }
                best_index.map(Highlight)
            }));
    }
}

impl<'a> HighlightIterLayer<'a> {
    /// Create a new 'layer' of highlighting for this document.
    ///
    /// In the even that the new layer contains "combined injections" (injections where multiple
    /// disjoint ranges are parsed as one syntax tree), these will be eagerly processed and
    /// added to the returned vector.
    fn new<F: FnMut(&str) -> Option<&'a HighlightConfiguration> + 'a>(
        tree: Tree,
        source: &'a [u8],
        highlighter: &mut Highlighter,
        injection_callback: &mut F,
        mut config: &'a HighlightConfiguration,
        mut depth: usize,
        mut ranges: Vec<Range>,
    ) -> Vec<Self> {
        let mut result = Vec::with_capacity(1);
        let mut queue = Vec::new();
        loop {
            let tree = tree.clone();
            let mut cursor =
                highlighter.cursors.pop().unwrap_or_else(QueryCursor::new);

            // Process combined injections.
            if let Some(combined_injections_query) =
                &config.combined_injections_query
            {
                let mut injections_by_pattern_index = vec![
                    (None, Vec::new(), false);
                    combined_injections_query
                        .pattern_count()
                ];
                let matches = cursor.matches(
                    combined_injections_query,
                    tree.root_node(),
                    source,
                );
                for mat in matches {
                    let entry = &mut injections_by_pattern_index[mat.pattern_index];
                    let (language_name, content_node, include_children) =
                        injection_for_match(
                            config,
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
                    entry.2 = include_children;
                }
                for (lang_name, content_nodes, includes_children) in
                    injections_by_pattern_index
                {
                    if let (Some(lang_name), false) =
                        (lang_name, content_nodes.is_empty())
                    {
                        if let Some(next_config) = (injection_callback)(lang_name) {
                            let ranges = Self::intersect_ranges(
                                &ranges,
                                &content_nodes,
                                includes_children,
                            );
                            if !ranges.is_empty() {
                                queue.push((next_config, depth + 1, ranges));
                            }
                        }
                    }
                }
            }

            // The `captures` iterator borrows the `Tree` and the `QueryCursor`, which
            // prevents them from being moved. But both of these values are really just
            // pointers, so it's actually ok to move them.
            let tree_ref = unsafe { mem::transmute::<_, &'static Tree>(&tree) };
            let cursor_ref = unsafe {
                mem::transmute::<_, &'static mut QueryCursor>(&mut cursor)
            };
            let captures = cursor_ref
                .captures(&config.query, tree_ref.root_node(), source)
                .peekable();

            result.push(HighlightIterLayer {
                highlight_end_stack: Vec::new(),
                scope_stack: vec![LocalScope {
                    inherits: false,
                    range: 0..usize::MAX,
                    local_defs: Vec::new(),
                }],
                cursor,
                depth,
                _tree: tree,
                captures,
                config,
                ranges,
            });

            if queue.is_empty() {
                break;
            } else {
                let (next_config, next_depth, next_ranges) = queue.remove(0);
                config = next_config;
                depth = next_depth;
                ranges = next_ranges;
            }
        }

        result
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
    fn intersect_ranges(
        parent_ranges: &[Range],
        nodes: &[Node],
        includes_children: bool,
    ) -> Vec<Range> {
        let mut cursor = nodes[0].walk();
        let mut result = Vec::new();
        let mut parent_range_iter = parent_ranges.iter();
        let mut parent_range = parent_range_iter.next().expect(
            "Layers should only be constructed with non-empty ranges vectors",
        );
        for node in nodes.iter() {
            let mut preceding_range = Range {
                start_byte: 0,
                start_point: Point::new(0, 0),
                end_byte: node.start_byte(),
                end_point: node.start_position(),
            };
            let following_range = Range {
                start_byte: node.end_byte(),
                start_point: node.end_position(),
                end_byte: usize::MAX,
                end_point: Point::new(usize::MAX, usize::MAX),
            };

            for excluded_range in node
                .children(&mut cursor)
                .filter_map(|child| {
                    if includes_children {
                        None
                    } else {
                        Some(child.range())
                    }
                })
                .chain([following_range].iter().cloned())
            {
                let mut range = Range {
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
                                result.push(Range {
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

    // First, sort scope boundaries by their byte offset in the document. At a
    // given position, emit scope endings before scope beginnings. Finally, emit
    // scope boundaries from deeper layers first.
    fn sort_key(&mut self) -> Option<(usize, bool, isize)> {
        let depth = -(self.depth as isize);
        let next_start = self
            .captures
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

impl<'a, F> HighlightIter<'a, F>
where
    F: FnMut(&str) -> Option<&'a HighlightConfiguration> + 'a,
{
    fn emit_event(
        &mut self,
        offset: usize,
        event: Option<HighlightEvent>,
    ) -> Option<Result<HighlightEvent, Error>> {
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

    fn sort_layers(&mut self) {
        while !self.layers.is_empty() {
            if let Some(sort_key) = self.layers[0].sort_key() {
                let mut i = 0;
                while i + 1 < self.layers.len() {
                    if let Some(next_offset) = self.layers[i + 1].sort_key() {
                        if next_offset < sort_key {
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                if i > 0 {
                    self.layers[0..(i + 1)].rotate_left(1);
                }
                break;
            } else {
                let layer = self.layers.remove(0);
                self.highlighter.cursors.push(layer.cursor);
            }
        }
    }

    fn insert_layer(&mut self, mut layer: HighlightIterLayer<'a>) {
        if let Some(sort_key) = layer.sort_key() {
            let mut i = 1;
            while i < self.layers.len() {
                if let Some(sort_key_i) = self.layers[i].sort_key() {
                    if sort_key_i > sort_key {
                        self.layers.insert(i, layer);
                        return;
                    }
                    i += 1;
                } else {
                    self.layers.remove(i);
                }
            }
            self.layers.push(layer);
        }
    }
}

impl<'a, F> Iterator for HighlightIter<'a, F>
where
    F: FnMut(&str) -> Option<&'a HighlightConfiguration> + 'a,
{
    type Item = Result<HighlightEvent, Error>;

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
                        return Some(Err(Error::Cancelled));
                    }
                }
            }

            // If none of the layers have any more highlight boundaries, terminate.
            if self.layers.is_empty() {
                return if self.byte_offset < self.source.len() {
                    let result = Some(Ok(HighlightEvent::Source {
                        start: self.byte_offset,
                        end: self.source.len(),
                    }));
                    self.byte_offset = self.source.len();
                    result
                } else {
                    None
                };
            }

            // Get the next capture from whichever layer has the earliest highlight boundary.
            let range;
            let layer = &mut self.layers[0];
            if let Some((next_match, capture_index)) = layer.captures.peek() {
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

            let (mut match_, capture_index) = layer.captures.next().unwrap();
            let mut capture = match_.captures[capture_index];

            // If this capture represents an injection, then process the injection.
            if match_.pattern_index < layer.config.locals_pattern_index {
                let (language_name, content_node, include_children) =
                    injection_for_match(
                        layer.config,
                        &layer.config.query,
                        &match_,
                        self.source,
                    );

                // Explicitly remove this match so that none of its other captures will remain
                // in the stream of captures.
                match_.remove();

                // If a language is found with the given name, then add a new language layer
                // to the highlighted document.
                if let (Some(language_name), Some(content_node)) =
                    (language_name, content_node)
                {
                    if let Some(config) = (self.injection_callback)(language_name) {
                        let ranges = HighlightIterLayer::intersect_ranges(
                            &self.layers[0].ranges,
                            &[content_node],
                            include_children,
                        );
                        if !ranges.is_empty() {
                            for layer in HighlightIterLayer::new(
                                self.tree.clone(),
                                self.source,
                                self.highlighter,
                                &mut self.injection_callback,
                                config,
                                self.layers[0].depth + 1,
                                ranges,
                            ) {
                                self.insert_layer(layer);
                            }
                        }
                    }
                }

                self.sort_layers();
                continue 'main;
            }

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
                        if prop.key.as_ref() == "local.scope-inherits" {
                            scope.inherits = prop
                                .value
                                .as_ref()
                                .map_or(true, |r| r.as_ref() == "true");
                        }
                    }
                    layer.scope_stack.push(scope);
                }
                // If the node represents a definition, add a new definition to the
                // local scope at the top of the scope stack.
                else if Some(capture.index) == layer.config.local_def_capture_index
                {
                    reference_highlight = None;
                    definition_highlight = None;
                    let scope = layer.scope_stack.last_mut().unwrap();

                    let mut value_range = 0..0;
                    for capture in match_.captures {
                        if Some(capture.index)
                            == layer.config.local_def_value_capture_index
                        {
                            value_range = capture.node.byte_range();
                        }
                    }

                    if let Ok(name) = str::from_utf8(&self.source[range.clone()]) {
                        scope.local_defs.push(LocalDef {
                            name,
                            value_range,
                            highlight: None,
                        });
                        definition_highlight =
                            scope.local_defs.last_mut().map(|s| &mut s.highlight);
                    }
                }
                // If the node represents a reference, then try to find the corresponding
                // definition in the scope stack.
                else if Some(capture.index) == layer.config.local_ref_capture_index
                    && definition_highlight.is_none()
                {
                    definition_highlight = None;
                    if let Ok(name) = str::from_utf8(&self.source[range.clone()]) {
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
                }

                // Continue processing any additional matches for the same node.
                if let Some((next_match, next_capture_index)) = layer.captures.peek()
                {
                    let next_capture = next_match.captures[*next_capture_index];
                    if next_capture.node == capture.node {
                        capture = next_capture;
                        match_ = layer.captures.next().unwrap().0;
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
                    match_.remove();
                    if let Some((next_match, next_capture_index)) =
                        layer.captures.peek()
                    {
                        let next_capture = next_match.captures[*next_capture_index];
                        if next_capture.node == capture.node {
                            capture = next_capture;
                            match_ = layer.captures.next().unwrap().0;
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
            while let Some((next_match, next_capture_index)) = layer.captures.peek()
            {
                let next_capture = next_match.captures[*next_capture_index];
                if next_capture.node == capture.node {
                    layer.captures.next();
                } else {
                    break;
                }
            }

            let current_highlight =
                layer.config.highlight_indices[capture.index as usize];

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

pub fn line_styles(
    text: &Rope,
    line: usize,
    styles: &Spans<Style>,
) -> Vec<LineStyle> {
    let start_offset = text.offset_of_line(line);
    let end_offset = text.offset_of_line(line + 1);
    let line_styles: Vec<LineStyle> = styles
        .iter_chunks(start_offset..end_offset)
        .filter_map(|(iv, style)| {
            let start = iv.start();
            let end = iv.end();
            if start > end_offset || end < start_offset {
                None
            } else {
                let start = if start > start_offset {
                    start - start_offset
                } else {
                    0
                };
                let end = end - start_offset;
                let style = style.clone();
                Some(LineStyle { start, end, style })
            }
        })
        .collect();
    line_styles
}

impl HtmlRenderer {
    pub fn new() -> Self {
        let mut result = HtmlRenderer {
            html: Vec::with_capacity(BUFFER_HTML_RESERVE_CAPACITY),
            line_offsets: Vec::with_capacity(BUFFER_LINES_RESERVE_CAPACITY),
            carriage_return_highlight: None,
        };
        result.line_offsets.push(0);
        result
    }

    pub fn set_carriage_return_highlight(&mut self, highlight: Option<Highlight>) {
        self.carriage_return_highlight = highlight;
    }

    pub fn reset(&mut self) {
        shrink_and_clear(&mut self.html, BUFFER_HTML_RESERVE_CAPACITY);
        shrink_and_clear(&mut self.line_offsets, BUFFER_LINES_RESERVE_CAPACITY);
        self.line_offsets.push(0);
    }

    pub fn render<'a, F>(
        &mut self,
        highlighter: impl Iterator<Item = Result<HighlightEvent, Error>>,
        source: &'a [u8],
        attribute_callback: &F,
    ) -> Result<(), Error>
    where
        F: Fn(Highlight) -> &'a [u8],
    {
        let mut highlights = Vec::new();
        for event in highlighter {
            match event {
                Ok(HighlightEvent::HighlightStart(s)) => {
                    highlights.push(s);
                    self.start_highlight(s, attribute_callback);
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    highlights.pop();
                    self.end_highlight();
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    self.add_text(
                        &source[start..end],
                        &highlights,
                        attribute_callback,
                    );
                }
                Err(a) => return Err(a),
            }
        }
        if self.html.last() != Some(&b'\n') {
            self.html.push(b'\n');
        }
        if self.line_offsets.last() == Some(&(self.html.len() as u32)) {
            self.line_offsets.pop();
        }
        Ok(())
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.line_offsets
            .iter()
            .enumerate()
            .map(move |(i, line_start)| {
                let line_start = *line_start as usize;
                let line_end = if i + 1 == self.line_offsets.len() {
                    self.html.len()
                } else {
                    self.line_offsets[i + 1] as usize
                };
                str::from_utf8(&self.html[line_start..line_end]).unwrap()
            })
    }

    fn add_carriage_return<'a, F>(&mut self, attribute_callback: &F)
    where
        F: Fn(Highlight) -> &'a [u8],
    {
        if let Some(highlight) = self.carriage_return_highlight {
            let attribute_string = (attribute_callback)(highlight);
            if !attribute_string.is_empty() {
                self.html.extend(b"<span ");
                self.html.extend(attribute_string);
                self.html.extend(b"></span>");
            }
        }
    }

    fn start_highlight<'a, F>(&mut self, h: Highlight, attribute_callback: &F)
    where
        F: Fn(Highlight) -> &'a [u8],
    {
        let attribute_string = (attribute_callback)(h);
        self.html.extend(b"<span");
        if !attribute_string.is_empty() {
            self.html.extend(b" ");
            self.html.extend(attribute_string);
        }
        self.html.extend(b">");
    }

    fn end_highlight(&mut self) {
        self.html.extend(b"</span>");
    }

    fn add_text<'a, F>(
        &mut self,
        src: &[u8],
        highlights: &[Highlight],
        attribute_callback: &F,
    ) where
        F: Fn(Highlight) -> &'a [u8],
    {
        let mut last_char_was_cr = false;
        for c in LossyUtf8::new(src).flat_map(|p| p.bytes()) {
            // Don't render carriage return characters, but allow lone carriage returns (not
            // followed by line feeds) to be styled via the attribute callback.
            if c == b'\r' {
                last_char_was_cr = true;
                continue;
            }
            if last_char_was_cr {
                if c != b'\n' {
                    self.add_carriage_return(attribute_callback);
                }
                last_char_was_cr = false;
            }

            // At line boundaries, close and re-open all of the open tags.
            if c == b'\n' {
                highlights.iter().for_each(|_| self.end_highlight());
                self.html.push(c);
                self.line_offsets.push(self.html.len() as u32);
                highlights.iter().for_each(|scope| {
                    self.start_highlight(*scope, attribute_callback)
                });
            } else if let Some(escape) = html_escape(c) {
                self.html.extend_from_slice(escape);
            } else {
                self.html.push(c);
            }
        }
    }
}

impl Default for HtmlRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn injection_for_match<'a>(
    config: &HighlightConfiguration,
    query: &'a Query,
    query_match: &QueryMatch<'a, 'a>,
    source: &'a [u8],
) -> (Option<&'a str>, Option<Node<'a>>, bool) {
    let content_capture_index = config.injection_content_capture_index;
    let language_capture_index = config.injection_language_capture_index;

    let mut language_name = None;
    let mut content_node = None;
    for capture in query_match.captures {
        let index = Some(capture.index);
        if index == language_capture_index {
            language_name = capture.node.utf8_text(source).ok();
        } else if index == content_capture_index {
            content_node = Some(capture.node);
        }
    }

    let mut include_children = false;
    for prop in query.property_settings(query_match.pattern_index) {
        match prop.key.as_ref() {
            // In addition to specifying the language name via the text of a
            // captured node, it can also be hard-coded via a `#set!` predicate
            // that sets the injection.language key.
            "injection.language" => {
                if language_name.is_none() {
                    language_name = prop.value.as_ref().map(|s| s.as_ref())
                }
            }

            // By default, injections do not include the *children* of an
            // `injection.content` node - only the ranges that belong to the
            // node itself. This can be changed using a `#set!` predicate that
            // sets the `injection.include-children` key.
            "injection.include-children" => include_children = true,
            _ => {}
        }
    }

    (language_name, content_node, include_children)
}

fn shrink_and_clear<T>(vec: &mut Vec<T>, capacity: usize) {
    if vec.len() > capacity {
        vec.truncate(capacity);
        vec.shrink_to_fit();
    }
    vec.clear();
}

fn html_escape(c: u8) -> Option<&'static [u8]> {
    match c as char {
        '>' => Some(b"&gt;"),
        '<' => Some(b"&lt;"),
        '&' => Some(b"&amp;"),
        '\'' => Some(b"&#39;"),
        '"' => Some(b"&quot;"),
        _ => None,
    }
}
