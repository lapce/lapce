use std::{ops::Range, sync::Arc};

use druid::{
    piet::TextStorage as PietTextStorage,
    text::{AttributesAdder, RichText, RichTextBuilder, TextStorage},
    theme, ArcStr, Color, Data, Env, ExtEventSink, FontFamily, FontStyle,
    FontWeight, PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout,
    WidgetId,
};
use lapce_rpc::buffer::BufferId;
use lsp_types::{
    Hover, HoverContents, MarkedString, MarkupContent, MarkupKind, Position,
};
use pulldown_cmark::Tag;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::LapceTabData,
    proxy::LapceProxy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverStatus {
    Inactive,
    Started,
}

#[derive(Clone)]
pub struct HoverData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    /// The current request status
    pub status: HoverStatus,
    /// The offset that is currently being handled, if there is one
    pub offset: usize,
    /// The buffer that this hover is for
    pub buffer_id: BufferId,
    /// A counter to keep track of the active requests
    pub request_id: usize,
    /// Stores the size of the hover box
    pub size: Size,

    /// The current hover string that is active, because there can be multiple for a single entry
    /// (such as if there is uncertainty over the exact version, such as in overloading or
    /// scripting languages where the declaration is uncertain)
    pub active_item_index: usize,
    /// The hover items that are currently loaded
    pub items: Arc<Vec<HoverItem>>,
}

impl HoverData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            status: HoverStatus::Inactive,
            offset: 0,
            buffer_id: BufferId(0),
            request_id: 0,
            // TODO: make this configurable by themes
            size: Size::new(500.0, 300.0),

            active_item_index: 0,
            items: Arc::new(Vec::new()),
        }
    }

    pub fn get_current_item(&self) -> Option<&HoverItem> {
        self.items.get(self.active_item_index)
    }

    /// The length of the current hover items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Move to the next hover item
    pub fn next(&mut self) {
        // If there is an item after the current one, then actually change
        if self.active_item_index + 1 < self.len() {
            self.active_item_index += 1;
        }
    }

    /// Move to the previous hover item
    pub fn previous(&mut self) {
        if self.active_item_index > 0 {
            self.active_item_index -= 1;
        }
    }

    /// Cancel the current hover information, clearing out held data
    pub fn cancel(&mut self) {
        if self.status == HoverStatus::Inactive {
            return;
        }

        self.status = HoverStatus::Inactive;
        Arc::make_mut(&mut self.items).clear();
        self.active_item_index = 0;
    }

    /// Send a request to update the hover at the given position anad file
    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        hover_widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        proxy.get_hover(
            request_id,
            buffer_id,
            position,
            Box::new(move |result| {
                if let Ok(resp) = result {
                    if let Ok(resp) = serde_json::from_value::<Hover>(resp) {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHover(request_id, resp),
                            Target::Widget(hover_widget_id),
                        );
                    }
                }
            }),
        );
    }

    /// Receive the result of a hover request
    pub fn receive(
        &mut self,
        style: &HoverTextStyle,
        request_id: usize,
        resp: lsp_types::Hover,
    ) {
        // If we've moved to inactive between the time the request started and now
        // or we've moved to a different request
        // then don't bother processing the received data
        if self.status == HoverStatus::Inactive || self.request_id != request_id {
            return;
        }

        let items = Arc::make_mut(&mut self.items);
        // Extract the items in the format that we want them to be in
        *items = match resp.contents {
            HoverContents::Scalar(text) => {
                vec![HoverItem::from_marked_string(text, style)]
            }
            HoverContents::Array(entries) => entries
                .into_iter()
                .map(|text| HoverItem::from_marked_string(text, style))
                .collect(),
            HoverContents::Markup(content) => {
                vec![HoverItem::from_markup_content(content, style)]
            }
        };
    }
}

impl Default for HoverData {
    fn default() -> Self {
        Self::new()
    }
}

/// Styling information for generated hover content
pub struct HoverTextStyle {
    /// Font size of normal text
    base_font_size: f64,
    link_color: Color,
    blockquote_color: Color,
}
impl HoverTextStyle {
    /// Extract the needed data from [`LapceTabData`]
    pub fn from_data(data: &LapceTabData) -> Self {
        Self {
            base_font_size: data.config.editor.font_size as f64,
            link_color: data
                .config
                .get_color_unchecked(LapceTheme::EDITOR_LINK)
                .clone(),
            blockquote_color: data
                .config
                .get_color_unchecked(LapceTheme::MARKDOWN_BLOCKQUOTE)
                .clone(),
        }
    }
}

/// A hover entry
/// This is separate from the lsp-types version to make using it more direct
#[derive(Clone)]
pub enum HoverItem {
    /// Rendered (druid) markdown text
    Markdown(MarkdownText),
    PlainText(String),
}
impl HoverItem {
    pub fn as_markdown_text(&self) -> MarkdownText {
        match self {
            HoverItem::Markdown(text) => text.clone(),
            // TODO: We could store PlainText as an ArcStr/related type to make these clones
            // significantly cheaper
            HoverItem::PlainText(text) => MarkdownText::new(
                RichText::new(ArcStr::from(text.as_str())),
                Vec::new(),
            ),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            HoverItem::Markdown(x) => x.as_str(),
            HoverItem::PlainText(x) => x.as_str(),
        }
    }

    fn from_marked_string(text: MarkedString, style: &HoverTextStyle) -> Self {
        match text {
            MarkedString::String(text) => {
                HoverItem::Markdown(parse_markdown(&text, style))
            }
            // This is a short version of a code block
            MarkedString::LanguageString(code) => {
                // TODO: We could simply construct the MarkdownText directly
                // Simply construct the string as if it was written directly
                HoverItem::Markdown(parse_markdown(
                    &format!("```{}\n{}\n```", code.language, code.value),
                    style,
                ))
            }
        }
    }

    fn from_markup_content(content: MarkupContent, style: &HoverTextStyle) -> Self {
        match content.kind {
            MarkupKind::Markdown => {
                HoverItem::Markdown(parse_markdown(&content.value, style))
            }
            MarkupKind::PlainText => HoverItem::PlainText(content.value),
        }
    }
}

// TODO: Markdown parsing/rendering could be moved to its own file/module/crate so that it can be
// used for more than just hovering (ex: markdown preview)

#[derive(Data, Clone)]
pub struct MarkdownText {
    /// Rendered rich text
    text: RichText,
    /// Indices into the underlying text that code blocks start and begin at
    /// This lets us calculate a rectangle surrounding the text to apply a background too, in order
    /// to differentiate it from normal text more.
    /// Note that this covers multiline and inline codeblocks
    #[data(eq)]
    code_block_indices: Vec<Range<usize>>,
}
impl MarkdownText {
    fn new(text: RichText, code_block_indices: Vec<Range<usize>>) -> Self {
        Self {
            text,
            code_block_indices,
        }
    }

    pub fn empty() -> Self {
        Self {
            text: RichText::new(ArcStr::from("")),
            code_block_indices: Vec::new(),
        }
    }

    /// Draw the markdown to the paint ctx, using the layout
    pub fn draw(
        &self,
        ctx: &mut PaintCtx,
        env: &Env,
        origin: Point,
        layout: &TextLayout<MarkdownText>,
        config: &Config,
    ) {
        let rect = ctx.region().bounding_box();

        let code_background_color = config
            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
            .clone();
        for range in &self.code_block_indices {
            // TODO: We could make a function to get just p0, since we don't need the other
            // calculated point
            let start_point = layout.cursor_line_for_text_position(range.start).p0;
            let end_point = layout.cursor_line_for_text_position(range.end).p0;

            // If the end_point and start_point x positions are the same (typically 0), then
            // they're probably a multiline code block.
            let end_x = if end_point.x == start_point.x {
                // Use the entire width, avoiding the scrollbar to make it appear nicer
                rect.x1
                    - env.get(theme::SCROLLBAR_WIDTH)
                    - env.get(theme::SCROLLBAR_PAD)
            } else {
                origin.x + end_point.x
            };

            let bg_rect = Rect::new(
                origin.x + start_point.x,
                origin.y + start_point.y,
                end_x,
                origin.y + end_point.y,
            );
            ctx.fill(bg_rect, &code_background_color);
        }
        layout.draw(ctx, origin);
    }
}
// Let this be treated as Text
impl PietTextStorage for MarkdownText {
    fn as_str(&self) -> &str {
        self.text.as_str()
    }
}
impl TextStorage for MarkdownText {
    fn add_attributes(
        &self,
        builder: druid::piet::PietTextLayoutBuilder,
        env: &druid::Env,
    ) -> druid::piet::PietTextLayoutBuilder {
        self.text.add_attributes(builder, env)
    }

    fn env_update(&self, ctx: &druid::text::EnvUpdateCtx) -> bool {
        self.text.env_update(ctx)
    }

    fn links(&self) -> &[druid::text::Link] {
        self.text.links()
    }
}

/// Parse the given markdown into renderable druid rich text with the given style information
fn parse_markdown(text: &str, style: &HoverTextStyle) -> MarkdownText {
    use pulldown_cmark::{Event, Options, Parser};

    let mut builder = RichTextBuilder::new();

    // Our position within the text
    let mut pos = 0;

    // TODO: (minor): This could use a smallvec since most tags are probably not that nested
    // Stores the current tags (like italics/bold/strikethrough) so that they can be nested
    let mut tag_stack = Vec::new();

    let mut code_block_indices = Vec::new();

    // Construct the markdown parser. We enable most of the options in order to provide the most
    // compatibility that pulldown_cmark allows.
    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES,
    );
    for event in parser {
        match event {
            Event::Start(tag) => {
                tag_stack.push((pos, tag));
            }
            Event::End(end_tag) => {
                if let Some((start_offset, tag)) = tag_stack.pop() {
                    if end_tag != tag {
                        log::warn!("Mismatched markdown tag");
                        continue;
                    }

                    if let Tag::CodeBlock(_kind) = &tag {
                        code_block_indices.push(start_offset..pos);
                    }

                    add_attribute_for_tag(
                        &tag,
                        builder.add_attributes_for_range(start_offset..pos),
                        style,
                    );

                    if should_add_newline_after_tag(&tag) {
                        builder.push("\n");
                        pos += 1;
                    }
                } else {
                    log::warn!("Unbalanced markdown tag")
                }
            }
            Event::Text(text) => {
                builder.push(&text);
                pos += text.len();
            }
            Event::Code(text) => {
                builder.push(&text).font_family(FontFamily::MONOSPACE);
                code_block_indices.push(pos..(pos + text.len()));
                pos += text.len();
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                builder
                    .push(&text)
                    .font_family(FontFamily::MONOSPACE)
                    .text_color(style.blockquote_color.clone());
                pos += text.len();
            }
            Event::HardBreak => {
                builder.push("\n");
                pos += 1;
            }
            Event::SoftBreak => {
                builder.push(" ");
                pos += 1;
            }
            Event::Rule => {}
            Event::FootnoteReference(_text) => {}
            Event::TaskListMarker(_text) => {}
        }
    }

    MarkdownText::new(builder.build(), code_block_indices)
}

fn add_attribute_for_tag(
    tag: &Tag,
    mut attrs: AttributesAdder,
    HoverTextStyle {
        base_font_size,
        link_color,
        blockquote_color,
    }: &HoverTextStyle,
) {
    use pulldown_cmark::HeadingLevel;
    match tag {
        Tag::Heading(level, _, _) => {
            // The size calculations are based on the em values given at
            // https://drafts.csswg.org/css2/#html-stylesheet
            let font_scale = match level {
                HeadingLevel::H1 => 2.0,
                HeadingLevel::H2 => 1.5,
                HeadingLevel::H3 => 1.17,
                HeadingLevel::H4 => 1.0,
                HeadingLevel::H5 => 0.83,
                HeadingLevel::H6 => 0.75,
            };
            let font_size = font_scale * base_font_size;
            attrs.size(font_size).weight(FontWeight::BOLD);
        }
        Tag::BlockQuote => {
            attrs
                .style(FontStyle::Italic)
                .text_color(blockquote_color.clone());
        }
        // TODO: We could use the language paired with treesitter to highlight the code
        // within code blocks.
        Tag::CodeBlock(_) => {
            attrs.font_family(FontFamily::MONOSPACE);
        }
        Tag::Emphasis => {
            attrs.style(FontStyle::Italic);
        }
        Tag::Strong => {
            attrs.weight(FontWeight::BOLD);
        }
        // TODO: Strikethrough support
        Tag::Link(_link_type, _target, _title) => {
            // TODO: Link support
            attrs.underline(true).text_color(link_color.clone());
        }
        // All other tags are currently ignored
        _ => {}
    }
}

/// Decides whether newlines should be added after a specific markdown tag
fn should_add_newline_after_tag(tag: &Tag) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link(..)
    )
}
