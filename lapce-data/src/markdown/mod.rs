use std::str::FromStr;

use druid::{FontStyle, FontWeight};
use lapce_core::{language::LapceLanguage, syntax::Syntax};
use lsp_types::MarkedString;
use pulldown_cmark::{CodeBlockKind, Tag};
use smallvec::SmallVec;
use xi_rope::Rope;

use crate::{
    config::{Config, LapceTheme},
    rich_text::{AttributesAdder, RichText, RichTextBuilder},
};

pub fn parse_markdown(text: &str, config: &Config) -> RichText {
    use pulldown_cmark::{CowStr, Event, Options, Parser};

    let mut builder = RichTextBuilder::new();
    builder.set_line_height(1.5);

    // Our position within the text
    let mut pos = 0;

    let mut tag_stack: SmallVec<[(usize, Tag); 4]> = SmallVec::new();

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
    // Whether we should add a newline on the next entry
    // This is used so that we don't emit newlines at the very end of the generation
    let mut add_newline = false;
    for event in parser {
        // Add the newline since we're going to be outputting more
        if add_newline {
            builder.push("\n");
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

                    if let Tag::CodeBlock(_kind) = &tag {
                        code_block_indices.push(start_offset..pos);
                    }

                    add_attribute_for_tag(
                        &tag,
                        builder.add_attributes_for_range(start_offset..pos),
                        config,
                    );

                    if let Tag::CodeBlock(kind) = &tag {
                        let language = if let CodeBlockKind::Fenced(language) = kind
                        {
                            md_language_to_lapce_language(language)
                        } else {
                            None
                        };

                        let syntax = language.map(Syntax::from_language);

                        let styles = syntax.and_then(|mut syntax| {
                            syntax.parse(0, Rope::from(&last_text), None);
                            syntax.styles
                        });

                        if let Some(styles) = styles {
                            for (range, style) in styles.iter() {
                                if let Some(color) = style
                                    .fg_color
                                    .as_ref()
                                    .and_then(|fg| config.get_style_color(fg))
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

                    if should_add_newline_after_tag(&tag) {
                        add_newline = true;
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
                builder.push(&text).font_family(config.editor.font_family());
                code_block_indices.push(pos..(pos + text.len()));
                pos += text.len();
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

pub fn from_marked_string(text: MarkedString, config: &Config) -> RichText {
    match text {
        MarkedString::String(text) => parse_markdown(&text, config),
        // This is a short version of a code block
        MarkedString::LanguageString(code) => {
            // TODO: We could simply construct the MarkdownText directly
            // Simply construct the string as if it was written directly
            parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                config,
            )
        }
    }
}

fn add_attribute_for_tag(tag: &Tag, mut attrs: AttributesAdder, config: &Config) {
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

fn md_language_to_lapce_language(lang: &str) -> Option<LapceLanguage> {
    // TODO: There are many other names commonly used that should be supported
    LapceLanguage::from_str(lang).ok()
}
