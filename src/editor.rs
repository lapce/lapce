use crate::{
    buffer::{Buffer, BufferId, BufferUIState, InvalLines},
    command::LapceCommand,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    container::LapceContainer,
    movement::ColPosition,
    movement::LinePosition,
    movement::Movement,
    movement::SelRegion,
    movement::Selection,
    scroll::LapceScroll,
    split::SplitMoveDirection,
    state::LapceUIState,
    state::Mode,
    state::VisualMode,
    state::LAPCE_STATE,
    theme::LapceTheme,
};
use anyhow::{anyhow, Result};
use druid::{
    kurbo::Line, piet::PietText, widget::IdentityWrapper, widget::Padding,
    Affine, BoxConstraints, Data, Env, Event, EventCtx, KeyEvent, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size,
    TextLayout, UpdateCtx, Vec2, Widget, WidgetExt, WidgetId, WidgetPod,
};
use druid::{
    piet::{PietTextLayout, Text, TextAttribute, TextLayoutBuilder},
    FontWeight,
};
use std::collections::HashMap;
use std::iter::Iterator;
use xi_rope::Interval;

pub struct LapceUI {
    container: LapceContainer,
}

#[derive(Debug, Default)]
pub struct Counter(usize);

impl Counter {
    pub fn next(&mut self) -> usize {
        let n = self.0;
        self.0 = n + 1;
        n + 1
    }
}

#[derive(Copy, Clone)]
pub struct EditorCount(Option<usize>);

#[derive(Copy, Clone)]
pub enum EditorOperator {
    Delete(EditorCount),
    Yank(EditorCount),
}

pub struct EditorState {
    editor_id: WidgetId,
    view_id: WidgetId,
    pub split_id: WidgetId,
    pub buffer_id: Option<BufferId>,
    selection: Selection,
    pub line_height: f64,
    pub char_width: f64,
    pub width: f64,
    pub height: f64,
    pub scroll_offset: Vec2,
}

impl EditorState {
    pub fn new(
        id: WidgetId,
        view_id: WidgetId,
        split_id: WidgetId,
        buffer_id: Option<BufferId>,
    ) -> EditorState {
        EditorState {
            editor_id: id,
            view_id,
            split_id,
            buffer_id,
            selection: Selection::new_simple(),
            line_height: 0.0,
            char_width: 0.0,
            width: 0.0,
            height: 0.0,
            scroll_offset: Vec2::ZERO,
        }
    }

    fn get_count(
        &self,
        count: Option<usize>,
        operator: Option<EditorOperator>,
    ) -> Option<usize> {
        count.or(operator
            .map(|o| match o {
                EditorOperator::Delete(count) => count.0,
                EditorOperator::Yank(count) => count.0,
            })
            .flatten())
    }

    fn move_command(
        &self,
        count: Option<usize>,
        cmd: &LapceCommand,
    ) -> Option<Movement> {
        match cmd {
            LapceCommand::Left => Some(Movement::Left),
            LapceCommand::Right => Some(Movement::Right),
            LapceCommand::Up => Some(Movement::Up),
            LapceCommand::Down => Some(Movement::Down),
            LapceCommand::LineStart => Some(Movement::StartOfLine),
            LapceCommand::LineEnd => Some(Movement::EndOfLine),
            LapceCommand::GotoLineDefaultFirst => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            }),
            LapceCommand::GotoLineDefaultLast => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            }),
            LapceCommand::WordBackward => Some(Movement::WordBackward),
            LapceCommand::WordFoward => Some(Movement::WordForward),
            LapceCommand::WordEndForward => Some(Movement::WordEndForward),
            _ => None,
        }
    }

    pub fn run_command(
        &mut self,
        mode: Mode,
        count: Option<usize>,
        buffer: &mut Buffer,
        cmd: LapceCommand,
        operator: Option<EditorOperator>,
    ) {
        let count = self.get_count(count, operator);
        if let Some(movement) = self.move_command(count, &cmd) {
            self.selection = buffer.do_move(
                &mode,
                movement,
                &self.selection,
                operator,
                count,
            );
            if mode != Mode::Insert {
                self.selection = buffer.correct_offset(&self.selection);
            }
            self.ensure_cursor_visible(buffer);
            self.request_paint();
            return;
        }

        match cmd {
            LapceCommand::PageDown => {
                let lines =
                    (self.height / self.line_height / 2.0).floor() as usize;
                self.selection = Movement::Down.update_selection(
                    &self.selection,
                    buffer,
                    lines,
                    mode == Mode::Insert,
                    mode == Mode::Visual,
                );
                self.request_paint();
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::Scroll((
                        0.0,
                        self.line_height * lines as f64,
                    )),
                    self.view_id,
                );
            }
            LapceCommand::PageUp => {
                let lines =
                    (self.height / self.line_height / 2.0).floor() as usize;
                self.selection = Movement::Up.update_selection(
                    &self.selection,
                    buffer,
                    lines,
                    mode == Mode::Insert,
                    mode == Mode::Visual,
                );
                self.request_paint();
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::Scroll((
                        0.0,
                        -self.line_height * lines as f64,
                    )),
                    self.view_id,
                );
            }
            LapceCommand::SplitVertical => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::Split(true, self.view_id),
                    self.split_id,
                );
            }
            LapceCommand::ScrollUp => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::Scroll((0.0, -self.line_height)),
                    self.editor_id,
                );
            }
            LapceCommand::ScrollDown => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::Scroll((0.0, self.line_height)),
                    self.editor_id,
                );
            }
            LapceCommand::SplitHorizontal => {}
            LapceCommand::SplitRight => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::SplitMove(
                        SplitMoveDirection::Right,
                        self.view_id,
                    ),
                    self.split_id,
                );
            }
            LapceCommand::SplitLeft => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::SplitMove(
                        SplitMoveDirection::Left,
                        self.view_id,
                    ),
                    self.split_id,
                );
            }
            LapceCommand::SplitExchange => {
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::SplitExchange(self.view_id),
                    self.split_id,
                );
            }
            LapceCommand::NewLineAbove => {}
            LapceCommand::NewLineBelow => {}
            _ => (),
        }

        self.ensure_cursor_visible(buffer);
    }

    pub fn get_selection(
        &self,
        buffer: &Buffer,
        mode: &Mode,
        visual_mode: &VisualMode,
        start_insert: bool,
    ) -> Selection {
        match mode {
            Mode::Normal => self.selection.expand(),
            Mode::Insert => self.selection.clone(),
            Mode::Visual => match visual_mode {
                VisualMode::Normal => self.selection.expand(),
                VisualMode::Linewise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, _) =
                            buffer.offset_to_line_col(region.min());
                        let start = buffer.offset_of_line(start_line);
                        let (end_line, _) =
                            buffer.offset_to_line_col(region.max());
                        let mut end = buffer.offset_of_line(end_line + 1);
                        if start_insert {
                            end -= 1;
                        }
                        new_selection.add_region(SelRegion::new(
                            start,
                            end,
                            Some(ColPosition::Col(0)),
                        ));
                    }
                    new_selection
                }
                VisualMode::Blockwise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, start_col) =
                            buffer.offset_to_line_col(region.min());
                        let (end_line, end_col) =
                            buffer.offset_to_line_col(region.max() + 1);
                        let left = start_col.min(end_col);
                        let right = start_col.max(end_col);
                        for line in start_line..end_line + 1 {
                            let max_col = buffer.line_max_col(line, true);
                            if left > max_col {
                                continue;
                            }
                            let right = match region.horiz() {
                                Some(&ColPosition::End) => max_col,
                                _ => {
                                    if right > max_col {
                                        max_col
                                    } else {
                                        right
                                    }
                                }
                            };
                            let offset = buffer.offset_of_line(line);
                            new_selection.add_region(SelRegion::new(
                                offset + left,
                                offset + right,
                                Some(ColPosition::Col(left)),
                            ));
                        }
                    }
                    new_selection
                }
            },
        }
    }

    pub fn insert_mode(
        &mut self,
        buffer: &mut Buffer,
        mode: &Mode,
        visual_mode: &VisualMode,
        position: ColPosition,
    ) {
        match mode {
            Mode::Visual => match visual_mode {
                VisualMode::Blockwise => match position {
                    ColPosition::FirstNonBlank => {
                        let mut selection = Selection::new();
                        for region in self.selection.regions() {
                            let (start_line, start_col) =
                                buffer.offset_to_line_col(region.min());
                            let (end_line, end_col) =
                                buffer.offset_to_line_col(region.max());
                            let left = start_col.min(end_col);
                            for line in start_line..end_line + 1 {
                                let max_col = buffer.line_max_col(line, true);
                                if left > max_col {
                                    continue;
                                }
                                let offset = buffer.offset_of_line(line) + left;
                                selection.add_region(SelRegion::new(
                                    offset,
                                    offset,
                                    Some(ColPosition::Col(left)),
                                ));
                            }
                        }
                        self.selection = selection;
                    }
                    _ => (),
                },
                _ => {
                    self.selection = self.selection.min();
                }
            },
            Mode::Normal => {
                self.selection = Movement::StartOfLine.update_selection(
                    &self.selection,
                    buffer,
                    1,
                    mode == &Mode::Insert,
                    mode == &Mode::Visual,
                )
            }
            _ => (),
        }
    }

    pub fn paste(&mut self, buffer: &mut Buffer, content: &RegisterContent) {
        match content.kind {
            VisualMode::Linewise => {
                let old_offset = self.selection.get_cursor_offset();
                let mut selection = Selection::caret(
                    buffer.line_end_offset(old_offset, true) + 1,
                );
                for s in &content.content {
                    selection = buffer.insert(&format!("{}", s), &selection);
                }
                let (old_line, _) = buffer.offset_to_line_col(old_offset);
                let new_offset = buffer.offset_of_line(old_line + 1);
                self.selection = Selection::caret(new_offset);
            }
            VisualMode::Normal => {
                let mut selection =
                    Selection::caret(self.selection.get_cursor_offset() + 1);
                for s in &content.content {
                    selection = buffer.insert(s, &selection);
                }
                self.selection =
                    Selection::caret(selection.get_cursor_offset() - 1);
            }
            VisualMode::Blockwise => (),
        };
        self.request_paint()
    }

    pub fn insert_new_line(&mut self, buffer: &mut Buffer, offset: usize) {
        let (line, col) = buffer.offset_to_line_col(offset);
        let indent = buffer.indent_on_line(line);

        let indent = if indent.len() >= col {
            indent[..col].to_string()
        } else {
            let next_line_indent = buffer.indent_on_line(line + 1);
            if next_line_indent.len() > indent.len() {
                next_line_indent
            } else {
                indent
            }
        };

        let content = format!("{}{}", "\n", indent);
        self.selection = buffer.insert(&content, &Selection::caret(offset));
        // let new_offset = offset + content.len();
        // self.selection = Selection::caret(new_offset);
        self.ensure_cursor_visible(buffer);
        self.request_layout();
    }

    pub fn ensure_cursor_visible(&self, buffer: &Buffer) {
        let offset = self.selection.get_cursor_offset();
        let (line, col) = buffer.offset_to_line_col(offset);
        LAPCE_STATE.submit_ui_command(
            LapceUICommand::EnsureVisible((
                Rect::ZERO
                    .with_origin(Point::new(
                        col as f64 * self.char_width,
                        line as f64 * self.line_height,
                    ))
                    .with_size(Size::new(self.char_width, self.line_height)),
                (self.char_width, self.line_height),
            )),
            self.view_id,
        );
    }

    pub fn request_layout(&self) {
        LAPCE_STATE
            .submit_ui_command(LapceUICommand::RequestLayout, self.view_id);
    }

    pub fn request_paint(&self) {
        LAPCE_STATE
            .submit_ui_command(LapceUICommand::RequestPaint, self.view_id);
    }

    pub fn request_paint_rect(&self, rect: Rect) {
        LAPCE_STATE.submit_ui_command(
            LapceUICommand::RequestPaintRect(rect),
            self.editor_id,
        );
    }

    pub fn set_line_height(&mut self, line_height: f64) {
        self.line_height = line_height;
    }
}

pub struct RegisterContent {
    kind: VisualMode,
    content: Vec<String>,
}

pub struct EditorSplitState {
    widget_id: Option<WidgetId>,
    active: WidgetId,
    pub editors: HashMap<WidgetId, EditorState>,
    buffers: HashMap<BufferId, Buffer>,
    open_files: HashMap<String, BufferId>,
    id_counter: Counter,
    mode: Mode,
    visual_mode: VisualMode,
    operator: Option<EditorOperator>,
    register: HashMap<String, RegisterContent>,
}

impl EditorSplitState {
    pub fn new() -> EditorSplitState {
        EditorSplitState {
            widget_id: None,
            active: WidgetId::next(),
            editors: HashMap::new(),
            id_counter: Counter::default(),
            buffers: HashMap::new(),
            open_files: HashMap::new(),
            mode: Mode::Normal,
            visual_mode: VisualMode::Normal,
            operator: None,
            register: HashMap::new(),
        }
    }

    pub fn set_widget_id(&mut self, widget_id: WidgetId) {
        self.widget_id = Some(widget_id);
    }

    pub fn set_active(&mut self, widget_id: WidgetId) {
        self.active = widget_id;
    }

    pub fn active(&self) -> WidgetId {
        self.active
    }

    pub fn set_editor_scroll_offset(
        &mut self,
        editor_id: WidgetId,
        offset: Vec2,
    ) {
        if let Some(editor) = self.editors.get_mut(&editor_id) {
            editor.scroll_offset = offset;
        }
    }

    pub fn set_editor_size(&mut self, editor_id: WidgetId, size: Size) {
        if let Some(editor) = self.editors.get_mut(&editor_id) {
            editor.height = size.height;
            editor.width = size.width;
        }
    }

    pub fn open_file(&mut self, path: &str) {
        let buffer_id = if let Some(buffer_id) = self.open_files.get(path) {
            buffer_id.clone()
        } else {
            let buffer_id = self.next_buffer_id();
            let buffer = Buffer::new(buffer_id.clone(), path);
            self.buffers.insert(buffer_id.clone(), buffer);
            self.open_files.insert(path.to_string(), buffer_id.clone());
            buffer_id
        };
        if let Some(active_editor) = self.editors.get_mut(&self.active) {
            if let Some(active_buffer) = active_editor.buffer_id.as_mut() {
                if active_buffer == &buffer_id {
                    return;
                }
            }
            active_editor.selection = Selection::new_simple();
            active_editor.buffer_id = Some(buffer_id);
            LAPCE_STATE.submit_ui_command(
                LapceUICommand::ScrollTo((0.0, 0.0)),
                active_editor.view_id,
            );
            LAPCE_STATE.submit_ui_command(
                LapceUICommand::RequestLayout,
                active_editor.view_id,
            );
        }
    }

    fn next_buffer_id(&mut self) -> BufferId {
        BufferId(self.id_counter.next())
    }

    pub fn get_buffer(&mut self, id: &BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(id)
    }

    pub fn get_buffer_id(&self, view_id: &WidgetId) -> Option<BufferId> {
        self.editors
            .get(view_id)
            .map(|e| e.buffer_id.clone())
            .unwrap()
    }

    fn get_editor(&mut self, view_id: &WidgetId) -> &mut EditorState {
        self.editors.get_mut(view_id).unwrap()
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        match self.mode {
            Mode::Visual => match self.visual_mode {
                _ if self.visual_mode == visual_mode => {
                    self.mode = Mode::Normal;
                    if let Some(editor) = self.editors.get_mut(&self.active) {
                        editor.selection = editor.selection.to_caret();
                    }
                }
                _ => self.visual_mode = visual_mode,
            },
            _ => {
                self.mode = Mode::Visual;
                self.visual_mode = visual_mode;
            }
        };
    }

    fn get_active_editor(&mut self) -> Option<&mut EditorState> {
        self.editors.get_mut(&self.active)
    }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    pub fn has_operator(&self) -> bool {
        self.operator.is_some()
    }

    pub fn insert(&mut self, content: &str) -> Result<()> {
        if self.mode != Mode::Insert {
            return Ok(());
        }

        let buffer_id = {
            let editor =
                self.editors.get_mut(&self.active).ok_or(anyhow!(""))?;
            let buffer_id = editor.buffer_id.as_ref().ok_or(anyhow!(""))?;
            let buffer = self.buffers.get_mut(buffer_id).ok_or(anyhow!(""))?;
            editor.selection = buffer.insert(content, &editor.selection);
            editor.ensure_cursor_visible(buffer);
            buffer_id.clone()
        };

        // for (_, e) in self.editors.iter() {
        //     if let Some(current_buffer_id) = &e.buffer_id {
        //         if current_buffer_id == &buffer_id {
        //             LAPCE_STATE.submit_ui_command(
        //                 LapceUICommand::RequestLayout,
        //                 e.view_id,
        //             );
        //         }
        //     }
        // }
        Ok(())
    }

    pub fn run_command(&mut self, count: Option<usize>, cmd: LapceCommand) {
        let operator = self.operator.take();
        match cmd {
            LapceCommand::InsertMode => {
                self.mode = Mode::Insert;
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    self.active,
                );
            }
            LapceCommand::InsertFirstNonBlank => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.insert_mode(
                                buffer,
                                &self.mode,
                                &self.visual_mode,
                                ColPosition::FirstNonBlank,
                            );
                            editor.ensure_cursor_visible(buffer);
                            editor.request_paint();
                        }
                    }
                }
                self.mode = Mode::Insert;
            }
            LapceCommand::NormalMode => {
                let old_mode = self.mode.clone();
                self.mode = Mode::Normal;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection = editor.selection.to_caret();
                            if old_mode == Mode::Insert {
                                editor.selection = Movement::Left
                                    .update_selection(
                                        &editor.selection,
                                        buffer,
                                        1,
                                        false,
                                        false,
                                    );
                            }
                            LAPCE_STATE.submit_ui_command(
                                LapceUICommand::RequestPaint,
                                self.active,
                            );
                        }
                    }
                }
            }
            LapceCommand::ToggleVisualMode => {
                self.toggle_visual(VisualMode::Normal);
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    self.active,
                );
            }
            LapceCommand::ToggleLinewiseVisualMode => {
                self.toggle_visual(VisualMode::Linewise);
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    self.active,
                );
            }
            LapceCommand::ToggleBlockwiseVisualMode => {
                self.toggle_visual(VisualMode::Blockwise);
                LAPCE_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    self.active,
                );
            }
            LapceCommand::Append => {
                self.mode = Mode::Insert;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection = Movement::Right
                                .update_selection(
                                    &editor.selection,
                                    buffer,
                                    1,
                                    true,
                                    false,
                                );
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::AppendEndOfLine => {
                self.mode = Mode::Insert;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection = Movement::EndOfLine
                                .update_selection(
                                    &editor.selection,
                                    buffer,
                                    1,
                                    true,
                                    false,
                                );
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::NewLineAbove => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let line = buffer.line_of_offset(
                                editor.selection.get_cursor_offset(),
                            );
                            let offset =
                                buffer.first_non_blank_character_on_line(line);
                            editor.insert_new_line(buffer, offset);
                            editor.selection = Selection::caret(offset);
                            self.mode = Mode::Insert;
                            editor.request_paint();
                        }
                    }
                }
            }

            LapceCommand::NewLineBelow => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            self.mode = Mode::Insert;
                            let offset = buffer.line_end_offset(
                                editor.selection.get_cursor_offset(),
                                true,
                            );
                            editor.insert_new_line(buffer, offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::InsertNewLine => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            if editor.selection.regions().len() == 1 {
                                editor.insert_new_line(
                                    buffer,
                                    editor.selection.get_cursor_offset(),
                                );
                            } else {
                                editor.selection =
                                    buffer.insert("\n", &editor.selection);
                            }
                            editor.ensure_cursor_visible(buffer);
                            editor.request_layout();
                        }
                    }
                }
            }
            LapceCommand::DeleteWordBackward => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let offset = editor.selection.get_cursor_offset();
                            let new_offset = buffer.word_backword(offset);
                            buffer.insert(
                                "",
                                &Selection::region(new_offset, offset),
                            );
                            editor.selection = Selection::caret(new_offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::DeleteBackward => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection =
                                buffer.delete_backward(&editor.selection);
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::DeleteForeward => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection = buffer.delete_foreward(
                                &editor.get_selection(
                                    buffer,
                                    &self.mode,
                                    &self.visual_mode,
                                    false,
                                ),
                                &self.mode,
                                count.unwrap_or(1),
                            );
                            if self.mode == Mode::Visual {
                                editor.selection = buffer.correct_offset(
                                    &editor.selection.collapse(),
                                );
                                self.mode = Mode::Normal;
                            }
                            editor.ensure_cursor_visible(buffer);
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::DeleteForewardAndInsert => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.selection = buffer.delete_foreward(
                                &editor.get_selection(
                                    buffer,
                                    &self.mode,
                                    &self.visual_mode,
                                    true,
                                ),
                                &self.mode,
                                count.unwrap_or(1),
                            );
                            self.mode = Mode::Insert;
                            editor.ensure_cursor_visible(buffer);
                            editor.request_paint();
                        }
                    }
                }
            }
            LapceCommand::DeleteOperator => {
                self.operator =
                    Some(EditorOperator::Delete(EditorCount(count)));
            }
            LapceCommand::Paste => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            if let Some(content) = self.register.get("x") {
                                editor.paste(buffer, content);
                            }
                        }
                    }
                }
            }
            LapceCommand::DeleteVisual => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let content = buffer.yank(&editor.get_selection(
                                buffer,
                                &self.mode,
                                &self.visual_mode,
                                false,
                            ));
                            self.register.insert(
                                "x".to_string(),
                                RegisterContent {
                                    kind: self.visual_mode.clone(),
                                    content,
                                },
                            );
                            editor.selection = buffer.delete(
                                &editor.get_selection(
                                    buffer,
                                    &self.mode,
                                    &self.visual_mode,
                                    false,
                                ),
                                &self.mode,
                            );
                            editor.selection = buffer
                                .correct_offset(&editor.selection.collapse());
                            self.mode = Mode::Normal;
                            editor.ensure_cursor_visible(buffer);
                            editor.request_paint();
                        }
                    }
                }
                self.mode = Mode::Normal;
            }
            LapceCommand::Yank => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let content = buffer.yank(&editor.get_selection(
                                buffer,
                                &self.mode,
                                &self.visual_mode,
                                false,
                            ));
                            self.register.insert(
                                "x".to_string(),
                                RegisterContent {
                                    kind: self.visual_mode.clone(),
                                    content,
                                },
                            );
                            editor.selection = editor.selection.min();
                            editor.request_paint();
                        }
                    }
                }
                self.mode = Mode::Normal;
            }
            _ => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.run_command(
                                self.mode.clone(),
                                count,
                                buffer,
                                cmd,
                                operator,
                            );
                        }
                    }
                }
                // self.get_active_editor().map(|e| e.run_command(cmd));
            }
        }
        // self.request_paint();
    }

    pub fn buffer_request_paint(
        &self,
        buffer_id: &BufferId,
        inval_lines: &InvalLines,
    ) {
        for (_, editor) in &self.editors {
            if let Some(b) = &editor.buffer_id {
                if b == buffer_id {
                    let start = inval_lines.start_line;
                    editor.request_paint_rect(
                        Rect::ZERO
                            .with_origin(Point::new(
                                0.0,
                                start as f64 * editor.line_height,
                            ))
                            .with_size(Size::new(
                                editor.width,
                                inval_lines.new_count as f64
                                    * editor.line_height,
                            )),
                    );
                }
            }
        }
    }

    pub fn buffer_request_layout(&self, buffer_id: &BufferId) {
        for (_, editor) in &self.editors {
            if let Some(b) = &editor.buffer_id {
                if b == buffer_id {
                    editor.request_layout();
                }
            }
        }
    }

    pub fn editor_update_layouts(
        &mut self,
        editor_id: &WidgetId,
        text: &mut PietText,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        if let Some(editor) = self.editors.get(editor_id) {
            if let Some(buffer_id) = editor.buffer_id.as_ref() {
                if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                    let start_line =
                        (editor.scroll_offset.y / editor.line_height) as usize;
                    let num_lines =
                        (editor.height / editor.line_height) as usize;
                    let lines = (start_line..start_line + num_lines)
                        .collect::<Vec<usize>>();

                    if let Some(buffer_ui) = data.buffers.get_mut(&buffer_id) {
                        buffer_ui.update_layouts(text, buffer, &lines, env);
                    } else {
                        let mut buffer_ui = BufferUIState {
                            id: buffer_id.clone(),
                            text_layouts: Vec::new(),
                        };
                        buffer_ui.update_layouts(text, buffer, &lines, env);
                        data.buffers.insert(buffer_id.clone(), buffer_ui);
                    }
                }
            }
        }
    }

    pub fn buffer_update(
        &mut self,
        text: &mut PietText,
        buffer_id: &BufferId,
        data: &mut LapceUIState,
        inval_lines: &InvalLines,
        env: &Env,
    ) {
        let buffer = self.buffers.get_mut(&buffer_id).unwrap();

        let mut buffer_lines = HashMap::new();
        for (_, editor) in &self.editors {
            if let Some(b) = &editor.buffer_id {
                if b == buffer_id {
                    let start_line =
                        (editor.scroll_offset.y / editor.line_height) as usize;
                    let num_lines =
                        (editor.height / editor.line_height) as usize;
                    for line in start_line..start_line + num_lines {
                        buffer_lines.insert(line, line);
                    }
                }
            }
        }
        if let Some(buffer_ui) = data.buffers.get_mut(&buffer_id) {
            buffer_ui.update(text, buffer, inval_lines, buffer_lines, env);
        } else {
            let mut buffer_ui = BufferUIState {
                id: buffer_id.clone(),
                text_layouts: Vec::new(),
            };
            buffer_ui.update(text, buffer, inval_lines, buffer_lines, env);
            data.buffers.insert(buffer_id.clone(), buffer_ui);
        }

        let mut request_layout = false;
        if buffer.max_len_line >= inval_lines.start_line
            && buffer.max_len_line
                < inval_lines.start_line + inval_lines.inval_count
        {
            buffer.update_max_line_len();
            request_layout = true;
        } else {
            let mut max_len = 0;
            let mut max_len_line = 0;
            for line in inval_lines.start_line
                ..inval_lines.start_line + inval_lines.new_count
            {
                let line_len = buffer.line_len(line);
                if line_len > max_len {
                    max_len = line_len;
                    max_len_line = line;
                }
            }
            if max_len > buffer.max_len {
                buffer.max_len = max_len;
                buffer.max_len_line = max_len_line;
                request_layout = true;
            } else if buffer.max_len >= inval_lines.start_line {
                buffer.max_len_line = buffer.max_len_line
                    + inval_lines.new_count
                    - inval_lines.inval_count;
            }
            if inval_lines.new_count != inval_lines.inval_count {
                request_layout = true;
            }
        }
        if request_layout {
            self.buffer_request_layout(buffer_id);
            return;
        }
        self.buffer_request_paint(buffer_id, inval_lines);
    }

    pub fn get_mode(&self) -> Mode {
        self.mode.clone()
    }

    pub fn request_paint(&self) {
        LAPCE_STATE.submit_ui_command(
            LapceUICommand::RequestPaint,
            self.widget_id.unwrap(),
        );
    }
}

pub struct EditorView {
    view_id: WidgetId,
    pub editor_id: WidgetId,
    editor: WidgetPod<
        LapceUIState,
        LapceScroll<LapceUIState, Padding<LapceUIState>>,
    >,
    gutter: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
}

impl EditorView {
    pub fn new(
        split_id: WidgetId,
        buffer_id: Option<BufferId>,
    ) -> IdentityWrapper<EditorView> {
        let view_id = WidgetId::next();
        let editor_id = WidgetId::next();
        let editor =
            IdentityWrapper::wrap(Editor::new(view_id), editor_id.clone());
        let scroll = LapceScroll::new(editor.padding((10.0, 0.0, 10.0, 0.0)));
        let editor_state =
            EditorState::new(editor_id, view_id, split_id, buffer_id);
        LAPCE_STATE
            .editor_split
            .lock()
            .unwrap()
            .editors
            .insert(view_id, editor_state);
        EditorView {
            view_id,
            editor_id,
            editor: WidgetPod::new(scroll),
            gutter: WidgetPod::new(
                EditorGutter::new(view_id).padding((10.0, 0.0, 10.0, 0.0)),
            )
            .boxed(),
        }
        .with_id(view_id)
    }
}

impl Widget<LapceUIState> for EditorView {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                self.gutter.event(ctx, event, data, env);
                self.editor.event(ctx, event, data, env);
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        LapceUICommand::EnsureVisible((rect, margin)) => {
                            let editor = self.editor.widget_mut();
                            if editor.ensure_visible(ctx.size(), rect, margin) {
                                let offset = editor.offset();
                                self.gutter.set_viewport_offset(Vec2::new(
                                    0.0, offset.y,
                                ));
                                ctx.request_paint();
                            }
                        }
                        LapceUICommand::ScrollTo((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll_to(*x, *y);
                        }
                        LapceUICommand::Scroll((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll(*x, *y);
                            ctx.request_paint();
                        }
                        _ => (),
                    }
                    LAPCE_STATE
                        .editor_split
                        .lock()
                        .unwrap()
                        .set_editor_scroll_offset(
                            self.view_id,
                            self.editor.widget_mut().offset(),
                        );
                    LAPCE_STATE
                        .editor_split
                        .lock()
                        .unwrap()
                        .editor_update_layouts(
                            &self.view_id,
                            ctx.text(),
                            data,
                            env,
                        );
                }
                _ => (),
            },
            _ => self.editor.event(ctx, event, data, env),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        self.gutter.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(gutter_size),
        );
        let editor_size =
            Size::new(self_size.width - gutter_size.width, self_size.height);
        LAPCE_STATE
            .editor_split
            .lock()
            .unwrap()
            .set_editor_size(self.view_id, editor_size);
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO
                .with_origin(Point::new(gutter_size.width, 0.0))
                .with_size(editor_size),
        );
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let viewport = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            let scroll_offset = self.editor.widget().offset();
            ctx.clip(viewport);
            ctx.transform(Affine::translate(-scroll_offset));

            let mut visible = ctx.region().clone();
            visible += scroll_offset;
            ctx.with_child_ctx(visible, |ctx| {
                self.gutter.paint(ctx, data, env);
            })
        });
        self.editor.paint(ctx, data, env);
    }
}

pub struct EditorGutter {
    view_id: WidgetId,
    text_layouts: HashMap<String, EditorTextLayout>,
}

impl EditorGutter {
    pub fn new(view_id: WidgetId) -> EditorGutter {
        EditorGutter {
            view_id,
            text_layouts: HashMap::new(),
        }
    }
}

impl<T: Data> Widget<T> for EditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        let buffer_id = {
            LAPCE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &LAPCE_STATE.editor_split.lock().unwrap().buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            Size::new(
                width * buffer.last_line().to_string().len() as f64,
                25.0 * buffer.num_lines() as f64,
            )
        } else {
            Size::new(50.0, 50.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = {
            LAPCE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
                .clone()
        };
        if let Some(buffer_id) = buffer_id {
            let mut editor_split = LAPCE_STATE.editor_split.lock().unwrap();
            let mut layout = TextLayout::new("W");
            layout.set_font(LapceTheme::EDITOR_FONT);
            layout.rebuild_if_needed(&mut ctx.text(), env);
            let width = layout.point_for_text_position(1).x;
            let buffers = &editor_split.buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let (current_line, _) = {
                let editor = editor_split.editors.get(&self.view_id).unwrap();
                buffer.offset_to_line_col(editor.selection.get_cursor_offset())
            };
            let active = editor_split.active;
            let rects = ctx.region().rects().to_vec();
            for rect in rects {
                let start_line = (rect.y0 / line_height).floor() as usize;
                let num_lines = (rect.height() / line_height).floor() as usize;
                let last_line = buffer.last_line();
                for line in start_line..start_line + num_lines {
                    if line > last_line {
                        break;
                    }
                    let content = if active != self.view_id {
                        line
                    } else {
                        if line == current_line {
                            line
                        } else if line > current_line {
                            line - current_line
                        } else {
                            current_line - line
                        }
                    };
                    let x = (last_line.to_string().len()
                        - content.to_string().len())
                        as f64
                        * width;
                    let content = content.to_string();
                    if let Some(text_layout) =
                        self.text_layouts.get_mut(&content)
                    {
                        if text_layout.text != content {
                            text_layout.layout.set_text(content.clone());
                            text_layout.text = content;
                            text_layout
                                .layout
                                .rebuild_if_needed(&mut ctx.text(), env);
                        }
                        text_layout.layout.draw(
                            ctx,
                            Point::new(x, line_height * line as f64),
                        );
                    } else {
                        let mut layout = TextLayout::new(content.clone());
                        layout.set_font(LapceTheme::EDITOR_FONT);
                        layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
                        layout.rebuild_if_needed(&mut ctx.text(), env);
                        layout.draw(
                            ctx,
                            Point::new(x, line_height * line as f64),
                        );
                        let text_layout = EditorTextLayout {
                            layout,
                            text: content.clone(),
                        };
                        self.text_layouts.insert(content, text_layout);
                    }
                }
            }
        }
    }
}

struct EditorTextLayout {
    layout: TextLayout,
    text: String,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

pub struct Editor {
    text_layout: TextLayout,
    view_id: WidgetId,
    text_layouts: HashMap<usize, HighlightTextLayout>,
}

impl Editor {
    pub fn new(view_id: WidgetId) -> Self {
        let text_layout = TextLayout::new("");
        Editor {
            text_layout,
            view_id,
            text_layouts: HashMap::new(),
        }
    }

    fn paint_line_new(
        &mut self,
        ctx: &mut PaintCtx,
        buffer: &mut Buffer,
        line_height: f64,
        line: usize,
        line_content: &str,
        // text_layouts: &mut Vec<Option<HighlightTextLayout>>,
        env: &Env,
    ) {
        let start_offset = buffer.offset_of_line(line);
        let end_offset = buffer.offset_of_line(line + 1);
        let mut offset = start_offset;
        let mut x = 0.0;
        let mut layout_builder = ctx
            .text()
            .new_text_layout(line_content.to_string())
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));

        for (start, end, hl) in buffer.get_line_highligh(line) {
            if let Some(color) = LAPCE_STATE.theme.lock().unwrap().get(hl) {
                layout_builder = layout_builder.range_attribute(
                    start - start_offset..end - start_offset,
                    TextAttribute::TextColor(color.clone()),
                );
            }
        }
        let layout = layout_builder.build().unwrap();
        ctx.draw_text(&layout, Point::new(0.0, line_height * line as f64));
        let text_layout = HighlightTextLayout {
            layout,
            text: line_content.to_string(),
            highlights: buffer.get_line_highligh(line).clone(),
        };
        // for _ in text_layouts.len()..line {
        //     text_layouts.push(None);
        // }
        // text_layouts[line] = Some(text_layout);
    }

    fn paint_line(
        &mut self,
        ctx: &mut PaintCtx,
        buffer: &mut Buffer,
        line_height: f64,
        line: usize,
        line_content: &str,
        env: &Env,
    ) {
        let start_offset = buffer.offset_of_line(line);
        let end_offset = buffer.offset_of_line(line + 1);
        let mut offset = start_offset;
        let mut x = 0.0;
        let mut layout_builder = ctx
            .text()
            .new_text_layout(line_content.to_string())
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));

        for (start, end, hl) in buffer.get_line_highligh(line) {
            if let Some(color) = LAPCE_STATE.theme.lock().unwrap().get(hl) {
                layout_builder = layout_builder.range_attribute(
                    start - start_offset..end - start_offset,
                    TextAttribute::TextColor(color.clone()),
                );
            }
        }
        let layout = layout_builder.build().unwrap();
        ctx.draw_text(&layout, Point::new(0.0, line_height * line as f64));
        let text_layout = HighlightTextLayout {
            layout,
            text: line_content.to_string(),
            highlights: buffer.get_line_highligh(line).clone(),
        };
        self.text_layouts.insert(line, text_layout);
    }

    fn paint_insert_cusor(
        &mut self,
        ctx: &mut PaintCtx,
        selection: &Selection,
        buffer: &mut Buffer,
        line_height: f64,
        width: f64,
        start_line: usize,
        number_lines: usize,
        env: &Env,
    ) {
        let start = buffer.offset_of_line(start_line);
        let last_line = buffer.last_line();
        let mut end_line = start_line + number_lines;
        if end_line > last_line {
            end_line = last_line;
        }
        let end = buffer.offset_of_line(end_line + 1);
        let regions = selection.regions_in_range(start, end);
        for region in regions {
            let (line, col) = buffer.offset_to_line_col(region.min());
            let x = col as f64 * width;
            let y = line as f64 * line_height;
            ctx.stroke(
                Line::new(Point::new(x, y), Point::new(x, y + line_height)),
                &env.get(LapceTheme::EDITOR_CURSOR_COLOR),
                2.0,
            )
        }
    }

    fn paint_selection(
        &mut self,
        ctx: &mut PaintCtx,
        mode: &Mode,
        visual_mode: &VisualMode,
        selection: &Selection,
        buffer: &mut Buffer,
        line_height: f64,
        width: f64,
        start_line: usize,
        number_lines: usize,
        env: &Env,
    ) {
        match mode {
            &Mode::Visual => (),
            _ => return,
        }
        let start = buffer.offset_of_line(start_line);
        let last_line = buffer.last_line();
        let mut end_line = start_line + number_lines;
        if end_line > last_line {
            end_line = last_line;
        }
        let end = buffer.offset_of_line(end_line + 1);

        let regions = selection.regions_in_range(start, end);
        for region in regions {
            let (start_line, start_col) =
                buffer.offset_to_line_col(region.min());
            let (end_line, end_col) = buffer.offset_to_line_col(region.max());

            for line in start_line..end_line + 1 {
                let x0 = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == start_line => start_col as f64 * width,
                        _ => 0.0,
                    },
                    &VisualMode::Linewise => 0.0,
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false);
                        let left = start_col.min(end_col);
                        if left > max_col {
                            continue;
                        }
                        left as f64 * width
                    }
                };

                let x1 = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == end_line => (end_col + 1) as f64 * width,
                        _ => {
                            (buffer.offset_of_line(line + 1)
                                - buffer.offset_of_line(line))
                                as f64
                                * width
                        }
                    },
                    &VisualMode::Linewise => {
                        (buffer.offset_of_line(line + 1)
                            - buffer.offset_of_line(line))
                            as f64
                            * width
                    }
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false) + 1;
                        let right = match region.horiz() {
                            Some(&ColPosition::End) => max_col,
                            _ => (end_col.max(start_col) + 1).min(max_col),
                        };
                        right as f64 * width
                    }
                };
                let y0 = line as f64 * line_height;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    &env.get(LapceTheme::EDITOR_SELECTION_COLOR),
                );
            }
        }
    }
}

impl Widget<LapceUIState> for Editor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            println!("editor request layout");
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            println!("editor request paint");
                            ctx.request_paint();
                        }
                        LapceUICommand::RequestPaintRect(rect) => {
                            ctx.request_paint_rect(*rect);
                        }
                        _ => println!(
                            "editor unprocessed ui command {:?}",
                            command
                        ),
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let buffer_id = {
            LAPCE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &LAPCE_STATE.editor_split.lock().unwrap().buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            Size::new(
                width * buffer.max_len as f64,
                25.0 * buffer.num_lines() as f64,
            )
        } else {
            Size::new(0.0, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = {
            LAPCE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
                .clone()
        };
        if let Some(buffer_id) = buffer_id {
            let mut editor_split = LAPCE_STATE.editor_split.lock().unwrap();

            let mut layout = TextLayout::new("W");
            layout.set_font(LapceTheme::EDITOR_FONT);
            layout.rebuild_if_needed(&mut ctx.text(), env);
            let width = layout.point_for_text_position(1).x;
            let mode = editor_split.get_mode().clone();
            let visual_mode = editor_split.visual_mode.clone();
            let active_view_id = editor_split.active.clone();
            let (editor_width, editor_offset, selection) = {
                let editor = editor_split.get_editor(&self.view_id);
                editor.set_line_height(line_height);
                editor.char_width = width;
                (
                    editor.width,
                    editor.selection.get_cursor_offset(),
                    editor.selection.clone(),
                )
            };

            let mut buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
            let cursor = buffer.offset_to_line_col(editor_offset);
            let rects = ctx.region().rects().to_vec();
            for rect in rects {
                let start_line = (rect.y0 / line_height).floor() as usize;
                let num_lines = (rect.height() / line_height).floor() as usize;
                if mode == Mode::Visual {
                    self.paint_selection(
                        ctx,
                        &mode,
                        &visual_mode,
                        &selection,
                        buffer,
                        line_height,
                        width,
                        start_line,
                        num_lines,
                        env,
                    );
                }
                let last_line = buffer.last_line();
                for line in start_line..start_line + num_lines + 1 {
                    if line > last_line {
                        break;
                    }
                    let line_content = buffer
                        .slice_to_cow(
                            buffer.offset_of_line(line)
                                ..buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    if line == cursor.0 {
                        match mode {
                            Mode::Visual => (),
                            _ => {
                                ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(
                                        0.0,
                                        cursor.0 as f64 * line_height,
                                    ))
                                    .with_size(Size::new(
                                        editor_width,
                                        line_height,
                                    )),
                                &env.get(
                                    LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                                ),
                            );
                            }
                        };

                        if active_view_id == self.view_id {
                            let cursor_x = (line_content[..cursor.1]
                                .chars()
                                .filter_map(|c| {
                                    if c == '\t' {
                                        Some('\t')
                                    } else {
                                        None
                                    }
                                })
                                .count()
                                * 3
                                + cursor.1)
                                as f64
                                * width;
                            match mode {
                                Mode::Insert => (),
                                _ => ctx.fill(
                                    Rect::ZERO
                                        .with_origin(Point::new(
                                            cursor_x,
                                            cursor.0 as f64 * line_height,
                                        ))
                                        .with_size(Size::new(
                                            width,
                                            line_height,
                                        )),
                                    &env.get(LapceTheme::EDITOR_CURSOR_COLOR),
                                ),
                            };
                        }
                    }

                    let mut cache_draw = false;
                    if let Some(buffer_ui) = data.buffers.get(&buffer_id) {
                        if buffer_ui.text_layouts.len() > line {
                            if let Some(layout) =
                                buffer_ui.text_layouts[line].as_ref()
                            {
                                if layout.text == line_content.to_string()
                                    && &layout.highlights
                                        == buffer.get_line_highligh(line)
                                {
                                    ctx.draw_text(
                                        &layout.layout,
                                        Point::new(
                                            0.0,
                                            line_height * line as f64,
                                        ),
                                    );
                                    cache_draw = true;
                                }
                            }
                        }
                    }
                    // if !cache_draw {
                    //     println!("can't draw from cache");
                    //     let text_layout = BufferUIState::get_text_layout(
                    //         ctx.text(),
                    //         buffer,
                    //         line,
                    //         line_content,
                    //         env,
                    //     );
                    //     ctx.draw_text(
                    //         &text_layout.layout,
                    //         Point::new(0.0, line as f64 * line_height),
                    //     );
                    // }

                    // if let Some(text_layout) = self.text_layouts.get_mut(&line)
                    // {
                    //     if text_layout.text != line_content.to_string()
                    //         || &text_layout.highlights
                    //             != buffer.get_line_highligh(line)
                    //     {
                    //         self.paint_line(
                    //             ctx,
                    //             &mut buffer,
                    //             line_height,
                    //             line,
                    //             &line_content,
                    //             env,
                    //         );
                    //     } else {
                    //         ctx.draw_text(
                    //             &text_layout.layout,
                    //             Point::new(0.0, line_height * line as f64),
                    //         );
                    //     }
                    // } else {
                    //     self.paint_line(
                    //         ctx,
                    //         &mut buffer,
                    //         line_height,
                    //         line,
                    //         &line_content,
                    //         env,
                    //     );
                    // }
                    if mode == Mode::Insert {
                        self.paint_insert_cusor(
                            ctx,
                            &selection,
                            buffer,
                            line_height,
                            width,
                            start_line,
                            num_lines,
                            env,
                        );
                    }
                }
            }
        }
    }
}
