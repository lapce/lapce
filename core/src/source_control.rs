use std::path::PathBuf;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll},
    Affine, BoxConstraints, Color, Command, Cursor, Data, Env, Event, EventCtx,
    FontFamily, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod, WindowId,
};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceEditorLens, LapceTabData},
    editor::{LapceEditorContainer, LapceEditorView},
    palette::{file_svg, svg_tree_size},
    panel::{PanelPosition, PanelProperty},
    split::LapceSplitNew,
    state::{LapceUIState, LAPCE_APP_STATE},
    theme::LapceTheme,
};

pub const SOURCE_CONTROL_BUFFER: &'static str = "[Source Control Buffer]";

pub struct SourceControlData {
    pub widget_id: WidgetId,
    pub editor_view_id: WidgetId,
}

impl SourceControlData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            editor_view_id: WidgetId::next(),
        }
    }
}

pub struct SourceControlNew {
    widget_id: WidgetId,
    editor_view_id: WidgetId,
    editor_container_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl SourceControlNew {
    pub fn new(data: &LapceTabData) -> Self {
        let split_id = WidgetId::next();
        let editor_data = data
            .main_split
            .editors
            .get(&data.source_control.editor_view_id)
            .unwrap();
        let editor = LapceEditorView::new(
            editor_data.view_id,
            editor_data.container_id,
            editor_data.editor_id,
        )
        .lens(LapceEditorLens(editor_data.view_id));
        let split =
            LapceSplitNew::new(split_id).with_flex_child(editor.boxed(), 0.5);
        Self {
            widget_id: data.source_control.widget_id,
            editor_view_id: data.source_control.editor_view_id,
            editor_container_id: editor_data.container_id,
            split: WidgetPod::new(split),
        }
    }
}

impl Widget<LapceTabData> for SourceControlNew {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::Focus => {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(self.editor_container_id),
                            ));
                            ctx.set_handled();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
        self.split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.split.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.split.layout(ctx, bc, data, env);
        self.split.set_origin(ctx, data, env, Point::ZERO);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.split.paint(ctx, data, env);
    }
}

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

        let padding = 10.0;
        let commit_height = line_height * 5.0 + padding * 2.0;
        let commit_rect = Rect::ZERO
            .with_size(Size::new(
                size.width - padding * 2.0,
                commit_height - padding * 2.0,
            ))
            .with_origin(Point::new(padding, header_height + padding));
        if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
            ctx.fill(commit_rect, background);
        }

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
                        line as f64 * line_height
                            + 5.0
                            + header_height
                            + commit_height,
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
                        line as f64 * line_height
                            + 4.0
                            + header_height
                            + commit_height,
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
                        line as f64 * line_height
                            + 5.0
                            + header_height
                            + commit_height,
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
}
