use std::{collections::HashMap, sync::Arc};

use crate::state::Mode;
use crate::{
    buffer::BufferId,
    buffer::BufferUIState,
    command::{LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND},
    completion::Completion,
    editor::Editor,
    editor::EditorState,
    editor::EditorUIState,
    editor::EditorView,
    state::LapceTabState,
    state::LapceUIState,
    state::LAPCE_APP_STATE,
    theme::LapceTheme,
};
use crate::{palette::Palette, split::LapceSplit};
use crate::{scroll::LapceScroll, state::LapceFocus};
use druid::piet::TextAttribute;
use druid::FontDescriptor;
use druid::FontFamily;
use druid::FontWeight;
use druid::TextLayout;
use druid::{
    kurbo::{Line, Rect},
    piet::Text,
    piet::TextLayoutBuilder,
    widget::Container,
    widget::Flex,
    widget::IdentityWrapper,
    widget::Label,
    widget::SizedBox,
    Color, Command, MouseEvent, Selector, Target, Vec2, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
    WidgetExt, WidgetPod, WindowId,
};

pub struct ChildState {
    pub origin: Option<Point>,
    pub size: Option<Size>,
    pub hidden: bool,
}

pub struct LapceContainer {
    window_id: WindowId,
    tab_id: WidgetId,
    palette_max_size: Size,
    palette_rect: Rect,
    palette: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    editor_split: WidgetPod<LapceUIState, LapceSplit>,
    completion: WidgetPod<LapceUIState, Completion>,
}

impl LapceContainer {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> Self {
        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let (widget_id, scroll_widget_id) = {
            let palette = state.palette.lock();
            (palette.widget_id.clone(), palette.scroll_widget_id.clone())
        };
        let palette =
            Palette::new(window_id, tab_id, scroll_widget_id).with_id(widget_id);
        // .border(theme::BORDER_LIGHT, 1.0)
        // .background(LapceTheme::EDITOR_SELECTION_COLOR)
        let palette = WidgetPod::new(palette).boxed();

        let editor_split_state = state.editor_split.lock();
        let editor_view = EditorView::new(
            window_id,
            tab_id.clone(),
            editor_split_state.widget_id,
            editor_split_state.active,
            WidgetId::next(),
        );
        let editor_split = WidgetPod::new(
            LapceSplit::new(window_id, tab_id, true)
                .with_id(editor_split_state.widget_id)
                .with_flex_child(editor_view, 1.0),
        );

        let completion = WidgetPod::new(Completion::new(
            window_id.clone(),
            tab_id.clone(),
            editor_split_state.completion.widget_id,
        ));

        LapceContainer {
            window_id,
            tab_id,
            palette_max_size: Size::new(600.0, 400.0),
            palette_rect: Rect::ZERO
                .with_origin(Point::new(200.0, 100.0))
                .with_size(Size::new(600.0, 400.0)),
            palette,
            editor_split,
            completion,
        }
    }
}

impl Widget<LapceUIState> for LapceContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                self.palette.event(ctx, event, data, env);
                self.editor_split.event(ctx, event, data, env);
                self.completion.event(ctx, event, data, env);
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::OpenFile(path) => {
                            LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id)
                                .editor_split
                                .lock()
                                .open_file(ctx, data, path);
                            ctx.request_layout();
                        }
                        LapceUICommand::UpdateLineChanges(buffer_id) => {}
                        LapceUICommand::UpdateHighlights(
                            buffer_id,
                            rev,
                            highlights,
                        ) => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
                            let buffer =
                                editor_split.buffers.get_mut(buffer_id).unwrap();
                            if *rev == buffer.rev {
                                buffer.highlights = highlights.to_owned();
                                buffer.line_highlights = HashMap::new();
                                editor_split
                                    .notify_fill_text_layouts(ctx, buffer_id);
                            }
                        }
                        _ => (),
                    }
                }
                _ if cmd.is(LAPCE_COMMAND) => {
                    let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                    match cmd {
                        LapceCommand::Palette => (),
                        _ => (),
                    };
                    self.palette.event(ctx, event, data, env)
                }
                _ => (),
            },
            Event::MouseDown(mouse)
            | Event::MouseUp(mouse)
            | Event::MouseMove(mouse)
            | Event::Wheel(mouse) => {
                if *LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .focus
                    .lock()
                    == LapceFocus::Palette
                    && self.palette_rect.contains(mouse.pos)
                {
                    self.palette.event(ctx, event, data, env);
                } else {
                    self.editor_split.event(ctx, event, data, env);
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        self.palette.lifecycle(ctx, event, data, env);
        self.editor_split.lifecycle(ctx, event, data, env);
        self.completion.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        // if data.focus != old_data.focus {
        //     ctx.request_paint();
        // }
        self.palette.update(ctx, data, env);
        self.editor_split.update(ctx, data, env);
        // println!("container data update");
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let size = bc.max();

        let palette_bc = BoxConstraints::new(Size::ZERO, self.palette_max_size);
        let palette_size = self.palette.layout(ctx, &palette_bc, data, env);
        self.palette_rect = Rect::ZERO
            .with_origin(Point::new(
                (size.width - self.palette_max_size.width) / 2.0,
                ((size.height - self.palette_max_size.height) / 4.0).max(0.0),
            ))
            .with_size(palette_size);
        self.palette
            .set_layout_rect(ctx, data, env, self.palette_rect);

        {
            self.completion.layout(ctx, bc, data, env);
            let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
            let editor_split = state.editor_split.lock();
            let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
            for child in &self.editor_split.widget().children {
                if child.widget.id() == editor_split.active {
                    let editor =
                        editor_split.editors.get(&editor_split.active).unwrap();
                    if editor.buffer_id.is_none() {
                        continue;
                    }
                    let buffer_id = editor.buffer_id.as_ref().unwrap();
                    let buffer = editor_split.buffers.get(buffer_id).unwrap();
                    let (line, col) =
                        buffer.offset_to_line_col(editor_split.completion.offset);
                    let line_content = buffer
                        .slice_to_cow(
                            buffer.offset_of_line(line)
                                ..buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    let col = col + &line_content[..col].matches('\t').count() * 3;
                    let char_width = 7.6171875;
                    let origin = child.widget.layout_rect().origin()
                        + Vec2::new(
                            editor.gutter_width + col as f64 * char_width - 20.0,
                            editor.header_height + (line + 1) as f64 * line_height
                                - 10.0,
                        )
                        - editor.scroll_offset;
                    let layout_rect = Rect::from_origin_size(
                        origin,
                        Size::new(300.0, 12.0 * line_height + 20.0),
                    );
                    self.completion.set_layout_rect(ctx, data, env, layout_rect);
                }
            }
        }

        self.editor_split.layout(ctx, bc, data, env);
        self.editor_split.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(size),
        );
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
                ctx.fill(rect, background);
            }
        }
        self.editor_split.paint(ctx, data, env);
        if *LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .focus
            .lock()
            == LapceFocus::Palette
        {
            // let blur_color = Color::grey8(100);
            // ctx.blurred_rect(self.palette.layout_rect(), 5.0, &blur_color);
            self.palette.paint(ctx, data, env);
        }

        if LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .editor_split
            .lock()
            .completion
            .len()
            > 0
        {
            self.completion.paint(ctx, data, env);
        }

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        if editor_split.get_mode() == Mode::Insert {
            let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
            if let Some(signature) = editor_split.signature.signature.as_ref() {
                if let Some(offset) = editor_split.signature.offset {
                    for child in &self.editor_split.widget().children {
                        if child.widget.id() == editor_split.active {
                            let editor = editor_split
                                .editors
                                .get(&editor_split.active)
                                .unwrap();
                            if editor.buffer_id.is_none() {
                                continue;
                            }
                            let buffer_id = editor.buffer_id.as_ref().unwrap();
                            let buffer =
                                editor_split.buffers.get(buffer_id).unwrap();
                            let (line, col) = buffer.offset_to_line_col(offset);
                            if line == 0 {
                                continue;
                            }
                            let line_content = buffer
                                .slice_to_cow(
                                    buffer.offset_of_line(line)
                                        ..buffer.offset_of_line(line + 1),
                                )
                                .to_string();
                            let col =
                                col + &line_content[..col].matches('\t').count() * 3;
                            let char_width = 7.6171875;
                            let label = signature.signatures[0].label.clone();
                            let origin = child.widget.layout_rect().origin()
                                + Vec2::new(
                                    editor.gutter_width + col as f64 * char_width
                                        - (label.find("(").unwrap() + 1) as f64
                                            * char_width,
                                    editor.header_height
                                        + (line - 1) as f64 * line_height,
                                )
                                - editor.scroll_offset;
                            let active = editor_split.signature.active;
                            let mut text_layout = ctx
                                .text()
                                .new_text_layout(label.clone())
                                .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
                                .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
                            if let Some(active) = active {
                                text_layout = text_layout.range_attribute(
                                    active.0..active.1,
                                    TextAttribute::TextColor(Color::rgb8(0, 0, 0)),
                                );
                                text_layout = text_layout.range_attribute(
                                    active.0..active.1,
                                    TextAttribute::Weight(FontWeight::BOLD),
                                );
                            }
                            let text_layout = text_layout.build().unwrap();
                            let rect =
                                Rect::ZERO.with_origin(origin).with_size(Size::new(
                                    label.len() as f64 * char_width + 20.0,
                                    line_height,
                                ));
                            ctx.fill(
                                rect,
                                &env.get(LapceTheme::EDITOR_SELECTION_COLOR),
                            );
                            ctx.stroke(rect, &env.get(theme::BORDER_LIGHT), 1.0);
                            ctx.draw_text(
                                &text_layout,
                                origin + Vec2::new(10.0, 5.0),
                            );
                        }
                    }
                }
            }
        }
    }
}
