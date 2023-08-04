use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{atomic, Arc},
};

use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    ext_event::create_ext_action,
    reactive::{ReadSignal, RwSignal, Scope},
};
use itertools::Itertools;
use lapce_core::{
    buffer::{
        diff::{rope_diff, DiffLines},
        rope_text::RopeText,
        Buffer, InvalLines,
    },
    command::EditCommand,
    cursor::Cursor,
    editor::{EditType, Editor},
    language::LapceLanguage,
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
    editor::view_data::{LineExtraStyle, TextLayoutCache, TextLayoutLine},
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
pub struct DocHistory {
    pub path: PathBuf,
    pub version: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DocContent {
    /// A file at some location. This can be a remote path.
    File(PathBuf),
    /// A local document, which doens't need to be sync to the disk.
    Local,
    /// A document of an old version in the source control
    History(DocHistory),
}

impl DocContent {
    pub fn is_local(&self) -> bool {
        matches!(self, DocContent::Local)
    }

    pub fn is_file(&self) -> bool {
        matches!(self, DocContent::File(_))
    }

    pub fn read_only(&self) -> bool {
        match self {
            DocContent::File(_) => false,
            DocContent::Local => false,
            DocContent::History(_) => true,
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            DocContent::File(path) => Some(path),
            DocContent::Local => None,
            DocContent::History(_) => None,
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

/// A single document that can be viewed by multiple [`EditorData`]'s
/// [`EditorViewData`]s and [`EditorView]s.  
#[derive(Clone)]
pub struct Document {
    pub scope: Scope,
    pub content: DocContent,
    pub buffer_id: BufferId,
    cache_rev: u64,
    buffer: Buffer,
    syntax: Syntax,
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
    /// The text layouts for the document. This may be shared with other views.
    text_layouts: Rc<RefCell<TextLayoutCache>>,

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
            cache_rev: 0,
            syntax,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics,
            completion_lens: None,
            completion_pos: (0, 0),
            content: DocContent::File(path),
            loaded: false,
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
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
            cache_rev: 0,
            content: DocContent::Local,
            syntax: Syntax::plaintext(),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
            },
            completion_lens: None,
            completion_pos: (0, 0),
            loaded: true,
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            code_actions: im::HashMap::new(),
            proxy,
            config,
            find,
            find_result: FindResult::new(cx),
        }
    }

    pub fn new_hisotry(
        cx: Scope,
        content: DocContent,
        find: Find,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let syntax = if let DocContent::History(history) = &content {
            Syntax::init(&history.path)
        } else {
            Syntax::plaintext()
        };
        let cx = cx.create_child();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: Buffer::new(""),
            cache_rev: 0,
            content,
            syntax,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
            },
            completion_lens: None,
            completion_pos: (0, 0),
            loaded: true,
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
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

    pub fn cache_rev(&self) -> u64 {
        self.cache_rev
    }

    pub fn syntax(&self) -> &Syntax {
        &self.syntax
    }

    pub fn set_syntax(&mut self, syntax: Syntax) {
        self.syntax = syntax;
        if self.semantic_styles.is_none() {
            self.clear_style_cache();
        }
        self.clear_sticky_headers_cache();
    }

    /// Set the syntax highlighting this document should use.
    pub fn set_language(&mut self, language: LapceLanguage) {
        self.syntax = Syntax::from_language(language);
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
        self.buffer.detect_indent(&self.syntax);
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
        if self.content.read_only() {
            return Vec::new();
        }

        let old_cursor = cursor.mode.clone();
        let deltas = Editor::insert(
            cursor,
            &mut self.buffer,
            s,
            &self.syntax,
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
        if self.content.read_only() && !cmd.not_changing_buffer() {
            return Vec::new();
        }

        let mut clipboard = SystemClipboard {};
        let old_cursor = cursor.mode.clone();
        let deltas = Editor::do_edit(
            cursor,
            &mut self.buffer,
            cmd,
            &self.syntax,
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
        if let Some(styles) = self.syntax.styles.as_mut() {
            Arc::make_mut(styles).apply_shape(delta);
        }

        self.syntax.lens.apply_delta(delta);
    }

    /// Update the inlay hints so their positions are correct after an edit.
    fn update_inlay_hints(&mut self, delta: &RopeDelta) {
        if let Some(hints) = self.inlay_hints.as_mut() {
            hints.apply_shape(delta);
        }
    }

    pub fn trigger_syntax_change(
        &mut self,
        edits: Option<SmallVec<[SyntaxEdit; 3]>>,
    ) {
        let rev = self.buffer.rev();
        let text = self.buffer.text().clone();

        self.syntax.parse(rev, text, edits.as_deref());
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
        self.cache_rev += 1;
        self.text_layouts.borrow_mut().clear(self.cache_rev);
    }

    fn clear_sticky_headers_cache(&mut self) {
        self.sticky_headers.borrow_mut().clear();
    }

    /// Get the active style information, either the semantic styles or the
    /// tree-sitter syntax styles.
    fn styles(&self) -> Option<&Arc<Spans<Style>>> {
        if let Some(semantic_styles) = self.semantic_styles.as_ref() {
            Some(semantic_styles)
        } else {
            self.syntax.styles.as_ref()
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

    pub fn tigger_proxy_update(doc: RwSignal<Document>, proxy: &ProxyRpcHandler) {
        Self::get_inlay_hints(doc, proxy);
        Self::get_semantic_styles(doc, proxy);
    }

    /// Request semantic styles for the buffer from the LSP through the proxy.
    fn get_semantic_styles(doc: RwSignal<Document>, proxy: &ProxyRpcHandler) {
        if !doc.with_untracked(|doc| doc.loaded) {
            return;
        }

        let path = match doc.with_untracked(|doc| doc.content.clone()) {
            DocContent::File(path) => path,
            DocContent::Local => return,
            DocContent::History(_) => return,
        };

        let (rev, len, cx) = doc
            .with_untracked(|doc| (doc.buffer.rev(), doc.buffer.len(), doc.scope));

        let syntactic_styles = doc.with_untracked(|doc| doc.syntax.styles.clone());

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
    fn get_inlay_hints(doc: RwSignal<Document>, proxy: &ProxyRpcHandler) {
        if !doc.with_untracked(|doc| doc.loaded) {
            return;
        }

        let path = match doc.with_untracked(|doc| doc.content.clone()) {
            DocContent::File(path) => path,
            DocContent::Local => return,
            DocContent::History(_) => return,
        };

        let (buffer, rev, len, cx) = doc.with_untracked(|doc| {
            (
                doc.buffer.clone(),
                doc.buffer.rev(),
                doc.buffer.len(),
                doc.scope,
            )
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
        let lines = self.syntax.sticky_headers(offset).map(|offsets| {
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

    /// Create rendable whitespace layout by creating a new text layout
    /// with invisible spaces and special utf8 characters that display
    /// the different white space characters.
    fn new_whitespace_layout(
        line_content: &str,
        text_layout: &TextLayout,
        phantom: &PhantomTextLine,
        config: &LapceConfig,
    ) -> Option<Vec<(char, (f64, f64))>> {
        let mut render_leading = false;
        let mut render_boundary = false;
        let mut render_between = false;

        // TODO: render whitespaces only on highlighted text
        match config.editor.render_whitespace.as_str() {
            "all" => {
                render_leading = true;
                render_boundary = true;
                render_between = true;
            }
            "boundary" => {
                render_leading = true;
                render_boundary = true;
            }
            "trailing" => {} // All configs include rendering trailing whitespace
            _ => return None,
        }

        let mut whitespace_buffer = Vec::new();
        let mut rendered_whitespaces: Vec<(char, (f64, f64))> = Vec::new();
        let mut char_found = false;
        let mut col = 0;
        for c in line_content.chars() {
            match c {
                '\t' => {
                    let col_left = phantom.col_after(col, true);
                    let col_right = phantom.col_after(col + 1, false);
                    let x0 = text_layout.hit_position(col_left).point.x;
                    let x1 = text_layout.hit_position(col_right).point.x;
                    whitespace_buffer.push(('\t', (x0, x1)));
                }
                ' ' => {
                    let col_left = phantom.col_after(col, true);
                    let col_right = phantom.col_after(col + 1, false);
                    let x0 = text_layout.hit_position(col_left).point.x;
                    let x1 = text_layout.hit_position(col_right).point.x;
                    whitespace_buffer.push((' ', (x0, x1)));
                }
                _ => {
                    if (char_found && render_between)
                        || (char_found
                            && render_boundary
                            && whitespace_buffer.len() > 1)
                        || (!char_found && render_leading)
                    {
                        rendered_whitespaces.extend(whitespace_buffer.iter());
                    } else {
                    }

                    char_found = true;
                    whitespace_buffer.clear();
                }
            }
            col += c.len_utf8();
        }
        rendered_whitespaces.extend(whitespace_buffer.iter());

        Some(rendered_whitespaces)
    }

    /// Create a new text layout for the given line.  
    /// Typically you should use [`Document::get_text_layout`] instead.
    fn new_text_layout(&self, line: usize, _font_size: usize) -> TextLayoutLine {
        let config = self.config.get_untracked();
        let line_content_original = self.buffer.line_content(line);

        // Get the line content with newline characters replaced with spaces
        // and the content without the newline characters
        let (line_content, line_content_original) =
            if let Some(s) = line_content_original.strip_suffix("\r\n") {
                (
                    format!("{s}  "),
                    &line_content_original[..line_content_original.len() - 2],
                )
            } else if let Some(s) = line_content_original.strip_suffix('\n') {
                (
                    format!("{s} ",),
                    &line_content_original[..line_content_original.len() - 1],
                )
            } else {
                (
                    line_content_original.to_string(),
                    &line_content_original[..],
                )
            };
        // Combine the phantom text with the line content
        let phantom_text = self.line_phantom_text(line);
        let line_content = phantom_text.combine_with_text(line_content);

        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .color(*color)
            .family(&family)
            .font_size(config.editor.font_size() as f32);
        let mut attrs_list = AttrsList::new(attrs);

        // Apply various styles to the line's text based on our semantic/syntax highlighting
        let styles = self.line_style(line);
        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.get_style_color(fg_color) {
                    let start = phantom_text.col_at(line_style.start);
                    let end = phantom_text.col_at(line_style.end);
                    attrs_list.add_span(start..end, attrs.color(*fg_color));
                }
            }
        }

        let font_size = config.editor.font_size();

        // Apply phantom text specific styling
        for (offset, size, col, phantom) in phantom_text.offset_size_iter() {
            let start = col + offset;
            let end = start + size;

            let mut attrs = attrs;
            if let Some(fg) = phantom.fg {
                attrs = attrs.color(fg);
            }
            if let Some(phantom_font_size) = phantom.font_size {
                attrs = attrs.font_size(phantom_font_size.min(font_size) as f32);
            }
            attrs_list.add_span(start..end, attrs);
            // if let Some(font_family) = phantom.font_family.clone() {
            //     layout_builder = layout_builder.range_attribute(
            //         start..end,
            //         TextAttribute::FontFamily(font_family),
            //     );
            // }
        }

        let mut text_layout = TextLayout::new();
        text_layout.set_text(&line_content, attrs_list);

        // Keep track of background styling from phantom text, which is done separately
        // from the text layout attributes
        let mut extra_style = Vec::new();
        for (offset, size, col, phantom) in phantom_text.offset_size_iter() {
            if phantom.bg.is_some() || phantom.under_line.is_some() {
                let start = col + offset;
                let end = start + size;
                let x0 = text_layout.hit_position(start).point.x;
                let x1 = text_layout.hit_position(end).point.x;
                extra_style.push(LineExtraStyle {
                    x: x0,
                    width: Some(x1 - x0),
                    bg_color: phantom.bg,
                    under_line: phantom.under_line,
                    wave_line: None,
                });
            }
        }

        // Add the styling for the diagnostic severity, if applicable
        if let Some(max_severity) = phantom_text.max_severity {
            let theme_prop = if max_severity == DiagnosticSeverity::ERROR {
                LapceColor::ERROR_LENS_ERROR_BACKGROUND
            } else if max_severity == DiagnosticSeverity::WARNING {
                LapceColor::ERROR_LENS_WARNING_BACKGROUND
            } else {
                LapceColor::ERROR_LENS_OTHER_BACKGROUND
            };

            let x1 = (!config.editor.error_lens_end_of_line)
                .then(|| text_layout.hit_position(line_content.len()).point.x);

            extra_style.push(LineExtraStyle {
                x: 0.0,
                width: x1,
                bg_color: Some(*config.get_color(theme_prop)),
                under_line: None,
                wave_line: None,
            });
        }

        self.diagnostics.diagnostics.with_untracked(|diags| {
            for diag in diags {
                if diag.diagnostic.range.start.line as usize <= line
                    && line <= diag.diagnostic.range.end.line as usize
                {
                    let start = if diag.diagnostic.range.start.line as usize == line
                    {
                        let (_, col) = self.buffer.offset_to_line_col(diag.range.0);
                        col
                    } else {
                        let offset =
                            self.buffer.first_non_blank_character_on_line(line);
                        let (_, col) = self.buffer.offset_to_line_col(offset);
                        col
                    };
                    let start = phantom_text.col_after(start, true);

                    let end = if diag.diagnostic.range.end.line as usize == line {
                        let (_, col) = self.buffer.offset_to_line_col(diag.range.1);
                        col
                    } else {
                        self.buffer.line_end_col(line, true)
                    };
                    let end = phantom_text.col_after(end, false);

                    let x0 = text_layout.hit_position(start).point.x;
                    let x1 = text_layout.hit_position(end).point.x;
                    let color_name = match diag.diagnostic.severity {
                        Some(DiagnosticSeverity::ERROR) => LapceColor::LAPCE_ERROR,
                        _ => LapceColor::LAPCE_WARN,
                    };
                    let color = *config.get_color(color_name);
                    extra_style.push(LineExtraStyle {
                        x: x0,
                        width: Some(x1 - x0),
                        bg_color: None,
                        under_line: None,
                        wave_line: Some(color),
                    });
                }
            }
        });

        let whitespaces = Self::new_whitespace_layout(
            line_content_original,
            &text_layout,
            &phantom_text,
            &config,
        );

        let indent_line = if line_content_original.trim().is_empty() {
            let offset = self.buffer.offset_of_line(line);
            if let Some(offset) = self.syntax.parent_offset(offset) {
                self.buffer.line_of_offset(offset)
            } else {
                line
            }
        } else {
            line
        };

        let indent = if indent_line != line {
            self.get_text_layout(indent_line, font_size).indent + 1.0
        } else {
            let offset = self.buffer.first_non_blank_character_on_line(indent_line);
            let (_, col) = self.buffer.offset_to_line_col(offset);
            text_layout.hit_position(col).point.x
        };

        TextLayoutLine {
            text: text_layout,
            extra_style,
            whitespaces,
            indent,
        }
    }

    /// Get the text layout for the given line.  
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout(
        &self,
        line: usize,
        font_size: usize,
    ) -> Arc<TextLayoutLine> {
        let config = self.config.get_untracked();
        // Check if the text layout needs to update due to the config being changed
        self.text_layouts.borrow_mut().check_attributes(config.id);
        // If we don't have a second layer of the hashmap initialized for this specific font size,
        // do it now
        if self.text_layouts.borrow().layouts.get(&font_size).is_none() {
            let mut cache = self.text_layouts.borrow_mut();
            cache.layouts.insert(font_size, HashMap::new());
        }

        // Get whether there's an entry for this specific font size and line
        let cache_exists = self
            .text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .is_some();
        // If there isn't an entry then we actually have to create it
        if !cache_exists {
            let text_layout = Arc::new(self.new_text_layout(line, font_size));
            let mut cache = self.text_layouts.borrow_mut();
            let width = text_layout.text.size().width;
            if width > cache.max_width {
                cache.max_width = width;
            }
            cache
                .layouts
                .get_mut(&font_size)
                .unwrap()
                .insert(line, text_layout);
        }

        // Just get the entry, assuming it has been created because we initialize it above.
        self.text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .cloned()
            .unwrap()
    }
}
