use std::path::PathBuf;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll},
    Affine, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size,
    TextLayout, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    palette::{file_svg, svg_tree_size},
    panel::{PanelPosition, PanelProperty},
    state::{LapceUIState, LAPCE_APP_STATE},
    theme::LapceTheme,
};

pub struct SourceControl {
    window_id: WindowId,
    tab_id: WidgetId,
    widget_id: WidgetId,
}

impl SourceControl {
    pub fn new(window_id: WindowId, tab_id: WidgetId, widget_id: WidgetId) -> Self {
        Self {
            window_id,
            tab_id,
            widget_id,
        }
    }
}

impl Widget<LapceUIState> for SourceControl {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

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
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let source_control = state.source_control.lock();
        source_control.paint(ctx, data, env);
    }
}

pub struct SourceControlState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub widget_id: WidgetId,
    position: PanelPosition,
    pub diff_files: Vec<PathBuf>,
}

impl PanelProperty for SourceControlState {
    fn widget_id(&self) -> WidgetId {
        self.widget_id
    }

    fn position(&self) -> &PanelPosition {
        &self.position
    }

    fn active(&self) -> usize {
        0
    }

    fn size(&self) -> (f64, f64) {
        (300.0, 0.5)
    }

    fn paint(&self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);

        let size = ctx.size();
        let header_height = line_height;
        let header_rect = Rect::ZERO.with_size(Size::new(size.width, header_height));
        if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
            ctx.fill(header_rect, background);
        }
        ctx.fill(
            Size::new(size.width, size.height - header_height)
                .to_rect()
                .with_origin(Point::new(0.0, header_height)),
            &env.get(LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND),
        );

        let text_layout = ctx
            .text()
            .new_text_layout("Source Control")
            .font(FontFamily::SYSTEM_UI, 14.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
        let text_layout = text_layout.build().unwrap();
        ctx.draw_text(&text_layout, Point::new(20.0, 5.0));

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let workspace_path = state.workspace.lock().path.clone();

        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            for (line, file) in self.diff_files.iter().enumerate() {
                let file_name =
                    file.file_name().unwrap().to_str().unwrap().to_string();
                let folder = file.parent().unwrap();
                let folder =
                    if let Ok(folder) = folder.strip_prefix(&workspace_path) {
                        folder
                    } else {
                        folder
                    }
                    .to_str()
                    .unwrap()
                    .to_string();
                let icon = if let Some(exten) = file.extension() {
                    match exten.to_str().unwrap() {
                        "rs" => "rust",
                        "md" => "markdown",
                        "cc" => "cpp",
                        s => s,
                    }
                } else {
                    ""
                };
                if let Some((svg_data, svg_tree)) = file_svg(&icon) {
                    let svg_size = svg_tree_size(&svg_tree);
                    let scale = 13.0 / svg_size.height;
                    let affine = Affine::new([
                        scale,
                        0.0,
                        0.0,
                        scale,
                        1.0,
                        line as f64 * line_height + 5.0 + header_height,
                    ]);
                    svg_data.to_piet(affine, ctx);
                }
                let mut text_layout = ctx
                    .text()
                    .new_text_layout(file_name.clone())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
                let text_layout = text_layout.build().unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        20.0,
                        line as f64 * line_height + 4.0 + header_height,
                    ),
                );
                let text_x =
                    text_layout.hit_test_text_position(file_name.len()).point.x;
                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        env.get(LapceTheme::EDITOR_FOREGROUND).with_alpha(0.6),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        20.0 + text_x + 4.0,
                        line as f64 * line_height + 5.0 + header_height,
                    ),
                );
            }
        }
    }
}

impl SourceControlState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> Self {
        Self {
            window_id,
            tab_id,
            widget_id: WidgetId::next(),
            diff_files: Vec::new(),
            position: PanelPosition::LeftBottom,
        }
    }

    pub fn widget_id(&self) -> WidgetId {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let panel = state.panel.lock();
        panel.widget_id(self.position())
    }
}
