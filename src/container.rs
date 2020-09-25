use crate::{
    command::{LapceCommand, LAPCE_COMMAND},
    editor::Editor,
    editor::EditorState,
    editor::EditorView,
    palette::PaletteWrapper,
};
use crate::{palette::Palette, split::LapceSplit};
use crate::{scroll::LapceScroll, state::LAPCE_STATE};
use druid::{
    kurbo::{Line, Rect},
    widget::Container,
    widget::Flex,
    widget::IdentityWrapper,
    widget::Label,
    widget::SizedBox,
    Command, MouseEvent, Selector, Target, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};

pub struct ChildState {
    pub origin: Option<Point>,
    pub size: Option<Size>,
    pub hidden: bool,
}

pub struct LapceContainer<T> {
    palette_max_size: Size,
    palette_rect: Rect,
    palette: WidgetPod<T, Box<dyn Widget<T>>>,
    editor_split: WidgetPod<T, Box<dyn Widget<T>>>,
}

impl<T: Data> LapceContainer<T> {
    pub fn new() -> Self {
        let palette = PaletteWrapper::new();
        let palette_id = WidgetId::next();
        let palette =
            WidgetPod::new(IdentityWrapper::wrap(palette, palette_id)).boxed();
        LAPCE_STATE
            .palette
            .lock()
            .unwrap()
            .set_widget_id(palette_id);

        let editor_split_id = WidgetId::next();
        LAPCE_STATE
            .editor_split
            .lock()
            .unwrap()
            .set_widget_id(editor_split_id);
        let editor_view = EditorView::new(editor_split_id, None);
        LAPCE_STATE
            .editor_split
            .lock()
            .unwrap()
            .set_active(editor_view.id().unwrap());
        let editor_split = WidgetPod::new(IdentityWrapper::wrap(
            LapceSplit::new(true).with_child(editor_view),
            editor_split_id,
        ))
        .boxed();

        LapceContainer {
            palette_max_size: Size::new(600.0, 400.0),
            palette_rect: Rect::ZERO
                .with_origin(Point::new(200.0, 100.0))
                .with_size(Size::new(600.0, 400.0)),
            palette,
            editor_split,
        }
    }
}

impl<T: Data> Widget<T> for LapceContainer<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        ctx.request_focus();
        match event {
            Event::Internal(_) => {
                self.palette.event(ctx, event, data, env);
                self.editor_split.event(ctx, event, data, env);
            }
            Event::KeyDown(key_event) => LAPCE_STATE.key_down(key_event),
            Event::Command(cmd) => {
                match cmd {
                    _ if cmd.is(LAPCE_COMMAND) => {
                        let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                        match cmd {
                            LapceCommand::Palette => (),
                            _ => (),
                        };
                        self.palette.event(ctx, event, data, env)
                    }
                    _ => (),
                }
                return;
            }
            _ => (),
        }

        match event {
            Event::MouseDown(mouse)
            | Event::MouseUp(mouse)
            | Event::MouseMove(mouse)
            | Event::Wheel(mouse) => {
                if !LAPCE_STATE.palette.lock().unwrap().hidden
                    && self.palette_rect.contains(mouse.pos)
                {
                    self.palette.event(ctx, event, data, env);
                    return;
                } else {
                    self.editor_split.event(ctx, event, data, env);
                }
            }
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
        self.palette.lifecycle(ctx, event, data, env);
        self.editor_split.lifecycle(ctx, event, data, env);
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
        let size = bc.max();

        let palette_bc = BoxConstraints::new(Size::ZERO, self.palette_max_size);
        let palette_size = self.palette.layout(ctx, &palette_bc, data, env);
        self.palette_rect = Rect::ZERO
            .with_origin(Point::new(
                (size.width - self.palette_max_size.width) / 2.0,
                ((size.height - self.palette_max_size.height) / 4.0).max(0.0),
            ))
            .with_size(palette_size);
        println!("palette_size {:?}", palette_size);
        self.palette
            .set_layout_rect(ctx, data, env, self.palette_rect);

        self.editor_split.layout(ctx, bc, data, env);
        self.editor_split.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(size),
        );
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        println!("container paint {:?}", ctx.region().rects());
        self.editor_split.paint(ctx, data, env);
        self.palette.paint(ctx, data, env);
    }
}
