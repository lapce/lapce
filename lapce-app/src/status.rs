use std::{
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
};

use floem::{
    reactive::{create_memo, ReadSignal, RwSignal},
    style::{AlignItems, CursorStyle, Display},
    view::View,
    views::{label, list, stack, svg, Decorators},
};
use indexmap::IndexMap;
use lapce_core::mode::Mode;
use lsp_types::{DiagnosticSeverity, ProgressToken};

use crate::{
    app::clickable_icon,
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    listener::Listener,
    palette::kind::PaletteKind,
    panel::{kind::PanelKind, position::PanelContainerPosition},
    source_control::SourceControlData,
    window_tab::{WindowTabData, WorkProgress},
};

pub fn status(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    status_height: RwSignal<f64>,
    _config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let editor = window_tab_data.main_split.active_editor;
    let panel = window_tab_data.panel.clone();
    let palette = window_tab_data.palette.clone();
    let diagnostic_count = create_memo(move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.diagnostics.get().iter() {
                if let Some(severity) = diagnostic.diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });
    let branch = source_control.branch;
    let file_diffs = source_control.file_diffs;
    let branch = move || {
        format!(
            "{}{}",
            branch.get(),
            if file_diffs.with(|diffs| diffs.is_empty()) {
                ""
            } else {
                "*"
            }
        )
    };

    let progresses = window_tab_data.progresses;
    let mode = create_memo(move |_| window_tab_data.mode());

    stack(move || {
        (
            stack(|| {
                (
                    label(move || match mode.get() {
                        Mode::Normal => "Normal".to_string(),
                        Mode::Insert => "Insert".to_string(),
                        Mode::Visual => "Visual".to_string(),
                        Mode::Terminal => "Terminal".to_string(),
                    })
                    .style(move |s| {
                        let config = config.get();
                        let display = if config.core.modal {
                            Display::Flex
                        } else {
                            Display::None
                        };

                        let (bg, fg) = match mode.get() {
                            Mode::Normal => (
                                LapceColor::STATUS_MODAL_NORMAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_NORMAL_FOREGROUND,
                            ),
                            Mode::Insert => (
                                LapceColor::STATUS_MODAL_INSERT_BACKGROUND,
                                LapceColor::STATUS_MODAL_INSERT_FOREGROUND,
                            ),
                            Mode::Visual => (
                                LapceColor::STATUS_MODAL_VISUAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_VISUAL_FOREGROUND,
                            ),
                            Mode::Terminal => (
                                LapceColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                            ),
                        };

                        let bg = *config.get_color(bg);
                        let fg = *config.get_color(fg);

                        s.display(display)
                            .padding_horiz_px(10.0)
                            .color(fg)
                            .background(bg)
                            .height_pct(100.0)
                            .align_items(Some(AlignItems::Center))
                    }),
                    stack(move || {
                        (
                            svg(move || config.get().ui_svg(LapceIcons::SCM)).style(
                                move |s| {
                                    let config = config.get();
                                    let icon_size = config.ui.icon_size() as f32;
                                    s.size_px(icon_size, icon_size).color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                                },
                            ),
                            label(branch).style(|s| s.margin_left_px(10.0)),
                        )
                    })
                    .style(move |s| {
                        s.display(if branch().is_empty() {
                            Display::None
                        } else {
                            Display::Flex
                        })
                        .height_pct(100.0)
                        .padding_horiz_px(10.0)
                        .align_items(Some(AlignItems::Center))
                    })
                    .on_click(move |_| {
                        workbench_command
                            .send(LapceWorkbenchCommand::PaletteSCMReferences);
                        true
                    })
                    .hover_style(move |s| {
                        s.cursor(CursorStyle::Pointer).background(
                            *config
                                .get()
                                .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    }),
                    {
                        let panel = panel.clone();
                        stack(|| {
                            (
                                svg(move || config.get().ui_svg(LapceIcons::ERROR))
                                    .style(move |s| {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        s.size_px(size, size).color(
                                            *config.get_color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                    }),
                                label(move || diagnostic_count.get().0.to_string())
                                    .style(|s| s.margin_left_px(5.0)),
                                svg(move || {
                                    config.get().ui_svg(LapceIcons::WARNING)
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    s.size_px(size, size).margin_left_px(5.0).color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                                }),
                                label(move || diagnostic_count.get().1.to_string())
                                    .style(|s| s.margin_left_px(5.0)),
                            )
                        })
                        .on_click(move |_| {
                            panel.show_panel(&PanelKind::Problem);
                            true
                        })
                        .style(|s| {
                            s.height_pct(100.0).padding_horiz_px(10.0).items_center()
                        })
                        .hover_style(move |s| {
                            s.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    },
                    progress_view(progresses),
                )
            })
            .style(|s| {
                s.height_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .items_center()
            }),
            stack(move || {
                (
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Left,
                                    true,
                                ) {
                                    LapceIcons::SIDEBAR_LEFT
                                } else {
                                    LapceIcons::SIDEBAR_LEFT_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Left,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Bottom,
                                    true,
                                ) {
                                    LapceIcons::LAYOUT_PANEL
                                } else {
                                    LapceIcons::LAYOUT_PANEL_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Bottom,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Right,
                                    true,
                                ) {
                                    LapceIcons::SIDEBAR_RIGHT
                                } else {
                                    LapceIcons::SIDEBAR_RIGHT_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Right,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                )
            })
            .style(|s| s.height_pct(100.0).items_center()),
            stack(|| {
                let palette_clone = palette.clone();
                let cursor_info = label(move || {
                    if let Some(editor) = editor.get() {
                        let mut status = String::new();
                        let cursor = editor.cursor.get();
                        if let Some((line, column, character)) = editor
                            .view
                            .doc
                            .get()
                            .buffer
                            .with(|buffer| cursor.get_line_col_char(buffer))
                        {
                            status = format!(
                                "Ln {}, Col {}, Char {}",
                                line, column, character,
                            );
                        }
                        if let Some(selection) = cursor.get_selection() {
                            let selection_range = selection.0.abs_diff(selection.1);

                            if selection.0 != selection.1 {
                                status =
                                    format!("{status} ({selection_range} selected)");
                            }
                        }
                        let selection_count = cursor.get_selection_count();
                        if selection_count > 1 {
                            status =
                                format!("{status} {selection_count} selections");
                        }
                        return status;
                    }
                    String::new()
                })
                .on_click(move |_| {
                    palette_clone.run(PaletteKind::Line);
                    true
                })
                .style(move |s| {
                    s.display(
                        if editor
                            .get()
                            .map(|editor| {
                                editor.view.doc.get().content.with(|c| c.is_file())
                            })
                            .unwrap_or(false)
                        {
                            Display::Flex
                        } else {
                            Display::None
                        },
                    )
                    .height_pct(100.0)
                    .padding_horiz_px(10.0)
                    .items_center()
                })
                .hover_style(move |s| {
                    s.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                });
                let palette_clone = palette.clone();
                let language_info = label(move || {
                    if let Some(editor) = editor.get() {
                        let doc = editor.view.doc.get_untracked();
                        doc.syntax.with(|s| s.language.to_string())
                    } else {
                        String::new()
                    }
                })
                .on_click(move |_| {
                    palette_clone.run(PaletteKind::Language);
                    true
                })
                .style(move |s| {
                    s.display(
                        if editor
                            .get()
                            .map(|editor| {
                                editor.view.doc.get().content.with(|c| c.is_file())
                            })
                            .unwrap_or(false)
                        {
                            Display::Flex
                        } else {
                            Display::None
                        },
                    )
                    .height_pct(100.0)
                    .padding_horiz_px(10.0)
                    .items_center()
                })
                .hover_style(move |s| {
                    s.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                });
                (cursor_info, language_info)
            })
            .style(|s| {
                s.height_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .justify_end()
            }),
        )
    })
    .on_resize(move |rect| {
        let height = rect.height();
        if height != status_height.get_untracked() {
            status_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.border_top(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::STATUS_BACKGROUND))
            .height_px(config.ui.status_height() as f32)
            .align_items(Some(AlignItems::Center))
    })
}

fn progress_view(
    progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
) -> impl View {
    let id = AtomicU64::new(0);
    list(
        move || progresses.get(),
        move |_| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        move |(_, p)| {
            stack(|| {
                (label(move || p.title.clone()), {
                    let message = p.message.unwrap_or_default();
                    let is_empty = message.is_empty();
                    label(move || format!(": {message}")).style(move |s| {
                        s.min_width_px(0.0)
                            .text_ellipsis()
                            .apply_if(is_empty, |s| s.hide())
                    })
                })
            })
            .style(|s| s.margin_left_px(10.0))
        },
    )
}
