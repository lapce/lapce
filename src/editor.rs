use crate::{
    buffer::WordCursor,
    buffer::{Buffer, BufferId},
    command::CraneCommand,
    command::CraneUICommand,
    command::CRANE_UI_COMMAND,
    container::CraneContainer,
    scroll::CraneScroll,
    split::SplitMoveDirection,
    state::Mode,
    state::CRANE_STATE,
    theme::CraneTheme,
};
use druid::{
    kurbo::Line, theme, widget::IdentityWrapper, widget::Padding, Affine,
    BoxConstraints, Data, Env, Event, EventCtx, ExtEventSink, Key, KeyEvent,
    LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, Rect,
    RenderContext, Selector, Size, Target, TextLayout, UpdateCtx, Vec2, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use lazy_static::lazy_static;
use std::time::Duration;
use std::{any::Any, thread};
use std::{collections::HashMap, sync::Arc, sync::Mutex};
use xi_core_lib::line_offset::{LineOffset, LogicalLines};
use xi_rope::{Cursor, Interval, Rope, RopeInfo};

pub struct CraneUI {
    container: CraneContainer<u32>,
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
    offset: usize,
    horiz: usize,
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
            offset: 0,
            horiz: 0,
            line_height: 0.0,
            char_width: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }

    pub fn line_start_offset(&mut self, buffer: &mut Buffer) -> usize {
        let line = buffer.line_of_offset(self.offset);
        buffer.offset_of_line(line)
    }

    pub fn col_on_line(
        &mut self,
        mode: Mode,
        buffer: &mut Buffer,
        line: usize,
    ) -> usize {
        let max_col = match buffer.offset_of_line(line + 1)
            - buffer.offset_of_line(line)
        {
            n if n == 0 => 0,
            n if n == 1 => 0,
            n => match mode {
                Mode::Insert => n - 1,
                _ => n - 2,
            },
        };
        let col = if max_col > self.horiz {
            self.horiz
        } else {
            max_col
        };
        col
    }

    pub fn line_end_offset(
        &mut self,
        mode: Mode,
        buffer: &mut Buffer,
    ) -> usize {
        let line = buffer.line_of_offset(self.offset);
        let line_start_offset = buffer.offset_of_line(line);
        let line_end_offset = buffer.offset_of_line(line + 1);
        let line_end_offset = if line_end_offset - line_start_offset <= 1 {
            line_start_offset
        } else {
            if mode == Mode::Insert {
                line_end_offset - 1
            } else {
                line_end_offset - 2
            }
        };
        line_end_offset
    }

    pub fn move_right(
        &mut self,
        mode: Mode,
        count: usize,
        buffer: &mut Buffer,
    ) {
        let line_end_offset = self.line_end_offset(mode, buffer);

        let mut new_offset = self.offset + count;
        if new_offset > buffer.rope.len() {
            new_offset = buffer.rope.len()
        }
        if new_offset > line_end_offset {
            new_offset = line_end_offset;
        }
        if new_offset == self.offset {
            return;
        }

        self.offset = new_offset;
        let (_, col) = buffer.offset_to_line_col(self.offset);
        self.horiz = col;
        self.request_paint();
    }

    pub fn move_left(&mut self, mode: Mode, count: usize, buffer: &mut Buffer) {
        let line = buffer.line_of_offset(self.offset);
        let line_start_offset = buffer.offset_of_line(line);
        let new_offset = if self.offset < count {
            0
        } else if self.offset - count > line_start_offset {
            self.offset - count
        } else {
            line_start_offset
        };
        if new_offset == self.offset {
            return;
        }
        self.offset = new_offset;
        let (_, col) = buffer.offset_to_line_col(self.offset);
        self.horiz = col;
        self.request_paint();
    }

    pub fn move_up(&mut self, mode: Mode, count: usize, buffer: &mut Buffer) {
        let line = buffer.line_of_offset(self.offset);
        let line = if line > count { line - count } else { 0 };
        let mut max_col = buffer.rope.offset_of_line(line + 1)
            - buffer.rope.offset_of_line(line)
            - 1;
        if max_col > 0 && mode != Mode::Insert {
            max_col -= 1;
        }
        let col = if max_col > self.horiz {
            self.horiz
        } else {
            max_col
        };
        self.offset = buffer.rope.offset_of_line(line) + col;
        self.request_paint();
    }

    pub fn move_down(&mut self, mode: Mode, count: usize, buffer: &mut Buffer) {
        let last_line = buffer.last_line();
        let line = buffer.line_of_offset(self.offset) + count;
        let line = if line > last_line { last_line } else { line };
        let col = self.col_on_line(mode, buffer, line);
        self.offset = buffer.offset_of_line(line) + col;
        self.request_paint();
    }

    pub fn move_to_line_end(&mut self, mode: Mode, buffer: &mut Buffer) {
        self.offset = self.line_end_offset(mode, buffer);
        let (_, col) = buffer.offset_to_line_col(self.offset);
        self.horiz = col;
        self.request_paint();
    }

    pub fn move_to_line(
        &mut self,
        mode: Mode,
        buffer: &mut Buffer,
        line: usize,
    ) {
        let col = self.col_on_line(mode, buffer, line);
        self.offset = buffer.offset_of_line(line) + col;
        self.request_paint();
    }

    pub fn run_command(
        &mut self,
        mode: Mode,
        count: Option<usize>,
        buffer: &mut Buffer,
        cmd: CraneCommand,
    ) {
        match cmd {
            CraneCommand::Left => {
                let count = count.unwrap_or(1);
                self.move_left(mode, count, buffer);
            }
            CraneCommand::Right => {
                let count = count.unwrap_or(1);
                self.move_right(mode, count, buffer);
            }
            CraneCommand::Up => {
                let count = count.unwrap_or(1);
                self.move_up(mode, count, buffer);
            }
            CraneCommand::Down => {
                let count = count.unwrap_or(1);
                self.move_down(mode, count, buffer);
            }
            CraneCommand::PageDown => {
                let lines =
                    (self.height / self.line_height / 2.0).floor() as usize;
                self.move_down(mode, lines, buffer);
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((
                        0.0,
                        self.line_height * lines as f64,
                    )),
                    self.view_id,
                );
            }
            CraneCommand::PageUp => {
                let lines =
                    (self.height / self.line_height / 2.0).floor() as usize;
                self.move_up(mode, lines, buffer);
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((
                        0.0,
                        -self.line_height * lines as f64,
                    )),
                    self.view_id,
                );
            }
            CraneCommand::SplitVertical => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Split(true, self.view_id),
                    self.split_id,
                );
            }
            CraneCommand::ScrollUp => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((0.0, -self.line_height)),
                    self.editor_id,
                );
            }
            CraneCommand::ScrollDown => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((0.0, self.line_height)),
                    self.editor_id,
                );
            }
            CraneCommand::LineEnd => {
                self.move_to_line_end(mode, buffer);
            }
            CraneCommand::LineStart => {
                self.offset = self.line_start_offset(buffer);
                self.horiz = 0;
                self.request_paint();
            }
            CraneCommand::GotoLineDefaultFirst => {
                let last_line = buffer.last_line();
                let line = match count {
                    Some(count) => match count {
                        _ if count > last_line => last_line,
                        _ => count,
                    },
                    None => 0,
                };
                self.move_to_line(mode, buffer, line);
            }
            CraneCommand::GotoLineDefaultLast => {
                let last_line = buffer.last_line();
                let line = match count {
                    Some(count) => match count {
                        _ if count > last_line => last_line,
                        _ => count,
                    },
                    None => last_line,
                };
                self.move_to_line(mode, buffer, line);
            }
            CraneCommand::WordFoward => {
                let new_offset = WordCursor::new(&buffer.rope, self.offset)
                    .next_boundary()
                    .unwrap();
                self.offset = new_offset;
                let (_, col) = buffer.offset_to_line_col(self.offset);
                self.horiz = col;
                self.request_paint();
            }
            CraneCommand::WordBackward => {
                let new_offset = WordCursor::new(&buffer.rope, self.offset)
                    .prev_boundary()
                    .unwrap();
                self.offset = new_offset;
                let (_, col) = buffer.offset_to_line_col(self.offset);
                self.horiz = col;
                self.request_paint();
            }
            CraneCommand::SplitHorizontal => {}
            CraneCommand::SplitRight => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitMove(
                        SplitMoveDirection::Right,
                        self.view_id,
                    ),
                    self.split_id,
                );
            }
            CraneCommand::SplitLeft => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitMove(
                        SplitMoveDirection::Left,
                        self.view_id,
                    ),
                    self.split_id,
                );
            }
            CraneCommand::SplitExchange => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitExchange(self.view_id),
                    self.split_id,
                );
            }
            CraneCommand::NewLineAbove => {}
            CraneCommand::NewLineBelow => {}
            _ => (),
        }

        let (line, col) = buffer.offset_to_line_col(self.offset);
        self.ensure_cursor_visible(buffer);
    }

    pub fn insert_new_line(&mut self, buffer: &mut Buffer, offset: usize) {
        let (line, col) = LogicalLines.offset_to_line_col(&buffer.rope, offset);
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
        buffer.rope.edit(Interval::new(offset, offset), &content);
        let new_offset = offset + content.len();
        self.offset = new_offset;
        self.ensure_cursor_visible(buffer);
        self.request_layout();
    }

    pub fn ensure_cursor_visible(&self, buffer: &Buffer) {
        let (line, col) = buffer.offset_to_line_col(self.offset);
        CRANE_STATE.submit_ui_command(
            CraneUICommand::EnsureVisible((
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
        CRANE_STATE
            .submit_ui_command(CraneUICommand::RequestLayout, self.view_id);
    }

    pub fn request_paint(&self) {
        CRANE_STATE
            .submit_ui_command(CraneUICommand::RequestPaint, self.view_id);
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
        }
    }

    pub fn set_widget_id(&mut self, widget_id: WidgetId) {
        self.widget_id = Some(widget_id);
    }

    pub fn set_active(&mut self, widget_id: WidgetId) {
        self.active = widget_id;
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
            active_editor.offset = 0;
            active_editor.buffer_id = Some(buffer_id);
            CRANE_STATE.submit_ui_command(
                CraneUICommand::ScrollTo((0.0, 0.0)),
                active_editor.view_id,
            );
            CRANE_STATE.submit_ui_command(
                CraneUICommand::RequestLayout,
                active_editor.view_id,
            );
        }
    }

    fn next_buffer_id(&mut self) -> BufferId {
        BufferId(self.id_counter.next())
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
                    let offset = editor.offset;
                    buffer.rope.edit(Interval::new(offset, offset), content);
                    editor.offset = editor.offset + 1;
                    let line = buffer.rope.line_of_offset(editor.offset);
                    let col = editor.offset - buffer.rope.offset_of_line(line);
                    editor.ensure_cursor_visible(buffer);
                    CRANE_STATE.submit_ui_command(
                        CraneUICommand::RequestPaint,
                        self.active,
                    );
                }
            }
        }
    }

    pub fn run_command(&mut self, count: Option<usize>, cmd: CraneCommand) {
        println!("run command {}", cmd);
        match cmd {
            CraneCommand::InsertMode => {
                self.mode = Mode::Insert;
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::RequestPaint,
                    self.active,
                );
            }
            CraneCommand::NormalMode => {
                self.mode = Mode::Normal;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.move_left(self.mode.clone(), 1, buffer);
                            CRANE_STATE.submit_ui_command(
                                CraneUICommand::RequestPaint,
                                self.active,
                            );
                        }
                    }
                }
            }
            CraneCommand::Append => {
                self.mode = Mode::Insert;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.move_right(self.mode.clone(), 1, buffer);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::AppendEndOfLine => {
                self.mode = Mode::Insert;
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.move_to_line_end(self.mode.clone(), buffer);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::NewLineAbove => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let line = buffer.line_of_offset(editor.offset);
                            let offset =
                                buffer.first_non_blank_character_on_line(line);
                            editor.insert_new_line(buffer, offset);
                            editor.offset = offset;
                            self.mode = Mode::Insert;
                            editor.request_paint();
                        }
                    }
                }
            }

            CraneCommand::NewLineBelow => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            self.mode = Mode::Insert;
                            let offset = editor
                                .line_end_offset(self.mode.clone(), buffer);
                            editor.insert_new_line(buffer, offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::InsertNewLine => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            self.mode = Mode::Insert;
                            editor.insert_new_line(buffer, editor.offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::DeleteWordBackward => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let new_offset =
                                WordCursor::new(&buffer.rope, editor.offset)
                                    .prev_boundary()
                                    .unwrap();
                            buffer.rope.edit(
                                Interval::new(new_offset, editor.offset),
                                "",
                            );
                            editor.offset = new_offset;
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::DeleteBackward => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            buffer.rope.edit(
                                Interval::new(editor.offset - 1, editor.offset),
                                "",
                            );
                            editor.offset = editor.offset - 1;
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
        CRANE_STATE.submit_ui_command(
            CraneUICommand::RequestPaint,
            self.widget_id.unwrap(),
        );
    }
}

pub struct EditorView<T> {
    view_id: WidgetId,
    pub editor_id: WidgetId,
    editor: WidgetPod<T, CraneScroll<T, Padding<T>>>,
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
        let scroll = CraneScroll::new(editor.padding((10.0, 0.0, 10.0, 0.0)));
        let editor_state =
            EditorState::new(editor_id, view_id, split_id, buffer_id);
        CRANE_STATE
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
                _ if cmd.is(CRANE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(CRANE_UI_COMMAND);
                    match command {
                        CraneUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        CraneUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        CraneUICommand::EnsureVisible((rect, margin)) => {
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
                        CraneUICommand::ScrollTo((x, y)) => {
                            self.editor.widget_mut().scroll_to(*x, *y);
                            return;
                        }
                        CraneUICommand::Scroll((x, y)) => {
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
        CRANE_STATE
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
            CRANE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &CRANE_STATE.editor_split.lock().unwrap().buffers;
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
        let line_height = env.get(CraneTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = {
            CRANE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
                .clone()
        };
        if let Some(buffer_id) = buffer_id {
            let mut editor_split = CRANE_STATE.editor_split.lock().unwrap();
            let mut layout = TextLayout::new("W");
            layout.set_font(CraneTheme::EDITOR_FONT);
            layout.rebuild_if_needed(&mut ctx.text(), env);
            let width = layout.point_for_text_position(1).x;
            let buffers = &editor_split.buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let (current_line, _) = {
                let editor = editor_split.editors.get(&self.view_id).unwrap();
                buffer.offset_to_line_col(editor.offset)
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
                        layout.set_font(CraneTheme::EDITOR_FONT);
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

pub struct Editor {
    text_layout: TextLayout,
    view_id: WidgetId,
    text_layouts: HashMap<usize, EditorTextLayout>,
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
                _ if cmd.is(CRANE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(CRANE_UI_COMMAND);
                    match command {
                        CraneUICommand::RequestLayout => {
                            println!("editor request layout");
                            ctx.request_layout();
                        }
                        CraneUICommand::RequestPaint => {
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
            CRANE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &CRANE_STATE.editor_split.lock().unwrap().buffers;
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
        let line_height = env.get(CraneTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = {
            CRANE_STATE
                .editor_split
                .lock()
                .unwrap()
                .get_buffer_id(&self.view_id)
                .clone()
        };
        if let Some(buffer_id) = buffer_id {
            let mut editor_split = CRANE_STATE.editor_split.lock().unwrap();

            let mut layout = TextLayout::new("W");
            layout.set_font(CraneTheme::EDITOR_FONT);
            layout.rebuild_if_needed(&mut ctx.text(), env);
            let width = layout.point_for_text_position(1).x;
            let editor = editor_split.get_editor(&self.view_id);
            editor.set_line_height(line_height);
            editor.char_width = width;

            let buffers = &editor_split.buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let (cursor, editor_width) = {
                let editor = editor_split.editors.get(&self.view_id).unwrap();
                (buffer.offset_to_line_col(editor.offset), editor.width)
            };
            let rects = ctx.region().rects().to_vec();
            for rect in rects {
                println!("print rect {:?} {:?}", self.view_id, rect);
                let start_line = (rect.y0 / line_height).floor() as usize;
                let num_lines = (rect.height() / line_height).floor() as usize;
                let last_line = buffer.last_line();
                for line in start_line..start_line + num_lines + 1 {
                    if line > last_line {
                        break;
                    }
                    let line_content = &buffer.rope.slice_to_cow(
                        buffer.offset_of_line(line)
                            ..buffer.offset_of_line(line + 1),
                    );
                    if line == cursor.0 {
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
                                CraneTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                            ),
                        );
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
                        match editor_split.get_mode() {
                            Mode::Insert => ctx.stroke(
                                Line::new(
                                    Point::new(
                                        cursor_x,
                                        cursor.0 as f64 * line_height,
                                    ),
                                    Point::new(
                                        cursor_x,
                                        (cursor.0 + 1) as f64 * line_height,
                                    ),
                                ),
                                &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
                                1.0,
                            ),
                            _ => ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(
                                        cursor_x,
                                        cursor.0 as f64 * line_height,
                                    ))
                                    .with_size(Size::new(width, line_height)),
                                &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
                            ),
                        };
                    }
                    let line_content = line_content.replace('\t', "    ");
                    if let Some(text_layout) = self.text_layouts.get_mut(&line)
                    {
                        if text_layout.text != line_content {
                            text_layout.layout.set_text(line_content.clone());
                            text_layout.text = line_content;
                            text_layout
                                .layout
                                .rebuild_if_needed(&mut ctx.text(), env);
                        }
                        text_layout.layout.draw(
                            ctx,
                            Point::new(0.0, line_height * line as f64),
                        );
                    } else {
                        let mut layout = TextLayout::new(line_content.clone());
                        layout.set_font(CraneTheme::EDITOR_FONT);
                        layout.rebuild_if_needed(&mut ctx.text(), env);
                        layout.draw(
                            ctx,
                            Point::new(0.0, line_height * line as f64),
                        );
                        let text_layout = EditorTextLayout {
                            layout,
                            text: line_content,
                        };
                        self.text_layouts.insert(line, text_layout);
                    }
                }
                // for (i, line) in buffer
                //     .rope
                //     .lines(
                //         buffer.rope.offset_of_line(start_line)
                //             ..buffer.rope.offset_of_line(
                //                 (start_line + num_lines + 1)
                //                     .min(buffer.num_lines()),
                //             ),
                //     )
                //     .enumerate()
                // {
                //     println!("{} {}", i, line);
                //     if i + start_line == cursor.0 {
                //         ctx.fill(
                //             Rect::ZERO
                //                 .with_origin(Point::new(
                //                     0.0,
                //                     cursor.0 as f64 * line_height,
                //                 ))
                //                 .with_size(Size::new(
                //                     editor_width,
                //                     line_height,
                //                 )),
                //             &env.get(
                //                 CraneTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                //             ),
                //         );
                //         let cursor_x = (line[..cursor.1]
                //             .chars()
                //             .filter_map(|c| {
                //                 if c == '\t' {
                //                     Some('\t')
                //                 } else {
                //                     None
                //                 }
                //             })
                //             .count()
                //             * 3
                //             + cursor.1)
                //             as f64
                //             * width;
                //         match editor_split.get_mode() {
                //             Mode::Insert => ctx.stroke(
                //                 Line::new(
                //                     Point::new(
                //                         cursor_x,
                //                         cursor.0 as f64 * line_height,
                //                     ),
                //                     Point::new(
                //                         cursor_x,
                //                         (cursor.0 + 1) as f64 * line_height,
                //                     ),
                //                 ),
                //                 &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
                //                 1.0,
                //             ),
                //             _ => ctx.fill(
                //                 Rect::ZERO
                //                     .with_origin(Point::new(
                //                         cursor_x,
                //                         cursor.0 as f64 * line_height,
                //                     ))
                //                     .with_size(Size::new(width, line_height)),
                //                 &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
                //             ),
                //         };
                //     }
                //     let mut layout =
                //         TextLayout::new(line.replace('\t', "    "));
                //     layout.set_font(CraneTheme::EDITOR_FONT);
                //     layout.rebuild_if_needed(&mut ctx.text(), env);
                //     layout.draw(
                //         ctx,
                //         Point::new(0.0, line_height * (i + start_line) as f64),
                //     );
                // }
            }
        }
    }
}
