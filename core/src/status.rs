use druid::theme;
use druid::Color;
use druid::Vec2;
use druid::{
    kurbo::Line, Event, FontDescriptor, FontFamily, Point, RenderContext, Size,
    TextLayout, Widget, WidgetId, WindowId,
};
use lsp_types::DiagnosticSeverity;

use crate::data::LapceTabData;
use crate::state::Mode;
use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    state::{LapceUIState, LAPCE_APP_STATE},
    theme::LapceTheme,
};

pub struct LapceStatus {
    status_id: WidgetId,
    window_id: WindowId,
    tab_id: WidgetId,
}

impl LapceStatus {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> LapceStatus {
        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let status_id = state.status_id;
        LapceStatus {
            window_id,
            tab_id,
            status_id,
        }
    }
}

impl Widget<LapceUIState> for LapceStatus {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &Event,
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
                        LapceUICommand::RequestPaintRect(rect) => {
                            ctx.request_paint_rect(*rect);
                        }
                        _ => println!("editor unprocessed ui command {:?}", command),
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        if old_data.mode != data.mode {
            ctx.request_paint();
            return;
        }
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceUIState,
        env: &druid::Env,
    ) -> druid::Size {
        Size::new(bc.max().width, 25.0)
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.fill(rect, &env.get(LapceTheme::EDITOR_SELECTION_COLOR));

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();

        let mut left = 0.0;
        let (mode, color) = match editor_split.get_mode() {
            Mode::Normal => ("Normal", Color::rgb8(64, 120, 242)),
            Mode::Insert => ("Insert", Color::rgb8(228, 86, 73)),
            Mode::Visual => ("Visual", Color::rgb8(193, 132, 1)),
        };
        let mut text_layout = TextLayout::<String>::from_text(mode);
        text_layout
            .set_font(FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0));
        text_layout.set_text_color(LapceTheme::EDITOR_BACKGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let text_size = text_layout.size();
        let fill_size = Size::new(text_size.width + 10.0, size.height);
        ctx.fill(fill_size.to_rect(), &color);
        text_layout.draw(ctx, Point::new(5.0, 4.0));
        left += text_size.width + 10.0;

        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnositics) in editor_split.diagnostics.iter() {
            for diagnositic in diagnositics {
                if let Some(severity) = diagnositic.severity {
                    match severity {
                        DiagnosticSeverity::Error => errors += 1,
                        DiagnosticSeverity::Warning => warnings += 1,
                        _ => (),
                    }
                }
            }
        }

        let mut text_layout =
            TextLayout::<String>::from_text(format!("{}  {}", errors, warnings));
        text_layout
            .set_font(FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0));
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        text_layout.draw(ctx, Point::new(left + 10.0, 4.0));
        left += 10.0 + text_layout.size().width;
    }

    fn id(&self) -> Option<WidgetId> {
        Some(self.status_id)
    }
}

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
        ctx.blurred_rect(rect, 5.0, &Color::grey8(180));
        ctx.fill(rect, &env.get(LapceTheme::LIST_BACKGROUND));

        let mut left = 0.0;
        let (mode, color) = match data.main_split.active_editor().cursor.get_mode() {
            Mode::Normal => ("Normal", Color::rgb8(64, 120, 242)),
            Mode::Insert => ("Insert", Color::rgb8(228, 86, 73)),
            Mode::Visual => ("Visual", Color::rgb8(193, 132, 1)),
        };
        let mut text_layout = TextLayout::<String>::from_text(mode);
        text_layout
            .set_font(FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0));
        text_layout.set_text_color(LapceTheme::EDITOR_BACKGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let text_size = text_layout.size();
        let fill_size = Size::new(text_size.width + 10.0, size.height);
        ctx.fill(fill_size.to_rect(), &color);
        text_layout.draw(ctx, Point::new(5.0, 4.0));
        left += text_size.width + 10.0;

        let mut text_layout = TextLayout::<String>::from_text(format!(
            "{}  {}",
            data.main_split.error_count, data.main_split.warning_count
        ));
        text_layout
            .set_font(FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0));
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        text_layout.draw(ctx, Point::new(left + 10.0, 4.0));
        left += 10.0 + text_layout.size().width;
    }
}
