use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use druid::piet::Piet;
use druid::{piet::PietTextLayout, FontWeight, Key, Vec2};
use druid::{
    piet::{PietText, Text, TextAttribute, TextLayoutBuilder},
    Color, Command, Data, EventCtx, ExtEventSink, Target, UpdateCtx, WidgetId,
    WindowId,
};
use druid::{Env, FontFamily, PaintCtx, Point};
use git2::Repository;
use language::{new_highlight_config, new_parser, LapceLanguage};
use lsp_types::SemanticTokensServerCapabilities;
use lsp_types::{CallHierarchyOptions, SemanticTokensLegend};
use lsp_types::{
    CodeActionResponse, Position, Range, TextDocumentContentChangeEvent,
};
use lsp_types::{Location, SemanticTokens};
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::{
    borrow::Cow,
    collections::BTreeSet,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use std::{collections::HashMap, fs::File};
use std::{fs, str::FromStr};
use tree_sitter::{Node, Parser, Tree};
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use xi_core_lib::selection::InsertDrift;
use xi_rope::{
    interval::IntervalBounds,
    multiset::Subset,
    rope::Rope,
    spans::{Spans, SpansBuilder, SpansInfo},
    Cursor, Delta, DeltaBuilder, Interval, LinesMetric, RopeDelta, RopeInfo,
    Transformer,
};

use crate::config::{Config, LapceTheme};
use crate::data::EditorKind;
use crate::editor::EditorLocationNew;
use crate::theme::OldLapceTheme;
use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    data::LapceEditorViewData,
    editor::EditorOperator,
    find::Find,
    language,
    movement::{ColPosition, LinePosition, Movement, SelRegion, Selection},
    proxy::LapceProxy,
    state::LapceWorkspaceType,
    state::{Counter, Mode},
};

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize, Data)]
pub struct BufferId(pub u64);

impl BufferId {
    pub fn next() -> Self {
        static BUFFER_ID_COUNTER: Counter = Counter::new();
        Self(BUFFER_ID_COUNTER.next())
    }
}

#[derive(Clone)]
pub struct BufferUIState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub id: BufferId,
    pub text_layouts: Vec<Arc<Option<Arc<HighlightTextLayout>>>>,
    pub line_changes: HashMap<usize, char>,
    pub max_len_line: usize,
    pub max_len: usize,
    pub dirty: bool,
}

#[derive(Data, Clone)]
pub enum BufferState {
    Loading,
    Open(Arc<BufferNew>),
}

pub struct StyledTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub styles: Arc<Vec<(usize, usize, Style)>>,
}

pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub fg_color: Option<String>,
}

pub enum UpdateEvent {
    Buffer(BufferUpdate),
    SemanticTokens(BufferUpdate, Vec<(usize, usize, String)>),
}

pub struct BufferUpdate {
    pub id: BufferId,
    pub path: PathBuf,
    pub rope: Rope,
    pub rev: u64,
    pub language: LapceLanguage,
    pub highlights: Arc<Spans<Style>>,
    pub semantic_tokens: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditType {
    Other,
    InsertChars,
    InsertNewline,
    Delete,
    Undo,
    Redo,
}

impl EditType {
    /// Checks whether a new undo group should be created between two edits.
    fn breaks_undo_group(self, previous: EditType) -> bool {
        self == EditType::Other || self != previous
    }
}

#[derive(Clone)]
enum Contents {
    Edit {
        /// Groups related edits together so that they are undone and re-done
        /// together. For example, an auto-indent insertion would be un-done
        /// along with the newline that triggered it.
        undo_group: usize,
        /// The subset of the characters of the union string from after this
        /// revision that were added by this revision.
        inserts: Subset,
        /// The subset of the characters of the union string from after this
        /// revision that were deleted by this revision.
        deletes: Subset,
    },
    Undo {
        /// The set of groups toggled between undone and done.
        /// Just the `symmetric_difference` (XOR) of the two sets.
        toggled_groups: BTreeSet<usize>, // set of undo_group id's
        /// Used to store a reversible difference between the old
        /// and new deletes_from_union
        deletes_bitxor: Subset,
    },
}

#[derive(Clone)]
struct Revision {
    max_undo_so_far: usize,
    edit: Contents,
}

#[derive(Clone)]
pub struct BufferNew {
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub text_layouts: Rc<RefCell<Vec<Option<Arc<StyledTextLayout>>>>>,
    pub line_styles: Rc<RefCell<Vec<Option<Arc<Vec<(usize, usize, Style)>>>>>>,
    pub styles: Arc<Spans<Style>>,
    pub semantic_tokens: bool,
    pub language: Option<LapceLanguage>,
    pub max_len: usize,
    pub max_len_line: usize,
    pub num_lines: usize,
    pub rev: u64,
    pub dirty: bool,
    pub loaded: bool,
    pub local: bool,
    update_sender: Arc<Sender<UpdateEvent>>,
    pub line_changes: HashMap<usize, char>,

    revs: Vec<Revision>,
    cur_undo: usize,
    undos: BTreeSet<usize>,
    undo_group_id: usize,
    live_undos: Vec<usize>,
    deletes_from_union: Subset,
    undone_groups: BTreeSet<usize>,
    tombstones: Rope,

    this_edit_type: EditType,
    last_edit_type: EditType,

    pub cursor_offset: usize,
    pub scroll_offset: Vec2,

    pub code_actions: im::HashMap<usize, CodeActionResponse>,
    pub syntax_tree: Option<Arc<Tree>>,
}

impl BufferNew {
    pub fn new(path: PathBuf, update_sender: Arc<Sender<UpdateEvent>>) -> Self {
        let rope = Rope::from("");
        let language = LapceLanguage::from_path(&path);
        let buffer = Self {
            id: BufferId::next(),
            rope,
            language,
            path,
            text_layouts: Rc::new(RefCell::new(Vec::new())),
            styles: Arc::new(SpansBuilder::new(0).build()),
            line_styles: Rc::new(RefCell::new(Vec::new())),
            semantic_tokens: false,
            max_len: 0,
            max_len_line: 0,
            num_lines: 0,
            rev: 0,
            loaded: false,
            dirty: false,
            update_sender,
            local: false,
            line_changes: HashMap::new(),

            revs: vec![Revision {
                max_undo_so_far: 0,
                edit: Contents::Undo {
                    toggled_groups: BTreeSet::new(),
                    deletes_bitxor: Subset::new(0),
                },
            }],
            cur_undo: 1,
            undos: BTreeSet::new(),
            undo_group_id: 1,
            live_undos: vec![0],
            deletes_from_union: Subset::new(0),
            undone_groups: BTreeSet::new(),
            tombstones: Rope::default(),

            last_edit_type: EditType::Other,
            this_edit_type: EditType::Other,

            cursor_offset: 0,
            scroll_offset: Vec2::ZERO,

            code_actions: im::HashMap::new(),
            syntax_tree: None,
        };
        *buffer.line_styles.borrow_mut() = vec![None; buffer.num_lines()];
        *buffer.text_layouts.borrow_mut() = vec![None; buffer.num_lines()];
        buffer
    }

    pub fn set_local(mut self) -> Self {
        self.local = true;
        self
    }

    pub fn reset_revs(&mut self) {
        self.rope = Rope::from("");
        self.revs = vec![Revision {
            max_undo_so_far: 0,
            edit: Contents::Undo {
                toggled_groups: BTreeSet::new(),
                deletes_bitxor: Subset::new(0),
            },
        }];
        self.cur_undo = 1;
        self.undo_group_id = 1;
        self.live_undos = vec![0];
        self.deletes_from_union = Subset::new(0);
        self.undone_groups = BTreeSet::new();
        self.tombstones = Rope::default();
        self.syntax_tree = None;
    }

    pub fn load_content(&mut self, content: &str) {
        self.reset_revs();

        if content != "" {
            let delta =
                Delta::simple_edit(Interval::new(0, 0), Rope::from(content), 0);
            let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
                self.mk_new_rev(0, delta.clone());
            self.revs.push(new_rev);
            self.rope = new_text.clone();
            self.tombstones = new_tombstones;
            self.deletes_from_union = new_deletes_from_union;
        }

        self.code_actions.clear();
        let (max_len, max_len_line) = self.get_max_line_len();
        self.max_len = max_len;
        self.max_len_line = max_len_line;
        self.num_lines = self.num_lines();
        *self.text_layouts.borrow_mut() = vec![None; self.num_lines()];
        *self.line_styles.borrow_mut() = vec![None; self.num_lines()];
        self.loaded = true;
        self.notify_update();
    }

    pub fn notify_update(&self) {
        if let Some(language) = self.language {
            self.update_sender.send(UpdateEvent::Buffer(BufferUpdate {
                id: self.id,
                path: self.path.clone(),
                rope: self.rope.clone(),
                rev: self.rev,
                language,
                highlights: self.styles.clone(),
                semantic_tokens: self.semantic_tokens,
            }));
        }
    }

    pub fn retrieve_file(&self, proxy: Arc<LapceProxy>, event_sink: ExtEventSink) {
        let id = self.id;
        let path = self.path.clone();
        thread::spawn(move || {
            let content = { proxy.new_buffer(id, path.clone()).unwrap() };
            println!("load file got content");
            event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::LoadBuffer { path, content },
                Target::Auto,
            );
        });
    }

    pub fn retrieve_file_and_go_to_location(
        &self,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
        editor_view_id: WidgetId,
        location: EditorLocationNew,
    ) {
        let id = self.id;
        let path = self.path.clone();
        thread::spawn(move || {
            let content = { proxy.new_buffer(id, path.clone()).unwrap() };
            println!("load file got content");
            event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::LoadBufferAndGoToPosition {
                    path,
                    content,
                    editor_view_id,
                    location,
                },
                Target::Auto,
            );
        });
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.rope.len()) + 1
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.rope.len())
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        self.rope.line_of_offset(offset)
    }

    pub fn offset_line_content(&self, offset: usize) -> Cow<str> {
        let line = self.line_of_offset(offset);
        let start_offset = self.offset_of_line(line);
        self.slice_to_cow(start_offset..offset)
    }

    pub fn line_content(&self, line: usize) -> String {
        self.slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
            .to_string()
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line + 1
        } else {
            line
        };
        self.rope.offset_of_line(line)
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        WordCursor::new(&self.rope, offset).select_word()
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        if self.len() == 0 {
            return None;
        }
        WordCursor::new(&self.rope, offset)
            .inner
            .peek_next_codepoint()
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line
        } else {
            line
        };
        let line_start_offset = self.rope.offset_of_line(line);
        WordCursor::new(&self.rope, line_start_offset).next_non_blank_char()
    }

    pub fn get_max_line_len(&self) -> (usize, usize) {
        let mut pre_offset = 0;
        let mut max_len = 0;
        let mut max_len_line = 0;
        for line in 0..self.num_lines() {
            let offset = self.rope.offset_of_line(line);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_len {
                max_len = line_len;
                max_len_line = line;
            }
        }
        (max_len, max_len_line)
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    fn get_line_styles(&self, line: usize) -> Arc<Vec<(usize, usize, Style)>> {
        if let Some(line_styles) = self.line_styles.borrow()[line].as_ref() {
            return line_styles.clone();
        }
        let start_offset = self.offset_of_line(line);
        let end_offset = self.offset_of_line(line + 1);
        let line_styles: Vec<(usize, usize, Style)> = self
            .styles
            .iter_chunks(start_offset..end_offset)
            .filter_map(|(iv, style)| {
                let start = iv.start();
                let end = iv.end();
                if start > end_offset {
                    None
                } else if end < start_offset {
                    None
                } else {
                    Some((
                        if start > start_offset {
                            start - start_offset
                        } else {
                            0
                        },
                        end - start_offset,
                        style.clone(),
                    ))
                }
            })
            .collect();
        let line_styles = Arc::new(line_styles);
        self.line_styles.borrow_mut()[line] = Some(line_styles.clone());
        line_styles
    }

    pub fn new_text_layout(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        line_content: &str,
        bounds: [f64; 2],
        config: &Config,
    ) -> PietTextLayout {
        let line_content = line_content.replace('\t', "    ");
        let styles = self.get_line_styles(line);
        let mut layout_builder = ctx
            .text()
            .new_text_layout(line_content)
            .font(config.editor.font_family(), config.editor.font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );
        for (start, end, style) in styles.iter() {
            if let Some(fg_color) = style.fg_color.as_ref() {
                if let Some(fg_color) =
                    config.get_color(&("style.".to_string() + fg_color))
                {
                    layout_builder = layout_builder.range_attribute(
                        start..end,
                        TextAttribute::TextColor(fg_color.clone()),
                    );
                }
            }
        }
        layout_builder.build_with_bounds(bounds)
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.rope.offset_of_line(line);
        let word_boundary =
            WordCursor::new(&self.rope, line_start_offset).next_non_blank_char();
        let indent = self.rope.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn offset_of_position(&self, pos: &Position) -> usize {
        self.offset_of_line_col(pos.line as usize, pos.character as usize)
    }

    pub fn offset_of_mouse(
        &self,
        text: &mut PietText,
        pos: Point,
        mode: Mode,
        config: &Config,
    ) -> usize {
        let line_height = config.editor.line_height as f64;
        let line = (pos.y / line_height).floor() as usize;
        let last_line = self.last_line();
        let (line, col) = if line > last_line {
            (last_line, 0)
        } else {
            let line_end = self.line_end_col(line, mode != Mode::Normal);
            let width = config.editor_text_width(text, "W");

            let col = (if mode == Mode::Insert {
                (pos.x / width).round() as usize
            } else {
                (pos.x / width).floor() as usize
            })
            .min(line_end);
            (line, col)
        };
        self.offset_of_line_col(line, col)
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        let line_content = self.line_content(line);
        let mut line_content = line_content.as_str();
        if line_content.ends_with("\n") {
            line_content = &line_content[..line_content.len() - 1];
        }
        let mut pos = 0;
        let mut offset = self.offset_of_line(line);
        for grapheme in line_content.graphemes(true) {
            pos += grapheme_column_width(grapheme);
            if pos > col {
                return offset;
            }

            offset += grapheme.len();
            if pos == col {
                return offset;
            }
        }
        offset
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let line = self.line_of_offset(offset);
        let line_start = self.offset_of_line(line);
        if offset == line_start {
            return (line, 0);
        }

        let col = str_col(&self.slice_to_cow(line_start..offset));
        (line, col)
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        let line_start = self.offset_of_line(line);
        let offset = self.line_end_offset(line, caret);
        let col = str_col(&self.slice_to_cow(line_start..offset));
        col
    }

    pub fn line_end_offset(&self, line: usize, caret: bool) -> usize {
        let mut offset = self.offset_of_line(line + 1);
        let line_content = self.line_content(line);
        let mut line_content = line_content.as_str();
        if line_content.ends_with("\n") {
            offset -= 1;
            line_content = &line_content[..line_content.len() - 1];
        }
        if !caret && line_content.len() > 0 {
            offset = self.prev_grapheme_offset(offset, 1, 0);
        }
        offset
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end_offset(line, caret)
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    // pub fn line_max_col(&self, line: usize, caret: bool) -> usize {
    //     let line_content = self.line_content(line);
    //     let n = self.line_len(line);
    //     match n {
    //         n if n == 0 => 0,
    //         n if !line_content.ends_with("\n") => match caret {
    //             true => n,
    //             false => n - 1,
    //         },
    //         n if n == 1 => 0,
    //         n => match caret {
    //             true => n - 1,
    //             false => n - 2,
    //         },
    //     }
    // }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        match horiz {
            &ColPosition::Col(n) => n.min(self.line_end_col(line, caret)),
            &ColPosition::End => self.line_end_col(line, caret),
            &ColPosition::Start => 0,
            &ColPosition::FirstNonBlank => {
                self.first_non_blank_character_on_line(line)
            }
        }
    }

    pub fn update_selection(
        &self,
        selection: &Selection,
        count: usize,
        movement: &Movement,
        mode: Mode,
        modify: bool,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            let region = self.update_region(region, count, movement, mode, modify);
            new_selection.add_region(region);
        }
        new_selection
    }

    pub fn update_region(
        &self,
        region: &SelRegion,
        count: usize,
        movement: &Movement,
        mode: Mode,
        modify: bool,
    ) -> SelRegion {
        let (end, horiz) =
            self.move_offset(region.end(), region.horiz(), count, movement, mode);

        let start = match modify {
            true => region.start(),
            false => end,
        };

        SelRegion::new(start, end, Some(horiz))
    }

    pub fn prev_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let mut cursor = Cursor::new(&self.rope, offset);
        let mut new_offset = offset;
        for i in 0..count {
            if let Some(prev_offset) = cursor.prev_grapheme() {
                if prev_offset < limit {
                    return new_offset;
                }
                new_offset = prev_offset;
                cursor.set(prev_offset);
            } else {
                return new_offset;
            }
        }
        new_offset
    }

    pub fn next_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let mut cursor = Cursor::new(&self.rope, offset);
        let mut new_offset = offset;
        for i in 0..count {
            if let Some(next_offset) = cursor.next_grapheme() {
                if next_offset > limit {
                    return new_offset;
                }
                new_offset = next_offset;
                cursor.set(next_offset);
            } else {
                return new_offset;
            }
        }
        new_offset
    }

    pub fn move_offset(
        &self,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
    ) -> (usize, ColPosition) {
        let horiz = if let Some(horiz) = horiz {
            horiz.clone()
        } else {
            let (_, col) = self.offset_to_line_col(offset);
            ColPosition::Col(col)
        };
        match movement {
            Movement::Left => {
                let line = self.line_of_offset(offset);
                let line_start_offset = self.offset_of_line(line);

                let min_offset = if mode == Mode::Insert {
                    0
                } else {
                    line_start_offset
                };

                let new_offset =
                    self.prev_grapheme_offset(offset, count, min_offset);
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::Right => {
                let line_end = self.offset_line_end(offset, mode != Mode::Normal);

                let max_offset = if mode == Mode::Insert {
                    self.len()
                } else {
                    line_end
                };

                let new_offset =
                    self.next_grapheme_offset(offset, count, max_offset);

                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::Up => {
                let line = self.line_of_offset(offset);
                let line = if line > count { line - count } else { 0 };
                let col = self.line_horiz_col(line, &horiz, mode != Mode::Normal);
                let new_offset = self.offset_of_line_col(line, col);
                (new_offset, horiz)
            }
            Movement::Down => {
                let last_line = self.last_line();
                let line = self.line_of_offset(offset) + count;
                let line = if line > last_line { last_line } else { line };
                let col = self.line_horiz_col(line, &horiz, mode != Mode::Normal);
                let new_offset = self.offset_of_line_col(line, col);
                (new_offset, horiz)
            }
            Movement::FirstNonBlank => {
                let line = self.line_of_offset(offset);
                let new_offset = self.first_non_blank_character_on_line(line);
                (new_offset, ColPosition::FirstNonBlank)
            }
            Movement::StartOfLine => {
                let line = self.line_of_offset(offset);
                let new_offset = self.offset_of_line(line);
                let new_offset = if new_offset == offset {
                    if new_offset > 0 {
                        new_offset - 1
                    } else {
                        0
                    }
                } else {
                    new_offset
                };
                (new_offset, ColPosition::Start)
            }
            Movement::EndOfLine => {
                let new_offset = self.offset_line_end(offset, mode != Mode::Normal);
                (new_offset, ColPosition::End)
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => {
                        let line = line - 1;
                        let last_line = self.last_line();
                        match line {
                            n if n > last_line => last_line,
                            n => n,
                        }
                    }
                    LinePosition::First => 0,
                    LinePosition::Last => self.last_line(),
                };
                let col = self.line_horiz_col(line, &horiz, mode != Mode::Normal);
                let new_offset = self.offset_of_line_col(line, col);
                (new_offset, horiz)
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let new_offset =
                    self.rope.prev_grapheme_offset(new_offset + 1).unwrap();
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordEndForward => {
                let mut new_offset = WordCursor::new(&self.rope, offset)
                    .end_boundary()
                    .unwrap_or(offset);
                if mode != Mode::Insert {
                    new_offset = self.prev_grapheme_offset(new_offset, 1, 0);
                }
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordForward => {
                let new_offset = WordCursor::new(&self.rope, offset)
                    .next_boundary()
                    .unwrap_or(offset);
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordBackward => {
                let new_offset = WordCursor::new(&self.rope, offset)
                    .prev_boundary()
                    .unwrap_or(offset);
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::NextUnmatched(c) => {
                if self.syntax_tree.is_some() {
                    let new_offset = self
                        .find_tag(offset, false, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = match WordCursor::new(&self.rope, offset)
                        .next_unmatched(*c)
                    {
                        Some(new_offset) => new_offset - 1,
                        None => offset,
                    };
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                }
            }
            Movement::PreviousUnmatched(c) => {
                if self.syntax_tree.is_some() {
                    let new_offset = self
                        .find_tag(offset, true, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = WordCursor::new(&self.rope, offset)
                        .previous_unmatched(*c)
                        .unwrap_or(offset);
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                }
            }
            Movement::MatchPairs => {
                if self.syntax_tree.is_some() {
                    let new_offset =
                        self.find_matching_pair(offset).unwrap_or(offset);
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = WordCursor::new(&self.rope, offset)
                        .match_pairs()
                        .unwrap_or(offset);
                    let (_, col) = self.offset_to_line_col(new_offset);
                    (new_offset, ColPosition::Col(col))
                }
            }
        }
    }

    pub fn previous_unmatched(&self, c: char, offset: usize) -> Option<usize> {
        if self.syntax_tree.is_some() {
            self.find_tag(offset, true, &c.to_string())
        } else {
            WordCursor::new(&self.rope, offset).previous_unmatched(c)
        }
    }

    fn find_matching_pair(&self, offset: usize) -> Option<usize> {
        let tree = self.syntax_tree.as_ref()?;
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

    fn find_tag(&self, offset: usize, previous: bool, tag: &str) -> Option<usize> {
        let tree = self.syntax_tree.as_ref()?;
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

    pub fn prev_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_code_boundary()
    }

    pub fn next_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_code_boundary()
    }

    pub fn update_syntax_tree(&mut self, rev: u64, tree: Tree) {
        if rev != self.rev {
            return;
        }
        self.syntax_tree = Some(Arc::new(tree));
    }

    pub fn update_styles(
        &mut self,
        rev: u64,
        highlights: Spans<Style>,
        semantic_tokens: bool,
    ) {
        if rev != self.rev {
            return;
        }
        if semantic_tokens {
            self.semantic_tokens = true;
        }
        self.styles = Arc::new(highlights);
        *self.line_styles.borrow_mut() = vec![None; self.num_lines];
    }

    fn update_size(&mut self, inval_lines: &InvalLines) {
        if inval_lines.inval_count != inval_lines.new_count {
            self.num_lines = self.num_lines();
        }
        if self.max_len_line >= inval_lines.start_line
            && self.max_len_line < inval_lines.start_line + inval_lines.inval_count
        {
            let (max_len, max_len_line) = self.get_max_line_len();
            self.max_len = max_len;
            self.max_len_line = max_len_line;
        } else {
            let mut max_len = 0;
            let mut max_len_line = 0;
            for line in inval_lines.start_line
                ..inval_lines.start_line + inval_lines.new_count
            {
                let line_len = self.line_len(line);
                if line_len > max_len {
                    max_len = line_len;
                    max_len_line = line;
                }
            }
            if max_len > self.max_len {
                self.max_len = max_len;
                self.max_len_line = max_len_line;
            } else if self.max_len >= inval_lines.start_line {
                self.max_len_line = self.max_len_line + inval_lines.new_count
                    - inval_lines.inval_count;
            }
        }
    }

    fn update_line_styles(&mut self, delta: &RopeDelta, inval_lines: &InvalLines) {
        Arc::make_mut(&mut self.styles).apply_shape(delta);
        let mut line_styles = self.line_styles.borrow_mut();
        let mut right = line_styles.split_off(inval_lines.start_line);
        let right = &right[inval_lines.inval_count..];
        let mut new = vec![None; inval_lines.new_count];
        line_styles.append(&mut new);
        line_styles.extend_from_slice(right);
    }

    fn update_text_layouts(&mut self, inval_lines: &InvalLines) {
        let mut text_layouts = self.text_layouts.borrow_mut();
        let mut right = text_layouts.split_off(inval_lines.start_line);
        let right = &right[inval_lines.inval_count..];

        let mut new = vec![None; inval_lines.new_count];
        text_layouts.append(&mut new);
        text_layouts.extend_from_slice(right);
    }

    fn mk_new_rev(
        &self,
        undo_group: usize,
        delta: RopeDelta,
    ) -> (Revision, Rope, Rope, Subset) {
        let (ins_delta, deletes) = delta.factor();

        let deletes_at_rev = &self.deletes_from_union;

        let union_ins_delta = ins_delta.transform_expand(&deletes_at_rev, true);
        let mut new_deletes = deletes.transform_expand(&deletes_at_rev);

        let new_inserts = union_ins_delta.inserted_subset();
        if !new_inserts.is_empty() {
            new_deletes = new_deletes.transform_expand(&new_inserts);
        }
        let cur_deletes_from_union = &self.deletes_from_union;
        let text_ins_delta =
            union_ins_delta.transform_shrink(cur_deletes_from_union);
        let text_with_inserts = text_ins_delta.apply(&self.rope);
        let rebased_deletes_from_union =
            cur_deletes_from_union.transform_expand(&new_inserts);

        let undone = self.undone_groups.contains(&undo_group);
        let new_deletes_from_union = {
            let to_delete = if undone { &new_inserts } else { &new_deletes };
            rebased_deletes_from_union.union(to_delete)
        };

        let (new_text, new_tombstones) = shuffle(
            &text_with_inserts,
            &self.tombstones,
            &rebased_deletes_from_union,
            &new_deletes_from_union,
        );

        let head_rev = &self.revs.last().unwrap();
        (
            Revision {
                max_undo_so_far: std::cmp::max(undo_group, head_rev.max_undo_so_far),
                edit: Contents::Edit {
                    undo_group,
                    inserts: new_inserts,
                    deletes: new_deletes,
                },
            },
            new_text,
            new_tombstones,
            new_deletes_from_union,
        )
    }

    fn calculate_undo_group(&mut self) -> usize {
        let has_undos = !self.live_undos.is_empty();
        let is_unbroken_group =
            !self.this_edit_type.breaks_undo_group(self.last_edit_type);

        if has_undos && is_unbroken_group {
            *self.live_undos.last().unwrap()
        } else {
            let undo_group = self.undo_group_id;
            self.live_undos.truncate(self.cur_undo);
            self.live_undos.push(undo_group);
            self.cur_undo += 1;
            self.undo_group_id += 1;
            undo_group
        }
    }

    fn apply_edit(
        &mut self,
        proxy: Arc<LapceProxy>,
        delta: &RopeDelta,
        new_rev: Revision,
        new_text: Rope,
        new_tombstones: Rope,
        new_deletes_from_union: Subset,
    ) {
        if !self.loaded {
            println!("not loaded");
            return;
        }
        self.rev += 1;
        self.dirty = true;

        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;

        if !self.local {
            proxy.update(self.id, &delta, self.rev);
        }

        self.revs.push(new_rev);
        self.rope = new_text.clone();
        self.tombstones = new_tombstones;
        self.deletes_from_union = new_deletes_from_union;
        self.code_actions.clear();
        self.syntax_tree = None;

        let logical_start_line = self.rope.line_of_offset(iv.start);
        let new_logical_end_line = self.rope.line_of_offset(iv.start + newlen) + 1;
        let old_hard_count = old_logical_end_line - logical_start_line;
        let new_hard_count = new_logical_end_line - logical_start_line;

        let inval_lines = InvalLines {
            start_line: logical_start_line,
            inval_count: old_hard_count,
            new_count: new_hard_count,
        };
        self.update_size(&inval_lines);
        self.update_text_layouts(&inval_lines);
        self.update_line_styles(&delta, &inval_lines);
        self.notify_update();
    }

    pub fn edit_multiple(
        &mut self,
        ctx: &mut EventCtx,
        edits: Vec<(&Selection, &str)>,
        proxy: Arc<LapceProxy>,
        edit_type: EditType,
    ) -> RopeDelta {
        let mut builder = DeltaBuilder::new(self.len());
        let mut interval_rope = Vec::new();
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.regions() {
                interval_rope.push((region.min(), region.max(), rope.clone()));
            }
        }
        interval_rope.sort_by(|a, b| {
            if a.0 == b.0 && a.1 == b.1 {
                Ordering::Equal
            } else if a.1 == b.0 {
                Ordering::Less
            } else {
                a.1.cmp(&b.0)
            }
        });
        for (start, end, rope) in interval_rope.into_iter() {
            builder.replace(start..end, rope);
        }
        let delta = builder.build();
        self.this_edit_type = edit_type;
        let undo_group = self.calculate_undo_group();
        self.last_edit_type = self.this_edit_type;

        let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
            self.mk_new_rev(undo_group, delta.clone());

        self.apply_edit(
            proxy,
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        delta
    }

    pub fn edit(
        &mut self,
        ctx: &mut EventCtx,
        selection: &Selection,
        content: &str,
        proxy: Arc<LapceProxy>,
        edit_type: EditType,
    ) -> RopeDelta {
        self.edit_multiple(ctx, vec![(selection, content)], proxy, edit_type)
    }

    pub fn do_undo(&mut self, proxy: Arc<LapceProxy>) -> Option<RopeDelta> {
        if self.cur_undo > 1 {
            self.cur_undo -= 1;
            self.undos.insert(self.live_undos[self.cur_undo]);
            self.last_edit_type = EditType::Undo;
            Some(self.undo(self.undos.clone(), proxy))
        } else {
            None
        }
    }

    pub fn do_redo(&mut self, proxy: Arc<LapceProxy>) -> Option<RopeDelta> {
        if self.cur_undo < self.live_undos.len() {
            self.undos.remove(&self.live_undos[self.cur_undo]);
            self.cur_undo += 1;
            self.last_edit_type = EditType::Redo;
            Some(self.undo(self.undos.clone(), proxy))
        } else {
            None
        }
    }

    fn undo(
        &mut self,
        groups: BTreeSet<usize>,
        proxy: Arc<LapceProxy>,
    ) -> RopeDelta {
        let (new_rev, new_deletes_from_union) = self.compute_undo(&groups);
        let delta = Delta::synthesize(
            &self.tombstones,
            &self.deletes_from_union,
            &new_deletes_from_union,
        );
        let new_text = delta.apply(&self.rope);
        let new_tombstones = shuffle_tombstones(
            &self.rope,
            &self.tombstones,
            &self.deletes_from_union,
            &new_deletes_from_union,
        );
        self.undone_groups = groups;
        self.apply_edit(
            proxy,
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );
        delta
    }

    fn deletes_from_union_before_index(
        &self,
        rev_index: usize,
        invert_undos: bool,
    ) -> Cow<Subset> {
        let mut deletes_from_union = Cow::Borrowed(&self.deletes_from_union);
        let mut undone_groups = Cow::Borrowed(&self.undone_groups);

        // invert the changes to deletes_from_union starting in the present and working backwards
        for rev in self.revs[rev_index..].iter().rev() {
            deletes_from_union = match rev.edit {
                Contents::Edit {
                    ref inserts,
                    ref deletes,
                    ref undo_group,
                    ..
                } => {
                    if undone_groups.contains(undo_group) {
                        // no need to un-delete undone inserts since we'll just shrink them out
                        Cow::Owned(deletes_from_union.transform_shrink(inserts))
                    } else {
                        let un_deleted = deletes_from_union.subtract(deletes);
                        Cow::Owned(un_deleted.transform_shrink(inserts))
                    }
                }
                Contents::Undo {
                    ref toggled_groups,
                    ref deletes_bitxor,
                } => {
                    if invert_undos {
                        let new_undone = undone_groups
                            .symmetric_difference(toggled_groups)
                            .cloned()
                            .collect();
                        undone_groups = Cow::Owned(new_undone);
                        Cow::Owned(deletes_from_union.bitxor(deletes_bitxor))
                    } else {
                        deletes_from_union
                    }
                }
            }
        }
        deletes_from_union
    }

    fn find_first_undo_candidate_index(
        &self,
        toggled_groups: &BTreeSet<usize>,
    ) -> usize {
        // find the lowest toggled undo group number
        if let Some(lowest_group) = toggled_groups.iter().cloned().next() {
            for (i, rev) in self.revs.iter().enumerate().rev() {
                if rev.max_undo_so_far < lowest_group {
                    return i + 1; // +1 since we know the one we just found doesn't have it
                }
            }
            0
        } else {
            // no toggled groups, return past end
            self.revs.len()
        }
    }

    fn compute_undo(&self, groups: &BTreeSet<usize>) -> (Revision, Subset) {
        let toggled_groups = self
            .undone_groups
            .symmetric_difference(&groups)
            .cloned()
            .collect();
        let first_candidate = self.find_first_undo_candidate_index(&toggled_groups);
        // the `false` below: don't invert undos since our first_candidate is based on the current undo set, not past
        let mut deletes_from_union = self
            .deletes_from_union_before_index(first_candidate, false)
            .into_owned();

        for rev in &self.revs[first_candidate..] {
            if let Contents::Edit {
                ref undo_group,
                ref inserts,
                ref deletes,
                ..
            } = rev.edit
            {
                if groups.contains(undo_group) {
                    if !inserts.is_empty() {
                        deletes_from_union =
                            deletes_from_union.transform_union(inserts);
                    }
                } else {
                    if !inserts.is_empty() {
                        deletes_from_union =
                            deletes_from_union.transform_expand(inserts);
                    }
                    if !deletes.is_empty() {
                        deletes_from_union = deletes_from_union.union(deletes);
                    }
                }
            }
        }

        let deletes_bitxor = self.deletes_from_union.bitxor(&deletes_from_union);
        let max_undo_so_far = self.revs.last().unwrap().max_undo_so_far;
        (
            Revision {
                max_undo_so_far,
                edit: Contents::Undo {
                    toggled_groups,
                    deletes_bitxor,
                },
            },
            deletes_from_union,
        )
    }

    pub fn update_edit_type(&mut self) {
        self.last_edit_type = self.this_edit_type;
        self.this_edit_type = EditType::Other
    }
}

fn shuffle_tombstones(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> Rope {
    // Taking the complement of deletes_from_union leads to an interleaving valid for swapped text and tombstones,
    // allowing us to use the same method to insert the text into the tombstones.
    let inverse_tombstones_map = old_deletes_from_union.complement();
    let move_delta = Delta::synthesize(
        text,
        &inverse_tombstones_map,
        &new_deletes_from_union.complement(),
    );
    move_delta.apply(tombstones)
}

fn shuffle(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> (Rope, Rope) {
    // Delta that deletes the right bits from the text
    let del_delta = Delta::synthesize(
        tombstones,
        old_deletes_from_union,
        new_deletes_from_union,
    );
    let new_text = del_delta.apply(text);
    // println!("shuffle: old={:?} new={:?} old_text={:?} new_text={:?} old_tombstones={:?}",
    //     old_deletes_from_union, new_deletes_from_union, text, new_text, tombstones);
    (
        new_text,
        shuffle_tombstones(
            text,
            tombstones,
            old_deletes_from_union,
            new_deletes_from_union,
        ),
    )
}

pub struct WordCursor<'a> {
    inner: Cursor<'a, RopeInfo>,
}

impl<'a> WordCursor<'a> {
    pub fn new(text: &'a Rope, pos: usize) -> WordCursor<'a> {
        let inner = Cursor::new(text, pos);
        WordCursor { inner }
    }

    /// Get previous boundary, and set the cursor at the boundary found.
    pub fn prev_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.prev_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_word_property(prev);
                if classify_boundary(prop_prev, prop).is_start() {
                    break;
                }
                prop = prop_prev;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn next_non_blank_char(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(next) = self.inner.next_codepoint() {
            let prop = get_word_property(next);
            if prop != WordProperty::Space {
                break;
            }
            candidate = self.inner.pos();
        }
        self.inner.set(candidate);
        candidate
    }

    /// Get next boundary, and set the cursor at the boundary found.
    pub fn next_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_word_property(next);
                if classify_boundary(prop, prop_next).is_start() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn end_boundary(&mut self) -> Option<usize> {
        self.inner.next_codepoint();
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_word_property(next);
                if classify_boundary(prop, prop_next).is_end() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn prev_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.prev_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        return candidate;
    }

    pub fn next_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.next_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        return candidate;
    }

    pub fn match_pairs(&mut self) -> Option<usize> {
        let c = self.inner.peek_next_codepoint()?;
        let other = matching_char(c)?;
        let left = matching_pair_direction(other)?;
        if left {
            self.previous_unmatched(other)
        } else {
            self.inner.next_codepoint();
            let offset = self.next_unmatched(other)?;
            Some(offset - 1)
        }
    }

    pub fn next_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.next_codepoint() {
            if current == c && n == 0 {
                return Some(self.inner.pos());
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    pub fn previous_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.prev_codepoint() {
            if current == c {
                if n == 0 {
                    return Some(self.inner.pos());
                }
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    pub fn select_word(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let end = self.next_code_boundary();
        self.inner.set(initial);
        let start = self.prev_code_boundary();
        (start, end)
    }

    /// Return the selection for the word containing the current cursor. The
    /// cursor is moved to the end of that selection.
    pub fn select_word_old(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let init_prop_after = self.inner.next_codepoint().map(get_word_property);
        self.inner.set(initial);
        let init_prop_before = self.inner.prev_codepoint().map(get_word_property);
        let mut start = initial;
        let init_boundary =
            if let (Some(pb), Some(pa)) = (init_prop_before, init_prop_after) {
                classify_boundary_initial(pb, pa)
            } else {
                WordBoundary::Both
            };
        let mut prop_after = init_prop_after;
        let mut prop_before = init_prop_before;
        if prop_after.is_none() {
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if start == initial {
                if init_boundary.is_start() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_start() {
                break;
            }
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        self.inner.set(initial);
        let mut end = initial;
        prop_after = init_prop_after;
        prop_before = init_prop_before;
        if prop_before.is_none() {
            prop_before = self.inner.next_codepoint().map(get_word_property);
            end = self.inner.pos();
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if end == initial {
                if init_boundary.is_end() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_end() {
                break;
            }
            end = self.inner.pos();
            prop_before = prop_after;
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        self.inner.set(end);
        (start, end)
    }
}

#[derive(PartialEq, Eq)]
enum WordBoundary {
    Interior,
    Start, // a boundary indicating the end of a word
    End,   // a boundary indicating the start of a word
    Both,
}

impl WordBoundary {
    fn is_start(&self) -> bool {
        *self == WordBoundary::Start || *self == WordBoundary::Both
    }

    fn is_end(&self) -> bool {
        *self == WordBoundary::End || *self == WordBoundary::Both
    }

    fn is_boundary(&self) -> bool {
        *self != WordBoundary::Interior
    }
}

fn classify_boundary(prev: WordProperty, next: WordProperty) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        (Lf, Lf) => Start,
        (Lf, Space) => Interior,
        (_, Lf) => End,
        (Lf, _) => Start,
        (Space, Space) => Interior,
        (_, Space) => End,
        (Space, _) => Start,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

fn classify_boundary_initial(
    prev: WordProperty,
    next: WordProperty,
) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        // (Lf, Other) => Start,
        // (Other, Lf) => End,
        // (Lf, Space) => Interior,
        // (Lf, Punctuation) => Interior,
        // (Space, Lf) => Interior,
        // (Punctuation, Lf) => Interior,
        // (Space, Punctuation) => Interior,
        // (Punctuation, Space) => Interior,
        _ => classify_boundary(prev, next),
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum WordProperty {
    Lf,
    Space,
    Punctuation,
    Other, // includes letters and all of non-ascii unicode
}

pub fn get_word_property(codepoint: char) -> WordProperty {
    if codepoint <= ' ' {
        if codepoint == '\n' {
            return WordProperty::Lf;
        }
        return WordProperty::Space;
    } else if codepoint <= '\u{3f}' {
        if (0xfc00fffe00000000u64 >> (codepoint as u32)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    } else if codepoint <= '\u{7f}' {
        // Hardcoded: @[\]^`{|}~
        if (0x7800000178000001u64 >> ((codepoint as u32) & 0x3f)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    }
    WordProperty::Other
}

pub fn has_unmatched_pair(line: &str) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line.chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            if !pair_first.contains_key(&key) {
                pair_first.insert(key, left);
            }
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn previous_has_unmatched_pair(line: &str, col: usize) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line[..col].chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            if !pair_first.contains_key(&key) {
                pair_first.insert(key, left);
            }
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn next_has_unmatched_pair(line: &str, col: usize) -> bool {
    let mut count = HashMap::new();
    for c in line[col..].chars() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            if !count.contains_key(&key) {
                count.insert(key, 0i32);
            }
            if left {
                count.insert(key, count.get(&key).unwrap_or(&0i32) - 1);
            } else {
                count.insert(key, count.get(&key).unwrap_or(&0i32) + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count > 0 {
            return true;
        }
    }
    false
}

pub fn matching_pair_direction(c: char) -> Option<bool> {
    Some(match c {
        '{' => true,
        '}' => false,
        '(' => true,
        ')' => false,
        '[' => true,
        ']' => false,
        _ => return None,
    })
}

pub fn matching_char(c: char) -> Option<char> {
    Some(match c {
        '{' => '}',
        '}' => '{',
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
}

fn get_git_diff(
    workspace_path: &PathBuf,
    path: &PathBuf,
    content: &str,
) -> Option<(Vec<DiffHunk>, HashMap<usize, char>)> {
    let repo = Repository::open(workspace_path.to_str()?).ok()?;
    let head = repo.head().ok()?;
    let tree = head.peel_to_tree().ok()?;
    let tree_entry = tree
        .get_path(path.strip_prefix(workspace_path).ok()?)
        .ok()?;
    let blob = repo.find_blob(tree_entry.id()).ok()?;
    let mut patch = git2::Patch::from_blob_and_buffer(
        &blob,
        None,
        content.as_bytes(),
        None,
        None,
    )
    .ok()?;
    let mut line_changes = HashMap::new();
    Some((
        (0..patch.num_hunks())
            .into_iter()
            .filter_map(|i| {
                let hunk = patch.hunk(i).ok()?;
                let hunk = DiffHunk {
                    old_start: hunk.0.old_start(),
                    old_lines: hunk.0.old_lines(),
                    new_start: hunk.0.new_start(),
                    new_lines: hunk.0.new_lines(),
                    header: String::from_utf8(hunk.0.header().to_vec()).ok()?,
                };
                let mut line_diff = 0;
                for line in 0..hunk.old_lines + hunk.new_lines {
                    if let Ok(diff_line) = patch.line_in_hunk(i, line as usize) {
                        match diff_line.origin() {
                            ' ' => {
                                let new_line = diff_line.new_lineno().unwrap();
                                let old_line = diff_line.old_lineno().unwrap();
                                line_diff = new_line as i32 - old_line as i32;
                            }
                            '-' => {
                                let old_line = diff_line.old_lineno().unwrap() - 1;
                                let new_line =
                                    (old_line as i32 + line_diff) as usize;
                                line_changes.insert(new_line, '-');
                                line_diff -= 1;
                            }
                            '+' => {
                                let new_line =
                                    diff_line.new_lineno().unwrap() as usize - 1;
                                if let Some(c) = line_changes.get(&new_line) {
                                    if c == &'-' {
                                        line_changes.insert(new_line, 'm');
                                    }
                                } else {
                                    line_changes.insert(new_line, '+');
                                }
                                line_diff += 1;
                            }
                            _ => continue,
                        }
                        diff_line.origin();
                    }
                }
                Some(hunk)
            })
            .collect(),
        line_changes,
    ))
}

//fn highlights_process(
//    language_id: String,
//    receiver: Receiver<u64>,
//    buffer_id: BufferId,
//    event_sink: ExtEventSink,
//) -> Result<()> {
//    let language = match language_id.as_ref() {
//        "rust" => LapceLanguage::Rust,
//        "go" => LapceLanguage::Go,
//        _ => return Ok(()),
//    };
//    let mut highlighter = Highlighter::new();
//    let (highlight_config, highlight_names) = new_highlight_config(language);
//    loop {
//        let rev = receiver.recv()?;
//        let rope_str = {
//            let state = LAPCE_APP_STATE.get_active_state();
//            let editor_split = state.editor_split.lock();
//            let buffer = editor_split.buffers.get(&buffer_id).unwrap();
//            if buffer.rev != rev {
//                continue;
//            } else {
//                buffer.slice_to_cow(..buffer.len()).to_string()
//            }
//        };
//
//        let mut highlights: Vec<(usize, usize, Highlight)> = Vec::new();
//        let mut current_hl: Option<Highlight> = None;
//        for hightlight in highlighter
//            .highlight(&highlight_config, &rope_str.as_bytes(), None, |_| None)
//            .unwrap()
//        {
//            if let Ok(highlight) = hightlight {
//                match highlight {
//                    HighlightEvent::Source { start, end } => {
//                        if let Some(hl) = current_hl {
//                            highlights.push((start, end, hl.clone()));
//                        }
//                    }
//                    HighlightEvent::HighlightStart(hl) => {
//                        current_hl = Some(hl);
//                    }
//                    HighlightEvent::HighlightEnd => current_hl = None,
//                }
//            }
//        }
//
//        let state = LAPCE_APP_STATE.get_active_state();
//        let mut editor_split = state.editor_split.lock();
//        let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
//        if buffer.rev != rev {
//            continue;
//        }
//        buffer.highlights = highlights.to_owned();
//        buffer.line_highlights = HashMap::new();
//
//        for (view_id, editor) in editor_split.editors.iter() {
//            if editor.buffer_id.as_ref() == Some(&buffer_id) {
//                event_sink.submit_command(
//                    LAPCE_UI_COMMAND,
//                    LapceUICommand::FillTextLayouts,
//                    Target::Widget(view_id.clone()),
//                );
//            }
//        }
//    }
//}

fn language_id_from_path(path: &str) -> Option<&str> {
    let path_buf = PathBuf::from_str(path).ok()?;
    Some(match path_buf.extension()?.to_str()? {
        "rs" => "rust",
        "go" => "go",
        _ => return None,
    })
}

fn semantic_tokens_lengend(
    semantic_tokens_provider: &SemanticTokensServerCapabilities,
) -> SemanticTokensLegend {
    match semantic_tokens_provider {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
            options.legend.clone()
        }
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
            options,
        ) => options.semantic_tokens_options.legend.clone(),
    }
}

pub fn str_col(s: &str) -> usize {
    s.graphemes(true).map(grapheme_column_width).sum()
}
//
/// Returns the number of cells visually occupied by a grapheme.
/// The input string must be a single grapheme.
pub fn grapheme_column_width(s: &str) -> usize {
    // Due to this issue:
    // https://github.com/unicode-rs/unicode-width/issues/4
    // we cannot simply use the unicode-width crate to compute
    // the desired value.
    // Let's check for emoji-ness for ourselves first
    use xi_unicode::EmojiExt;
    for c in s.chars() {
        if c.is_emoji_modifier_base() || c.is_emoji_modifier() {
            // treat modifier sequences as double wide
            return 2;
        }
    }
    UnicodeWidthStr::width(s)
}
