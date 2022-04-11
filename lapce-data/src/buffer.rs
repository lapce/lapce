use druid::PaintCtx;
use druid::{piet::PietTextLayout, Vec2};
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    Data, ExtEventSink, Target, WidgetId, WindowId,
};
use lapce_core::indent::{auto_detect_indent_style, IndentStyle};
use lapce_core::style::line_styles;
use lapce_core::syntax::Syntax;
use lapce_rpc::buffer::{BufferHeadResponse, BufferId, NewBufferResponse};
use lapce_rpc::style::{LineStyle, LineStyles, Style};
use lsp_types::SemanticTokensLegend;
use lsp_types::SemanticTokensServerCapabilities;
use lsp_types::{CodeActionResponse, Position};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::{self, AtomicU64};
use std::{borrow::Cow, collections::BTreeSet, path::PathBuf, sync::Arc, thread};
use unicode_width::UnicodeWidthChar;
use xi_rope::{
    multiset::Subset, rope::Rope, spans::Spans, Cursor, Delta, Interval, RopeDelta,
    RopeInfo,
};
use xi_unicode::EmojiExt;

use crate::buffer::data::{
    BufferData, BufferDataListener, EditableBufferData, DEFAULT_INDENT,
};
use crate::buffer::decoration::BufferDecoration;
use crate::config::{Config, LapceTheme};
use crate::editor::EditorLocationNew;
use crate::find::FindProgress;
use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    find::Find,
    movement::{ColPosition, LinePosition, Movement, SelRegion, Selection},
    proxy::LapceProxy,
    state::Mode,
};

pub mod data;
pub mod decoration;

#[allow(dead_code)]
const FIND_BATCH_SIZE: usize = 500000;

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Clone, Debug)]
pub enum DiffLines {
    Left(Range<usize>),
    Both(Range<usize>, Range<usize>),
    Skip(Range<usize>, Range<usize>),
    Right(Range<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiffResult<T> {
    Left(T),
    Both(T, T),
    Right(T),
}

#[derive(Clone)]
pub struct BufferUIState {
    #[allow(dead_code)]
    window_id: WindowId,

    #[allow(dead_code)]
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
    Open(Arc<Buffer>),
}

pub struct StyledTextLayout {
    pub text: String,
    pub layout: PietTextLayout,
    pub styles: Arc<Vec<(usize, usize, Style)>>,
    pub bounds: [f64; 2],
}

pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
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

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum LocalBufferKind {
    Search,
    SourceControl,
    Empty,
    FilePicker,
    Keymap,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BufferContent {
    File(PathBuf),
    Local(LocalBufferKind),
    Value(String),
}

impl BufferContent {
    pub fn is_special(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::SourceControl
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty => false,
            },
            BufferContent::Value(_) => true,
        }
    }

    pub fn is_input(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty | LocalBufferKind::SourceControl => false,
            },
            BufferContent::Value(_) => true,
        }
    }

    pub fn is_search(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Value(_) => false,
            BufferContent::Local(local) => matches!(local, LocalBufferKind::Search),
        }
    }
}

#[derive(Clone)]
pub struct Buffer {
    data: BufferData,
    pub start_to_load: Rc<RefCell<bool>>,

    pub history_styles: im::HashMap<String, Arc<Spans<Style>>>,
    pub history_line_styles: Rc<RefCell<HashMap<String, LineStyles>>>,
    pub history_changes: im::HashMap<String, Arc<Vec<DiffLines>>>,

    pub cursor_offset: usize,
    pub scroll_offset: Vec2,

    pub code_actions: im::HashMap<usize, CodeActionResponse>,

    decoration: BufferDecoration,
}

pub struct BufferEditListener<'a> {
    decoration: &'a mut BufferDecoration,
    proxy: &'a LapceProxy,
}

impl BufferDataListener for BufferEditListener<'_> {
    fn should_apply_edit(&self) -> bool {
        self.decoration.loaded
    }

    fn on_edit_applied(&mut self, buffer: &BufferData, delta: &RopeDelta) {
        if !self.decoration.local {
            self.proxy.update(buffer.id, delta, buffer.rev);
        }

        self.decoration.update_styles(delta);
        self.decoration.find.borrow_mut().unset();
        *self.decoration.find_progress.borrow_mut() = FindProgress::Started;
        self.decoration.notify_update(buffer, Some(delta));
        self.decoration.notify_special(buffer);
    }
}

impl Buffer {
    pub fn new(
        content: BufferContent,
        tab_id: WidgetId,
        event_sink: ExtEventSink,
    ) -> Self {
        let syntax = match &content {
            BufferContent::File(path) => Syntax::init(path),
            BufferContent::Local(_) => None,
            BufferContent::Value(_) => None,
        };

        Self {
            data: BufferData::new("", content),
            decoration: BufferDecoration {
                syntax,
                line_styles: Rc::new(RefCell::new(HashMap::new())),
                semantic_styles: None,
                find: Rc::new(RefCell::new(Find::new(0))),
                find_progress: Rc::new(RefCell::new(FindProgress::Ready)),
                loaded: false,
                local: false,
                histories: im::HashMap::new(),
                tab_id,
                event_sink,
            },
            start_to_load: Rc::new(RefCell::new(false)),
            history_styles: im::HashMap::new(),
            history_line_styles: Rc::new(RefCell::new(HashMap::new())),
            history_changes: im::HashMap::new(),

            cursor_offset: 0,
            scroll_offset: Vec2::ZERO,

            code_actions: im::HashMap::new(),
        }
    }

    pub fn data(&self) -> &BufferData {
        &self.data
    }

    pub fn id(&self) -> BufferId {
        self.data.id()
    }

    pub fn rope(&self) -> &Rope {
        self.data.rope()
    }

    pub fn content(&self) -> &BufferContent {
        self.data.content()
    }

    pub fn max_len(&self) -> usize {
        self.data.max_len
    }

    pub fn max_len_line(&self) -> usize {
        self.data.max_len_line
    }

    pub fn num_lines(&self) -> usize {
        self.data.num_lines
    }

    pub fn rev(&self) -> u64 {
        self.data.rev
    }

    pub fn set_rev(&mut self, rev: u64) {
        self.data.rev = rev;
    }

    pub fn dirty(&self) -> bool {
        self.data.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.data.dirty = dirty;
    }

    pub fn set_local(mut self) -> Self {
        self.decoration.local = true;
        self
    }

    pub fn reset_revs(&mut self) {
        self.data.reset_revs();
    }

    pub fn update_edit_type(&mut self) {
        self.data.last_edit_type = EditType::Other;
    }

    pub fn loaded(&self) -> bool {
        self.decoration.loaded
    }

    pub fn local(&self) -> bool {
        self.decoration.local
    }

    pub fn syntax(&self) -> Option<&Syntax> {
        self.decoration.syntax.as_ref()
    }

    pub fn set_syntax(&mut self, syntax: Option<Syntax>) {
        self.decoration.syntax = syntax;
    }

    pub fn histories(&self) -> &im::HashMap<String, Rope> {
        &self.decoration.histories
    }

    pub fn line_styles(&self) -> Rc<RefCell<LineStyles>> {
        self.decoration.line_styles.clone()
    }

    pub fn semantic_styles(&self) -> Option<Arc<Spans<Style>>> {
        self.decoration.semantic_styles.clone()
    }

    pub fn set_semantic_styles(&mut self, styles: Option<Arc<Spans<Style>>>) {
        self.decoration.semantic_styles = styles;
    }

    pub fn find(&self) -> Rc<RefCell<Find>> {
        self.decoration.find.clone()
    }

    pub fn editable<'a>(
        &'a mut self,
        proxy: &'a LapceProxy,
    ) -> EditableBufferData<'a, BufferEditListener> {
        EditableBufferData {
            listener: BufferEditListener {
                decoration: &mut self.decoration,
                proxy,
            },
            buffer: &mut self.data,
        }
    }

    pub fn load_history(&mut self, version: &str, content: Rope) {
        self.decoration
            .histories
            .insert(version.to_string(), content.clone());
        self.trigger_history_change();
        self.retrieve_history_styles(version, content);
    }

    pub fn load_content(&mut self, content: &str) {
        self.reset_revs();

        if !content.is_empty() {
            let delta =
                Delta::simple_edit(Interval::new(0, 0), Rope::from(content), 0);
            let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
                self.data.mk_new_rev(0, delta);
            self.data.revs.push(new_rev);
            self.data.rope = new_text;
            self.data.tombstones = new_tombstones;
            self.data.deletes_from_union = new_deletes_from_union;
        }

        self.code_actions.clear();
        let (max_len, max_len_line) = self.get_max_line_len();
        self.data.max_len = max_len;
        self.data.max_len_line = max_len_line;
        self.data.num_lines = self.calc_num_lines();
        self.decoration.loaded = true;
        self.detect_indent();
        self.notify_update(None);
    }

    pub fn detect_indent(&mut self) {
        self.data.indent_style = auto_detect_indent_style(&self.data.rope)
            .unwrap_or_else(|| {
                self.syntax()
                    .map(|s| IndentStyle::from_str(s.language.indent_unit()))
                    .unwrap_or(DEFAULT_INDENT)
            });
    }

    pub fn indent_unit(&self) -> &'static str {
        self.data.indent_unit()
    }

    fn retrieve_history_styles(&self, version: &str, content: Rope) {
        if let BufferContent::File(path) = &self.data.content {
            let id = self.id();
            let path = path.clone();
            let tab_id = self.decoration.tab_id;
            let version = version.to_string();
            let event_sink = self.decoration.event_sink.clone();
            rayon::spawn(move || {
                if let Some(syntax) =
                    Syntax::init(&path).map(|s| s.parse(0, content, None))
                {
                    if let Some(styles) = syntax.styles {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHistoryStyle {
                                id,
                                path,
                                history: version,
                                highlights: styles,
                            },
                            Target::Widget(tab_id),
                        );
                    }
                }
            });
        }
    }

    fn trigger_history_change(&self) {
        if let BufferContent::File(path) = &self.data.content {
            if let Some(head) = self.histories().get("head") {
                let id = self.id();
                let rev = self.rev();
                let atomic_rev = self.data.atomic_rev.clone();
                let path = path.clone();
                let left_rope = head.clone();
                let right_rope = self.rope().clone();
                let event_sink = self.decoration.event_sink.clone();
                let tab_id = self.decoration.tab_id;
                rayon::spawn(move || {
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let changes =
                        rope_diff(left_rope, right_rope, rev, atomic_rev.clone());
                    if changes.is_none() {
                        return;
                    }
                    let changes = changes.unwrap();
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }

                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateHistoryChanges {
                            id,
                            path,
                            rev,
                            history: "head".to_string(),
                            changes: Arc::new(changes),
                        },
                        Target::Widget(tab_id),
                    );
                });
            }
        }
    }

    pub fn notify_update(&self, delta: Option<&RopeDelta>) {
        self.trigger_syntax_change(delta);
        self.trigger_history_change();
    }

    fn trigger_syntax_change(&self, delta: Option<&RopeDelta>) {
        if let BufferContent::File(path) = &self.data.content {
            if let Some(syntax) = self.decoration.syntax.clone() {
                let path = path.clone();
                let rev = self.rev();
                let text = self.data.rope.clone();
                let delta = delta.cloned();
                let atomic_rev = self.data.atomic_rev.clone();
                let event_sink = self.decoration.event_sink.clone();
                let tab_id = self.decoration.tab_id;
                rayon::spawn(move || {
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let new_syntax = syntax.parse(rev, text, delta);
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSyntax {
                            path,
                            rev,
                            syntax: new_syntax,
                        },
                        Target::Widget(tab_id),
                    );
                });
            }
        }
    }

    pub fn retrieve_file_head(
        &self,
        tab_id: WidgetId,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
    ) {
        let id = self.data.id;
        if let BufferContent::File(path) = &self.data.content {
            let path = path.clone();
            thread::spawn(move || {
                proxy.get_buffer_head(
                    id,
                    path.clone(),
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<BufferHeadResponse>(res)
                            {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::LoadBufferHead {
                                        path,
                                        content: Rope::from(resp.content),
                                        id: resp.id,
                                    },
                                    Target::Widget(tab_id),
                                );
                            }
                        }
                    }),
                )
            });
        }
    }

    pub fn retrieve_file(
        &self,
        tab_id: WidgetId,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
        locations: Vec<(WidgetId, EditorLocationNew)>,
    ) {
        if self.loaded() || *self.start_to_load.borrow() {
            return;
        }
        *self.start_to_load.borrow_mut() = true;
        let id = self.data.id;
        if let BufferContent::File(path) = &self.data.content {
            let path = path.clone();
            let proxy = proxy.clone();
            let event_sink = event_sink.clone();
            thread::spawn(move || {
                proxy.new_buffer(
                    id,
                    path.clone(),
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<NewBufferResponse>(res)
                            {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::LoadBuffer {
                                        path,
                                        content: resp.content,
                                        locations,
                                    },
                                    Target::Widget(tab_id),
                                );
                            }
                        };
                    }),
                )
            });
        }

        self.retrieve_file_head(tab_id, proxy, event_sink);
    }

    pub fn reset_find(&self, current_find: &Find) {
        {
            let find = self.decoration.find.borrow();
            if find.search_string == current_find.search_string
                && find.case_matching == current_find.case_matching
                && find.regex.as_ref().map(|r| r.as_str())
                    == current_find.regex.as_ref().map(|r| r.as_str())
                && find.whole_words == current_find.whole_words
            {
                return;
            }
        }

        let mut find = self.decoration.find.borrow_mut();
        find.unset();
        find.search_string = current_find.search_string.clone();
        find.case_matching = current_find.case_matching;
        find.regex = current_find.regex.clone();
        find.whole_words = current_find.whole_words;
        *self.decoration.find_progress.borrow_mut() = FindProgress::Started;
    }

    pub fn update_find(
        &self,
        current_find: &Find,
        start_line: usize,
        end_line: usize,
    ) {
        self.reset_find(current_find);

        let mut find_progress = self.decoration.find_progress.borrow_mut();
        let search_range = match &find_progress.clone() {
            FindProgress::Started => {
                // start incremental find on visible region
                let start = self.offset_of_line(start_line);
                let end = self.offset_of_line(end_line + 1);
                *find_progress =
                    FindProgress::InProgress(Selection::region(start, end));
                Some((start, end))
            }
            FindProgress::InProgress(searched_range) => {
                if searched_range.regions().len() == 1
                    && searched_range.min_offset() == 0
                    && searched_range.max_offset() >= self.len()
                {
                    // the entire text has been searched
                    // end find by executing multi-line regex queries on entire text
                    // stop incremental find
                    *find_progress = FindProgress::Ready;
                    Some((0, self.len()))
                } else {
                    let start = self.offset_of_line(start_line);
                    let end = self.offset_of_line(end_line + 1);
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
                        *find_progress = FindProgress::InProgress(new_range);
                    }
                    range
                }
            }
            _ => None,
        };

        let mut find = self.decoration.find.borrow_mut();
        if let Some((search_range_start, search_range_end)) = search_range {
            if !find.is_multiline_regex() {
                find.update_find(
                    &self.data.rope,
                    search_range_start,
                    search_range_end,
                    true,
                );
            } else {
                // only execute multi-line regex queries if we are searching the entire text (last step)
                if search_range_start == 0 && search_range_end == self.len() {
                    find.update_find(
                        &self.data.rope,
                        search_range_start,
                        search_range_end,
                        true,
                    );
                }
            }
        }
    }

    fn calc_num_lines(&self) -> usize {
        self.line_of_offset(self.len()) + 1
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.len())
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.data.line_of_offset(offset)
    }

    pub fn offset_line_content(&self, offset: usize) -> Cow<str> {
        self.data.offset_line_content(offset)
    }

    pub fn line_content(&self, line: usize) -> Cow<str> {
        self.data.line_content(line)
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.data.offset_of_line(line)
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        self.data.select_word(offset)
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        self.data.char_at_offset(offset)
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        self.data.first_non_blank_character_on_line(line)
    }

    pub fn get_max_line_len(&self) -> (usize, usize) {
        self.data.get_max_line_len()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn get_history_line_styles(
        &self,
        history: &str,
        line: usize,
    ) -> Option<Arc<Vec<LineStyle>>> {
        let rope = self.decoration.histories.get(history)?;
        let styles = self.history_styles.get(history)?;
        let mut cached_line_styles = self.history_line_styles.borrow_mut();
        let cached_line_styles = cached_line_styles.get_mut(history)?;
        if let Some(line_styles) = cached_line_styles.get(&line) {
            return Some(line_styles.clone());
        }

        let start_offset = rope.offset_of_line(line);
        let end_offset = rope.offset_of_line(line + 1);

        let line_styles: Vec<LineStyle> = styles
            .iter_chunks(start_offset..end_offset)
            .filter_map(|(iv, style)| {
                let start = iv.start();
                let end = iv.end();
                if start > end_offset || end < start_offset {
                    None
                } else {
                    Some(LineStyle {
                        start: if start > start_offset {
                            start - start_offset
                        } else {
                            0
                        },
                        end: end - start_offset,
                        style: style.clone(),
                    })
                }
            })
            .collect();
        let line_styles = Arc::new(line_styles);
        cached_line_styles.insert(line, line_styles.clone());
        Some(line_styles)
    }

    pub fn styles(&self) -> Option<&Arc<Spans<Style>>> {
        let styles = self
            .decoration
            .semantic_styles
            .as_ref()
            .or_else(|| self.syntax().and_then(|s| s.styles.as_ref()));
        styles
    }

    fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        if self.line_styles().borrow().get(&line).is_none() {
            let styles = self
                .decoration
                .semantic_styles
                .as_ref()
                .or_else(|| self.syntax().and_then(|s| s.styles.as_ref()));

            let line_styles = styles
                .map(|styles| line_styles(&self.data.rope, line, &styles))
                .unwrap_or_default();
            self.line_styles()
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        self.line_styles().borrow().get(&line).cloned().unwrap()
    }

    pub fn history_text_layout(
        &self,
        ctx: &mut PaintCtx,
        history: &str,
        line: usize,

        #[allow(unused_variables)] cursor_index: Option<usize>,

        bounds: [f64; 2],
        config: &Config,
    ) -> Option<PietTextLayout> {
        let rope = self.decoration.histories.get(history)?;
        let start_offset = rope.offset_of_line(line);
        let end_offset = rope.offset_of_line(line + 1);
        let line_content = rope.slice_to_cow(start_offset..end_offset).to_string();

        let mut layout_builder = ctx
            .text()
            .new_text_layout(line_content)
            .font(config.editor.font_family(), config.editor.font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );

        if let Some(styles) = self.get_history_line_styles(history, line) {
            for line_style in styles.iter() {
                if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                    if let Some(fg_color) = config.get_style_color(fg_color) {
                        layout_builder = layout_builder.range_attribute(
                            line_style.start..line_style.end,
                            TextAttribute::TextColor(fg_color.clone()),
                        );
                    }
                }
            }
        }
        Some(layout_builder.build_with_info(
            true,
            config.editor.tab_width,
            Some(bounds),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_text_layout(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        line_content: &str,
        cursor_index: Option<usize>,
        font_size: usize,
        bounds: [f64; 2],
        config: &Config,
    ) -> PietTextLayout {
        let styles = self.line_style(line);
        let mut layout_builder = ctx
            .text()
            .new_text_layout(line_content.to_string())
            .font(config.editor.font_family(), font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );

        if let Some(index) = cursor_index {
            layout_builder = layout_builder.range_attribute(
                index..index + 1,
                TextAttribute::TextColor(
                    config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        .clone(),
                ),
            );
        }

        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.get_style_color(fg_color) {
                    layout_builder = layout_builder.range_attribute(
                        line_style.start..line_style.end,
                        TextAttribute::TextColor(fg_color.clone()),
                    );
                }
            }
        }
        layout_builder.build_with_info(true, config.editor.tab_width, Some(bounds))
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        self.data.indent_on_line(line)
    }

    pub fn slice_to_cow(&self, range: Range<usize>) -> Cow<str> {
        self.data.slice_to_cow(range)
    }

    pub fn offset_to_position(&self, offset: usize, tab_width: usize) -> Position {
        self.data.offset_to_position(offset, tab_width)
    }

    pub fn offset_of_position(&self, pos: &Position, tab_width: usize) -> usize {
        self.offset_of_line_col(pos.line as usize, pos.character as usize, tab_width)
    }

    pub fn offset_of_line_col(
        &self,
        line: usize,
        col: usize,
        tab_width: usize,
    ) -> usize {
        self.data.offset_of_line_col(line, col, tab_width)
    }

    pub fn offset_to_line_col(
        &self,
        offset: usize,
        tab_width: usize,
    ) -> (usize, usize) {
        self.data.offset_to_line_col(offset, tab_width)
    }

    pub fn line_end_col(&self, line: usize, caret: bool, tab_width: usize) -> usize {
        self.data.line_end_col(line, caret, tab_width)
    }

    pub fn line_end_offset(&self, line: usize, caret: bool) -> usize {
        self.data.line_end_offset(line, caret)
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        self.data.offset_line_end(offset, caret)
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.data.line_len(line)
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        caret: bool,
        tab_width: usize,
    ) -> usize {
        self.data.line_horiz_col(line, horiz, caret, tab_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_selection(
        &self,
        selection: &Selection,
        count: usize,
        movement: &Movement,
        mode: Mode,
        modify: bool,
        code_lens: bool,
        compare: Option<&str>,
        config: &Config,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            let region = self.update_region(
                region, count, movement, mode, modify, code_lens, compare, config,
            );
            new_selection.add_region(region);
        }
        new_selection
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_region(
        &self,
        region: &SelRegion,
        count: usize,
        movement: &Movement,
        mode: Mode,
        modify: bool,
        code_lens: bool,
        compare: Option<&str>,
        config: &Config,
    ) -> SelRegion {
        let (end, horiz) = self.move_offset(
            region.end(),
            region.horiz(),
            count,
            movement,
            mode,
            code_lens,
            compare,
            config,
        );

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
        self.data.prev_grapheme_offset(offset, count, limit)
    }

    pub fn next_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        self.data.next_grapheme_offset(offset, count, limit)
    }

    pub fn diff_visual_line(&self, compare: &str, line: usize) -> usize {
        let mut visual_line = 0;
        if let Some(changes) = self.history_changes.get(compare) {
            for (_i, change) in changes.iter().enumerate() {
                match change {
                    DiffLines::Left(range) => {
                        visual_line += range.len();
                    }
                    DiffLines::Both(_, r) | DiffLines::Right(r) => {
                        if r.contains(&line) {
                            visual_line += line - r.start;
                            break;
                        }
                        visual_line += r.len();
                    }
                    DiffLines::Skip(_, r) => {
                        if r.contains(&line) {
                            break;
                        }
                        visual_line += 1;
                    }
                }
            }
        }
        visual_line
    }

    pub fn diff_actual_line_from_visual(
        &self,
        compare: &str,
        visual_line: usize,
    ) -> usize {
        let mut current_visual_line = 0;
        let mut line = 0;
        if let Some(changes) = self.history_changes.get(compare) {
            for (i, change) in changes.iter().enumerate() {
                match change {
                    DiffLines::Left(range) => {
                        current_visual_line += range.len();
                        if current_visual_line > visual_line {
                            if let Some(change) = changes.get(i + 1) {
                                match change {
                                    DiffLines::Left(_) => {}
                                    DiffLines::Both(_, r)
                                    | DiffLines::Skip(_, r)
                                    | DiffLines::Right(r) => {
                                        line = r.start;
                                    }
                                }
                            } else if i > 0 {
                                if let Some(change) = changes.get(i - 1) {
                                    match change {
                                        DiffLines::Left(_) => {}
                                        DiffLines::Both(_, r)
                                        | DiffLines::Skip(_, r)
                                        | DiffLines::Right(r) => {
                                            line = r.end - 1;
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                    DiffLines::Skip(_, r) => {
                        current_visual_line += 1;
                        if current_visual_line > visual_line {
                            line = r.end;
                            break;
                        }
                    }
                    DiffLines::Both(_, r) | DiffLines::Right(r) => {
                        current_visual_line += r.len();
                        if current_visual_line > visual_line {
                            line = r.end - (current_visual_line - visual_line);
                            break;
                        }
                    }
                }
            }
        }
        if current_visual_line <= visual_line {
            self.last_line()
        } else {
            line
        }
    }

    fn diff_cursor_line(&self, compare: &str, line: usize) -> usize {
        let mut cursor_line = 0;
        if let Some(changes) = self.history_changes.get(compare) {
            for (_i, change) in changes.iter().enumerate() {
                match change {
                    DiffLines::Left(_range) => {}
                    DiffLines::Both(_, r) | DiffLines::Right(r) => {
                        if r.contains(&line) {
                            cursor_line += line - r.start;
                            break;
                        }
                        cursor_line += r.len();
                    }
                    DiffLines::Skip(_, r) => {
                        if r.contains(&line) {
                            break;
                        }
                    }
                }
            }
        }
        cursor_line
    }

    fn diff_actual_line(&self, compare: &str, cursor_line: usize) -> usize {
        let mut current_cursor_line = 0;
        let mut line = 0;
        if let Some(changes) = self.history_changes.get(compare) {
            for (_i, change) in changes.iter().enumerate() {
                match change {
                    DiffLines::Left(_range) => {}
                    DiffLines::Skip(_, _r) => {}
                    DiffLines::Both(_, r) | DiffLines::Right(r) => {
                        current_cursor_line += r.len();
                        if current_cursor_line > cursor_line {
                            line = r.end - (current_cursor_line - cursor_line);
                            break;
                        }
                    }
                }
            }
        }
        if current_cursor_line <= cursor_line {
            self.last_line()
        } else {
            line
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn move_offset(
        &self,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
        code_lens: bool,
        compare: Option<&str>,
        config: &Config,
    ) -> (usize, ColPosition) {
        let horiz = if let Some(horiz) = horiz {
            *horiz
        } else {
            let (_, col) = self.offset_to_line_col(offset, config.editor.tab_width);
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
                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
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

                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
                (new_offset, ColPosition::Col(col))
            }
            Movement::Up => {
                let line = self.line_of_offset(offset);
                let line = if line == 0 {
                    0
                } else if let Some(compare) = compare {
                    let cursor_line = self.diff_cursor_line(compare, line);
                    let cursor_line = cursor_line.saturating_sub(count);
                    self.diff_actual_line(compare, cursor_line)
                } else if code_lens && count == 1 {
                    let empty_lens = Syntax::lens_from_normal_lines(
                        self.len(),
                        config.editor.line_height,
                        config.editor.code_lens_font_size,
                        &[],
                    );

                    let lens = self
                        .decoration
                        .syntax
                        .as_ref()
                        .map_or(&empty_lens, |syntax| &syntax.lens);

                    let mut line = line - 1;
                    while line != 0 {
                        let line_height = lens.height_of_line(line + 1)
                            - lens.height_of_line(line);
                        if line_height == config.editor.line_height {
                            break;
                        }
                        line -= 1;
                    }
                    line
                } else {
                    line.saturating_sub(count)
                };

                let col = self.line_horiz_col(
                    line,
                    &horiz,
                    mode != Mode::Normal,
                    config.editor.tab_width,
                );
                let new_offset =
                    self.offset_of_line_col(line, col, config.editor.tab_width);
                (new_offset, horiz)
            }
            Movement::Down => {
                let last_line = self.last_line();
                let line = self.line_of_offset(offset);

                let line = if let Some(compare) = compare {
                    let cursor_line = self.diff_cursor_line(compare, line);
                    let cursor_line = cursor_line + count;

                    self.diff_actual_line(compare, cursor_line)
                } else if code_lens && count == 1 {
                    let empty_lens = Syntax::lens_from_normal_lines(
                        self.len(),
                        config.editor.line_height,
                        config.editor.code_lens_font_size,
                        &[],
                    );
                    let lens = self
                        .decoration
                        .syntax
                        .as_ref()
                        .map_or(&empty_lens, |syntax| &syntax.lens);
                    let mut line = (line + 1).min(last_line);
                    while line != last_line {
                        let line_height = lens.height_of_line(line + 1)
                            - lens.height_of_line(line);
                        if line_height == config.editor.line_height {
                            break;
                        }
                        line += 1;
                    }
                    line
                } else {
                    (line + count).min(last_line)
                };

                let col = self.line_horiz_col(
                    line,
                    &horiz,
                    mode != Mode::Normal,
                    config.editor.tab_width,
                );
                let new_offset =
                    self.offset_of_line_col(line, col, config.editor.tab_width);
                (new_offset, horiz)
            }
            Movement::DocumentStart => (0, ColPosition::Start),
            Movement::DocumentEnd => {
                let last_offset =
                    self.offset_line_end(self.len(), mode != Mode::Normal);
                (last_offset, ColPosition::End)
            }
            Movement::FirstNonBlank => {
                let line = self.line_of_offset(offset);
                let new_offset = self.first_non_blank_character_on_line(line);
                (new_offset, ColPosition::FirstNonBlank)
            }
            Movement::StartOfLine => {
                let line = self.line_of_offset(offset);
                let new_offset = self.offset_of_line(line);
                (new_offset, ColPosition::Start)
            }
            Movement::EndOfLine => {
                let new_offset = self.offset_line_end(offset, mode != Mode::Normal);
                (new_offset, ColPosition::End)
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => (line - 1).min(self.last_line()),
                    LinePosition::First => 0,
                    LinePosition::Last => self.last_line(),
                };
                let col = self.line_horiz_col(
                    line,
                    &horiz,
                    mode != Mode::Normal,
                    config.editor.tab_width,
                );
                let new_offset =
                    self.offset_of_line_col(line, col, config.editor.tab_width);
                (new_offset, horiz)
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let new_offset =
                    self.data.rope.prev_grapheme_offset(new_offset + 1).unwrap();
                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordEndForward => {
                let mut new_offset = WordCursor::new(&self.data.rope, offset)
                    .end_boundary()
                    .unwrap_or(offset);
                if mode != Mode::Insert {
                    new_offset = self.prev_grapheme_offset(new_offset, 1, 0);
                }
                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordForward => {
                let new_offset = WordCursor::new(&self.data.rope, offset)
                    .next_boundary()
                    .unwrap_or(offset);
                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
                (new_offset, ColPosition::Col(col))
            }
            Movement::WordBackward => {
                let new_offset = WordCursor::new(&self.data.rope, offset)
                    .prev_boundary()
                    .unwrap_or(offset);
                let (_, col) =
                    self.offset_to_line_col(new_offset, config.editor.tab_width);
                (new_offset, ColPosition::Col(col))
            }
            Movement::NextUnmatched(c) => {
                if let Some(syntax) = self.syntax() {
                    let new_offset = syntax
                        .find_tag(offset, false, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = WordCursor::new(&self.data.rope, offset)
                        .next_unmatched(*c)
                        .map_or(offset, |new| new - 1);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                }
            }
            Movement::PreviousUnmatched(c) => {
                if let Some(syntax) = self.syntax() {
                    let new_offset = syntax
                        .find_tag(offset, true, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = WordCursor::new(&self.data.rope, offset)
                        .previous_unmatched(*c)
                        .unwrap_or(offset);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                }
            }
            Movement::MatchPairs => {
                if let Some(syntax) = self.syntax() {
                    let new_offset =
                        syntax.find_matching_pair(offset).unwrap_or(offset);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                } else {
                    let new_offset = WordCursor::new(&self.data.rope, offset)
                        .match_pairs()
                        .unwrap_or(offset);
                    let (_, col) =
                        self.offset_to_line_col(new_offset, config.editor.tab_width);
                    (new_offset, ColPosition::Col(col))
                }
            }
        }
    }

    pub fn previous_unmatched(&self, c: char, offset: usize) -> Option<usize> {
        if let Some(syntax) = self.syntax() {
            syntax.find_tag(offset, true, &c.to_string())
        } else {
            WordCursor::new(&self.data.rope, offset).previous_unmatched(c)
        }
    }

    pub fn prev_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.data.rope, offset).prev_code_boundary()
    }

    pub fn next_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.data.rope, offset).next_code_boundary()
    }

    pub fn update_history_changes(
        &mut self,
        rev: u64,
        history: &str,
        changes: Arc<Vec<DiffLines>>,
    ) {
        if rev != self.rev() {
            return;
        }
        self.history_changes.insert(history.to_string(), changes);
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
        candidate
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
        candidate
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
        (Cr, Lf) => Interior,
        (Space, Lf) => Interior,
        (Space, Cr) => Interior,
        (Space, Space) => Interior,
        (_, Space) => End,
        (Space, _) => Start,
        (Lf, _) => Start,
        (_, Cr) => End,
        (_, Lf) => End,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

fn classify_boundary_initial(
    prev: WordProperty,
    next: WordProperty,
) -> WordBoundary {
    #[allow(clippy::match_single_binding)]
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
    Cr,
    Lf,
    Space,
    Punctuation,
    Other, // includes letters and all of non-ascii unicode
}

pub fn get_word_property(codepoint: char) -> WordProperty {
    if codepoint <= ' ' {
        if codepoint == '\r' {
            return WordProperty::Cr;
        }
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
            pair_first.entry(key).or_insert(left);
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
            pair_first.entry(key).or_insert(left);
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
            count.entry(key).or_insert(0i32);
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

// fn get_git_diff(
//     workspace_path: &PathBuf,
//     path: &PathBuf,
//     content: &str,
// ) -> Option<(Vec<DiffHunk>, HashMap<usize, char>)> {
//     let repo = Repository::open(workspace_path.to_str()?).ok()?;
//     let head = repo.head().ok()?;
//     let tree = head.peel_to_tree().ok()?;
//     let tree_entry = tree
//         .get_path(path.strip_prefix(workspace_path).ok()?)
//         .ok()?;
//     let blob = repo.find_blob(tree_entry.id()).ok()?;
//     let mut patch = git2::Patch::from_blob_and_buffer(
//         &blob,
//         None,
//         content.as_bytes(),
//         None,
//         None,
//     )
//     .ok()?;
//     let mut line_changes = HashMap::new();
//     Some((
//         (0..patch.num_hunks())
//             .into_iter()
//             .filter_map(|i| {
//                 let hunk = patch.hunk(i).ok()?;
//                 let hunk = DiffHunk {
//                     old_start: hunk.0.old_start(),
//                     old_lines: hunk.0.old_lines(),
//                     new_start: hunk.0.new_start(),
//                     new_lines: hunk.0.new_lines(),
//                     header: String::from_utf8(hunk.0.header().to_vec()).ok()?,
//                 };
//                 let mut line_diff = 0;
//                 for line in 0..hunk.old_lines + hunk.new_lines {
//                     if let Ok(diff_line) = patch.line_in_hunk(i, line as usize) {
//                         match diff_line.origin() {
//                             ' ' => {
//                                 let new_line = diff_line.new_lineno().unwrap();
//                                 let old_line = diff_line.old_lineno().unwrap();
//                                 line_diff = new_line as i32 - old_line as i32;
//                             }
//                             '-' => {
//                                 let old_line = diff_line.old_lineno().unwrap() - 1;
//                                 let new_line =
//                                     (old_line as i32 + line_diff) as usize;
//                                 line_changes.insert(new_line, '-');
//                                 line_diff -= 1;
//                             }
//                             '+' => {
//                                 let new_line =
//                                     diff_line.new_lineno().unwrap() as usize - 1;
//                                 if let Some(c) = line_changes.get(&new_line) {
//                                     if c == &'-' {
//                                         line_changes.insert(new_line, 'm');
//                                     }
//                                 } else {
//                                     line_changes.insert(new_line, '+');
//                                 }
//                                 line_diff += 1;
//                             }
//                             _ => continue,
//                         }
//                         diff_line.origin();
//                     }
//                 }
//                 Some(hunk)
//             })
//             .collect(),
//         line_changes,
//     ))
// }

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

#[allow(dead_code)]
fn language_id_from_path(path: &str) -> Option<&str> {
    let path_buf = PathBuf::from_str(path).ok()?;
    Some(match path_buf.extension()?.to_str()? {
        "rs" => "rust",
        "go" => "go",
        _ => return None,
    })
}

#[allow(dead_code)]
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

pub fn char_width(c: char) -> usize {
    if c == '\t' {
        return 8;
    }
    if c.is_emoji_modifier_base() || c.is_emoji_modifier() {
        // treat modifier sequences as double wide
        return 2;
    }
    c.width().unwrap_or(0)
}

pub fn str_col(s: &str, tab_width: usize) -> usize {
    let mut total_width = 0;

    for c in s.chars() {
        let width = if c == '\t' {
            tab_width - total_width % tab_width
        } else {
            char_width(c)
        };

        total_width += width;
    }

    total_width
}

#[allow(dead_code)]
fn buffer_diff(
    left_rope: Rope,
    right_rope: Rope,
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
) -> Option<Vec<DiffLines>> {
    let mut changes = Vec::new();
    let left_str = &left_rope.slice_to_cow(0..left_rope.len());
    let right_str = &right_rope.slice_to_cow(0..right_rope.len());
    let mut left_line = 0;
    let mut right_line = 0;
    for diff in diff::lines(left_str, right_str) {
        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
            return None;
        }
        match diff {
            diff::Result::Left(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Left(r)) => r.end = left_line + 1,
                    _ => changes.push(DiffLines::Left(left_line..left_line + 1)),
                }
                left_line += 1;
            }
            diff::Result::Both(_, _) => {
                match changes.last_mut() {
                    Some(DiffLines::Both(l, r)) => {
                        l.end = left_line + 1;
                        r.end = right_line + 1;
                    }
                    _ => changes.push(DiffLines::Both(
                        left_line..left_line + 1,
                        right_line..right_line + 1,
                    )),
                }
                left_line += 1;
                right_line += 1;
            }
            diff::Result::Right(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Right(r)) => r.end = right_line + 1,
                    _ => changes.push(DiffLines::Right(right_line..right_line + 1)),
                }
                right_line += 1;
            }
        }
    }
    for (i, change) in changes.clone().iter().enumerate().rev() {
        if let DiffLines::Both(l, r) = change {
            if r.len() > 6 {
                changes[i] = DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                changes.insert(
                    i,
                    DiffLines::Skip(l.start + 3..l.end - 3, r.start + 3..r.end - 3),
                );
                changes.insert(
                    i,
                    DiffLines::Both(l.start..l.start + 3, r.start..r.start + 3),
                );
            }
        }
    }
    Some(changes)
}

fn rope_diff(
    left_rope: Rope,
    right_rope: Rope,
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
) -> Option<Vec<DiffLines>> {
    let left_lines = left_rope.lines(..).collect::<Vec<Cow<str>>>();
    let right_lines = right_rope.lines(..).collect::<Vec<Cow<str>>>();

    let left_count = left_lines.len();
    let right_count = right_lines.len();
    let min_count = cmp::min(left_count, right_count);

    let leading_equals = left_lines
        .iter()
        .zip(right_lines.iter())
        .take_while(|p| p.0 == p.1)
        .count();
    let trailing_equals = left_lines
        .iter()
        .rev()
        .zip(right_lines.iter().rev())
        .take(min_count - leading_equals)
        .take_while(|p| p.0 == p.1)
        .count();

    let left_diff_size = left_count - leading_equals - trailing_equals;
    let right_diff_size = right_count - leading_equals - trailing_equals;

    let table: Vec<Vec<u32>> = {
        let mut table = vec![vec![0; right_diff_size + 1]; left_diff_size + 1];
        let left_skip = left_lines.iter().skip(leading_equals).take(left_diff_size);
        let right_skip = right_lines
            .iter()
            .skip(leading_equals)
            .take(right_diff_size);

        for (i, l) in left_skip.enumerate() {
            for (j, r) in right_skip.clone().enumerate() {
                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    return None;
                }
                table[i + 1][j + 1] = if l == r {
                    table[i][j] + 1
                } else {
                    std::cmp::max(table[i][j + 1], table[i + 1][j])
                };
            }
        }

        table
    };

    let diff = {
        let mut diff = Vec::with_capacity(left_diff_size + right_diff_size);
        let mut i = left_diff_size;
        let mut j = right_diff_size;
        let mut li = left_lines.iter().rev().skip(trailing_equals);
        let mut ri = right_lines.iter().skip(trailing_equals);

        loop {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if j > 0 && (i == 0 || table[i][j] == table[i][j - 1]) {
                j -= 1;
                diff.push(DiffResult::Right(ri.next().unwrap()));
            } else if i > 0 && (j == 0 || table[i][j] == table[i - 1][j]) {
                i -= 1;
                diff.push(DiffResult::Left(li.next().unwrap()));
            } else if i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                diff.push(DiffResult::Both(li.next().unwrap(), ri.next().unwrap()));
            } else {
                break;
            }
        }

        diff
    };

    let mut changes = Vec::new();
    let mut left_line = 0;
    let mut right_line = 0;
    if leading_equals > 0 {
        changes.push(DiffLines::Both(0..leading_equals, 0..leading_equals));
    }
    left_line += leading_equals;
    right_line += leading_equals;

    for diff in diff.iter().rev() {
        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
            return None;
        }
        match diff {
            DiffResult::Left(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Left(r)) => r.end = left_line + 1,
                    _ => changes.push(DiffLines::Left(left_line..left_line + 1)),
                }
                left_line += 1;
            }
            DiffResult::Both(_, _) => {
                match changes.last_mut() {
                    Some(DiffLines::Both(l, r)) => {
                        l.end = left_line + 1;
                        r.end = right_line + 1;
                    }
                    _ => changes.push(DiffLines::Both(
                        left_line..left_line + 1,
                        right_line..right_line + 1,
                    )),
                }
                left_line += 1;
                right_line += 1;
            }
            DiffResult::Right(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Right(r)) => r.end = right_line + 1,
                    _ => changes.push(DiffLines::Right(right_line..right_line + 1)),
                }
                right_line += 1;
            }
        }
    }

    if trailing_equals > 0 {
        changes.push(DiffLines::Both(
            left_count - trailing_equals..left_count,
            right_count - trailing_equals..right_count,
        ));
    }
    if !changes.is_empty() {
        let changes_last = changes.len() - 1;
        for (i, change) in changes.clone().iter().enumerate().rev() {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if let DiffLines::Both(l, r) = change {
                if i == 0 || i == changes_last {
                    if r.len() > 3 {
                        if i == 0 {
                            changes[i] =
                                DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                            changes.insert(
                                i,
                                DiffLines::Skip(
                                    l.start..l.end - 3,
                                    r.start..r.end - 3,
                                ),
                            );
                        } else {
                            changes[i] = DiffLines::Skip(
                                l.start + 3..l.end,
                                r.start + 3..r.end,
                            );
                            changes.insert(
                                i,
                                DiffLines::Both(
                                    l.start..l.start + 3,
                                    r.start..r.start + 3,
                                ),
                            );
                        }
                    }
                } else if r.len() > 6 {
                    changes[i] = DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                    changes.insert(
                        i,
                        DiffLines::Skip(
                            l.start + 3..l.end - 3,
                            r.start + 3..r.end - 3,
                        ),
                    );
                    changes.insert(
                        i,
                        DiffLines::Both(l.start..l.start + 3, r.start..r.start + 3),
                    );
                }
            }
        }
    }
    Some(changes)
}

#[allow(dead_code)]
fn iter_diff<I, T>(left: I, right: I) -> Vec<DiffResult<T>>
where
    I: Clone + Iterator<Item = T> + DoubleEndedIterator,
    T: PartialEq,
{
    let left_count = left.clone().count();
    let right_count = right.clone().count();
    let min_count = cmp::min(left_count, right_count);

    let leading_equals = left
        .clone()
        .zip(right.clone())
        .take_while(|p| p.0 == p.1)
        .count();
    let trailing_equals = left
        .clone()
        .rev()
        .zip(right.clone().rev())
        .take(min_count - leading_equals)
        .take_while(|p| p.0 == p.1)
        .count();

    let left_diff_size = left_count - leading_equals - trailing_equals;
    let right_diff_size = right_count - leading_equals - trailing_equals;

    let table: Vec<Vec<u32>> = {
        let mut table = vec![vec![0; right_diff_size + 1]; left_diff_size + 1];
        let left_skip = left.clone().skip(leading_equals).take(left_diff_size);
        let right_skip = right.clone().skip(leading_equals).take(right_diff_size);

        for (i, l) in left_skip.enumerate() {
            for (j, r) in right_skip.clone().enumerate() {
                table[i + 1][j + 1] = if l == r {
                    table[i][j] + 1
                } else {
                    std::cmp::max(table[i][j + 1], table[i + 1][j])
                };
            }
        }

        table
    };

    let diff = {
        let mut diff = Vec::with_capacity(left_diff_size + right_diff_size);
        let mut i = left_diff_size;
        let mut j = right_diff_size;
        let mut li = left.clone().rev().skip(trailing_equals);
        let mut ri = right.clone().rev().skip(trailing_equals);

        loop {
            if j > 0 && (i == 0 || table[i][j] == table[i][j - 1]) {
                j -= 1;
                diff.push(DiffResult::Right(ri.next().unwrap()));
            } else if i > 0 && (j == 0 || table[i][j] == table[i - 1][j]) {
                i -= 1;
                diff.push(DiffResult::Left(li.next().unwrap()));
            } else if i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                diff.push(DiffResult::Both(li.next().unwrap(), ri.next().unwrap()));
            } else {
                break;
            }
        }

        diff
    };

    let diff_size = leading_equals + diff.len() + trailing_equals;
    let mut total_diff = Vec::with_capacity(diff_size);

    total_diff.extend(
        left.clone()
            .zip(right.clone())
            .take(leading_equals)
            .map(|(l, r)| DiffResult::Both(l, r)),
    );
    total_diff.extend(diff.into_iter().rev());
    total_diff.extend(
        left.skip(leading_equals + left_diff_size)
            .zip(right.skip(leading_equals + right_diff_size))
            .map(|(l, r)| DiffResult::Both(l, r)),
    );

    total_diff
}
// pub fn grapheme_column_width(s: &str) -> usize {
//     // Due to this issue:
//     // https://github.com/unicode-rs/unicode-width/issues/4
//     // we cannot simply use the unicode-width crate to compute
//     // the desired value.
//     // Let's check for emoji-ness for ourselves first
//     use xi_unicode::EmojiExt;
//     for c in s.chars() {
//         if c == '\t' {
//             return 8;
//         }
//         if c.is_emoji_modifier_base() || c.is_emoji_modifier() {
//             // treat modifier sequences as double wide
//             return 2;
//         }
//     }
//     UnicodeWidthStr::width(s)
// }
