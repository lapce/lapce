use druid::{
    FontDescriptor, FontFamily, Point, RenderContext, Size, TextLayout, Widget,
};
use lsp_types::DiagnosticSeverity;

use crate::{
    state::{LapceUIState, LAPCE_STATE},
    theme::LapceTheme,
};

pub struct LapceStatus {}

impl LapceStatus {
    pub fn new() -> LapceStatus {
        LapceStatus {}
    }
}

impl Widget<LapceUIState> for LapceStatus {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut LapceUIState,
        env: &druid::Env,
    ) {
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

        let editor_split = LAPCE_STATE.editor_split.lock();

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

        let mut text_layout = TextLayout::new(format!("{}  {}", errors, warnings));
        text_layout
            .set_font(FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0));
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        text_layout.draw(ctx, Point::new(10.0, 5.0));
    }
}
