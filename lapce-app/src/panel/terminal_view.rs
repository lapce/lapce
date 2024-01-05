use std::rc::Rc;

use floem::{
    event::{Event, EventListener},
    kurbo::Size,
    reactive::create_rw_signal,
    view::View,
    views::{
        container, dyn_stack, empty, label,
        scroll::{scroll, Thickness},
        stack, svg, tab, Decorators,
    },
    EventPropagation,
};

use super::kind::PanelKind;
use crate::{
    app::clickable_icon,
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons},
    debug::RunDebugMode,
    terminal::{
        panel::TerminalPanelData, tab::TerminalTabData, view::terminal_view,
    },
    window_tab::{Focus, WindowTabData},
};

pub fn terminal_panel(window_tab_data: Rc<WindowTabData>) -> impl View {
    let focus = window_tab_data.common.focus;
    stack((
        terminal_tab_header(window_tab_data.clone()),
        terminal_tab_content(window_tab_data),
    ))
    .on_event_cont(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::Terminal) {
            focus.set(Focus::Panel(PanelKind::Terminal));
        }
    })
    .style(|s| s.absolute().size_pct(100.0, 100.0).flex_col())
}

fn terminal_tab_header(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let active_index = move || terminal.tab_info.with(|info| info.active);
    let tab_info = terminal.tab_info;
    let header_width = create_rw_signal(0.0);
    let header_height = create_rw_signal(0.0);
    let icon_width = create_rw_signal(0.0);
    let scroll_size = create_rw_signal(Size::ZERO);
    let workbench_command = window_tab_data.common.workbench_command;

    stack((
        scroll(dyn_stack(
            move || {
                let tabs = terminal.tab_info.with(|info| info.tabs.clone());
                for (i, (index, _)) in tabs.iter().enumerate() {
                    if index.get_untracked() != i {
                        index.set(i);
                    }
                }
                tabs
            },
            |(_, tab)| tab.terminal_tab_id,
            move |(index, tab)| {
                let terminal = terminal.clone();
                let local_terminal = terminal.clone();
                let terminal_tab_id = tab.terminal_tab_id;

                let title = {
                    let tab = tab.clone();
                    move || {
                        let terminal = tab.active_terminal(true);
                        let run_debug = terminal.as_ref().map(|t| t.run_debug);
                        if let Some(run_debug) = run_debug {
                            if let Some(name) = run_debug.with(|run_debug| {
                                run_debug.as_ref().map(|r| r.config.name.clone())
                            }) {
                                return name;
                            }
                        }

                        let title = terminal.map(|t| t.title);
                        let title = title.map(|t| t.get());
                        title.unwrap_or_default()
                    }
                };

                let svg_string = move || {
                    let terminal = tab.active_terminal(true);
                    let run_debug = terminal.as_ref().map(|t| t.run_debug);
                    if let Some(run_debug) = run_debug {
                        if let Some((mode, stopped)) = run_debug.with(|run_debug| {
                            run_debug.as_ref().map(|r| (r.mode, r.stopped))
                        }) {
                            let svg = match (mode, stopped) {
                                (RunDebugMode::Run, false) => LapceIcons::START,
                                (RunDebugMode::Run, true) => LapceIcons::RUN_ERRORS,
                                (RunDebugMode::Debug, false) => LapceIcons::DEBUG,
                                (RunDebugMode::Debug, true) => {
                                    LapceIcons::DEBUG_DISCONNECT
                                }
                            };
                            return svg;
                        }
                    }
                    LapceIcons::TERMINAL
                };
                stack((
                    container({
                        stack((
                            container(
                                svg(move || config.get().ui_svg(svg_string()))
                                    .style(move |s| {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        s.size(size, size).color(
                                            config.color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                    }),
                            )
                            .style(|s| s.padding_horiz(10.0).padding_vert(12.0)),
                            label(title).style(|s| {
                                s.min_width(0.0)
                                    .flex_basis(0.0)
                                    .flex_grow(1.0)
                                    .text_ellipsis()
                            }),
                            clickable_icon(
                                || LapceIcons::CLOSE,
                                move || {
                                    terminal.close_tab(Some(terminal_tab_id));
                                },
                                || false,
                                || false,
                                config,
                            )
                            .style(|s| s.margin_horiz(6.0)),
                            empty().style(move |s| {
                                s.absolute()
                                    .width_full()
                                    .height(header_height.get() - 15.0)
                                    .border_right(1.0)
                                    .border_color(
                                        config.get().color(LapceColor::LAPCE_BORDER),
                                    )
                            }),
                        ))
                        .style(move |s| {
                            s.items_center().width(200.0).border_color(
                                config.get().color(LapceColor::LAPCE_BORDER),
                            )
                        })
                    })
                    .style(|s| s.items_center()),
                    container({
                        label(|| "".to_string()).style(move |s| {
                            s.size_pct(100.0, 100.0)
                                .border_bottom(if active_index() == index.get() {
                                    2.0
                                } else {
                                    0.0
                                })
                                .border_color(config.get().color(
                                    if focus.get()
                                        == Focus::Panel(PanelKind::Terminal)
                                    {
                                        LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                                    } else {
                                        LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                                    },
                                ))
                        })
                    })
                    .style(|s| {
                        s.absolute().padding_horiz(3.0).size_pct(100.0, 100.0)
                    }),
                ))
                .on_event_cont(
                    EventListener::PointerDown,
                    move |_| {
                        if tab_info.with_untracked(|tab| tab.active)
                            != index.get_untracked()
                        {
                            tab_info.update(|tab| {
                                tab.active = index.get_untracked();
                            });
                            local_terminal.update_debug_active_term();
                        }
                    },
                )
            },
        ))
        .vertical_scroll_as_horizontal(|| true)
        .on_resize(move |rect| {
            if rect.size() != scroll_size.get_untracked() {
                scroll_size.set(rect.size());
            }
        })
        .style(move |s| {
            let header_width = header_width.get();
            let icon_width = icon_width.get();
            s.absolute()
                .max_width(header_width - icon_width)
                .set(Thickness, 3)
        }),
        empty().style(move |s| {
            let size = scroll_size.get();
            s.size(size.width, size.height)
        }),
        container(clickable_icon(
            || LapceIcons::ADD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::NewTerminalTab);
            },
            || false,
            || false,
            config,
        ))
        .on_resize(move |rect| {
            let width = rect.size().width;
            if icon_width.get_untracked() != width {
                icon_width.set(width);
            }
        })
        .style(|s| s.padding_horiz(10)),
    ))
    .on_resize(move |rect| {
        let size = rect.size();
        if header_width.get_untracked() != size.width {
            header_width.set(size.width);
        }
        if header_height.get_untracked() != size.height {
            header_height.set(size.height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .items_center()
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    })
}

fn terminal_tab_split(
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
) -> impl View {
    let config = terminal_panel_data.common.config;
    let active = terminal_tab_data.active;
    let terminal_tab_scope = terminal_tab_data.scope;
    dyn_stack(
        move || {
            let terminals = terminal_tab_data.terminals.get();
            for (i, (index, _)) in terminals.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            terminals
        },
        |(_, terminal)| terminal.term_id,
        move |(index, terminal)| {
            let terminal_panel_data = terminal_panel_data.clone();
            let terminal_scope = terminal.scope;
            container({
                terminal_view(
                    terminal.term_id,
                    terminal.raw.read_only(),
                    terminal.mode.read_only(),
                    terminal.run_debug.read_only(),
                    terminal_panel_data,
                    terminal.launch_error,
                )
                .on_event_cont(EventListener::PointerDown, move |_| {
                    active.set(index.get_untracked());
                })
                .on_event(EventListener::PointerWheel, move |event| {
                    if let Event::PointerWheel(pointer_event) = event {
                        terminal.clone().wheel_scroll(pointer_event.delta.y);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_cleanup(move || {
                    terminal_scope.dispose();
                })
                .style(|s| s.size_pct(100.0, 100.0))
            })
            .style(move |s| {
                s.size_pct(100.0, 100.0).padding_horiz(10.0).apply_if(
                    index.get() > 0,
                    |s| {
                        s.border_left(1.0).border_color(
                            config.get().color(LapceColor::LAPCE_BORDER),
                        )
                    },
                )
            })
        },
    )
    .on_cleanup(move || {
        terminal_tab_scope.dispose();
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

fn terminal_tab_content(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |(_, tab)| terminal_tab_split(terminal.clone(), tab),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}
