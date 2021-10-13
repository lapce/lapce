use std::sync::Arc;

use druid::{
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use lapce_proxy::terminal::TermId;

use crate::{
    config::LapceTheme,
    data::LapceTabData,
    proxy::{LapceProxy, TerminalContent},
    split::LapceSplitNew,
};

#[derive(Clone)]
pub struct TerminalSplitData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub terminals: im::HashMap<TermId, LapceTerminalData>,
}

impl TerminalSplitData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let split_id = WidgetId::next();
        let mut terminals = im::HashMap::new();

        let terminal = LapceTerminalData::new(proxy);
        terminals.insert(terminal.id, terminal);

        Self {
            widget_id: WidgetId::next(),
            split_id,
            terminals,
        }
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
            proxy.new_terminal(id);
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
}

impl LapceTerminal {
    pub fn new(term_id: TermId) -> Self {
        Self { term_id }
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
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
        println!("{:?}", terminal.content);
    }
}
