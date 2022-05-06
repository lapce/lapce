use std::{cell::RefCell, rc::Rc, sync::Arc};

use druid::{ExtEventSink, FontStyle, FontWeight, Size, Target, WidgetId};
use lapce_core::syntax::Syntax;
use lapce_rpc::buffer::BufferId;
use lsp_types::{Hover, HoverContents, MarkedString, MarkupKind, Position};
use pulldown_cmark::Tag;
use xi_rope::Rope;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, EditorConfig, LapceTheme, Themes},
    document::Document,
    proxy::LapceProxy,
    rich_text::{AttributesAdder, RichText, RichTextBuilder},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverStatus {
    Inactive,
    Started,
    Done,
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
    /// Stores the actual size of the hover content
    pub content_size: Rc<RefCell<Size>>,

    /// The current hover string that is active, because there can be multiple for a single entry
    /// (such as if there is uncertainty over the exact version, such as in overloading or
    /// scripting languages where the declaration is uncertain)
    pub active_item_index: usize,
    /// The hover items that are currently loaded
    pub items: Arc<Vec<RichText>>,
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
            size: Size::new(600.0, 300.0),
            content_size: Rc::new(RefCell::new(Size::ZERO)),

            active_item_index: 0,
            items: Arc::new(Vec::new()),
        }
    }

    pub fn get_current_item(&self) -> Option<&RichText> {
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
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        doc: Arc<Document>,
        position: Position,
        hover_widget_id: WidgetId,
        event_sink: ExtEventSink,
        config: Arc<Config>,
    ) {
        let buffer_id = doc.id();
        let syntax = doc.syntax().cloned();
        let editor_config = config.editor.clone();
        let themes = config.themes.clone();

        proxy.get_hover(
            request_id,
            buffer_id,
            position,
            Box::new(move |result| {
                if let Ok(resp) = result {
                    if let Ok(resp) = serde_json::from_value::<Hover>(resp) {
                        let items = parse_hover_resp(
                            syntax.as_ref(),
                            resp,
                            &editor_config,
                            &themes,
                        );

                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHover(request_id, Arc::new(items)),
                            Target::Widget(hover_widget_id),
                        );
                    }
                }
            }),
        );
    }

    /// Receive the result of a hover request
    pub fn receive(&mut self, request_id: usize, items: Arc<Vec<RichText>>) {
        // If we've moved to inactive between the time the request started and now
        // or we've moved to a different request
        // then don't bother processing the received data
        if self.status == HoverStatus::Inactive || self.request_id != request_id {
            return;
        }

        self.status = HoverStatus::Done;
        self.items = items;
    }
}

impl Default for HoverData {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_hover_markdown(
    syntax: Option<&Syntax>,
    text: &str,
    config: &EditorConfig,
    themes: &Themes,
) -> RichText {
    use pulldown_cmark::{CowStr, Event, Options, Parser};

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
    let mut last_text = CowStr::from("");
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
                        config,
                        themes,
                    );

                    if let Tag::CodeBlock(_) = &tag {
                        if let Some(syntax) = syntax {
                            if let Some(styles) =
                                syntax.parse(0, Rope::from(&last_text), None).styles
                            {
                                for (range, style) in styles.iter() {
                                    if let Some(color) = style
                                        .fg_color
                                        .as_ref()
                                        .and_then(|fg| themes.style_color(fg))
                                    {
                                        builder
                                            .add_attributes_for_range(
                                                start_offset + range.start
                                                    ..start_offset + range.end,
                                            )
                                            .text_color(color.clone());
                                    }
                                }
                            }
                        }
                    }

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
                last_text = text;
            }
            Event::Code(text) => {
                builder.push(&text).font_family(config.font_family());
                code_block_indices.push(pos..(pos + text.len()));
                pos += text.len();
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                builder
                    .push(&text)
                    .font_family(config.font_family())
                    .text_color(
                        themes
                            .color(LapceTheme::MARKDOWN_BLOCKQUOTE)
                            .cloned()
                            .unwrap(),
                    );
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

    builder.build()
}

fn from_marked_string(
    syntax: Option<&Syntax>,
    text: MarkedString,
    config: &EditorConfig,
    themes: &Themes,
) -> RichText {
    match text {
        MarkedString::String(text) => {
            parse_hover_markdown(syntax, &text, config, themes)
        }
        // This is a short version of a code block
        MarkedString::LanguageString(code) => {
            // TODO: We could simply construct the MarkdownText directly
            // Simply construct the string as if it was written directly
            parse_hover_markdown(
                syntax,
                &format!("```{}\n{}\n```", code.language, code.value),
                config,
                themes,
            )
        }
    }
}

fn parse_hover_resp(
    syntax: Option<&Syntax>,
    hover: lsp_types::Hover,
    config: &EditorConfig,
    themes: &Themes,
) -> Vec<RichText> {
    match hover.contents {
        HoverContents::Scalar(text) => match text {
            MarkedString::String(text) => {
                vec![parse_hover_markdown(syntax, &text, config, themes)]
            }
            MarkedString::LanguageString(code) => vec![parse_hover_markdown(
                syntax,
                &format!("```{}\n{}\n```", code.language, code.value),
                config,
                themes,
            )],
        },
        HoverContents::Array(array) => array
            .into_iter()
            .map(|t| from_marked_string(syntax, t, config, themes))
            .collect(),
        HoverContents::Markup(content) => match content.kind {
            MarkupKind::PlainText => {
                let mut builder = RichTextBuilder::new();
                builder.push(&content.value);
                vec![builder.build()]
            }
            MarkupKind::Markdown => {
                vec![parse_hover_markdown(syntax, &content.value, config, themes)]
            }
        },
    }
}

fn add_attribute_for_tag(
    tag: &Tag,
    mut attrs: AttributesAdder,
    config: &EditorConfig,
    themes: &Themes,
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
            let font_size = font_scale * config.font_size as f64;
            attrs.size(font_size).weight(FontWeight::BOLD);
        }
        Tag::BlockQuote => {
            attrs.style(FontStyle::Italic).text_color(
                themes
                    .color(LapceTheme::MARKDOWN_BLOCKQUOTE)
                    .cloned()
                    .unwrap(),
            );
        }
        // TODO: We could use the language paired with treesitter to highlight the code
        // within code blocks.
        Tag::CodeBlock(_) => {
            attrs.font_family(config.font_family());
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
            attrs
                .underline(true)
                .text_color(themes.color(LapceTheme::EDITOR_LINK).cloned().unwrap());
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
