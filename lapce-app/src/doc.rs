use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{atomic, Arc},
    time::Duration,
};

use floem::{
    action::exec_after,
    cosmic_text::{
        Attrs, AttrsList, FamilyOwned, LineHeightValue, Stretch, TextLayout, Weight,
    },
    ext_event::create_ext_action,
    peniko::Color,
    reactive::{batch, RwSignal, Scope},
};
use itertools::Itertools;
use lapce_core::{
    buffer::{
        diff::{rope_diff, DiffLines},
        rope_text::RopeText,
        Buffer, InvalLines,
    },
    char_buffer::CharBuffer,
    command::EditCommand,
    cursor::Cursor,
    editor::{EditType, Editor},
    language::LapceLanguage,
    register::{Clipboard, Register},
    selection::{InsertDrift, Selection},
    style::line_styles,
    syntax::{edit::SyntaxEdit, Syntax},
    word::WordCursor,
};
use lapce_rpc::{
    buffer::BufferId,
    plugin::PluginId,
    proxy::ProxyResponse,
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
    editor::{
        view_data::{LineExtraStyle, TextLayoutLine},
        visual_line::TextLayoutCache,
    },
    find::{Find, FindProgress, FindResult},
    history::DocumentHistory,
    window_tab::CommonData,
    workspace::LapceWorkspace,
};

pub mod phantom_text;

pub struct SystemClipboard;

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl Clipboard for SystemClipboard {
    fn get_string(&mut self) -> Option<String> {
        floem::Clipboard::get_contents().ok()
    }

    fn put_string(&mut self, s: impl AsRef<str>) {
        let _ = floem::Clipboard::set_contents(s.as_ref().to_string());
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
    File { path: PathBuf, read_only: bool },
    /// A local document, which doens't need to be sync to the disk.
    Local,
    /// A document of an old version in the source control
    History(DocHistory),
    /// A new file which doesn't exist in the file system
    Scratch { id: BufferId, name: String },
}

impl DocContent {
    pub fn is_local(&self) -> bool {
        matches!(self, DocContent::Local)
    }

    pub fn is_file(&self) -> bool {
        matches!(self, DocContent::File { .. })
    }

    pub fn read_only(&self) -> bool {
        match self {
            DocContent::File { read_only, .. } => *read_only,
            DocContent::Local => false,
            DocContent::History(_) => true,
            DocContent::Scratch { .. } => false,
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            DocContent::File { path, .. } => Some(path),
            DocContent::Local => None,
            DocContent::History(_) => None,
            DocContent::Scratch { .. } => None,
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

#[derive(Clone)]
pub struct Preedit {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
    pub offset: usize,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Document {:?}", self.buffer_id))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderWhitespace {
    #[default]
    None,
    All,
    Boundary,
    Trailing,
}

// TODO(floem-editor): Provide a struct version which just has all the fields as a decent default
/// Style information for a specific line.  
/// Created by [`Backend::line_style(line)`]  
/// This provides a way to query for specific line information. It is not necessarily still valid
/// if there have been edits since it was created.
pub trait LineStyling: Sized {
    // TODO: should this return LineHeightValue
    /// Default line-height for this line
    fn line_height(&self) -> f32 {
        let font_size = self.font_size() as f32;
        (1.5 * font_size).round().max(font_size)
    }

    /// Default font family for this line
    fn font_family(&self) -> Cow<[FamilyOwned]> {
        Cow::Borrowed(&[FamilyOwned::SansSerif])
    }

    /// Default font size for this line
    fn font_size(&self) -> usize {
        16
    }

    fn color(&self) -> Color {
        Color::BLACK
    }

    fn weight(&self) -> Weight {
        Weight::NORMAL
    }

    // TODO(minor): better name?
    fn italic_style(&self) -> floem::cosmic_text::Style {
        floem::cosmic_text::Style::Normal
    }

    fn stretch(&self) -> Stretch {
        Stretch::Normal
    }

    fn tab_width(&self) -> usize {
        4
    }

    fn render_whitespace(&self) -> RenderWhitespace {
        RenderWhitespace::None
    }

    /// Get the color for specific style names returned by [`Self::line_style`]  
    /// In Lapce this is used for getting `style.` colors, such as for syntax highlighting.
    fn style_color(&self, _style: &str) -> Option<Color> {
        None
    }

    // TODO: provide functions to do common phantom text operations without actually creating the
    // entire phantom text, it would make many pieces of logic cheaper due to not having to
    // allocate and construct them.

    // This does not have a default implementation because it *should* provide relevant IME phantom
    // text by default!
    fn phantom_text(&self) -> PhantomTextLine;

    fn line_style(&self) -> Arc<Vec<LineStyle>> {
        Arc::new(Vec::new())
    }
}

/// The response from a save operation.  
///
/// Currently nothing.
#[derive(Debug)]
pub struct SaveResponse {}

/// A backend for [`Document`] related operations, such as saving and opening files.  
/// This allows swapping out the supplier of the documents.  
/// Ex: A direct implementation that saves files to disk.  
/// Ex: An implementation that uses a proxy, like `lapce-proxy` to load from local and remote
/// locations.
///   
/// These functions do not take a reference to `&self`, but rather to the `Document<Self>` which
/// the backend is accessible from.    
///   
/// This requires `Clone` due to some logic on other threads needing references. This does mean
/// that types implementing backend should handle that gracefully.
pub trait Backend: Sized + Clone {
    /// The general error type for the backend's operations.  
    /// This does not require [`std::error::Error`] due to the common `anyhow::Error` not
    /// implementing it.
    ///
    /// A single type is used rather than many different error types at the moment, but this may be
    /// changed in the future if it seems beneficial
    type Error: std::fmt::Debug;

    /// The type for style information on the line.
    type LineStyling: LineStyling;

    /// Get an identifier for the config, used for clearing the cache if it were to change.
    fn config_id(doc: &Document<Self>) -> u64;

    /// We've initialized the content of the document. The buffer holds the new content.  
    /// This is ran before `on_update` is called.  
    /// Note that this is called from within a [`batch`]
    fn pre_update_init_content(_doc: &Document<Self>) {}

    /// We're initializing the content of the document. The buffer holds the new content.
    /// Note that this is called from within a [`batch`]
    fn init_content(_doc: &Document<Self>) {}

    /// Called when the document is updated, like when there is an edit.  
    /// Note: this is called from within a [`batch`]
    fn on_update(_doc: &Document<Self>, _edits: Option<&[SyntaxEdit]>) {}

    // TODO(floem-editor): We may need to pass in the computed `rev` since updating proxy uses it
    /// Apply a single edit delta  
    fn apply_delta(
        _doc: &Document<Self>,
        _rev: u64,
        _delta: &RopeDelta,
        _inval: &InvalLines,
    ) {
    }

    /// Save a file (potentially asynchronously).  
    /// The callback will be called with the result of the save operation
    fn save(
        doc: &Document<Self>,
        cb: impl FnOnce(Result<SaveResponse, Self::Error>) + 'static,
    );

    /// How often should the document autosave  
    /// Returns `None` if autosave is disabled
    fn autosave_interval(_doc: &Document<Self>) -> Option<Duration> {
        None
    }

    fn line_styling(doc: &Document<Self>, line: usize) -> Self::LineStyling;

    /// Apply styles onto the text layout line
    fn apply_styles(
        _doc: &Document<Self>,
        _line: usize,
        _text_layout_line: &mut TextLayoutLine,
    ) {
    }

    fn clear_style_cache(_doc: &Document<Self>) {}

    // TODO: should sticky headers be supplied conditionally from the outside instead? So they can
    // set them however they want to whatever they want?
    /// Get the sticky headers for a line
    fn sticky_headers(_doc: &Document<Self>, _line: usize) -> Option<Vec<usize>> {
        None
    }

    /// Get the indentation line for a line
    fn indent_line(
        _doc: &Document<Self>,
        line: usize,
        _line_content: &str,
    ) -> usize {
        line
    }

    /// Get the previous unmatched character `c` from the offset
    fn previous_unmatched(
        &self,
        buffer: &Buffer,
        c: char,
        offset: usize,
    ) -> Option<usize> {
        WordCursor::new(buffer.text(), offset).previous_unmatched(c)
    }

    fn comment_token(_doc: &Document<Self>) -> &str {
        ""
    }

    // TODO: configurable
    /// Wheter it should automatically close matching pairs like `()`, `[]`, `""`, etc.
    fn auto_closing_matching_pairs(_doc: &Document<Self>) -> bool {
        false
    }

    /// Whether it should automatically surround the selection with matching pairs like `()`, `""`,
    /// etc.
    fn auto_surround(_doc: &Document<Self>) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct DocLineStyling {
    line: usize,
    // TODO: should we just clone document due to how much of this it grabs....
    config: Arc<LapceConfig>,
    doc: Document<DocBackend>,
}
impl DocLineStyling {
    /// Iterate over the editor diagnostics on this line
    fn iter_diagnostics(&self) -> impl Iterator<Item = EditorDiagnostic> + '_ {
        self.config
            .editor
            .enable_completion_lens
            .then_some(())
            .map(|_| self.doc.backend.diagnostics.diagnostics.get_untracked())
            .into_iter()
            .flatten()
            .filter(|diag| {
                diag.diagnostic.range.end.line as usize == self.line
                    && diag.diagnostic.severity < Some(DiagnosticSeverity::HINT)
            })
    }

    /// Get the max severity of the diagnostics.  
    /// This is used to determine the color given to the background of the line
    fn max_diag_severity(&self) -> Option<DiagnosticSeverity> {
        let mut max_severity = None;
        for diag in self.iter_diagnostics() {
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
        }

        max_severity
    }
}
impl LineStyling for DocLineStyling {
    fn line_height(&self) -> f32 {
        self.config.editor.line_height() as f32
    }

    fn font_family(&self) -> Cow<[FamilyOwned]> {
        // TODO: cache this font family
        let families =
            FamilyOwned::parse_list(&self.config.editor.font_family).collect();

        Cow::Owned(families)
    }

    fn font_size(&self) -> usize {
        self.config.editor.font_size()
    }

    fn color(&self) -> Color {
        self.config.color(LapceColor::EDITOR_FOREGROUND)
    }

    fn tab_width(&self) -> usize {
        self.config.editor.tab_width
    }

    fn style_color(&self, style: &str) -> Option<Color> {
        self.config.style_color(style)
    }

    fn phantom_text(&self) -> PhantomTextLine {
        let backend = &self.doc.backend;
        let config = &self.config;

        let (start_offset, end_offset) = self.doc.buffer.with_untracked(|buffer| {
            (
                buffer.offset_of_line(self.line),
                buffer.offset_of_line(self.line + 1),
            )
        });

        let inlay_hints = backend.inlay_hints.get_untracked();
        // If hints are enabled, and the hints field is filled, then get the hints for this line
        // and convert them into PhantomText instances
        let hints = config
            .editor
            .enable_inlay_hints
            .then_some(())
            .and(inlay_hints.as_ref())
            .map(|hints| hints.iter_chunks(start_offset..end_offset))
            .into_iter()
            .flatten()
            .filter(|(interval, _)| {
                interval.start >= start_offset && interval.start < end_offset
            })
            .map(|(interval, inlay_hint)| {
                let (_, col) = self
                    .doc
                    .buffer
                    .with_untracked(|b| b.offset_to_line_col(interval.start));
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
                    fg: Some(config.color(LapceColor::INLAY_HINT_FOREGROUND)),
                    // font_family: Some(config.editor.inlay_hint_font_family()),
                    font_size: Some(config.editor.inlay_hint_font_size()),
                    bg: Some(config.color(LapceColor::INLAY_HINT_BACKGROUND)),
                    under_line: None,
                }
            });
        // You're quite unlikely to have more than six hints on a single line
        // this later has the diagnostics added onto it, but that's still likely to be below six
        // overall.
        let mut text: SmallVec<[PhantomText; 6]> = hints.collect();

        // If error lens is enabled, and the diagnostics field is filled, then get the diagnostics
        // that end on this line which have a severity worse than HINT and convert them into
        // PhantomText instances
        let diag_text = config
            .editor
            .enable_error_lens
            .then_some(())
            .map(|_| backend.diagnostics.diagnostics.get_untracked())
            .into_iter()
            .flatten()
            .filter(|diag| {
                diag.diagnostic.range.end.line as usize == self.line
                    && diag.diagnostic.severity < Some(DiagnosticSeverity::HINT)
            })
            .map(|diag| {
                let col = self.doc.buffer.with_untracked(|buffer| {
                    buffer.offset_of_line(self.line + 1)
                        - buffer.offset_of_line(self.line)
                });
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

                    config.color(theme_prop)
                };

                let text = if config.editor.error_lens_multiline {
                    format!("    {}", diag.diagnostic.message)
                } else {
                    format!("    {}", diag.diagnostic.message.lines().join(" "))
                };
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

        let (completion_line, completion_col) =
            backend.completion_pos.get_untracked();
        let completion_text = config
            .editor
            .enable_completion_lens
            .then_some(())
            .and(backend.completion_lens.get_untracked())
            // TODO: We're probably missing on various useful completion things to include here!
            .filter(|_| self.line == completion_line)
            .map(|completion| PhantomText {
                kind: PhantomTextKind::Completion,
                col: completion_col,
                text: completion.clone(),
                fg: Some(config.color(LapceColor::COMPLETION_LENS_FOREGROUND)),
                font_size: Some(config.editor.completion_lens_font_size()),
                // font_family: Some(config.editor.completion_lens_font_family()),
                bg: None,
                under_line: None,
                // TODO: italics?
            });
        if let Some(completion_text) = completion_text {
            text.push(completion_text);
        }

        // TODO: don't display completion lens and inline completion at the same time
        // and/or merge them so that they can be shifted between like multiple inline completions
        // can
        let (inline_completion_line, inline_completion_col) =
            backend.inline_completion_pos.get_untracked();
        let inline_completion_text = config
            .editor
            .enable_inline_completion
            .then_some(())
            .and(backend.inline_completion.get_untracked())
            .filter(|_| self.line == inline_completion_line)
            .map(|completion| PhantomText {
                kind: PhantomTextKind::Completion,
                col: inline_completion_col,
                text: completion.clone(),
                fg: Some(config.color(LapceColor::COMPLETION_LENS_FOREGROUND)),
                font_size: Some(config.editor.completion_lens_font_size()),
                // font_family: Some(config.editor.completion_lens_font_family()),
                bg: None,
                under_line: None,
                // TODO: italics?
            });
        if let Some(inline_completion_text) = inline_completion_text {
            text.push(inline_completion_text);
        }

        if let Some(preedit) = self.doc.preedit_phantom_text(
            Some(config.color(LapceColor::EDITOR_FOREGROUND)),
            self.line,
        ) {
            text.push(preedit)
        }

        text.sort_by(|a, b| {
            if a.col == b.col {
                a.kind.cmp(&b.kind)
            } else {
                a.col.cmp(&b.col)
            }
        });

        PhantomTextLine { text }
    }

    /// Get the style information for the particular line from semantic/syntax highlighting.
    /// This caches the result if possible.
    fn line_style(&self) -> Arc<Vec<LineStyle>> {
        let line = self.line;
        let backend = &self.doc.backend;
        if backend.line_styles.borrow().get(&line).is_none() {
            let styles = backend.styles();

            let line_styles = styles
                .map(|styles| {
                    let text = self
                        .doc
                        .buffer
                        .with_untracked(|buffer| buffer.text().clone());
                    line_styles(&text, line, &styles)
                })
                .unwrap_or_default();
            backend
                .line_styles
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        backend.line_styles.borrow().get(&line).cloned().unwrap()
    }
}

/// (Offset -> (Plugin the code actions are from, Code Actions))
pub type CodeActions = im::HashMap<usize, Arc<(PluginId, CodeActionResponse)>>;
// TODO(minor): we could try stripping this down to the fields it exactly needs, like proxy
/// Lapce backend for files accessible through proxy (local or remote).
#[derive(Clone)]
pub struct DocBackend {
    pub syntax: RwSignal<Syntax>,
    /// LSP Semantic highlighting information
    semantic_styles: RwSignal<Option<Spans<Style>>>,
    line_styles: Rc<RefCell<LineStyles>>,

    /// Stores information about different versions of the document from source control.
    histories: RwSignal<im::HashMap<String, DocumentHistory>>,
    pub head_changes: RwSignal<im::Vector<DiffLines>>,

    /// Inlay hints for the document
    pub inlay_hints: RwSignal<Option<Spans<InlayHint>>>,

    /// The diagnostics for the document
    pub diagnostics: DiagnosticData,

    /// Current completion lens text, if any.
    /// This will be displayed even on views that are not focused.
    pub completion_lens: RwSignal<Option<String>>,
    /// (line, col)
    pub completion_pos: RwSignal<(usize, usize)>,

    /// Current inline completion text, if any.  
    /// This will be displayed even on views that are not focused.
    pub inline_completion: RwSignal<Option<String>>,
    /// (line, col)
    pub inline_completion_pos: RwSignal<(usize, usize)>,

    /// (Offset -> (Plugin the code actions are from, Code Actions))
    pub code_actions: RwSignal<CodeActions>,

    pub find_result: FindResult,

    common: Rc<CommonData>,
}
impl DocBackend {
    pub fn new(
        cx: Scope,
        syntax: Syntax,
        diagnostics: Option<DiagnosticData>,
        common: Rc<CommonData>,
    ) -> Self {
        let diagnostics = diagnostics.unwrap_or_else(|| DiagnosticData {
            expanded: cx.create_rw_signal(true),
            diagnostics: cx.create_rw_signal(im::Vector::new()),
        });
        Self {
            syntax: cx.create_rw_signal(syntax),
            semantic_styles: cx.create_rw_signal(None),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics,
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            inline_completion: cx.create_rw_signal(None),
            inline_completion_pos: cx.create_rw_signal((0, 0)),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            common,
        }
    }

    /// Update the styles after an edit, so the highlights are at the correct positions.
    /// This does not do a reparse of the document itself.
    fn update_styles(&self, delta: &RopeDelta) {
        batch(|| {
            self.semantic_styles.update(|styles| {
                if let Some(styles) = styles.as_mut() {
                    styles.apply_shape(delta);
                }
            });
            self.syntax.update(|syntax| {
                if let Some(styles) = syntax.styles.as_mut() {
                    styles.apply_shape(delta);
                }
                syntax.lens.apply_delta(delta);
            });
        });
    }

    /// Update the inlay hints so their positions are correct after an edit.
    fn update_inlay_hints(&self, delta: &RopeDelta) {
        self.inlay_hints.update(|inlay_hints| {
            if let Some(hints) = inlay_hints.as_mut() {
                hints.apply_shape(delta);
            }
        });
    }

    fn clear_code_actions(&self) {
        self.code_actions.update(|c| {
            c.clear();
        });
    }

    fn update_find_result(&self, delta: &RopeDelta) {
        self.find_result.occurrences.update(|s| {
            *s = s.apply_delta(delta, true, InsertDrift::Default);
        })
    }

    /// Get the active style information, either the semantic styles or the
    /// tree-sitter syntax styles.
    fn styles(&self) -> Option<Spans<Style>> {
        if let Some(semantic_styles) = self.semantic_styles.get_untracked() {
            Some(semantic_styles)
        } else {
            self.syntax.with_untracked(|syntax| syntax.styles.clone())
        }
    }
}
impl Backend for DocBackend {
    type Error = ();
    type LineStyling = DocLineStyling;

    fn config_id(doc: &Document<Self>) -> u64 {
        doc.backend.common.config.with_untracked(|config| config.id)
    }

    fn pre_update_init_content(doc: &Document<Self>) {
        doc.backend.syntax.with_untracked(|syntax| {
            doc.buffer.update(|buffer| {
                buffer.detect_indent(syntax);
            });
        });
    }

    fn init_content(doc: &Document<Self>) {
        doc.init_diagnostics();
        doc.retrieve_head();
    }

    fn on_update(doc: &Document<Self>, edits: Option<&[SyntaxEdit]>) {
        doc.backend.clear_code_actions();
        doc.trigger_syntax_change(edits);
        doc.trigger_head_change();
        doc.get_semantic_styles();
        doc.get_inlay_hints();
        doc.backend.find_result.reset();
    }

    fn apply_delta(
        doc: &Document<Self>,
        rev: u64,
        delta: &RopeDelta,
        inval: &InvalLines,
    ) {
        doc.backend.update_find_result(delta);
        doc.backend.update_styles(delta);
        doc.backend.update_inlay_hints(delta);
        doc.update_diagnostics(delta);
        doc.update_completion_lens(delta);
        if let DocContent::File { path, .. } = doc.content.get_untracked() {
            doc.update_breakpoints(delta, &path, &inval.old_text);
            doc.backend.common.proxy.update(path, delta.clone(), rev);
        }
    }

    fn save(
        doc: &Document<Self>,
        cb: impl FnOnce(Result<SaveResponse, Self::Error>) + 'static,
    ) {
        let content = doc.content.get_untracked();
        if let DocContent::File { path, .. } = content {
            let rev = doc.rev();
            let buffer = doc.buffer;
            let send = create_ext_action(doc.scope, move |result| {
                if let Ok(ProxyResponse::SaveResponse {}) = result {
                    let current_rev = buffer.with_untracked(|buffer| buffer.rev());
                    if current_rev == rev {
                        buffer.update(|buffer| {
                            buffer.set_pristine();
                        });
                        cb(Ok(SaveResponse {}));
                    }
                }
            });

            doc.backend
                .common
                .proxy
                .save(rev, path, true, move |result| {
                    send(result);
                })
        }
    }

    fn autosave_interval(doc: &Document<Self>) -> Option<Duration> {
        let interval = doc
            .backend
            .common
            .config
            .with_untracked(|config| config.editor.autosave_interval);

        if interval > 0 {
            Some(Duration::from_millis(interval))
        } else {
            None
        }
    }

    fn line_styling(doc: &Document<Self>, line: usize) -> Self::LineStyling {
        DocLineStyling {
            line,
            config: doc.backend.common.config.get_untracked(),
            doc: doc.clone(),
        }
    }

    fn apply_styles(
        doc: &Document<Self>,
        line: usize,
        text_layout_line: &mut TextLayoutLine,
    ) {
        let backend = &doc.backend;
        let config = backend.common.config.get_untracked();

        text_layout_line.extra_style.clear();
        let text_layout = &text_layout_line.text;

        let styling = doc.line_styling(line);
        let phantom_text = styling.phantom_text();

        let phantom_styles = phantom_text
            .offset_size_iter()
            .filter(move |(_, _, _, p)| p.bg.is_some() || p.under_line.is_some())
            .flat_map(move |(col_shift, size, col, phantom)| {
                let start = col + col_shift;
                let end = start + size;

                extra_styles_for_range(
                    text_layout,
                    start,
                    end,
                    phantom.bg,
                    phantom.under_line,
                    None,
                )
            });
        text_layout_line.extra_style.extend(phantom_styles);

        // Add the styling for the diagnostic severity, if applicable
        if let Some(max_severity) = styling.max_diag_severity() {
            let theme_prop = if max_severity == DiagnosticSeverity::ERROR {
                LapceColor::ERROR_LENS_ERROR_BACKGROUND
            } else if max_severity == DiagnosticSeverity::WARNING {
                LapceColor::ERROR_LENS_WARNING_BACKGROUND
            } else {
                LapceColor::ERROR_LENS_OTHER_BACKGROUND
            };

            let size = text_layout.size();
            let x1 = if !config.editor.error_lens_end_of_line {
                let error_end_x = text_layout.size().width;
                Some(error_end_x.max(size.width))
            } else {
                None
            };

            // TODO(minor): Should we show the background only on wrapped lines that have the
            // diagnostic actually on that line?
            // That would make it more obvious where it is from and matches other editors.
            text_layout_line.extra_style.push(LineExtraStyle {
                x: 0.0,
                y: 0.0,
                width: x1,
                height: text_layout.size().height,
                bg_color: Some(config.color(theme_prop)),
                under_line: None,
                wave_line: None,
            });
        }

        backend.diagnostics.diagnostics.with_untracked(|diags| {
            doc.buffer.with_untracked(|buffer| {
                for diag in diags {
                    if diag.diagnostic.range.start.line as usize <= line
                        && line <= diag.diagnostic.range.end.line as usize
                    {
                        let start = if diag.diagnostic.range.start.line as usize
                            == line
                        {
                            let (_, col) = buffer.offset_to_line_col(diag.range.0);
                            col
                        } else {
                            let offset =
                                buffer.first_non_blank_character_on_line(line);
                            let (_, col) = buffer.offset_to_line_col(offset);
                            col
                        };
                        let start = phantom_text.col_after(start, true);

                        let end = if diag.diagnostic.range.end.line as usize == line
                        {
                            let (_, col) = buffer.offset_to_line_col(diag.range.1);
                            col
                        } else {
                            buffer.line_end_col(line, true)
                        };
                        let end = phantom_text.col_after(end, false);

                        // let x0 = text_layout.hit_position(start).point.x;
                        // let x1 = text_layout.hit_position(end).point.x;
                        let color_name = match diag.diagnostic.severity {
                            Some(DiagnosticSeverity::ERROR) => {
                                LapceColor::LAPCE_ERROR
                            }
                            _ => LapceColor::LAPCE_WARN,
                        };
                        let color = config.color(color_name);

                        let styles = extra_styles_for_range(
                            text_layout,
                            start,
                            end,
                            None,
                            None,
                            Some(color),
                        );

                        text_layout_line.extra_style.extend(styles);
                    }
                }
            })
        });
    }

    fn clear_style_cache(doc: &Document<Self>) {
        doc.backend.line_styles.borrow_mut().clear();
    }

    fn sticky_headers(doc: &Document<Self>, line: usize) -> Option<Vec<usize>> {
        doc.buffer.with_untracked(|buffer| {
            let offset = buffer.offset_of_line(line + 1);
            doc.backend.syntax.with_untracked(|syntax| {
                syntax.sticky_headers(offset).map(|offsets| {
                    offsets
                        .iter()
                        .filter_map(|offset| {
                            let l = buffer.line_of_offset(*offset);
                            if l <= line {
                                Some(l)
                            } else {
                                None
                            }
                        })
                        .dedup()
                        .sorted()
                        .collect()
                })
            })
        })
    }

    fn indent_line(doc: &Document<Self>, line: usize, line_content: &str) -> usize {
        if line_content.trim().is_empty() {
            let offset = doc.buffer.with_untracked(|b| b.offset_of_line(line));
            if let Some(offset) = doc
                .backend
                .syntax
                .with_untracked(|s| s.parent_offset(offset))
            {
                return doc.buffer.with_untracked(|b| b.line_of_offset(offset));
            }
        }

        line
    }

    fn previous_unmatched(
        &self,
        buffer: &Buffer,
        c: char,
        offset: usize,
    ) -> Option<usize> {
        if self.syntax.with_untracked(|syntax| syntax.layers.is_some()) {
            self.syntax.with_untracked(|syntax| {
                syntax.find_tag(offset, true, &CharBuffer::new(c))
            })
        } else {
            WordCursor::new(buffer.text(), offset).previous_unmatched(c)
        }
    }

    fn comment_token(doc: &Document<Self>) -> &str {
        doc.backend
            .syntax
            .with_untracked(|syntax| syntax.language)
            .comment_token()
    }

    fn auto_closing_matching_pairs(doc: &Document<Self>) -> bool {
        doc.backend
            .common
            .config
            .with_untracked(|config| config.editor.auto_closing_matching_pairs)
    }

    fn auto_surround(doc: &Document<Self>) -> bool {
        doc.backend
            .common
            .config
            .with_untracked(|config| config.editor.auto_surround)
    }
}

/// Lapce extension functions for [`Document`].  
/// Some of these are not actually made to be public, but putting them on this trait simplifies
/// giving them the [`Document`]
pub trait DocumentExt {
    fn find(&self) -> &Find;

    fn update_find(&self);

    fn syntax(&self) -> RwSignal<Syntax>;

    fn set_syntax(&self, syntax: Syntax);

    /// Set the syntax highlighting this document should use.
    fn set_language(&self, language: LapceLanguage);

    fn trigger_syntax_change(&self, edits: Option<&[SyntaxEdit]>);

    fn get_semantic_styles(&self);

    fn head_changes(&self) -> RwSignal<im::Vector<DiffLines>>;

    /// Retrieve the `head` version of the buffer
    fn retrieve_head(&self);

    fn trigger_head_change(&self);

    /// Get the diagnostics
    fn diagnostics(&self) -> &DiagnosticData;

    /// Init diagnostics' offset ranges from lsp positions
    fn init_diagnostics(&self);

    /// Update the diagnostics' positions after an edit
    fn update_diagnostics(&self, delta: &RopeDelta);

    fn get_inlay_hints(&self);
    /// Get the current completion lens text
    fn completion_lens(&self) -> Option<String>;

    fn set_completion_lens(&self, completion_lens: String, line: usize, col: usize);

    fn clear_completion_lens(&self);

    /// Update the completion lens position after an edit so that it appears in the correct place.
    fn update_completion_lens(&self, delta: &RopeDelta);

    fn set_inline_completion(
        &self,
        inline_completion: String,
        line: usize,
        col: usize,
    );

    fn clear_inline_completion(&self);

    /// Update the inline completion position after an edit so that it appears in the correct place.
    fn update_inline_completion(&self, delta: &RopeDelta);

    fn code_actions(&self) -> RwSignal<CodeActions>;

    fn find_enclosing_brackets(&self, offset: usize) -> Option<(usize, usize)>;

    fn update_breakpoints(&self, delta: &RopeDelta, path: &Path, old_text: &Rope);
}
impl DocumentExt for Document<DocBackend> {
    fn find(&self) -> &Find {
        &self.backend.common.find
    }

    fn update_find(&self) {
        let find_result = &self.backend.find_result;
        let common = &self.backend.common;
        let find_rev = common.find.rev.get_untracked();
        if find_result.find_rev.get_untracked() != find_rev {
            if common.find.search_string.with_untracked(|search_string| {
                search_string
                    .as_ref()
                    .map(|s| s.content.is_empty())
                    .unwrap_or(true)
            }) {
                find_result.occurrences.set(Selection::new());
            }
            find_result.reset();
            find_result.find_rev.set(find_rev);
        }

        if find_result.progress.get_untracked() != FindProgress::Started {
            return;
        }

        let search = common.find.search_string.get_untracked();
        let search = match search {
            Some(search) => search,
            None => return,
        };
        if search.content.is_empty() {
            return;
        }

        find_result
            .progress
            .set(FindProgress::InProgress(Selection::new()));

        let find_result = find_result.clone();
        let send = create_ext_action(self.scope, move |occurrences| {
            find_result.occurrences.set(occurrences);
            find_result.progress.set(FindProgress::Ready);
        });

        let text = self.buffer.with_untracked(|b| b.text().clone());
        let case_matching = common.find.case_matching.get_untracked();
        let whole_words = common.find.whole_words.get_untracked();
        rayon::spawn(move || {
            let mut occurrences = Selection::new();
            Find::find(
                &text,
                &search,
                0,
                text.len(),
                case_matching,
                whole_words,
                true,
                &mut occurrences,
            );
            send(occurrences);
        });
    }

    fn syntax(&self) -> RwSignal<Syntax> {
        self.backend.syntax
    }

    fn set_syntax(&self, syntax: Syntax) {
        batch(|| {
            self.backend.syntax.set(syntax);
            if self.backend.semantic_styles.with_untracked(|s| s.is_none()) {
                self.clear_style_cache();
            }
            self.clear_sticky_headers_cache();
        });
    }

    fn set_language(&self, language: LapceLanguage) {
        self.backend.syntax.set(Syntax::from_language(language));
    }

    fn trigger_syntax_change(&self, edits: Option<&[SyntaxEdit]>) {
        let (rev, text) =
            self.buffer.with_untracked(|b| (b.rev(), b.text().clone()));

        self.backend.syntax.update(|syntax| {
            syntax.parse(rev, text, edits);
        });
    }

    /// Request semantic styles for the buffer from the LSP through the proxy.
    fn get_semantic_styles(&self) {
        if !self.loaded() {
            return;
        }

        let path =
            if let DocContent::File { path, .. } = self.content.get_untracked() {
                path
            } else {
                return;
            };

        let (rev, len) = self.buffer.with_untracked(|b| (b.rev(), b.len()));

        let syntactic_styles = self
            .backend
            .syntax
            .with_untracked(|syntax| syntax.styles.clone());

        let doc = self.clone();
        let send = create_ext_action(self.scope, move |styles| {
            if doc.buffer.with_untracked(|b| b.rev()) == rev {
                doc.backend.semantic_styles.set(Some(styles));
                doc.clear_style_cache();
            }
        });

        self.backend
            .common
            .proxy
            .get_semantic_tokens(path, move |result| {
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

                        let styles = if let Some(syntactic_styles) = syntactic_styles
                        {
                            syntactic_styles.merge(&styles, |a, b| {
                                if let Some(b) = b {
                                    return b.clone();
                                }
                                a.clone()
                            })
                        } else {
                            styles
                        };

                        send(styles);
                    });
                }
            });
    }

    fn head_changes(&self) -> RwSignal<im::Vector<DiffLines>> {
        self.backend.head_changes
    }

    fn retrieve_head(&self) {
        if let DocContent::File { path, .. } = self.content.get_untracked() {
            let histories = self.backend.histories;

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
            let proxy = self.backend.common.proxy.clone();
            std::thread::spawn(move || {
                proxy.get_buffer_head(path, move |result| {
                    send(result);
                });
            });
        }
    }

    fn trigger_head_change(&self) {
        let history = if let Some(text) =
            self.backend.histories.with_untracked(|histories| {
                histories
                    .get("head")
                    .map(|history| history.buffer.text().clone())
            }) {
            text
        } else {
            return;
        };

        let rev = self.rev();
        let left_rope = history;
        let (atomic_rev, right_rope) = self
            .buffer
            .with_untracked(|b| (b.atomic_rev(), b.text().clone()));

        let send = {
            let atomic_rev = atomic_rev.clone();
            let head_changes = self.backend.head_changes;
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

    fn diagnostics(&self) -> &DiagnosticData {
        &self.backend.diagnostics
    }

    fn init_diagnostics(&self) {
        self.clear_text_cache();
        self.backend.clear_code_actions();
        self.backend.diagnostics.diagnostics.update(|diagnostics| {
            for diagnostic in diagnostics.iter_mut() {
                let (start, end) = self.buffer.with_untracked(|buffer| {
                    (
                        buffer
                            .offset_of_position(&diagnostic.diagnostic.range.start),
                        buffer.offset_of_position(&diagnostic.diagnostic.range.end),
                    )
                });
                diagnostic.range = (start, end);
            }
        });
    }

    fn update_diagnostics(&self, delta: &RopeDelta) {
        if self
            .backend
            .diagnostics
            .diagnostics
            .with_untracked(|d| d.is_empty())
        {
            return;
        }
        self.backend.diagnostics.diagnostics.update(|diagnostics| {
            for diagnostic in diagnostics.iter_mut() {
                let mut transformer = Transformer::new(delta);
                let (start, end) = diagnostic.range;
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );

                let (new_start_pos, new_end_pos) = self.buffer.with_untracked(|b| {
                    (
                        b.offset_to_position(new_start),
                        b.offset_to_position(new_end),
                    )
                });

                diagnostic.range = (new_start, new_end);

                diagnostic.diagnostic.range.start = new_start_pos;
                diagnostic.diagnostic.range.end = new_end_pos;
            }
        });
    }

    fn get_inlay_hints(&self) {
        if !self.loaded() {
            return;
        }

        let backend = &self.backend;

        let path =
            if let DocContent::File { path, .. } = self.content.get_untracked() {
                path
            } else {
                return;
            };

        let (buffer, rev, len) = self
            .buffer
            .with_untracked(|b| (b.clone(), b.rev(), b.len()));

        let doc = self.clone();
        let send = create_ext_action(self.scope, move |hints| {
            if doc.buffer.with_untracked(|b| b.rev()) == rev {
                doc.backend.inlay_hints.set(Some(hints));
                doc.clear_text_cache();
            }
        });

        backend.common.proxy.get_inlay_hints(path, move |result| {
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

    fn completion_lens(&self) -> Option<String> {
        self.backend.completion_lens.get_untracked()
    }

    fn set_completion_lens(&self, completion_lens: String, line: usize, col: usize) {
        // TODO: more granular invalidation
        self.clear_text_cache();
        self.backend.completion_lens.set(Some(completion_lens));
        self.backend.completion_pos.set((line, col));
    }

    fn clear_completion_lens(&self) {
        // TODO: more granular invalidation
        if self.backend.completion_lens.with_untracked(Option::is_some) {
            self.backend.completion_lens.set(None);
            self.clear_text_cache();
        }
    }

    /// Update the completion lens position after an edit so that it appears in the correct place.
    fn update_completion_lens(&self, delta: &RopeDelta) {
        let backend = &self.backend;
        let Some(completion) = backend.completion_lens.get_untracked() else {
            return;
        };

        let (line, col) = backend.completion_pos.get_untracked();
        let offset = self
            .buffer
            .with_untracked(|b| b.offset_of_line_col(line, col));

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
                backend
                    .completion_lens
                    .set(Some(completion[new_len..].to_string()));
            }
        }

        // Shift the position by the rope delta
        let mut transformer = Transformer::new(delta);

        let new_offset = transformer.transform(offset, true);
        let new_pos = self
            .buffer
            .with_untracked(|b| b.offset_to_line_col(new_offset));

        backend.completion_pos.set(new_pos);
    }

    fn set_inline_completion(
        &self,
        inline_completion: String,
        line: usize,
        col: usize,
    ) {
        // TODO: more granular invalidation
        batch(|| {
            self.clear_text_cache();
            self.backend.inline_completion.set(Some(inline_completion));
            self.backend.inline_completion_pos.set((line, col));
        });
    }

    fn clear_inline_completion(&self) {
        if self
            .backend
            .inline_completion
            .with_untracked(Option::is_some)
        {
            self.backend.inline_completion.set(None);
            self.clear_text_cache();
        }
    }

    fn update_inline_completion(&self, delta: &RopeDelta) {
        let backend = &self.backend;
        let Some(completion) = backend.inline_completion.get_untracked() else {
            return;
        };

        let (line, col) = backend.completion_pos.get_untracked();
        let offset = self
            .buffer
            .with_untracked(|b| b.offset_of_line_col(line, col));

        // If the edit is easily checkable + updateable from, then we alter the text.
        // In normal typing, if we didn't do this, then the text would jitter forward and then
        // backwards as the completion is updated.
        // TODO: this could also handle simple deletion, but we don't currently keep track of
        // the past completion string content in the field.
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
                backend
                    .inline_completion
                    .set(Some(completion[new_len..].to_string()));
            }
        }

        // Shift the position by the rope delta
        let mut transformer = Transformer::new(delta);

        let new_offset = transformer.transform(offset, true);
        let new_pos = self
            .buffer
            .with_untracked(|b| b.offset_to_line_col(new_offset));

        backend.inline_completion_pos.set(new_pos);
    }

    fn code_actions(&self) -> RwSignal<CodeActions> {
        self.backend.code_actions
    }

    fn find_enclosing_brackets(&self, offset: usize) -> Option<(usize, usize)> {
        self.backend
            .syntax
            .with_untracked(|syntax| {
                (!syntax.text.is_empty()).then(|| syntax.find_enclosing_pair(offset))
            })
            // If syntax.text is empty, either the buffer is empty or we don't have syntax support
            // for the current language.
            // Try a language unaware search for enclosing brackets in case it is the latter.
            .unwrap_or_else(|| {
                self.buffer.with_untracked(|buffer| {
                    WordCursor::new(buffer.text(), offset).find_enclosing_pair()
                })
            })
    }

    fn update_breakpoints(&self, delta: &RopeDelta, path: &Path, old_text: &Rope) {
        if self
            .backend
            .common
            .breakpoints
            .with_untracked(|breakpoints| breakpoints.contains_key(path))
        {
            self.backend.common.breakpoints.update(|breakpoints| {
                if let Some(path_breakpoints) = breakpoints.get_mut(path) {
                    let mut transformer = Transformer::new(delta);
                    self.buffer.with_untracked(|buffer| {
                        *path_breakpoints = path_breakpoints
                            .clone()
                            .into_values()
                            .map(|mut b| {
                                let offset = old_text.offset_of_line(b.line);
                                let offset = transformer.transform(offset, false);
                                let line = buffer.line_of_offset(offset);
                                b.line = line;
                                b.offset = offset;
                                (b.line, b)
                            })
                            .collect();
                    });
                }
            });
        }
    }
}

// TODO(floem-editor): when we split it out, export a type alias with the default generic
/// A single document that can be viewed by multiple [`EditorData`]'s
/// [`EditorViewData`]s and [`EditorView]s.
#[derive(Clone)]
pub struct Document<B: Backend = DocBackend> {
    pub scope: Scope,
    pub buffer_id: BufferId,
    pub content: RwSignal<DocContent>,
    pub cache_rev: RwSignal<u64>,
    /// Whether the buffer's content has been loaded/initialized into the buffer.
    pub loaded: RwSignal<bool>,
    pub buffer: RwSignal<Buffer>,
    /// ime preedit information
    pub preedit: RwSignal<Option<Preedit>>,
    /// The text layouts for the document. This may be shared with other views.
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    /// A cache for the sticky headers which maps a line to the lines it should show in the header.
    pub sticky_headers: Rc<RefCell<HashMap<usize, Option<Vec<usize>>>>>,
    pub backend: B,
}
impl Document<DocBackend> {
    // TODO(floem-editor): These are wrapper shims to avoid needing to call `DocumentBackend::from
    // (common)` at every current caller site in Lapce.
    // In floem-editor, the `new_backend` should simply be the `new` functions since it wouldn't be
    // able to infer what backend instance to use anyway
    //    (ignoring special functions for backends that impl `Default`)
    // An annoyance is that once floem-editor is its own crate, we can't implement fns on the
    // `Document` type for ourselves, and a newtype would require even more boilerplate.
    // So once we do that, we likely swap these to a couple utility functions.
    pub fn new(
        cx: Scope,
        path: PathBuf,
        diagnostics: DiagnosticData,
        common: Rc<CommonData>,
    ) -> Self {
        let syntax = Syntax::init(&path);
        Self::new_backend(
            cx,
            path,
            DocBackend::new(cx, syntax, Some(diagnostics), common.clone()),
        )
    }

    pub fn new_local(cx: Scope, common: Rc<CommonData>) -> Self {
        let syntax = Syntax::plaintext();
        Self::new_local_backend(
            cx,
            DocBackend::new(cx, syntax, None, common.clone()),
        )
    }

    pub fn new_content(
        cx: Scope,
        content: DocContent,
        common: Rc<CommonData>,
    ) -> Self {
        let syntax = Syntax::plaintext();
        Self::new_content_backend(
            cx,
            content,
            DocBackend::new(cx, syntax, None, common.clone()),
        )
    }

    pub fn new_hisotry(
        cx: Scope,
        content: DocContent,
        common: Rc<CommonData>,
    ) -> Self {
        let syntax = if let DocContent::History(history) = &content {
            Syntax::init(&history.path)
        } else {
            Syntax::plaintext()
        };
        Self::new_hisotry_backend(
            cx,
            content,
            DocBackend::new(cx, syntax, None, common.clone()),
        )
    }
}
impl<B: Backend + 'static> Document<B> {
    pub fn new_backend(cx: Scope, path: PathBuf, backend: B) -> Self {
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(DocContent::File {
                path,
                read_only: false,
            }),
            loaded: cx.create_rw_signal(false),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            preedit: cx.create_rw_signal(None),
            backend,
        }
    }

    pub fn new_local_backend(cx: Scope, backend: B) -> Self {
        Self::new_content_backend(cx, DocContent::Local, backend)
    }

    pub fn new_content_backend(cx: Scope, content: DocContent, backend: B) -> Self {
        let cx = cx.create_child();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            loaded: cx.create_rw_signal(true),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            preedit: cx.create_rw_signal(None),
            backend,
        }
    }

    pub fn new_hisotry_backend(cx: Scope, content: DocContent, backend: B) -> Self {
        let cx = cx.create_child();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            loaded: cx.create_rw_signal(true),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            preedit: cx.create_rw_signal(None),
            backend,
        }
    }

    /// Whether or not the underlying buffer is loaded
    pub fn loaded(&self) -> bool {
        self.loaded.get_untracked()
    }

    //// Initialize the content with some text, this marks the document as loaded.
    pub fn init_content(&self, content: Rope) {
        batch(|| {
            self.buffer.update(|buffer| {
                buffer.init_content(content);
            });
            B::pre_update_init_content(self);
            self.loaded.set(true);
            self.on_update(None);
            // Call the backend's init from within the batch, this ensures that any effects
            // depending on loaded/content/etc don't update until we're all done.
            B::init_content(self);
        });
    }

    /// Reload the document's content, and is what you should typically use when you want to *set*
    /// an existing document's content.
    pub fn reload(&self, content: Rope, set_pristine: bool) {
        // self.code_actions.clear();
        // self.inlay_hints = None;
        let delta = self
            .buffer
            .try_update(|buffer| buffer.reload(content, set_pristine))
            .unwrap();
        self.apply_deltas(&[delta]);
    }

    pub fn handle_file_changed(&self, content: Rope) {
        if self.is_pristine() {
            self.reload(content, true);
        }
    }

    pub fn do_insert(
        &self,
        cursor: &mut Cursor,
        s: &str,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        if self.content.with_untracked(|c| c.read_only()) {
            return Vec::new();
        }

        let auto_closing_matching_pairs = B::auto_closing_matching_pairs(self);
        let auto_surround = B::auto_surround(self);

        let old_cursor = cursor.mode.clone();
        let deltas = self
            .buffer
            .try_update(|buffer| {
                Editor::insert(
                    cursor,
                    buffer,
                    s,
                    &|buffer, c, offset| {
                        self.backend.previous_unmatched(buffer, c, offset)
                    },
                    auto_closing_matching_pairs,
                    auto_surround,
                )
            })
            .unwrap();
        // Keep track of the change in the cursor mode for undo/redo
        self.buffer.update(|buffer| {
            buffer.set_cursor_before(old_cursor);
            buffer.set_cursor_after(cursor.mode.clone());
        });
        self.apply_deltas(&deltas);
        deltas
    }

    pub fn do_raw_edit(
        &self,
        edits: &[(impl AsRef<Selection>, &str)],
        edit_type: EditType,
    ) -> Option<(RopeDelta, InvalLines, SyntaxEdit)> {
        if self.content.with_untracked(|c| c.read_only()) {
            return None;
        }
        let (delta, inval_lines, edits) = self
            .buffer
            .try_update(|buffer| buffer.edit(edits, edit_type))
            .unwrap();
        self.apply_deltas(&[(delta.clone(), inval_lines.clone(), edits.clone())]);
        Some((delta, inval_lines, edits))
    }

    pub fn do_edit(
        &self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
        smart_tab: bool,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        if self.content.with_untracked(|c| c.read_only())
            && !cmd.not_changing_buffer()
        {
            return Vec::new();
        }

        let mut clipboard = SystemClipboard::new();
        let old_cursor = cursor.mode.clone();
        let comment_token = B::comment_token(self);
        let deltas = self
            .buffer
            .try_update(|buffer| {
                Editor::do_edit(
                    cursor,
                    buffer,
                    cmd,
                    comment_token,
                    &mut clipboard,
                    modal,
                    register,
                    smart_tab,
                )
            })
            .unwrap();

        if !deltas.is_empty() {
            self.buffer.update(|buffer| {
                buffer.set_cursor_before(old_cursor);
                buffer.set_cursor_after(cursor.mode.clone());
            });
        }

        self.apply_deltas(&deltas);
        deltas
    }

    pub fn apply_deltas(&self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        let rev = self.rev() - deltas.len() as u64;
        batch(|| {
            for (i, (delta, inval, _)) in deltas.iter().enumerate() {
                let rev = rev + i as u64 + 1;
                B::apply_delta(self, rev, delta, inval);
            }
        });

        // TODO(minor): We could avoid this potential allocation since most apply_delta callers are actually using a Vec
        // which we could reuse.
        // We use a smallvec because there is unlikely to be more than a couple of deltas
        let edits: SmallVec<[SyntaxEdit; 3]> =
            deltas.iter().map(|(_, _, edits)| edits.clone()).collect();
        self.on_update(Some(&edits));
    }

    pub fn is_pristine(&self) -> bool {
        self.buffer.with_untracked(|b| b.is_pristine())
    }

    /// Get the buffer's current revision. This is used to track whether the buffer has changed.
    pub fn rev(&self) -> u64 {
        self.buffer.with_untracked(|b| b.rev())
    }

    fn on_update(&self, edits: Option<&[SyntaxEdit]>) {
        batch(|| {
            self.clear_style_cache();
            self.clear_sticky_headers_cache();
            self.check_auto_save();
            B::on_update(self, edits);
        });
    }

    fn check_auto_save(&self) {
        let Some(autosave_interval) = B::autosave_interval(self) else {
            return;
        };

        if !self.content.with_untracked(|c| c.is_file()) {
            return;
        };
        let rev = self.rev();
        let doc = self.clone();
        exec_after(autosave_interval, move |_| {
            let current_rev = match doc
                .buffer
                .try_with_untracked(|b| b.as_ref().map(|b| b.rev()))
            {
                Some(rev) => rev,
                None => return,
            };

            if current_rev != rev || doc.is_pristine() {
                return;
            }

            doc.save(|| {});
        });
    }

    fn clear_style_cache(&self) {
        self.clear_text_cache();
        B::clear_style_cache(self);
    }

    pub fn set_preedit(
        &self,
        text: String,
        cursor: Option<(usize, usize)>,
        offset: usize,
    ) {
        self.preedit.set(Some(Preedit {
            text,
            cursor,
            offset,
        }));
        self.clear_text_cache();
    }

    pub fn clear_preedit(&self) {
        if self.preedit.get_untracked().is_some() {
            self.preedit.set(None);
            self.clear_text_cache();
        }
    }

    /// Get the phantom text for the preedit.  
    /// This should be included in the [`LineStyling`]'s returned [`PhantomTextLine`] to support IME
    pub fn preedit_phantom_text(
        &self,
        under_line: Option<Color>,
        line: usize,
    ) -> Option<PhantomText> {
        let preedit = self.preedit.get_untracked()?;

        let (ime_line, col) = self
            .buffer
            .with_untracked(|b| b.offset_to_line_col(preedit.offset));

        if line != ime_line {
            return None;
        }

        Some(PhantomText {
            kind: PhantomTextKind::Ime,
            text: preedit.text,
            col,
            font_size: None,
            fg: None,
            bg: None,
            under_line,
        })
    }

    /// Inform any dependents on this document that they should clear any cached text.
    pub fn clear_text_cache(&self) {
        self.cache_rev.try_update(|cache_rev| {
            *cache_rev += 1;

            // Update the text layouts within the callback so that those alerted to cache rev
            // will see the now empty layouts.
            self.text_layouts.borrow_mut().clear(*cache_rev, None);
        });
    }

    fn clear_sticky_headers_cache(&self) {
        self.sticky_headers.borrow_mut().clear();
    }

    pub fn line_styling(&self, line: usize) -> B::LineStyling {
        B::line_styling(self, line)
    }

    /// Get the phantom text for a given line  
    /// If using other style information, prefer using [`Self::line_styling`]
    pub fn line_phantom_text(&self, line: usize) -> PhantomTextLine {
        B::line_styling(self, line).phantom_text()
    }

    /// Get the sticky headers for a particular line, creating them if necessary.
    pub fn sticky_headers(&self, line: usize) -> Option<Vec<usize>> {
        if let Some(lines) = self.sticky_headers.borrow().get(&line) {
            return lines.clone();
        }

        let lines = B::sticky_headers(self, line);
        self.sticky_headers.borrow_mut().insert(line, lines.clone());
        lines
    }

    /// Create rendable whitespace layout by creating a new text layout
    /// with invisible spaces and special utf8 characters that display
    /// the different white space characters.
    fn new_whitespace_layout(
        line_content: &str,
        text_layout: &TextLayout,
        phantom: &PhantomTextLine,
        render_whitespace: RenderWhitespace,
    ) -> Option<Vec<(char, (f64, f64))>> {
        let mut render_leading = false;
        let mut render_boundary = false;
        let mut render_between = false;

        // TODO: render whitespaces only on highlighted text
        match render_whitespace {
            RenderWhitespace::All => {
                render_leading = true;
                render_boundary = true;
                render_between = true;
            }
            RenderWhitespace::Boundary => {
                render_leading = true;
                render_boundary = true;
            }
            RenderWhitespace::Trailing => {} // All configs include rendering trailing whitespace
            RenderWhitespace::None => return None,
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
        let line_content_original = self
            .buffer
            .with_untracked(|b| b.line_content(line).to_string());

        let styling = B::line_styling(self, line);
        let font_size = styling.font_size();

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
        let phantom_text = styling.phantom_text();
        let line_content = phantom_text.combine_with_text(line_content);

        let family = styling.font_family();
        let attrs = Attrs::new()
            .color(styling.color())
            .family(&family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Px(styling.line_height()));
        let mut attrs_list = AttrsList::new(attrs);

        // Apply various styles to the line's text based on our semantic/syntax highlighting
        let styles = styling.line_style();
        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = styling.style_color(fg_color) {
                    let start = phantom_text.col_at(line_style.start);
                    let end = phantom_text.col_at(line_style.end);
                    attrs_list.add_span(start..end, attrs.color(fg_color));
                }
            }
        }

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
        text_layout.set_tab_width(styling.tab_width());
        text_layout.set_text(&line_content, attrs_list);

        let whitespaces = Self::new_whitespace_layout(
            line_content_original,
            &text_layout,
            &phantom_text,
            styling.render_whitespace(),
        );

        let indent_line = B::indent_line(self, line, line_content_original);

        let indent = if indent_line != line {
            self.get_text_layout(indent_line, font_size).indent + 1.0
        } else {
            let (_, col) = self.buffer.with_untracked(|buffer| {
                let offset = buffer.first_non_blank_character_on_line(indent_line);
                buffer.offset_to_line_col(offset)
            });
            text_layout.hit_position(col).point.x
        };

        TextLayoutLine {
            text: text_layout,
            extra_style: Vec::new(),
            whitespaces,
            indent,
        }
    }

    pub fn apply_styles(&self, line: usize, text_layout_line: &mut TextLayoutLine) {
        B::apply_styles(self, line, text_layout_line)
    }

    /// Get the text layout for the given line.
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout(
        &self,
        line: usize,
        font_size: usize,
    ) -> Arc<TextLayoutLine> {
        let config_id = B::config_id(self);
        // Check if the text layout needs to update due to the config being changed
        self.text_layouts.borrow_mut().check_attributes(config_id);
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

    pub fn save(&self, after_action: impl Fn() + 'static) {
        B::save(self, move |_| after_action());
    }
}

fn extra_styles_for_range(
    text_layout: &TextLayout,
    start: usize,
    end: usize,
    bg_color: Option<Color>,
    under_line: Option<Color>,
    wave_line: Option<Color>,
) -> impl Iterator<Item = LineExtraStyle> + '_ {
    let start_hit = text_layout.hit_position(start);
    let end_hit = text_layout.hit_position(end);

    text_layout
        .layout_runs()
        .enumerate()
        .filter_map(move |(current_line, run)| {
            if current_line < start_hit.line || current_line > end_hit.line {
                return None;
            }

            let x = if current_line == start_hit.line {
                start_hit.point.x
            } else {
                run.glyphs.first().map(|g| g.x).unwrap_or(0.0) as f64
            };
            let end_x = if current_line == end_hit.line {
                end_hit.point.x
            } else {
                run.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0) as f64
            };
            let width = end_x - x;

            if width == 0.0 {
                return None;
            }

            let y = (run.line_height - run.glyph_ascent - run.glyph_descent) as f64
                / 2.0;
            let height = (run.glyph_ascent + run.glyph_descent) as f64;

            Some(LineExtraStyle {
                x,
                y,
                width: Some(width),
                height,
                bg_color,
                under_line,
                wave_line,
            })
        })
}
