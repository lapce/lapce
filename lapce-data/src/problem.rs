use std::path::PathBuf;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext,
    Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
};
use itertools::Itertools;
use lsp_types::DiagnosticSeverity;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{EditorDiagnostic, LapceTabData, PanelKind},
    editor::EditorLocationNew,
    split::SplitDirection,
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
}

impl Default for ProblemData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProblemContent {
    severity: DiagnosticSeverity,
    mouse_pos: Point,
    line_height: f64,
}

impl ProblemContent {
    pub fn new(severity: DiagnosticSeverity) -> Self {
        Self {
            severity,
            mouse_pos: Point::ZERO,
            line_height: 25.0,
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
