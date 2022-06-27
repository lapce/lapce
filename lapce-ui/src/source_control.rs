use std::sync::Arc;

use druid::{
    kurbo::BezPath,
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target, UpdateCtx, Widget,
    WidgetExt, WidgetId,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{FocusArea, LapceTabData, PanelKind},
};
use lapce_rpc::source_control::FileDiff;

use crate::{
    button::Button,
    editor::view::LapceEditorView,
    panel::{LapcePanel, PanelHeaderKind},
    svg::{file_svg, get_svg},
};

pub fn new_source_control_panel(data: &LapceTabData) -> LapcePanel {
    let editor_data = data
        .main_split
        .editors
        .get(&data.source_control.editor_view_id)
        .unwrap();
    let input =
        LapceEditorView::new(editor_data.view_id, editor_data.editor_id, None)
            .hide_header()
            .hide_gutter()
            .set_placeholder("Commit Message".to_string())
            .padding((15.0, 15.0));

    let commit_button = Button::new(data, "Commit")
        .on_click(|ctx, data, _env| {
            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::SourceControlCommit,
                    ),
                    data: None,
                },
                Target::Widget(data.id),
            ));
        })
        .expand_width()
        .with_id(data.source_control.commit_button_id);

    let content = SourceControlFileList::new(data.source_control.file_list_id);

    LapcePanel::new(
        PanelKind::SourceControl,
        data.source_control.widget_id,
        data.source_control.split_id,
        data.source_control.split_direction,
        PanelHeaderKind::Simple("Source Control".into()),
        vec![
            (
                editor_data.view_id,
                PanelHeaderKind::None,
                input.boxed(),
                Some(300.0),
            ),
            (
                data.source_control.commit_button_id,
                PanelHeaderKind::None,
                commit_button.boxed(),
                None,
            ),
            (
                data.source_control.file_list_id,
                PanelHeaderKind::Simple("Changes".into()),
                content.boxed(),
                None,
            ),
        ],
    )
}

struct SourceControlFileList {
    widget_id: WidgetId,
    mouse_down: Option<usize>,
    line_height: f64,
}

impl SourceControlFileList {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            mouse_down: None,
            line_height: 25.0,
        }
    }

    pub fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        ctx.request_focus();
        let source_control = Arc::make_mut(&mut data.source_control);
        source_control.active = self.widget_id;
        data.focus_area = FocusArea::Panel(PanelKind::SourceControl);
        data.focus = self.widget_id;
    }
}

impl Widget<LapceTabData> for SourceControlFileList {
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
            Event::MouseMove(_mouse_event) => {
                ctx.set_cursor(&druid::Cursor::Pointer);
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                let y = mouse_event.pos.y;
                if y > 0.0 {
                    let line = (y / self.line_height).floor() as usize;
                    if line < data.source_control.file_diffs.len()
                        && mouse_event.pos.x < self.line_height
                    {
                        if let Some(mouse_down) = self.mouse_down {
                            if mouse_down == line {
                                let source_control =
                                    Arc::make_mut(&mut data.source_control);
                                source_control.file_diffs[line].1 =
                                    !source_control.file_diffs[line].1;
                            }
                        }
                    }
                }
                self.mouse_down = None;
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down = None;
                let source_control = Arc::make_mut(&mut data.source_control);
                let y = mouse_event.pos.y;
                if y > 0.0 {
                    let line = (y / self.line_height).floor() as usize;
                    if line < source_control.file_diffs.len() {
                        source_control.file_list_index = line;
                        if mouse_event.pos.x < self.line_height {
                            self.mouse_down = Some(line);
                        } else {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::OpenFileDiff(
                                    source_control.file_diffs[line].0.path().clone(),
                                    "head".to_string(),
                                ),
                                Target::Widget(data.id),
                            ));
                        }
                    }
                }
                self.request_focus(ctx, data);
                ctx.set_handled();
            }
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut source_control = data.source_control.clone();
                Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    Arc::make_mut(&mut source_control),
                    env,
                );

                data.keypress = keypress.clone();
                data.source_control = source_control.clone();
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx, data);
                    ctx.set_handled();
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::FocusChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if data.source_control.file_diffs.len()
            != old_data.source_control.file_diffs.len()
        {
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let height = self.line_height * data.source_control.file_diffs.len() as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let self_size = ctx.size();

        let diffs = &data.source_control.file_diffs;

        if ctx.is_focused() && !diffs.is_empty() {
            let rect = Size::new(ctx.size().width, self.line_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    data.source_control.file_list_index as f64 * self.line_height,
                ));
            ctx.fill(
                rect,
                data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
            );
        }

        let rect = ctx.region().bounding_box();
        let start_line = (rect.y0 / self.line_height).floor() as usize;
        let end_line = (rect.y1 / self.line_height).ceil() as usize;
        for line in start_line..end_line {
            if line >= diffs.len() {
                break;
            }
            let y = self.line_height * line as f64;
            let (diff, checked) = diffs[line].clone();
            let mut path = diff.path().clone();
            if let Some(workspace_path) = data.workspace.path.as_ref() {
                path = path
                    .strip_prefix(workspace_path)
                    .unwrap_or(&path)
                    .to_path_buf();
            }
            {
                let width = 13.0;
                let height = 13.0;
                let origin = Point::new(
                    (self.line_height - width) / 2.0 + 5.0,
                    (self.line_height - height) / 2.0 + y,
                );
                let rect = Size::new(width, height).to_rect().with_origin(origin);
                ctx.stroke(rect, &Color::rgb8(0, 0, 0), 1.0);

                if checked {
                    let mut path = BezPath::new();
                    path.move_to((origin.x + 3.0, origin.y + 7.0));
                    path.line_to((origin.x + 6.0, origin.y + 9.5));
                    path.line_to((origin.x + 10.0, origin.y + 3.0));
                    ctx.stroke(path, &Color::rgb8(0, 0, 0), 2.0);
                }
            }
            let svg = file_svg(&path);
            let width = 13.0;
            let height = 13.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (self.line_height - width) / 2.0 + self.line_height,
                (self.line_height - height) / 2.0 + y,
            ));
            ctx.draw_svg(&svg, rect, None);

            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let text_layout = ctx
                .text()
                .new_text_layout(file_name)
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(
                    self.line_height * 2.0,
                    y + (self.line_height - text_layout.size().height) / 2.0,
                ),
            );
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !folder.is_empty() {
                let x = text_layout.size().width;

                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        self.line_height * 2.0 + x + 5.0,
                        y + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );
            }

            let (svg, color) = match diff {
                FileDiff::Modified(_) => (
                    "diff-modified.svg",
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_MODIFIED),
                ),
                FileDiff::Added(_) => (
                    "diff-added.svg",
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_ADDED),
                ),
                FileDiff::Deleted(_) => (
                    "diff-removed.svg",
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_REMOVED),
                ),
                FileDiff::Renamed(_, _) => (
                    "diff-renamed.svg",
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_MODIFIED),
                ),
            };
            let svg = get_svg(svg).unwrap();

            let svg_size = 15.0;
            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        self_size.width - svg_size - 10.0,
                        line as f64 * self.line_height
                            + (self.line_height - svg_size) / 2.0,
                    ));
            ctx.draw_svg(&svg, rect, Some(&color.clone().with_alpha(0.9)));
        }
    }
}
