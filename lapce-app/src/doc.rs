use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{atomic, Arc},
};

use floem::{
    ext_event::create_ext_action,
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, Scope, SignalGetUntracked,
        SignalSet, SignalUpdate, SignalWithUntracked,
    },
};
use itertools::Itertools;
use lapce_core::{
    buffer::{rope_diff, rope_text::RopeText, Buffer, DiffLines, InvalLines},
    command::EditCommand,
    cursor::Cursor,
    editor::{EditType, Editor},
    register::{Clipboard, Register},
    selection::{SelRegion, Selection},
    style::line_styles,
    syntax::{edit::SyntaxEdit, Syntax},
};
use lapce_rpc::{
    buffer::BufferId,
    plugin::PluginId,
    proxy::{ProxyResponse, ProxyRpcHandler},
    style::{LineStyle, LineStyles, Style},
};
use lapce_xi_rope::{
    spans::{Spans, SpansBuilder},
    Interval, Rope, RopeDelta, Transformer,
};
use lsp_types::{
    CodeActionResponse, Diagnostic, DiagnosticSeverity, InlayHint, InlayHintLabel,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use self::phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine};
use crate::{
    config::{color::LapceColor, LapceConfig},
    find::{Find, FindProgress, FindResult},
    history::DocumentHistory,
    workspace::LapceWorkspace,
};

pub mod phantom_text;

pub struct SystemClipboard {}

impl SystemClipboard {
    fn clipboard() -> floem::glazier::Clipboard {
        floem::glazier::Application::global().clipboard()
    }
}

impl Clipboard for SystemClipboard {
    fn get_string(&self) -> Option<String> {
        Self::clipboard().get_string()
    }

    fn put_string(&mut self, s: impl AsRef<str>) {
        Self::clipboard().put_string(s)
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticData {
    pub expanded: RwSignal<bool>,
    pub diagnostics: RwSignal<im::Vector<EditorDiagnostic>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorDiagnostic {
    pub range: (usize, usize),
    pub diagnostic: Diagnostic,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DocContent {
    /// A file at some location. This can be a remote path.
    File(PathBuf),
    /// A local document, which doens't need to be sync to the disk.
    Local,
}

impl DocContent {
    pub fn is_local(&self) -> bool {
        matches!(self, DocContent::Local)
    }

    pub fn is_file(&self) -> bool {
        matches!(self, DocContent::File(_))
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            DocContent::File(path) => Some(path),
            DocContent::Local => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocInfo {
    pub workspace: LapceWorkspace,
    pub path: PathBuf,
    pub scroll_offset: (f64, f64),
    pub cursor_offset: usize,
}

/// A trait for listening to when the text cache should be cleared, such as when the document is
/// changed.
pub trait TextCacheListener {
    fn clear(&self);
}

type TextCacheListeners = Rc<RefCell<SmallVec<[Rc<dyn TextCacheListener>; 2]>>>;

/// A single document that can be viewed by multiple [`EditorData`]'s
/// [`EditorViewData`]s and [`EditorView]s.  
#[derive(Clone)]
pub struct Document {
    pub scope: Scope,
    pub content: DocContent,
    pub buffer_id: BufferId,
    style_rev: u64,
    // TODO(minor): Perhaps use dyn-clone to avoid the need for Rc?
    /// The text cache listeners, which are told to clear cached text when the document is changed.
    text_cache_listeners: TextCacheListeners,
    buffer: Buffer,
    syntax: Option<Syntax>,
    line_styles: Rc<RefCell<LineStyles>>,
    /// Semantic highlighting information (which is provided by the LSP)
    semantic_styles: Option<Arc<Spans<Style>>>,
    /// Inlay hints for the document
    pub inlay_hints: Option<Spans<InlayHint>>,
    /// The diagnostics for the document
    pub diagnostics: DiagnosticData,
    /// Current completion lens text, if any.  
    /// This will be displayed even on views that are not focused.
    completion_lens: Option<String>,
    /// (line, col)
    completion_pos: (usize, usize),
    /// (Offset -> (Plugin the code actions are from, Code Actions))
    pub code_actions: im::HashMap<usize, Arc<(PluginId, CodeActionResponse)>>,
    /// Whether the buffer's content has been loaded/initialized into the buffer.
    loaded: bool,
    /// Stores information about different versions of the document from source control.
    histories: RwSignal<im::HashMap<String, DocumentHistory>>,
    pub head_changes: RwSignal<im::Vector<DiffLines>>,

    /// A cache for the sticky headers which maps a line to the lines it should show in the header.
    pub sticky_headers: Rc<RefCell<HashMap<usize, Option<Vec<usize>>>>>,
    proxy: ProxyRpcHandler,
    config: ReadSignal<Arc<LapceConfig>>,
    find: Find,
    pub find_result: FindResult,
}

impl Document {
    pub fn new(
        cx: Scope,
        path: PathBuf,
        diagnostics: DiagnosticData,
        find: Find,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let syntax = Syntax::init(&path);
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: Buffer::new(""),
            style_rev: 0,
            text_cache_listeners: Rc::new(RefCell::new(SmallVec::new())),
            syntax: syntax.ok(),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics,
            completion_lens: None,
            completion_pos: (0, 0),
            content: DocContent::File(path),
            loaded: false,
            histories: create_rw_signal(cx, im::HashMap::new()),
            head_changes: create_rw_signal(cx, im::Vector::new()),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            code_actions: im::HashMap::new(),
            proxy,
            config,
            find,
            find_result: FindResult::new(cx),
        }
    }

    pub fn new_local(
        cx: Scope,
        find: Find,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: Buffer::new(""),
            style_rev: 0,
            text_cache_listeners: Rc::new(RefCell::new(SmallVec::new())),
            content: DocContent::Local,
            syntax: None,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics: DiagnosticData {
                expanded: create_rw_signal(cx, true),
                diagnostics: create_rw_signal(cx, im::Vector::new()),
            },
            completion_lens: None,
            completion_pos: (0, 0),
            loaded: true,
            histories: create_rw_signal(cx, im::HashMap::new()),
            head_changes: create_rw_signal(cx, im::Vector::new()),
            code_actions: im::HashMap::new(),
            proxy,
            config,
            find,
            find_result: FindResult::new(cx),
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn style_rev(&self) -> u64 {
        self.style_rev
    }

    pub fn syntax(&self) -> Option<&Syntax> {
        self.syntax.as_ref()
    }

    pub fn find(&self) -> &Find {
        &self.find
    }

    /// Whether or not the underlying buffer is loaded
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    //// Initialize the content with some text, this marks the document as loaded.
    pub fn init_content(&mut self, content: Rope) {
        self.buffer.init_content(content);
        self.buffer.detect_indent(self.syntax.as_ref());
        self.loaded = true;
        self.on_update(None);
        self.init_diagnostics();
        self.retrieve_head();
    }

    /// Reload the document's content, and is what you should typically use when you want to *set*
    /// an existing document's content.
    pub fn reload(&mut self, content: Rope, set_pristine: bool) {
        // self.code_actions.clear();
        // self.inlay_hints = None;
        let delta = self.buffer.reload(content, set_pristine);
        self.apply_deltas(&[delta]);
    }

    pub fn handle_file_changed(&mut self, content: Rope) {
        if self.buffer.is_pristine() {
            self.reload(content, true);
        }
    }

    pub fn do_insert(
        &mut self,
        cursor: &mut Cursor,
        s: &str,
        config: &LapceConfig,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        let old_cursor = cursor.mode.clone();
        let deltas = Editor::insert(
            cursor,
            &mut self.buffer,
            s,
            self.syntax.as_ref(),
            config.editor.auto_closing_matching_pairs,
        );
        // Keep track of the change in the cursor mode for undo/redo
        self.buffer.set_cursor_before(old_cursor);
        self.buffer.set_cursor_after(cursor.mode.clone());
        self.apply_deltas(&deltas);
        deltas
    }

    pub fn do_raw_edit(
        &mut self,
        edits: &[(impl AsRef<Selection>, &str)],
        edit_type: EditType,
    ) -> (RopeDelta, InvalLines, SyntaxEdit) {
        let (delta, inval_lines, edits) = self.buffer.edit(edits, edit_type);
        self.apply_deltas(&[(delta.clone(), inval_lines.clone(), edits.clone())]);
        (delta, inval_lines, edits)
    }

    pub fn do_edit(
        &mut self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        let mut clipboard = SystemClipboard {};
        let old_cursor = cursor.mode.clone();
        let deltas = Editor::do_edit(
            cursor,
            &mut self.buffer,
            cmd,
            self.syntax.as_ref(),
            &mut clipboard,
            modal,
            register,
        );

        if !deltas.is_empty() {
            self.buffer.set_cursor_before(old_cursor);
            self.buffer.set_cursor_after(cursor.mode.clone());
        }

        self.apply_deltas(&deltas);
        deltas
    }

    pub fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        let rev = self.rev() - deltas.len() as u64;
        for (i, (delta, _, _)) in deltas.iter().enumerate() {
            self.update_styles(delta);
            self.update_inlay_hints(delta);
            self.update_diagnostics(delta);
            self.update_completion_lens(delta);
            if let DocContent::File(path) = &self.content {
                self.proxy
                    .update(path.clone(), delta.clone(), rev + i as u64 + 1);
            }
        }

        // TODO(minor): We could avoid this potential allocation since most apply_delta callers are actually using a Vec
        // which we could reuse.
        // We use a smallvec because there is unlikely to be more than a couple of deltas
        let edits = deltas.iter().map(|(_, _, edits)| edits.clone()).collect();
        self.on_update(Some(edits));
    }

    /// Get the buffer's current revision. This is used to track whether the buffer has changed.
    pub fn rev(&self) -> u64 {
        self.buffer.rev()
    }

    fn on_update(&mut self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        self.clear_code_actions();
        // self.find.borrow_mut().unset();
        // *self.find_progress.borrow_mut() = FindProgress::Started;
        // self.get_inlay_hints();
        self.clear_style_cache();
        self.trigger_syntax_change(edits);
        self.clear_sticky_headers_cache();
        // self.find_result.reset();
        // self.clear_sticky_headers_cache();
        self.trigger_head_change();
        // self.notify_special();
    }

    /// Update the styles after an edit, so the highlights are at the correct positions.  
    /// This does not do a reparse of the document itself.
    fn update_styles(&mut self, delta: &RopeDelta) {
        if let Some(styles) = self.semantic_styles.as_mut() {
            Arc::make_mut(styles).apply_shape(delta);
        }
        if let Some(syntax) = self.syntax.as_mut() {
            if let Some(styles) = syntax.styles.as_mut() {
                Arc::make_mut(styles).apply_shape(delta);
            }
        }

        if let Some(syntax) = self.syntax.as_mut() {
            syntax.lens.apply_delta(delta);
        }
    }

    /// Update the inlay hints so their positions are correct after an edit.
    fn update_inlay_hints(&mut self, delta: &RopeDelta) {
        if let Some(hints) = self.inlay_hints.as_mut() {
            hints.apply_shape(delta);
        }
    }

    fn trigger_syntax_change(&mut self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        let Some(syntax) = self.syntax.as_mut() else { return };

        let rev = self.buffer.rev();
        let text = self.buffer.text().clone();

        syntax.parse(rev, text, edits.as_deref());
    }

    fn clear_style_cache(&mut self) {
        self.line_styles.borrow_mut().clear();
        self.clear_text_cache();
    }

    fn clear_code_actions(&mut self) {
        self.code_actions.clear();
    }

    /// Inform any dependents on this document that they should clear any cached text.
    pub fn clear_text_cache(&mut self) {
        let mut text_cache_listeners = self.text_cache_listeners.borrow_mut();
        for entry in text_cache_listeners.iter_mut() {
            entry.clear();
        }

        self.style_rev += 1;
    }

    fn clear_sticky_headers_cache(&mut self) {
        self.sticky_headers.borrow_mut().clear();
    }

    /// Add a text cache listener which will be informed when the text cache should be cleared.
    pub fn add_text_cache_listener(&self, listener: Rc<dyn TextCacheListener>) {
        self.text_cache_listeners.borrow_mut().push(listener);
    }

    /// Remove any text cache listeners which only have our weak reference left.
    pub fn clean_text_cache_listeners(&self) {
        self.text_cache_listeners
            .borrow_mut()
            .retain(|entry| Rc::strong_count(entry) > 1);
    }

    /// Get the active style information, either the semantic styles or the
    /// tree-sitter syntax styles.
    fn styles(&self) -> Option<&Arc<Spans<Style>>> {
        if let Some(semantic_styles) = self.semantic_styles.as_ref() {
            Some(semantic_styles)
        } else {
            self.syntax.as_ref().and_then(|s| s.styles.as_ref())
        }
    }

    /// Get the style information for the particular line from semantic/syntax highlighting.  
    /// This caches the result if possible.
    pub fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        if self.line_styles.borrow().get(&line).is_none() {
            let styles = self.styles();

            let line_styles = styles
                .map(|styles| line_styles(self.buffer.text(), line, styles))
                .unwrap_or_default();
            self.line_styles
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        self.line_styles.borrow().get(&line).cloned().unwrap()
    }

    pub fn tigger_proxy_update(
        cx: Scope,
        doc: RwSignal<Document>,
        proxy: &ProxyRpcHandler,
    ) {
        Self::get_inlay_hints(cx, doc, proxy);
        Self::get_semantic_styles(cx, doc, proxy);
    }

    /// Request semantic styles for the buffer from the LSP through the proxy.
    fn get_semantic_styles(
        cx: Scope,
        doc: RwSignal<Document>,
        proxy: &ProxyRpcHandler,
    ) {
        if !doc.with_untracked(|doc| doc.loaded) {
            return;
        }

        let path = match doc.with_untracked(|doc| doc.content.clone()) {
            DocContent::File(path) => path,
            DocContent::Local => return,
        };

        let (rev, len) =
            doc.with_untracked(|doc| (doc.buffer.rev(), doc.buffer.len()));

        let syntactic_styles = doc.with_untracked(|doc| {
            doc.syntax.as_ref().and_then(|s| s.styles.as_ref()).cloned()
        });

        let send = create_ext_action(cx, move |styles| {
            doc.update(|doc| {
                if doc.buffer.rev() == rev {
                    doc.semantic_styles = Some(styles);
                    doc.clear_style_cache();
                }
            })
        });

        proxy.get_semantic_tokens(path, move |result| {
            if let Ok(ProxyResponse::GetSemanticTokens { styles }) = result {
                rayon::spawn(move || {
                    let mut styles_span = SpansBuilder::new(len);
                    for style in styles.styles {
                        styles_span.add_span(
                            Interval::new(style.start, style.end),
                            style.style,
                        );
                    }

                    let styles = styles_span.build();

                    let styles = if let Some(syntactic_styles) = syntactic_styles {
                        syntactic_styles.merge(&styles, |a, b| {
                            if let Some(b) = b {
                                return b.clone();
                            }
                            a.clone()
                        })
                    } else {
                        styles
                    };
                    let styles = Arc::new(styles);

                    send(styles);
                });
            }
        });
    }

    /// Request inlay hints for the buffer from the LSP through the proxy.
    fn get_inlay_hints(cx: Scope, doc: RwSignal<Document>, proxy: &ProxyRpcHandler) {
        if !doc.with_untracked(|doc| doc.loaded) {
            return;
        }

        let path = match doc.with_untracked(|doc| doc.content.clone()) {
            DocContent::File(path) => path,
            DocContent::Local => return,
        };

        let (buffer, rev, len) = doc.with_untracked(|doc| {
            (doc.buffer.clone(), doc.buffer.rev(), doc.buffer.len())
        });

        let send = create_ext_action(cx, move |hints| {
            doc.update(|doc| {
                if doc.buffer.rev() == rev {
                    doc.inlay_hints = Some(hints);
                    doc.clear_text_cache();
                }
            })
        });

        proxy.get_inlay_hints(path, move |result| {
            if let Ok(ProxyResponse::GetInlayHints { mut hints }) = result {
                // Sort the inlay hints by their position, as the LSP does not guarantee that it will
                // provide them in the order that they are in within the file
                // as well, Spans does not iterate in the order that they appear
                hints.sort_by(|left, right| left.position.cmp(&right.position));

                let mut hints_span = SpansBuilder::new(len);
                for hint in hints {
                    let offset = buffer.offset_of_position(&hint.position).min(len);
                    hints_span.add_span(
                        Interval::new(offset, (offset + 1).min(len)),
                        hint,
                    );
                }
                let hints = hints_span.build();
                send(hints);
            }
        });
    }

    /// Get the phantom text for a given line
    pub fn line_phantom_text(&self, line: usize) -> PhantomTextLine {
        let config = self.config.get_untracked();

        let start_offset = self.buffer.offset_of_line(line);
        let end_offset = self.buffer.offset_of_line(line + 1);

        // If hints are enabled, and the hints field is filled, then get the hints for this line
        // and convert them into PhantomText instances
        let hints = config
            .editor
            .enable_inlay_hints
            .then_some(())
            .and(self.inlay_hints.as_ref())
            .map(|hints| hints.iter_chunks(start_offset..end_offset))
            .into_iter()
            .flatten()
            .filter(|(interval, _)| {
                interval.start >= start_offset && interval.start < end_offset
            })
            .map(|(interval, inlay_hint)| {
                let (_, col) = self.buffer.offset_to_line_col(interval.start);
                let text = match &inlay_hint.label {
                    InlayHintLabel::String(label) => label.to_string(),
                    InlayHintLabel::LabelParts(parts) => {
                        parts.iter().map(|p| &p.value).join("")
                    }
                };
                PhantomText {
                    kind: PhantomTextKind::InlayHint,
                    col,
                    text,
                    fg: Some(*config.get_color(LapceColor::INLAY_HINT_FOREGROUND)),
                    // font_family: Some(config.editor.inlay_hint_font_family()),
                    font_size: Some(config.editor.inlay_hint_font_size()),
                    bg: Some(*config.get_color(LapceColor::INLAY_HINT_BACKGROUND)),
                    under_line: None,
                }
            });
        // You're quite unlikely to have more than six hints on a single line
        // this later has the diagnostics added onto it, but that's still likely to be below six
        // overall.
        let mut text: SmallVec<[PhantomText; 6]> = hints.collect();

        // The max severity is used to determine the color given to the background of the line
        let mut max_severity = None;
        // If error lens is enabled, and the diagnostics field is filled, then get the diagnostics
        // that end on this line which have a severity worse than HINT and convert them into
        // PhantomText instances
        let diag_text = config
            .editor
            .enable_error_lens
            .then_some(())
            .map(|_| self.diagnostics.diagnostics.get_untracked())
            .into_iter()
            .flatten()
            .filter(|diag| {
                diag.diagnostic.range.end.line as usize == line
                    && diag.diagnostic.severity < Some(DiagnosticSeverity::HINT)
            })
            .map(|diag| {
                match (diag.diagnostic.severity, max_severity) {
                    (Some(severity), Some(max)) => {
                        if severity < max {
                            max_severity = Some(severity);
                        }
                    }
                    (Some(severity), None) => {
                        max_severity = Some(severity);
                    }
                    _ => {}
                }

                let col = self.buffer.offset_of_line(line + 1)
                    - self.buffer.offset_of_line(line);
                let fg = {
                    let severity = diag
                        .diagnostic
                        .severity
                        .unwrap_or(DiagnosticSeverity::WARNING);
                    let theme_prop = if severity == DiagnosticSeverity::ERROR {
                        LapceColor::ERROR_LENS_ERROR_FOREGROUND
                    } else if severity == DiagnosticSeverity::WARNING {
                        LapceColor::ERROR_LENS_WARNING_FOREGROUND
                    } else {
                        // information + hint (if we keep that) + things without a severity
                        LapceColor::ERROR_LENS_OTHER_FOREGROUND
                    };

                    *config.get_color(theme_prop)
                };
                let text =
                    format!("    {}", diag.diagnostic.message.lines().join(" "));
                PhantomText {
                    kind: PhantomTextKind::Diagnostic,
                    col,
                    text,
                    fg: Some(fg),
                    font_size: Some(config.editor.error_lens_font_size()),
                    // font_family: Some(config.editor.error_lens_font_family()),
                    bg: None,
                    under_line: None,
                }
            });
        let mut diag_text: SmallVec<[PhantomText; 6]> = diag_text.collect();

        text.append(&mut diag_text);

        let (completion_line, completion_col) = self.completion_pos;
        let completion_text = config
            .editor
            .enable_completion_lens
            .then_some(())
            .and(self.completion_lens.as_ref())
            // TODO: We're probably missing on various useful completion things to include here!
            .filter(|_| line == completion_line)
            .map(|completion| PhantomText {
                kind: PhantomTextKind::Completion,
                col: completion_col,
                text: completion.clone(),
                fg: Some(*config.get_color(LapceColor::COMPLETION_LENS_FOREGROUND)),
                font_size: Some(config.editor.completion_lens_font_size()),
                // font_family: Some(config.editor.completion_lens_font_family()),
                bg: None,
                under_line: None,
                // TODO: italics?
            });
        if let Some(completion_text) = completion_text {
            text.push(completion_text);
        }

        // if let Some(ime_text) = self.ime_text.as_ref() {
        //     let (ime_line, col, _) = self.ime_pos;
        //     if line == ime_line {
        //         text.push(PhantomText {
        //             kind: PhantomTextKind::Ime,
        //             text: ime_text.to_string(),
        //             col,
        //             font_size: None,
        //             font_family: None,
        //             fg: None,
        //             bg: None,
        //             under_line: Some(
        //                 config
        //                     .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
        //                     .clone(),
        //             ),
        //         });
        //     }
        // }

        text.sort_by(|a, b| {
            if a.col == b.col {
                a.kind.cmp(&b.kind)
            } else {
                a.col.cmp(&b.col)
            }
        });

        PhantomTextLine { text, max_severity }
    }

    /// Update the diagnostics' positions after an edit so that they appear in the correct place.
    fn update_diagnostics(&self, delta: &RopeDelta) {
        if self
            .diagnostics
            .diagnostics
            .with_untracked(|d| d.is_empty())
        {
            return;
        }
        self.diagnostics.diagnostics.update(|diagnostics| {
            for diagnostic in diagnostics.iter_mut() {
                let mut transformer = Transformer::new(delta);
                let (start, end) = diagnostic.range;
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );

                let new_start_pos = self.buffer().offset_to_position(new_start);

                let new_end_pos = self.buffer().offset_to_position(new_end);

                diagnostic.range = (new_start, new_end);

                diagnostic.diagnostic.range.start = new_start_pos;
                diagnostic.diagnostic.range.end = new_end_pos;
            }
        });
    }

    /// init diagnostics offset ranges from lsp positions
    pub fn init_diagnostics(&mut self) {
        self.clear_text_cache();
        self.clear_code_actions();
        self.diagnostics.diagnostics.update(|diagnostics| {
            for diagnostic in diagnostics.iter_mut() {
                let start = self
                    .buffer()
                    .offset_of_position(&diagnostic.diagnostic.range.start);
                let end = self
                    .buffer()
                    .offset_of_position(&diagnostic.diagnostic.range.end);
                diagnostic.range = (start, end);
            }
        });
    }

    /// Get the current completion lens text
    pub fn completion_lens(&self) -> Option<&str> {
        self.completion_lens.as_deref()
    }

    pub fn set_completion_lens(
        &mut self,
        completion_lens: String,
        line: usize,
        col: usize,
    ) {
        // TODO: more granular invalidation
        self.clear_text_cache();
        self.completion_lens = Some(completion_lens);
        self.completion_pos = (line, col);
    }

    pub fn clear_completion_lens(&mut self) {
        // TODO: more granular invalidation
        self.clear_text_cache();
        if self.completion_lens.is_some() {
            self.completion_lens = None;
        }
    }

    /// Update the completion lens position after an edit so that it appears in the correct place.
    pub fn update_completion_lens(&mut self, delta: &RopeDelta) {
        let Some(completion) = self.completion_lens.as_ref() else { return };

        let (line, col) = self.completion_pos;
        let offset = self.buffer().offset_of_line_col(line, col);

        // If the edit is easily checkable + updateable from, then we alter the lens' text.
        // In normal typing, if we didn't do this, then the text would jitter forward and then
        // backwards as the completion lens is updated.
        // TODO: this could also handle simple deletion, but we don't currently keep track of
        // the past copmletion lens string content in the field.
        if delta.as_simple_insert().is_some() {
            let (iv, new_len) = delta.summary();
            if iv.start() == iv.end()
                && iv.start() == offset
                && new_len <= completion.len()
            {
                // Remove the # of newly inserted characters
                // These aren't necessarily the same as the characters literally in the
                // text, but the completion will be updated when the completion widget
                // receives the update event, and it will fix this if needed.
                // TODO: this could be smarter and use the insert's content
                self.completion_lens = Some(completion[new_len..].to_string());
            }
        }

        // Shift the position by the rope delta
        let mut transformer = Transformer::new(delta);

        let new_offset = transformer.transform(offset, true);
        let new_pos = self.buffer().offset_to_line_col(new_offset);

        self.completion_pos = new_pos;
    }

    pub fn update_find(&self, start_line: usize, end_line: usize) {
        let search_string_same =
            self.find_result
                .search_string
                .with_untracked(|current_search_string| {
                    self.find.search_string.with_untracked(|search_string| {
                        match (current_search_string, search_string) {
                            (Some(old), Some(new)) => {
                                old.content == new.content
                                    && old.regex.is_some() == new.regex.is_some()
                            }
                            (None, None) => true,
                            (None, Some(_)) => false,
                            (Some(_), None) => false,
                        }
                    })
                });
        if !search_string_same {
            self.find_result
                .search_string
                .set(self.find.search_string.get_untracked());
        }
        let is_regex_same = self.find_result.is_regex.get_untracked()
            == self.find.is_regex.get_untracked();
        if !is_regex_same {
            self.find_result
                .is_regex
                .set(self.find.is_regex.get_untracked());
        }
        let case_matching_same = self.find_result.case_matching.get_untracked()
            == self.find.case_matching.get_untracked();
        if !case_matching_same {
            self.find_result
                .case_matching
                .set(self.find.case_matching.get_untracked());
        }
        let whole_words_same = self.find_result.whole_words.get_untracked()
            == self.find.whole_words.get_untracked();
        if !whole_words_same {
            self.find_result
                .whole_words
                .set(self.find.whole_words.get_untracked())
        }

        if !search_string_same
            || !case_matching_same
            || !whole_words_same
            || !is_regex_same
        {
            self.find_result.reset();
        }

        let mut find_progress = self.find_result.progress.get_untracked();
        let search_range = match &find_progress {
            FindProgress::Started => {
                // start incremental find on visible region
                let start = self.buffer.offset_of_line(start_line);
                let end = self.buffer.offset_of_line(end_line + 1);
                find_progress =
                    FindProgress::InProgress(Selection::region(start, end));
                Some((start, end))
            }
            FindProgress::InProgress(searched_range) => {
                if searched_range.regions().len() == 1
                    && searched_range.min_offset() == 0
                    && searched_range.max_offset() >= self.buffer.len()
                {
                    // the entire text has been searched
                    // end find by executing multi-line regex queries on entire text
                    // stop incremental find
                    find_progress = FindProgress::Ready;
                    Some((0, self.buffer.len()))
                } else {
                    let start = self.buffer.offset_of_line(start_line);
                    let end = self.buffer.offset_of_line(end_line + 1);
                    let mut range = Some((start, end));
                    for region in searched_range.regions() {
                        if region.min() <= start && region.max() >= end {
                            range = None;
                            break;
                        }
                    }
                    if range.is_some() {
                        let mut new_range = searched_range.clone();
                        new_range.add_region(SelRegion::new(start, end, None));
                        find_progress = FindProgress::InProgress(new_range);
                    }
                    range
                }
            }
            FindProgress::Ready => None,
        };
        if search_range.is_some() {
            self.find_result.progress.set(find_progress);
        }

        if let Some((search_range_start, search_range_end)) = search_range {
            let mut occurrences = self.find_result.occurrences.get_untracked();
            if !self.find.is_multiline_regex() {
                self.find.update_find(
                    self.buffer.text(),
                    search_range_start,
                    search_range_end,
                    true,
                    &mut occurrences,
                );
            } else {
                // only execute multi-line regex queries if we are searching the entire text (last step)
                if search_range_start == 0 && search_range_end == self.buffer.len() {
                    self.find.update_find(
                        self.buffer.text(),
                        search_range_start,
                        search_range_end,
                        true,
                        &mut occurrences,
                    );
                }
            }
            self.find_result.occurrences.set(occurrences);
        }
    }

    /// Get the sticky headers for a particular line, creating them if necessary.
    pub fn sticky_headers(&self, line: usize) -> Option<Vec<usize>> {
        if let Some(lines) = self.sticky_headers.borrow().get(&line) {
            return lines.clone();
        }
        let offset = self.buffer.offset_of_line(line + 1);
        let lines = self.syntax.as_ref()?.sticky_headers(offset).map(|offsets| {
            offsets
                .iter()
                .filter_map(|offset| {
                    let l = self.buffer.line_of_offset(*offset);
                    if l <= line {
                        Some(l)
                    } else {
                        None
                    }
                })
                .dedup()
                .sorted()
                .collect()
        });
        self.sticky_headers.borrow_mut().insert(line, lines.clone());
        lines
    }

    /// Retrieve the `head` version of the buffer
    pub fn retrieve_head(&self) {
        if let DocContent::File(path) = &self.content {
            let histories = self.histories;

            let send = {
                let path = path.clone();
                let doc = self.clone();
                create_ext_action(self.scope, move |result| {
                    if let Ok(ProxyResponse::BufferHeadResponse {
                        content, ..
                    }) = result
                    {
                        let hisotry = DocumentHistory::new(
                            path.clone(),
                            "head".to_string(),
                            &content,
                        );
                        histories.update(|histories| {
                            histories.insert("head".to_string(), hisotry);
                        });

                        doc.trigger_head_change();
                    }
                })
            };

            let path = path.clone();
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                proxy.get_buffer_head(path, move |result| {
                    send(result);
                });
            });
        }
    }

    pub fn trigger_head_change(&self) {
        let history = if let Some(text) =
            self.histories.with_untracked(|histories| {
                histories
                    .get("head")
                    .map(|history| history.buffer.text().clone())
            }) {
            text
        } else {
            return;
        };

        let atomic_rev = self.buffer().atomic_rev();
        let rev = self.rev();
        let left_rope = history;
        let right_rope = self.buffer().text().clone();

        let send = {
            let atomic_rev = atomic_rev.clone();
            let head_changes = self.head_changes;
            create_ext_action(self.scope, move |changes| {
                let changes = if let Some(changes) = changes {
                    changes
                } else {
                    return;
                };

                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    return;
                }

                head_changes.set(changes);
            })
        };

        rayon::spawn(move || {
            let changes =
                rope_diff(left_rope, right_rope, rev, atomic_rev.clone(), None);
            send(changes.map(im::Vector::from));
        });
    }
}
