use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    container::LapceContainer,
    explorer::FileExplorer,
    split::LapceSplit,
    state::{LapceUIState, LAPCE_STATE},
    status::LapceStatus,
    theme::LapceTheme,
};
use druid::{
    widget::IdentityWrapper, widget::WidgetExt, BoxConstraints, Event, Point, Rect,
    RenderContext, Size, Widget, WidgetId, WidgetPod,
};

pub struct LapceWindow {
    main_split: WidgetPod<LapceUIState, LapceSplit>,
    status: WidgetPod<LapceUIState, LapceStatus>,
}

impl LapceWindow {
    pub fn new() -> IdentityWrapper<LapceWindow> {
        let container_id = WidgetId::next();
        let container = LapceContainer::new().with_id(container_id.clone());
        let main_split = LapceSplit::new(true)
            .with_child(FileExplorer::new(), 300.0)
            .with_flex_child(container, 1.0);
        let status = LapceStatus::new();
        LapceWindow {
            main_split: WidgetPod::new(main_split),
            status: WidgetPod::new(status),
        }
        .with_id(LAPCE_STATE.window_id)
    }
}

impl Widget<LapceUIState> for LapceWindow {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut LapceUIState,
        env: &druid::Env,
    ) {
        match event {
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
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
        self.main_split.event(ctx, event, data, env);
        self.status.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        self.main_split.lifecycle(ctx, event, data, env);
        self.status.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        self.main_split.update(ctx, data, env);
        self.status.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceUIState,
        env: &druid::Env,
    ) -> druid::Size {
        let self_size = bc.max();
        let status_size = self.status.layout(ctx, bc, data, env);
        let main_split_size =
            Size::new(self_size.width, self_size.height - status_size.height);
        let main_split_bc = BoxConstraints::new(Size::ZERO, main_split_size);
        self.main_split.layout(ctx, &main_split_bc, data, env);
        self.main_split
            .set_layout_rect(ctx, data, env, main_split_size.to_rect());
        self.status.set_layout_rect(
            ctx,
            data,
            env,
            Rect::from_origin_size(
                Point::new(0.0, main_split_size.height),
                status_size,
            ),
        );
        self_size
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            ctx.fill(rect, &env.get(LapceTheme::EDITOR_BACKGROUND));
        }
        self.main_split.paint(ctx, data, env);
        self.status.paint(ctx, data, env);
    }
}
