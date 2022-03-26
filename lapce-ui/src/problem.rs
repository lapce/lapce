use std::path::PathBuf;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext,
    Size, Target, UpdateCtx, Widget, WidgetExt,
};
use itertools::Itertools;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{EditorDiagnostic, LapceTabData, PanelKind},
    editor::EditorLocationNew,
    problem::ProblemData,
    proxy::path_from_url,
    split::SplitDirection,
};
use lsp_types::DiagnosticSeverity;

use crate::{
    panel::{LapcePanel, PanelHeaderKind},
    svg::{file_svg_new, get_svg},
};

pub fn new_problem_panel(data: &ProblemData) -> LapcePanel {
    LapcePanel::new(
        PanelKind::Problem,
        data.widget_id,
        data.split_id,
        SplitDirection::Vertical,
        PanelHeaderKind::Simple("Problem".to_string()),
        vec![
            (
                data.error_widget_id,
                PanelHeaderKind::Simple("Errors".to_string()),
                ProblemContent::new(DiagnosticSeverity::Error).boxed(),
                None,
            ),
            (
                data.warning_widget_id,
                PanelHeaderKind::Simple("Warnings".to_string()),
                ProblemContent::new(DiagnosticSeverity::Warning).boxed(),
                None,
            ),
        ],
    )
}

pub struct ProblemContent {
    severity: DiagnosticSeverity,
    mouse_pos: Point,
    line_height: f64,
    content_height: f64,
}

impl ProblemContent {
    pub fn new(severity: DiagnosticSeverity) -> Self {
        Self {
            severity,
            mouse_pos: Point::ZERO,
            line_height: 25.0,
            content_height: 0.0,
        }
    }

    fn items<'a>(
        &self,
        data: &'a LapceTabData,
    ) -> Vec<(&'a PathBuf, Vec<&'a EditorDiagnostic>)> {
        let items: Vec<(&PathBuf, Vec<&EditorDiagnostic>)> = data
            .main_split
            .diagnostics
            .iter()
            .filter_map(|(path, diagnostic)| {
                let diagnostics: Vec<&EditorDiagnostic> = diagnostic
                    .iter()
                    .filter(|d| d.diagnositc.severity == Some(self.severity))
                    .collect();
                if !diagnostics.is_empty() {
                    Some((path, diagnostics))
                } else {
                    None
                }
            })
            .sorted_by_key(|(path, _)| (*path).clone())
            .collect();
        items
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &LapceTabData,
    ) {
        let n = (mouse_event.pos.y / self.line_height).floor() as usize;

        let items = self.items(data);
        let mut i = 0;
        for (path, diagnositcs) in items {
            let diagnositcs_len = diagnositcs
                .iter()
                .map(|d| {
                    d.diagnositc
                        .related_information
                        .as_ref()
                        .map(|r| r.len())
                        .unwrap_or(0)
                        + 1
                })
                .sum::<usize>();
            if diagnositcs_len + 1 + i < n {
                i += diagnositcs_len + 1;
                continue;
            }

            for d in diagnositcs {
                i += 1;
                if i == n {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::JumpToLocation(
                            None,
                            EditorLocationNew {
                                path: path.clone(),
                                position: Some(
                                    d.range
                                        .map(|(line, col)| lsp_types::Position {
                                            line: line as u32,
                                            character: col as u32,
                                        })
                                        .unwrap_or_else(|| d.diagnositc.range.start),
                                ),
                                scroll_offset: None,
                                hisotry: None,
                            },
                        ),
                        Target::Widget(data.id),
                    ));
                    return;
                }
                for related in d
                    .diagnositc
                    .related_information
                    .as_ref()
                    .unwrap_or(&Vec::new())
                {
                    i += 1;
                    if i == n {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::JumpToLocation(
                                None,
                                EditorLocationNew {
                                    path: related
                                        .location
                                        .uri
                                        .to_file_path()
                                        .unwrap(),
                                    position: Some(related.location.range.start),
                                    scroll_offset: None,
                                    hisotry: None,
                                },
                            ),
                            Target::Widget(data.id),
                        ));
                        return;
                    }
                }
            }
            i += 1;
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
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let items = self.items(data);
        let n = items
            .iter()
            .map(|(_, diagnositcs)| {
                diagnositcs
                    .iter()
                    .map(|d| {
                        d.diagnositc
                            .related_information
                            .as_ref()
                            .map(|r| r.len())
                            .unwrap_or(0)
                            + 1
                    })
                    .sum::<usize>()
                    + 1
            })
            .sum::<usize>();
        let line_height = data.config.editor.line_height as f64;
        self.content_height = line_height * n as f64;

        Size::new(bc.max().width, self.content_height.max(bc.max().height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let line_height = data.config.editor.line_height as f64;

        if ctx.is_hot() {
            let size = ctx.size();
            if self.mouse_pos.y < self.content_height {
                let n = (self.mouse_pos.y / line_height).floor() as usize;
                ctx.fill(
                    Size::new(size.width, line_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, line_height * n as f64)),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
        }

        let rect = ctx.region().bounding_box();
        let min = (rect.y0 / line_height).floor() as usize;
        let max = (rect.y1 / line_height) as usize + 2;

        let items = self.items(data);
        let mut i = 0;
        for (path, diagnositcs) in items {
            let diagnositcs_len = diagnositcs
                .iter()
                .map(|d| {
                    d.diagnositc
                        .related_information
                        .as_ref()
                        .map(|r| r.len())
                        .unwrap_or(0)
                        + 1
                })
                .sum::<usize>();
            if diagnositcs_len + 1 + i < min {
                i += diagnositcs_len + 1;
                continue;
            }

            let padding = (line_height - 14.0) / 2.0;
            let svg = file_svg_new(path);
            let rect = Size::new(line_height, line_height)
                .to_rect()
                .with_origin(Point::new(0.0, line_height * i as f64))
                .inflate(-padding, -padding);
            ctx.draw_svg(&svg, rect, None);

            let text_layout = ctx
                .text()
                .new_text_layout(
                    path.file_name().unwrap().to_str().unwrap().to_string(),
                )
                .font(FontFamily::SYSTEM_UI, 13.0)
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
                    line_height * i as f64
                        + (line_height - text_layout.size().height) / 2.0,
                ),
            );

            let mut path = path.clone();
            if let Some(workspace_path) = data.workspace.path.as_ref() {
                path = path
                    .strip_prefix(workspace_path)
                    .unwrap_or(&path)
                    .to_path_buf();
            }
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !folder.is_empty() {
                let x = text_layout.size().width + line_height + 5.0;

                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
                    .font(FontFamily::SYSTEM_UI, 13.0)
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
                        line_height * i as f64
                            + (line_height - text_layout.size().height) / 2.0,
                    ),
                );
            }

            for d in diagnositcs {
                i += 1;
                if i > max {
                    return;
                }

                if i >= min {
                    let svg = match self.severity {
                        DiagnosticSeverity::Error => get_svg("error.svg").unwrap(),
                        _ => get_svg("warning.svg").unwrap(),
                    };
                    let rect = Size::new(line_height, line_height)
                        .to_rect()
                        .with_origin(Point::new(line_height, line_height * i as f64))
                        .inflate(-padding, -padding);
                    ctx.draw_svg(
                        &svg,
                        rect,
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );

                    let text_layout = ctx
                        .text()
                        .new_text_layout(d.diagnositc.message.clone())
                        .font(FontFamily::SYSTEM_UI, 13.0)
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
                            line_height * i as f64
                                + (line_height - text_layout.size().height) / 2.0,
                        ),
                    );
                }

                for related in d
                    .diagnositc
                    .related_information
                    .as_ref()
                    .unwrap_or(&Vec::new())
                {
                    i += 1;

                    if i >= min {
                        let svg = get_svg("link.svg").unwrap();
                        let rect = Size::new(line_height, line_height)
                            .to_rect()
                            .with_origin(Point::new(
                                2.0 * line_height,
                                line_height * i as f64,
                            ))
                            .inflate(-padding, -padding);
                        ctx.draw_svg(
                            &svg,
                            rect,
                            Some(
                                data.config.get_color_unchecked(
                                    LapceTheme::EDITOR_FOREGROUND,
                                ),
                            ),
                        );

                        let text = format!(
                            "{}[{}, {}]: {}",
                            path_from_url(&related.location.uri)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or(""),
                            related.location.range.start.line,
                            related.location.range.start.character,
                            related.message
                        );
                        let text_layout = ctx
                            .text()
                            .new_text_layout(text)
                            .font(FontFamily::SYSTEM_UI, 13.0)
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
                                line_height * i as f64
                                    + (line_height - text_layout.size().height)
                                        / 2.0,
                            ),
                        );
                    }
                }
            }
            i += 1;
        }
    }
}
