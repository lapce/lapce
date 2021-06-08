use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use druid::{piet::PietTextLayout, FontWeight, Key, Vec2};
use druid::{
    piet::{PietText, Text, TextAttribute, TextLayoutBuilder},
    Color, Command, Data, EventCtx, ExtEventSink, Target, UpdateCtx, WidgetId,
    WindowId,
};
use druid::{Env, PaintCtx};
use git2::Repository;
use language::{new_highlight_config, new_parser, LapceLanguage};
use lsp_types::SemanticTokens;
use lsp_types::SemanticTokensLegend;
use lsp_types::SemanticTokensServerCapabilities;
use lsp_types::{
    CodeActionResponse, Position, Range, TextDocumentContentChangeEvent,
};
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
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
use tree_sitter::{Parser, Tree};
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use xi_core_lib::selection::InsertDrift;
use xi_rope::{
    interval::IntervalBounds,
    multiset::Subset,
    rope::Rope,
    spans::{Spans, SpansBuilder, SpansInfo},
    Cursor, Delta, DeltaBuilder, Interval, LinesMetric, RopeDelta, RopeInfo,
    Transformer,
};

use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    data::LapceEditorViewData,
    editor::EditorOperator,
    find::Find,
    language,
    movement::{ColPosition, LinePosition, Movement, SelRegion, Selection},
    proxy::LapceProxy,
    state::LapceTabState,
    state::LapceWorkspaceType,
    state::LAPCE_APP_STATE,
    state::{Counter, Mode},
    theme::LapceTheme,
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
    pub rope: Rope,
    pub rev: u64,
    pub language: LapceLanguage,
    pub highlights: Spans<Style>,
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
    pub text_layouts: im::Vector<Arc<Option<Arc<StyledTextLayout>>>>,
    pub line_styles: im::Vector<Option<Arc<Vec<(usize, usize, Style)>>>>,
    pub styles: Spans<Style>,
    pub semantic_tokens: bool,
    pub language: Option<LapceLanguage>,
    pub max_len: usize,
    pub max_len_line: usize,
    pub num_lines: usize,
    pub rev: u64,
    pub dirty: bool,
    pub loaded: bool,
    update_sender: Arc<Sender<UpdateEvent>>,

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
}

impl BufferNew {
    pub fn new(path: PathBuf, update_sender: Arc<Sender<UpdateEvent>>) -> Self {
        let rope = Rope::from_str("").unwrap();
        let mut buffer = Self {
            id: BufferId::next(),
            rope,
            language: LapceLanguage::from_path(&path),
            path,
            text_layouts: im::Vector::new(),
            styles: SpansBuilder::new(0).build(),
            line_styles: im::Vector::new(),
            semantic_tokens: false,
            max_len: 0,
            max_len_line: 0,
            num_lines: 0,
            rev: 0,
            loaded: false,
            dirty: false,
            update_sender,

            revs: vec![Revision {
                max_undo_so_far: 0,
                edit: Contents::Undo {
                    toggled_groups: BTreeSet::new(),
                    deletes_bitxor: Subset::new(0),
                },
            }],
            cur_undo: 0,
            undos: BTreeSet::new(),
            undo_group_id: 0,
            live_undos: Vec::new(),
            deletes_from_union: Subset::new(0),
            undone_groups: BTreeSet::new(),
            tombstones: Rope::default(),

            last_edit_type: EditType::Other,
            this_edit_type: EditType::Other,
        };
        buffer.text_layouts =
            im::Vector::from(vec![Arc::new(None); buffer.num_lines()]);
        buffer.line_styles = im::Vector::from(vec![None; buffer.num_lines()]);
        buffer
    }

    pub fn load_content(&mut self, content: &str) {
        let delta = Delta::simple_edit(Interval::new(0, 0), Rope::from(content), 0);
        let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
            self.mk_new_rev(0, delta.clone());
        self.revs.push(new_rev);
        self.rope = new_text;
        self.tombstones = new_tombstones;
        self.deletes_from_union = new_deletes_from_union;

        let (max_len, max_len_line) = self.get_max_line_len();
        self.max_len = max_len;
        self.max_len_line = max_len_line;
        self.num_lines = self.num_lines();
        self.text_layouts = im::Vector::from(vec![Arc::new(None); self.num_lines]);
        self.line_styles = im::Vector::from(vec![None; self.num_lines]);
        self.loaded = true;
        self.notify_update();
    }

    pub fn notify_update(&self) {
        if let Some(language) = self.language {
            if !self.semantic_tokens {
                self.update_sender.send(UpdateEvent::Buffer(BufferUpdate {
                    id: self.id,
                    rope: self.rope.clone(),
                    rev: self.rev,
                    language,
                    highlights: self.styles.clone(),
                }));
            }
        }
    }

    pub fn retrieve_file(
        &self,
        proxy: Arc<LapceProxy>,
        tab_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        let id = self.id;
        let path = self.path.clone();
        thread::spawn(move || {
            let content = { proxy.new_buffer(id, path.clone()).unwrap() };
            println!("load file got content");
            event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::LoadBuffer { id, content },
                Target::Widget(tab_id),
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

    pub fn line_content(&self, line: usize) -> String {
        self.slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
            .to_string()
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line
        } else {
            line
        };
        self.rope.offset_of_line(line)
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
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

    pub fn update_line_layouts(
        &mut self,
        text: &mut PietText,
        line: usize,
        theme: &Arc<HashMap<String, Color>>,
        env: &Env,
    ) {
        if line >= self.text_layouts.len() {
            return;
        }
        let styles = self.get_line_styles(line);
        if self.text_layouts[line].is_none() || {
            let old_styles =
                (*self.text_layouts[line]).as_ref().unwrap().styles.clone();
            if old_styles.same(&styles) {
                false
            } else {
                let changed = *old_styles != *styles;
                if changed {
                    println!("stlye changed {}", line);
                }
                changed
            }
        } {
            let line_content = self.line_content(line);
            self.text_layouts[line] = Arc::new(Some(Arc::new(
                self.get_text_layout(text, line_content, styles, theme, env),
            )));
            return;
        }
    }

    fn get_line_styles(&mut self, line: usize) -> Arc<Vec<(usize, usize, Style)>> {
        if let Some(line_styles) = self.line_styles[line].as_ref() {
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
        self.line_styles[line] = Some(line_styles.clone());
        line_styles
    }

    pub fn get_text_layout(
        &mut self,
        text: &mut PietText,
        line_content: String,
        styles: Arc<Vec<(usize, usize, Style)>>,
        theme: &Arc<HashMap<String, Color>>,
        env: &Env,
    ) -> StyledTextLayout {
        let mut layout_builder = text
            .new_text_layout(line_content.replace('\t', "    "))
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
        for (start, end, style) in styles.iter() {
            if let Some(fg_color) = style.fg_color.as_ref() {
                if let Some(fg_color) = theme.get(fg_color) {
                    layout_builder = layout_builder.range_attribute(
                        start..end,
                        TextAttribute::TextColor(fg_color.clone()),
                    );
                }
            }
        }
        let layout = layout_builder.build().unwrap();
        StyledTextLayout {
            layout,
            text: line_content,
            styles,
        }
    }

    pub fn col_x(&self, line: usize, col: usize, width: f64) -> f64 {
        let line_content = self.line_content(line);
        let col = if col > line_content.len() {
            line_content.len()
        } else {
            col
        };
        let x = (line_content[..col]
            .chars()
            .filter_map(|c| if c == '\t' { Some('\t') } else { None })
            .count()
            * 3
            + col) as f64
            * width;
        x
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
            line: line as u64,
            character: col as u64,
        }
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let line = self.line_of_offset(offset);
        (line, offset - self.offset_of_line(line))
    }

    pub fn line_end_offset(&self, line: usize, caret: bool) -> usize {
        self.offset_of_line(line) + self.line_max_col(line, caret)
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end_offset(line, caret)
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    pub fn line_max_col(&self, line: usize, caret: bool) -> usize {
        let line_content = self.line_content(line);
        let n = self.line_len(line);
        match n {
            n if n == 0 => 0,
            n if !line_content.ends_with("\n") => match caret {
                true => n,
                false => n - 1,
            },
            n if n == 1 => 0,
            n => match caret {
                true => n - 1,
                false => n - 2,
            },
        }
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        let max_col = self.line_max_col(line, caret);
        match horiz {
            &ColPosition::Col(n) => match max_col > n {
                true => n,
                false => max_col,
            },
            &ColPosition::End => max_col,
            _ => 0,
        }
    }

    pub fn update_selection(
        &self,
        selection: &Selection,
        count: usize,
        movement: &Movement,
        caret: bool,
        across_line: bool,
        modify: bool,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            let region = self.update_region(
                region,
                count,
                movement,
                caret,
                across_line,
                modify,
            );
            new_selection.add_region(region);
        }
        new_selection
    }

    pub fn update_region(
        &self,
        region: &SelRegion,
        count: usize,
        movement: &Movement,
        caret: bool,
        across_line: bool,
        modify: bool,
    ) -> SelRegion {
        let (end, horiz) = self.move_offset(
            region.end(),
            region.horiz(),
            count,
            movement,
            caret,
            across_line,
        );

        let start = match modify {
            true => region.start(),
            false => end,
        };

        SelRegion::new(start, end, Some(horiz))
    }

    pub fn move_offset(
        &self,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        caret: bool,
        across_line: bool,
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
                let new_offset = if offset < count {
                    0
                } else if across_line {
                    offset - count
                } else if offset - count > line_start_offset {
                    offset - count
                } else {
                    line_start_offset
                };
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::Right => {
                let line_end = self.offset_line_end(offset, caret);

                let mut new_offset = offset + count;
                if new_offset > line_end {
                    new_offset = line_end;
                }

                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::Up => {
                let line = self.line_of_offset(offset);
                let line = if line > count { line - count } else { 0 };
                let col = self.line_horiz_col(line, &horiz, caret);
                let new_offset = self.offset_of_line(line) + col;
                (new_offset, horiz)
            }
            Movement::Down => {
                let last_line = self.last_line();
                let line = self.line_of_offset(offset) + count;
                let line = if line > last_line { last_line } else { line };
                let col = self.line_horiz_col(line, &horiz, caret);
                let new_offset = self.offset_of_line(line) + col;
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
                let new_offset = self.offset_line_end(offset, caret);
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
                let col = self.line_horiz_col(line, &horiz, caret);
                let new_offset = self.offset_of_line(line) + col;
                (new_offset, horiz)
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let (_, col) = self.offset_to_line_col(new_offset);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordEndForward => {
                let new_offset = WordCursor::new(&self.rope, offset)
                    .end_boundary()
                    .unwrap_or(offset);
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
            Movement::NextUnmatched(_) => (offset, horiz),
            Movement::PreviousUnmatched(_) => (offset, horiz),
            Movement::MatchPairs => (offset, horiz),
        }
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
        self.styles = highlights;
        self.line_styles = im::Vector::from(vec![None; self.num_lines]);
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
        self.styles.apply_shape(delta);
        let right = self.line_styles.split_off(inval_lines.start_line);
        let right = right.skip(inval_lines.inval_count);
        let new = im::Vector::from(vec![None; inval_lines.new_count]);
        self.line_styles.append(new);
        self.line_styles.append(right);
    }

    fn update_text_layouts(&mut self, inval_lines: &InvalLines) {
        let right = self.text_layouts.split_off(inval_lines.start_line);
        let right = right.skip(inval_lines.inval_count);
        let new = im::Vector::from(vec![Arc::new(None); inval_lines.new_count]);
        self.text_layouts.append(new);
        self.text_layouts.append(right);
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
            return;
        }
        self.rev += 1;
        self.dirty = true;

        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;

        proxy.update(self.id, &delta, self.rev);

        self.revs.push(new_rev);
        self.rope = new_text;
        self.tombstones = new_tombstones;
        self.deletes_from_union = new_deletes_from_union;

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

    //     fn apply_delta(&mut self, proxy: Arc<LapceProxy>, delta: &RopeDelta) {
    //         if !self.loaded {
    //             return;
    //         }
    //         self.rev += 1;
    //         self.dirty = true;
    //         let (iv, newlen) = delta.summary();
    //         let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;
    //
    //         proxy.update(self.id, delta, self.rev);
    //
    //         let undo_group = self.calculate_undo_group();
    //         let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
    //             self.mk_new_rev(undo_group, delta.clone());
    //         self.revs.push(new_rev);
    //         self.rope = new_text;
    //         self.tombstones = new_tombstones;
    //         self.deletes_from_union = new_deletes_from_union;
    //
    //         let logical_start_line = self.rope.line_of_offset(iv.start);
    //         let new_logical_end_line = self.rope.line_of_offset(iv.start + newlen) + 1;
    //         let old_hard_count = old_logical_end_line - logical_start_line;
    //         let new_hard_count = new_logical_end_line - logical_start_line;
    //
    //         let inval_lines = InvalLines {
    //             start_line: logical_start_line,
    //             inval_count: old_hard_count,
    //             new_count: new_hard_count,
    //         };
    //         self.update_size(&inval_lines);
    //         self.update_text_layouts(&inval_lines);
    //         self.update_line_styles(delta, &inval_lines);
    //         self.notify_update();
    // }

    pub fn edit_multiple(
        &mut self,
        ctx: &mut EventCtx,
        edits: Vec<(&Selection, &str)>,
        proxy: Arc<LapceProxy>,
        edit_type: EditType,
    ) -> RopeDelta {
        let mut builder = DeltaBuilder::new(self.len());
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.regions() {
                builder.replace(region.min()..region.max(), rope.clone());
            }
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
            self.this_edit_type = EditType::Undo;
            Some(self.undo(self.undos.clone(), proxy))
        } else {
            None
        }
    }

    pub fn do_redo(&mut self, proxy: Arc<LapceProxy>) -> Option<RopeDelta> {
        if self.cur_undo < self.live_undos.len() {
            self.undos.remove(&self.live_undos[self.cur_undo]);
            self.cur_undo += 1;
            self.this_edit_type = EditType::Redo;
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

pub struct Buffer {
    window_id: WindowId,
    tab_id: WidgetId,
    pub id: BufferId,
    pub rope: Rope,
    highlight_names: Vec<String>,
    pub semantic_tokens: Option<Vec<(usize, usize, String)>>,
    pub highlights: Vec<(usize, usize, Highlight)>,
    pub line_highlights: HashMap<usize, Vec<(usize, usize, String)>>,
    undos: Vec<Vec<(RopeDelta, RopeDelta)>>,
    current_undo: usize,
    pub path: String,
    pub language_id: String,
    pub rev: u64,
    pub dirty: bool,
    pub code_actions: HashMap<usize, CodeActionResponse>,
    sender: Sender<(WindowId, WidgetId, BufferId, u64)>,
    pub diff: Vec<DiffHunk>,
    pub line_changes: HashMap<usize, char>,
    pub view_offset: Vec2,
    pub offset: usize,
    pub tree: Option<Tree>,
}

impl Buffer {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        buffer_id: BufferId,
        path: &str,
        sender: Sender<(WindowId, WidgetId, BufferId, u64)>,
    ) -> Buffer {
        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let content = state
            .proxy
            .lock()
            .as_ref()
            .unwrap()
            .new_buffer(buffer_id, PathBuf::from(path.to_string()))
            .unwrap();
        let rope = Rope::from_str(&content).unwrap();

        let (highlight_config, highlight_names) =
            new_highlight_config(LapceLanguage::Rust);

        let mut buffer = Buffer {
            window_id: window_id.clone(),
            tab_id: tab_id.clone(),
            id: buffer_id.clone(),
            rope,
            semantic_tokens: None,
            highlight_names,
            highlights: Vec::new(),
            line_highlights: HashMap::new(),
            undos: Vec::new(),
            current_undo: 0,
            code_actions: HashMap::new(),
            rev: 0,
            dirty: false,
            language_id: language_id_from_path(path).unwrap_or("").to_string(),
            path: path.to_string(),
            diff: Vec::new(),
            line_changes: HashMap::new(),
            view_offset: Vec2::ZERO,
            offset: 0,
            tree: None,
            sender,
        };
        buffer.update_highlights();
        buffer
    }

    pub fn reload(&mut self, rev: u64, new_content: &str) {
        if self.rev + 1 != rev {
            return;
        }
        self.rope = Rope::from_str(new_content).unwrap();
        self.semantic_tokens = None;
        self.highlights = Vec::new();
        self.line_highlights = HashMap::new();
        self.undos = Vec::new();
        self.current_undo = 0;
        self.code_actions = HashMap::new();
        self.rev += 1;
        self.dirty = false;
        self.diff = Vec::new();
        self.line_changes = HashMap::new();
        self.tree = None;
        self.update_highlights();
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn highlights_apply_delta(&mut self, delta: &RopeDelta) {
        let mut transformer = Transformer::new(delta);
        if let Some(semantic_tokens) = self.semantic_tokens.as_mut() {
            self.semantic_tokens = Some(
                semantic_tokens
                    .iter()
                    .map(|h| {
                        (
                            transformer.transform(h.0, true),
                            transformer.transform(h.1, true),
                            h.2.clone(),
                        )
                    })
                    .collect(),
            )
        } else {
            self.highlights = self
                .highlights
                .iter()
                .map(|h| {
                    (
                        transformer.transform(h.0, true),
                        transformer.transform(h.1, true),
                        h.2.clone(),
                    )
                })
                .collect()
        }
    }

    fn format_semantic_tokens(
        &self,
        semantic_tokens_provider: Option<SemanticTokensServerCapabilities>,
        value: Value,
    ) -> Option<Vec<(usize, usize, String)>> {
        let semantic_tokens: SemanticTokens = serde_json::from_value(value).ok()?;
        let semantic_tokens_provider = semantic_tokens_provider.as_ref()?;
        let semantic_lengends = semantic_tokens_lengend(semantic_tokens_provider);

        let mut highlights = Vec::new();
        let mut line = 0;
        let mut start = 0;
        for semantic_token in &semantic_tokens.data {
            if semantic_token.delta_line > 0 {
                line += semantic_token.delta_line as usize;
                start = self.offset_of_line(line);
            }
            start += semantic_token.delta_start as usize;
            let end = start + semantic_token.length as usize;
            let kind = semantic_lengends.token_types
                [semantic_token.token_type as usize]
                .as_str()
                .to_string();
            highlights.push((start, end, kind));
        }

        Some(highlights)
    }

    pub fn set_semantic_tokens(
        &mut self,
        semantic_tokens_provider: Option<SemanticTokensServerCapabilities>,
        value: Value,
    ) -> Option<()> {
        let semantic_tokens =
            self.format_semantic_tokens(semantic_tokens_provider, value)?;
        self.semantic_tokens = Some(semantic_tokens);
        self.line_highlights = HashMap::new();
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let buffer_id = self.id;
        thread::spawn(move || {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            for (view_id, editor) in state.editor_split.lock().editors.iter() {
                if editor.buffer_id.as_ref() == Some(&buffer_id) {
                    LAPCE_APP_STATE.submit_ui_command(
                        LapceUICommand::FillTextLayouts,
                        view_id.clone(),
                    );
                }
            }
        });
        None
    }

    pub fn update_highlights(&mut self) {
        self.line_highlights = HashMap::new();
        self.sender
            .send((self.window_id, self.tab_id, self.id, self.rev));
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        // state.lsp.lock().get_semantic_tokens(&self);
    }

    pub fn get_line_highligh(
        &mut self,
        line: usize,
    ) -> &Vec<(usize, usize, String)> {
        if self.line_highlights.get(&line).is_none() {
            let mut line_highlight = Vec::new();
            if let Some(semantic_tokens) = self.semantic_tokens.as_ref() {
                let start_offset = self.offset_of_line(line);
                let end_offset = self.offset_of_line(line + 1) - 1;
                for (start, end, hl) in semantic_tokens {
                    if *start > end_offset {
                        break;
                    }
                    if *start >= start_offset && *start <= end_offset {
                        let end = if *end > end_offset {
                            end_offset - start_offset
                        } else {
                            end - start_offset
                        };
                        line_highlight.push((start - start_offset, end, hl.clone()));
                    }
                }
            } else {
                let start_offset = self.offset_of_line(line);
                let end_offset = self.offset_of_line(line + 1) - 1;
                for (start, end, hl) in &self.highlights {
                    if *start > end_offset {
                        break;
                    }
                    if *start >= start_offset && *start <= end_offset {
                        let end = if *end > end_offset {
                            end_offset - start_offset
                        } else {
                            end - start_offset
                        };
                        line_highlight.push((
                            start - start_offset,
                            end,
                            self.highlight_names[hl.0].to_string(),
                        ));
                    }
                }
            }
            self.line_highlights.insert(line, line_highlight);
        }
        self.line_highlights.get(&line).unwrap()
    }

    pub fn correct_offset(&self, selection: &Selection) -> Selection {
        let mut result = Selection::new();
        for region in selection.regions() {
            let (line, col) = self.offset_to_line_col(region.start());
            let max_col = self.line_max_col(line, false);
            let (start, col) = if col > max_col {
                (self.offset_of_line(line) + max_col, max_col)
            } else {
                (region.start(), col)
            };

            let (line, col) = self.offset_to_line_col(region.start());
            let max_col = self.line_max_col(line, false);
            let end = if col > max_col {
                self.offset_of_line(line) + max_col
            } else {
                region.end()
            };

            let new_region =
                SelRegion::new(start, end, region.horiz().map(|h| h.clone()));
            result.add_region(new_region);
        }
        result
    }

    pub fn fill_horiz(&self, selection: &Selection) -> Selection {
        let mut result = Selection::new();
        for region in selection.regions() {
            let new_region = if region.horiz().is_some() {
                region.clone()
            } else {
                let (_, col) = self.offset_to_line_col(region.min());
                SelRegion::new(
                    region.start(),
                    region.end(),
                    Some(ColPosition::Col(col)),
                )
            };
            result.add_region(new_region);
        }
        result
    }

    fn update_size(
        &mut self,
        ui_state: &mut BufferUIState,
        inval_lines: &InvalLines,
    ) {
        if ui_state.max_len_line >= inval_lines.start_line
            && ui_state.max_len_line
                < inval_lines.start_line + inval_lines.inval_count
        {
            let (max_len, max_len_line) = self.get_max_line_len();
            ui_state.max_len = max_len;
            ui_state.max_len_line = max_len_line;
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
            if max_len > ui_state.max_len {
                ui_state.max_len = max_len;
                ui_state.max_len_line = max_len_line;
            } else if ui_state.max_len >= inval_lines.start_line {
                ui_state.max_len_line = ui_state.max_len_line
                    + inval_lines.new_count
                    - inval_lines.inval_count;
            }
        }
    }

    fn inv_delta(&self, delta: &RopeDelta) -> RopeDelta {
        let (ins, del) = delta.clone().factor();
        let del_rope = del.complement().delete_from(&self.rope);
        let ins = ins.inserted_subset();
        let del = del.transform_expand(&ins);
        Delta::synthesize(&del_rope, &del, &ins)
    }

    fn add_undo(&mut self, delta: &RopeDelta, new_undo_group: bool) {
        let inv_delta = self.inv_delta(delta);
        if new_undo_group {
            self.undos.truncate(self.current_undo);
            self.undos.push(vec![(delta.clone(), inv_delta)]);
            self.current_undo += 1;
        } else {
            if self.undos.is_empty() {
                self.undos.push(Vec::new());
                self.current_undo += 1;
            }
            // let mut undos = &self.undos[self.current_undo - 1];
            // let last_undo = &undos[undos.len() - 1];
            // last_undo.0.is_identity();
            self.undos[self.current_undo - 1].push((delta.clone(), inv_delta));
        }
    }

    pub fn redo(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
    ) -> Option<usize> {
        if self.current_undo >= self.undos.len() {
            return None;
        }
        let deltas = self.undos[self.current_undo].clone();
        self.current_undo += 1;
        for (delta, __) in deltas.iter() {
            self.apply_delta(ctx, ui_state, &delta);
        }
        self.update_highlights();
        Some(deltas[0].1.summary().0.start())
    }

    pub fn undo(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
    ) -> Option<usize> {
        if self.current_undo < 1 {
            return None;
        }

        self.current_undo -= 1;
        let deltas = self.undos[self.current_undo].clone();
        for (_, delta) in deltas.iter().rev() {
            self.apply_delta(ctx, ui_state, &delta);
        }
        self.update_highlights();
        Some(deltas[0].1.summary().0.start())
    }

    fn apply_delta(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        delta: &RopeDelta,
    ) {
        self.rev += 1;
        self.dirty = true;
        self.tree = None;
        ui_state.dirty = true;
        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;
        let old_logical_end_offset = self.rope.offset_of_line(old_logical_end_line);

        let content_change = get_document_content_changes(delta, self);

        self.rope = delta.apply(&self.rope);
        let content_change = match content_change {
            Some(content_change) => content_change,
            None => TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            },
        };

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        // state.plugins.lock().update(
        //     &self.id,
        //     delta,
        //     self.len(),
        //     self.num_lines(),
        //     self.rev,
        // );
        // state.lsp.lock().update(&self, &content_change, self.rev);
        state
            .proxy
            .lock()
            .as_ref()
            .unwrap()
            .update(self.id, delta, self.rev);

        let logical_start_line = self.rope.line_of_offset(iv.start);
        let new_logical_end_line = self.rope.line_of_offset(iv.start + newlen) + 1;
        let old_hard_count = old_logical_end_line - logical_start_line;
        let new_hard_count = new_logical_end_line - logical_start_line;

        let inval_lines = InvalLines {
            start_line: logical_start_line,
            inval_count: old_hard_count,
            new_count: new_hard_count,
        };
        self.code_actions = HashMap::new();
        self.highlights_apply_delta(delta);
        self.update_size(ui_state, &inval_lines);
        ui_state.update_text_layouts(&inval_lines);
    }

    pub fn yank(&self, selection: &Selection) -> Vec<String> {
        selection
            .regions()
            .iter()
            .map(|region| {
                self.rope
                    .slice_to_cow(region.min()..region.max())
                    .to_string()
            })
            .collect()
    }

    pub fn do_move(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        mode: &Mode,
        movement: &Movement,
        selection: &Selection,
        operator: Option<EditorOperator>,
        count: Option<usize>,
    ) -> Selection {
        if let Some(operator) = operator {
            let selection = movement.update_selection(
                selection,
                &self,
                count.unwrap_or(1),
                true,
                true,
            );
            let mut new_selection = Selection::new();
            for region in selection.regions() {
                let start_line = self.line_of_offset(region.min());
                let end_line = self.line_of_offset(region.max());
                let new_region = if movement.is_vertical() {
                    let region = SelRegion::new(
                        self.offset_of_line(start_line),
                        self.offset_of_line(end_line + 1),
                        Some(ColPosition::Col(0)),
                    );
                    region
                } else {
                    if movement.is_inclusive() {
                        SelRegion::new(
                            region.min(),
                            region.max() + 1,
                            region.horiz().map(|h| h.clone()),
                        )
                    } else {
                        region.clone()
                    }
                };
                new_selection.add_region(new_region);
            }
            match operator {
                EditorOperator::Delete(_) => {
                    let delta = self.edit(ctx, ui_state, "", &new_selection, true);
                    new_selection.apply_delta(&delta, true, InsertDrift::Default)
                }
                EditorOperator::Yank(_) => new_selection,
            }
        } else {
            movement.update_selection(
                &selection,
                &self,
                count.unwrap_or(1),
                mode == &Mode::Insert,
                mode == &Mode::Visual,
            )
        }
    }

    pub fn edit_multiple(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        edits: Vec<(&Selection, &str)>,
        new_undo_group: bool,
    ) -> RopeDelta {
        let mut builder = DeltaBuilder::new(self.len());
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.regions() {
                builder.replace(region.min()..region.max(), rope.clone());
            }
        }
        let delta = builder.build();
        self.add_undo(&delta, new_undo_group);
        self.apply_delta(ctx, ui_state, &delta);
        self.update_highlights();
        delta
    }

    pub fn edit(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        content: &str,
        selection: &Selection,
        new_undo_group: bool,
    ) -> RopeDelta {
        self.edit_multiple(ctx, ui_state, vec![(selection, content)], new_undo_group)
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.rope.offset_of_line(line);
        let word_boundary =
            WordCursor::new(&self.rope, line_start_offset).next_non_blank_char();
        let indent = self.rope.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        self.rope.line_of_offset(offset)
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.rope.offset_of_line(line)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let line = self.line_of_offset(offset);
        (line, offset - self.offset_of_line(line))
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let (line, col) = self.offset_to_line_col(offset);
        Position {
            line: line as u64,
            character: col as u64,
        }
    }

    pub fn offset_of_position(&self, position: &Position) -> Option<usize> {
        let line = position.line as usize;
        if line > self.num_lines() {
            return None;
        }
        let offset = self.offset_of_line(line) + position.character as usize;
        if offset > self.len() {
            return None;
        }
        Some(offset)
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.rope.len()) + 1
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
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

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.rope.len())
    }

    pub fn line_max_col(&self, line: usize, include_newline: bool) -> usize {
        match self.offset_of_line(line + 1) - self.offset_of_line(line) {
            n if n == 0 => 0,
            n if n == 1 => 0,
            n => match include_newline {
                true => n - 1,
                false => n - 2,
            },
        }
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        include_newline: bool,
    ) -> usize {
        let max_col = self.line_max_col(line, include_newline);
        match horiz {
            &ColPosition::Col(n) => match max_col > n {
                true => n,
                false => max_col,
            },
            &ColPosition::End => max_col,
            _ => 0,
        }
    }

    pub fn line_end(&self, line: usize, include_newline: bool) -> usize {
        let line_start_offset = self.offset_of_line(line);
        let line_end_offset = self.offset_of_line(line + 1);
        let line_end_offset = if line_end_offset - line_start_offset <= 1 {
            line_start_offset
        } else {
            if include_newline {
                line_end_offset - 1
            } else {
                line_end_offset - 2
            }
        };
        line_end_offset
    }

    pub fn line_end_offset(&self, offset: usize, include_newline: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end(line, include_newline)
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        WordCursor::new(&self.rope, offset)
            .inner
            .peek_next_codepoint()
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let line_start_offset = self.rope.offset_of_line(line);
        WordCursor::new(&self.rope, line_start_offset).next_non_blank_char()
    }

    pub fn word_forward(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_boundary().unwrap()
    }

    pub fn word_end_forward(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).end_boundary().unwrap()
    }

    pub fn word_backword(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_boundary().unwrap()
    }

    pub fn match_pairs(&self, offset: usize) -> Option<usize> {
        WordCursor::new(&self.rope, offset).match_pairs()
    }

    pub fn previous_unmmatched(&self, offset: usize, c: char) -> Option<usize> {
        WordCursor::new(&self.rope, offset).previous_unmatched(c)
    }

    pub fn next_unmmatched(&self, offset: usize, c: char) -> Option<usize> {
        WordCursor::new(&self.rope, offset).next_unmatched(c)
    }

    pub fn prev_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_code_boundary()
    }

    pub fn next_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_code_boundary()
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        WordCursor::new(&self.rope, offset).select_word()
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    pub fn update_line_layouts(
        &mut self,
        text: &mut PietText,
        line: usize,
        env: &Env,
    ) -> bool {
        // if line >= self.num_lines() {
        //     return false;
        // }

        // let theme = &LAPCE_STATE.theme;

        // let line_hightlight = self.get_line_highligh(line).clone();
        // if self.text_layouts[line].is_none()
        //     || self.text_layouts[line]
        //         .as_ref()
        //         .as_ref()
        //         .unwrap()
        //         .highlights
        //         != line_hightlight
        // {
        //     let line_content = self
        //         .slice_to_cow(
        //             self.offset_of_line(line)..self.offset_of_line(line + 1),
        //         )
        //         .to_string();
        //     self.text_layouts[line] = Arc::new(Some(self.get_text_layout(
        //         text,
        //         theme,
        //         line,
        //         line_content,
        //         env,
        //     )));
        //     return true;
        // }

        false
    }

    // pub fn get_text_layout(
    //     &mut self,
    //     text: &mut PietText,
    //     theme: &HashMap<String, Color>,
    //     line: usize,
    //     line_content: String,
    //     env: &Env,
    // ) -> HighlightTextLayout {
    //     let mut layout_builder = text
    //         .new_text_layout(line_content.clone())
    //         .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
    //         .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
    //     for (start, end, hl) in self.get_line_highligh(line) {
    //         if let Some(color) = theme.get(hl) {
    //             layout_builder = layout_builder.range_attribute(
    //                 start..end,
    //                 TextAttribute::TextColor(color.clone()),
    //             );
    //         }
    //     }
    //     let layout = layout_builder.build().unwrap();
    //     HighlightTextLayout {
    //         layout,
    //         text: line_content,
    //         highlights: self.get_line_highligh(line).clone(),
    //     }
    // }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
    }
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
            return Some(candidate - 1);
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

impl BufferUIState {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        buffer_id: BufferId,
        lines: usize,
        max_len: usize,
        max_len_line: usize,
    ) -> BufferUIState {
        BufferUIState {
            window_id,
            tab_id,
            id: buffer_id,
            text_layouts: vec![Arc::new(None); lines],
            line_changes: HashMap::new(),
            max_len,
            max_len_line,
            dirty: false,
        }
    }

    fn update_text_layouts(&mut self, inval_lines: &InvalLines) {
        let mut new_layouts = Vec::new();
        if inval_lines.start_line < self.text_layouts.len() {
            new_layouts
                .extend_from_slice(&self.text_layouts[..inval_lines.start_line]);
        }
        for _ in 0..inval_lines.new_count {
            new_layouts.push(Arc::new(None));
        }
        if inval_lines.start_line + inval_lines.inval_count < self.text_layouts.len()
        {
            new_layouts.extend_from_slice(
                &self.text_layouts
                    [inval_lines.start_line + inval_lines.inval_count..],
            );
        }
        self.text_layouts = new_layouts;
    }

    pub fn update_line_layouts(
        &mut self,
        text: &mut PietText,
        buffer: &mut Buffer,
        line: usize,
        env: &Env,
    ) -> bool {
        if line >= self.text_layouts.len() {
            return false;
        }

        //let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let theme = &LAPCE_APP_STATE.theme;

        let line_hightlight = buffer.get_line_highligh(line).clone();
        if self.text_layouts[line].is_none()
            || self.text_layouts[line]
                .as_ref()
                .as_ref()
                .unwrap()
                .highlights
                != line_hightlight
        {
            let line_content = buffer
                .slice_to_cow(
                    buffer.offset_of_line(line)..buffer.offset_of_line(line + 1),
                )
                .to_string();
            self.text_layouts[line] = Arc::new(Some(Arc::new(
                self.get_text_layout(text, buffer, theme, line, line_content, env),
            )));
            return true;
        }

        false
    }

    pub fn get_text_layout(
        &mut self,
        text: &mut PietText,
        buffer: &mut Buffer,
        theme: &HashMap<String, Color>,
        line: usize,
        line_content: String,
        env: &Env,
    ) -> HighlightTextLayout {
        let mut layout_builder = text
            .new_text_layout(line_content.replace('\t', "    "))
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
        let highlights = buffer.get_line_highligh(line);
        for (start, end, hl) in highlights {
            let start = start + &line_content[..*start].matches('\t').count() * 3;
            let end = end + &line_content[..*end].matches('\t').count() * 3;
            if let Some(color) = theme.get(hl) {
                layout_builder = layout_builder.range_attribute(
                    start..end,
                    TextAttribute::TextColor(color.clone()),
                );
            } else {
                // println!("no color for {} {}", hl, start);
            }
        }
        let layout = layout_builder.build().unwrap();
        HighlightTextLayout {
            layout,
            text: line_content,
            highlights: highlights.clone(),
        }
    }
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

pub fn start_buffer_highlights(
    receiver: Receiver<(WindowId, WidgetId, BufferId, u64)>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let mut highlighter = Highlighter::new();
    let mut highlight_configs = HashMap::new();
    let mut parsers = HashMap::new();

    loop {
        let (window_id, tab_id, buffer_id, rev) = receiver.recv()?;
        let (workspace_path, language, path, rope_str) = {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let editor_split = state.editor_split.lock();
            let buffer = editor_split.buffers.get(&buffer_id).unwrap();
            let language = match buffer.language_id.as_str() {
                "rust" => LapceLanguage::Rust,
                // "go" => LapceLanguage::Go,
                _ => continue,
            };
            if buffer.rev != rev {
                continue;
            } else {
                (
                    state.workspace.lock().path.clone(),
                    language,
                    buffer.path.clone(),
                    buffer.slice_to_cow(..buffer.len()).to_string(),
                )
            }
        };

        if let Some((diff, line_changes)) =
            get_git_diff(&workspace_path, &PathBuf::from(path), &rope_str)
        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let mut editor_split = state.editor_split.lock();
            let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
            if buffer.rev != rev {
                continue;
            }
            let buffer_id = buffer.id;
            buffer.diff = diff;
            buffer.line_changes = line_changes;
            // for (view_id, editor) in editor_split.editors.iter() {
            //     if editor.buffer_id.as_ref() == Some(&buffer_id) {
            event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateLineChanges(buffer_id),
                Target::Widget(tab_id),
            );
            //     }
            // }
        }

        if !highlight_configs.contains_key(&language) {
            let (highlight_config, highlight_names) = new_highlight_config(language);
            highlight_configs.insert(language, highlight_config);
        }
        let highlight_config = highlight_configs.get(&language).unwrap();

        let mut highlights: Vec<(usize, usize, Highlight)> = Vec::new();
        let mut current_hl: Option<Highlight> = None;
        for hightlight in highlighter
            .highlight(highlight_config, &rope_str.as_bytes(), None, |_| None)
            .unwrap()
        {
            if let Ok(highlight) = hightlight {
                match highlight {
                    HighlightEvent::Source { start, end } => {
                        if let Some(hl) = current_hl {
                            highlights.push((start, end, hl.clone()));
                        }
                    }
                    HighlightEvent::HighlightStart(hl) => {
                        current_hl = Some(hl);
                    }
                    HighlightEvent::HighlightEnd => current_hl = None,
                }
            }
        }

        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let mut editor_split = state.editor_split.lock();
            let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
            if buffer.rev != rev {
                continue;
            }
            buffer.highlights = highlights.to_owned();
            buffer.line_highlights = HashMap::new();

            for (view_id, editor) in editor_split.editors.iter() {
                if editor.buffer_id.as_ref() == Some(&buffer_id) {
                    event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::FillTextLayouts,
                        Target::Widget(view_id.clone()),
                    );
                }
            }
        }

        if !parsers.contains_key(&language) {
            let parser = new_parser(language);
            parsers.insert(language, parser);
        }
        let parser = parsers.get_mut(&language).unwrap();
        if let Some(tree) = parser.parse(&rope_str, None) {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let mut editor_split = state.editor_split.lock();
            let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
            if buffer.rev != rev {
                continue;
            }
            buffer.tree = Some(tree);

            let editor = editor_split.editors.get(&editor_split.active).unwrap();
            if editor.buffer_id == Some(buffer_id) {
                editor_split.update_signature();
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use xi_rope::Delta;
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_edit() {
        let rope = Rope::from_str("0123456789\n").unwrap();
        let mut builder = DeltaBuilder::new(rope.len());
        assert_eq!(11, rope.len());
        builder.replace(11..11, Rope::from_str("a").unwrap());
        let delta = builder.build();
        let new_rope = delta.apply(&rope);
        assert_eq!("", new_rope.to_string());
    }

    #[test]
    fn test_reverse_delta() {
        let rope = Rope::from_str("0123456789").unwrap();
        let mut builder = DeltaBuilder::new(rope.len());
        builder.replace(3..4, Rope::from_str("a").unwrap());
        let delta1 = builder.build();
        println!("{:?}", delta1);
        let middle_rope = delta1.apply(&rope);

        let mut builder = DeltaBuilder::new(middle_rope.len());
        builder.replace(1..5, Rope::from_str("b").unwrap());
        let delta2 = builder.build();
        println!("{:?}", delta2);
        let new_rope = delta2.apply(&middle_rope);

        let (ins1, del1) = delta1.factor();
        let in1 = ins1.inserted_subset();
        let (ins2, del2) = delta2.factor();
        let in2 = ins2.inserted_subset();

        ins2.transform_expand(&in1, true)
            .inserted_subset()
            .transform_union(&in1);
        // del1.transform_expand(&in1).transform_expand(&del2);
        // let del1 = del1.transform_expand(&in1).transform_expand(&in2);
        // let del2 = del2.transform_expand(&in2);
        // let del = del1.union(&del2);
        let union = ins2.transform_expand(&in1, true).apply(&ins1.apply(&rope));

        println!("{}", union);

        // if delta1.is_simple_delete()
    }
}

fn language_id_from_path(path: &str) -> Option<&str> {
    let path_buf = PathBuf::from_str(path).ok()?;
    Some(match path_buf.extension()?.to_str()? {
        "rs" => "rust",
        "go" => "go",
        _ => return None,
    })
}

pub fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let text = String::from(node);

        let (start, end) = interval.start_end();
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: buffer.offset_to_position(end),
            }),
            range_length: Some((end - start) as u64),
            text,
        };

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = buffer.offset_to_position(end);

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: end_position,
            }),
            range_length: Some((end - start) as u64),
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
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
