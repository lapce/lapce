use std::str::FromStr;

use druid::{FontStyle, FontWeight};
use lapce_core::{
    language::LapceLanguage,
    syntax::{highlight::HighlightIssue, Syntax},
};
use lapce_xi_rope::Rope;
use lsp_types::{Documentation, MarkedString, MarkupKind};
use pulldown_cmark::{CodeBlockKind, Tag};
use smallvec::SmallVec;

use crate::{
    config::{LapceConfig, LapceTheme},
    rich_text::{AttributesAdder, RichText, RichTextBuilder},
};

pub mod layout_content;

#[derive(Clone)]
pub enum Content {
    Text(RichText),
    Image { url: String, title: String },
    Separator,
}

/// Parse the LSP documentation structure
pub fn parse_documentation(
    doc: &Documentation,
    config: &LapceConfig,
) -> Vec<Content> {
    match doc {
        // We assume this is plain text
        Documentation::String(text) => {
            let mut builder = RichTextBuilder::new();
            builder.set_line_height(1.5);
            builder.push(text);
            vec![Content::Text(builder.build())]
        }
        Documentation::MarkupContent(content) => match content.kind {
            MarkupKind::PlainText => {
                let mut builder = RichTextBuilder::new();
                builder.set_line_height(1.5);
                builder.push(&content.value);
                vec![Content::Text(builder.build())]
            }
            MarkupKind::Markdown => parse_markdown(&content.value, 1.5, config),
        },
    }
}

// TODO: It would be nicer if this returned an iterator
pub fn parse_markdown(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
) -> Vec<Content> {
    use pulldown_cmark::{CowStr, Event, Options, Parser};

    let mut res = Vec::new();

    let mut builder = RichTextBuilder::new();
    builder.set_line_height(line_height);

    let mut builder_dirty = false;

    let mut pos = 0;

    let mut tag_stack: SmallVec<[(usize, Tag); 4]> = SmallVec::new();

    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES,
    );
    let mut last_text = CowStr::from("");
    // Whether we should add a newline on the next entry
    // This is used so that we don't emit newlines at the very end of the generation
    let mut add_newline = false;
    for event in parser {
        // Add the newline since we're going to be outputting more
        if add_newline {
            builder.push("\n");
            builder_dirty = true;
            pos += 1;
            add_newline = false;
        }

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

                    add_attribute_for_tag(
                        &tag,
                        builder.add_attributes_for_range(start_offset..pos),
                        config,
                    );

                    if should_add_newline_after_tag(&tag) {
                        add_newline = true;
                    }

                    match &tag {
                        Tag::CodeBlock(kind) => {
                            let language =
                                if let CodeBlockKind::Fenced(language) = kind {
                                    md_language_to_lapce_language(language)
                                } else {
                                    None
                                };

                            highlight_as_code(
                                &mut builder,
                                config,
                                language,
                                &last_text,
                                start_offset,
                            );
                            builder_dirty = true;
                        }
                        Tag::Image(_link_type, dest, title) => {
                            // TODO: Are there any link types that would change how the
                            // image is rendered?

                            if builder_dirty {
                                res.push(Content::Text(builder.build()));
                                builder = RichTextBuilder::new();
                                builder.set_line_height(line_height);
                                pos = 0;
                                builder_dirty = false;
                            }

                            res.push(Content::Image {
                                url: dest.to_string(),
                                title: title.to_string(),
                            });
                        }
                        _ => {
                            // Presumably?
                            builder_dirty = true;
                        }
                    }
                } else {
                    log::warn!("Unbalanced markdown tag")
                }
            }
            Event::Text(text) => {
                if let Some((_, tag)) = tag_stack.last() {
                    if should_skip_text_in_tag(tag) {
                        continue;
                    }
                }
                builder.push(&text);
                pos += text.len();
                last_text = text;
                builder_dirty = true;
            }
            Event::Code(text) => {
                builder.push(&text).font_family(config.editor.font_family());
                pos += text.len();
                builder_dirty = true;
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                builder
                    .push(&text)
                    .font_family(config.editor.font_family())
                    .text_color(
                        config
                            .get_color_unchecked(LapceTheme::MARKDOWN_BLOCKQUOTE)
                            .clone(),
                    );
                pos += text.len();
                builder_dirty = true;
            }
            Event::HardBreak => {
                builder.push("\n");
                pos += 1;
                builder_dirty = true;
            }
            Event::SoftBreak => {
                builder.push(" ");
                pos += 1;
                builder_dirty = true;
            }
            Event::Rule => {}
            Event::FootnoteReference(_text) => {}
            Event::TaskListMarker(_text) => {}
        }
    }

    if builder_dirty {
        res.push(Content::Text(builder.build()));
    }

    res
}

/// Highlight the text in a richtext builder like it was a markdown codeblock
pub fn highlight_as_code(
    builder: &mut RichTextBuilder,
    config: &LapceConfig,
    language: Option<LapceLanguage>,
    text: &str,
    start_offset: usize,
) {
    let syntax = language
        .map(Syntax::from_language)
        .unwrap_or(Err(HighlightIssue::NotAvailable));

    let styles = syntax
        .map(|mut syntax| {
            syntax.parse(0, Rope::from(text), None);
            syntax.styles
        })
        .unwrap_or(None);

    if let Some(styles) = styles {
        for (range, style) in styles.iter() {
            if let Some(color) = style
                .fg_color
                .as_ref()
                .and_then(|fg| config.get_style_color(fg))
            {
                builder
                    .add_attributes_for_range(
                        start_offset + range.start..start_offset + range.end,
                    )
                    .text_color(color.clone());
            }
        }
    }
}

pub fn from_marked_string(text: MarkedString, config: &LapceConfig) -> Vec<Content> {
    match text {
        MarkedString::String(text) => parse_markdown(&text, 1.5, config),
        // This is a short version of a code block
        MarkedString::LanguageString(code) => {
            // TODO: We could simply construct the MarkdownText directly
            // Simply construct the string as if it was written directly
            parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                1.5,
                config,
            )
        }
    }
}

fn add_attribute_for_tag(
    tag: &Tag,
    mut attrs: AttributesAdder,
    config: &LapceConfig,
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
            let font_size = font_scale * config.ui.font_size() as f64;
            attrs.size(font_size).weight(FontWeight::BOLD);
        }
        Tag::BlockQuote => {
            attrs.style(FontStyle::Italic).text_color(
                config
                    .get_color_unchecked(LapceTheme::MARKDOWN_BLOCKQUOTE)
                    .clone(),
            );
        }
        Tag::CodeBlock(_) => {
            attrs.font_family(config.editor.font_family());
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
            attrs.underline(true).text_color(
                config.get_color_unchecked(LapceTheme::EDITOR_LINK).clone(),
            );
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

/// Whether it should skip the text node after a specific tag  
/// For example, images are skipped because it emits their title as a separate text node.  
fn should_skip_text_in_tag(tag: &Tag) -> bool {
    matches!(tag, Tag::Image(..))
}

fn md_language_to_lapce_language(lang: &str) -> Option<LapceLanguage> {
    // TODO: There are many other names commonly used that should be supported
    LapceLanguage::from_str(lang).ok()
}
