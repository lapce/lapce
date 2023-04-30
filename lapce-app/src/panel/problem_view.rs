use std::{path::PathBuf, sync::Arc};

use floem::{
    reactive::{create_memo, ReadSignal, SignalGet, SignalWith},
    style::Style,
    view::View,
    views::{container, label, list, scroll, stack, svg, Decorators},
    AppContext,
};
use lsp_types::DiagnosticSeverity;

use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::EditorDiagnostic,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

use super::{position::PanelPosition, view::panel_header};

pub fn problem_panel(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let is_bottom = position.is_bottom();
    stack(cx, |cx| {
        (
            stack(cx, move |cx| {
                (panel_header(cx, "Errors".to_string(), config),)
            })
            .style(cx, move || {
                let config = config.get();
                Style::BASE
                    .flex_col()
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .apply_if(is_bottom, |s| s.border_right(1.0))
                    .apply_if(!is_bottom, |s| s.border_bottom(1.0))
            }),
            stack(cx, |cx| {
                (
                    panel_header(cx, "Warnings".to_string(), config),
                    problem_section(
                        cx,
                        window_tab_data,
                        DiagnosticSeverity::WARNING,
                    ),
                )
            })
            .style(cx, || {
                Style::BASE.flex_col().flex_basis_px(0.0).flex_grow(1.0)
            }),
        )
    })
    .style(cx, move || {
        Style::BASE
            .dimension_pct(1.0, 1.0)
            .apply_if(!is_bottom, |s| s.flex_col())
    })
}

fn problem_section(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    severity: DiagnosticSeverity,
) -> impl View {
    let config = window_tab_data.common.config;
    let main_split = window_tab_data.main_split.clone();
    container(cx, |cx| {
        scroll(cx, move |cx| {
            let main_split = main_split.clone();
            let workspace = main_split.common.workspace.clone();
            list(
                cx,
                move || main_split.diagnostics_items(severity, true),
                |(p, _)| p.clone(),
                move |cx, (path, diagnostics)| {
                    file_view(
                        cx,
                        workspace.clone(),
                        path,
                        diagnostics,
                        severity,
                        config,
                    )
                },
            )
            .style(cx, || Style::BASE.flex_col().line_height(1.6))
        })
        .style(cx, || Style::BASE.absolute().dimension_pct(1.0, 1.0))
    })
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
}

fn file_view(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    path: PathBuf,
    diagnostics: Vec<EditorDiagnostic>,
    severity: DiagnosticSeverity,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let full_path = path.clone();
    let path = if let Some(workspace_path) = workspace.path.as_ref() {
        path.strip_prefix(workspace_path)
            .unwrap_or(&full_path)
            .to_path_buf()
    } else {
        path
    };
    let style_path = path.clone();

    let icon = match severity {
        DiagnosticSeverity::ERROR => LapceIcons::ERROR,
        _ => LapceIcons::WARNING,
    };
    let icon_color = move || {
        let config = config.get();
        match severity {
            DiagnosticSeverity::ERROR => *config.get_color(LapceColor::LAPCE_ERROR),
            _ => *config.get_color(LapceColor::LAPCE_WARN),
        }
    };

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let folder = path
        .parent()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    stack(cx, move |cx| {
        (
            stack(cx, |cx| {
                (
                    svg(cx, move || config.get().file_svg(&path).0).style(
                        cx,
                        move || {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            let color = config.file_svg(&style_path).1.copied();
                            Style::BASE
                                .min_width_px(size)
                                .dimension_px(size, size)
                                .apply_opt(color, Style::color)
                        },
                    ),
                    label(cx, || " ".to_string()),
                )
            })
            .style(cx, || Style::BASE.items_center()),
            stack(cx, |cx| {
                (
                    stack(cx, |cx| {
                        (
                            label(cx, move || file_name.clone()).style(cx, || {
                                Style::BASE.margin_right(6.0).max_width_pct(1.0)
                            }),
                            label(cx, move || folder.clone()).style(cx, move || {
                                Style::BASE
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                        )
                    }),
                    list(
                        cx,
                        move || diagnostics.clone(),
                        |_| 0,
                        move |cx, d| {
                            let link = d.diagnostic.related_information;
                            stack(cx, |cx| {
                                (
                                    stack(cx, |cx| {
                                        (
                                            svg(cx, move || {
                                                config.get().ui_svg(icon)
                                            })
                                            .style(cx, move || {
                                                let config = config.get();
                                                let size =
                                                    config.ui.icon_size() as f32;
                                                Style::BASE
                                                    .dimension_px(size, size)
                                                    .color(icon_color())
                                            }),
                                            label(cx, || " ".to_string()),
                                        )
                                    })
                                    .style(cx, move || Style::BASE.items_center()),
                                    stack(cx, |cx| {
                                        (
                                            label(cx, move || {
                                                d.diagnostic.message.clone()
                                            }),
                                            stack(cx, |cx| {
                                                (
                                                    stack(cx, |cx| {
                                                        (
                                                            label(cx, || {
                                                                " ".to_string()
                                                            }),
                                                            svg(cx, move || {
                                                                config.get().ui_svg(
                                                                    LapceIcons::LINK,
                                                                )
                                                            })
                                                            .style(cx, move || {
                                                                let config =
                                                                    config.get();
                                                                let size = config
                                                                    .ui
                                                                    .icon_size()
                                                                    as f32;
                                                                Style::BASE
                                                    .dimension_px(size, size)
                                                    .color(*config.get_color(
                                                    LapceColor::EDITOR_DIM,
                                                ))
                                                            }),
                                                            label(cx, || {
                                                                " ".to_string()
                                                            }),
                                                        )
                                                    })
                                                    .style(cx, move || {
                                                        Style::BASE.items_center()
                                                    }),
                                                    label(cx, || {
                                                        "linked information"
                                                            .to_string()
                                                    })
                                                    .style(cx, move || {
                                                        Style::BASE.color(
                                                    *config.get().get_color(
                                                        LapceColor::EDITOR_DIM,
                                                    ),
                                                )
                                                    }),
                                                )
                                            })
                                            .style(cx, || Style::BASE.items_start()),
                                        )
                                    })
                                    .style(cx, || Style::BASE.flex_col()),
                                )
                            })
                            .style(cx, || Style::BASE.items_start())
                        },
                    )
                    .style(cx, || Style::BASE.flex_col()),
                )
            })
            .style(cx, || Style::BASE.flex_col()),
        )
    })
    .style(cx, || Style::BASE.items_start().margin_horiz(10.0))
}
