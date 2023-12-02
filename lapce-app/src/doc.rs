use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{atomic, Arc},
    time::Duration,
};

use floem::{
    action::exec_after,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
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

/// A single document that can be viewed by multiple [`EditorData`]'s
/// [`EditorViewData`]s and [`EditorView]s.
#[derive(Clone)]
pub struct Document {
    pub scope: Scope,
    pub buffer_id: BufferId,
    pub content: RwSignal<DocContent>,
    pub cache_rev: RwSignal<u64>,
    /// Whether the buffer's content has been loaded/initialized into the buffer.
    pub loaded: RwSignal<bool>,
    pub buffer: RwSignal<Buffer>,
    pub syntax: RwSignal<Syntax>,
    /// Semantic highlighting information (which is provided by the LSP)
    semantic_styles: RwSignal<Option<Spans<Style>>>,
    /// Inlay hints for the document
    pub inlay_hints: RwSignal<Option<Spans<InlayHint>>>,
    /// Current completion lens text, if any.
    /// This will be displayed even on views that are not focused.
    pub completion_lens: RwSignal<Option<String>>,
    /// (line, col)
    pub completion_pos: RwSignal<(usize, usize)>,
    /// ime preedit information
    pub preedit: RwSignal<Option<Preedit>>,
    /// (Offset -> (Plugin the code actions are from, Code Actions))
    pub code_actions:
        RwSignal<im::HashMap<usize, Arc<(PluginId, CodeActionResponse)>>>,
    /// Stores information about different versions of the document from source control.
    histories: RwSignal<im::HashMap<String, DocumentHistory>>,
    pub head_changes: RwSignal<im::Vector<DiffLines>>,
    line_styles: Rc<RefCell<LineStyles>>,
    /// The text layouts for the document. This may be shared with other views.
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    /// A cache for the sticky headers which maps a line to the lines it should show in the header.
    pub sticky_headers: Rc<RefCell<HashMap<usize, Option<Vec<usize>>>>>,
    pub find_result: FindResult,
    /// The diagnostics for the document
    pub diagnostics: DiagnosticData,
    common: Rc<CommonData>,
}

impl Document {
    pub fn new(
        cx: Scope,
        path: PathBuf,
        diagnostics: DiagnosticData,
        common: Rc<CommonData>,
    ) -> Self {
        let syntax = Syntax::init(&path);
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            syntax: cx.create_rw_signal(syntax),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics,
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            content: cx.create_rw_signal(DocContent::File {
                path,
                read_only: false,
            }),
            loaded: cx.create_rw_signal(false),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            preedit: cx.create_rw_signal(None),
            common,
        }
    }

    pub fn new_local(cx: Scope, common: Rc<CommonData>) -> Self {
        Self::new_content(cx, DocContent::Local, common)
    }

    pub fn new_content(
        cx: Scope,
        content: DocContent,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = cx.create_child();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            syntax: cx.create_rw_signal(Syntax::plaintext()),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
            },
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            loaded: cx.create_rw_signal(true),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            preedit: cx.create_rw_signal(None),
            common,
        }
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
        let cx = cx.create_child();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            syntax: cx.create_rw_signal(syntax),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
            },
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            loaded: cx.create_rw_signal(true),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            preedit: cx.create_rw_signal(None),
            common,
        }
    }

    pub fn set_syntax(&self, syntax: Syntax) {
        batch(|| {
            self.syntax.set(syntax);
            if self.semantic_styles.with_untracked(|s| s.is_none()) {
                self.clear_style_cache();
            }
            self.clear_sticky_headers_cache();
        });
    }

    /// Set the syntax highlighting this document should use.
    pub fn set_language(&self, language: LapceLanguage) {
        self.syntax.set(Syntax::from_language(language));
    }

    pub fn find(&self) -> &Find {
        &self.common.find
    }

    /// Whether or not the underlying buffer is loaded
    pub fn loaded(&self) -> bool {
        self.loaded.get_untracked()
    }

    //// Initialize the content with some text, this marks the document as loaded.
    pub fn init_content(&self, content: Rope) {
        batch(|| {
            self.syntax.with_untracked(|syntax| {
                self.buffer.update(|buffer| {
                    buffer.init_content(content);
                    buffer.detect_indent(syntax);
                });
            });
            self.loaded.set(true);
            self.on_update(None);
            self.init_diagnostics();
            self.retrieve_head();
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
        config: &LapceConfig,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        if self.content.with_untracked(|c| c.read_only()) {
            return Vec::new();
        }

        let old_cursor = cursor.mode.clone();
        let deltas = self.syntax.with_untracked(|syntax| {
            self.buffer
                .try_update(|buffer| {
                    Editor::insert(
                        cursor,
                        buffer,
                        s,
                        syntax,
                        config.editor.auto_closing_matching_pairs,
                        config.editor.auto_surround,
                    )
                })
                .unwrap()
        });
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
        let deltas = self.syntax.with_untracked(|syntax| {
            self.buffer
                .try_update(|buffer| {
                    Editor::do_edit(
                        cursor,
                        buffer,
                        cmd,
                        syntax,
                        &mut clipboard,
                        modal,
                        register,
                        smart_tab,
                    )
                })
                .unwrap()
        });

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
                self.update_styles(delta);
                self.update_inlay_hints(delta);
                self.update_diagnostics(delta);
                self.update_completion_lens(delta);
                self.update_find_result(delta);
                if let DocContent::File { path, .. } = self.content.get_untracked() {
                    self.update_breakpoints(delta, &path, &inval.old_text);
                    self.common.proxy.update(
                        path,
                        delta.clone(),
                        rev + i as u64 + 1,
                    );
                }
            }
        });

        // TODO(minor): We could avoid this potential allocation since most apply_delta callers are actually using a Vec
        // which we could reuse.
        // We use a smallvec because there is unlikely to be more than a couple of deltas
        let edits = deltas.iter().map(|(_, _, edits)| edits.clone()).collect();
        self.on_update(Some(edits));
    }

    pub fn is_pristine(&self) -> bool {
        self.buffer.with_untracked(|b| b.is_pristine())
    }

    /// Get the buffer's current revision. This is used to track whether the buffer has changed.
    pub fn rev(&self) -> u64 {
        self.buffer.with_untracked(|b| b.rev())
    }

    fn on_update(&self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        batch(|| {
            self.clear_code_actions();
            self.clear_style_cache();
            self.trigger_syntax_change(edits);
            self.clear_sticky_headers_cache();
            self.trigger_head_change();
            self.check_auto_save();
            self.get_semantic_styles();
            self.get_inlay_hints();
            self.find_result.reset();
        });
    }

    fn check_auto_save(&self) {
        let config = self.common.config.get_untracked();
        if config.editor.autosave_interval > 0 {
            if !self.content.with_untracked(|c| c.is_file()) {
                return;
            };
            let rev = self.rev();
            let doc = self.clone();
            exec_after(
                Duration::from_millis(config.editor.autosave_interval),
                move |_| {
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
                },
            );
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

    pub fn trigger_syntax_change(&self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        let (rev, text) =
            self.buffer.with_untracked(|b| (b.rev(), b.text().clone()));

        self.syntax.update(|syntax| {
            syntax.parse(rev, text, edits.as_deref());
        });
    }

    fn clear_style_cache(&self) {
        self.line_styles.borrow_mut().clear();
        self.clear_text_cache();
    }

    fn clear_code_actions(&self) {
        self.code_actions.update(|c| {
            c.clear();
        });
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

    /// Get the active style information, either the semantic styles or the
    /// tree-sitter syntax styles.
    fn styles(&self) -> Option<Spans<Style>> {
        if let Some(semantic_styles) = self.semantic_styles.get_untracked() {
            Some(semantic_styles)
        } else {
            self.syntax.with_untracked(|syntax| syntax.styles.clone())
        }
    }

    /// Get the style information for the particular line from semantic/syntax highlighting.
    /// This caches the result if possible.
    pub fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        if self.line_styles.borrow().get(&line).is_none() {
            let styles = self.styles();

            let line_styles = styles
                .map(|styles| {
                    let text =
                        self.buffer.with_untracked(|buffer| buffer.text().clone());
                    line_styles(&text, line, &styles)
                })
                .unwrap_or_default();
            self.line_styles
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        self.line_styles.borrow().get(&line).cloned().unwrap()
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

        let syntactic_styles =
            self.syntax.with_untracked(|syntax| syntax.styles.clone());

        let doc = self.clone();
        let send = create_ext_action(self.scope, move |styles| {
            if doc.buffer.with_untracked(|b| b.rev()) == rev {
                doc.semantic_styles.set(Some(styles));
                doc.clear_style_cache();
            }
        });

        self.common.proxy.get_semantic_tokens(path, move |result| {
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

                    send(styles);
                });
            }
        });
    }

    /// Request inlay hints for the buffer from the LSP through the proxy.
    fn get_inlay_hints(&self) {
        if !self.loaded() {
            return;
        }

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
                doc.inlay_hints.set(Some(hints));
                doc.clear_text_cache();
            }
        });

        self.common.proxy.get_inlay_hints(path, move |result| {
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
        let config = self.common.config.get_untracked();

        let (start_offset, end_offset) = self.buffer.with_untracked(|buffer| {
            (buffer.offset_of_line(line), buffer.offset_of_line(line + 1))
        });

        let inlay_hints = self.inlay_hints.get_untracked();
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

                let col = self.buffer.with_untracked(|buffer| {
                    buffer.offset_of_line(line + 1) - buffer.offset_of_line(line)
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

                    *config.get_color(theme_prop)
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

        let (completion_line, completion_col) = self.completion_pos.get_untracked();
        let completion_text = config
            .editor
            .enable_completion_lens
            .then_some(())
            .and(self.completion_lens.get_untracked())
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

        if let Some(preedit) = self.preedit.get_untracked() {
            let (ime_line, col) = self
                .buffer
                .with_untracked(|b| b.offset_to_line_col(preedit.offset));
            if line == ime_line {
                text.push(PhantomText {
                    kind: PhantomTextKind::Ime,
                    text: preedit.text,
                    col,
                    font_size: None,
                    fg: None,
                    bg: None,
                    under_line: Some(
                        *self
                            .common
                            .config
                            .get_untracked()
                            .get_color(LapceColor::EDITOR_FOREGROUND),
                    ),
                });
            }
        }

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

    /// init diagnostics offset ranges from lsp positions
    pub fn init_diagnostics(&self) {
        self.clear_text_cache();
        self.clear_code_actions();
        self.diagnostics.diagnostics.update(|diagnostics| {
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

    /// Get the current completion lens text
    pub fn completion_lens(&self) -> Option<String> {
        self.completion_lens.get_untracked()
    }

    pub fn set_completion_lens(
        &self,
        completion_lens: String,
        line: usize,
        col: usize,
    ) {
        // TODO: more granular invalidation
        self.clear_text_cache();
        self.completion_lens.set(Some(completion_lens));
        self.completion_pos.set((line, col));
    }

    pub fn clear_completion_lens(&self) {
        // TODO: more granular invalidation
        if self.completion_lens.get_untracked().is_some() {
            self.clear_text_cache();
            self.completion_lens.set(None);
        }
    }

    fn update_find_result(&self, delta: &RopeDelta) {
        self.find_result.occurrences.update(|s| {
            *s = s.apply_delta(delta, true, InsertDrift::Default);
        })
    }

    fn update_breakpoints(&self, delta: &RopeDelta, path: &Path, old_text: &Rope) {
        if self
            .common
            .breakpoints
            .with_untracked(|breakpoints| breakpoints.contains_key(path))
        {
            self.common.breakpoints.update(|breakpoints| {
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

    /// Update the completion lens position after an edit so that it appears in the correct place.
    pub fn update_completion_lens(&self, delta: &RopeDelta) {
        let Some(completion) = self.completion_lens.get_untracked() else {
            return;
        };

        let (line, col) = self.completion_pos.get_untracked();
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
                self.completion_lens
                    .set(Some(completion[new_len..].to_string()));
            }
        }

        // Shift the position by the rope delta
        let mut transformer = Transformer::new(delta);

        let new_offset = transformer.transform(offset, true);
        let new_pos = self
            .buffer
            .with_untracked(|b| b.offset_to_line_col(new_offset));

        self.completion_pos.set(new_pos);
    }

    pub fn update_find(&self) {
        let find_rev = self.common.find.rev.get_untracked();
        if self.find_result.find_rev.get_untracked() != find_rev {
            if self
                .common
                .find
                .search_string
                .with_untracked(|search_string| {
                    search_string
                        .as_ref()
                        .map(|s| s.content.is_empty())
                        .unwrap_or(true)
                })
            {
                self.find_result.occurrences.set(Selection::new());
            }
            self.find_result.reset();
            self.find_result.find_rev.set(find_rev);
        }

        if self.find_result.progress.get_untracked() != FindProgress::Started {
            return;
        }

        let search = self.common.find.search_string.get_untracked();
        let search = match search {
            Some(search) => search,
            None => return,
        };
        if search.content.is_empty() {
            return;
        }

        self.find_result
            .progress
            .set(FindProgress::InProgress(Selection::new()));

        let find_result = self.find_result.clone();
        let send = create_ext_action(self.scope, move |occurrences| {
            find_result.occurrences.set(occurrences);
            find_result.progress.set(FindProgress::Ready);
        });

        let text = self.buffer.with_untracked(|b| b.text().clone());
        let case_matching = self.common.find.case_matching.get_untracked();
        let whole_words = self.common.find.whole_words.get_untracked();
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

    /// Get the sticky headers for a particular line, creating them if necessary.
    pub fn sticky_headers(&self, line: usize) -> Option<Vec<usize>> {
        if let Some(lines) = self.sticky_headers.borrow().get(&line) {
            return lines.clone();
        }
        let lines = self.buffer.with_untracked(|buffer| {
            let offset = buffer.offset_of_line(line + 1);
            self.syntax.with_untracked(|syntax| {
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
        });
        self.sticky_headers.borrow_mut().insert(line, lines.clone());
        lines
    }

    /// Retrieve the `head` version of the buffer
    pub fn retrieve_head(&self) {
        if let DocContent::File { path, .. } = self.content.get_untracked() {
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
            let proxy = self.common.proxy.clone();
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

        let rev = self.rev();
        let left_rope = history;
        let (atomic_rev, right_rope) = self
            .buffer
            .with_untracked(|b| (b.atomic_rev(), b.text().clone()));

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
        let config = self.common.config.get_untracked();
        let line_content_original = self
            .buffer
            .with_untracked(|b| b.line_content(line).to_string());
        let line_height = config.editor.line_height();

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
            .font_size(config.editor.font_size() as f32)
            .line_height(LineHeightValue::Px(line_height as f32));
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
        text_layout.set_tab_width(config.editor.tab_width);
        text_layout.set_text(&line_content, attrs_list);

        let whitespaces = Self::new_whitespace_layout(
            line_content_original,
            &text_layout,
            &phantom_text,
            &config,
        );

        let indent_line = if line_content_original.trim().is_empty() {
            let offset = self.buffer.with_untracked(|b| b.offset_of_line(line));
            if let Some(offset) =
                self.syntax.with_untracked(|s| s.parent_offset(offset))
            {
                self.buffer.with_untracked(|b| b.line_of_offset(offset))
            } else {
                line
            }
        } else {
            line
        };

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

    /// Get the extra styles for the given text layout
    pub fn create_styles(&self, line: usize, text_layout_line: &mut TextLayoutLine) {
        let config = self.common.config.get_untracked();

        text_layout_line.extra_style.clear();
        let text_layout = &text_layout_line.text;

        let phantom_text = self.line_phantom_text(line);

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
        if let Some(max_severity) = phantom_text.max_severity {
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
                bg_color: Some(*config.get_color(theme_prop)),
                under_line: None,
                wave_line: None,
            });
        }

        self.diagnostics.diagnostics.with_untracked(|diags| {
            self.buffer.with_untracked(|buffer| {
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
                        let color = *config.get_color(color_name);

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

    /// Get the text layout for the given line.
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout(
        &self,
        line: usize,
        font_size: usize,
    ) -> Arc<TextLayoutLine> {
        let config = self.common.config.get_untracked();
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

    pub fn save(&self, after_action: impl Fn() + 'static) {
        let content = self.content.get_untracked();
        if let DocContent::File { path, .. } = content {
            let rev = self.rev();
            let buffer = self.buffer;
            let send = create_ext_action(self.scope, move |result| {
                if let Ok(ProxyResponse::SaveResponse {}) = result {
                    let current_rev = buffer.with_untracked(|buffer| buffer.rev());
                    if current_rev == rev {
                        buffer.update(|buffer| {
                            buffer.set_pristine();
                        });
                        after_action();
                    }
                }
            });

            self.common.proxy.save(rev, path, true, move |result| {
                send(result);
            })
        }
    }

    /// Returns the offsets of the brackets enclosing the given offset.
    /// Uses a language aware algorithm if syntax support is available for the current language,
    /// else falls back to a language unaware algorithm.
    pub fn find_enclosing_brackets(&self, offset: usize) -> Option<(usize, usize)> {
        self.syntax
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

            let y = (run.line_y - run.line_height + run.glyph_descent) as f64;
            let height = run.line_height as f64;

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
