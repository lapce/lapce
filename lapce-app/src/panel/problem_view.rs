use std::{path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    peniko::Color,
    reactive::{create_memo, create_rw_signal, ReadSignal},
    style::{CursorStyle, Style},
    view::View,
    views::{container, dyn_stack, label, scroll, stack, svg, Decorators},
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
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let is_bottom = position.is_bottom();
    stack((
        stack((
            panel_header("Errors".to_string(), config),
            problem_section(window_tab_data.clone(), DiagnosticSeverity::ERROR),
        ))
        .style(move |s| {
            let config = config.get();
            s.flex_col()
                .flex_basis(0.0)
                .flex_grow(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .apply_if(is_bottom, |s| s.border_right(1.0))
                .apply_if(!is_bottom, |s| s.border_bottom(1.0))
        }),
        stack((
            panel_header("Warnings".to_string(), config),
            problem_section(window_tab_data, DiagnosticSeverity::WARNING),
        ))
        .style(|s| s.flex_col().flex_basis(0.0).flex_grow(1.0)),
    ))
    .style(move |s| {
        s.size_pct(100.0, 100.0)
            .apply_if(!is_bottom, |s| s.flex_col())
    })
}

fn problem_section(
    window_tab_data: Rc<WindowTabData>,
    severity: DiagnosticSeverity,
) -> impl View {
    let config = window_tab_data.common.config;
    let main_split = window_tab_data.main_split.clone();
    let internal_command = window_tab_data.common.internal_command;
    container({
        scroll(
            dyn_stack(
                move || main_split.diagnostics.get(),
                |(p, _)| p.clone(),
                move |(path, diagnostic_data)| {
                    file_view(
                        main_split.common.workspace.clone(),
                        path,
                        diagnostic_data,
                        severity,
                        internal_command,
                        config,
                    )
                },
            )
            .style(|s| s.flex_col().width_pct(100.0).line_height(1.6)),
        )
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

fn file_view(
    workspace: Arc<LapceWorkspace>,
    path: PathBuf,
    diagnostic_data: DiagnosticData,
    severity: DiagnosticSeverity,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let collpased = create_rw_signal(false);

    let diagnostics = create_memo(move |_| {
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
            DiagnosticSeverity::ERROR => config.color(LapceColor::LAPCE_ERROR),
            _ => config.color(LapceColor::LAPCE_WARN),
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

    stack((
        stack((
            container(
                stack((
                    label(move || file_name.clone()).style(|s| {
                        s.margin_right(6.0).max_width_pct(100.0).text_ellipsis()
                    }),
                    label(move || folder.clone()).style(move |s| {
                        s.color(config.get().color(LapceColor::EDITOR_DIM))
                            .min_width(0.0)
                            .text_ellipsis()
                    }),
                ))
                .style(move |s| s.width_pct(100.0).min_width(0.0)),
            )
            .on_click_stop(move |_| {
                collpased.update(|collpased| *collpased = !*collpased);
            })
            .style(move |s| {
                let config = config.get();
                s.width_pct(100.0)
                    .min_width(0.0)
                    .padding_left(10.0 + (config.ui.icon_size() as f32 + 6.0) * 2.0)
                    .padding_right(10.0)
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            stack((
                svg(move || {
                    config.get().ui_svg(if collpased.get() {
                        LapceIcons::ITEM_CLOSED
                    } else {
                        LapceIcons::ITEM_OPENED
                    })
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    s.margin_right(6.0)
                        .size(size, size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
                svg(move || config.get().file_svg(&path).0).style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = config.file_svg(&style_path).1;
                    s.min_width(size)
                        .size(size, size)
                        .apply_opt(color, Style::color)
                }),
                label(|| " ".to_string()),
            ))
            .style(|s| s.absolute().items_center().margin_left(10.0)),
        ))
        .style(move |s| s.width_pct(100.0).min_width(0.0)),
        dyn_stack(
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
        .style(|s| s.flex_col().width_pct(100.0).min_width_pct(0.0)),
    ))
    .style(move |s| {
        s.width_pct(100.0)
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
    stack((
        container({
            stack((
                label(move || d.diagnostic.message.clone()).style(move |s| {
                    s.width_pct(100.0)
                        .min_width(0.0)
                        .padding_left(
                            10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 3.0,
                        )
                        .padding_right(10.0)
                }),
                stack((
                    svg(move || config.get().ui_svg(icon)).style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size).color(icon_color())
                    }),
                    label(|| " ".to_string()),
                ))
                .style(move |s| {
                    s.absolute().items_center().margin_left(
                        10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 2.0,
                    )
                }),
            ))
            .style(move |s| {
                s.width_pct(100.0).min_width(0.0).hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
        })
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::JumpToLocation {
                location: location.clone(),
            });
        })
        .style(|s| s.width_pct(100.0).min_width_pct(0.0)),
        related_view(related, internal_command, config),
    ))
    .style(|s| s.width_pct(100.0).min_width_pct(0.0).flex_col())
}

fn related_view(
    related: Vec<DiagnosticRelatedInformation>,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let is_empty = related.is_empty();
    stack((
        dyn_stack(
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
                    .unwrap_or_default();
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
                container(
                    label(move || message.clone())
                        .style(move |s| s.width_pct(100.0).min_width(0.0)),
                )
                .on_click_stop(move |_| {
                    internal_command.send(InternalCommand::JumpToLocation {
                        location: location.clone(),
                    });
                })
                .style(move |s| {
                    let config = config.get();
                    s.padding_left(10.0 + (config.ui.icon_size() as f32 + 6.0) * 4.0)
                        .padding_right(10.0)
                        .width_pct(100.0)
                        .min_width(0.0)
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
        )
        .style(|s| s.width_pct(100.0).min_width(0.0).flex_col()),
        stack((
            svg(move || config.get().ui_svg(LapceIcons::LINK)).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            label(|| " ".to_string()),
        ))
        .style(move |s| {
            s.absolute()
                .items_center()
                .margin_left(10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 3.0)
        }),
    ))
    .style(move |s| {
        s.width_pct(100.0)
            .min_width(0.0)
            .items_start()
            .color(config.get().color(LapceColor::EDITOR_DIM))
            .apply_if(is_empty, |s| s.hide())
    })
}
