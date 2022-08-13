use std::{cmp::Ordering, fmt::Display, sync::Arc};

use anyhow::Error;
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    theme, ArcStr, BoxConstraints, Command, Data, Env, Event, EventCtx,
    FontDescriptor, FontFamily, FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout, UpdateCtx,
    Widget, WidgetId, WidgetPod,
};
use itertools::Itertools;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    completion::{CompletionData, CompletionStatus, ScoredCompletionItem},
    config::{Config, LapceTheme},
    data::LapceTabData,
    markdown::parse_markdown,
    rich_text::{RichText, RichTextBuilder},
};
use lsp_types::{CompletionItem, Documentation, MarkupKind};
use regex::Regex;
use std::str::FromStr;

use crate::{
    scroll::{LapceIdentityWrapper, LapceScroll},
    svg::completion_svg,
};

#[derive(Debug)]
struct Snippet {
    elements: Vec<SnippetElement>,
}

impl Snippet {
    fn extract_elements(
        s: &str,
        pos: usize,
        escs: Vec<&str>,
        loose_escs: Vec<&str>,
    ) -> (Vec<SnippetElement>, usize) {
        let mut elements = Vec::new();
        let mut pos = pos;
        loop {
            if s.len() == pos {
                break;
            } else if let Some((ele, end)) = Self::extract_tabstop(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) = Self::extract_placeholder(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) =
                Self::extract_text(s, pos, escs.clone(), loose_escs.clone())
            {
                elements.push(ele);
                pos = end;
            } else {
                break;
            }
        }
        (elements, pos)
    }

    fn extract_tabstop(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        for re in &[
            Regex::new(r#"^\$(\d+)"#).unwrap(),
            Regex::new(r#"^\$\{(\d+)\}"#).unwrap(),
        ] {
            if let Some(caps) = re.captures(&s[pos..]) {
                let end = pos + re.find(&s[pos..])?.end();
                let m = caps.get(1)?;
                let n = m.as_str().parse::<usize>().ok()?;
                return Some((SnippetElement::Tabstop(n), end));
            }
        }

        None
    }

    fn extract_placeholder(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        let re = Regex::new(r#"^\$\{(\d+):(.*?)\}"#).unwrap();
        let end = pos + re.find(&s[pos..])?.end();

        let caps = re.captures(&s[pos..])?;

        let tab = caps.get(1)?.as_str().parse::<usize>().ok()?;

        let m = caps.get(2)?;
        let content = m.as_str();
        if content.is_empty() {
            return Some((
                SnippetElement::PlaceHolder(
                    tab,
                    vec![SnippetElement::Text("".to_string())],
                ),
                end,
            ));
        }
        let (els, pos) =
            Self::extract_elements(s, pos + m.start(), vec!["$", "}", "\\"], vec![]);
        Some((SnippetElement::PlaceHolder(tab, els), pos + 1))
    }

    fn extract_text(
        s: &str,
        pos: usize,
        escs: Vec<&str>,
        loose_escs: Vec<&str>,
    ) -> Option<(SnippetElement, usize)> {
        let mut s = &s[pos..];
        let mut ele = "".to_string();
        let mut end = pos;

        while !s.is_empty() {
            if s.len() >= 2 {
                let esc = &s[..2];
                let mut new_escs = escs.clone();
                new_escs.extend_from_slice(&loose_escs);

                if new_escs
                    .iter()
                    .map(|e| format!("\\{}", e))
                    .any(|x| x == *esc)
                {
                    ele = ele + &s[1..2];
                    end += 2;
                    s = &s[2..];
                    continue;
                }
            }
            if escs.contains(&&s[0..1]) {
                break;
            }
            ele = ele + &s[0..1];
            end += 1;
            s = &s[1..];
        }
        if ele.is_empty() {
            return None;
        }
        Some((SnippetElement::Text(ele), end))
    }
}

impl FromStr for Snippet {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (elements, _) = Self::extract_elements(s, 0, vec!["$", "\\"], vec!["}"]);
        Ok(Snippet { elements })
    }
}

impl Display for Snippet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = self.elements.iter().map(|e| e.to_string()).join("");
        f.write_str(&text)
    }
}

#[derive(Debug)]
enum SnippetElement {
    Text(String),
    PlaceHolder(usize, Vec<SnippetElement>),
    Tabstop(usize),
}

impl Display for SnippetElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SnippetElement::Text(text) => f.write_str(text),
            SnippetElement::PlaceHolder(tab, elements) => {
                let elements = elements.iter().map(|e| e.to_string()).join("");
                write!(f, "${{{}:{}}}", tab, elements)
            }
            SnippetElement::Tabstop(tab) => write!(f, "${}", tab),
        }
    }
}

pub struct CompletionContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    completion: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScroll<LapceTabData, Completion>>,
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
        let completion = LapceIdentityWrapper::wrap(
            LapceScroll::new(Completion::new()).vertical(),
            data.scroll_id,
        );
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
        let width = ctx.size().width;
        let line_height = data.config.editor.line_height as f64;
        let rect = Size::new(width, line_height)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                data.completion.index as f64 * line_height,
            ));
        if self
            .completion
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

    /// Like [`Self::ensure_item_visible`] but instead making so that it is at the very top of the display
    /// rather than just scrolling the minimal distance to make it visible.
    pub fn ensure_item_top_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceTabData,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let point = Point::new(0.0, data.completion.index as f64 * line_height);
        if self.completion.widget_mut().inner_mut().scroll_to(point) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    fn update_documentation(&mut self, data: &LapceTabData) {
        let documentation = if data.config.editor.completion_show_documentation {
            let current_item = (!data.completion.is_empty())
                .then(|| data.completion.current_item());

            current_item.and_then(|item| item.documentation.as_ref())
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
                match command {
                    LapceUICommand::UpdateCompletion(request_id, input, resp) => {
                        let completion = Arc::make_mut(&mut data.completion);
                        completion.receive(
                            *request_id,
                            input.to_owned(),
                            resp.to_owned(),
                        );
                    }
                    LapceUICommand::CancelCompletion(request_id) => {
                        if data.completion.request_id == *request_id {
                            let completion = Arc::make_mut(&mut data.completion);
                            completion.cancel();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.completion.event(ctx, event, data, env);
        self.documentation.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.completion.lifecycle(ctx, event, data, env);
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
        }

        if old_data.completion.input != data.completion.input
            || old_data.completion.request_id != data.completion.request_id
            || old_data.completion.status != data.completion.status
            || !old_data
                .completion
                .current_items()
                .same(data.completion.current_items())
            || !old_data
                .completion
                .filtered_items
                .same(&data.completion.filtered_items)
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

        if old_completion.index != completion.index {
            self.ensure_item_visible(ctx, data, env);
            self.update_documentation(data);
            ctx.request_paint();
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
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let completion_size = data.completion.size;
        let bc = BoxConstraints::new(Size::ZERO, completion_size);
        self.completion_content_size = self.completion.layout(ctx, &bc, data, env);
        self.completion.set_origin(ctx, data, env, Point::ZERO);

        // Position the documentation over the current completion item to the right
        let documentation_size = data.completion.documentation_size;
        let bc = BoxConstraints::new(Size::ZERO, documentation_size);
        self.documentation_content_size =
            self.documentation.layout(ctx, &bc, data, env);
        self.documentation.set_origin(
            ctx,
            data,
            env,
            Point::new(self.completion_content_size.width, 0.0),
        );

        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));

        Size::new(
            completion_size.width + documentation_size.width,
            completion_size.height.max(documentation_size.height),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.completion.status != CompletionStatus::Inactive
            && data.completion.len() > 0
        {
            let rect = self.completion_content_size.to_rect();
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
            self.completion.paint(ctx, data, env);
            self.documentation.paint(ctx, data, env);
        }
    }
}

pub struct Completion {}

impl Completion {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Completion {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for Completion {
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
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let line_height = data.config.editor.line_height as f64;
        let height = data.completion.len();
        let height = height as f64 * line_height;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if data.completion.status == CompletionStatus::Inactive {
            return;
        }
        let line_height = data.config.editor.line_height as f64;
        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let _input = &data.completion.input;
        let items: &Vec<ScoredCompletionItem> = data.completion.current_items();

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::COMPLETION_BACKGROUND),
        );

        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;

        for line in start_line..end_line {
            if line >= items.len() {
                break;
            }

            if line == data.completion.index {
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, line as f64 * line_height))
                        .with_size(Size::new(size.width, line_height)),
                    data.config
                        .get_color_unchecked(LapceTheme::COMPLETION_CURRENT),
                );
            }

            let item = &items[line];

            let y = line_height * line as f64 + 5.0;

            if let Some((svg, color)) = completion_svg(item.item.kind, &data.config)
            {
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
                let rect =
                    Size::new(width, height).to_rect().with_origin(Point::new(
                        (line_height - width) / 2.0,
                        (line_height - height) / 2.0 + line_height * line as f64,
                    ));
                ctx.draw_svg(&svg, rect, Some(&color));
            }

            let focus_color =
                data.config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);
            let content = item.item.label.as_str();
            let point = Point::new(line_height + 5.0, y);

            let mut text_layout = ctx
                .text()
                .new_text_layout(content.to_string())
                .font(
                    FontFamily::new_unchecked(
                        data.config.editor.font_family.clone(),
                    ),
                    data.config.editor.font_size as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                );
            for i in &item.indices {
                let i = *i;
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::TextColor(focus_color.clone()),
                );
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::Weight(FontWeight::BOLD),
                );
            }
            let text_layout = text_layout.build().unwrap();
            ctx.draw_text(&text_layout, point);
        }
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

fn parse_documentation(doc: &Documentation, config: &Config) -> RichText {
    match doc {
        // We assume this is plain text
        Documentation::String(text) => {
            let mut builder = RichTextBuilder::new();
            builder.set_line_height(1.5);
            builder.push(text);
            builder.build()
        }
        Documentation::MarkupContent(content) => match content.kind {
            MarkupKind::PlainText => {
                let mut builder = RichTextBuilder::new();
                builder.set_line_height(1.5);
                builder.push(&content.value);
                builder.build()
            }
            MarkupKind::Markdown => parse_markdown(&content.value, config),
        },
    }
}

#[derive(Clone)]
pub struct CompletionState {
    pub widget_id: WidgetId,
    pub items: Vec<ScoredCompletionItem>,
    pub input: String,
    pub offset: usize,
    pub index: usize,
    pub scroll_offset: f64,
}

impl CompletionState {
    pub fn new() -> CompletionState {
        CompletionState {
            widget_id: WidgetId::next(),
            items: Vec::new(),
            input: "".to_string(),
            offset: 0,
            index: 0,
            scroll_offset: 0.0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.iter().filter(|i| i.score != 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn current_items(&self) -> Vec<&ScoredCompletionItem> {
        self.items.iter().filter(|i| i.score != 0).collect()
    }

    pub fn clear(&mut self) {
        self.input = "".to_string();
        self.items = Vec::new();
        self.offset = 0;
        self.index = 0;
        self.scroll_offset = 0.0;
    }

    pub fn cancel(&mut self, ctx: &mut EventCtx) {
        self.clear();
        self.request_paint(ctx);
    }

    pub fn request_paint(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::RequestPaint,
            Target::Widget(self.widget_id),
        ));
    }

    pub fn update(&mut self, input: String, completion_items: Vec<CompletionItem>) {
        self.items = completion_items
            .iter()
            .enumerate()
            .map(|(index, item)| ScoredCompletionItem {
                item: item.to_owned(),
                score: -1 - index as i64,
                label_score: -1 - index as i64,
                indices: Vec::new(),
            })
            .collect();
        self.items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        self.input = input;
    }
}

impl Default for CompletionState {
    fn default() -> Self {
        Self::new()
    }
}
