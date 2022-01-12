use std::sync::Arc;

use druid::{
    BoxConstraints, Cursor, Env, Event, EventCtx, LayoutCtx, LensExt, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
};

use crate::{
    config::LapceTheme, data::LapceTabData, panel::PanelPosition, svg::get_svg,
};

pub struct ActivityBar {}

impl ActivityBar {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<LapceTabData> for ActivityBar {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    let index = (mouse.pos.y / 50.0) as usize;
                    if let Some(panel) = data.panels.get_mut(&PanelPosition::LeftTop)
                    {
                        if let Some((widget_id, kind)) = panel
                            .widgets
                            .get(index.min(panel.widgets.len() - 1))
                            .as_ref()
                        {
                            Arc::make_mut(panel).active = *widget_id;
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
                if n > 0 && mouse.pos.y < 50.0 * n as f64 {
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
        Size::new(50.0, bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.size().to_rect();

        let size = 50.0;

        let shadow_width = 5.0;
        // if let Some((active_index, _)) =
        //     data.panels.get(&PanelPosition::LeftTop).and_then(|panel| {
        //         panel
        //             .widgets
        //             .iter()
        //             .map(|(id, kind)| *id)
        //             .enumerate()
        //             .find(|(i, id)| id == &panel.active)
        //     })
        // {
        //     let active_offset = size * active_index as f64;
        //     let shadow_width = 5.0;
        //     if active_offset > 0.0 {
        //         ctx.with_save(|ctx| {
        //             let clip_rect = Size::new(size + 100.0, active_offset).to_rect();
        //             ctx.clip(clip_rect);
        //             ctx.blurred_rect(
        //                 rect,
        //                 shadow_width,
        //                 data.config
        //                     .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        //             );
        //         });
        //     }
        //     ctx.with_save(|ctx| {
        //         let clip_rect =
        //             Size::new(size + 100.0, rect.height() - size - active_offset)
        //                 .to_rect()
        //                 .with_origin(Point::new(0.0, size + active_offset));
        //         ctx.clip(clip_rect);
        //         ctx.blurred_rect(
        //             rect,
        //             shadow_width,
        //             data.config
        //                 .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        //         );
        //     });
        // }
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );

        let mut offset = 0.0;
        let svg_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();
        if let Some(panel) = data.panels.get(&PanelPosition::LeftTop) {
            for (widget_id, kind) in panel.widgets.iter() {
                let svg = kind.svg();
                if &panel.active == widget_id {
                    ctx.fill(
                        Size::new(size, size)
                            .to_rect()
                            .with_origin(Point::new(0.0, offset)),
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                    );
                }
                let svg_size = 25.0;
                let rect =
                    Size::new(svg_size, svg_size)
                        .to_rect()
                        .with_origin(Point::new(
                            (size - svg_size) / 2.0,
                            (size - svg_size) / 2.0 + offset,
                        ));
                ctx.draw_svg(&svg, rect, Some(&svg_color));
                offset += size;
            }
        }
    }
}
