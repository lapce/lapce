use floem::cosmic_text::{
    Attrs, AttrsList, FamilyOwned, LineHeightValue, Style, TextLayout, Weight,
};
use lapce_core::{language::LapceLanguage, syntax::Syntax};
use lapce_xi_rope::Rope;
use lsp_types::MarkedString;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use smallvec::SmallVec;

use crate::config::{color::LapceColor, LapceConfig};

#[derive(Clone)]
pub enum MarkdownContent {
    Text(TextLayout),
    Image { url: String, title: String },
    Separator,
}

pub fn parse_markdown(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    let mut res = Vec::new();

    let mut current_text = String::new();
    let code_font_family: Vec<FamilyOwned> =
        FamilyOwned::parse_list(&config.editor.font_family).collect();

    let default_attrs = Attrs::new()
        .color(config.color(LapceColor::EDITOR_FOREGROUND))
        .font_size(config.ui.font_size() as f32)
        .line_height(LineHeightValue::Normal(line_height as f32));
    let mut attr_list = AttrsList::new(default_attrs);

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
            current_text.push('\n');
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
                        tracing::warn!("Mismatched markdown tag");
                        continue;
                    }

                    if let Some(attrs) = attribute_for_tag(
                        default_attrs,
                        &tag,
                        &code_font_family,
                        config,
                    ) {
                        attr_list
                            .add_span(start_offset..pos.max(start_offset), attrs);
                    }

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
                                &mut attr_list,
                                default_attrs.family(&code_font_family),
                                language,
                                &last_text,
                                start_offset,
                                config,
                            );
                            builder_dirty = true;
                        }
                        Tag::Image(_link_type, dest, title) => {
                            // TODO: Are there any link types that would change how the
                            // image is rendered?

                            if builder_dirty {
                                let mut text_layout = TextLayout::new();
                                text_layout.set_text(&current_text, attr_list);
                                res.push(MarkdownContent::Text(text_layout));
                                attr_list = AttrsList::new(default_attrs);
                                current_text.clear();
                                pos = 0;
                                builder_dirty = false;
                            }

                            res.push(MarkdownContent::Image {
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
                    tracing::warn!("Unbalanced markdown tag")
                }
            }
            Event::Text(text) => {
                if let Some((_, tag)) = tag_stack.last() {
                    if should_skip_text_in_tag(tag) {
                        continue;
                    }
                }
                current_text.push_str(&text);
                pos += text.len();
                last_text = text;
                builder_dirty = true;
            }
            Event::Code(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs.family(&code_font_family),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs
                        .family(&code_font_family)
                        .color(config.color(LapceColor::MARKDOWN_BLOCKQUOTE)),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            Event::HardBreak => {
                current_text.push('\n');
                pos += 1;
                builder_dirty = true;
            }
            Event::SoftBreak => {
                current_text.push(' ');
                pos += 1;
                builder_dirty = true;
            }
            Event::Rule => {}
            Event::FootnoteReference(_text) => {}
            Event::TaskListMarker(_text) => {}
        }
    }

    if builder_dirty {
        let mut text_layout = TextLayout::new();
        text_layout.set_text(&current_text, attr_list);
        res.push(MarkdownContent::Text(text_layout));
    }

    res
}

fn attribute_for_tag<'a>(
    default_attrs: Attrs<'a>,
    tag: &Tag,
    code_font_family: &'a [FamilyOwned],
    config: &LapceConfig,
) -> Option<Attrs<'a>> {
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
            Some(
                default_attrs
                    .font_size(font_size as f32)
                    .weight(Weight::BOLD),
            )
        }
        Tag::BlockQuote => Some(
            default_attrs
                .style(Style::Italic)
                .color(config.color(LapceColor::MARKDOWN_BLOCKQUOTE)),
        ),
        Tag::CodeBlock(_) => Some(default_attrs.family(code_font_family)),
        Tag::Emphasis => Some(default_attrs.style(Style::Italic)),
        Tag::Strong => Some(default_attrs.weight(Weight::BOLD)),
        // TODO: Strikethrough support
        Tag::Link(_link_type, _target, _title) => {
            // TODO: Link support
            Some(default_attrs.color(config.color(LapceColor::EDITOR_LINK)))
        }
        // All other tags are currently ignored
        _ => None,
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
    LapceLanguage::from_name(lang)
}

/// Highlight the text in a richtext builder like it was a markdown codeblock
pub fn highlight_as_code(
    attr_list: &mut AttrsList,
    default_attrs: Attrs,
    language: Option<LapceLanguage>,
    text: &str,
    start_offset: usize,
    config: &LapceConfig,
) {
    let syntax = language.map(Syntax::from_language);

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
                .and_then(|fg| config.style_color(fg))
            {
                attr_list.add_span(
                    start_offset + range.start..start_offset + range.end,
                    default_attrs.color(color),
                );
            }
        }
    }
}

pub fn from_marked_string(
    text: MarkedString,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
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

pub fn from_plaintext(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    let mut text_layout = TextLayout::new();
    text_layout.set_text(
        text,
        AttrsList::new(
            Attrs::new()
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(line_height as f32)),
        ),
    );
    vec![MarkdownContent::Text(text_layout)]
}
