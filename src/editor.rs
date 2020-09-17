use crate::{
    buffer::{Buffer, BufferId},
    command::CraneUICommand,
    command::CRANE_UI_COMMAND,
    container::CraneContainer,
    state::CRANE_STATE,
    theme::CraneTheme,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, ExtEventSink,
    Key, KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx,
    Point, RenderContext, Selector, Size, Target, TextLayout, UpdateCtx,
    Widget, WidgetId, WidgetPod,
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
    scroll_id: WidgetId,
    buffer_id: Option<BufferId>,
}

impl EditorState {
    pub fn new(id: WidgetId, scroll_id: WidgetId) -> EditorState {
        EditorState {
            id,
            scroll_id,
            buffer_id: None,
        }
    }
}

pub struct EditorSplitState {
    active: WidgetId,
    pub editors: HashMap<WidgetId, EditorState>,
    buffers: HashMap<BufferId, Buffer>,
    open_files: HashMap<String, BufferId>,
    id_counter: Counter,
}

impl EditorSplitState {
    pub fn new() -> EditorSplitState {
        EditorSplitState {
            active: WidgetId::next(),
            editors: HashMap::new(),
            id_counter: Counter::default(),
            buffers: HashMap::new(),
            open_files: HashMap::new(),
        }
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
            println!("submit ui scroll request layout");
            CRANE_STATE.submit_ui_command(
                CraneUICommand::RequestLayout,
                active_editor.scroll_id,
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
        if let Some(buffer_id) = buffer_id {
            let buffers = &CRANE_STATE.editor_split.lock().unwrap().buffers;
            let buffer = buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            let rects = ctx.region().rects().to_vec();
            for rect in rects {
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
                    let mut layout = TextLayout::new(line);
                    layout.set_font(CraneTheme::EDITOR_FONT);
                    layout.rebuild_if_needed(&mut ctx.text(), env);
                    layout.draw(
                        ctx,
                        Point::new(0.0, line_height * (i + start_line) as f64),
                    );
                }
            }
        }
    }
}
