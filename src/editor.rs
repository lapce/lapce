use crate::{
    buffer::{Buffer, BufferId},
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
    state::Mode,
    state::VisualMode,
    state::LAPCE_STATE,
    theme::LapceTheme,
};
use druid::piet::{PietTextLayout, Text, TextAttribute, TextLayoutBuilder};
use druid::{
    kurbo::Line, widget::IdentityWrapper, widget::Padding, Affine,
    BoxConstraints, Data, Env, Event, EventCtx, KeyEvent, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, TextLayout,
    UpdateCtx, Vec2, Widget, WidgetExt, WidgetId, WidgetPod,
};
use std::collections::HashMap;
use xi_rope::Interval;

pub struct LapceUI {
    container: LapceContainer<u32>,
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
        }
    }

    pub fn run_command(
        &mut self,
        mode: Mode,
        count: Option<usize>,
        buffer: &mut Buffer,
        cmd: LapceCommand,
    ) {
        match cmd {
            LapceCommand::Left => {
                self.selection = Movement::Left(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
            }
            LapceCommand::Right => {
                self.selection = Movement::Right(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
            }
            LapceCommand::Up => {
                self.selection = Movement::Up(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
            }
            LapceCommand::Down => {
                self.selection = Movement::Down(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
            }
            LapceCommand::PageDown => {
                let lines =
                    (self.height / self.line_height / 2.0).floor() as usize;
                self.selection = Movement::Down(lines).update_selection(
                    &self.selection,
                    buffer,
                    &mode,
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
                self.selection = Movement::Up(lines).update_selection(
                    &self.selection,
                    buffer,
                    &mode,
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
            LapceCommand::LineEnd => {
                self.selection = Movement::EndOfLine.update_selection(
                    &self.selection,
                    buffer,
                    &mode,
                );
                self.request_paint();
            }
            LapceCommand::LineStart => {
                self.selection = Movement::StartOfLine.update_selection(
                    &self.selection,
                    buffer,
                    &mode,
                );
                self.request_paint();
            }
            LapceCommand::GotoLineDefaultFirst => {
                let position = match count {
                    Some(count) => LinePosition::Line(count),
                    None => LinePosition::First,
                };
                self.selection = Movement::Line(position).update_selection(
                    &self.selection,
                    buffer,
                    &mode,
                );
                self.request_paint();
            }
            LapceCommand::GotoLineDefaultLast => {
                let position = match count {
                    Some(count) => LinePosition::Line(count),
                    None => LinePosition::Last,
                };
                self.selection = Movement::Line(position).update_selection(
                    &self.selection,
                    buffer,
                    &mode,
                );
                self.request_paint();
            }
            LapceCommand::WordFoward => {
                self.selection = Movement::WordForward(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
            }
            LapceCommand::WordBackward => {
                self.selection = Movement::WordBackward(count.unwrap_or(1))
                    .update_selection(&self.selection, buffer, &mode);
                self.request_paint();
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
    ) -> Selection {
        match mode {
            Mode::Normal => self.selection.clone(),
            Mode::Insert => self.selection.clone(),
            Mode::Visual => match visual_mode {
                VisualMode::Normal => self.selection.clone(),
                VisualMode::Linewise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, _) =
                            buffer.offset_to_line_col(region.min());
                        let start = buffer.offset_of_line(start_line);
                        let (end_line, _) =
                            buffer.offset_to_line_col(region.max());
                        let max_col = buffer.max_col(mode, end_line);
                        let end = buffer.offset_of_line(end_line) + max_col;
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
                            buffer.offset_to_line_col(region.max());
                        let left = start_col.min(end_col);
                        let right = start_col.max(end_col);
                        for line in start_line..end_line + 1 {
                            let max_col = buffer.max_col(mode, line);
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
                                let max_col =
                                    buffer.max_col(&Mode::Insert, line);
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
                    let offset = self.selection.min();
                    self.selection = Selection::caret(offset);
                }
            },
            Mode::Normal => {
                self.selection = Movement::StartOfLine.update_selection(
                    &self.selection,
                    buffer,
                    mode,
                )
            }
            _ => (),
        }
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

    pub fn set_line_height(&mut self, line_height: f64) {
        self.line_height = line_height;
    }
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

    pub fn insert(&mut self, content: &str) {
        if self.mode != Mode::Insert {
            return;
        }

        if let Some(editor) = self.editors.get_mut(&self.active) {
            if let Some(buffer_id) = editor.buffer_id.as_ref() {
                if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                    editor.selection =
                        buffer.insert(content, &editor.selection);
                    editor.ensure_cursor_visible(buffer);
                    LAPCE_STATE.submit_ui_command(
                        LapceUICommand::RequestPaint,
                        self.active,
                    );
                }
            }
        }
    }

    pub fn run_command(&mut self, count: Option<usize>, cmd: LapceCommand) {
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
                                editor.selection =
                                    Movement::Left(count.unwrap_or(1))
                                        .update_selection(
                                            &editor.selection,
                                            buffer,
                                            &self.mode,
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
                            editor.selection =
                                Movement::Right(count.unwrap_or(1))
                                    .update_selection(
                                        &editor.selection,
                                        buffer,
                                        &self.mode,
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
                                    &self.mode,
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
                                &self.mode,
                                editor.selection.get_cursor_offset(),
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
                            editor.selection = buffer
                                .delete_foreward(
                                    &editor.get_selection(
                                        buffer,
                                        &self.mode,
                                        &self.visual_mode,
                                    ),
                                    &self.mode,
                                    count.unwrap_or(1),
                                )
                                .collapse();
                            self.mode = Mode::Normal;
                            editor.ensure_cursor_visible(buffer);
                            editor.request_paint();
                        }
                    }
                }
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
                            );
                        }
                    }
                }
                // self.get_active_editor().map(|e| e.run_command(cmd));
            }
        }
        // self.request_paint();
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

pub struct EditorView<T> {
    view_id: WidgetId,
    pub editor_id: WidgetId,
    editor: WidgetPod<T, LapceScroll<T, Padding<T>>>,
    gutter: WidgetPod<T, Box<dyn Widget<T>>>,
}

impl<T: Data> EditorView<T> {
    pub fn new(
        split_id: WidgetId,
        buffer_id: Option<BufferId>,
    ) -> IdentityWrapper<EditorView<T>> {
        let view_id = WidgetId::next();
        let editor_id = WidgetId::next();
        let editor = Editor::new(view_id);
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

impl<T: Data> Widget<T> for EditorView<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
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
                            return;
                        }
                        LapceUICommand::ScrollTo((x, y)) => {
                            self.editor.widget_mut().scroll_to(*x, *y);
                            return;
                        }
                        LapceUICommand::Scroll((x, y)) => {
                            self.editor.widget_mut().scroll(*x, *y);
                            ctx.request_paint();
                            return;
                        }
                        _ => (),
                    }
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
        data: &T,
        env: &Env,
    ) {
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
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

struct HighlightTextLayout {
    layout: PietTextLayout,
    text: String,
    highlights: Vec<(usize, usize, String)>,
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
                        let max_col = buffer.max_col(mode, line);
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
                        let max_col = buffer.max_col(mode, line) + 1;
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

impl<T: Data> Widget<T> for Editor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
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
                width * buffer.max_line_len as f64,
                25.0 * buffer.num_lines() as f64,
            )
        } else {
            Size::new(0.0, 0.0)
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
                    if let Some(text_layout) = self.text_layouts.get_mut(&line)
                    {
                        if text_layout.text != line_content.to_string()
                            || &text_layout.highlights
                                != buffer.get_line_highligh(line)
                        {
                            self.paint_line(
                                ctx,
                                &mut buffer,
                                line_height,
                                line,
                                &line_content,
                                env,
                            );
                        } else {
                            ctx.draw_text(
                                &text_layout.layout,
                                Point::new(0.0, line_height * line as f64),
                            );
                        }
                    } else {
                        self.paint_line(
                            ctx,
                            &mut buffer,
                            line_height,
                            line,
                            &line_content,
                            env,
                        );
                    }
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
