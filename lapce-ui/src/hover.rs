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
    markdown::layout_content::{
        layout_content_clean_up, layouts_from_contents, LayoutContent,
    },
    rich_text::RichText,
};

use crate::scroll::{LapceIdentityWrapper, LapceScroll};

pub struct HoverContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    hover: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScroll<LapceTabData, Hover>>,
    >,
    content_size: Size,
}
impl HoverContainer {
    pub fn new(data: &HoverData) -> Self {
        let hover = LapceIdentityWrapper::wrap(
            LapceScroll::new(Hover::new()).vertical(),
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
                if let LapceUICommand::UpdateHover { request_id, items } = command {
                    // TODO: Should we check whether it has actually changed?
                    let hover = Arc::make_mut(&mut data.hover);
                    hover.receive(*request_id, items.clone());

                    self.hover
                        .widget_mut()
                        .inner_mut()
                        .child_mut()
                        .update_layouts(ctx, data);

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
            self.ensure_visible(ctx, data, env);
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
    active_layout: Vec<LayoutContent>,
    active_diagnostic_layout: TextLayout<RichText>,
}

impl Hover {
    const STARTING_Y: f64 = 5.0;
    const STARTING_X: f64 = 10.0;

    fn new() -> Self {
        Hover {
            active_layout: { Vec::new() },
            active_diagnostic_layout: {
                let mut layout = TextLayout::new();
                layout.set_text(RichText::new(ArcStr::from("")));
                layout
            },
        }
    }

    fn update_layouts(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        let items = data.hover.items.clone();

        layout_content_clean_up(&mut self.active_layout, data);
        self.active_layout = layouts_from_contents(ctx, data, items.iter());

        if let Some(diagnostic_content) = &data.hover.diagnostic_content {
            self.active_diagnostic_layout
                .set_text(diagnostic_content.clone());
        } else {
            self.active_diagnostic_layout
                .set_text(RichText::new(ArcStr::from("")));
        }

        let font = FontDescriptor::new(data.config.ui.hover_font_family())
            .with_size(data.config.ui.hover_font_size() as f64);
        let text_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();

        for layout in self.active_layout.iter_mut() {
            layout.set_font(font.clone());
            layout.set_text_color(text_color.clone());
        }

        self.active_diagnostic_layout.set_font(font);
        self.active_diagnostic_layout.set_text_color(text_color);

        ctx.request_layout();
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
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
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

        let mut max_layout_width = 0.0;
        for layout in self.active_layout.iter_mut() {
            layout.set_max_width(&data.images, max_width);
            layout.rebuild_if_needed(ctx.text(), env);
            let layout_width = layout.size(&data.images, &data.config).width;
            if layout_width > max_layout_width {
                max_layout_width = layout_width;
            }
        }

        self.active_diagnostic_layout.set_wrap_width(max_width);
        self.active_diagnostic_layout
            .rebuild_if_needed(ctx.text(), env);

        let items_height = if self.active_layout.is_empty() {
            0.0
        } else {
            let mut height = 0.0;
            for layout in self.active_layout.iter() {
                height += layout.size(&data.images, &data.config).height;
            }

            if self.active_layout.len() > 1 {
                let line_height = data.config.editor.line_height() as f64;
                height += (self.active_layout.len() - 1) as f64 * line_height
            }
            height
        };

        let diagnostic_size = self.active_diagnostic_layout.size();
        let diagnostic_height = if diagnostic_size.is_empty() {
            0.0
        } else {
            let diagnostic_text_metrics =
                self.active_diagnostic_layout.layout_metrics();

            diagnostic_text_metrics.size.height + Hover::STARTING_Y * 3.0
        };

        if diagnostic_size.width > max_layout_width {
            max_layout_width = diagnostic_size.width;
        }

        let width = if max_layout_width < max_width {
            max_layout_width
                + Hover::STARTING_X
                + env.get(theme::SCROLLBAR_WIDTH)
                + env.get(theme::SCROLLBAR_PAD)
        } else {
            width
        };

        Size::new(
            width,
            items_height + diagnostic_height + Hover::STARTING_Y * 2.0,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.hover.status != HoverStatus::Done {
            return;
        }

        let side_margin =
            env.get(theme::SCROLLBAR_WIDTH) + env.get(theme::SCROLLBAR_PAD);

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
            let diagnostic_text_metrics =
                self.active_diagnostic_layout.layout_metrics();

            let line = {
                let x0 = rect.x0 + side_margin;
                let y =
                    diagnostic_text_metrics.size.height + Hover::STARTING_Y * 3.0;
                let x1 = rect.x1 - side_margin;
                Line::new(Point::new(x0, y), Point::new(x1, y))
            };

            ctx.stroke(
                line,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );

            self.active_diagnostic_layout.draw(ctx, diagnostic_origin);

            diagnostic_text_metrics.size.height + Hover::STARTING_Y * 3.0
        };

        let doc_origin = diagnostic_origin + (0.0, height);

        let mut start_height = 0.0;

        for layout in self.active_layout.iter_mut() {
            layout.draw(
                ctx,
                &data.images,
                &data.config,
                doc_origin + (0.0, start_height),
            );

            let layout_size = layout.size(&data.images, &data.config);
            start_height += layout_size.height;
        }
    }
}
