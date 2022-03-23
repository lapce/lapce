use druid::{
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{CommandTarget, LapceCommand, LapceCommandNew, LAPCE_NEW_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
};

use crate::{editor::LapceEditorView, svg::get_svg, tab::LapceIcon};

pub struct FindBox {
    input_width: f64,
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
}

impl FindBox {
    pub fn new(view_id: WidgetId, parent_view_id: WidgetId) -> Self {
        let input = LapceEditorView::new(view_id, None)
            .hide_header()
            .hide_gutter()
            .padding((10.0, 10.0));
        let icons = vec![
            LapceIcon {
                icon: "arrow-up.svg".to_string(),
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::SearchBackward.to_string(),
                        data: None,
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: "arrow-down.svg".to_string(),
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::SearchForward.to_string(),
                        data: None,
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: "close.svg".to_string(),
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::ClearSearch.to_string(),
                        data: None,
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
        ];
        Self {
            input_width: 200.0,
            input: WidgetPod::new(input.boxed()),
            icons,
            mouse_pos: Point::ZERO,
        }
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }
}

impl Widget<LapceTabData> for FindBox {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.input.event(ctx, event, data, env);
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let bc = BoxConstraints::tight(Size::new(self.input_width, bc.max().height));
        let input_size = self.input.layout(ctx, &bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);
        let height = input_size.height;
        let width = self.input_width + height * 3.0;

        for (i, icon) in self.icons.iter_mut().enumerate() {
            icon.rect = Size::new(height, height)
                .to_rect()
                .with_origin(Point::new(self.input_width + i as f64 * height, 0.0))
                .inflate(-5.0, -5.0);
        }

        Size::new(width, height)
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.update(ctx, data, env);
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !data.find.visual {
            return;
        }
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.clip(rect.inset((100.0, 0.0, 100.0, 100.0)));
            let shadow_width = 5.0;
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
        });
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        self.input.paint(ctx, data, env);

        for icon in self.icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    &icon.rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }

            let svg = get_svg(&icon.icon).unwrap();
            ctx.draw_svg(
                &svg,
                icon.rect.inflate(-7.0, -7.0),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }
    }
}
