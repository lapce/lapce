use std::sync::Arc;

use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Size, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_proxy::terminal::TermId;

use crate::{data::LapceTabData, proxy::LapceProxy, split::LapceSplitNew};

pub struct TerminalSplitData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub terminals: im::HashMap<TermId, LapceTerminalData>,
}

impl TerminalSplitData {
    pub fn new() -> Self {
        let split_id = WidgetId::next();
        let mut terminals = im::HashMap::new();

        Self {
            widget_id: WidgetId::next(),
            split_id,
            terminals,
        }
    }
}

pub struct LapceTerminalData {
    id: TermId,
}

impl LapceTerminalData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let id = TermId::next();
        proxy.new_terminal(id);
        Self { id }
    }
}

pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let split = LapceSplitNew::new(data.terminal.split_id);
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
        self.split.paint(ctx, data, env);
    }
}
