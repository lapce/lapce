use std::{cmp::Ordering, collections::HashMap, fmt::Display, sync::Arc};

use anyhow::Error;
use bit_vec::BitVec;
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    scroll_component::ScrollComponent,
    theme,
    widget::SvgData,
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx,
    ExtEventSink, FontWeight, Insets, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    Point, Rect, RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2, Widget,
    WidgetExt, WidgetId, WidgetPod, WindowId,
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
use lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};
use regex::Regex;
use std::str::FromStr;

use crate::{
    buffer::BufferId,
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::LapceTabData,
    explorer::ICONS_DIR,
    movement::Movement,
    proxy::LapceProxy,
    scroll::{LapceIdentityWrapper, LapceScrollNew},
    state::LapceUIState,
    state::LAPCE_APP_STATE,
    svg::Svg,
    theme::LapceTheme,
};

#[derive(Debug)]
pub struct Snippet {
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
        if content == "" {
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

        while s.len() > 0 {
            if s.len() >= 2 {
                let esc = &s[..2];
                let mut new_escs = escs.clone();
                new_escs.extend_from_slice(&loose_escs);
                let new_escs: Vec<String> =
                    new_escs.iter().map(|e| format!("\\{}", e)).collect();
                if new_escs.contains(&esc.to_string()) {
                    ele = ele + &s[1..2].to_string();
                    end += 2;
                    s = &s[2..];
                    continue;
                }
            }
            if escs.contains(&&s[0..1]) {
                break;
            }
            ele = ele + &s[0..1].to_string();
            end += 1;
            s = &s[1..];
        }
        if ele.len() == 0 {
            return None;
        }
        Some((SnippetElement::Text(ele), end))
    }

    pub fn text(&self) -> String {
        self.elements.iter().map(|e| e.text()).join("")
    }

    pub fn tabs(&self, pos: usize) -> Vec<(usize, (usize, usize))> {
        Self::elements_tabs(&self.elements, pos)
    }

    pub fn elements_tabs(
        elements: &[SnippetElement],
        start: usize,
    ) -> Vec<(usize, (usize, usize))> {
        let mut tabs = Vec::new();
        let mut pos = start;
        for el in elements {
            match el {
                SnippetElement::Text(t) => {
                    pos += t.len();
                }
                SnippetElement::PlaceHolder(tab, els) => {
                    let placeholder_tabs = Self::elements_tabs(els, pos);
                    let end = pos + els.iter().map(|e| e.len()).sum::<usize>();
                    tabs.push((*tab, (pos, end)));
                    tabs.extend_from_slice(&placeholder_tabs);
                    pos = end;
                }
                SnippetElement::Tabstop(tab) => {
                    tabs.push((*tab, (pos, pos)));
                }
            }
        }
        tabs
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
pub enum SnippetElement {
    Text(String),
    PlaceHolder(usize, Vec<SnippetElement>),
    Tabstop(usize),
}

impl SnippetElement {
    pub fn len(&self) -> usize {
        match &self {
            SnippetElement::Text(text) => text.len(),
            SnippetElement::PlaceHolder(_, elements) => {
                elements.iter().map(|e| e.len()).sum()
            }
            SnippetElement::Tabstop(_) => 0,
        }
    }

    pub fn text(&self) -> String {
        match &self {
            SnippetElement::Text(t) => t.to_string(),
            SnippetElement::PlaceHolder(_, elements) => {
                elements.iter().map(|e| e.text()).join("")
            }
            SnippetElement::Tabstop(_) => "".to_string(),
        }
    }
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

#[derive(Clone, PartialEq)]
pub enum CompletionStatus {
    Inactive,
    Started,
}

#[derive(Clone)]
pub struct CompletionData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    pub request_id: usize,
    pub status: CompletionStatus,
    pub offset: usize,
    pub buffer_id: BufferId,
    pub input: String,
    pub index: usize,
    pub input_items: im::HashMap<String, Arc<Vec<ScoredCompletionItem>>>,
    empty: Arc<Vec<ScoredCompletionItem>>,
    pub filtered_items: Arc<Vec<ScoredCompletionItem>>,
    pub matcher: Arc<SkimMatcherV2>,
    pub size: Size,
}

impl CompletionData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            request_id: 0,
            index: 0,
            offset: 0,
            status: CompletionStatus::Inactive,
            buffer_id: BufferId(0),
            input: "".to_string(),
            input_items: im::HashMap::new(),
            filtered_items: Arc::new(Vec::new()),
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            size: Size::new(400.0, 300.0),
            empty: Arc::new(Vec::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.current_items().len()
    }

    pub fn next(&mut self) {
        self.index = Movement::Down.update_index(self.index, self.len(), 1, true);
    }

    pub fn previous(&mut self) {
        self.index = Movement::Up.update_index(self.index, self.len(), 1, true);
    }

    pub fn current_items(&self) -> &Arc<Vec<ScoredCompletionItem>> {
        if self.input == "" {
            self.all_items()
        } else {
            &self.filtered_items
        }
    }

    pub fn all_items(&self) -> &Arc<Vec<ScoredCompletionItem>> {
        self.input_items
            .get(&self.input)
            .unwrap_or_else(move || self.input_items.get("").unwrap_or(&self.empty))
    }

    pub fn current_item(&self) -> &CompletionItem {
        &self.current_items()[self.index].item
    }

    pub fn current(&self) -> &str {
        self.current_items()[self.index].item.label.as_str()
    }

    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        buffer_id: BufferId,
        input: String,
        position: Position,
        completion_widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        proxy.get_completion(
            request_id,
            buffer_id,
            position,
            Box::new(move |result| {
                if let Ok(res) = result {
                    println!("proxy completion result");
                    if let Ok(resp) =
                        serde_json::from_value::<CompletionResponse>(res)
                    {
                        event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateCompletion(
                                request_id, input, resp,
                            ),
                            Target::Widget(completion_widget_id),
                        );
                        return;
                    }
                }
            }),
        );
    }

    pub fn cancel(&mut self) {
        if self.status == CompletionStatus::Inactive {
            return;
        }
        println!("completion cancel");
        self.status = CompletionStatus::Inactive;
        self.input = "".to_string();
        self.input_items.clear();
        self.index = 0;
    }

    pub fn update_input(&mut self, input: String) {
        self.input = input;
        self.index = 0;
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.filter_items();
    }

    pub fn receive(
        &mut self,
        request_id: usize,
        input: String,
        resp: CompletionResponse,
    ) {
        if self.status == CompletionStatus::Inactive || self.request_id != request_id
        {
            return;
        }

        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let items = items
            .iter()
            .map(|i| ScoredCompletionItem {
                item: i.to_owned(),
                score: 0,
                index: 0,
                indices: Vec::new(),
            })
            .collect();

        self.input_items.insert(input, Arc::new(items));
        self.filter_items();
    }

    pub fn filter_items(&mut self) {
        if self.input == "" {
            return;
        }

        let mut items: Vec<ScoredCompletionItem> = self
            .all_items()
            .iter()
            .filter_map(|i| {
                let filter_text =
                    i.item.filter_text.as_ref().unwrap_or(&i.item.label);
                let shift = i.item.label.match_indices(filter_text).next()?.0;
                if let Some((score, mut indices)) =
                    self.matcher.fuzzy_indices(filter_text, &self.input)
                {
                    if shift > 0 {
                        indices = indices.iter().map(|i| i + shift).collect();
                    }
                    let mut item = i.clone();
                    item.score = score;
                    item.indices = indices;
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
        items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        self.filtered_items = Arc::new(items);
    }
}

pub struct CompletionContainer {
    id: WidgetId,
    scroll_id: WidgetId,
    completion: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScrollNew<LapceTabData, CompletionNew>>,
    >,
    content_size: Size,
}

impl CompletionContainer {
    pub fn new(data: &CompletionData) -> Self {
        let completion = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(CompletionNew::new()).vertical(),
            data.scroll_id,
        );
        Self {
            id: data.id,
            completion: WidgetPod::new(completion),
            scroll_id: data.scroll_id,
            content_size: Size::ZERO,
        }
    }

    pub fn ensure_item_visble(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rect = Size::new(width, line_height)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                data.completion.index as f64 * line_height,
            ));
        self.completion.widget_mut().inner_mut().scroll_to_visible(
            rect,
            |d| ctx.request_timer(d),
            env,
        );
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
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.completion.lifecycle(ctx, event, data, env);
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
            let editor = data.main_split.active_editor();
            if old_editor.window_origin != editor.window_origin
                || old_editor.scroll_offset != editor.scroll_offset
            {
                println!("completion request layout");
                ctx.request_local_layout();
                ctx.request_paint();
            }
        }

        if old_data.completion.input != data.completion.input
            || old_data.completion.request_id != data.completion.request_id
            || old_data.completion.status != data.completion.status
            || !old_data
                .completion
                .current_items()
                .same(&data.completion.current_items())
            || !old_data
                .completion
                .filtered_items
                .same(&data.completion.filtered_items)
        {
            ctx.request_local_layout();
            ctx.request_paint();
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
            self.ensure_item_visble(ctx, data, env);
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = data.completion.size.clone();
        let bc = BoxConstraints::new(Size::ZERO, size);
        self.content_size = self.completion.layout(ctx, &bc, data, env);
        self.completion.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.completion.status != CompletionStatus::Inactive
            && data.completion.len() > 0
        {
            let blur_color = Color::grey8(180);
            let shadow_width = 5.0;
            let rect = self.content_size.to_rect();
            ctx.blurred_rect(rect, shadow_width, &blur_color);
            self.completion.paint(ctx, data, env);
        }
    }
}

pub struct CompletionNew {}

impl CompletionNew {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<LapceTabData> for CompletionNew {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let height = data.completion.len();
        let height = height as f64 * line_height;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let size = ctx.size();

        let input = &data.completion.input;
        let items: &Vec<ScoredCompletionItem> = data.completion.current_items();

        for rect in rects {
            ctx.fill(rect, &env.get(LapceTheme::LIST_BACKGROUND));

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
                        &env.get(LapceTheme::LIST_CURRENT),
                    );
                }

                let item = &items[line];

                let y = line_height * line as f64 + 5.0;

                if let Some((svg, color)) =
                    completion_svg_new(item.item.kind, data.theme.clone())
                {
                    let color =
                        color.unwrap_or(env.get(LapceTheme::EDITOR_FOREGROUND));
                    let rect = Size::new(line_height, line_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, line_height * line as f64));
                    ctx.fill(rect, &color.clone().with_alpha(0.2));

                    let width = 16.0;
                    let height = 16.0;
                    let rect =
                        Size::new(width, height).to_rect().with_origin(Point::new(
                            (line_height - width) / 2.0,
                            (line_height - height) / 2.0 + line_height * line as f64,
                        ));
                    svg.paint(ctx, rect, Some(&color));
                }

                let focus_color = Color::rgb8(0, 0, 0);
                let content = item.item.label.as_str();
                let point = Point::new(line_height + 5.0, y);
                let mut text_layout = ctx
                    .text()
                    .new_text_layout(content.to_string())
                    .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
                    .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
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
}

#[derive(Clone)]
pub struct ScoredCompletionItem {
    pub item: CompletionItem,
    index: usize,
    score: i64,
    indices: Vec<usize>,
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
            .map(|(index, item)| {
                let mut item = ScoredCompletionItem {
                    item: item.to_owned(),
                    score: -1 - index as i64,
                    index: index,
                    indices: Vec::new(),
                };
                if input != "" {
                    // if let Some((score, indices)) =
                    //     self.matcher.fuzzy_indices(&item.item.label, &self.input)
                    // {
                    //     item.score = score;
                    // } else {
                    //     item.score = 0;
                    // }
                }
                item
            })
            .collect();
        self.items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        self.input = input;
    }

    pub fn update_input(&mut self, ctx: &mut EventCtx, input: String) {
        for item in self.items.iter_mut() {
            if input != "" {
                // if has_match(&input, &item.item.label) {
                //     let result = locate(&input, &item.item.label);
                //     item.score = result.score;
                //     item.match_mask = result.match_mask;
                // } else {
                //     item.score = f64::NEG_INFINITY;
                // }
            } else {
                // item.score = -1.0 - item.index as f64;
            }
        }
        self.items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        self.input = input;
        self.index = 0;
        self.scroll_offset = 0.0;
        self.request_paint(ctx);
    }
}

pub struct CompletionWidget {
    window_id: WindowId,
    tab_id: WidgetId,
    id: WidgetId,
}

impl CompletionWidget {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        id: WidgetId,
    ) -> CompletionWidget {
        CompletionWidget {
            window_id,
            tab_id,
            id,
        }
    }

    fn paint_raw(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let mut completion = &mut state.editor_split.lock().completion;
        let items = completion.current_items();
        let rect = ctx.region().rects()[0];
        let size = rect.size();

        ctx.fill(rect, &env.get(LapceTheme::EDITOR_SELECTION_COLOR));

        let current_line_offset = completion.index as f64 * line_height;
        let items_height = items.len() as f64 * line_height;
        let scroll_offset = if completion.scroll_offset
            < current_line_offset + line_height - size.height
        {
            (current_line_offset + line_height - size.height)
                .min(items_height - size.height)
        } else if completion.scroll_offset > current_line_offset {
            current_line_offset
        } else {
            completion.scroll_offset
        };

        let start_line = (scroll_offset / line_height).floor() as usize;
        let num_lines = (size.height / line_height).floor() as usize;
        for line in start_line..start_line + num_lines {
            if line >= items.len() {
                break;
            }

            if line == completion.index {
                let rect = Size::new(size.width, line_height).to_rect().with_origin(
                    Point::new(0.0, line_height * line as f64 - scroll_offset),
                );
                if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
                    ctx.fill(rect, background);
                }
            }

            let item = items[line];

            if let Some((svg, color)) =
                completion_svg(item.item.kind, Arc::new(HashMap::new()))
            {
                svg.to_piet(
                    Affine::translate(Vec2::new(
                        1.0,
                        line_height * line as f64 - scroll_offset,
                    )),
                    ctx,
                );
            }

            let mut layout =
                TextLayout::<String>::from_text(item.item.label.as_str());
            layout.set_font(LapceTheme::EDITOR_FONT);
            layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
            layout.rebuild_if_needed(&mut ctx.text(), env);
            let point = Point::new(20.0, line_height * line as f64 - scroll_offset);
            layout.draw(ctx, point);
        }

        if size.height < items_height {
            let scroll_bar_height = size.height * (size.height / items_height);
            let scroll_y = size.height * (scroll_offset / items_height);
            let scroll_bar_width = 10.0;
            ctx.render_ctx.fill(
                Rect::ZERO
                    .with_origin(Point::new(size.width - scroll_bar_width, scroll_y))
                    .with_size(Size::new(scroll_bar_width, scroll_bar_height)),
                &env.get(theme::SCROLLBAR_COLOR),
            );
        }

        completion.scroll_offset = scroll_offset;
    }
}

impl Widget<LapceUIState> for CompletionWidget {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceUIState,
        env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let shadow_width = 5.0;
        let shift = shadow_width * 2.0;
        let size = {
            let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
            let completion = &mut state.editor_split.lock().completion;
            let items = completion.current_items();
            let items_height = line_height * (items.len() as f64) + shift * 2.0;
            if items_height < ctx.size().height {
                Size::new(ctx.size().width, items_height)
            } else {
                ctx.size()
            }
        };

        let content_rect = size.to_rect() - Insets::new(shift, shift, shift, shift);

        let blur_color = Color::grey8(100);
        ctx.blurred_rect(content_rect, shadow_width, &blur_color);

        ctx.with_save(|ctx| {
            let origin = content_rect.origin().to_vec2();
            ctx.transform(Affine::translate(origin));
            ctx.with_child_ctx(content_rect - origin, |ctx| {
                self.paint_raw(ctx, data, env);
            });
        });
    }

    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }
}

fn completion_svg(
    kind: Option<CompletionItemKind>,
    theme: Arc<HashMap<String, Color>>,
) -> Option<(SvgData, Option<Color>)> {
    let kind = kind?;
    let kind_str = match kind {
        CompletionItemKind::Method => "method",
        CompletionItemKind::Function => "method",
        CompletionItemKind::Enum => "enum",
        CompletionItemKind::EnumMember => "enum-member",
        CompletionItemKind::Class => "class",
        CompletionItemKind::Variable => "variable",
        CompletionItemKind::Struct => "structure",
        CompletionItemKind::Keyword => "keyword",
        CompletionItemKind::Constant => "constant",
        CompletionItemKind::Property => "property",
        CompletionItemKind::Field => "field",
        CompletionItemKind::Interface => "interface",
        CompletionItemKind::Snippet => "snippet",
        CompletionItemKind::Module => "namespace",
        _ => return None,
    };
    Some((
        SvgData::from_str(
            ICONS_DIR
                .get_file(format!("symbol-{}.svg", kind_str))
                .unwrap()
                .contents_utf8()?,
        )
        .ok()?,
        theme.get(kind_str).map(|c| c.clone()),
    ))
}

fn completion_svg_new(
    kind: Option<CompletionItemKind>,
    theme: Arc<HashMap<String, Color>>,
) -> Option<(Svg, Option<Color>)> {
    let kind = kind?;
    let kind_str = match kind {
        CompletionItemKind::Method => "method",
        CompletionItemKind::Function => "method",
        CompletionItemKind::Enum => "enum",
        CompletionItemKind::EnumMember => "enum-member",
        CompletionItemKind::Class => "class",
        CompletionItemKind::Variable => "variable",
        CompletionItemKind::Struct => "structure",
        CompletionItemKind::Keyword => "keyword",
        CompletionItemKind::Constant => "constant",
        CompletionItemKind::Property => "property",
        CompletionItemKind::Field => "field",
        CompletionItemKind::Interface => "interface",
        CompletionItemKind::Snippet => "snippet",
        CompletionItemKind::Module => "namespace",
        _ => "string",
    };
    let theme_str = match kind_str {
        "namespace" => "builtinType",
        "variable" => "field",
        _ => kind_str,
    };
    Some((
        Svg::from_str(
            ICONS_DIR
                .get_file(format!("symbol-{}.svg", kind_str))
                .unwrap()
                .contents_utf8()?,
        )
        .ok()?,
        theme.get(theme_str).map(|c| c.clone()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet() {
        let s = "start $1${2:second ${3:third}} $0";
        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(s, parsed.to_string());

        let text = "start second third ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(1, (6, 6)), (2, (6, 18)), (3, (13, 18)), (0, (19, 19))],
            parsed.tabs(0)
        );
    }
}
