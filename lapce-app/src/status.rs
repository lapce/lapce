use std::sync::Arc;

use floem::{
    reactive::{create_memo, ReadSignal},
    style::{AlignItems, CursorStyle, Display},
    view::View,
    views::{label, stack, svg, Decorators},
};
use lapce_core::mode::Mode;
use lsp_types::DiagnosticSeverity;

use crate::{
    app::clickable_icon,
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    listener::Listener,
    palette::kind::PaletteKind,
    panel::{kind::PanelKind, position::PanelContainerPosition},
    source_control::SourceControlData,
    window_tab::WindowTabData,
};

pub fn status(
    window_tab_data: Arc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<LapceWorkbenchCommand>,
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

    let mode = create_memo(move |_| window_tab_data.mode());

    stack(move || {
        let editor = move || editor.clone().get();
        (
            stack(|| {
                (
                    label(move || match mode.get() {
                        Mode::Normal => "Normal".to_string(),
                        Mode::Insert => "Insert".to_string(),
                        Mode::Visual => "Visual".to_string(),
                        Mode::Terminal => "Terminal".to_string(),
                    })
                    .style(move  |base| {
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

                         base
                            .display(display)
                            .padding_horiz_px(10.0)
                            .color(fg)
                            .background(bg)
                            .height_pct(100.0)
                            .align_items(Some(AlignItems::Center))
                    }),
                    stack(move || {
                        (
                            svg(move || config.get().ui_svg(LapceIcons::SCM)).style(
                                move  |base| {
                                    let config = config.get();
                                    let icon_size = config.ui.icon_size() as f32;
                                     base.size_px(icon_size, icon_size).color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                                },
                            ),
                            label(branch).style( |base|  base.margin_left_px(10.0)),
                        )
                    })
                    .style(move  |base| {
                         base
                            .display(if branch().is_empty() {
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
                    .hover_style(move  |base| {
                         base.cursor(CursorStyle::Pointer).background(
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
                                    .style(move  |base| {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                         base.size_px(size, size).color(
                                            *config.get_color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                    }),
                                label(move || diagnostic_count.get().0.to_string())
                                    .style( |base|  base.margin_left_px(5.0)),
                                svg(move || {
                                    config.get().ui_svg(LapceIcons::WARNING)
                                })
                                .style(move  |base| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                     base
                                        .size_px(size, size)
                                        .margin_left_px(5.0)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                }),
                                label(move || diagnostic_count.get().1.to_string())
                                    .style( |base|  base.margin_left_px(5.0)),
                            )
                        })
                        .on_click(move |_| {
                            panel.show_panel(&PanelKind::Problem);
                            true
                        })
                        .style( |base| {
                             base
                                .height_pct(100.0)
                                .padding_horiz_px(10.0)
                                .items_center()
                        })
                        .hover_style(move  |base| {
                             base.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    },
                )
            })
            .style( |base| {
                 base
                    .height_pct(100.0)
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
            .style( |base|  base.height_pct(100.0).items_center()),
            stack(|| {
                let palette_clone = palette.clone();
                let cursor_info = label(move || {
                    if let Some(editor) = editor() {
                        let mut status = String::new();
                        let cursor = editor.get().cursor.get();
                        if let Some((line, column, character)) = cursor
                            .get_line_col_char(editor.get().view.doc.get().buffer())
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
                .style(move  |base| {
                     base
                        .display(
                            if editor()
                                .map(|f| f.get().view.doc.get().content.is_file())
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
                .hover_style(move  |base| {
                     base.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                });
                let palette_clone = palette.clone();
                let language_info = label(move || {
                    if let Some(editor) = editor() {
                        let doc = editor.with(|editor| editor.view.doc);
                        doc.with(|doc| doc.syntax().language.to_string())
                    } else {
                        String::new()
                    }
                })
                .on_click(move |_| {
                    palette_clone.run(PaletteKind::Language);
                    true
                })
                .style(move  |base| {
                     base
                        .display(
                            if editor()
                                .map(|f| f.get().view.doc.get().content.is_file())
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
                .hover_style(move  |base| {
                     base.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                });
                (cursor_info, language_info)
            })
            .style( |base| {
                 base
                    .height_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .justify_end()
            }),
        )
    })
    .style(move  |base| {
        let config = config.get();
         base
            .border_top(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::STATUS_BACKGROUND))
            .height_px(config.ui.status_height() as f32)
            .align_items(Some(AlignItems::Center))
    })
}
