use std::sync::Arc;

use druid::{
    piet::{PietText, PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    theme, BoxConstraints, Color, Command, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    hover::{HoverData, HoverStatus},
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
            LapceScrollNew::new(Hover {}).vertical(),
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
                if let LapceUICommand::UpdateHover(request_id, resp) = command {
                    let hover = Arc::make_mut(&mut data.hover);
                    hover.receive(*request_id, resp.to_owned());
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
        self.hover.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.hover.status != HoverStatus::Inactive && !data.hover.is_empty() {
            let shadow_width = 5.0;
            let rect = self.content_size.to_rect();
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            self.hover.paint(ctx, data, env);
        }
    }
}

enum HoverLayout {
    Text {
        text_layout: PietTextLayout,
        /// The y position including this layout
        y: f64,
    },
    /// Empty, should be ignored
    Empty,
    /// The final entry
    Final { height: f64 },
}

#[derive(Default)]
pub struct Hover {}
impl Hover {
    const STARTING_Y: f64 = 5.0;
    const STARTING_X: f64 = 10.0;

    /// Map the text to the line and the layout builder instance
    fn iter_text<'a>(
        mut piet_text: PietText,
        line_height: f64,
        editor_foreground: Color,
        font_size: f64,
        font: FontFamily,
        lines: impl Iterator<Item = &'a str> + 'a,
        max_width: f64,
    ) -> impl Iterator<Item = HoverLayout> + 'a {
        let mut last_was_empty = false;
        let mut y = Hover::STARTING_Y;
        lines
            .map(Option::Some)
            .chain(std::iter::once(None))
            .map(move |line| {
                let line = if let Some(line) = line {
                    line
                } else {
                    // We've finished
                    return HoverLayout::Final { height: y };
                };
                let prev_y = y;
                if line.trim().is_empty() {
                    if !last_was_empty {
                        y += line_height;
                    }

                    last_was_empty = true;
                    return HoverLayout::Empty;
                } else {
                    last_was_empty = false;
                }

                let text_layout = piet_text
                    .new_text_layout(line.to_string())
                    .font(font.clone(), font_size)
                    .text_color(editor_foreground.clone())
                    .max_width(max_width)
                    .build()
                    .unwrap();

                let text_layout_size = text_layout.size();
                // Advance past any wrapping and the line height
                y += text_layout_size.height;

                HoverLayout::Text {
                    text_layout,
                    y: prev_y,
                }
            })
    }
}
impl Widget<LapceTabData> for Hover {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
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
        let item = match data.hover.get_current_item() {
            Some(item) => item,
            // There is nothing to render
            None => return Size::ZERO,
        };
        let text = item.as_str();

        let line_height = data.config.editor.line_height as f64;
        let editor_foreground = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();
        let font_size = data.config.editor.font_size as f64;
        let font = FontFamily::new_unchecked(data.config.editor.font_family.clone());

        let width = bc.max().width;
        let max_width = width
            - Hover::STARTING_X
            - env.get(theme::SCROLLBAR_WIDTH)
            - env.get(theme::SCROLLBAR_PAD);

        let mut height = 0.0;
        for layout in Hover::iter_text(
            ctx.text().clone(),
            line_height,
            editor_foreground,
            font_size,
            font,
            text.lines(),
            max_width,
        ) {
            if let HoverLayout::Final {
                height: final_height,
            } = layout
            {
                height = final_height
            }
        }

        height += line_height;
        Size::new(width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.hover.status == HoverStatus::Inactive {
            return;
        }
        let item = match data.hover.get_current_item() {
            Some(item) => item,
            // There is nothing to render
            None => return,
        };
        let text = item.as_str();

        let line_height = data.config.editor.line_height as f64;
        let editor_foreground = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();
        let font_size = data.config.editor.font_size as f64;
        let font = FontFamily::new_unchecked(data.config.editor.font_family.clone());

        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let max_width = size.width
            - Hover::STARTING_X
            - env.get(theme::SCROLLBAR_WIDTH)
            - env.get(theme::SCROLLBAR_PAD);

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::HOVER_BACKGROUND),
        );

        for layout in Hover::iter_text(
            ctx.text().clone(),
            line_height,
            editor_foreground,
            font_size,
            font,
            text.lines(),
            max_width,
        ) {
            if let HoverLayout::Text { y, text_layout } = layout {
                let text_point = Point::new(Hover::STARTING_X, y);
                ctx.draw_text(&text_layout, text_point);
            }
        }
    }
}
