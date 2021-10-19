use druid::piet::Text;
use druid::piet::TextLayout;
use druid::piet::TextLayoutBuilder;
use druid::theme;
use druid::Color;
use druid::Vec2;
use druid::{
    kurbo::Line, Event, FontDescriptor, FontFamily, Point, RenderContext, Size,
    Widget, WidgetId, WindowId,
};
use lsp_types::DiagnosticSeverity;

use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};
use crate::config::LapceTheme;
use crate::data::LapceTabData;
use crate::state::Mode;
use crate::theme::OldLapceTheme;

pub struct LapceStatusNew {}

impl LapceStatusNew {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<LapceTabData> for LapceStatusNew {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &druid::Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceTabData,
        env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &druid::Env,
    ) {
        if old_data.main_split.active_editor().cursor.get_mode()
            != data.main_split.active_editor().cursor.get_mode()
        {
            ctx.request_paint();
            return;
        }

        if old_data.main_split.warning_count != data.main_split.warning_count
            || old_data.main_split.error_count != data.main_split.error_count
        {
            ctx.request_paint();
            return;
        }
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceTabData,
        env: &druid::Env,
    ) -> Size {
        ctx.set_paint_insets((0.0, 10.0, 0.0, 0.0));
        Size::new(bc.max().width, 25.0)
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceTabData,
        env: &druid::Env,
    ) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.blurred_rect(
            rect,
            5.0,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::STATUS_BACKGROUND),
        );

        let mut left = 0.0;

        if data.config.lapce.modal {
            let (mode, color) =
                match data.main_split.active_editor().cursor.get_mode() {
                    Mode::Normal => ("Normal", Color::rgb8(64, 120, 242)),
                    Mode::Insert => ("Insert", Color::rgb8(228, 86, 73)),
                    Mode::Visual => ("Visual", Color::rgb8(193, 132, 1)),
                    Mode::Terminal => ("Terminal", Color::rgb8(64, 120, 242)),
                };

            let text_layout = ctx
                .text()
                .new_text_layout(mode)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let fill_size = Size::new(text_size.width + 10.0, size.height);
            ctx.fill(fill_size.to_rect(), &color);
            ctx.draw_text(&text_layout, Point::new(5.0, 4.0));
            left += text_size.width + 10.0;
        }

        let text_layout = ctx
            .text()
            .new_text_layout(format!(
                "{}  {}",
                data.main_split.error_count, data.main_split.warning_count
            ))
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(&text_layout, Point::new(left + 10.0, 4.0));
        left += 10.0 + text_layout.size().width;
    }
}
