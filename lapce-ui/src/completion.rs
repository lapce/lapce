use std::sync::Arc;

use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    theme, ArcStr, BoxConstraints, Command, Data, Env, Event, EventCtx,
    FontDescriptor, FontFamily, FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout, UpdateCtx,
    Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    completion::{CompletionData, CompletionStatus, ScoredCompletionItem},
    config::LapceTheme,
    data::LapceTabData,
    document::BufferContent,
    list::ListData,
    markdown::parse_documentation,
    rich_text::RichText,
};

use crate::{
    list::{List, ListPaint},
    scroll::{LapceIdentityWrapper, LapceScroll},
};

pub struct CompletionContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    completion: WidgetPod<
        ListData<ScoredCompletionItem, ()>,
        List<ScoredCompletionItem, ()>,
    >,
    completion_content_size: Size,
    documentation: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScroll<LapceTabData, CompletionDocumentation>>,
    >,
    documentation_content_size: Size,
}

impl CompletionContainer {
    pub fn new(data: &CompletionData) -> Self {
        let completion = List::new(data.scroll_id);
        let completion_doc = LapceIdentityWrapper::wrap(
            LapceScroll::new(CompletionDocumentation::new()).vertical(),
            data.documentation_scroll_id,
        );
        Self {
            id: data.id,
            completion: WidgetPod::new(completion),
            scroll_id: data.scroll_id,
            completion_content_size: Size::ZERO,
            documentation: WidgetPod::new(completion_doc),
            documentation_content_size: Size::ZERO,
        }
    }

    pub fn ensure_item_visible(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.completion.widget_mut().ensure_item_visible(
            ctx,
            &data
                .completion
                .completion_list
                .clone_with(data.config.clone()),
            env,
        );
    }

    /// Like [`Self::ensure_item_visible`] but instead making so that it is at the very top of the display
    /// rather than just scrolling the minimal distance to make it visible.
    pub fn ensure_item_top_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceTabData,
    ) {
        let line_height = data.completion.completion_list.line_height() as f64;
        let point = Point::new(
            0.0,
            data.completion.completion_list.selected_index as f64 * line_height,
        );
        if self.completion.widget_mut().scroll_to(point) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    fn update_documentation(&mut self, data: &LapceTabData) {
        if data.completion.status == CompletionStatus::Inactive {
            return;
        }

        let documentation = if data.config.editor.completion_show_documentation {
            let current_item = (!data.completion.is_empty())
                .then(|| data.completion.current_item())
                .flatten();

            current_item.and_then(|item| item.item.documentation.as_ref())
        } else {
            None
        };

        let text = if let Some(documentation) = documentation {
            parse_documentation(documentation, &data.config)
        } else {
            // There is no documentation so we clear the text
            RichText::new(ArcStr::from(""))
        };

        let doc = self.documentation.widget_mut().inner_mut().child_mut();

        doc.doc_layout.set_text(text);

        let font = FontDescriptor::new(data.config.ui.hover_font_family())
            .with_size(data.config.ui.hover_font_size() as f64);
        let text_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();

        doc.doc_layout.set_font(font);
        doc.doc_layout.set_text_color(text_color);
    }
}

impl Widget<LapceTabData> for CompletionContainer {
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
                if let LapceUICommand::ListItemSelected = command {
                    if let Some(editor) = data
                        .main_split
                        .active
                        .and_then(|active| data.main_split.editors.get(&active))
                        .cloned()
                    {
                        let mut editor_data =
                            data.editor_view_content(editor.view_id);
                        let doc = editor_data.doc.clone();
                        editor_data.completion_item_select(ctx);
                        data.update_from_editor_buffer_data(
                            editor_data,
                            &editor,
                            &doc,
                        );
                    }
                }
            }
            _ => {}
        }

        let completion = Arc::make_mut(&mut data.completion);
        completion.completion_list.update_data(data.config.clone());
        self.completion
            .event(ctx, event, &mut completion.completion_list, env);

        if let Some(editor) = data
            .main_split
            .active
            .and_then(|active| data.main_split.editors.get(&active))
            .cloned()
        {
            if let BufferContent::File(path) = &editor.content {
                if let Some(doc) = data.main_split.open_docs.get_mut(path) {
                    completion.update_document_completion(
                        &editor,
                        doc,
                        &data.config,
                    );
                }
            }
        }

        self.documentation.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.completion.lifecycle(
            ctx,
            event,
            &data
                .completion
                .completion_list
                .clone_with(data.config.clone()),
            env,
        );
        self.documentation.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        let old_completion = &old_data.completion;
        let completion = &data.completion;

        if data.completion.status != CompletionStatus::Inactive {
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

            let completion_list_changed = !completion
                .completion_list
                .same(&old_completion.completion_list);
            if old_data.completion.input != data.completion.input
                || old_data.completion.request_id != data.completion.request_id
                || old_data.completion.status != data.completion.status
                || completion_list_changed
            {
                self.update_documentation(data);
                ctx.request_layout();
            }

            if (old_completion.status == CompletionStatus::Inactive
                && completion.status != CompletionStatus::Inactive)
                || (old_completion.input != completion.input)
            {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.scroll_id),
                ));
            }

            if self
                .documentation
                .widget_mut()
                .inner_mut()
                .child_mut()
                .doc_layout
                .needs_rebuild_after_update(ctx)
            {
                ctx.request_layout();
            }

            self.completion.update(
                ctx,
                &data
                    .completion
                    .completion_list
                    .clone_with(data.config.clone()),
                env,
            );
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        // TODO: Let this be configurable
        let width = 400.0;

        let bc = BoxConstraints::tight(Size::new(width, bc.max().height));

        let completion_list = data
            .completion
            .completion_list
            .clone_with(data.config.clone());
        self.completion_content_size =
            self.completion.layout(ctx, &bc, &completion_list, env);
        self.completion
            .set_origin(ctx, &completion_list, env, Point::ZERO);

        // Position the documentation over the current completion item to the right
        let bc = BoxConstraints::new(Size::ZERO, data.completion.documentation_size);
        self.documentation_content_size =
            self.documentation.layout(ctx, &bc, data, env);
        self.documentation.set_origin(
            ctx,
            data,
            env,
            Point::new(self.completion_content_size.width, 0.0),
        );

        Size::new(
            self.completion_content_size.width
                + self.documentation_content_size.width,
            self.completion_content_size
                .height
                .max(self.documentation_content_size.height),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.completion.status != CompletionStatus::Inactive
            && data.completion.len() > 0
        {
            let rect = self.completion_content_size.to_rect();

            // Draw the background
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::COMPLETION_BACKGROUND),
            );

            // Draw the shadow
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

            // Draw the completion list over the background
            self.completion.paint(
                ctx,
                &data
                    .completion
                    .completion_list
                    .clone_with(data.config.clone()),
                env,
            );

            // Draw the documentation to the side
            self.documentation.paint(ctx, data, env);
        }
    }
}

impl<D: Data> ListPaint<D> for ScoredCompletionItem {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &ListData<Self, D>,
        _env: &Env,
        line: usize,
    ) {
        let size = ctx.size();
        let line_height = data.line_height() as f64;
        if line == data.selected_index {
            ctx.fill(
                Rect::ZERO
                    .with_origin(Point::new(0.0, line as f64 * line_height))
                    .with_size(Size::new(size.width, line_height)),
                data.config
                    .get_color_unchecked(LapceTheme::COMPLETION_CURRENT),
            );
        }

        if let Some((svg, color)) = data.config.completion_svg(self.item.kind) {
            let color = color.unwrap_or_else(|| {
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone()
            });
            let rect = Size::new(line_height, line_height)
                .to_rect()
                .with_origin(Point::new(0.0, line_height * line as f64));
            ctx.fill(rect, &color.clone().with_alpha(0.3));

            let width = 16.0;
            let height = 16.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (line_height - width) / 2.0,
                (line_height - height) / 2.0 + line_height * line as f64,
            ));
            ctx.draw_svg(&svg, rect, Some(&color));
        }

        let focus_color = data.config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);
        let content = self.item.label.as_str();

        let mut text_layout = ctx
            .text()
            .new_text_layout(content.to_string())
            .font(
                FontFamily::new_unchecked(data.config.editor.font_family.clone()),
                data.config.editor.font_size as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );
        for i in &self.indices {
            let i = *i;
            text_layout = text_layout.range_attribute(
                i..i + 1,
                TextAttribute::TextColor(focus_color.clone()),
            );
            text_layout = text_layout
                .range_attribute(i..i + 1, TextAttribute::Weight(FontWeight::BOLD));
        }
        let text_layout = text_layout.build().unwrap();
        let y = line_height * line as f64 + text_layout.y_offset(line_height);
        let point = Point::new(line_height + 5.0, y);
        ctx.draw_text(&text_layout, point);
    }
}

struct CompletionDocumentation {
    doc_layout: TextLayout<RichText>,
}
impl CompletionDocumentation {
    const STARTING_Y: f64 = 5.0;
    const STARTING_X: f64 = 10.0;

    fn new() -> CompletionDocumentation {
        Self {
            doc_layout: {
                let mut layout = TextLayout::new();
                layout.set_text(RichText::new(ArcStr::from("")));
                layout
            },
        }
    }

    fn has_text(&self) -> bool {
        self.doc_layout
            .text()
            .map(|text| !text.is_empty())
            .unwrap_or(false)
    }
}
impl Widget<LapceTabData> for CompletionDocumentation {
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
        // This is managed by the completion container
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        env: &Env,
    ) -> Size {
        if !self.has_text() {
            return Size::ZERO;
        }

        let width = bc.max().width;
        let max_width = width
            - CompletionDocumentation::STARTING_X
            - env.get(theme::SCROLLBAR_WIDTH)
            - env.get(theme::SCROLLBAR_PAD);

        self.doc_layout.set_wrap_width(max_width);
        self.doc_layout.rebuild_if_needed(ctx.text(), env);

        let text_metrics = self.doc_layout.layout_metrics();
        ctx.set_baseline_offset(
            text_metrics.size.height - text_metrics.first_baseline,
        );

        Size::new(
            width,
            text_metrics.size.height + CompletionDocumentation::STARTING_Y * 2.0,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if data.completion.status == CompletionStatus::Inactive || !self.has_text() {
            return;
        }

        let rect = ctx.region().bounding_box();

        // Draw the background
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::HOVER_BACKGROUND),
        );

        let origin = Point::new(Self::STARTING_X, Self::STARTING_Y);

        self.doc_layout.draw(ctx, origin);
    }
}
