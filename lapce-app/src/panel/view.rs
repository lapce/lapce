use std::sync::Arc;

use floem::{
    reactive::{ReadSignal, SignalGet, SignalWith},
    style::Style,
    view::View,
    views::{container, container_box, label, list, stack, tab, Decorators},
    AppContext,
};

use crate::{
    app::clickable_icon,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    file_explorer::view::file_explorer_panel,
    window_tab::WindowTabData,
};

use super::{
    debug_view::debug_panel,
    kind::PanelKind,
    position::{PanelContainerPosition, PanelPosition},
    problem_view::problem_panel,
    terminal_view::terminal_panel,
};

pub fn panel_container_view(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelContainerPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    let is_bottom = position.is_bottom();
    stack(cx, |cx| {
        (
            panel_picker(cx, window_tab_data.clone(), position.first()),
            panel_view(cx, window_tab_data.clone(), position.first()),
            panel_view(cx, window_tab_data.clone(), position.second()),
            panel_picker(cx, window_tab_data.clone(), position.second()),
        )
    })
    .style(cx, move || {
        let size = panel.size.with(|s| match position {
            PanelContainerPosition::Left => s.left,
            PanelContainerPosition::Bottom => s.bottom,
            PanelContainerPosition::Right => s.right,
        });
        let is_maximized = panel.panel_bottom_maximized(true);
        let config = config.get();
        Style::BASE
            .apply_if(!panel.is_container_shown(&position, true), |s| s.hide())
            .apply_if(position == PanelContainerPosition::Bottom, |s| {
                // s.border_top(1.0)
                s.width_pct(100.0)
                    .apply_if(!is_maximized, |s| s.height_px(size as f32))
                    .apply_if(is_maximized, |s| s.flex_grow(1.0))
            })
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.border_right(1.0)
                    .width_px(size as f32)
                    .height_pct(100.0)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.border_left(1.0)
                    .width_px(size as f32)
                    .height_pct(100.0)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(!is_bottom, |s| s.flex_col())
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .color(*config.get_color(LapceColor::PANEL_FOREGROUND))
    })
}

fn panel_view(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        cx,
        active_fn,
        panels,
        |p| *p,
        move |cx, kind| {
            let view = match kind {
                PanelKind::Terminal => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::FileExplorer => container_box(cx, |cx| {
                    Box::new(file_explorer_panel(
                        cx,
                        window_tab_data.clone(),
                        position,
                    ))
                }),
                PanelKind::SourceControl => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Plugin => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Search => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Problem => container_box(cx, |cx| {
                    Box::new(problem_panel(cx, window_tab_data.clone(), position))
                }),
                PanelKind::Debug => container_box(cx, |cx| {
                    Box::new(debug_panel(cx, window_tab_data.clone(), position))
                }),
            };
            view.style(cx, || Style::BASE.size_pct(100.0, 100.0))
        },
    )
    .style(cx, move || {
        Style::BASE
            .size_pct(100.0, 100.0)
            .apply_if(!panel.is_position_shown(&position, true), |s| s.hide())
    })
}

pub fn panel_header(
    cx: AppContext,
    header: String,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(cx, |cx| label(cx, move || header.clone())).style(cx, move || {
        Style::BASE
            .padding_horiz_px(10.0)
            .padding_vert_px(6.0)
            .width_pct(100.0)
            .background(*config.get().get_color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn panel_picker(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = panel.panels;
    let config = window_tab_data.common.config;
    let is_bottom = position.is_bottom();
    let is_first = position.is_first();
    list(
        cx,
        move || {
            panel
                .panels
                .with(|panels| panels.get(&position).cloned().unwrap_or_default())
        },
        |p| *p,
        move |cx, p| {
            let window_tab_data = window_tab_data.clone();
            let icon = match p {
                PanelKind::Terminal => LapceIcons::TERMINAL,
                PanelKind::FileExplorer => LapceIcons::FILE_EXPLORER,
                PanelKind::SourceControl => LapceIcons::SCM,
                PanelKind::Plugin => LapceIcons::EXTENSIONS,
                PanelKind::Search => LapceIcons::SEARCH,
                PanelKind::Problem => LapceIcons::PROBLEM,
                PanelKind::Debug => LapceIcons::DEBUG_ALT,
            };
            let is_active = {
                let window_tab_data = window_tab_data.clone();
                move || {
                    if let Some((active_panel, _)) = window_tab_data
                        .panel
                        .active_panel_at_position(&position, true)
                    {
                        active_panel == p
                    } else {
                        false
                    }
                }
            };
            container(cx, |cx| {
                stack(cx, |cx| {
                    (
                        clickable_icon(
                            cx,
                            || icon,
                            move || {
                                window_tab_data.toggle_panel_visual(p);
                            },
                            || false,
                            config,
                        )
                        .style(cx, || Style::BASE.padding_px(1.0)),
                        label(cx, || "".to_string()).style(cx, move || {
                            Style::BASE
                                .absolute()
                                .size_pct(100.0, 100.0)
                                .apply_if(!is_bottom && is_first, |s| {
                                    s.margin_top_px(2.0)
                                })
                                .apply_if(!is_bottom && !is_first, |s| {
                                    s.margin_top_px(-2.0)
                                })
                                .apply_if(is_bottom && is_first, |s| {
                                    s.margin_left_px(-2.0)
                                })
                                .apply_if(is_bottom && !is_first, |s| {
                                    s.margin_left_px(2.0)
                                })
                                .apply_if(is_active(), |s| {
                                    s.apply_if(!is_bottom && is_first, |s| {
                                        s.border_bottom(2.0)
                                    })
                                    .apply_if(!is_bottom && !is_first, |s| {
                                        s.border_top(2.0)
                                    })
                                    .apply_if(is_bottom && is_first, |s| {
                                        s.border_left(2.0)
                                    })
                                    .apply_if(is_bottom && !is_first, |s| {
                                        s.border_right(2.0)
                                    })
                                })
                                .border_color(*config.get().get_color(
                                    LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE,
                                ))
                        }),
                    )
                })
            })
            .style(cx, || Style::BASE.padding_px(6.0))
        },
    )
    .style(cx, move || {
        Style::BASE
            .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
            .apply_if(
                panels.with(|p| {
                    p.get(&position).map(|p| p.is_empty()).unwrap_or(true)
                }),
                |s| s.hide(),
            )
            .apply_if(is_bottom, |s| s.flex_col())
            .apply_if(is_bottom && is_first, |s| s.border_right(1.0))
            .apply_if(is_bottom && !is_first, |s| s.border_left(1.0))
            .apply_if(!is_bottom && is_first, |s| s.border_bottom(1.0))
            .apply_if(is_bottom && !is_first, |s| s.border_top(1.0))
    })
}

fn blank_panel(cx: AppContext) -> impl View {
    label(cx, || "blank".to_string())
}
