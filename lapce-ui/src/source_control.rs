use std::sync::Arc;

use druid::{
    kurbo::BezPath,
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point, Rect, RenderContext,
    Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{FocusArea, LapceData, LapceTabData},
    panel::PanelKind,
};
use lapce_rpc::source_control::FileDiff;

use crate::{
    button::Button,
    editor::view::LapceEditorView,
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
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
        .with_id(data.source_control.commit_button_id)
        .padding((10.0, 0.0, 10.0, 10.0));

    let content = SourceControlFileList::new(data.source_control.file_list_id);

    LapcePanel::new(
        PanelKind::SourceControl,
        data.source_control.widget_id,
        data.source_control.split_id,
        vec![
            (
                editor_data.view_id,
                PanelHeaderKind::None,
                input.boxed(),
                PanelSizing::Size(300.0),
            ),
            (
                data.source_control.commit_button_id,
                PanelHeaderKind::None,
                commit_button.boxed(),
                PanelSizing::Flex(false),
            ),
            (
                data.source_control.file_list_id,
                PanelHeaderKind::Simple("Changes".into()),
                content.boxed(),
                PanelSizing::Flex(false),
            ),
        ],
    )
}

struct SourceControlFileList {
    widget_id: WidgetId,
    mouse_pos: Option<Point>,
    mouse_down: Option<usize>,
    current_line: Option<usize>,
    line_rects: Vec<Rect>,
    line_height: f64,
}

impl SourceControlFileList {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            mouse_pos: None,
            mouse_down: None,
            current_line: None,
            line_rects: vec![],
            line_height: 25.0,
        }
    }

    pub fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        ctx.request_focus();
        let source_control = Arc::make_mut(&mut data.source_control);
        source_control.active = self.widget_id;
        data.focus_area = FocusArea::Panel(PanelKind::SourceControl);
        data.focus = Arc::new(self.widget_id);
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> Option<usize> {
        for (i, rect) in self.line_rects.iter().enumerate() {
            if rect.contains(mouse_event.pos) {
                return Some(i);
            }
        }
        None
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
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                self.mouse_pos = Some(mouse_event.pos);
                let current_line = self.icon_hit_test(mouse_event);
                if current_line.is_some() {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                if current_line != self.current_line {
                    ctx.request_paint();
                    self.current_line = current_line;
                }
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
                                ctx.request_paint();
                            }
                        }
                    }
                }
                self.mouse_down = None;
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                if mouse_event.pos.y < 0.0 {
                    return;
                }

                let target_line =
                    (mouse_event.pos.y / self.line_height).floor() as usize;

                match mouse_event.button {
                    MouseButton::Left => {
                        self.mouse_down = None;
                        let source_control = Arc::make_mut(&mut data.source_control);

                        if target_line < source_control.file_diffs.len() {
                            source_control.file_list_index = target_line;
                            if mouse_event.pos.x < self.line_height {
                                self.mouse_down = Some(target_line);
                            } else {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::OpenFileDiff {
                                        path: source_control
                                            .file_diffs
                                            .get_index(target_line)
                                            .unwrap()
                                            .0
                                            .clone(),
                                        history: "head".to_string(),
                                    },
                                    Target::Widget(data.id),
                                ));
                            }
                        }

                        self.request_focus(ctx, data);
                        ctx.set_handled();
                    }
                    MouseButton::Right => {
                        let source_control = data.source_control.clone();
                        let (target_file_path, target_file_diff) = source_control
                            .file_diffs
                            .get_index(target_line)
                            .map(|(path, (diff, _))| (path.clone(), diff.clone()))
                            .unwrap();

                        let mut menu = druid::Menu::<LapceData>::new("");
                        let mut item = druid::MenuItem::new("Open Changes").command(
                            Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::OpenFileDiff {
                                    path: target_file_path.clone(),
                                    history: "head".to_string(),
                                },
                                Target::Auto,
                            ),
                        );

                        menu = menu.entry(item);

                        let enable_open_file =
                            !matches!(target_file_diff, FileDiff::Deleted(_));

                        item = druid::MenuItem::new("Open File")
                            .enabled(enable_open_file)
                            .on_activate(move |ctx, _, _| {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::OpenFile(
                                        target_file_path.clone(),
                                        true,
                                    ),
                                    Target::Auto,
                                ));
                            });

                        menu = menu.entry(item);

                        menu = menu.separator();

                        item = druid::MenuItem::new("Discard Changes")
                            .on_activate(move |ctx, _, _| {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Workbench(
                                             LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges
                                        ),
                                        data: Some(serde_json::json!(target_file_diff.clone()))
                                    },
                                    Target::Auto,
                                ));
                            });

                        menu = menu.entry(item);

                        ctx.show_context_menu(menu, mouse_event.window_pos)
                    }
                    _ => {}
                }
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

        if ctx.is_focused() && !data.source_control.file_diffs.is_empty() {
            let rect = Size::new(ctx.size().width, self.line_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    data.source_control.file_list_index as f64 * self.line_height,
                ));
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_CURRENT_BACKGROUND),
            );
        }

        let rect = ctx.region().bounding_box();
        let start_line = (rect.y0 / self.line_height).floor() as usize;
        let end_line = (rect.y1 / self.line_height).ceil() as usize;
        self.line_rects = vec![];
        for line in start_line..end_line {
            if line >= data.source_control.file_diffs.len() {
                break;
            }
            let y = self.line_height * line as f64;

            let current_line = Size::new(ctx.size().width, self.line_height)
                .to_rect()
                .with_origin(Point::new(0.0, y));
            self.line_rects.push(current_line);
            if let Some(mouse_pos) = self.mouse_pos {
                if current_line.contains(mouse_pos) {
                    ctx.fill(
                        current_line,
                        data.config.get_color_unchecked(
                            LapceTheme::PANEL_CURRENT_BACKGROUND,
                        ),
                    );
                }
            }

            let (mut path, (diff, checked)) = data
                .source_control
                .file_diffs
                .get_index(line)
                .map(|d| (d.0.clone(), d.1))
                .unwrap();
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
                ctx.stroke(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                    1.0,
                );

                if *checked {
                    let mut path = BezPath::new();
                    path.move_to((origin.x + 3.0, origin.y + 7.0));
                    path.line_to((origin.x + 6.0, origin.y + 9.5));
                    path.line_to((origin.x + 10.0, origin.y + 3.0));
                    ctx.stroke(
                        path,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                        2.0,
                    );
                }
            }

            let svg_size = data.config.ui.icon_size() as f64;
            let (svg, svg_color) = data.config.file_svg(&path);
            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        (self.line_height - svg_size) / 2.0 + self.line_height,
                        (self.line_height - svg_size) / 2.0 + y,
                    ));
            ctx.draw_svg(&svg, rect, svg_color);

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
                    y + text_layout.y_offset(self.line_height),
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
                        y + text_layout.y_offset(self.line_height),
                    ),
                );
            }

            let (svg, color) = match diff {
                FileDiff::Modified(_) => (
                    LapceIcons::SCM_DIFF_MODIFIED,
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_MODIFIED),
                ),
                FileDiff::Added(_) => (
                    LapceIcons::SCM_DIFF_ADDED,
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_ADDED),
                ),
                FileDiff::Deleted(_) => (
                    LapceIcons::SCM_DIFF_REMOVED,
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_REMOVED),
                ),
                FileDiff::Renamed(_, _) => (
                    LapceIcons::SCM_DIFF_RENAMED,
                    data.config
                        .get_color_unchecked(LapceTheme::SOURCE_CONTROL_MODIFIED),
                ),
            };
            let svg = data.config.ui_svg(svg);

            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        self_size.width - svg_size - 10.0,
                        line as f64 * self.line_height
                            + (self.line_height - svg_size) / 2.0,
                    ));
            ctx.draw_svg(&svg, rect, Some(color));
        }
    }
}
