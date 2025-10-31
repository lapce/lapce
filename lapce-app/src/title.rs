use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    event::EventListener,
    menu::{Menu, MenuItem},
    peniko::Color,
    prelude::SignalWith,
    reactive::{Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, create_memo},
    style::{AlignItems, CursorStyle, JustifyContent},
    views::{Decorators, container, drag_window_area, empty, label, stack, svg},
};
use lapce_core::meta;
use lapce_rpc::proxy::ProxyStatus;

use crate::{
    app::{clickable_icon, not_clickable_icon, tooltip_label, window_menu},
    command::{LapceCommand, LapceWorkbenchCommand, WindowCommand},
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    listener::Listener,
    main_split::MainSplitData,
    update::ReleaseInfo,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

fn left(
    workspace: Arc<LapceWorkspace>,
    lapce_command: Listener<LapceCommand>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
    proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
) -> impl View {
    let is_local = workspace.kind.is_local();
    let is_macos = cfg!(target_os = "macos");

    stack((
        empty().style(move |s| {
            let should_hide = if is_macos {
                num_window_tabs.get() > 1
            } else {
                true
            };
            s.width(75.0).apply_if(should_hide, |s| s.hide())
        }),
        container(svg(move || config.get().ui_svg(LapceIcons::LOGO)).style(
            move |s| {
                let config = config.get();
                let icon_size = config.ui.icon_size() as f32;
                s.size(icon_size, icon_size)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            },
        ))
        .style(move |s| s.margin_horiz(10.0).apply_if(is_macos, |s| s.hide())),
        title_button(
            lapce_icon(move || LapceIcons::MENU, config),
            || "Menu",
            config,
        )
        .popout_menu(move || window_menu(lapce_command, workbench_command))
        .style(move |s| s.apply_if(is_macos, |s| s.hide())),
        title_button(
            lapce_icon(move || LapceIcons::REMOTE, config).style(move |s| {
                let config = config.get();
                let size = (config.ui.icon_size() as f32 + 2.0).min(30.0);
                s.size(size, size).color(if is_local {
                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                } else {
                    match proxy_status.get() {
                        Some(_) => Color::WHITE,
                        None => config.color(LapceColor::LAPCE_ICON_ACTIVE),
                    }
                })
            }),
            || "Connect to Remote",
            config,
        )
        .popout_menu(move || {
            #[allow(unused_mut)]
            let mut menu = Menu::new("").entry(
                MenuItem::new("Connect to SSH Host").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::ConnectSshHost);
                }),
            );
            if !is_local
                && proxy_status.get().is_some_and(|p| {
                    matches!(p, ProxyStatus::Connecting | ProxyStatus::Connected)
                })
            {
                menu = menu.entry(MenuItem::new("Disconnect remote").action(
                    move || {
                        workbench_command
                            .send(LapceWorkbenchCommand::DisconnectRemote);
                    },
                ));
            }
            #[cfg(windows)]
            {
                menu = menu.entry(MenuItem::new("Connect to WSL Host").action(
                    move || {
                        workbench_command
                            .send(LapceWorkbenchCommand::ConnectWslHost);
                    },
                ));
            }
            menu
        })
        .style(move |s| {
            let config = config.get();
            let color = if is_local {
                Color::TRANSPARENT
            } else {
                match proxy_status.get() {
                    Some(ProxyStatus::Connected) => {
                        config.color(LapceColor::LAPCE_REMOTE_CONNECTED)
                    }
                    Some(ProxyStatus::Connecting) => {
                        config.color(LapceColor::LAPCE_REMOTE_CONNECTING)
                    }
                    Some(ProxyStatus::Disconnected) => {
                        config.color(LapceColor::LAPCE_REMOTE_DISCONNECTED)
                    }
                    None => Color::TRANSPARENT,
                }
            };
            s.height_pct(100.0)
                .padding_horiz(10.0)
                .items_center()
                .background(color)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        }),
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
    ))
    .style(move |s| {
        s.height_pct(100.0)
            .flex_basis(0.0)
            .flex_grow(1.0)
            .items_center()
    })
    .debug_name("Left Side of Top Bar")
}

fn middle(
    workspace: Arc<LapceWorkspace>,
    main_split: MainSplitData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let local_workspace = workspace.clone();
    let can_jump_backward = {
        let main_split = main_split.clone();
        create_memo(move |_| main_split.can_jump_location_backward(true))
    };
    let can_jump_forward =
        create_memo(move |_| main_split.can_jump_location_forward(true));

    let jump_backward = move || {
        clickable_icon(
            || LapceIcons::LOCATION_BACKWARD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::JumpLocationBackward);
            },
            || false,
            move || !can_jump_backward.get(),
            || "Jump Backward",
            config,
        )
        .style(move |s| s.margin_horiz(6.0))
    };
    let jump_forward = move || {
        clickable_icon(
            || LapceIcons::LOCATION_FORWARD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::JumpLocationForward);
            },
            || false,
            move || !can_jump_forward.get(),
            || "Jump Forward",
            config,
        )
        .style(move |s| s.margin_right(6.0))
    };

    let open_folder = move || {
        not_clickable_icon(
            || LapceIcons::PALETTE_MENU,
            || false,
            || false,
            || "Open Folder / Recent Workspace",
            config,
        )
        .popout_menu(move || {
            Menu::new("")
                .entry(MenuItem::new("Open Folder").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::OpenFolder);
                }))
                .entry(MenuItem::new("Open Recent Workspace").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::PaletteWorkspace);
                }))
        })
    };

    stack((
        stack((
            drag_window_area(empty())
                .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
            jump_backward(),
            jump_forward(),
        ))
        .style(|s| {
            s.flex_basis(0)
                .flex_grow(1.0)
                .justify_content(Some(JustifyContent::FlexEnd))
        }),
        container(
            stack((
                svg(move || config.get().ui_svg(LapceIcons::SEARCH)).style(
                    move |s| {
                        let config = config.get();
                        let icon_size = config.ui.icon_size() as f32;
                        s.size(icon_size, icon_size)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    },
                ),
                label(move || {
                    if let Some(s) = local_workspace.display() {
                        s
                    } else {
                        "Open Folder".to_string()
                    }
                })
                .style(|s| s.padding_left(10).padding_right(5).selectable(false)),
                open_folder(),
            ))
            .style(|s| s.align_items(Some(AlignItems::Center))),
        )
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_click_stop(move |_| {
            if workspace.clone().path.is_some() {
                workbench_command.send(LapceWorkbenchCommand::PaletteHelpAndFile);
            } else {
                workbench_command.send(LapceWorkbenchCommand::PaletteWorkspace);
            }
        })
        .style(move |s| {
            let config = config.get();
            s.flex_basis(0)
                .flex_grow(10.0)
                .min_width(200.0)
                .max_width(500.0)
                .min_height(26.0)
                .justify_content(Some(JustifyContent::Center))
                .align_items(Some(AlignItems::Center))
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .border_radius(6.0)
                .background(config.color(LapceColor::EDITOR_BACKGROUND))
                .cursor(CursorStyle::Pointer)
        }),
        stack((
            clickable_icon(
                || LapceIcons::START,
                move || {
                    workbench_command.send(LapceWorkbenchCommand::PaletteRunAndDebug)
                },
                || false,
                || false,
                || "Run and Debug",
                config,
            )
            .style(move |s| s.margin_horiz(6.0)),
            drag_window_area(empty())
                .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
        ))
        .style(move |s| {
            s.flex_basis(0)
                .flex_grow(1.0)
                .justify_content(Some(JustifyContent::FlexStart))
        }),
    ))
    .style(|s| {
        s.flex_basis(0)
            .flex_grow(2.0)
            .align_items(Some(AlignItems::Center))
            .justify_content(Some(JustifyContent::Center))
    })
    .debug_name("Middle of Top Bar")
}

fn right(
    window_command: Listener<WindowCommand>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    update_in_progress: RwSignal<bool>,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let latest_version = create_memo(move |_| {
        let latest_release = latest_release.get();
        let latest_version =
            latest_release.as_ref().as_ref().map(|r| r.version.clone());
        if latest_version.is_some()
            && latest_version.as_deref() != Some(meta::VERSION)
        {
            latest_version
        } else {
            None
        }
    });

    stack((
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
        container(stack((
            title_button(
                lapce_icon(|| LapceIcons::SETTINGS, config),
                || "Settings",
                config,
            )
            .popout_menu(move || {
                Menu::new("")
                    .entry(MenuItem::new("Command Palette").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::PaletteCommand)
                    }))
                    .separator()
                    .entry(MenuItem::new("Open Settings").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::OpenSettings)
                    }))
                    .entry(MenuItem::new("Open Keyboard Shortcuts").action(
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenKeyboardShortcuts)
                        },
                    ))
                    .entry(MenuItem::new("Open Theme Color Settings").action(
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenThemeColorSettings)
                        },
                    ))
                    .separator()
                    .entry(if let Some(v) = latest_version.get_untracked() {
                        if update_in_progress.get_untracked() {
                            MenuItem::new(format!("Update in progress ({v})"))
                                .enabled(false)
                        } else {
                            MenuItem::new(format!("Restart to update ({v})")).action(
                                move || {
                                    workbench_command
                                        .send(LapceWorkbenchCommand::RestartToUpdate)
                                },
                            )
                        }
                    } else {
                        MenuItem::new("No update available").enabled(false)
                    })
                    .separator()
                    .entry(MenuItem::new("About Lapce").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::ShowAbout)
                    }))
            }),
            update_indicator(config, latest_release, false),
        ))),
        window_controls_view(
            window_command,
            true,
            num_window_tabs,
            window_maximized,
            config,
        ),
    ))
    .style(|s| {
        s.height_full()
            .flex_basis(0)
            .flex_grow(1.0)
            .justify_content(Some(JustifyContent::FlexEnd))
    })
    .debug_name("Right of top bar")
}

fn update_indicator(
    config: ReadSignal<Arc<LapceConfig>>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    offset_indicator: bool,
) -> impl View {
    let has_update = move || {
        latest_release.with(|r| {
            r.as_ref()
                .as_ref()
                .is_some_and(|r| r.version != meta::VERSION)
        })
    };

    container(empty().style(move |mut s| {
        let config = config.get();
        let base_icon_size = config.ui.icon_size() as f32;
        let width = base_icon_size * 0.5;

        if offset_indicator {
            s = s.translate_x(width * 0.5).translate_y(base_icon_size * 0.2)
        } else {
            s = s.margin(base_icon_size * 0.15)
        }

        s.size(width, width)
            .border_radius(100.0)
            .background(config.color(LapceColor::EDITOR_CARET))
    }))
    .style(move |s| {
        let has_update = has_update();
        s.z_index(1)
            .absolute()
            .size_full()
            .justify_end()
            .items_end()
            .pointer_events_none()
            .apply_if(!has_update, |s| s.hide())
    })
}

fn lapce_icon(
    icon: impl Fn() -> &'static str + 'static,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    svg(move || config.get().ui_svg(icon())).style(move |s| {
        let config = config.get();
        let size = config.ui.icon_size() as f32;

        s.size(size, size)
            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
    })
}

fn title_button<S: std::fmt::Display + 'static>(
    child: impl View + 'static,
    label_text: impl Fn() -> S + 'static + Clone,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    tooltip_label(config, child, label_text).style(move |s| {
        let config = config.get();

        s.height_pct(100.0)
            .padding_horiz(10.0)
            .items_center()
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
            })
            .active(|s| {
                s.cursor(CursorStyle::Pointer).background(
                    config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                )
            })
    })
}

pub fn title(window_tab_data: Rc<WindowTabData>) -> impl View {
    let workspace = window_tab_data.workspace.clone();
    let lapce_command = window_tab_data.common.lapce_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_command = window_tab_data.common.window_common.window_command;
    let latest_release = window_tab_data.common.window_common.latest_release;
    let proxy_status = window_tab_data.common.proxy_status;
    let num_window_tabs = window_tab_data.common.window_common.num_window_tabs;
    let window_maximized = window_tab_data.common.window_common.window_maximized;
    let title_height = window_tab_data.title_height;
    let update_in_progress = window_tab_data.update_in_progress;
    let config = window_tab_data.common.config;
    stack((
        left(
            workspace.clone(),
            lapce_command,
            workbench_command,
            config,
            proxy_status,
            num_window_tabs,
        ),
        middle(
            workspace,
            window_tab_data.main_split.clone(),
            workbench_command,
            config,
        ),
        right(
            window_command,
            workbench_command,
            latest_release,
            update_in_progress,
            num_window_tabs,
            window_maximized,
            config,
        ),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != title_height.get_untracked() {
            title_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .min_height(37.0)
            .items_center()
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    })
    .debug_name("Title / Top Bar")
}

pub fn window_controls_view(
    window_command: Listener<WindowCommand>,
    is_title: bool,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack((
        title_button(
            lapce_icon(|| LapceIcons::WINDOW_MINIMIZE, config),
            || "Minimize",
            config,
        )
        .on_click_stop(|_| {
            floem::action::minimize_window();
        }),
        title_button(
            lapce_icon(
                move || {
                    if window_maximized.get() {
                        LapceIcons::WINDOW_RESTORE
                    } else {
                        LapceIcons::WINDOW_MAXIMIZE
                    }
                },
                config,
            ),
            || "Maximize",
            config,
        )
        .on_click_stop(move |_| {
            floem::action::set_window_maximized(!window_maximized.get_untracked());
        }),
        title_button(
            lapce_icon(|| LapceIcons::WINDOW_CLOSE, config),
            || "Close Window",
            config,
        )
        .on_click_stop(move |_| {
            window_command.send(WindowCommand::CloseWindow);
        }),
    ))
    .style(move |s| {
        s.height_full().apply_if(
            cfg!(target_os = "macos")
                || !config.get_untracked().core.custom_titlebar
                || (is_title && num_window_tabs.get() > 1),
            |s| s.hide(),
        )
    })
}
