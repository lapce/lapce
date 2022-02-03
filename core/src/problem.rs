use std::{path::PathBuf, sync::Arc};

use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll, SvgData},
    Affine, BoxConstraints, Color, Command, Cursor, Data, Env, Event, EventCtx,
    FontFamily, FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent,
    PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2,
    Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};
use itertools::Itertools;
use lsp_types::DiagnosticSeverity;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{EditorDiagnostic, FocusArea, LapceTabData, PanelKind},
    editor::EditorLocationNew,
    panel::{LapcePanel, PanelHeaderKind, PanelSection},
    split::{LapceSplitNew, SplitDirection},
    svg::{file_svg_new, get_svg},
};

pub struct ProblemData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub error_widget_id: WidgetId,
    pub warning_widget_id: WidgetId,
}

impl ProblemData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            error_widget_id: WidgetId::next(),
            warning_widget_id: WidgetId::next(),
        }
    }

    pub fn new_panel(&self) -> LapcePanel {
        LapcePanel::new(
            PanelKind::Problem,
            self.widget_id,
            self.split_id,
            SplitDirection::Vertical,
            PanelHeaderKind::Simple("Problem".to_string()),
            vec![
                (
                    self.error_widget_id,
                    PanelHeaderKind::Simple("Errors".to_string()),
                    ProblemContent::new(DiagnosticSeverity::Error).boxed(),
                    None,
                ),
                (
                    self.warning_widget_id,
                    PanelHeaderKind::Simple("Warnings".to_string()),
                    ProblemContent::new(DiagnosticSeverity::Warning).boxed(),
                    None,
                ),
            ],
        )
    }
}

pub struct ProblemContent {
    severity: DiagnosticSeverity,
    mouse_pos: Point,
}

impl ProblemContent {
    pub fn new(severity: DiagnosticSeverity) -> Self {
        Self {
            severity,
            mouse_pos: Point::ZERO,
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
                if diagnostics.len() > 0 {
                    Some((path, diagnostics))
                } else {
                    None
                }
            })
            .sorted_by_key(|(path, _)| path.clone())
            .collect();
        items
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &LapceTabData,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let n = (mouse_event.pos.y / line_height).floor() as usize;

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
                                        .unwrap_or(d.diagnositc.range.start.clone()),
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
                                    position: Some(
                                        related.location.range.start.clone(),
                                    ),
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
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                ctx.set_cursor(&Cursor::Pointer);
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
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
        let height = line_height * n as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let line_height = data.config.editor.line_height as f64;

        if ctx.is_hot() {
            let size = ctx.size();
            let n = (self.mouse_pos.y / line_height).floor() as usize;
            ctx.fill(
                Size::new(size.width, line_height)
                    .to_rect()
                    .with_origin(Point::new(0.0, line_height * n as f64)),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
            );
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
            if let Some(workspace) = data.workspace.as_ref() {
                path = path
                    .strip_prefix(&workspace.path)
                    .unwrap_or(&path)
                    .to_path_buf();
            }
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if folder != "" {
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
                            PathBuf::from(related.location.uri.path())
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
