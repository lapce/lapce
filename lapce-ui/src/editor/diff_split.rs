use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Size, UpdateCtx, Widget, WidgetPod,
};
use lapce_data::data::LapceTabData;

use crate::editor::container::LapceEditorContainer;

pub struct EditorDiffSplit {
    left: WidgetPod<LapceTabData, LapceEditorContainer>,
    right: WidgetPod<LapceTabData, LapceEditorContainer>,
}

impl Widget<LapceTabData> for EditorDiffSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.left.event(ctx, event, data, env);
        self.right.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.lifecycle(ctx, event, data, env);
        self.right.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.update(ctx, data, env);
        self.right.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.left.layout(ctx, bc, data, env);
        self.right.layout(ctx, bc, data, env);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.left.paint(ctx, data, env);
        self.right.paint(ctx, data, env);
    }
}
