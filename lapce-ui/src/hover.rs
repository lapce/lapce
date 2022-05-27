use std::sync::Arc;

use druid::{
    kurbo::Line, theme, ArcStr, BoxConstraints, Command, Data, Env, Event, EventCtx,
    FontDescriptor, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    hover::{HoverData, HoverStatus},
    rich_text::RichText,
};

use crate::scroll::{LapceIdentityWrapper, LapceScrollNew};

pub struct HoverContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    hover: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScrollNew<LapceTabData, Hover>>,
    >,
    content_size: Size,
}
impl HoverContainer {
    pub fn new(data: &HoverData) -> Self {
        let hover = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(Hover::new()).vertical(),
            data.scroll_id,
        );
        Self {
            id: data.id,
            scroll_id: data.scroll_id,
            hover: WidgetPod::new(hover),
            content_size: Size::ZERO,
        }
    }

    fn ensure_visible(
        &mut self,
        ctx: &mut UpdateCtx,
        _data: &LapceTabData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let rect = Size::new(width, 0.0).to_rect();
        if self
            .hover
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }
}
impl Widget<LapceTabData> for HoverContainer {
    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::UpdateHover(request_id, items) = command {
                    let hover = Arc::make_mut(&mut data.hover);
                    hover.receive(*request_id, items.clone());
                    ctx.request_paint();
                }
            }
            _ => {}
        }
        self.hover.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.hover.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        let old_hover = &old_data.hover;
        let hover = &data.hover;

        if hover.status != HoverStatus::Inactive {
            let old_editor = old_data.main_split.active_editor();
            let old_editor = match old_editor {
                Some(editor) => editor,
                None => return,
            };
            let editor = data.main_split.active_editor();
            let editor = match editor {
                Some(editor) => editor,
                None => return,
            };
            if old_editor.window_origin != editor.window_origin
                || old_editor.scroll_offset != editor.scroll_offset
            {
                ctx.request_layout();
            }
        }

        if old_hover.request_id != hover.request_id
            || old_hover.status != hover.status
            || !old_hover.items.same(&hover.items)
        {
            ctx.request_layout();
        }

        if old_hover.status == HoverStatus::Inactive
            && hover.status != HoverStatus::Inactive
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }

        if old_hover.active_item_index != hover.active_item_index {
            self.ensure_visible(ctx, data, env);
            ctx.request_paint();
        }

        self.hover.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = data.hover.size;
        let bc = BoxConstraints::new(Size::ZERO, size);
        self.content_size = self.hover.layout(ctx, &bc, data, env);
        *data.hover.content_size.borrow_mut() = self.content_size;
        self.hover.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.hover.status != HoverStatus::Inactive && !data.hover.is_empty() {
            let rect = self.content_size.to_rect();
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
                    rect.inflate(0.5, 0.5),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
            self.hover.paint(ctx, data, env);
        }
    }
}

#[derive(Default)]
struct Hover {
    active_layout: TextLayout<RichText>,
    active_diagnostic_layout: TextLayout<RichText>,
}

impl Hover {
    const STARTING_Y: f64 = 5.0;
    const STARTING_X: f64 = 10.0;

    fn new() -> Self {
        Hover {
            active_layout: {
                let mut layout = TextLayout::new();
                layout.set_text(RichText::new(ArcStr::from("")));
                layout
            },
            active_diagnostic_layout: {
                let mut layout = TextLayout::new();
                layout.set_text(RichText::new(ArcStr::from("")));
                layout
            },
        }
    }
}
impl Widget<LapceTabData> for Hover {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        if let Event::MouseMove(_) = event {
            ctx.set_handled();
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
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        // If the active item has changed or we've switched our current item existence status
        // or we've gotten new diagnostic text
        // then update the layout
        if old_data.hover.request_id != data.hover.request_id
            || old_data.hover.active_item_index != data.hover.active_item_index
            || old_data.hover.get_current_item().is_some()
                != data.hover.get_current_item().is_some()
            || !old_data
                .hover
                .diagnostic_content
                .same(&data.hover.diagnostic_content)
        {
            if let Some(item) = data.hover.get_current_item() {
                self.active_layout.set_text(item.clone());
            } else {
                self.active_layout.set_text(RichText::new(ArcStr::from("")));
            }

            if let Some(diagnostic_content) = &data.hover.diagnostic_content {
                self.active_diagnostic_layout
                    .set_text(diagnostic_content.clone());
            } else {
                self.active_diagnostic_layout
                    .set_text(RichText::new(ArcStr::from("")));
            }

            let font = FontDescriptor::new(data.config.ui.font_family())
                .with_size(data.config.ui.font_size() as f64);
            let text_color = data
                .config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                .clone();

            self.active_layout.set_font(font.clone());
            self.active_layout.set_text_color(text_color.clone());

            self.active_diagnostic_layout.set_font(font);
            self.active_diagnostic_layout.set_text_color(text_color);

            if self.active_layout.needs_rebuild_after_update(ctx)
                || self
                    .active_diagnostic_layout
                    .needs_rebuild_after_update(ctx)
            {
                ctx.request_layout();
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = bc.max().width;
        let max_width = width
            - Hover::STARTING_X
            - env.get(theme::SCROLLBAR_WIDTH)
            - env.get(theme::SCROLLBAR_PAD);

        self.active_layout.set_wrap_width(max_width);
        self.active_layout.rebuild_if_needed(ctx.text(), env);

        self.active_diagnostic_layout.set_wrap_width(max_width);
        self.active_diagnostic_layout
            .rebuild_if_needed(ctx.text(), env);

        let text_metrics = self.active_layout.layout_metrics();
        ctx.set_baseline_offset(
            text_metrics.size.height - text_metrics.first_baseline,
        );

        let diagnostic_height = if self.active_diagnostic_layout.size().is_empty() {
            0.0
        } else {
            let diagnostic_text_metrics =
                self.active_diagnostic_layout.layout_metrics();

            diagnostic_text_metrics.size.height
                + data.config.editor.line_height as f64
        };

        Size::new(
            width,
            text_metrics.size.height + diagnostic_height + Hover::STARTING_Y * 2.0,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.hover.status != HoverStatus::Done {
            return;
        }

        let rect = ctx.region().bounding_box();
        let diagnostic_origin = Point::new(Self::STARTING_X, Self::STARTING_Y);

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::HOVER_BACKGROUND),
        );

        // Draw diagnostic text if it exists
        let height = if self.active_diagnostic_layout.size().is_empty() {
            0.0
        } else {
            // let text_metrics = self.active_layout.layout_metrics();
            let diagnostic_text_metrics =
                self.active_diagnostic_layout.layout_metrics();

            let side_margin =
                env.get(theme::SCROLLBAR_WIDTH) + env.get(theme::SCROLLBAR_PAD);
            let line_height = data.config.editor.line_height as f64;

            // Create a separating line
            let line = {
                let x0 = rect.x0 + side_margin;
                let y = diagnostic_text_metrics.size.height + line_height;
                let x1 = rect.x1 - side_margin;
                Line::new(Point::new(x0, y), Point::new(x1, y))
            };

            // Draw the separator
            ctx.stroke(
                line,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );

            // Draw the diagnostic text
            self.active_diagnostic_layout.draw(ctx, diagnostic_origin);

            diagnostic_text_metrics.size.height + line_height
        };

        let doc_origin = diagnostic_origin + (0.0, height);

        self.active_layout.draw(ctx, doc_origin);
    }
}
