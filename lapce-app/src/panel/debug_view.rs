use std::sync::Arc;

use floem::{
    cosmic_text::Style as FontStyle,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalGetUntracked, SignalSet,
        SignalUpdate, SignalWith, SignalWithUntracked,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{container, container_box, label, list, scroll, stack, svg, Decorators},
};
use lapce_rpc::{
    dap_types::{DapId, ThreadId},
    terminal::TermId,
};

use super::{position::PanelPosition, view::panel_header};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    debug::{RunDebugMode, StackTraceData},
    editor::location::{EditorLocation, EditorPosition},
    listener::Listener,
    terminal::panel::TerminalPanelData,
    window_tab::WindowTabData,
};

pub fn debug_panel(
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let terminal = window_tab_data.terminal.clone();
    let internal_command = window_tab_data.common.internal_command;

    stack(move || {
        (
            {
                let terminal = terminal.clone();
                stack(move || {
                    (
                        panel_header("Processes".to_string(), config),
                        debug_processes(terminal, config),
                    )
                })
                .style(|| Style::BASE.width_pct(100.0).flex_col().height_px(150.0))
            },
            stack(move || {
                (
                    panel_header("Stack Frames".to_string(), config),
                    debug_stack_traces(terminal, internal_command, config),
                )
            })
            .style(|| {
                Style::BASE
                    .width_pct(100.0)
                    .flex_grow(1.0)
                    .flex_basis_px(0.0)
                    .flex_col()
            }),
        )
    })
    .style(move || {
        Style::BASE
            .width_pct(100.0)
            .apply_if(!position.is_bottom(), |s| s.flex_col())
    })
}

fn debug_process_icons(
    terminal: TerminalPanelData,
    term_id: TermId,
    dap_id: DapId,
    mode: RunDebugMode,
    stopped: bool,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let paused = move || {
        let stopped = terminal
            .debug
            .daps
            .with_untracked(|daps| daps.get(&dap_id).map(|dap| dap.stopped));
        stopped.map(|stopped| stopped.get()).unwrap_or(false)
    };
    match mode {
        RunDebugMode::Run => container_box(|| {
            Box::new(stack(|| {
                (
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_RESTART,
                            move || {
                                terminal.restart_run_debug(term_id);
                            },
                            || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.margin_horiz_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_STOP,
                            move || {
                                terminal.stop_run_debug(term_id);
                            },
                            || false,
                            move || stopped,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::CLOSE,
                            move || {
                                terminal.close_terminal(&term_id);
                            },
                            || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                )
            }))
        }),
        RunDebugMode::Debug => container_box(|| {
            Box::new(stack(|| {
                (
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_CONTINUE,
                            move || {
                                terminal.dap_continue(term_id);
                            },
                            || false,
                            move || !paused() || stopped,
                            config,
                        )
                        .style(|| Style::BASE.margin_horiz_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_PAUSE,
                            move || {
                                terminal.dap_pause(term_id);
                            },
                            || false,
                            move || paused() || stopped,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_RESTART,
                            move || {
                                terminal.restart_run_debug(term_id);
                            },
                            || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_STOP,
                            move || {
                                terminal.stop_run_debug(term_id);
                            },
                            || false,
                            move || stopped,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            || LapceIcons::CLOSE,
                            move || {
                                terminal.close_terminal(&term_id);
                            },
                            || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.margin_right_px(6.0))
                    },
                )
            }))
        }),
    }
}

fn debug_processes(
    terminal: TerminalPanelData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    scroll(move || {
        let terminal = terminal.clone();
        let local_terminal = terminal.clone();
        list(
            move || local_terminal.run_debug_process(true),
            |(term_id, p)| (*term_id, p.stopped),
            move |(term_id, p)| {
                let terminal = terminal.clone();
                let is_active =
                    move || terminal.debug.active_term.get() == Some(term_id);
                let local_terminal = terminal.clone();
                stack(move || {
                    (
                        {
                            let svg_str = match (&p.mode, p.stopped) {
                                (RunDebugMode::Run, false) => LapceIcons::START,
                                (RunDebugMode::Run, true) => LapceIcons::RUN_ERRORS,
                                (RunDebugMode::Debug, false) => LapceIcons::DEBUG,
                                (RunDebugMode::Debug, true) => {
                                    LapceIcons::DEBUG_DISCONNECT
                                }
                            };
                            svg(move || config.get().ui_svg(svg_str)).style(
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    Style::BASE
                                        .size_px(size, size)
                                        .margin_horiz_px(10.0)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                },
                            )
                        },
                        label(move || p.config.name.clone()).style(|| {
                            Style::BASE
                                .flex_grow(1.0)
                                .flex_basis_px(0.0)
                                .min_width_px(0.0)
                                .text_ellipsis()
                        }),
                        debug_process_icons(
                            terminal.clone(),
                            term_id,
                            p.config.dap_id,
                            p.mode,
                            p.stopped,
                            config,
                        ),
                    )
                })
                .on_click(move |_| {
                    local_terminal.debug.active_term.set(Some(term_id));
                    local_terminal.focus_terminal(term_id);
                    true
                })
                .style(move || {
                    Style::BASE
                        .padding_vert_px(6.0)
                        .width_pct(100.0)
                        .items_center()
                        .apply_if(is_active(), |s| {
                            s.background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_CURRENT_BACKGROUND),
                            )
                        })
                })
                .hover_style(move || {
                    Style::BASE.cursor(CursorStyle::Pointer).background(
                        (*config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND))
                        .with_alpha_factor(0.3),
                    )
                })
            },
        )
        .style(|| Style::BASE.width_pct(100.0).flex_col())
    })
}

fn debug_stack_frames(
    thread_id: ThreadId,
    stack_trace: StackTraceData,
    stopped: RwSignal<bool>,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let expanded = stack_trace.expanded;
    stack(move || {
        (
            container(|| label(move || thread_id.to_string()))
                .on_click(move |_| {
                    expanded.update(|expanded| {
                        *expanded = !*expanded;
                    });
                    true
                })
                .style(|| Style::BASE.padding_horiz_px(10.0).min_width_pct(100.0))
                .hover_style(move || {
                    Style::BASE.cursor(CursorStyle::Pointer).background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                }),
            list(
                move || {
                    let expanded = stack_trace.expanded.get() && stopped.get();
                    if expanded {
                        stack_trace.frames.get()
                    } else {
                        im::Vector::new()
                    }
                },
                |frame| frame.id,
                move |frame| {
                    let full_path =
                        frame.source.as_ref().and_then(|s| s.path.clone());
                    let line = frame.line.saturating_sub(1);
                    let col = frame.column.saturating_sub(1);

                    let source_path = frame
                        .source
                        .as_ref()
                        .and_then(|s| s.path.as_ref())
                        .and_then(|p| p.file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let has_source = !source_path.is_empty();
                    let source_path = format!("{source_path}:{}", frame.line);

                    container(|| {
                        stack(|| {
                            (
                                label(move || frame.name.clone()).hover_style(
                                    move || {
                                        Style::BASE.background(
                                            *config.get().get_color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            ),
                                        )
                                    },
                                ),
                                label(move || source_path.clone()).style(
                                    move || {
                                        Style::BASE
                                            .margin_left_px(10.0)
                                            .color(
                                                *config.get().get_color(
                                                    LapceColor::EDITOR_DIM,
                                                ),
                                            )
                                            .font_style(FontStyle::Italic)
                                            .apply_if(!has_source, |s| s.hide())
                                    },
                                ),
                            )
                        })
                    })
                    .on_click(move |_| {
                        if let Some(path) = full_path.clone() {
                            internal_command.send(InternalCommand::JumpToLocation {
                                location: EditorLocation {
                                    path,
                                    position: Some(EditorPosition::Position(
                                        lsp_types::Position {
                                            line: line as u32,
                                            character: col as u32,
                                        },
                                    )),
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                },
                            });
                        }
                        true
                    })
                    .style(move || {
                        Style::BASE
                            .padding_left_px(20.0)
                            .padding_right_px(10.0)
                            .min_width_pct(100.0)
                            .apply_if(!has_source, |s| {
                                s.color(
                                    *config.get().get_color(LapceColor::EDITOR_DIM),
                                )
                            })
                    })
                    .hover_style(move || {
                        Style::BASE
                            .background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                            .apply_if(has_source, |s| s.cursor(CursorStyle::Pointer))
                    })
                },
            )
            .style(|| Style::BASE.flex_col().min_width_pct(100.0)),
        )
    })
    .style(|| Style::BASE.flex_col().min_width_pct(100.0))
}

fn debug_stack_traces(
    terminal: TerminalPanelData,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(move || {
        scroll(move || {
            let local_terminal = terminal.clone();
            list(
                move || {
                    let dap = local_terminal.get_active_dap(true);
                    if let Some(dap) = dap {
                        let process_stopped = local_terminal
                            .get_terminal(&dap.term_id)
                            .and_then(|t| {
                                t.run_debug.with(|r| r.as_ref().map(|r| r.stopped))
                            })
                            .unwrap_or(true);
                        if process_stopped {
                            return Vec::new();
                        }
                        let main_thread = dap.thread_id.get();
                        let stack_traces = dap.stack_traces.get();
                        let mut traces = stack_traces
                            .into_iter()
                            .map(|(thread_id, stack_trace)| {
                                (dap.dap_id, dap.stopped, thread_id, stack_trace)
                            })
                            .collect::<Vec<_>>();
                        traces.sort_by_key(|(_, _, id, _)| main_thread != Some(*id));
                        traces
                    } else {
                        Vec::new()
                    }
                },
                |(dap_id, stopped, thread_id, _)| {
                    (*dap_id, *thread_id, stopped.get_untracked())
                },
                move |(_, stopped, thread_id, stack_trace)| {
                    debug_stack_frames(
                        thread_id,
                        stack_trace,
                        stopped,
                        internal_command,
                        config,
                    )
                },
            )
            .style(|| Style::BASE.flex_col().min_width_pct(100.0))
        })
        .scroll_bar_color(move || {
            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
        })
        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
    })
    .style(|| {
        Style::BASE
            .width_pct(100.0)
            .line_height(1.6)
            .flex_grow(1.0)
            .flex_basis_px(0.0)
    })
}
