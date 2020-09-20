use crate::{
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
    kurbo::Line, theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx,
    ExtEventSink, Key, KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers,
    PaintCtx, Point, Rect, RenderContext, Selector, Size, Target, TextLayout,
    UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lazy_static::lazy_static;
use std::time::Duration;
use std::{any::Any, thread};
use std::{collections::HashMap, sync::Arc, sync::Mutex};

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
    line_height: f64,
    width: f64,
}

impl EditorState {
    pub fn new(
        id: WidgetId,
        // scroll_id: WidgetId,
        split_id: WidgetId,
    ) -> EditorState {
        EditorState {
            id,
            // scroll_id,
            split_id,
            buffer_id: None,
            cursor: (0, 0),
            line_height: 0.0,
            width: 0.0,
        }
    }

    pub fn run_command(&mut self, cmd: CraneCommand) {
        match cmd {
            CraneCommand::Left => {
                self.cursor.1 -= 1;
            }
            CraneCommand::Right => {
                self.cursor.1 += 1;
            }
            CraneCommand::Up => {
                if self.cursor.0 > 0 {
                    self.cursor.0 -= 1;
                }
            }
            CraneCommand::Down => {
                self.cursor.0 += 1;
            }
            CraneCommand::SplitVertical => {
                CRANE_STATE.submit_ui_command(
                    CraneUICommand::Split(true, self.id),
                    self.split_id,
                );
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
            _ => (),
        }
        CRANE_STATE.submit_ui_command(
            CraneUICommand::EnsureVisible((
                Rect::ZERO
                    .with_origin(Point::new(
                        self.cursor.1 as f64 * self.width,
                        self.cursor.0 as f64 * self.line_height,
                    ))
                    .with_size(Size::new(self.width, self.line_height)),
                (self.width, self.line_height),
            )),
            self.id,
        );
        CRANE_STATE.submit_ui_command(CraneUICommand::RequestPaint, self.id);
    }

    pub fn set_line_height(&mut self, line_height: f64) {
        self.line_height = line_height;
    }

    pub fn set_width(&mut self, width: f64) {
        self.width = width;
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

    pub fn run_command(&mut self, cmd: CraneCommand) {
        println!("run command {}", cmd);
        match cmd {
            _ => {
                self.get_active_editor().map(|e| e.run_command(cmd));
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
        let cursor = {
            let mut state = CRANE_STATE.editor_split.lock().unwrap();
            let eidtor = state.get_editor(&self.widget_id);
            eidtor.cursor
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
                                    rect.x0,
                                    cursor.0 as f64 * line_height,
                                ))
                                .with_size(Size::new(
                                    rect.width(),
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
            let eidtor = state.get_editor(&self.widget_id);
            eidtor.set_line_height(line_height);
            eidtor.set_width(width);
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
