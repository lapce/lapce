use std::{collections::HashMap, sync::Arc};

use crate::{
    buffer::BufferId,
    buffer::BufferUIState,
    command::{LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND},
    editor::Editor,
    editor::EditorState,
    editor::EditorView,
    state::LapceState,
    theme::LapceTheme,
};
use crate::{palette::Palette, split::LapceSplit};
use crate::{scroll::LapceScroll, state::LapceFocus};
use druid::{
    kurbo::{Line, Rect},
    widget::Container,
    widget::Flex,
    widget::IdentityWrapper,
    widget::Label,
    widget::SizedBox,
    Color, Command, MouseEvent, Selector, Target, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};

pub struct ChildState {
    pub origin: Option<Point>,
    pub size: Option<Size>,
    pub hidden: bool,
}

pub struct LapceContainer {
    palette_max_size: Size,
    palette_rect: Rect,
    palette: WidgetPod<LapceState, Box<dyn Widget<LapceState>>>,
    editor_split: WidgetPod<LapceState, Box<dyn Widget<LapceState>>>,
}

impl LapceContainer {
    pub fn new(state: LapceState) -> Self {
        let palette = Palette::new(state.palette.scroll_widget_id)
            .border(theme::BORDER_LIGHT, 1.0)
            .background(LapceTheme::PALETTE_BACKGROUND);
        let palette_id = WidgetId::next();
        let palette =
            WidgetPod::new(IdentityWrapper::wrap(palette, palette_id)).boxed();
        let editor_view = EditorView::new(
            state.editor_split.widget_id,
            state.editor_split.active,
            WidgetId::next(),
        );
        let editor_split = WidgetPod::new(IdentityWrapper::wrap(
            LapceSplit::new(true).with_child(editor_view),
            state.editor_split.widget_id,
        ))
        .boxed();

        LapceContainer {
            palette_max_size: Size::new(600.0, 400.0),
            palette_rect: Rect::ZERO
                .with_origin(Point::new(200.0, 100.0))
                .with_size(Size::new(600.0, 400.0)),
            palette,
            editor_split,
        }
    }
}

impl Widget<LapceState> for LapceContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
        ctx.request_focus();
        match event {
            Event::Internal(_) => {
                self.palette.event(ctx, event, data, env);
                self.editor_split.event(ctx, event, data, env);
            }
            Event::KeyDown(key_event) => data.key_down(ctx, key_event, env),
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::OpenFile(path) => {
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split.open_file(ctx, path);
                            ctx.request_layout();
                        }
                        LapceUICommand::UpdateHighlights(
                            buffer_id,
                            version,
                            highlights,
                        ) => {
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            let buffer = editor_split
                                .buffers
                                .get_mut(buffer_id)
                                .unwrap();
                            if version == &buffer.highlight_version {
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
                if data.focus == LapceFocus::Palette
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
        data: &LapceState,
        env: &Env,
    ) {
        self.palette.lifecycle(ctx, event, data, env);
        self.editor_split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        self.palette.update(ctx, data, env);
        self.editor_split.update(ctx, data, env);
        // println!("container data update");
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
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
        println!("palette_size {:?}", palette_size);
        self.palette
            .set_layout_rect(ctx, data, env, self.palette_rect);

        self.editor_split.layout(ctx, bc, data, env);
        self.editor_split.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(size),
        );
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            if let Some(background) = data.theme.get("background") {
                ctx.fill(rect, background);
            }
        }
        self.editor_split.paint(ctx, data, env);
        if data.focus == LapceFocus::Palette {
            self.palette.paint(ctx, data, env);
        }
    }
}
