use crate::svg::get_svg;
use druid::{
    kurbo::Line, BoxConstraints, Command, Cursor, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target,
    UpdateCtx, Widget,
};
use lapce_data::{
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand, LAPCE_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    panel::PanelPosition,
};
use serde_json::json;

pub struct ActivityBar {}

impl ActivityBar {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ActivityBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for ActivityBar {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        let width = data.config.ui.activity_width() as f64;
        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    let index = (mouse.pos.y / width) as usize;
                    if let Some(panel) = data.panels.get_mut(&PanelPosition::LeftTop)
                    {
                        if let Some(kind) = panel.widgets.get(index) {
                            if panel.active == *kind {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Workbench(
                                            LapceWorkbenchCommand::TogglePanelVisual,
                                        ),
                                        name: None,
                                        data: Some(json!(kind)),
                                    },
                                    Target::Widget(data.id),
                                ));
                            } else {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Workbench(
                                            LapceWorkbenchCommand::ShowPanel,
                                        ),
                                        name: None,
                                        data: Some(json!(kind)),
                                    },
                                    Target::Widget(data.id),
                                ));
                            }
                        }
                    }
                }
            }
            Event::MouseMove(mouse) => {
                let n = data
                    .panels
                    .get(&PanelPosition::LeftTop)
                    .map(|panel| panel.widgets.len())
                    .unwrap_or(0);
                if n > 0 && mouse.pos.y < width * n as f64 {
                    ctx.set_cursor(&Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        Size::new(data.config.ui.activity_width() as f64, bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();

        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
        } else {
            ctx.stroke(
                Line::new(
                    Point::new(rect.x1 + 0.5, rect.y0),
                    Point::new(rect.x1 + 0.5, rect.y1),
                ),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::ACTIVITY_BACKGROUND),
        );

        let mut offset = 0.0;
        let svg_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();
        if let Some(panel) = data.panels.get(&PanelPosition::LeftTop) {
            for kind in panel.widgets.iter() {
                let svg = get_svg(kind.svg_name()).unwrap();
                if &panel.active == kind && panel.shown {
                    ctx.fill(
                        Size::new(size.width, size.width)
                            .to_rect()
                            .with_origin(Point::new(0.0, offset)),
                        data.config
                            .get_color_unchecked(LapceTheme::ACTIVITY_CURRENT),
                    );
                }
                let svg_size = 25.0;
                let rect =
                    Size::new(svg_size, svg_size)
                        .to_rect()
                        .with_origin(Point::new(
                            (size.width - svg_size) / 2.0,
                            (size.width - svg_size) / 2.0 + offset,
                        ));
                ctx.draw_svg(&svg, rect, Some(&svg_color));
                offset += size.width;
            }
        }
    }
}
