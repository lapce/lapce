use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{self},
        Arc,
    },
};

use druid::{
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    ExtEventSink, Point, SingleUse, Size, Target, Vec2, WidgetId,
};
use lapce_core::{
    buffer::{Buffer, DiffLines, InvalLines},
    command::{EditCommand, MultiSelectionCommand},
    cursor::{ColPosition, Cursor, CursorMode},
    editor::{EditType, Editor},
    language::LapceLanguage,
    mode::{Mode, MotionMode},
    movement::{LinePosition, Movement},
    register::{Clipboard, Register, RegisterData},
    selection::{SelRegion, Selection},
    style::line_styles,
    syntax::Syntax,
    word::WordCursor,
};
use lapce_rpc::{
    buffer::{BufferId, NewBufferResponse},
    style::{LineStyle, LineStyles, Style},
};
use lsp_types::{CodeActionOrCommand, CodeActionResponse};
use serde::{Deserialize, Serialize};
use xi_rope::{spans::Spans, Rope, RopeDelta};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    editor::EditorLocation,
    find::{Find, FindProgress},
    history::DocumentHistory,
    proxy::LapceProxy,
    settings::SettingsValueKind,
};

pub struct SystemClipboard {}

impl Clipboard for SystemClipboard {
    fn get_string(&self) -> Option<String> {
        druid::Application::global().clipboard().get_string()
    }

    fn put_string(&mut self, s: impl AsRef<str>) {
        druid::Application::global().clipboard().put_string(s)
    }
}

#[derive(Clone, Default)]
pub struct TextLayoutCache {
    config_id: u64,
    pub layouts: HashMap<usize, Arc<PietTextLayout>>,
}

impl TextLayoutCache {
    pub fn new() -> Self {
        Self {
            config_id: 0,
            layouts: HashMap::new(),
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

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum LocalBufferKind {
    Empty,
    Palette,
    Search,
    SourceControl,
    FilePicker,
    Keymap,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BufferContent {
    File(PathBuf),
    Local(LocalBufferKind),
    SettingsValue(String, SettingsValueKind, String, String),
    Scratch(BufferId, String),
}

impl BufferContent {
    pub fn is_file(&self) -> bool {
        matches!(self, BufferContent::File(_))
    }

    pub fn is_special(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::Palette
                | LocalBufferKind::SourceControl
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty => false,
            },
            BufferContent::SettingsValue(..) => true,
            BufferContent::Scratch(..) => false,
        }
    }

    pub fn is_input(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::Palette
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty | LocalBufferKind::SourceControl => false,
            },
            BufferContent::SettingsValue(..) => true,
            BufferContent::Scratch(..) => false,
        }
    }

    pub fn is_search(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::SettingsValue(..) => false,
            BufferContent::Scratch(..) => false,
            BufferContent::Local(local) => matches!(local, LocalBufferKind::Search),
        }
    }

    pub fn is_settings(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::SettingsValue(..) => true,
            BufferContent::Local(_) => false,
            BufferContent::Scratch(..) => false,
        }
    }

    pub fn file_name(&self) -> &str {
        match self {
            BufferContent::File(p) => {
                p.file_name().and_then(|f| f.to_str()).unwrap_or("")
            }
            BufferContent::Scratch(_, scratch_doc_name) => scratch_doc_name,
            _ => "",
        }
    }
}

#[derive(Clone)]
pub struct Document {
    id: BufferId,
    pub tab_id: WidgetId,
    buffer: Buffer,
    content: BufferContent,
    syntax: Option<Syntax>,
    line_styles: Rc<RefCell<LineStyles>>,
    semantic_styles: Option<Arc<Spans<Style>>>,
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    load_started: Rc<RefCell<bool>>,
    loaded: bool,
    histories: im::HashMap<String, DocumentHistory>,
    pub cursor_offset: usize,
    pub scroll_offset: Vec2,
    pub code_actions: im::HashMap<usize, CodeActionResponse>,
    pub find: Rc<RefCell<Find>>,
    find_progress: Rc<RefCell<FindProgress>>,
    pub event_sink: ExtEventSink,
    pub proxy: Arc<LapceProxy>,
}

impl Document {
    pub fn new(
        content: BufferContent,
        tab_id: WidgetId,
        event_sink: ExtEventSink,
        proxy: Arc<LapceProxy>,
    ) -> Self {
        let syntax = match &content {
            BufferContent::File(path) => Syntax::init(path),
            BufferContent::Local(_) => None,
            BufferContent::SettingsValue(..) => None,
            BufferContent::Scratch(..) => None,
        };
        let id = match &content {
            BufferContent::Scratch(id, _) => *id,
            _ => BufferId::next(),
        };

        Self {
            id,
            tab_id,
            buffer: Buffer::new(""),
            content,
            syntax,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            semantic_styles: None,
            load_started: Rc::new(RefCell::new(false)),
            histories: im::HashMap::new(),
            loaded: false,
            cursor_offset: 0,
            scroll_offset: Vec2::ZERO,
            code_actions: im::HashMap::new(),
            find: Rc::new(RefCell::new(Find::new(0))),
            find_progress: Rc::new(RefCell::new(FindProgress::Ready)),
            event_sink,
            proxy,
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn set_content(&mut self, content: BufferContent) {
        self.content = content;
        self.syntax = match &self.content {
            BufferContent::File(path) => Syntax::init(path),
            BufferContent::Local(_) => None,
            BufferContent::SettingsValue(..) => None,
            BufferContent::Scratch(..) => None,
        };
        self.on_update(None);
    }

    pub fn content(&self) -> &BufferContent {
        &self.content
    }

    pub fn rev(&self) -> u64 {
        self.buffer.rev()
    }

    pub fn init_content(&mut self, content: Rope) {
        self.buffer.init_content(content);
        self.buffer.detect_indent(self.syntax.as_ref());
        self.loaded = true;
        self.on_update(None);
    }

    pub fn set_language(&mut self, language: LapceLanguage) {
        self.syntax = Some(Syntax::from_language(language));
    }

    pub fn reload(&mut self, content: Rope, set_pristine: bool) {
        self.code_actions.clear();
        let delta = self.buffer.reload(content, set_pristine);
        self.apply_deltas(&[delta]);
    }

    pub fn handle_file_changed(&mut self, content: Rope) {
        if self.buffer.is_pristine() {
            self.reload(content, true);
        }
    }

    pub fn retrieve_file(&mut self, locations: Vec<(WidgetId, EditorLocation)>) {
        if self.loaded || *self.load_started.borrow() {
            return;
        }

        *self.load_started.borrow_mut() = true;
        if let BufferContent::File(path) = &self.content {
            let id = self.id;
            let tab_id = self.tab_id;
            let path = path.clone();
            let event_sink = self.event_sink.clone();
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
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
                                    LapceUICommand::InitBufferContent {
                                        path,
                                        content: Rope::from(resp.content),
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

        self.retrieve_history("head");
    }

    pub fn retrieve_history(&mut self, version: &str) {
        if self.histories.contains_key(version) {
            return;
        }

        let history = DocumentHistory::new(version.to_string());
        history.retrieve(self);
        self.histories.insert(version.to_string(), history);
    }

    pub fn reload_history(&self, version: &str) {
        if let Some(history) = self.histories.get(version) {
            history.retrieve(self);
        }
    }

    pub fn load_history(&mut self, version: &str, content: Rope) {
        let mut history = DocumentHistory::new(version.to_string());
        history.load_content(content, self);
        self.histories.insert(version.to_string(), history);
    }

    pub fn get_history(&self, version: &str) -> Option<&DocumentHistory> {
        self.histories.get(version)
    }

    pub fn history_visual_line(&self, version: &str, line: usize) -> usize {
        let mut visual_line = 0;
        if let Some(history) = self.histories.get(version) {
            for (_i, change) in history.changes().iter().enumerate() {
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

    pub fn history_actual_line_from_visual(
        &self,
        version: &str,
        visual_line: usize,
    ) -> usize {
        let mut current_visual_line = 0;
        let mut line = 0;
        if let Some(history) = self.histories.get(version) {
            for (i, change) in history.changes().iter().enumerate() {
                match change {
                    DiffLines::Left(range) => {
                        current_visual_line += range.len();
                        if current_visual_line > visual_line {
                            if let Some(change) = history.changes().get(i + 1) {
                                match change {
                                    DiffLines::Left(_) => {}
                                    DiffLines::Both(_, r)
                                    | DiffLines::Skip(_, r)
                                    | DiffLines::Right(r) => {
                                        line = r.start;
                                    }
                                }
                            } else if i > 0 {
                                if let Some(change) = history.changes().get(i - 1) {
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
            self.buffer.last_line()
        } else {
            line
        }
    }

    fn trigger_head_change(&self) {
        if let Some(head) = self.histories.get("head") {
            head.trigger_update_change(self);
        }
    }

    pub fn update_history_changes(
        &mut self,
        rev: u64,
        version: &str,
        changes: Arc<Vec<DiffLines>>,
    ) {
        if rev != self.rev() {
            return;
        }
        if let Some(history) = self.histories.get_mut(version) {
            history.update_changes(changes);
        }
    }

    pub fn update_history_styles(
        &mut self,
        version: &str,
        styles: Arc<Spans<Style>>,
    ) {
        if let Some(history) = self.histories.get_mut(version) {
            history.update_styles(styles);
        }
    }

    fn on_update(&mut self, delta: Option<&RopeDelta>) {
        self.find.borrow_mut().unset();
        *self.find_progress.borrow_mut() = FindProgress::Started;
        self.clear_style_cache();
        self.trigger_syntax_change(delta);
        self.trigger_head_change();
        self.notify_special();
    }

    fn notify_special(&self) {
        match &self.content {
            BufferContent::File(_) => {}
            BufferContent::Scratch(..) => {}
            BufferContent::Local(local) => {
                let s = self.buffer.text().to_string();
                match local {
                    LocalBufferKind::Search => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSearch(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::SourceControl => {}
                    LocalBufferKind::Empty => {}
                    LocalBufferKind::Palette => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePaletteInput(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::FilePicker => {
                        let pwd = PathBuf::from(s);
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePickerPwd(pwd),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::Keymap => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateKeymapsFilter(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::Settings => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSettingsFilter(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                }
            }
            BufferContent::SettingsValue(..) => {}
        }
    }

    pub fn set_syntax(&mut self, syntax: Option<Syntax>) {
        self.syntax = syntax;
        if self.semantic_styles.is_none() {
            self.clear_style_cache();
        }
    }

    pub fn set_semantic_styles(&mut self, styles: Option<Arc<Spans<Style>>>) {
        self.semantic_styles = styles;
        self.clear_style_cache();
    }

    fn clear_style_cache(&self) {
        self.line_styles.borrow_mut().clear();
        self.clear_text_layout_cache();
    }

    fn clear_text_layout_cache(&self) {
        self.text_layouts.borrow_mut().clear();
    }

    fn trigger_syntax_change(&self, delta: Option<&RopeDelta>) {
        if let Some(syntax) = self.syntax.clone() {
            let content = self.content.clone();
            let rev = self.buffer.rev();
            let text = self.buffer.text().clone();
            let delta = delta.cloned();
            let atomic_rev = self.buffer.atomic_rev();
            let event_sink = self.event_sink.clone();
            let tab_id = self.tab_id;
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
                        content,
                        rev,
                        syntax: SingleUse::new(new_syntax),
                    },
                    Target::Widget(tab_id),
                );
            });
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn syntax(&self) -> Option<&Syntax> {
        self.syntax.as_ref()
    }

    fn update_styles(&mut self, delta: &RopeDelta) {
        if let Some(styles) = self.semantic_styles.as_mut() {
            Arc::make_mut(styles).apply_shape(delta);
        } else if let Some(syntax) = self.syntax.as_mut() {
            if let Some(styles) = syntax.styles.as_mut() {
                Arc::make_mut(styles).apply_shape(delta);
            }
        }

        if let Some(syntax) = self.syntax.as_mut() {
            syntax.lens.apply_delta(delta);
        }
    }

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines)]) {
        let rev = self.rev() - deltas.len() as u64;
        for (i, (delta, _)) in deltas.iter().enumerate() {
            self.update_styles(delta);
            if self.content.is_file() {
                self.proxy.update(self.id, delta, rev + i as u64 + 1);
            }
        }

        let delta = if deltas.len() == 1 {
            Some(&deltas[0].0)
        } else {
            None
        };
        self.on_update(delta);
    }

    pub fn do_insert(
        &mut self,
        cursor: &mut Cursor,
        s: &str,
    ) -> Vec<(RopeDelta, InvalLines)> {
        let old_cursor = cursor.mode.clone();
        let deltas =
            Editor::insert(cursor, &mut self.buffer, s, self.syntax.as_ref());
        self.buffer_mut().set_cursor_before(old_cursor);
        self.buffer_mut().set_cursor_after(cursor.mode.clone());
        self.apply_deltas(&deltas);
        deltas
    }

    pub fn do_raw_edit(
        &mut self,
        edits: &[(impl AsRef<Selection>, &str)],
        edit_type: EditType,
    ) -> (RopeDelta, InvalLines) {
        let (delta, inval_lines) = self.buffer.edit(edits, edit_type);
        self.apply_deltas(&[(delta.clone(), inval_lines.clone())]);
        (delta, inval_lines)
    }

    pub fn do_edit(
        &mut self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
    ) -> Vec<(RopeDelta, InvalLines)> {
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
        self.buffer_mut().set_cursor_before(old_cursor);
        self.buffer_mut().set_cursor_after(cursor.mode.clone());
        self.apply_deltas(&deltas);
        deltas
    }

    pub fn do_multi_selection(
        &self,
        text: &mut PietText,
        cursor: &mut Cursor,
        cmd: &MultiSelectionCommand,
        config: &Config,
    ) {
        use MultiSelectionCommand::*;
        match cmd {
            SelectUndo => {
                if let CursorMode::Insert(_) = cursor.mode.clone() {
                    if let Some(selection) =
                        cursor.history_selections.last().cloned()
                    {
                        cursor.mode = CursorMode::Insert(selection);
                    }
                    cursor.history_selections.pop();
                }
            }
            InsertCursorAbove => {
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    let offset = selection.first().map(|s| s.end()).unwrap_or(0);
                    let (new_offset, _) = self.move_offset(
                        text,
                        offset,
                        cursor.horiz.as_ref(),
                        1,
                        &Movement::Up,
                        Mode::Insert,
                        config.editor.font_size,
                        config,
                    );
                    if new_offset != offset {
                        selection.add_region(SelRegion::new(
                            new_offset, new_offset, None,
                        ));
                    }
                    cursor.set_insert(selection);
                }
            }
            InsertCursorBelow => {
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    let offset = selection.last().map(|s| s.end()).unwrap_or(0);
                    let (new_offset, _) = self.move_offset(
                        text,
                        offset,
                        cursor.horiz.as_ref(),
                        1,
                        &Movement::Down,
                        Mode::Insert,
                        config.editor.font_size,
                        config,
                    );
                    if new_offset != offset {
                        selection.add_region(SelRegion::new(
                            new_offset, new_offset, None,
                        ));
                    }
                    cursor.set_insert(selection);
                }
            }
            InsertCursorEndOfLine => {
                if let CursorMode::Insert(selection) = cursor.mode.clone() {
                    let mut new_selection = Selection::new();
                    for region in selection.regions() {
                        let (start_line, _) =
                            self.buffer.offset_to_line_col(region.min());
                        let (end_line, end_col) =
                            self.buffer.offset_to_line_col(region.max());
                        for line in start_line..end_line + 1 {
                            let offset = if line == end_line {
                                self.buffer.offset_of_line_col(line, end_col)
                            } else {
                                self.buffer.line_end_offset(line, true)
                            };
                            new_selection
                                .add_region(SelRegion::new(offset, offset, None));
                        }
                    }
                    cursor.set_insert(new_selection);
                }
            }
            SelectCurrentLine => {
                if let CursorMode::Insert(selection) = cursor.mode.clone() {
                    let mut new_selection = Selection::new();
                    for region in selection.regions() {
                        let start_line = self.buffer.line_of_offset(region.min());
                        let start = self.buffer.offset_of_line(start_line);
                        let end_line = self.buffer.line_of_offset(region.max());
                        let end = self.buffer.offset_of_line(end_line + 1);
                        new_selection.add_region(SelRegion::new(start, end, None));
                    }
                    cursor.set_insert(selection);
                }
            }
            SelectAllCurrent => {
                if let CursorMode::Insert(selection) = cursor.mode.clone() {
                    let mut new_selection = Selection::new();
                    if !selection.is_empty() {
                        let first = selection.first().unwrap();
                        let (start, end) = if first.is_caret() {
                            self.buffer.select_word(first.start())
                        } else {
                            (first.min(), first.max())
                        };
                        let search_str = self.buffer.slice_to_cow(start..end);
                        let mut find = Find::new(0);
                        find.set_find(&search_str, false, false, false);
                        let mut offset = 0;
                        while let Some((start, end)) =
                            find.next(self.buffer.text(), offset, false, false)
                        {
                            offset = end;
                            new_selection
                                .add_region(SelRegion::new(start, end, None));
                        }
                    }
                    cursor.set_insert(selection);
                }
            }
            SelectNextCurrent => {
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    if !selection.is_empty() {
                        let mut had_caret = false;
                        for region in selection.regions_mut() {
                            if region.is_caret() {
                                had_caret = true;
                                let (start, end) =
                                    self.buffer.select_word(region.start());
                                region.start = start;
                                region.end = end;
                            }
                        }
                        if !had_caret {
                            let r = selection.last_inserted().unwrap();
                            let search_str =
                                self.buffer.slice_to_cow(r.min()..r.max());
                            let mut find = Find::new(0);
                            find.set_find(&search_str, false, false, false);
                            let mut offset = r.max();
                            let mut seen = HashSet::new();
                            while let Some((start, end)) =
                                find.next(self.buffer.text(), offset, false, true)
                            {
                                if !selection
                                    .regions()
                                    .iter()
                                    .any(|r| r.min() == start && r.max() == end)
                                {
                                    selection.add_region(SelRegion::new(
                                        start, end, None,
                                    ));
                                    break;
                                }
                                if seen.contains(&end) {
                                    break;
                                }
                                offset = end;
                                seen.insert(offset);
                            }
                        }
                    }
                    cursor.set_insert(selection);
                }
            }
            SelectSkipCurrent => {
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    if !selection.is_empty() {
                        let r = selection.last_inserted().unwrap();
                        if r.is_caret() {
                            let (start, end) = self.buffer.select_word(r.start());
                            selection.replace_last_inserted_region(SelRegion::new(
                                start, end, None,
                            ));
                        } else {
                            let search_str =
                                self.buffer.slice_to_cow(r.min()..r.max());
                            let mut find = Find::new(0);
                            find.set_find(&search_str, false, false, false);
                            let mut offset = r.max();
                            let mut seen = HashSet::new();
                            while let Some((start, end)) =
                                find.next(self.buffer.text(), offset, false, true)
                            {
                                if !selection
                                    .regions()
                                    .iter()
                                    .any(|r| r.min() == start && r.max() == end)
                                {
                                    selection.replace_last_inserted_region(
                                        SelRegion::new(start, end, None),
                                    );
                                    break;
                                }
                                if seen.contains(&end) {
                                    break;
                                }
                                offset = end;
                                seen.insert(offset);
                            }
                        }
                    }
                    cursor.set_insert(selection);
                }
            }
            SelectAll => {
                let new_selection = Selection::region(0, self.buffer.len());
                cursor.set_insert(new_selection);
            }
        }
    }

    pub fn do_motion_mode(
        &mut self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        register: &mut Register,
    ) {
        if let Some(m) = &cursor.motion_mode {
            if m == &motion_mode {
                let offset = cursor.offset();
                let deltas = Editor::execute_motion_mode(
                    cursor,
                    &mut self.buffer,
                    motion_mode,
                    offset,
                    offset,
                    true,
                    register,
                );
                self.apply_deltas(&deltas);
            }
            cursor.motion_mode = None;
        } else {
            cursor.motion_mode = Some(motion_mode);
        }
    }

    pub fn do_paste(&mut self, cursor: &mut Cursor, data: &RegisterData) {
        let deltas = Editor::do_paste(cursor, &mut self.buffer, data);
        self.apply_deltas(&deltas)
    }

    pub fn styles(&self) -> Option<&Arc<Spans<Style>>> {
        let styles = self
            .semantic_styles
            .as_ref()
            .or_else(|| self.syntax().and_then(|s| s.styles.as_ref()));
        styles
    }

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

    pub fn point_of_line_col(
        &self,
        text: &mut PietText,
        line: usize,
        col: usize,
        font_size: usize,
        config: &Config,
    ) -> Point {
        let text_layout = self.get_text_layout(text, line, font_size, config);
        text_layout.hit_test_text_position(col).point
    }

    pub fn offset_of_point(
        &self,
        text: &mut PietText,
        mode: Mode,
        point: Point,
        font_size: usize,
        config: &Config,
    ) -> (usize, bool) {
        let last_line = self.buffer.last_line();
        let line = ((point.y / config.editor.line_height as f64).floor() as usize)
            .min(last_line);
        let text_layout = self.get_text_layout(text, line, font_size, config);
        let hit_point = text_layout.hit_test_point(Point::new(point.x, 0.0));
        let col = hit_point.idx;
        let max_col = self.buffer.line_end_col(line, mode != Mode::Normal);
        (
            self.buffer.offset_of_line_col(line, col.min(max_col)),
            hit_point.is_inside,
        )
    }

    pub fn point_of_offset(
        &self,
        text: &mut PietText,
        offset: usize,
        font_size: usize,
        config: &Config,
    ) -> Point {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let text_layout = self.get_text_layout(text, line, font_size, config);
        text_layout.hit_test_text_position(col).point
    }

    pub fn get_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        config: &Config,
    ) -> Arc<PietTextLayout> {
        self.text_layouts.borrow_mut().check_attributes(config.id);
        if self.text_layouts.borrow().layouts.get(&line).is_none() {
            self.text_layouts.borrow_mut().layouts.insert(
                line,
                Arc::new(self.new_text_layout(text, line, font_size, config)),
            );
        }
        self.text_layouts
            .borrow()
            .layouts
            .get(&line)
            .cloned()
            .unwrap()
    }

    fn new_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        config: &Config,
    ) -> PietTextLayout {
        let line_content = self.buffer.line_content(line);
        let tab_width =
            config.tab_width(text, config.editor.font_family(), font_size);

        let font_family = if self.content.is_input() {
            config.ui.font_family()
        } else {
            config.editor.font_family()
        };
        let font_size = if self.content.is_input() {
            config.ui.font_size()
        } else {
            font_size
        };
        let mut layout_builder = text
            .new_text_layout(line_content.to_string())
            .font(font_family, font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .set_tab_width(tab_width);

        let styles = self.line_style(line);
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

        layout_builder.build().unwrap()
    }

    pub fn line_horiz_col(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        horiz: &ColPosition,
        caret: bool,
        config: &Config,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let text_layout =
                    self.get_text_layout(text, line, font_size, config);
                let n = text_layout.hit_test_point(Point::new(x, 0.0)).idx;
                n.min(self.buffer.line_end_col(line, caret))
            }
            ColPosition::End => self.buffer.line_end_col(line, caret),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.buffer.first_non_blank_character_on_line(line)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn move_region(
        &self,
        text: &mut PietText,
        region: &SelRegion,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> SelRegion {
        let (end, horiz) = self.move_offset(
            text,
            region.end,
            region.horiz.as_ref(),
            count,
            movement,
            mode,
            font_size,
            config,
        );
        let start = match modify {
            true => region.start(),
            false => end,
        };
        SelRegion::new(start, end, horiz)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_cursor(
        &mut self,
        text: &mut PietText,
        cursor: &mut Cursor,
        movement: &Movement,
        count: usize,
        modify: bool,
        font_size: usize,
        register: &mut Register,
        config: &Config,
    ) {
        match cursor.mode {
            CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.move_offset(
                    text,
                    offset,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                    font_size,
                    config,
                );
                if let Some(motion_mode) = cursor.motion_mode.clone() {
                    let (moved_new_offset, _) = self.move_offset(
                        text,
                        new_offset,
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                        font_size,
                        config,
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
                    text,
                    end,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                    font_size,
                    config,
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
                    text,
                    selection,
                    cursor.horiz.as_ref(),
                    count,
                    modify,
                    movement,
                    Mode::Insert,
                    font_size,
                    config,
                );
                cursor.set_insert(selection);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_selection(
        &self,
        text: &mut PietText,
        selection: &Selection,
        _horiz: Option<&ColPosition>,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            new_selection.add_region(self.move_region(
                text, region, count, modify, movement, mode, font_size, config,
            ));
        }
        new_selection
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_offset(
        &self,
        text: &mut PietText,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> (usize, Option<ColPosition>) {
        match movement {
            Movement::Left => {
                let line = self.buffer.line_of_offset(offset);
                let line_start_offset = self.buffer.offset_of_line(line);

                let min_offset = if mode == Mode::Insert {
                    0
                } else {
                    line_start_offset
                };

                let new_offset =
                    self.buffer.prev_grapheme_offset(offset, count, min_offset);
                (new_offset, None)
            }
            Movement::Right => {
                let line_end =
                    self.buffer.offset_line_end(offset, mode != Mode::Normal);

                let max_offset = if mode == Mode::Insert {
                    self.buffer.len()
                } else {
                    line_end
                };

                let new_offset =
                    self.buffer.next_grapheme_offset(offset, count, max_offset);

                (new_offset, None)
            }
            Movement::Up => {
                let line = self.buffer.line_of_offset(offset);
                let line = if line == 0 {
                    0
                } else {
                    line.saturating_sub(count)
                };

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Down => {
                let last_line = self.buffer.last_line();
                let line = self.buffer.line_of_offset(offset);

                let line = (line + count).min(last_line);

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
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
                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
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
                let new_offset = self.buffer.move_n_words_backward(offset, count);
                (new_offset, None)
            }
            Movement::NextUnmatched(c) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, false, &c.to_string())
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .next_unmatched(*c)
                        .map_or(offset, |new| new - 1);
                    (new_offset, None)
                }
            }
            Movement::PreviousUnmatched(c) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, true, &c.to_string())
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .previous_unmatched(*c)
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
        }
    }

    pub fn code_action_size(
        &self,
        text: &mut PietText,
        offset: usize,
        config: &Config,
    ) -> Size {
        let prev_offset = self.buffer.prev_code_boundary(offset);
        let empty_vec = Vec::new();
        let code_actions = self.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

        let action_text_layouts: Vec<PietTextLayout> = code_actions
            .iter()
            .map(|code_action| {
                let title = match code_action {
                    CodeActionOrCommand::Command(cmd) => cmd.title.to_string(),
                    CodeActionOrCommand::CodeAction(action) => {
                        action.title.to_string()
                    }
                };

                text.new_text_layout(title)
                    .font(config.ui.font_family(), config.ui.font_size() as f64)
                    .build()
                    .unwrap()
            })
            .collect();

        let mut width = 0.0;
        for text_layout in &action_text_layouts {
            let line_width = text_layout.size().width + 10.0;
            if line_width > width {
                width = line_width;
            }
        }
        let line_height = config.editor.line_height as f64;
        Size::new(width, code_actions.len() as f64 * line_height)
    }

    pub fn reset_find(&self, current_find: &Find) {
        {
            let find = self.find.borrow();
            if find.search_string == current_find.search_string
                && find.case_matching == current_find.case_matching
                && find.regex.as_ref().map(|r| r.as_str())
                    == current_find.regex.as_ref().map(|r| r.as_str())
                && find.whole_words == current_find.whole_words
            {
                return;
            }
        }

        let mut find = self.find.borrow_mut();
        find.unset();
        find.search_string = current_find.search_string.clone();
        find.case_matching = current_find.case_matching;
        find.regex = current_find.regex.clone();
        find.whole_words = current_find.whole_words;
        *self.find_progress.borrow_mut() = FindProgress::Started;
    }

    pub fn update_find(
        &self,
        current_find: &Find,
        start_line: usize,
        end_line: usize,
    ) {
        self.reset_find(current_find);

        let mut find_progress = self.find_progress.borrow_mut();
        let search_range = match &find_progress.clone() {
            FindProgress::Started => {
                // start incremental find on visible region
                let start = self.buffer.offset_of_line(start_line);
                let end = self.buffer.offset_of_line(end_line + 1);
                *find_progress =
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
                    *find_progress = FindProgress::Ready;
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
                        *find_progress = FindProgress::InProgress(new_range);
                    }
                    range
                }
            }
            _ => None,
        };

        let mut find = self.find.borrow_mut();
        if let Some((search_range_start, search_range_end)) = search_range {
            if !find.is_multiline_regex() {
                find.update_find(
                    self.buffer.text(),
                    search_range_start,
                    search_range_end,
                    true,
                );
            } else {
                // only execute multi-line regex queries if we are searching the entire text (last step)
                if search_range_start == 0 && search_range_end == self.buffer.len() {
                    find.update_find(
                        self.buffer.text(),
                        search_range_start,
                        search_range_end,
                        true,
                    );
                }
            }
        }
    }
}
