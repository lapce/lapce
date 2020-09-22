use crate::{
    buffer::WordCursor,
    buffer::{Buffer, BufferId},
    command::CraneCommand,
    command::CraneUICommand,
    command::CRANE_UI_COMMAND,
    container::CraneContainer,
    split::SplitMoveDirection,
    state::Mode,
    state::CRANE_STATE,
    theme::CraneTheme,
};
use druid::{
    kurbo::Line, theme, BoxConstraints, Data, Env, Event, EventCtx,
    ExtEventSink, Key, KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers,
    PaintCtx, Point, Rect, RenderContext, Selector, Size, Target, TextLayout,
    UpdateCtx, Widget, WidgetId, WidgetPod,
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
    id: WidgetId,
    // scroll_id: WidgetId,
    pub split_id: WidgetId,
    pub buffer_id: Option<BufferId>,
    cursor: (usize, usize),
    offset: usize,
    pub line_height: f64,
    pub char_width: f64,
    pub width: f64,
    pub height: f64,
}

impl EditorState {
    pub fn new(id: WidgetId, split_id: WidgetId) -> EditorState {
        EditorState {
            id,
            split_id,
            buffer_id: None,
            cursor: (0, 0),
            offset: 0,
            line_height: 0.0,
            char_width: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }

    pub fn run_command(&mut self, buffer: &mut Buffer, cmd: CraneCommand) {
        match cmd {
            CraneCommand::Left => {
                self.cursor.1 -= 1;
                self.offset -= 1;
                CRANE_STATE
                    .submit_ui_command(CraneUICommand::RequestPaint, self.id);
            }
            CraneCommand::Right => {
                self.cursor.1 += 1;
                self.offset += 1;
                CRANE_STATE
                    .submit_ui_command(CraneUICommand::RequestPaint, self.id);
            }
            CraneCommand::Up => {
                if self.cursor.0 > 0 {
                    self.cursor.0 -= 1;
                    self.offset = buffer.rope.offset_of_line(self.cursor.0)
                        + self.cursor.1;
                }
                CRANE_STATE
                    .submit_ui_command(CraneUICommand::RequestPaint, self.id);
            }
            CraneCommand::Down => {
                self.cursor.0 += 1;
                self.offset =
                    buffer.rope.offset_of_line(self.cursor.0) + self.cursor.1;
                CRANE_STATE
                    .submit_ui_command(CraneUICommand::RequestPaint, self.id);
            }
            CraneCommand::SplitVertical => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Split(true, self.id),
                    self.split_id,
                );
            }
            CraneCommand::ScrollUp => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((0.0, -self.line_height)),
                    self.id,
                );
            }
            CraneCommand::ScrollDown => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Scroll((0.0, self.line_height)),
                    self.id,
                );
            }
            CraneCommand::FirstLine => {
                self.cursor.0 = 0;
                self.offset = self.cursor.1;
                self.request_paint();
            }
            CraneCommand::LastLine => {
                self.cursor.0 = buffer.line_of_offset(buffer.rope.len());
                self.offset =
                    buffer.offset_of_line(self.cursor.0) + self.cursor.1;
                self.request_paint();
            }
            CraneCommand::WordFoward => {
                let new_offset = WordCursor::new(&buffer.rope, self.offset)
                    .next_boundary()
                    .unwrap();
                self.offset = new_offset;
                self.cursor = buffer.offset_to_line_col(self.offset);
                self.request_paint();
            }
            CraneCommand::WordBackward => {
                let new_offset = WordCursor::new(&buffer.rope, self.offset)
                    .prev_boundary()
                    .unwrap();
                self.offset = new_offset;
                self.cursor = buffer.offset_to_line_col(self.offset);
                self.request_paint();
            }
            CraneCommand::SplitHorizontal => {}
            CraneCommand::SplitRight => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitMove(
                        SplitMoveDirection::Right,
                        self.id,
                    ),
                    self.split_id,
                );
            }
            CraneCommand::SplitLeft => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitMove(
                        SplitMoveDirection::Left,
                        self.id,
                    ),
                    self.split_id,
                );
            }
            CraneCommand::SplitExchange => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::SplitExchange(self.id),
                    self.split_id,
                );
            }
            CraneCommand::NewLineAbove => {}
            CraneCommand::NewLineBelow => {}
            _ => (),
        }

        CRANE_STATE.submit_ui_command(
            CraneUICommand::EnsureVisible((
                Rect::ZERO
                    .with_origin(Point::new(
                        self.cursor.1 as f64 * self.char_width,
                        self.cursor.0 as f64 * self.line_height,
                    ))
                    .with_size(Size::new(self.char_width, self.line_height)),
                (self.char_width, self.line_height),
            )),
            self.id,
        );
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
        self.cursor = LogicalLines.offset_to_line_col(&buffer.rope, new_offset);
    }

    pub fn request_paint(&self) {
        CRANE_STATE.submit_ui_command(CraneUICommand::RequestPaint, self.id);
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
            active_editor.buffer_id = Some(buffer_id);
            CRANE_STATE.submit_ui_command(
                CraneUICommand::ScrollTo((0.0, 0.0)),
                active_editor.id,
            );
            CRANE_STATE.submit_ui_command(
                CraneUICommand::RequestLayout,
                active_editor.id,
            );
        }
    }

    fn next_buffer_id(&mut self) -> BufferId {
        BufferId(self.id_counter.next())
    }

    pub fn get_buffer_id(
        &self,
        editor_widget_id: &WidgetId,
    ) -> Option<BufferId> {
        self.editors
            .get(editor_widget_id)
            .map(|e| e.buffer_id.clone())
            .unwrap()
    }

    fn get_editor(&mut self, widget_id: &WidgetId) -> &mut EditorState {
        self.editors.get_mut(widget_id).unwrap()
    }

    fn get_active_editor(&mut self) -> Option<&mut EditorState> {
        self.editors.get_mut(&self.active)
    }

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
                    editor.cursor = (line, col);
                    CRANE_STATE.submit_ui_command(
                        CraneUICommand::RequestPaint,
                        self.active,
                    );
                }
            }
        }
    }

    pub fn run_command(&mut self, cmd: CraneCommand) {
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
                    if editor.cursor.1 > 0 {
                        editor.cursor.1 = editor.cursor.1 - 1;
                        editor.offset = editor.offset - 1;
                    }
                }
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::RequestPaint,
                    self.active,
                );
            }
            CraneCommand::PageDown => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let lines =
                                (editor.height / editor.line_height / 2.0)
                                    .floor()
                                    as usize;
                            editor.cursor.0 = editor.cursor.0 + lines;
                            editor.offset = buffer
                                .offset_of_line(editor.cursor.0)
                                + editor.cursor.1;
                            CRANE_STATE.submit_ui_command(
                                CraneUICommand::Scroll((
                                    0.0,
                                    editor.line_height * lines as f64,
                                )),
                                editor.id,
                            );
                        }
                    }
                }
            }
            CraneCommand::PageUp => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let lines =
                                (editor.height / editor.line_height / 2.0)
                                    .floor()
                                    as usize;
                            let line = if editor.cursor.0 < lines {
                                0
                            } else {
                                editor.cursor.0 - lines
                            };
                            editor.cursor.0 = line;
                            editor.offset = buffer
                                .offset_of_line(editor.cursor.0)
                                + editor.cursor.1;
                            CRANE_STATE.submit_ui_command(
                                CraneUICommand::Scroll((
                                    0.0,
                                    -editor.line_height * lines as f64,
                                )),
                                editor.id,
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
                            editor.cursor.1 += 1;
                            editor.offset += 1;
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
                            let new_offset =
                                buffer.offset_of_line(editor.cursor.0 + 1) - 1;
                            editor.offset = new_offset;
                            editor.cursor =
                                buffer.offset_to_line_col(editor.offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::LineEnd => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            let new_offset =
                                buffer.offset_of_line(editor.cursor.0 + 1) - 2;
                            editor.offset = new_offset;
                            editor.cursor =
                                buffer.offset_to_line_col(editor.offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::LineStart => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.cursor.1 = 0;
                            let new_offset =
                                buffer.offset_of_line(editor.cursor.0);
                            editor.offset = new_offset;
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
                            editor.cursor = LogicalLines
                                .offset_to_line_col(&buffer.rope, offset);
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
                            let offset = LogicalLines.line_col_to_offset(
                                &buffer.rope,
                                editor.cursor.0 + 1,
                                0,
                            ) - 1;
                            editor.insert_new_line(buffer, offset);
                            self.mode = Mode::Insert;
                            editor.request_paint();
                        }
                    }
                }
            }
            CraneCommand::InsertNewLine => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.insert_new_line(buffer, editor.offset);
                            self.mode = Mode::Insert;
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
                            editor.cursor =
                                buffer.offset_to_line_col(editor.offset);
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
                            editor.cursor =
                                buffer.offset_to_line_col(editor.offset);
                            editor.request_paint();
                        }
                    }
                }
            }
            _ => {
                if let Some(editor) = self.editors.get_mut(&self.active) {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
                            editor.run_command(buffer, cmd);
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

pub struct Editor {
    text_layout: TextLayout,
    widget_id: WidgetId,
}

impl Editor {
    pub fn new(widget_id: WidgetId) -> Self {
        let text_layout = TextLayout::new("");
        Editor {
            text_layout,
            widget_id,
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
                .get_buffer_id(&self.widget_id)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &CRANE_STATE.editor_split.lock().unwrap().buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            Size::new(
                width * buffer.max_line_len as f64,
                25.0 * buffer.num_lines as f64,
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
                .get_buffer_id(&self.widget_id)
        };
        let (cursor, editor_width) = {
            let mut state = CRANE_STATE.editor_split.lock().unwrap();
            let editor = state.get_editor(&self.widget_id);
            (editor.cursor, editor.width)
        };
        if let Some(buffer_id) = buffer_id {
            let buffers = &CRANE_STATE.editor_split.lock().unwrap().buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let rects = ctx.region().rects().to_vec();
            for rect in rects {
                println!("print rect {:?} {:?}", self.widget_id, rect);
                let start_line = (rect.y0 / line_height).floor() as usize;
                let num_lines = (rect.height() / line_height).floor() as usize;
                for (i, line) in buffer
                    .rope
                    .lines_raw(
                        buffer.rope.offset_of_line(start_line)
                            ..buffer.rope.offset_of_line(
                                (start_line + num_lines + 1)
                                    .min(buffer.num_lines),
                            ),
                    )
                    .enumerate()
                {
                    if i + start_line == cursor.0 {
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
                        )
                    }
                    let mut layout =
                        TextLayout::new(line.replace('\t', "    "));
                    layout.set_font(CraneTheme::EDITOR_FONT);
                    layout.rebuild_if_needed(&mut ctx.text(), env);
                    layout.draw(
                        ctx,
                        Point::new(0.0, line_height * (i + start_line) as f64),
                    );
                }
            }
        }

        let mut layout = TextLayout::new("W");
        layout.set_font(CraneTheme::EDITOR_FONT);
        layout.rebuild_if_needed(&mut ctx.text(), env);
        let width = layout.point_for_text_position(1).x;
        let mode = { CRANE_STATE.editor_split.lock().unwrap().get_mode() };
        {
            let mut state = CRANE_STATE.editor_split.lock().unwrap();
            let editor = state.get_editor(&self.widget_id);
            editor.set_line_height(line_height);
            editor.char_width = width;
        };
        match mode {
            Mode::Insert => ctx.stroke(
                Line::new(
                    Point::new(
                        cursor.1 as f64 * width,
                        cursor.0 as f64 * line_height,
                    ),
                    Point::new(
                        cursor.1 as f64 * width,
                        (cursor.0 + 1) as f64 * line_height,
                    ),
                ),
                &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
                1.0,
            ),
            _ => ctx.fill(
                Rect::ZERO
                    .with_origin(Point::new(
                        cursor.1 as f64 * width,
                        cursor.0 as f64 * line_height,
                    ))
                    .with_size(Size::new(width, line_height)),
                &env.get(CraneTheme::EDITOR_CURSOR_COLOR),
            ),
        };
    }
}
