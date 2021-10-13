use std::sync::Arc;

use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use lapce_proxy::terminal::TermId;

use crate::{
    config::LapceTheme,
    data::LapceTabData,
    keypress::KeyPressFocus,
    proxy::{LapceProxy, TerminalContent},
    split::LapceSplitNew,
    state::Mode,
};

#[derive(Clone)]
pub struct TerminalSplitData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub terminals: im::HashMap<TermId, Arc<LapceTerminalData>>,
}

impl TerminalSplitData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let split_id = WidgetId::next();
        let mut terminals = im::HashMap::new();

        let terminal = Arc::new(LapceTerminalData::new(proxy));
        terminals.insert(terminal.id, terminal);

        Self {
            widget_id: WidgetId::next(),
            split_id,
            terminals,
        }
    }
}

pub struct LapceTerminalViewData {
    terminal: Arc<LapceTerminalData>,
    proxy: Arc<LapceProxy>,
}

impl KeyPressFocus for LapceTerminalViewData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        false
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        self.proxy.terminal_insert(self.terminal.id, c);
    }
}

#[derive(Clone)]
pub struct LapceTerminalData {
    id: TermId,
    pub content: TerminalContent,
}

impl LapceTerminalData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let id = TermId::next();
        std::thread::spawn(move || {
            proxy.new_terminal(id, 50, 20);
        });
        Self {
            id,
            content: TerminalContent::new(),
        }
    }
}

pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let (term_id, _) = data.terminal.terminals.iter().next().unwrap();
        let terminal = LapceTerminal::new(*term_id);
        let split = LapceSplitNew::new(data.terminal.split_id).with_flex_child(
            terminal.boxed(),
            None,
            1.0,
        );
        Self {
            widget_id: data.terminal.widget_id,
            split: WidgetPod::new(split),
        }
    }
}

impl Widget<LapceTabData> for TerminalPanel {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if !data.terminal.same(&old_data.terminal) {
            ctx.request_paint();
        }
        self.split.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.split.layout(ctx, bc, data, env);
        self.split.set_origin(ctx, data, env, Point::ZERO);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(
            rect,
            5.0,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        self.split.paint(ctx, data, env);
    }
}

pub struct LapceTerminal {
    term_id: TermId,
    width: f64,
    height: f64,
}

impl LapceTerminal {
    pub fn new(term_id: TermId) -> Self {
        Self {
            term_id,
            width: 0.0,
            height: 0.0,
        }
    }
}

impl Widget<LapceTabData> for LapceTerminal {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        let old_terminal_data =
            data.terminal.terminals.get(&self.term_id).unwrap().clone();
        let mut term_data = LapceTerminalViewData {
            terminal: old_terminal_data.clone(),
            proxy: data.proxy.clone(),
        };
        match event {
            Event::MouseDown(mouse_event) => {
                ctx.request_focus();
            }
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    &mut term_data,
                    env,
                );
                data.keypress = keypress.clone();
            }
            _ => (),
        }
        if !term_data.terminal.same(&old_terminal_data) {
            Arc::make_mut(&mut data.terminal)
                .terminals
                .insert(term_data.terminal.id, term_data.terminal.clone());
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = bc.max();
        if self.width != size.width || self.height != size.height {
            self.width = size.width;
            self.height = size.height;
            let width = data.config.editor_text_width(ctx.text(), "W");
            let line_height = data.config.editor.line_height as f64;
            let width = (self.width / width).ceil() as usize;
            let height = (self.height / line_height).ceil() as usize;
            data.proxy.terminal_resize(self.term_id, width, height);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let width = data.config.editor_text_width(ctx.text(), "W");
        let line_height = data.config.editor.line_height as f64;

        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
        for (p, cell) in terminal.content.iter() {
            let x = p.column.0 as f64 * width;
            let y = p.line.0 as f64 * line_height;
            let text_layout = ctx
                .text()
                .new_text_layout(cell.c.to_string())
                .font(
                    data.config.editor.font_family(),
                    data.config.editor.font_size as f64,
                )
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, Point::new(x, y));
        }
    }
}
