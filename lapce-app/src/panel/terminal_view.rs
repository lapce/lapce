use std::sync::Arc;

use floem::{
    event::{Event, EventListner},
    glazier::PointerType,
    reactive::{SignalGet, SignalGetUntracked, SignalSet, SignalWith},
    style::Style,
    view::View,
    views::{container, label, list, stack, svg, tab, Decorators},
    AppContext,
};

use crate::{
    app::clickable_icon,
    config::{color::LapceColor, icon::LapceIcons},
    debug::RunDebugMode,
    terminal::{
        panel::TerminalPanelData, tab::TerminalTabData, view::terminal_view,
    },
    window_tab::{Focus, WindowTabData},
};

use super::kind::PanelKind;

pub fn terminal_panel(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    stack(cx, |cx| {
        (
            terminal_tab_header(cx, window_tab_data.clone()),
            terminal_tab_content(cx, window_tab_data),
        )
    })
    .style(cx, || Style::BASE.size_pct(100.0, 100.0).flex_col())
}

fn terminal_tab_header(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let active = move || terminal.tab_info.with(|info| info.active);

    list(
        cx,
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
        move |cx, (index, tab)| {
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
            stack(cx, |cx| {
                (
                    container(cx, |cx| {
                        stack(cx, |cx| {
                            (
                                container(cx, |cx| {
                                    svg(cx, move || {
                                        config.get().ui_svg(svg_string())
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::BASE.size_px(size, size).color(
                                                *config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ),
                                            )
                                        },
                                    )
                                })
                                .style(cx, || {
                                    Style::BASE
                                        .padding_horiz_px(10.0)
                                        .padding_vert_px(11.0)
                                }),
                                label(cx, title).style(cx, || {
                                    Style::BASE
                                        .min_width_px(0.0)
                                        .flex_basis_px(0.0)
                                        .flex_grow(1.0)
                                        .text_ellipsis()
                                }),
                                clickable_icon(
                                    cx,
                                    || LapceIcons::CLOSE,
                                    || {},
                                    || false,
                                    config,
                                )
                                .style(cx, || Style::BASE.margin_horiz_px(6.0)),
                            )
                        })
                        .style(cx, move || {
                            Style::BASE
                                .items_center()
                                .width_px(200.0)
                                .border_right(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                    })
                    .style(cx, || Style::BASE.items_center()),
                    container(cx, |cx| {
                        label(cx, || "".to_string()).style(cx, move || {
                            Style::BASE
                                .size_pct(100.0, 100.0)
                                .border_bottom(if active() == index.get() {
                                    2.0
                                } else {
                                    0.0
                                })
                                .border_color(*config.get().get_color(
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
                    .style(cx, || {
                        Style::BASE
                            .absolute()
                            .padding_horiz_px(3.0)
                            .size_pct(100.0, 100.0)
                    }),
                )
            })
        },
    )
    .style(cx, move || {
        let config = config.get();
        Style::BASE
            .width_pct(100.0)
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
    })
}

fn terminal_tab_split(
    cx: AppContext,
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
) -> impl View {
    let config = terminal_panel_data.common.config;
    list(
        cx,
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
        move |cx, (index, terminal)| {
            let focus = terminal.common.focus;
            let terminal_panel_data = terminal_panel_data.clone();
            container(cx, move |cx| {
                terminal_view(
                    cx,
                    terminal.term_id,
                    terminal.raw.read_only(),
                    terminal.mode.read_only(),
                    terminal.run_debug.read_only(),
                    terminal_panel_data,
                )
                .on_event(EventListner::PointerWheel, move |event| {
                    if let Event::PointerWheel(pointer_event) = event {
                        if let PointerType::Mouse(info) = &pointer_event.pointer_type
                        {
                            terminal.clone().wheel_scroll(info.wheel_delta.y);
                        }
                        true
                    } else {
                        false
                    }
                })
                .style(cx, || Style::BASE.size_pct(100.0, 100.0))
            })
            .on_click(move |_| {
                focus.set(Focus::Panel(PanelKind::Terminal));
                true
            })
            .style(cx, move || {
                Style::BASE
                    .size_pct(100.0, 100.0)
                    .padding_horiz_px(10.0)
                    .apply_if(index.get() > 0, |s| {
                        s.border_left(1.0).border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                    })
            })
        },
    )
    .style(cx, || Style::BASE.size_pct(100.0, 100.0))
}

fn terminal_tab_content(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        cx,
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |cx, (_, tab)| terminal_tab_split(cx, terminal.clone(), tab),
    )
    .style(cx, || Style::BASE.size_pct(100.0, 100.0))
}
