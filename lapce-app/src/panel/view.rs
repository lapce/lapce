use std::{rc::Rc, sync::Arc};

use floem::{
    event::{Event, EventListener},
    kurbo::{Point, Size},
    reactive::{create_rw_signal, ReadSignal, RwSignal},
    style::CursorStyle,
    view::View,
    views::{
        container, container_box, dyn_stack, empty, label, stack, tab, Decorators,
    },
    EventPropagation,
};

use super::{
    debug_view::debug_panel,
    global_search_view::global_search_panel,
    kind::PanelKind,
    plugin_view::plugin_panel,
    position::{PanelContainerPosition, PanelPosition},
    problem_view::problem_panel,
    source_control_view::source_control_panel,
    terminal_view::terminal_panel,
};
use crate::{
    app::clickable_icon,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    file_explorer::view::file_explorer_panel,
    window_tab::{DragContent, WindowTabData},
};

pub fn panel_container_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelContainerPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let current_size = create_rw_signal(Size::ZERO);
    let available_size = window_tab_data.panel.available_size;
    let is_dragging_panel = move || {
        dragging
            .with_untracked(|d| d.as_ref().map(|d| d.is_panel()))
            .unwrap_or(false)
    };
    let drop_view = {
        let panel = panel.clone();
        move |position: PanelPosition| {
            let panel = panel.clone();
            let dragging_over = create_rw_signal(false);
            empty()
                .on_event(EventListener::DragEnter, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(true);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::DragLeave, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(false);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some(DragContent::Panel(kind)) = dragging.get_untracked()
                    {
                        dragging_over.set(false);
                        panel.move_panel_to_position(kind, &position);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .style(move |s| {
                    s.size_pct(100.0, 100.0).apply_if(dragging_over.get(), |s| {
                        s.background(
                            config
                                .get()
                                .color(LapceColor::EDITOR_DRAG_DROP_BACKGROUND),
                        )
                    })
                })
        }
    };

    let resize_drag_view = {
        let panel = panel.clone();
        let panel_size = panel.size;
        move |position: PanelContainerPosition| {
            panel.panel_info();
            let view = empty();
            let view_id = view.id();
            let drag_start: RwSignal<Option<Point>> = create_rw_signal(None);
            view.on_event_stop(EventListener::PointerDown, move |event| {
                view_id.request_active();
                if let Event::PointerDown(pointer_event) = event {
                    drag_start.set(Some(pointer_event.pos));
                }
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    if let Some(drag_start_point) = drag_start.get_untracked() {
                        let current_size = current_size.get_untracked();
                        let available_size = available_size.get_untracked();
                        match position {
                            PanelContainerPosition::Left => {
                                let new_size = current_size.width
                                    + pointer_event.pos.x
                                    - drag_start_point.x;
                                let current_panel_size = panel_size.get_untracked();
                                let new_size = new_size
                                    .max(150.0)
                                    .min(available_size.width - 150.0 - 150.0);
                                if new_size != current_panel_size.left {
                                    panel_size.update(|size| {
                                        size.left = new_size;
                                        size.right = size.right.min(
                                            available_size.width - new_size - 150.0,
                                        )
                                    })
                                }
                            }
                            PanelContainerPosition::Bottom => {
                                let new_size = current_size.height
                                    - (pointer_event.pos.y - drag_start_point.y);
                                let maximized = panel.panel_bottom_maximized(false);
                                if (maximized
                                    && new_size < available_size.height - 50.0)
                                    || (!maximized
                                        && new_size > available_size.height - 50.0)
                                {
                                    panel.toggle_bottom_maximize();
                                }

                                let new_size = new_size
                                    .max(100.0)
                                    .min(available_size.height - 100.0);
                                let current_size =
                                    panel_size.with_untracked(|s| s.bottom);
                                if current_size != new_size {
                                    panel_size.update(|size| {
                                        size.bottom = new_size;
                                    })
                                }
                            }
                            PanelContainerPosition::Right => {
                                let new_size = current_size.width
                                    - (pointer_event.pos.x - drag_start_point.x);
                                let current_panel_size = panel_size.get_untracked();
                                let new_size = new_size
                                    .max(150.0)
                                    .min(available_size.width - 150.0 - 150.0);
                                if new_size != current_panel_size.right {
                                    panel_size.update(|size| {
                                        size.right = new_size;
                                        size.left = size.left.min(
                                            available_size.width - new_size - 150.0,
                                        )
                                    })
                                }
                            }
                        }
                    }
                }
            })
            .on_event_stop(EventListener::PointerUp, move |_| {
                drag_start.set(None);
            })
            .style(move |s| {
                let is_dragging = drag_start.get().is_some();
                let current_size = current_size.get();
                let config = config.get();
                s.absolute()
                    .apply_if(position == PanelContainerPosition::Bottom, |s| {
                        s.width_pct(100.0).height(4.0).margin_top(-2.0)
                    })
                    .apply_if(position == PanelContainerPosition::Left, |s| {
                        s.width(4.0)
                            .margin_left(current_size.width as f32 - 2.0)
                            .height_pct(100.0)
                    })
                    .apply_if(position == PanelContainerPosition::Right, |s| {
                        s.width(4.0).margin_left(-2.0).height_pct(100.0)
                    })
                    .apply_if(is_dragging, |s| {
                        s.background(config.color(LapceColor::EDITOR_CARET))
                            .apply_if(
                                position == PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::RowResize),
                            )
                            .apply_if(
                                position != PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::ColResize),
                            )
                            .z_index(2)
                    })
                    .hover(|s| {
                        s.background(config.color(LapceColor::EDITOR_CARET))
                            .apply_if(
                                position == PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::RowResize),
                            )
                            .apply_if(
                                position != PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::ColResize),
                            )
                            .z_index(2)
                    })
            })
        }
    };

    let is_bottom = position.is_bottom();
    stack((
        panel_picker(window_tab_data.clone(), position.first()),
        panel_view(window_tab_data.clone(), position.first()),
        panel_view(window_tab_data.clone(), position.second()),
        panel_picker(window_tab_data.clone(), position.second()),
        resize_drag_view(position),
        stack((drop_view(position.first()), drop_view(position.second()))).style(
            move |s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .apply_if(!is_bottom, |s| s.flex_col())
            },
        ),
    ))
    .on_resize(move |rect| {
        let size = rect.size();
        if size != current_size.get_untracked() {
            current_size.set(size);
        }
    })
    .style(move |s| {
        let size = panel.size.with(|s| match position {
            PanelContainerPosition::Left => s.left,
            PanelContainerPosition::Bottom => s.bottom,
            PanelContainerPosition::Right => s.right,
        });
        let is_maximized = panel.panel_bottom_maximized(true);
        let config = config.get();
        s.apply_if(!panel.is_container_shown(&position, true), |s| s.hide())
            .apply_if(position == PanelContainerPosition::Bottom, |s| {
                s.width_pct(100.0)
                    .apply_if(!is_maximized, |s| {
                        s.border_top(1.0).height(size as f32)
                    })
                    .apply_if(is_maximized, |s| s.flex_grow(1.0))
            })
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.border_right(1.0)
                    .width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.border_left(1.0)
                    .width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(!is_bottom, |s| s.flex_col())
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .color(config.color(LapceColor::PANEL_FOREGROUND))
    })
}

fn panel_view(
    window_tab_data: Rc<WindowTabData>,
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
        active_fn,
        panels,
        |p| *p,
        move |kind| {
            let view = match kind {
                PanelKind::Terminal => {
                    container_box(terminal_panel(window_tab_data.clone()))
                }
                PanelKind::FileExplorer => container_box(file_explorer_panel(
                    window_tab_data.clone(),
                    position,
                )),
                PanelKind::SourceControl => container_box(source_control_panel(
                    window_tab_data.clone(),
                    position,
                )),
                PanelKind::Plugin => {
                    container_box(plugin_panel(window_tab_data.clone(), position))
                }
                PanelKind::Search => container_box(global_search_panel(
                    window_tab_data.clone(),
                    position,
                )),
                PanelKind::Problem => {
                    container_box(problem_panel(window_tab_data.clone(), position))
                }
                PanelKind::Debug => {
                    container_box(debug_panel(window_tab_data.clone(), position))
                }
            };
            view.style(|s| s.size_pct(100.0, 100.0))
        },
    )
    .style(move |s| {
        s.size_pct(100.0, 100.0).apply_if(
            !panel.is_position_shown(&position, true)
                || panel.is_position_empty(&position, true),
            |s| s.hide(),
        )
    })
}

pub fn panel_header(
    header: String,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(label(move || header.clone())).style(move |s| {
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .background(config.get().color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn panel_picker(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = panel.panels;
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let is_bottom = position.is_bottom();
    let is_first = position.is_first();
    dyn_stack(
        move || {
            panel
                .panels
                .with(|panels| panels.get(&position).cloned().unwrap_or_default())
        },
        |p| *p,
        move |p| {
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
                    if let Some((active_panel, shown)) = window_tab_data
                        .panel
                        .active_panel_at_position(&position, true)
                    {
                        shown && active_panel == p
                    } else {
                        false
                    }
                }
            };
            container(stack((
                clickable_icon(
                    || icon,
                    move || {
                        window_tab_data.toggle_panel_visual(p);
                    },
                    || false,
                    || false,
                    config,
                )
                .draggable()
                .on_event_stop(EventListener::DragStart, move |_| {
                    dragging.set(Some(DragContent::Panel(p)));
                })
                .on_event_stop(EventListener::DragEnd, move |_| {
                    dragging.set(None);
                })
                .dragging_style(move |s| {
                    let config = config.get();
                    s.border(1.0)
                        .border_radius(6.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                        .padding(6.0)
                        .background(
                            config
                                .color(LapceColor::PANEL_BACKGROUND)
                                .with_alpha_factor(0.7),
                        )
                })
                .style(|s| s.padding(1.0)),
                label(|| "".to_string()).style(move |s| {
                    s.absolute()
                        .size_pct(100.0, 100.0)
                        .apply_if(!is_bottom && is_first, |s| s.margin_top(2.0))
                        .apply_if(!is_bottom && !is_first, |s| s.margin_top(-2.0))
                        .apply_if(is_bottom && is_first, |s| s.margin_left(-2.0))
                        .apply_if(is_bottom && !is_first, |s| s.margin_left(2.0))
                        .apply_if(is_active(), |s| {
                            s.apply_if(!is_bottom && is_first, |s| {
                                s.border_bottom(2.0)
                            })
                            .apply_if(!is_bottom && !is_first, |s| s.border_top(2.0))
                            .apply_if(is_bottom && is_first, |s| s.border_left(2.0))
                            .apply_if(is_bottom && !is_first, |s| {
                                s.border_right(2.0)
                            })
                        })
                        .border_color(
                            config
                                .get()
                                .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                        )
                }),
            )))
            .style(|s| s.padding(6.0))
        },
    )
    .style(move |s| {
        s.border_color(config.get().color(LapceColor::LAPCE_BORDER))
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
            .apply_if(!is_bottom && !is_first, |s| s.border_top(1.0))
    })
}
