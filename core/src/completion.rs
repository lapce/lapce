use std::cmp::Ordering;

use bit_vec::BitVec;
use druid::{
    scroll_component::ScrollComponent, theme, widget::SvgData, Affine, Color,
    Command, Env, Event, EventCtx, Insets, PaintCtx, Point, Rect, RenderContext,
    Size, Target, TextLayout, Vec2, Widget, WidgetId, WindowId,
};
use fzyr::{has_match, locate};
use lsp_types::{CompletionItem, CompletionItemKind};
use std::str::FromStr;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    explorer::ICONS_DIR,
    state::LapceUIState,
    state::LAPCE_APP_STATE,
    theme::LapceTheme,
};

#[derive(Clone)]
pub struct ScoredCompletionItem {
    pub item: CompletionItem,
    index: usize,
    score: f64,
    match_mask: BitVec,
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
        self.items
            .iter()
            .filter(|i| i.score != f64::NEG_INFINITY)
            .count()
    }

    pub fn current_items(&self) -> Vec<&ScoredCompletionItem> {
        self.items
            .iter()
            .filter(|i| i.score != f64::NEG_INFINITY)
            .collect()
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
                    score: -1.0 - index as f64,
                    index: index,
                    match_mask: BitVec::new(),
                };
                if input != "" {
                    if has_match(&input, &item.item.label) {
                        let result = locate(&input, &item.item.label);
                        item.score = result.score;
                        item.match_mask = result.match_mask;
                    } else {
                        item.score = f64::NEG_INFINITY;
                    }
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
                if has_match(&input, &item.item.label) {
                    let result = locate(&input, &item.item.label);
                    item.score = result.score;
                    item.match_mask = result.match_mask;
                } else {
                    item.score = f64::NEG_INFINITY;
                }
            } else {
                item.score = -1.0 - item.index as f64;
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

pub struct Completion {
    window_id: WindowId,
    tab_id: WidgetId,
    id: WidgetId,
}

impl Completion {
    pub fn new(window_id: WindowId, tab_id: WidgetId, id: WidgetId) -> Completion {
        Completion {
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

            if let Some(svg) = completion_svg(item.item.kind) {
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

impl Widget<LapceUIState> for Completion {
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

fn completion_svg(kind: Option<CompletionItemKind>) -> Option<SvgData> {
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
    Some(
        SvgData::from_str(
            ICONS_DIR
                .get_file(format!("symbol-{}.svg", kind_str))
                .unwrap()
                .contents_utf8()?,
        )
        .ok()?,
    )
}
