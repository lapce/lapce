use std::path::Path;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetExt,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::{EditorDiagnostic, LapceTabData},
    editor::EditorLocation,
    panel::PanelKind,
    problem::ProblemData,
    proxy::path_from_url,
};
use lsp_types::DiagnosticSeverity;

use crate::panel::{LapcePanel, PanelHeaderKind, PanelSizing};

pub fn new_problem_panel(data: &ProblemData) -> LapcePanel {
    LapcePanel::new(
        PanelKind::Problem,
        data.widget_id,
        data.split_id,
        vec![
            (
                data.error_widget_id,
                PanelHeaderKind::Simple("Errors".into()),
                ProblemContent::new(DiagnosticSeverity::ERROR).boxed(),
                PanelSizing::Flex(true),
            ),
            (
                data.warning_widget_id,
                PanelHeaderKind::Simple("Warnings".into()),
                ProblemContent::new(DiagnosticSeverity::WARNING).boxed(),
                PanelSizing::Flex(true),
            ),
        ],
    )
}

fn is_collapsed(data: &LapceTabData, path: &Path) -> bool {
    data.problem.collapsed.get(path).copied().unwrap_or(false)
}

struct ProblemContent {
    severity: DiagnosticSeverity,
    mouse_pos: Point,
    content_height: f64,
}

impl ProblemContent {
    pub fn new(severity: DiagnosticSeverity) -> Self {
        Self {
            severity,
            mouse_pos: Point::ZERO,
            content_height: 0.0,
        }
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &LapceTabData,
    ) {
        // If it isn't hot then we don't bother checking
        if !ctx.is_hot() {
            return;
        }

        let line_height = data.config.editor.line_height() as f64;
        let click_line = (mouse_event.pos.y / line_height).floor() as usize;

        let items = data.main_split.diagnostics_items(self.severity);

        let mut line_cursor = 0;

        // Skip files before clicked section.
        let mut current_file = None;
        for (path, diagnostics) in items {
            let diag_lines = if is_collapsed(data, path) {
                // If section is collapsed count only header with file name.
                1
            } else {
                // Total file lines and header with file name.
                diagnostics.iter().map(|d| d.lines).sum::<usize>() + 1 /* file name header */
            };
            let line_range = line_cursor..(line_cursor + diag_lines);

            // Did we reach the clicked section?
            if line_range.contains(&click_line) {
                // The current file is what we are looking for.
                current_file = Some((path, diagnostics));
                break;
            }

            // No. Move line cursor.
            line_cursor += diag_lines;
        }

        // Check if we have clicked something interesting.
        let (path, diagnostics) = if let Some(diag) = current_file {
            diag
        } else {
            // The user clicked an empty area.
            return;
        };

        // Handle click on header with file name.
        if line_cursor == click_line {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ToggleProblem(path.to_path_buf()),
                Target::Widget(data.id),
            ));
            return;
        }

        assert!(
            !is_collapsed(data, path),
            "Can't click any element because the file is collapsed. This shouldn't happen, please report a bug."
        );

        // Skip header.
        line_cursor += 1;

        // Skip to clicked diagnostic.
        let mut clicked_file_diagnostic = None;
        for file_diagnostic in diagnostics {
            let line_range = line_cursor..(line_cursor + file_diagnostic.lines);

            // Is the current diagnostic the clicked one?
            if line_range.contains(&click_line) {
                // We found the diagnostic we are looking for.
                clicked_file_diagnostic = Some(file_diagnostic);
                break;
            }

            // No. Move line cursor and consume diagnostic.
            line_cursor += file_diagnostic.lines;
        }

        // Handle current diagnostic.
        let file_diagnostic = clicked_file_diagnostic.expect("Editor diagnostic not found. We should have found something, please report a bug");

        assert!(
            line_cursor <= click_line,
            "Line cursor ({line_cursor}) is larger than clicked line ({click_line}). This should never happen!"
        );

        let msg_lines = message_lines(file_diagnostic);

        // Widget has mouse about it and line is clicked one.
        if (line_cursor..(line_cursor + msg_lines)).contains(&click_line) {
            // Rust example: description without location.
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLspLocation(
                    None,
                    EditorLocation {
                        path: path.to_path_buf(),
                        position: Some(file_diagnostic.diagnostic.range.start),
                        scroll_offset: None,
                        history: None,
                    },
                    false,
                ),
                Target::Widget(data.id),
            ));
            return;
        }
        line_cursor += msg_lines;

        // Skip to clicked related information.
        let related = file_diagnostic
            .diagnostic
            .related_information
            .as_deref()
            .unwrap_or(&[]);

        let mut clicked_related = None;
        for related in related {
            let lines = related.message.lines().count();
            let item_line_range = line_cursor..(line_cursor + lines);

            // Is the current line the clicked one?
            if item_line_range.contains(&click_line) {
                // We found the related info we are looking for.
                clicked_related = Some(related);
                break;
            }

            // No. Move line cursor.
            line_cursor += lines;
        }

        if let Some(related) = clicked_related {
            let path = related.location.uri.to_file_path().unwrap();
            let start = related.location.range.start;
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLspLocation(
                    None,
                    EditorLocation {
                        path,
                        position: Some(start),
                        scroll_offset: None,
                        history: None,
                    },
                    false,
                ),
                Target::Widget(data.id),
            ));
        }
    }
}

impl Widget<LapceTabData> for ProblemContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;

                if mouse_event.pos.y < self.content_height {
                    ctx.set_cursor(&Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }

                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event, data);
            }
            _ => {}
        }
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
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if !data
            .main_split
            .diagnostics
            .same(&old_data.main_split.diagnostics)
        {
            ctx.request_layout();
        }

        if data.problem.collapsed != old_data.problem.collapsed {
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
        let items = data.main_split.diagnostics_items(self.severity);
        let lines = items
            .iter()
            .map(|(path, diagnostics)| {
                if is_collapsed(data, path) {
                    1
                } else {
                    diagnostics.iter().map(|d| d.lines).sum::<usize>() + 1 /* file name header */
                }
            })
            .sum::<usize>();
        let line_height = data.config.editor.line_height() as f64;
        self.content_height = line_height * lines as f64;

        Size::new(bc.max().width, self.content_height.max(bc.max().height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let line_height = data.config.editor.line_height() as f64;
        let padding = (line_height - 14.0) / 2.0;
        let size = ctx.size();
        let mouse_line = (self.mouse_pos.y / line_height).floor() as usize;

        let rect = ctx.region().bounding_box();
        let min = (rect.y0 / line_height).floor() as usize;
        let max = (rect.y1 / line_height) as usize + 2;

        let ui_font_family = data.config.ui.font_family();
        let ui_font_size = data.config.ui.font_size() as f64;

        let items = data.main_split.diagnostics_items(self.severity);
        let mut current_line = 0;
        for (path, diagnostics) in items {
            let diagnostics_len =
                diagnostics.iter().map(|d| d.lines).sum::<usize>() + 1 /* file name header */;

            if !is_collapsed(data, path) && diagnostics_len + current_line < min {
                current_line += diagnostics_len + 1;
                continue;
            }

            let svg_size = data.config.ui.icon_size() as f64;
            let (svg, svg_color) = data.config.file_svg(path);
            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        (line_height - svg_size) / 2.0,
                        line_height * current_line as f64
                            + (line_height - svg_size) / 2.0,
                    ));
            ctx.draw_svg(&svg, rect, svg_color);

            let text_layout = ctx
                .text()
                .new_text_layout(
                    path.file_name().unwrap().to_str().unwrap().to_string(),
                )
                .font(ui_font_family.clone(), ui_font_size)
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
                    line_height,
                    line_height * current_line as f64
                        + text_layout.y_offset(line_height),
                ),
            );

            if is_collapsed(data, path) {
                current_line += 1;
                continue;
            }

            let folder = data
                .workspace
                .path
                .as_ref()
                .and_then(|workspace_path| path.strip_prefix(workspace_path).ok())
                .unwrap_or(path)
                .parent()
                .and_then(Path::to_str)
                .unwrap_or("")
                .to_string();

            if !folder.is_empty() {
                let x = text_layout.size().width + line_height + 5.0;

                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
                    .font(ui_font_family.clone(), ui_font_size)
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
                        x,
                        line_height * current_line as f64
                            + text_layout.y_offset(line_height),
                    ),
                );
            }

            for d in diagnostics {
                if current_line > max {
                    return;
                }
                let msg_lines = message_lines(d);
                let related_lines = related_line_count(d);
                if current_line + 1 + msg_lines + related_lines < min {
                    current_line += msg_lines + related_lines;
                    continue;
                }

                if ctx.is_hot()
                    && current_line < mouse_line
                    && mouse_line < current_line + 1 + msg_lines
                {
                    ctx.fill(
                        Size::new(size.width, line_height * msg_lines as f64)
                            .to_rect()
                            .with_origin(Point::new(
                                0.0,
                                line_height * (current_line + 1) as f64,
                            )),
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }

                let svg = match self.severity {
                    DiagnosticSeverity::ERROR => {
                        data.config.ui_svg(LapceIcons::ERROR)
                    }
                    _ => data.config.ui_svg(LapceIcons::WARNING),
                };
                let rect = Size::new(line_height, line_height)
                    .to_rect()
                    .with_origin(Point::new(
                        line_height,
                        line_height * (current_line + 1) as f64,
                    ))
                    .inflate(-padding, -padding);
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                    ),
                );

                for line in d.diagnostic.message.lines() {
                    current_line += 1;
                    let text_layout = ctx
                        .text()
                        .new_text_layout(line.to_string())
                        .font(ui_font_family.clone(), ui_font_size)
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
                            2.0 * line_height,
                            line_height * current_line as f64
                                + text_layout.y_offset(line_height),
                        ),
                    );
                }

                for related in
                    d.diagnostic.related_information.as_deref().unwrap_or(&[])
                {
                    current_line += 1;

                    if ctx.is_hot() && mouse_line >= current_line {
                        let lines = related.message.lines().count() + 1;
                        if mouse_line < current_line + lines {
                            ctx.fill(
                                Size::new(size.width, line_height * lines as f64)
                                    .to_rect()
                                    .with_origin(Point::new(
                                        0.0,
                                        line_height * current_line as f64,
                                    )),
                                data.config.get_color_unchecked(
                                    LapceTheme::EDITOR_CURRENT_LINE,
                                ),
                            );
                        }
                    }

                    let svg = data.config.ui_svg(LapceIcons::LINK);
                    let rect = Size::new(line_height, line_height)
                        .to_rect()
                        .with_origin(Point::new(
                            2.0 * line_height,
                            line_height * current_line as f64,
                        ))
                        .inflate(-padding, -padding);
                    ctx.draw_svg(
                        &svg,
                        rect,
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                        ),
                    );
                    let path = path_from_url(&related.location.uri);
                    let text = format!(
                        "{}[{}, {}]:",
                        path.file_name().and_then(|f| f.to_str()).unwrap_or(""),
                        related.location.range.start.line,
                        related.location.range.start.character,
                    );
                    let text_layout = ctx
                        .text()
                        .new_text_layout(text)
                        .font(ui_font_family.clone(), ui_font_size)
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
                            3.0 * line_height,
                            line_height * current_line as f64
                                + text_layout.y_offset(line_height),
                        ),
                    );
                    for line in related.message.lines() {
                        current_line += 1;

                        let text_layout = ctx
                            .text()
                            .new_text_layout(line.to_string())
                            .font(ui_font_family.clone(), ui_font_size)
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
                                3.0 * line_height,
                                line_height * current_line as f64
                                    + text_layout.y_offset(line_height),
                            ),
                        );
                    }
                }
            }
            current_line += 1;
        }
    }
}

fn message_lines(diagnostic: &EditorDiagnostic) -> usize {
    diagnostic.diagnostic.message.lines().count()
}

fn related_line_count(diagnostic: &EditorDiagnostic) -> usize {
    diagnostic
        .diagnostic
        .related_information
        .as_ref()
        .map(|r| r.iter().map(|r| r.message.lines().count() + 1).sum())
        .unwrap_or(0)
}
