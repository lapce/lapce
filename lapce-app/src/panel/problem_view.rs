use std::{path::PathBuf, sync::Arc};

use floem::{
    peniko::Color,
    reactive::{
        create_memo, create_rw_signal, ReadSignal, SignalGet, SignalUpdate,
        SignalWith,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{container, label, list, scroll, stack, svg, Decorators},
    ViewContext,
};
use lsp_types::{DiagnosticRelatedInformation, DiagnosticSeverity};

use super::{position::PanelPosition, view::panel_header};
use crate::{
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::{DiagnosticData, EditorDiagnostic},
    editor::location::{EditorLocation, EditorPosition},
    listener::Listener,
    proxy::path_from_url,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

pub fn problem_panel(
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let is_bottom = position.is_bottom();
    stack(|| {
        (
            stack(|| {
                (
                    panel_header("Errors".to_string(), config),
                    problem_section(
                        window_tab_data.clone(),
                        DiagnosticSeverity::ERROR,
                    ),
                )
            })
            .style(move || {
                let config = config.get();
                Style::BASE
                    .flex_col()
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .apply_if(is_bottom, |s| s.border_right(1.0))
                    .apply_if(!is_bottom, |s| s.border_bottom(1.0))
            }),
            stack(|| {
                (
                    panel_header("Warnings".to_string(), config),
                    problem_section(window_tab_data, DiagnosticSeverity::WARNING),
                )
            })
            .style(|| Style::BASE.flex_col().flex_basis_px(0.0).flex_grow(1.0)),
        )
    })
    .style(move || {
        Style::BASE
            .size_pct(100.0, 100.0)
            .apply_if(!is_bottom, |s| s.flex_col())
    })
}

fn problem_section(
    window_tab_data: Arc<WindowTabData>,
    severity: DiagnosticSeverity,
) -> impl View {
    let config = window_tab_data.common.config;
    let main_split = window_tab_data.main_split.clone();
    let internal_command = window_tab_data.common.internal_command;
    container(|| {
        scroll(move || {
            let workspace = main_split.common.workspace.clone();
            list(
                move || main_split.diagnostics.get(),
                |(p, _)| p.clone(),
                move |(path, diagnostic_data)| {
                    file_view(
                        workspace.clone(),
                        path,
                        diagnostic_data,
                        severity,
                        internal_command,
                        config,
                    )
                },
            )
            .style(|| Style::BASE.flex_col().width_pct(100.0).line_height(1.6))
        })
        .scroll_bar_color(move || {
            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
        })
        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
    })
    .style(|| Style::BASE.size_pct(100.0, 100.0))
}

fn file_view(
    workspace: Arc<LapceWorkspace>,
    path: PathBuf,
    diagnostic_data: DiagnosticData,
    severity: DiagnosticSeverity,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let cx = ViewContext::get_current();
    let collpased = create_rw_signal(cx.scope, false);

    let diagnostics = create_memo(cx.scope, move |_| {
        let diagnostics = diagnostic_data.diagnostics.get();
        let diagnostics: im::Vector<EditorDiagnostic> = diagnostics
            .into_iter()
            .filter_map(|d| {
                if d.diagnostic.severity == Some(severity) {
                    Some(d)
                } else {
                    None
                }
            })
            .collect();
        diagnostics
    });

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

    stack(move || {
        (
            stack(|| {
                (
                    container(|| {
                        stack(|| {
                            (
                                label(move || file_name.clone()).style(|| {
                                    Style::BASE
                                        .margin_right_px(6.0)
                                        .max_width_pct(100.0)
                                        .text_ellipsis()
                                }),
                                label(move || folder.clone()).style(move || {
                                    Style::BASE
                                        .color(
                                            *config
                                                .get()
                                                .get_color(LapceColor::EDITOR_DIM),
                                        )
                                        .min_width_px(0.0)
                                        .text_ellipsis()
                                }),
                            )
                        })
                        .style(move || {
                            Style::BASE.width_pct(100.0).min_width_px(0.0)
                        })
                    })
                    .on_click(move |_| {
                        collpased.update(|collpased| *collpased = !*collpased);
                        true
                    })
                    .style(move || {
                        Style::BASE
                            .width_pct(100.0)
                            .min_width_px(0.0)
                            .padding_left_px(
                                10.0 + (config.get().ui.icon_size() as f32 + 6.0)
                                    * 2.0,
                            )
                            .padding_right_px(10.0)
                    })
                    .hover_style(move || {
                        Style::BASE.cursor(CursorStyle::Pointer).background(
                            *config
                                .get()
                                .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    }),
                    stack(|| {
                        (
                            svg(move || {
                                config.get().ui_svg(if collpased.get() {
                                    LapceIcons::ITEM_CLOSED
                                } else {
                                    LapceIcons::ITEM_OPENED
                                })
                            })
                            .style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .margin_right_px(6.0)
                                    .size_px(size, size)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            svg(move || config.get().file_svg(&path).0).style(
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color =
                                        config.file_svg(&style_path).1.copied();
                                    Style::BASE
                                        .min_width_px(size)
                                        .size_px(size, size)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            label(|| " ".to_string()),
                        )
                    })
                    .style(|| {
                        Style::BASE.absolute().items_center().margin_left_px(10.0)
                    }),
                )
            })
            .style(move || Style::BASE.width_pct(100.0).min_width_px(0.0)),
            list(
                move || {
                    if collpased.get() {
                        im::Vector::new()
                    } else {
                        diagnostics.get()
                    }
                },
                |_| 0,
                move |d| {
                    item_view(
                        full_path.clone(),
                        d,
                        icon,
                        icon_color,
                        internal_command,
                        config,
                    )
                },
            )
            .style(|| Style::BASE.flex_col().width_pct(100.0).min_width_pct(0.0)),
        )
    })
    .style(move || {
        Style::BASE
            .width_pct(100.0)
            .items_start()
            .flex_col()
            .apply_if(diagnostics.with(|d| d.is_empty()), |s| s.hide())
    })
}

fn item_view(
    path: PathBuf,
    d: EditorDiagnostic,
    icon: &'static str,
    icon_color: impl Fn() -> Color + 'static,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let related = d.diagnostic.related_information.unwrap_or_default();
    let location = EditorLocation {
        path,
        position: Some(EditorPosition::Position(d.diagnostic.range.start)),
        scroll_offset: None,
        ignore_unconfirmed: false,
        same_editor_tab: false,
    };
    stack(|| {
        (
            container(|| {
                stack(|| {
                    (
                        label(move || d.diagnostic.message.clone()).style(
                            move || {
                                Style::BASE
                                    .width_pct(100.0)
                                    .min_width_px(0.0)
                                    .padding_left_px(
                                        10.0 + (config.get().ui.icon_size() as f32
                                            + 6.0)
                                            * 3.0,
                                    )
                                    .padding_right_px(10.0)
                            },
                        ),
                        stack(|| {
                            (
                                svg(move || config.get().ui_svg(icon)).style(
                                    move || {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        Style::BASE
                                            .size_px(size, size)
                                            .color(icon_color())
                                    },
                                ),
                                label(|| " ".to_string()),
                            )
                        })
                        .style(move || {
                            Style::BASE.absolute().items_center().margin_left_px(
                                10.0 + (config.get().ui.icon_size() as f32 + 6.0)
                                    * 2.0,
                            )
                        }),
                    )
                })
                .style(move || Style::BASE.width_pct(100.0).min_width_px(0.0))
                .hover_style(move || {
                    Style::BASE.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
            .on_click(move |_| {
                internal_command.send(InternalCommand::JumpToLocation {
                    location: location.clone(),
                });
                true
            })
            .style(|| Style::BASE.width_pct(100.0).min_width_pct(0.0)),
            related_view(related, internal_command, config),
        )
    })
    .style(|| Style::BASE.width_pct(100.0).min_width_pct(0.0).flex_col())
}

fn related_view(
    related: Vec<DiagnosticRelatedInformation>,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let is_empty = related.is_empty();
    stack(move || {
        (
            list(
                move || related.clone(),
                |_| 0,
                move |related| {
                    let full_path = path_from_url(&related.location.uri);
                    let path = full_path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .map(|f| {
                            format!(
                                "{f} [{}, {}]: ",
                                related.location.range.start.line,
                                related.location.range.start.character
                            )
                        })
                        .unwrap_or_else(|| "".to_string());
                    let location = EditorLocation {
                        path: full_path,
                        position: Some(EditorPosition::Position(
                            related.location.range.start,
                        )),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    };
                    let message = format!("{path}{}", related.message);
                    container(|| {
                        label(move || message.clone()).style(move || {
                            Style::BASE.width_pct(100.0).min_width_px(0.0)
                        })
                    })
                    .on_click(move |_| {
                        internal_command.send(InternalCommand::JumpToLocation {
                            location: location.clone(),
                        });
                        true
                    })
                    .style(move || {
                        Style::BASE
                            .padding_left_px(
                                10.0 + (config.get().ui.icon_size() as f32 + 6.0)
                                    * 4.0,
                            )
                            .padding_right_px(10.0)
                            .width_pct(100.0)
                            .min_width_px(0.0)
                    })
                    .hover_style(move || {
                        Style::BASE.cursor(CursorStyle::Pointer).background(
                            *config
                                .get()
                                .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
                },
            )
            .style(|| Style::BASE.width_pct(100.0).min_width_px(0.0).flex_col()),
            stack(|| {
                (
                    svg(move || config.get().ui_svg(LapceIcons::LINK)).style(
                        move || {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            Style::BASE
                                .size_px(size, size)
                                .color(*config.get_color(LapceColor::EDITOR_DIM))
                        },
                    ),
                    label(|| " ".to_string()),
                )
            })
            .style(move || {
                Style::BASE.absolute().items_center().margin_left_px(
                    10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 3.0,
                )
            }),
        )
    })
    .style(move || {
        Style::BASE
            .width_pct(100.0)
            .min_width_px(0.0)
            .items_start()
            .color(*config.get().get_color(LapceColor::EDITOR_DIM))
            .apply_if(is_empty, |s| s.hide())
    })
}
