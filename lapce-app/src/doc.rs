use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{self, AtomicUsize},
    },
    time::Duration,
};

use floem::{
    ViewId,
    action::exec_after,
    ext_event::create_ext_action,
    keyboard::Modifiers,
    peniko::Color,
    reactive::{
        ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, batch,
    },
    text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    views::editor::{
        CursorInfo, Editor, EditorStyle,
        actions::CommonAction,
        command::{Command, CommandExecuted},
        id::EditorId,
        layout::{LineExtraStyle, TextLayoutLine},
        phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine},
        text::{Document, DocumentPhantom, PreeditData, Styling, SystemClipboard},
        view::{ScreenLines, ScreenLinesBase},
    },
};
use itertools::Itertools;
use lapce_core::{
    buffer::{
        Buffer, InvalLines,
        diff::{DiffLines, rope_diff},
        rope_text::RopeText,
    },
    char_buffer::CharBuffer,
    command::EditCommand,
    cursor::{Cursor, CursorAffinity},
    editor::{Action, EditConf, EditType},
    indent::IndentStyle,
    language::LapceLanguage,
    line_ending::LineEnding,
    mode::MotionMode,
    register::Register,
    rope_text_pos::RopeTextPosition,
    selection::{InsertDrift, Selection},
    style::line_styles,
    syntax::{BracketParser, Syntax, edit::SyntaxEdit},
    word::{CharClassification, WordCursor, get_char_property},
};
use lapce_rpc::{
    buffer::BufferId,
    plugin::PluginId,
    proxy::ProxyResponse,
    style::{LineStyle, LineStyles, Style},
};
use lapce_xi_rope::{
    Interval, Rope, RopeDelta, Transformer,
    spans::{Spans, SpansBuilder},
};
use lsp_types::{
    CodeActionOrCommand, CodeLens, Diagnostic, DiagnosticSeverity,
    DocumentSymbolResponse, InlayHint, InlayHintLabel, TextEdit,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    command::{CommandKind, LapceCommand},
    config::{LapceConfig, color::LapceColor},
    editor::{EditorData, compute_screen_lines, gutter::FoldingRanges},
    find::{Find, FindProgress, FindResult},
    history::DocumentHistory,
    keypress::KeyPressFocus,
    main_split::Editors,
    panel::{
        document_symbol::{SymbolData, SymbolInformationItemData},
        kind::PanelKind,
    },
    window_tab::{CommonData, Focus},
    workspace::LapceWorkspace,
};

#[derive(Clone, Debug)]
pub struct DiagnosticData {
    pub expanded: RwSignal<bool>,
    pub diagnostics: RwSignal<im::Vector<Diagnostic>>,
    pub diagnostics_span: RwSignal<Spans<Diagnostic>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorDiagnostic {
    pub range: Option<(usize, usize)>,
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

/// (Offset -> (Plugin the code actions are from, Code Actions))
pub type CodeActions =
    im::HashMap<usize, (PluginId, im::Vector<CodeActionOrCommand>)>;

pub type AllCodeLens = im::HashMap<usize, (PluginId, usize, im::Vector<CodeLens>)>;

#[derive(Clone)]
pub struct Doc {
    pub scope: Scope,
    pub buffer_id: BufferId,
    pub content: RwSignal<DocContent>,
    pub cache_rev: RwSignal<u64>,
    /// Whether the buffer's content has been loaded/initialized into the buffer.
    pub loaded: RwSignal<bool>,
    pub buffer: RwSignal<Buffer>,
    pub syntax: RwSignal<Syntax>,
    semantic_styles: RwSignal<Option<Spans<Style>>>,
    /// Inlay hints for the document
    pub inlay_hints: RwSignal<Option<Spans<InlayHint>>>,
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

    pub code_lens: RwSignal<AllCodeLens>,

    pub folding_ranges: RwSignal<FoldingRanges>,

    /// Stores information about different versions of the document from source control.
    histories: RwSignal<im::HashMap<String, DocumentHistory>>,
    pub head_changes: RwSignal<im::Vector<DiffLines>>,

    line_styles: Rc<RefCell<LineStyles>>,
    pub parser: Rc<RefCell<BracketParser>>,

    /// A cache for the sticky headers which maps a line to the lines it should show in the header.
    pub sticky_headers: Rc<RefCell<HashMap<usize, Option<Vec<usize>>>>>,

    pub preedit: PreeditData,

    pub find_result: FindResult,

    /// The diagnostics for the document
    pub diagnostics: DiagnosticData,

    editors: Editors,
    pub common: Rc<CommonData>,

    pub document_symbol_data: RwSignal<Option<SymbolData>>,
}
impl Doc {
    pub fn new(
        cx: Scope,
        path: PathBuf,
        diagnostics: DiagnosticData,
        editors: Editors,
        common: Rc<CommonData>,
    ) -> Self {
        let syntax = Syntax::init(&path);
        let config = common.config.get_untracked();
        Doc {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            syntax: cx.create_rw_signal(syntax),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            parser: Rc::new(RefCell::new(BracketParser::new(
                String::new(),
                config.editor.bracket_pair_colorization,
                config.editor.bracket_colorization_limit,
            ))),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics,
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            inline_completion: cx.create_rw_signal(None),
            inline_completion_pos: cx.create_rw_signal((0, 0)),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(DocContent::File {
                path,
                read_only: false,
            }),
            loaded: cx.create_rw_signal(false),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            preedit: PreeditData::new(cx),
            editors,
            common,
            code_lens: cx.create_rw_signal(im::HashMap::new()),
            document_symbol_data: cx.create_rw_signal(None),
            folding_ranges: cx.create_rw_signal(FoldingRanges::default()),
        }
    }

    pub fn new_local(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Doc {
        Self::new_content(cx, DocContent::Local, editors, common)
    }

    pub fn new_content(
        cx: Scope,
        content: DocContent,
        editors: Editors,
        common: Rc<CommonData>,
    ) -> Doc {
        let cx = cx.create_child();
        let config = common.config.get_untracked();
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            syntax: cx.create_rw_signal(Syntax::plaintext()),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            parser: Rc::new(RefCell::new(BracketParser::new(
                String::new(),
                config.editor.bracket_pair_colorization,
                config.editor.bracket_colorization_limit,
            ))),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
                diagnostics_span: cx.create_rw_signal(SpansBuilder::new(0).build()),
            },
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            inline_completion: cx.create_rw_signal(None),
            inline_completion_pos: cx.create_rw_signal((0, 0)),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            loaded: cx.create_rw_signal(true),
            find_result: FindResult::new(cx),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            preedit: PreeditData::new(cx),
            editors,
            common,
            code_lens: cx.create_rw_signal(im::HashMap::new()),
            document_symbol_data: cx.create_rw_signal(None),
            folding_ranges: cx.create_rw_signal(FoldingRanges::default()),
        }
    }

    pub fn new_history(
        cx: Scope,
        content: DocContent,
        editors: Editors,
        common: Rc<CommonData>,
    ) -> Doc {
        let config = common.config.get_untracked();
        let syntax = if let DocContent::History(history) = &content {
            Syntax::init(&history.path)
        } else {
            Syntax::plaintext()
        };
        Self {
            scope: cx,
            buffer_id: BufferId::next(),
            buffer: cx.create_rw_signal(Buffer::new("")),
            syntax: cx.create_rw_signal(syntax),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            parser: Rc::new(RefCell::new(BracketParser::new(
                String::new(),
                config.editor.bracket_pair_colorization,
                config.editor.bracket_colorization_limit,
            ))),
            semantic_styles: cx.create_rw_signal(None),
            inlay_hints: cx.create_rw_signal(None),
            diagnostics: DiagnosticData {
                expanded: cx.create_rw_signal(true),
                diagnostics: cx.create_rw_signal(im::Vector::new()),
                diagnostics_span: cx.create_rw_signal(SpansBuilder::new(0).build()),
            },
            completion_lens: cx.create_rw_signal(None),
            completion_pos: cx.create_rw_signal((0, 0)),
            inline_completion: cx.create_rw_signal(None),
            inline_completion_pos: cx.create_rw_signal((0, 0)),
            cache_rev: cx.create_rw_signal(0),
            content: cx.create_rw_signal(content),
            sticky_headers: Rc::new(RefCell::new(HashMap::new())),
            loaded: cx.create_rw_signal(true),
            histories: cx.create_rw_signal(im::HashMap::new()),
            head_changes: cx.create_rw_signal(im::Vector::new()),
            code_actions: cx.create_rw_signal(im::HashMap::new()),
            find_result: FindResult::new(cx),
            preedit: PreeditData::new(cx),
            editors,
            common,
            code_lens: cx.create_rw_signal(im::HashMap::new()),
            document_symbol_data: cx.create_rw_signal(None),
            folding_ranges: cx.create_rw_signal(FoldingRanges::default()),
        }
    }

    /// Create a styling instance for this doc
    pub fn styling(self: &Rc<Doc>) -> Rc<DocStyling> {
        Rc::new(DocStyling {
            config: self.common.config,
            doc: self.clone(),
        })
    }

    /// Create an [`Editor`] instance from this [`Doc`]. Note that this needs to be registered
    /// appropriately to create the [`EditorData`] and such.
    pub fn create_editor(
        self: &Rc<Doc>,
        cx: Scope,
        id: EditorId,
        is_local: bool,
    ) -> Editor {
        let common = &self.common;
        let config = common.config.get_untracked();
        let modal = config.core.modal && !is_local;

        let register = common.register;
        // TODO: we could have these Rcs created once and stored somewhere, maybe on
        // common, to avoid recreating them everytime.
        let cursor_info = CursorInfo {
            blink_interval: Rc::new(move || config.editor.blink_interval()),
            blink_timer: common.window_common.cursor_blink_timer,
            hidden: common.window_common.hide_cursor,
            should_blink: Rc::new(should_blink(common.focus, common.keyboard_focus)),
        };
        let mut editor =
            Editor::new_direct(cx, id, self.clone(), self.styling(), modal);

        editor.register = register;
        editor.cursor_info = cursor_info;
        editor.ime_allowed = common.window_common.ime_allowed;

        editor.recreate_view_effects();

        editor
    }

    fn editor_data(&self, id: EditorId) -> Option<EditorData> {
        self.editors.editor_untracked(id)
    }

    pub fn syntax(&self) -> ReadSignal<Syntax> {
        self.syntax.read_only()
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
                    buffer.detect_indent(|| {
                        IndentStyle::from_str(syntax.language.indent_unit())
                    });
                });
            });
            self.loaded.set(true);
            self.on_update(None);
            self.init_parser();
            self.init_diagnostics();
            self.retrieve_head();
        });
    }

    fn init_parser(&self) {
        let code = self.buffer.get_untracked().to_string();
        self.syntax.with_untracked(|syntax| {
            if syntax.styles.is_some() {
                self.parser.borrow_mut().update_code(
                    code,
                    &self.buffer.get_untracked(),
                    Some(syntax),
                );
            } else {
                self.parser.borrow_mut().update_code(
                    code,
                    &self.buffer.get_untracked(),
                    None,
                );
            }
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
    ) -> Vec<(Rope, RopeDelta, InvalLines)> {
        if self.content.with_untracked(|c| c.read_only()) {
            return Vec::new();
        }

        let old_cursor = cursor.mode.clone();
        let deltas = self.syntax.with_untracked(|syntax| {
            self.buffer
                .try_update(|buffer| {
                    Action::insert(
                        cursor,
                        buffer,
                        s,
                        &|buffer, c, offset| {
                            syntax_prev_unmatched(buffer, syntax, c, offset)
                        },
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
    ) -> Option<(Rope, RopeDelta, InvalLines)> {
        if self.content.with_untracked(|c| c.read_only()) {
            return None;
        }

        let (text, delta, inval_lines) = self
            .buffer
            .try_update(|buffer| buffer.edit(edits, edit_type))
            .unwrap();
        self.apply_deltas(&[(text.clone(), delta.clone(), inval_lines.clone())]);
        Some((text, delta, inval_lines))
    }

    pub fn do_edit(
        &self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
        smart_tab: bool,
    ) -> Vec<(Rope, RopeDelta, InvalLines)> {
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
                    Action::do_edit(
                        cursor,
                        buffer,
                        cmd,
                        &mut clipboard,
                        register,
                        EditConf {
                            comment_token: syntax.language.comment_token(),
                            modal,
                            smart_tab,
                            keep_indent: true,
                            auto_indent: true,
                        },
                    )
                })
                .unwrap()
        });

        if !deltas.is_empty() {
            self.buffer.update(|buffer| {
                buffer.set_cursor_before(old_cursor);
                buffer.set_cursor_after(cursor.mode.clone());
            });
            self.apply_deltas(&deltas);
        }

        deltas
    }

    pub fn apply_deltas(&self, deltas: &[(Rope, RopeDelta, InvalLines)]) {
        let rev = self.rev() - deltas.len() as u64;
        batch(|| {
            for (i, (_, delta, inval)) in deltas.iter().enumerate() {
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
        let edits: SmallVec<[SyntaxEdit; 3]> = deltas
            .iter()
            .map(|(before_text, delta, _)| {
                SyntaxEdit::from_delta(before_text, delta.clone())
            })
            .collect();
        self.on_update(Some(edits));
    }

    pub fn is_pristine(&self) -> bool {
        self.buffer.with_untracked(|b| b.is_pristine())
    }

    /// Get the buffer's current revision. This is used to track whether the buffer has changed.
    pub fn rev(&self) -> u64 {
        self.buffer.with_untracked(|b| b.rev())
    }

    /// Get the buffer's line-ending.
    /// Note: this may not be the same as what the actual line endings in the file are, rather this
    /// is what the line-ending is set to (and what it will be saved as).
    pub fn line_ending(&self) -> LineEnding {
        self.buffer.with_untracked(|b| b.line_ending())
    }

    fn on_update(&self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        batch(|| {
            self.trigger_syntax_change(edits);
            self.trigger_head_change();
            self.check_auto_save();
            self.get_inlay_hints();
            self.find_result.reset();
            self.get_semantic_styles();
            self.do_bracket_colorization();
            self.clear_code_actions();
            self.clear_style_cache();
            self.get_code_lens();
            self.get_document_symbol();
            self.get_folding_range();
        });
    }

    fn do_bracket_colorization(&self) {
        if self.parser.borrow().active {
            self.syntax.with_untracked(|syntax| {
                if syntax.rev == self.rev() && syntax.styles.is_some() {
                    self.parser.borrow_mut().update_code(
                        self.buffer.get_untracked().to_string(),
                        &self.buffer.get_untracked(),
                        Some(syntax),
                    );
                } else {
                    self.parser.borrow_mut().update_code(
                        self.buffer.get_untracked().to_string(),
                        &self.buffer.get_untracked(),
                        None,
                    );
                }
            })
        }
    }

    pub fn do_text_edit(&self, edits: &[TextEdit]) {
        let edits = self.buffer.with_untracked(|buffer| {
            edits
                .iter()
                .map(|edit| {
                    let selection = lapce_core::selection::Selection::region(
                        buffer.offset_of_position(&edit.range.start),
                        buffer.offset_of_position(&edit.range.end),
                    );
                    (selection, edit.new_text.as_str())
                })
                .collect::<Vec<_>>()
        });
        self.do_raw_edit(&edits, EditType::Completion);
    }

    fn check_auto_save(&self) {
        let config = self.common.config.get_untracked();
        if config.editor.autosave_interval > 0 {
            let Some(path) = self.content.with_untracked(|c| c.path().cloned())
            else {
                return;
            };
            let rev = self.rev();
            let doc = self.clone();
            let scope = self.scope;
            let proxy = self.common.proxy.clone();
            let format = config.editor.format_on_save;
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

                    if format {
                        let send = create_ext_action(scope, move |result| {
                            let current_rev = doc.rev();
                            if current_rev != rev {
                                return;
                            }
                            if let Ok(ProxyResponse::GetDocumentFormatting {
                                edits,
                            }) = result
                            {
                                doc.do_text_edit(&edits);
                            }
                            doc.save(|| {});
                        });
                        proxy.get_document_formatting(path, move |result| {
                            send(result);
                        });
                    } else {
                        doc.save(|| {});
                    }
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

        let doc = self.clone();
        let send = create_ext_action(self.scope, move |syntax| {
            if doc.buffer.with_untracked(|b| b.rev()) == rev {
                doc.syntax.set(syntax);
                doc.do_bracket_colorization();
                doc.clear_style_cache();
                doc.clear_sticky_headers_cache();
            }
        });

        self.syntax.update(|syntax| {
            syntax.cancel_flag.store(1, atomic::Ordering::Relaxed);
            syntax.cancel_flag = Arc::new(AtomicUsize::new(0));
        });
        let mut syntax = self.syntax.get_untracked();
        rayon::spawn(move || {
            syntax.parse(rev, text, edits.as_deref());
            send(syntax);
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

    /// Inform any dependents on this document that they should clear any cached text.
    pub fn clear_text_cache(&self) {
        self.cache_rev.try_update(|cache_rev| {
            *cache_rev += 1;

            // TODO: ???
            // Update the text layouts within the callback so that those alerted to cache rev
            // will see the now empty layouts.
            // self.text_layouts.borrow_mut().clear(*cache_rev, None);
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
    pub fn get_semantic_styles(&self) {
        if !self.loaded() {
            return;
        }

        let path =
            if let DocContent::File { path, .. } = self.content.get_untracked() {
                path
            } else {
                return;
            };

        let (atomic_rev, rev, len) = self
            .buffer
            .with_untracked(|b| (b.atomic_rev(), b.rev(), b.len()));

        let doc = self.clone();
        let send = create_ext_action(self.scope, move |styles| {
            if let Some(styles) = styles {
                if doc.buffer.with_untracked(|b| b.rev()) == rev {
                    doc.semantic_styles.set(Some(styles));
                    doc.clear_style_cache();
                }
            }
        });

        self.common.proxy.get_semantic_tokens(path, move |result| {
            if let Ok(ProxyResponse::GetSemanticTokens { styles }) = result {
                if styles.styles.is_empty() {
                    send(None);
                    return;
                }
                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    send(None);
                    return;
                }
                std::thread::spawn(move || {
                    let mut styles_span = SpansBuilder::new(len);
                    for style in styles.styles {
                        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                            send(None);
                            return;
                        }
                        styles_span.add_span(
                            Interval::new(style.start, style.end),
                            style.style,
                        );
                    }

                    let styles = styles_span.build();
                    send(Some(styles));
                });
            } else {
                send(None);
            }
        });
    }

    pub fn get_code_lens(&self) {
        let cx = self.scope;
        let doc = self.clone();
        self.code_lens.update(|code_lens| {
            code_lens.clear();
        });
        let rev = self.rev();
        if let DocContent::File { path, .. } = doc.content.get_untracked() {
            let send = create_ext_action(cx, move |result| {
                if rev != doc.rev() {
                    return;
                }
                if let Ok(ProxyResponse::GetCodeLensResponse { plugin_id, resp }) =
                    result
                {
                    let Some(codelens) = resp else {
                        return;
                    };
                    doc.code_lens.update(|code_lens| {
                        for codelens in codelens {
                            if codelens.command.is_none() {
                                continue;
                            }
                            let entry = code_lens
                                .entry(codelens.range.start.line as usize)
                                .or_insert_with(|| {
                                    (
                                        plugin_id,
                                        doc.buffer.with_untracked(|b| {
                                            b.offset_of_line(
                                                codelens.range.start.line as usize,
                                            )
                                        }),
                                        im::Vector::new(),
                                    )
                                });
                            entry.2.push_back(codelens);
                        }
                    });
                }
            });
            self.common.proxy.get_code_lens(path, move |result| {
                send(result);
            });
        }
    }

    pub fn get_document_symbol(&self) {
        let cx = self.scope;
        let doc = self.clone();
        let rev = self.rev();
        if let DocContent::File { path, .. } = doc.content.get_untracked() {
            let send = create_ext_action(cx, {
                let path = path.clone();
                move |result| {
                    if rev != doc.rev() {
                        return;
                    }
                    if let Ok(ProxyResponse::GetDocumentSymbols { resp }) = result {
                        let items: Vec<RwSignal<SymbolInformationItemData>> =
                            match resp {
                                DocumentSymbolResponse::Flat(_symbols) => {
                                    Vec::with_capacity(0)
                                }
                                DocumentSymbolResponse::Nested(symbols) => symbols
                                    .into_iter()
                                    .map(|x| {
                                        cx.create_rw_signal(
                                            SymbolInformationItemData::from((x, cx)),
                                        )
                                    })
                                    .collect(),
                            };
                        let symbol_new = Some(SymbolData::new(items, path, cx));
                        doc.document_symbol_data.update(|symbol| {
                            *symbol = symbol_new;
                        });
                    }
                }
            });

            self.common.proxy.get_document_symbols(path, move |result| {
                send(result);
            });
        }
    }

    /// Request inlay hints for the buffer from the LSP through the proxy.
    pub fn get_inlay_hints(&self) {
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

    pub fn diagnostics(&self) -> &DiagnosticData {
        &self.diagnostics
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

        self.diagnostics.diagnostics_span.update(|diagnostics| {
            diagnostics.apply_shape(delta);
        });
    }

    /// init diagnostics offset ranges from lsp positions
    pub fn init_diagnostics(&self) {
        let len = self.buffer.with_untracked(|b| b.len());
        let diagnostics = self.diagnostics.diagnostics.get_untracked();

        let span = self.buffer.with_untracked(|buffer| {
            let mut span = SpansBuilder::new(len);
            for diag in diagnostics.iter() {
                let start = buffer.offset_of_position(&diag.range.start);
                let end = buffer.offset_of_position(&diag.range.end);
                span.add_span(Interval::new(start, end), diag.to_owned());
            }
            span.build()
        });
        self.diagnostics.diagnostics_span.set(span);

        self.clear_text_cache();
        self.clear_code_actions();
    }

    pub fn get_folding_range(&self) {
        // let cx = self.scope;
        // let doc = self.clone();
        // let rev = self.rev();
        // if let DocContent::File { path, .. } = doc.content.get_untracked() {
        //     let send = create_ext_action(cx, {
        //         move |result| {
        //             if rev != doc.rev() {
        //                 return;
        //             }
        //             if let Ok(ProxyResponse::LspFoldingRangeResponse {
        //                 resp, ..
        //             }) = result
        //             {
        //                 let folding = resp
        //                     .unwrap_or_default()
        //                     .into_iter()
        //                     .map(|x| {
        //                         crate::editor::gutter::FoldingRange::from_lsp(x)
        //                     })
        //                     .sorted_by(|x, y| x.start.line.cmp(&y.start.line))
        //                     .collect();
        //                 doc.folding_ranges.update(|symbol| {
        //                     symbol.0 = folding;
        //                 });
        //             }
        //         }
        //     });

        //     self.common
        //         .proxy
        //         .get_lsp_folding_range(path, move |result| {
        //             send(result);
        //         });
        // }
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
        self.completion_lens.set(Some(completion_lens));
        self.completion_pos.set((line, col));
        self.clear_text_cache();
    }

    pub fn clear_completion_lens(&self) {
        // TODO: more granular invalidation
        if self.completion_lens.get_untracked().is_some() {
            self.completion_lens.set(None);
            self.clear_text_cache();
        }
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

    fn update_find_result(&self, delta: &RopeDelta) {
        self.find_result.occurrences.update(|s| {
            *s = s.apply_delta(delta, true, InsertDrift::Default);
        })
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
        let send = create_ext_action(self.scope, move |occurrences: Selection| {
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
                            if l <= line { Some(l) } else { None }
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

    pub fn head_changes(&self) -> RwSignal<im::Vector<DiffLines>> {
        self.head_changes
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

    pub fn save(&self, after_action: impl FnOnce() + 'static) {
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

    pub fn set_inline_completion(
        &self,
        inline_completion: String,
        line: usize,
        col: usize,
    ) {
        // TODO: more granular invalidation
        batch(|| {
            self.inline_completion.set(Some(inline_completion));
            self.inline_completion_pos.set((line, col));
            self.clear_text_cache();
        });
    }

    pub fn clear_inline_completion(&self) {
        if self.inline_completion.with_untracked(Option::is_some) {
            self.inline_completion.set(None);
            self.clear_text_cache();
        }
    }

    pub fn update_inline_completion(&self, delta: &RopeDelta) {
        let Some(completion) = self.inline_completion.get_untracked() else {
            return;
        };

        let (line, col) = self.completion_pos.get_untracked();
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
                self.inline_completion
                    .set(Some(completion[new_len..].to_string()));
            }
        }

        // Shift the position by the rope delta
        let mut transformer = Transformer::new(delta);

        let new_offset = transformer.transform(offset, true);
        let new_pos = self
            .buffer
            .with_untracked(|b| b.offset_to_line_col(new_offset));

        self.inline_completion_pos.set(new_pos);
    }

    pub fn code_actions(&self) -> RwSignal<CodeActions> {
        self.code_actions
    }

    /// Returns the offsets of the brackets enclosing the given offset.
    /// Uses a language aware algorithm if syntax support is available for the current language,
    /// else falls back to a language unaware algorithm.
    pub fn find_enclosing_brackets(&self, offset: usize) -> Option<(usize, usize)> {
        let rev = self.rev();
        self.syntax
            .with_untracked(|syntax| {
                (!syntax.text.is_empty() && syntax.rev == rev)
                    .then(|| syntax.find_enclosing_pair(offset))
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
impl Document for Doc {
    fn text(&self) -> Rope {
        self.buffer.with_untracked(|buffer| buffer.text().clone())
    }

    fn cache_rev(&self) -> RwSignal<u64> {
        self.cache_rev
    }

    fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        self.syntax().with_untracked(|syntax| {
            if syntax.layers.is_some() {
                syntax
                    .find_tag(offset, previous, &CharBuffer::from(ch))
                    .unwrap_or(offset)
            } else {
                let text = self.text();
                let mut cursor = WordCursor::new(&text, offset);
                let new_offset = if previous {
                    cursor.previous_unmatched(ch)
                } else {
                    cursor.next_unmatched(ch)
                };

                new_offset.unwrap_or(offset)
            }
        })
    }

    fn find_matching_pair(&self, offset: usize) -> usize {
        self.syntax().with_untracked(|syntax| {
            if syntax.layers.is_some() {
                syntax.find_matching_pair(offset).unwrap_or(offset)
            } else {
                let text = self.text();
                WordCursor::new(&text, offset)
                    .match_pairs()
                    .unwrap_or(offset)
            }
        })
    }

    fn preedit(&self) -> PreeditData {
        self.preedit.clone()
    }

    fn compute_screen_lines(
        &self,
        editor: &Editor,
        base: RwSignal<ScreenLinesBase>,
    ) -> ScreenLines {
        let Some(editor_data) = self.editor_data(editor.id()) else {
            return ScreenLines {
                lines: Default::default(),
                info: Default::default(),
                diff_sections: Default::default(),
                base,
            };
        };

        compute_screen_lines(
            self.common.config,
            base,
            editor_data.kind.read_only(),
            &editor_data.doc_signal().get(),
            editor.lines(),
            editor.text_prov(),
            editor.config_id(),
        )
    }

    fn run_command(
        &self,
        ed: &Editor,
        cmd: &Command,
        count: Option<usize>,
        modifiers: Modifiers,
    ) -> CommandExecuted {
        let Some(editor_data) = self.editor_data(ed.id()) else {
            return CommandExecuted::No;
        };

        let cmd = CommandKind::from(cmd.clone());
        let cmd = LapceCommand {
            kind: cmd,
            data: None,
        };
        editor_data.run_command(&cmd, count, modifiers)
    }

    fn receive_char(&self, ed: &Editor, c: &str) {
        let Some(editor_data) = self.editor_data(ed.id()) else {
            return;
        };

        editor_data.receive_char(c);
    }

    fn edit(
        &self,
        iter: &mut dyn Iterator<Item = (Selection, &str)>,
        edit_type: EditType,
    ) {
        let delta = self
            .buffer
            .try_update(|buffer| buffer.edit(iter, edit_type))
            .unwrap();
        self.apply_deltas(&[delta]);
    }
}

impl DocumentPhantom for Doc {
    fn phantom_text(
        &self,
        _: EditorId,
        _: &EditorStyle,
        line: usize,
    ) -> PhantomTextLine {
        let config = &self.common.config.get_untracked();

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
                let (col, affinity) = self.buffer.with_untracked(|b| {
                    let mut cursor =
                        lapce_xi_rope::Cursor::new(b.text(), interval.start);

                    let next_char = cursor.peek_next_codepoint();
                    let prev_char = cursor.prev_codepoint();

                    let mut affinity = None;
                    if let Some(prev_char) = prev_char {
                        let c = get_char_property(prev_char);
                        if c == CharClassification::Other {
                            affinity = Some(CursorAffinity::Backward)
                        } else if matches!(
                            c,
                            CharClassification::Lf
                                | CharClassification::Cr
                                | CharClassification::Space
                        ) {
                            affinity = Some(CursorAffinity::Forward)
                        }
                    };
                    if affinity.is_none() {
                        if let Some(next_char) = next_char {
                            let c = get_char_property(next_char);
                            if c == CharClassification::Other {
                                affinity = Some(CursorAffinity::Forward)
                            } else if matches!(
                                c,
                                CharClassification::Lf
                                    | CharClassification::Cr
                                    | CharClassification::Space
                            ) {
                                affinity = Some(CursorAffinity::Backward)
                            }
                        }
                    }

                    let (_, col) = b.offset_to_line_col(interval.start);
                    (col, affinity)
                });
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
                    affinity,
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

        let mut diag_text: SmallVec<[PhantomText; 6]> =
            self.buffer.with_untracked(|buffer| {
                config
                    .editor
                    .enable_error_lens
                    .then_some(())
                    .map(|_| self.diagnostics.diagnostics_span.get_untracked())
                    .map(|diags| {
                        diags
                            .iter_chunks(start_offset..end_offset)
                            .filter_map(|(iv, diag)| {
                                let end = iv.end();
                                let end_line = buffer.line_of_offset(end);
                                if end_line == line
                                    && diag.severity < Some(DiagnosticSeverity::HINT)
                                {
                                    let fg = {
                                        let severity = diag
                                            .severity
                                            .unwrap_or(DiagnosticSeverity::WARNING);
                                        let theme_prop = if severity
                                            == DiagnosticSeverity::ERROR
                                        {
                                            LapceColor::ERROR_LENS_ERROR_FOREGROUND
                                        } else if severity
                                            == DiagnosticSeverity::WARNING
                                        {
                                            LapceColor::ERROR_LENS_WARNING_FOREGROUND
                                        } else {
                                            // information + hint (if we keep that) + things without a severity
                                            LapceColor::ERROR_LENS_OTHER_FOREGROUND
                                        };

                                        config.color(theme_prop)
                                    };

                                    let text =
                                        if config.editor.only_render_error_styling {
                                            "".to_string()
                                        } else if config.editor.error_lens_multiline
                                        {
                                            format!("    {}", diag.message)
                                        } else {
                                            format!(
                                                "    {}",
                                                diag.message.lines().join(" ")
                                            )
                                        };
                                    Some(PhantomText {
                                        kind: PhantomTextKind::Diagnostic,
                                        col: end_offset - start_offset,
                                        affinity: Some(CursorAffinity::Backward),
                                        text,
                                        fg: Some(fg),
                                        font_size: Some(
                                            config.editor.error_lens_font_size(),
                                        ),
                                        bg: None,
                                        under_line: None,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect::<SmallVec<[PhantomText; 6]>>()
                    })
                    .unwrap_or_default()
            });

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
                fg: Some(config.color(LapceColor::COMPLETION_LENS_FOREGROUND)),
                font_size: Some(config.editor.completion_lens_font_size()),
                affinity: Some(CursorAffinity::Backward),
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
            self.inline_completion_pos.get_untracked();
        let inline_completion_text = config
            .editor
            .enable_inline_completion
            .then_some(())
            .and(self.inline_completion.get_untracked())
            .filter(|_| line == inline_completion_line)
            .map(|completion| PhantomText {
                kind: PhantomTextKind::Completion,
                col: inline_completion_col,
                text: completion.clone(),
                affinity: Some(CursorAffinity::Backward),
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

        if let Some(preedit) = self
            .preedit_phantom(Some(config.color(LapceColor::EDITOR_FOREGROUND)), line)
        {
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

    fn has_multiline_phantom(&self, _: EditorId, _: &EditorStyle) -> bool {
        // TODO: actually check
        true
    }
}
impl CommonAction for Doc {
    fn exec_motion_mode(
        &self,
        _ed: &Editor,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        range: Range<usize>,
        is_vertical: bool,
        register: &mut Register,
    ) {
        let deltas = self
            .buffer
            .try_update(move |buffer| {
                Action::execute_motion_mode(
                    cursor,
                    buffer,
                    motion_mode,
                    range,
                    is_vertical,
                    register,
                )
            })
            .unwrap();
        self.apply_deltas(&deltas);
    }

    fn do_edit(
        &self,
        _ed: &Editor,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
        smart_tab: bool,
    ) -> bool {
        let deltas = Doc::do_edit(self, cursor, cmd, modal, register, smart_tab);
        !deltas.is_empty()
    }
}

#[derive(Clone)]
pub struct DocStyling {
    config: ReadSignal<Arc<LapceConfig>>,
    doc: Rc<Doc>,
}
impl DocStyling {
    fn apply_colorization(
        &self,
        edid: EditorId,
        style: &EditorStyle,
        line: usize,
        attrs: &Attrs,
        attrs_list: &mut AttrsList,
    ) {
        let config = self.config.get_untracked();
        let phantom_text = self.doc.phantom_text(edid, style, line);
        if let Some(bracket_styles) = self.doc.parser.borrow().bracket_pos.get(&line)
        {
            for bracket_style in bracket_styles.iter() {
                if let Some(fg_color) = bracket_style.style.fg_color.as_ref() {
                    if let Some(fg_color) = config.style_color(fg_color) {
                        let start = phantom_text.col_at(bracket_style.start);
                        let end = phantom_text.col_at(bracket_style.end);
                        attrs_list
                            .add_span(start..end, attrs.clone().color(fg_color));
                    }
                }
            }
        }
    }
}
impl Styling for DocStyling {
    fn id(&self) -> u64 {
        self.config.with_untracked(|config| config.id)
    }

    fn font_size(&self, _: EditorId, _line: usize) -> usize {
        self.config
            .with_untracked(|config| config.editor.font_size())
    }

    fn line_height(&self, _: EditorId, _line: usize) -> f32 {
        self.config
            .with_untracked(|config| config.editor.line_height()) as f32
    }

    fn font_family(
        &self,
        _: EditorId,
        _line: usize,
    ) -> std::borrow::Cow<'_, [floem::text::FamilyOwned]> {
        // TODO: cache this
        Cow::Owned(self.config.with_untracked(|config| {
            FamilyOwned::parse_list(&config.editor.font_family).collect()
        }))
    }

    fn weight(&self, _: EditorId, _line: usize) -> floem::text::Weight {
        floem::text::Weight::NORMAL
    }

    fn italic_style(&self, _: EditorId, _line: usize) -> floem::text::Style {
        floem::text::Style::Normal
    }

    fn stretch(&self, _: EditorId, _line: usize) -> floem::text::Stretch {
        floem::text::Stretch::Normal
    }

    fn indent_line(&self, _: EditorId, line: usize, line_content: &str) -> usize {
        if line_content.trim().is_empty() {
            let text = self.doc.rope_text();
            let offset = text.offset_of_line(line);
            if let Some(offset) =
                self.doc.syntax.with_untracked(|s| s.parent_offset(offset))
            {
                return text.line_of_offset(offset);
            }
        }

        line
    }

    fn tab_width(&self, _: EditorId, _line: usize) -> usize {
        self.config.with_untracked(|config| config.editor.tab_width)
    }

    fn atomic_soft_tabs(&self, _: EditorId, _line: usize) -> bool {
        self.config
            .with_untracked(|config| config.editor.atomic_soft_tabs)
    }

    fn apply_attr_styles(
        &self,
        edid: EditorId,
        style: &EditorStyle,
        line: usize,
        default: Attrs,
        attrs_list: &mut AttrsList,
    ) {
        let config = self.doc.common.config.get_untracked();

        self.apply_colorization(edid, style, line, &default, attrs_list);

        let phantom_text = self.doc.phantom_text(edid, style, line);
        for line_style in self.doc.line_style(line).iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.style_color(fg_color) {
                    let start = phantom_text.col_at(line_style.start);
                    let end = phantom_text.col_at(line_style.end);
                    attrs_list.add_span(start..end, default.clone().color(fg_color));
                }
            }
        }
    }

    fn apply_layout_styles(
        &self,
        edid: EditorId,
        style: &EditorStyle,
        line: usize,
        layout_line: &mut TextLayoutLine,
    ) {
        let doc = &self.doc;
        let config = doc.common.config.get_untracked();

        layout_line.extra_style.clear();
        let layout = &layout_line.text;

        let phantom_text = doc.phantom_text(edid, style, line);

        let phantom_styles = phantom_text
            .offset_size_iter()
            .filter(move |(_, _, _, p)| p.bg.is_some() || p.under_line.is_some())
            .flat_map(move |(col_shift, size, col, phantom)| {
                let start = col + col_shift;
                let end = start + size;

                extra_styles_for_range(
                    layout,
                    start,
                    end,
                    phantom.bg,
                    phantom.under_line,
                    None,
                )
            });
        layout_line.extra_style.extend(phantom_styles);

        let (start_offset, end_offset) = doc.buffer.with_untracked(|buffer| {
            (buffer.offset_of_line(line), buffer.offset_of_line(line + 1))
        });

        let mut max_severity: Option<DiagnosticSeverity> = None;
        doc.diagnostics.diagnostics_span.with_untracked(|diags| {
            diags
                .iter_chunks(start_offset..end_offset)
                .for_each(|(iv, diag)| {
                    let start = iv.start();
                    let end = iv.end();

                    if start <= end_offset
                        && end >= start_offset
                        && diag.severity < Some(DiagnosticSeverity::HINT)
                    {
                        let start = start.saturating_sub(start_offset);
                        let end = end - start_offset;
                        let start = phantom_text.col_after(start, true);
                        let end = phantom_text.col_after(end, false);

                        match (diag.severity, max_severity) {
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

                        let color_name = match diag.severity {
                            Some(DiagnosticSeverity::ERROR) => {
                                LapceColor::LAPCE_ERROR
                            }
                            _ => LapceColor::LAPCE_WARN,
                        };
                        let color = config.color(color_name);
                        let styles = extra_styles_for_range(
                            layout,
                            start,
                            end,
                            None,
                            None,
                            Some(color),
                        );
                        layout_line.extra_style.extend(styles);
                    }
                });
        });

        // Add the styling for the diagnostic severity, if applicable
        if let Some(max_severity) = max_severity {
            let theme_prop = if max_severity == DiagnosticSeverity::ERROR {
                LapceColor::ERROR_LENS_ERROR_BACKGROUND
            } else if max_severity == DiagnosticSeverity::WARNING {
                LapceColor::ERROR_LENS_WARNING_BACKGROUND
            } else {
                LapceColor::ERROR_LENS_OTHER_BACKGROUND
            };

            let size = layout.size();
            let x1 = if !config.editor.error_lens_end_of_line {
                let error_end_x = size.width;
                Some(error_end_x.max(size.width))
            } else {
                None
            };

            // TODO(minor): Should we show the background only on wrapped lines that have the
            // diagnostic actually on that line?
            // That would make it more obvious where it is from and matches other editors.
            layout_line.extra_style.push(LineExtraStyle {
                x: 0.0,
                y: 0.0,
                width: x1,
                height: size.height,
                bg_color: Some(config.color(theme_prop)),
                under_line: None,
                wave_line: None,
            });
        }
    }

    fn paint_caret(&self, edid: EditorId, _line: usize) -> bool {
        let Some(e_data) = self.doc.editor_data(edid) else {
            return true;
        };

        // If the find is active, then we don't want to paint the caret
        !e_data.find_focus.get_untracked()
    }
}

impl std::fmt::Debug for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Document {:?}", self.buffer_id))
    }
}

/// Get the previous unmatched character `c` from the `offset` using `syntax` if applicable
fn syntax_prev_unmatched(
    buffer: &Buffer,
    syntax: &Syntax,
    c: char,
    offset: usize,
) -> Option<usize> {
    if syntax.layers.is_some() {
        syntax.find_tag(offset, true, &CharBuffer::new(c))
    } else {
        WordCursor::new(buffer.text(), offset).previous_unmatched(c)
    }
}

fn should_blink(
    focus: RwSignal<Focus>,
    keyboard_focus: RwSignal<Option<ViewId>>,
) -> impl Fn() -> bool {
    move || {
        let Some(focus) = focus.try_get_untracked() else {
            return false;
        };
        if matches!(
            focus,
            Focus::Workbench
                | Focus::Palette
                | Focus::Panel(PanelKind::Plugin)
                | Focus::Panel(PanelKind::Search)
                | Focus::Panel(PanelKind::SourceControl)
        ) {
            return true;
        }

        if keyboard_focus.get_untracked().is_some() {
            return true;
        }
        false
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

            let height = (run.max_ascent + run.max_descent) as f64;
            let y = run.line_y as f64 - run.max_ascent as f64;

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
