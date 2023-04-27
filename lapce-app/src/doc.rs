use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    ext_event::create_ext_action,
    peniko::{kurbo::Point, Color},
    reactive::{
        ReadSignal, RwSignal, Scope, SignalGetUntracked, SignalUpdate,
        SignalWithUntracked,
    },
    views::VirtualListVector,
    AppContext,
};
use itertools::Itertools;
use lapce_core::{
    buffer::{Buffer, InvalLines},
    char_buffer::CharBuffer,
    command::EditCommand,
    cursor::{ColPosition, Cursor, CursorMode},
    editor::{EditType, Editor},
    mode::Mode,
    movement::{LinePosition, Movement},
    register::{Clipboard, Register},
    selection::{SelRegion, Selection},
    soft_tab::{snap_to_soft_tab, SnapDirection},
    style::line_styles,
    syntax::{edit::SyntaxEdit, Syntax},
    word::WordCursor,
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

use crate::{
    config::{color::LapceColor, LapceConfig},
    workspace::LapceWorkspace,
};

use self::phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine};

mod phantom_text;

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
pub struct EditorDiagnostic {
    pub range: (usize, usize),
    pub diagnostic: Diagnostic,
}

#[derive(Clone)]
pub struct LineExtraStyle {
    pub x: f64,
    pub width: Option<f64>,
    pub bg_color: Option<Color>,
    pub under_line: Option<Color>,
    pub wave_line: Option<Color>,
}

#[derive(Clone)]
pub struct TextLayoutLine {
    /// Extra styling that should be applied to the text
    /// (x0, x1 or line display end, style)
    pub extra_style: Vec<LineExtraStyle>,
    pub text: TextLayout,
    pub whitespaces: Option<Vec<(char, (f64, f64))>>,
    pub indent: f64,
}

/// Keeps track of the text layouts so that we can efficiently reuse them.
#[derive(Clone, Default)]
pub struct TextLayoutCache {
    /// The id of the last config, which lets us know when the config changes so we can update
    /// the cache.
    config_id: u64,
    /// (Font Size -> (Line Number -> Text Layout))  
    /// Different font-sizes are cached separately, which is useful for features like code lens
    /// where the text becomes small but you may wish to revert quickly.
    pub layouts: HashMap<usize, HashMap<usize, Arc<TextLayoutLine>>>,
    pub max_width: f64,
}

impl TextLayoutCache {
    pub fn new() -> Self {
        Self {
            config_id: 0,
            layouts: HashMap::new(),
            max_width: 0.0,
        }
    }

    fn clear(&mut self) {
        self.layouts.clear();
    }

    pub fn check_attributes(&mut self, config_id: u64) {
        if self.config_id != config_id {
            self.clear();
            self.config_id = config_id;
        }
    }
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

#[derive(Clone)]
pub struct Document {
    pub content: DocContent,
    pub buffer_id: BufferId,
    style_rev: u64,
    buffer: Buffer,
    syntax: Option<Syntax>,
    line_styles: Rc<RefCell<LineStyles>>,
    /// Semantic highlighting information (which is provided by the LSP)
    semantic_styles: Option<Arc<Spans<Style>>>,
    /// Inlay hints for the document
    pub inlay_hints: Option<Spans<InlayHint>>,
    /// The diagnostics for the document
    pub diagnostics: Option<im::Vector<EditorDiagnostic>>,
    /// (Offset -> (Plugin the code actions are from, Code Actions))
    pub code_actions: im::HashMap<usize, Arc<(PluginId, CodeActionResponse)>>,
    /// Whether the buffer's content has been loaded/initialized into the buffer.
    loaded: bool,

    /// The ready-to-render text layouts for the document.  
    /// This is an `Rc<RefCell<_>>` due to needing to access it even when the document is borrowed,
    /// since we may need to fill it with constructed text layouts.
    pub text_layouts: Rc<RefCell<TextLayoutCache>>,
    proxy: ProxyRpcHandler,
    config: ReadSignal<Arc<LapceConfig>>,
}

pub struct DocLine {
    pub rev: u64,
    pub style_rev: u64,
    pub line: usize,
    pub text: Arc<TextLayoutLine>,
    pub code_actions: Option<Arc<(PluginId, CodeActionResponse)>>,
}

impl VirtualListVector<DocLine> for Document {
    type ItemIterator = std::vec::IntoIter<DocLine>;

    fn total_len(&self) -> usize {
        self.buffer.num_lines()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> Self::ItemIterator {
        let lines = range
            .into_iter()
            .map(|line| DocLine {
                rev: self.buffer.rev(),
                style_rev: self.style_rev,
                line,
                text: self.get_text_layout(line, 12),
                code_actions: self.code_actions.get(&line).cloned(),
            })
            .collect::<Vec<_>>();
        lines.into_iter()
    }
}

impl Document {
    pub fn new(
        cx: Scope,
        path: PathBuf,
        diagnostics: Option<im::Vector<EditorDiagnostic>>,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let syntax = Syntax::init(&path);
        Self {
            buffer_id: BufferId::next(),
            buffer: Buffer::new(""),
            style_rev: 0,
            syntax: syntax.ok(),
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics,
            content: DocContent::File(path),
            loaded: false,
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            code_actions: im::HashMap::new(),
            proxy,
            config,
        }
    }

    pub fn new_local(
        cx: Scope,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        Self {
            buffer_id: BufferId::next(),
            buffer: Buffer::new(""),
            style_rev: 0,
            content: DocContent::Local,
            syntax: None,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
            inlay_hints: None,
            diagnostics: None,
            loaded: true,
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            code_actions: im::HashMap::new(),
            proxy,
            config,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
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
    }

    /// Reload the document's content, and is what you should typically use when you want to *set*
    /// an existing document's content.
    pub fn reload(&mut self, content: Rope, set_pristine: bool) {
        // self.code_actions.clear();
        // self.inlay_hints = None;
        let delta = self.buffer.reload(content, set_pristine);
        self.apply_deltas(&[delta]);
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

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        let rev = self.rev() - deltas.len() as u64;
        for (i, (delta, _, _)) in deltas.iter().enumerate() {
            self.update_styles(delta);
            self.update_inlay_hints(delta);
            self.update_diagnostics(delta);
            // self.update_completion(delta);
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
        // self.clear_sticky_headers_cache();
        // self.trigger_head_change();
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
        self.clear_text_layout_cache();
    }

    fn clear_text_layout_cache(&mut self) {
        self.text_layouts.borrow_mut().clear();
        self.style_rev += 1;
    }

    fn clear_code_actions(&mut self) {
        self.code_actions.clear();
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        font_size: usize,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let text_layout = self.get_text_layout(line, font_size);
                let hit_point = text_layout.text.hit_point(Point::new(x, 0.0));
                let n = hit_point.index;

                n.min(self.buffer.line_end_col(line, caret))
            }
            ColPosition::End => self.buffer.line_end_col(line, caret),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.buffer.first_non_blank_character_on_line(line)
            }
        }
    }

    /// Move a selection region by a given movement.  
    /// Much of the time, this will just be a matter of moving the cursor, but
    /// some movements may depend on the current selection.
    fn move_region(
        &self,
        region: &SelRegion,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
    ) -> SelRegion {
        let (count, region) = if count >= 1 && !modify && !region.is_caret() {
            // If we're not a caret, and we are moving left/up or right/down, we want to move
            // the cursor to the left or right side of the selection.
            // Ex: `|abc|` -> left/up arrow key -> `|abc`
            // Ex: `|abc|` -> right/down arrow key -> `abc|`
            // and it doesn't matter which direction the selection os going, so we use min/max
            match movement {
                Movement::Left | Movement::Up => {
                    let leftmost = region.min();
                    (count - 1, SelRegion::new(leftmost, leftmost, region.horiz))
                }
                Movement::Right | Movement::Down => {
                    let rightmost = region.max();
                    (
                        count - 1,
                        SelRegion::new(rightmost, rightmost, region.horiz),
                    )
                }
                _ => (count, *region),
            }
        } else {
            (count, *region)
        };

        let (end, horiz) = self.move_offset(
            region.end,
            region.horiz.as_ref(),
            count,
            movement,
            mode,
        );
        let start = match modify {
            true => region.start,
            false => end,
        };
        SelRegion::new(start, end, horiz)
    }

    pub fn move_selection(
        &self,
        selection: &Selection,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            new_selection
                .add_region(self.move_region(region, count, modify, movement, mode));
        }
        new_selection
    }

    pub fn move_offset(
        &self,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
    ) -> (usize, Option<ColPosition>) {
        let config = self.config.get_untracked();
        match movement {
            Movement::Left => {
                let mut new_offset = self.buffer.move_left(offset, mode, count);

                if config.editor.atomic_soft_tabs && config.editor.tab_width > 1 {
                    new_offset = snap_to_soft_tab(
                        &self.buffer,
                        new_offset,
                        SnapDirection::Left,
                        config.editor.tab_width,
                    );
                }

                (new_offset, None)
            }
            Movement::Right => {
                let mut new_offset = self.buffer.move_right(offset, mode, count);

                if config.editor.atomic_soft_tabs && config.editor.tab_width > 1 {
                    new_offset = snap_to_soft_tab(
                        &self.buffer,
                        new_offset,
                        SnapDirection::Right,
                        config.editor.tab_width,
                    );
                }

                (new_offset, None)
            }
            Movement::Up => {
                let font_size = config.editor.font_size;

                let line = self.buffer.line_of_offset(offset);
                if line == 0 {
                    let line = self.buffer.line_of_offset(offset);
                    let new_offset = self.buffer.offset_of_line(line);
                    let horiz = horiz.cloned().unwrap_or_else(|| {
                        ColPosition::Col(
                            self.line_point_of_offset(offset, font_size).x,
                        )
                    });
                    return (new_offset, Some(horiz));
                }

                let line = line.saturating_sub(count);

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(self.line_point_of_offset(offset, font_size).x)
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Down => {
                let font_size = config.editor.font_size;

                let last_line = self.buffer.last_line();
                let line = self.buffer.line_of_offset(offset);
                if line == last_line {
                    let new_offset =
                        self.buffer.offset_line_end(offset, mode != Mode::Normal);
                    let horiz = horiz.cloned().unwrap_or_else(|| {
                        ColPosition::Col(
                            self.line_point_of_offset(offset, font_size).x,
                        )
                    });
                    return (new_offset, Some(horiz));
                }

                let line = line + count;

                let line = line.min(last_line);

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(self.line_point_of_offset(offset, font_size).x)
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::DocumentStart => (0, Some(ColPosition::Start)),
            Movement::DocumentEnd => {
                let last_offset = self
                    .buffer
                    .offset_line_end(self.buffer.len(), mode != Mode::Normal);
                (last_offset, Some(ColPosition::End))
            }
            Movement::FirstNonBlank => {
                let line = self.buffer.line_of_offset(offset);
                let non_blank_offset =
                    self.buffer.first_non_blank_character_on_line(line);
                let start_line_offset = self.buffer.offset_of_line(line);
                if offset > non_blank_offset {
                    // Jump to the first non-whitespace character if we're strictly after it
                    (non_blank_offset, Some(ColPosition::FirstNonBlank))
                } else {
                    // If we're at the start of the line, also jump to the first not blank
                    if start_line_offset == offset {
                        (non_blank_offset, Some(ColPosition::FirstNonBlank))
                    } else {
                        // Otherwise, jump to the start of the line
                        (start_line_offset, Some(ColPosition::Start))
                    }
                }
            }
            Movement::StartOfLine => {
                let line = self.buffer.line_of_offset(offset);
                let new_offset = self.buffer.offset_of_line(line);
                (new_offset, Some(ColPosition::Start))
            }
            Movement::EndOfLine => {
                let new_offset =
                    self.buffer.offset_line_end(offset, mode != Mode::Normal);
                (new_offset, Some(ColPosition::End))
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => {
                        (line - 1).min(self.buffer.last_line())
                    }
                    LinePosition::First => 0,
                    LinePosition::Last => self.buffer.last_line(),
                };
                let font_size = config.editor.font_size;
                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(self.line_point_of_offset(offset, font_size).x)
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let new_offset = self
                    .buffer
                    .text()
                    .prev_grapheme_offset(new_offset + 1)
                    .unwrap();
                (new_offset, None)
            }
            Movement::WordEndForward => {
                let new_offset = self.buffer.move_n_wordends_forward(
                    offset,
                    count,
                    mode == Mode::Insert,
                );
                (new_offset, None)
            }
            Movement::WordForward => {
                let new_offset = self.buffer.move_n_words_forward(offset, count);
                (new_offset, None)
            }
            Movement::WordBackward => {
                let new_offset =
                    self.buffer.move_n_words_backward(offset, count, mode);
                (new_offset, None)
            }
            Movement::NextUnmatched(char) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, false, &CharBuffer::from(char))
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .next_unmatched(*char)
                        .map_or(offset, |new| new - 1);
                    (new_offset, None)
                }
            }
            Movement::PreviousUnmatched(char) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, true, &CharBuffer::from(char))
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .previous_unmatched(*char)
                        .unwrap_or(offset);
                    (new_offset, None)
                }
            }
            Movement::MatchPairs => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset =
                        syntax.find_matching_pair(offset).unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .match_pairs()
                        .unwrap_or(offset);
                    (new_offset, None)
                }
            }
            Movement::ParagraphForward => {
                let new_offset =
                    self.buffer.move_n_paragraphs_forward(offset, count);
                (new_offset, None)
            }
            Movement::ParagraphBackward => {
                let new_offset =
                    self.buffer.move_n_paragraphs_backward(offset, count);
                (new_offset, None)
            }
        }
    }

    pub fn move_cursor(
        &mut self,
        cursor: &mut Cursor,
        movement: &Movement,
        count: usize,
        modify: bool,
        register: &mut Register,
        config: &LapceConfig,
    ) {
        match cursor.mode {
            CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.move_offset(
                    offset,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                );
                if let Some(motion_mode) = cursor.motion_mode.clone() {
                    let (moved_new_offset, _) = self.move_offset(
                        new_offset,
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                    );
                    let (start, end) = match movement {
                        Movement::EndOfLine | Movement::WordEndForward => {
                            (offset, moved_new_offset)
                        }
                        Movement::MatchPairs => {
                            if new_offset > offset {
                                (offset, moved_new_offset)
                            } else {
                                (moved_new_offset, new_offset)
                            }
                        }
                        _ => (offset, new_offset),
                    };
                    let deltas = Editor::execute_motion_mode(
                        cursor,
                        &mut self.buffer,
                        motion_mode,
                        start,
                        end,
                        movement.is_vertical(),
                        register,
                    );
                    self.apply_deltas(&deltas);
                    cursor.motion_mode = None;
                } else {
                    cursor.mode = CursorMode::Normal(new_offset);
                    cursor.horiz = horiz;
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.move_offset(
                    end,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                );
                cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                cursor.horiz = horiz;
            }
            CursorMode::Insert(ref selection) => {
                let selection = self.move_selection(
                    selection,
                    count,
                    modify,
                    movement,
                    Mode::Insert,
                );
                cursor.set_insert(selection);
            }
        }
    }

    /// Returns the point into the text layout of the line at the given offset.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_offset(&self, offset: usize, font_size: usize) -> Point {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        self.line_point_of_line_col(line, col, font_size)
    }

    /// Returns the point into the text layout of the line at the given line and column.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_line_col(
        &self,
        line: usize,
        col: usize,
        font_size: usize,
    ) -> Point {
        let text_layout = self.get_text_layout(line, font_size);
        text_layout.text.hit_position(col).point
    }

    /// Get the (point above, point below) of a particular offset within the editor.
    pub fn points_of_offset(&self, offset: usize) -> (Point, Point) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        self.points_of_line_col(line, col)
    }

    /// Get the (point above, point below) of a particular (line, col) within the editor.
    pub fn points_of_line_col(&self, line: usize, col: usize) -> (Point, Point) {
        let config = self.config.get_untracked();
        let (y, line_height, font_size) = (
            config.editor.line_height() * line,
            config.editor.line_height(),
            config.editor.font_size,
        );

        let line = line.min(self.buffer.last_line());

        let phantom_text = self.line_phantom_text(line);
        let col = phantom_text.col_after(col, false);

        let mut x_shift = 0.0;
        if font_size < config.editor.font_size {
            let line_content = self.buffer.line_content(line);
            let mut col = 0usize;
            for ch in line_content.chars() {
                if ch == ' ' || ch == '\t' {
                    col += 1;
                } else {
                    break;
                }
            }

            if col > 0 {
                let normal_text_layout =
                    self.get_text_layout(line, config.editor.font_size);
                let small_text_layout = self.get_text_layout(line, font_size);
                x_shift = normal_text_layout.text.hit_position(col).point.x
                    - small_text_layout.text.hit_position(col).point.x;
            }
        }

        let x = self.line_point_of_line_col(line, col, font_size).x + x_shift;
        (
            Point::new(x, y as f64),
            Point::new(x, (y + line_height) as f64),
        )
    }

    /// Create a new text layout for the given line.  
    /// Typically you should use [`Document::get_text_layout`] instead.
    fn new_text_layout(&self, line: usize, font_size: usize) -> TextLayoutLine {
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
            .font_size(config.editor.font_size as f32);
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

        let font_size = config.editor.font_size;

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

        if let Some(diags) = self.diagnostics.as_ref() {
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
        }

        TextLayoutLine {
            text: text_layout,
            extra_style,
            whitespaces: None,
            indent: 0.0,
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
    fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
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
                    doc.clear_text_layout_cache();
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
            .and(self.diagnostics.as_ref())
            .map(|x| x.iter())
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

                let rope_text = self.buffer.rope_text();
                let col = rope_text.offset_of_line(line + 1)
                    - rope_text.offset_of_line(line);
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

        // let (completion_line, completion_col) = self.completion_pos;
        // let completion_text = config
        //     .editor
        //     .enable_completion_lens
        //     .then_some(())
        //     .and(self.completion.as_ref())
        //     // TODO: We're probably missing on various useful completion things to include here!
        //     .filter(|_| line == completion_line)
        //     .map(|completion| PhantomText {
        //         kind: PhantomTextKind::Completion,
        //         col: completion_col,
        //         text: completion.to_string(),
        //         fg: Some(
        //             config
        //                 .get_color_unchecked(LapceTheme::COMPLETION_LENS_FOREGROUND)
        //                 .clone(),
        //         ),
        //         font_size: Some(config.editor.completion_lens_font_size()),
        //         font_family: Some(config.editor.completion_lens_font_family()),
        //         bg: None,
        //         under_line: None,
        //         // TODO: italics?
        //     });
        // if let Some(completion_text) = completion_text {
        //     text.push(completion_text);
        // }

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

    pub fn set_diagnostics(&mut self, diagnostics: im::Vector<EditorDiagnostic>) {
        self.clear_text_layout_cache();
        self.clear_code_actions();
        self.diagnostics = Some(diagnostics);
        self.init_diagnostics();
    }

    /// Update the diagnostics' positions after an edit so that they appear in the correct place.
    fn update_diagnostics(&mut self, delta: &RopeDelta) {
        let Some(mut diagnostics) = self.diagnostics.clone() else { return };
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
        self.diagnostics = Some(diagnostics);
    }

    /// init diagnostics offset ranges from lsp positions
    fn init_diagnostics(&mut self) {
        let Some(mut diagnostics) = self.diagnostics.clone() else { return };
        for diagnostic in diagnostics.iter_mut() {
            let start = self
                .buffer()
                .offset_of_position(&diagnostic.diagnostic.range.start);
            let end = self
                .buffer()
                .offset_of_position(&diagnostic.diagnostic.range.end);
            diagnostic.range = (start, end);
        }
        self.diagnostics = Some(diagnostics);
    }
}
