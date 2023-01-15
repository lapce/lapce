use std::ops::Range;

use druid::{
    theme, ArcStr, BoxConstraints, Color, Command, Env, Event, EventCtx,
    FontDescriptor, FontWeight, LayoutCtx, LifeCycle, PaintCtx, Point,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_core::{encoding::offset_utf16_to_utf8, language::LapceLanguage};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceTheme},
    data::LapceTabData,
    document::BufferContent,
    markdown::{
        highlight_as_code,
        layout_content::{
            layout_content_clean_up, layouts_from_contents, LayoutContent,
        },
        parse_documentation, Content,
    },
    rich_text::{RichText, RichTextBuilder},
    signature::{SignatureData, SignatureStatus},
};
use lsp_types::{ParameterLabel, SignatureInformation};

use crate::scroll::{LapceIdentityWrapper, LapceScroll};

pub struct SignatureContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    signature: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScroll<LapceTabData, Signature>>,
    >,
    signature_content_size: Size,
    pub label_offset: f64,
}
impl SignatureContainer {
    pub fn new(data: &SignatureData) -> Self {
        let signature = LapceIdentityWrapper::wrap(
            LapceScroll::new(Signature::new()).vertical(),
            data.scroll_id,
        );
        Self {
            id: data.id,
            scroll_id: data.scroll_id,
            signature: WidgetPod::new(signature),
            signature_content_size: Size::ZERO,
            label_offset: 0.0,
        }
    }

    fn update_signature(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        if data.signature.status == SignatureStatus::Inactive {
            return;
        }

        let signature = if data.config.editor.show_signature {
            data.signature.current()
        } else {
            None
        };

        let (label, param_doc, doc) = if let Some(signature) = signature {
            // Get the language of the current file, so that we can style the label
            let language = {
                let editor = data.main_split.active_editor();
                let document = editor.and_then(|editor| {
                    if matches!(
                        editor.content,
                        BufferContent::File(_) | BufferContent::Scratch(_, _)
                    ) {
                        Some(data.main_split.editor_doc(editor.view_id))
                    } else {
                        None
                    }
                });
                document.and_then(|document| {
                    document.syntax().map(|syntax| syntax.language)
                })
            };

            parse_signature(
                signature,
                data.signature.active_parameter,
                language,
                &data.config,
            )
        } else {
            (RichText::new(ArcStr::from("")), None, None)
        };
        let label_text = signature.map(|s| s.label.clone()).unwrap_or_default();
        let param_doc = param_doc.unwrap_or_default();
        let doc = doc.unwrap_or_default();

        let sig = self.signature.widget_mut().inner_mut().child_mut();
        sig.label_layout.set_text(label);

        layout_content_clean_up(&mut sig.param_doc_layout, data);
        layout_content_clean_up(&mut sig.doc_layout, data);

        sig.param_doc_layout = layouts_from_contents(ctx, data, param_doc.iter());

        sig.doc_layout = layouts_from_contents(ctx, data, doc.iter());

        sig.label = label_text;

        // Set font / text color information
        let font = FontDescriptor::new(data.config.ui.hover_font_family())
            .with_size(data.config.ui.hover_font_size() as f64);
        let text_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();

        sig.label_layout.set_font(font.clone());
        sig.label_layout.set_text_color(text_color.clone());

        for param_doc in sig.param_doc_layout.iter_mut() {
            param_doc.set_font(font.clone());
            param_doc.set_text_color(text_color.clone());
        }

        for doc in sig.doc_layout.iter_mut() {
            doc.set_font(font.clone());
            doc.set_text_color(text_color.clone());
        }
    }
}
impl Widget<LapceTabData> for SignatureContainer {
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
        if let Event::Command(cmd) = event {
            if cmd.is(LAPCE_UI_COMMAND) {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::RefreshSignature = *command {
                    self.update_signature(ctx, data);
                    ctx.request_layout();
                }
            }
        }

        self.signature.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.signature.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        let old_signature = &old_data.signature;
        let signature = &data.signature;

        if data.signature.status != SignatureStatus::Inactive {
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

            if old_signature.request_id != signature.request_id
                || old_signature.status != data.signature.status
                || old_signature.signatures != signature.signatures
                || old_signature.current_signature != signature.current_signature
            {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RefreshSignature,
                    Target::Widget(self.id),
                ));
            }

            if old_signature.status == SignatureStatus::Inactive
                && signature.status != SignatureStatus::Inactive
            {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.scroll_id),
                ));
            }

            let sig = self.signature.widget_mut().inner_mut().child_mut();
            // Note: this deliberately uses bitwise-or so that both are executed
            // since `needs_rebuild_after_update` alters internal state of the layout
            // (Is there a more clear way of doing this?)
            let mut needs_layout = false;
            needs_layout |= sig.label_layout.needs_rebuild_after_update(ctx);

            for param_doc in sig.param_doc_layout.iter_mut() {
                needs_layout |= param_doc.needs_rebuild_after_update(ctx);
            }

            for doc in sig.doc_layout.iter_mut() {
                needs_layout |= doc.needs_rebuild_after_update(ctx);
            }

            if needs_layout {
                ctx.request_layout();
            }

            self.signature.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let bc = BoxConstraints::new(Size::ZERO, data.signature.size);
        self.signature_content_size = self.signature.layout(ctx, &bc, data, env);
        self.signature.set_origin(ctx, data, env, Point::ZERO);

        self.label_offset = self.signature.widget().inner().child().label_offset;

        self.signature_content_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.signature.status != SignatureStatus::Inactive {
            let rect = self.signature_content_size.to_rect();
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
            self.signature.paint(ctx, data, env);
        }
    }
}

pub struct Signature {
    label: String,
    label_offset: f64,
    label_layout: TextLayout<RichText>,
    param_doc_layout: Vec<LayoutContent>,
    doc_layout: Vec<LayoutContent>,
}
impl Signature {
    const STARTING_Y: f64 = 5.0;
    const STARTING_X: f64 = 10.0;
    /// Padding between the label and the documentation
    const PADDING: f64 = 5.0;

    fn new() -> Signature {
        Self {
            label: "".to_string(),
            label_offset: 0.0,
            label_layout: {
                let mut layout = TextLayout::new();
                layout.set_text(RichText::new(ArcStr::from("")));
                layout
            },
            param_doc_layout: Vec::new(),
            doc_layout: Vec::new(),
        }
    }

    fn has_label_text(&self) -> bool {
        self.label_layout
            .text()
            .map(|text| !text.is_empty())
            .unwrap_or(false)
    }

    fn has_param_doc_text(&self) -> bool {
        !self.param_doc_layout.is_empty()
    }

    fn has_doc_text(&self) -> bool {
        !self.doc_layout.is_empty()
    }

    fn has_text(&self) -> bool {
        self.has_label_text() || self.has_param_doc_text() || self.has_doc_text()
    }
}
impl Widget<LapceTabData> for Signature {
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
        _ctx: &mut druid::LifeCycleCtx,
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
    ) -> druid::Size {
        if !self.has_text() {
            return Size::ZERO;
        }

        let width = bc.max().width;
        let max_width = width
            - Self::STARTING_X
            - env.get(theme::SCROLLBAR_WIDTH)
            - env.get(theme::SCROLLBAR_PAD);

        self.label_layout.set_wrap_width(max_width);
        self.label_layout.rebuild_if_needed(ctx.text(), env);

        if let Some(col) = self.label.find('(') {
            self.label_offset = self.label_layout.point_for_text_position(col + 1).x;
        }

        for param_doc in self.param_doc_layout.iter_mut() {
            param_doc.set_max_width(&data.images, max_width);
            param_doc.rebuild_if_needed(ctx.text(), env);
        }

        for doc in self.doc_layout.iter_mut() {
            doc.set_max_width(&data.images, max_width);
            doc.rebuild_if_needed(ctx.text(), env);
        }

        let mut height = 0.0;

        if self.has_label_text() {
            let text_metrics = self.label_layout.layout_metrics();
            ctx.set_baseline_offset(
                text_metrics.size.height - text_metrics.first_baseline,
            );
            height += text_metrics.size.height;
        }

        // TODO: draw separator around param docs?

        for param_doc in self.param_doc_layout.iter() {
            let size = param_doc.size(&data.images, &data.config);
            height += size.height + Self::PADDING;
        }

        for doc in self.doc_layout.iter() {
            let size = doc.size(&data.images, &data.config);
            height += size.height + Self::PADDING;
        }

        Size::new(width, height + Self::STARTING_Y + Self::PADDING)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if data.signature.status == SignatureStatus::Inactive || !self.has_text() {
            return;
        }

        let rect = ctx.region().bounding_box();

        // Draw the background
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::HOVER_BACKGROUND),
        );

        let mut origin = Point::new(Self::STARTING_X, Self::STARTING_Y);

        if self.has_label_text() {
            self.label_layout.draw(ctx, origin);
            origin.y +=
                self.label_layout.layout_metrics().size.height + Self::PADDING;
        }

        for param_doc in self.param_doc_layout.iter() {
            let size = param_doc.size(&data.images, &data.config);
            param_doc.draw(ctx, &data.images, &data.config, origin);
            origin.y += size.height + Self::PADDING;
        }

        for doc in self.doc_layout.iter() {
            let size = doc.size(&data.images, &data.config);
            doc.draw(ctx, &data.images, &data.config, origin);
            origin.y += size.height + Self::PADDING;
        }
    }
}

fn parse_signature(
    sig: &SignatureInformation,
    active_parameter: Option<usize>,
    language: Option<LapceLanguage>,
    config: &LapceConfig,
) -> (RichText, Option<Vec<Content>>, Option<Vec<Content>>) {
    let doc = sig
        .documentation
        .as_ref()
        .map(|doc| parse_documentation(doc, config));

    let (label, param_doc) = {
        let mut builder = RichTextBuilder::new();
        builder.set_line_height(1.5);

        let mut attrs = builder.push(&sig.label);
        // Display the the label in a code block for the specific file's language
        // This will work fine for most languages, though could be mishighlighted in some situations
        // Unfortunately, the LSP does not provide markdown to explicitly say whether and how
        // it should be highlighted.
        if config.editor.signature_label_code_block {
            attrs.font_family(config.editor.font_family());

            highlight_as_code(&mut builder, config, language, &sig.label, 0);
        }

        // If the parameters are defined and we know the active parameter, then
        // we can apply styling to make it clear to the user what the current parameter is
        let param_doc = if let Some(parameter) = sig
            .parameters
            .as_deref()
            .zip(active_parameter)
            .and_then(|(params, idx)| params.get(idx))
        {
            let active_color =
                config.get_color_unchecked(LapceTheme::EDITOR_FOREGROUND);
            match &parameter.label {
                ParameterLabel::Simple(name) => {
                    // TODO: test this
                    if let Some(offset_start) = sig.label.find(name) {
                        let offset_end = offset_start + name.len();

                        add_parameter_attr(
                            &mut builder,
                            &sig.label,
                            offset_start..offset_end,
                            active_color.clone(),
                        );
                    }
                    // Otherwise, if it failed to find it, we just ignore the bad indices
                }
                ParameterLabel::LabelOffsets(offsets) => {
                    // The offsets are utf16 into the `label`
                    let offset_start = offset_utf16_to_utf8(
                        sig.label.char_indices(),
                        offsets[0] as usize,
                    );
                    let offset_end = offset_utf16_to_utf8(
                        sig.label.char_indices(),
                        offsets[1] as usize,
                    );

                    add_parameter_attr(
                        &mut builder,
                        &sig.label,
                        offset_start..offset_end,
                        active_color.clone(),
                    );
                }
            }

            parameter
                .documentation
                .as_ref()
                .map(|doc| parse_documentation(doc, config))
        } else {
            None
        };

        // TODO: make this a code block of the current language

        (builder.build(), param_doc)
    };

    (label, param_doc, doc)
}

/// Add the attributes for the parameter range onto the [`RichTextBuilder`]
fn add_parameter_attr(
    builder: &mut RichTextBuilder,
    label: &str,
    range: Range<usize>,
    color: Color,
) {
    if range.start < range.end && label.get(range.clone()).is_some() {
        // TODO: This could be configurable by the user
        builder
            .add_attributes_for_range(range)
            .weight(FontWeight::BOLD)
            .text_color(color);
    }
}
