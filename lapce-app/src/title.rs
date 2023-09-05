use std::sync::Arc;

use floem::{
    event::EventListener,
    menu::{Menu, MenuItem},
    peniko::Color,
    reactive::{create_memo, Memo, ReadSignal, RwSignal},
    style::{AlignItems, CursorStyle, Dimension, JustifyContent},
    view::View,
    views::{container, empty, handle_titlebar_area, label, stack, svg, Decorators},
};
use lapce_core::meta;
use lapce_rpc::proxy::ProxyStatus;

use crate::{
    app::clickable_icon,
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    listener::Listener,
    main_split::MainSplitData,
    update::ReleaseInfo,
    workspace::LapceWorkspace,
};

fn left(
    workspace: Arc<LapceWorkspace>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
    proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
) -> impl View {
    let is_local = workspace.kind.is_local();
    stack(move || {
        (
            empty().style(move |s| {
                let is_macos = cfg!(target_os = "macos");
                let should_hide = if is_macos {
                    num_window_tabs.get() > 1
                } else {
                    true
                };
                s.width_px(75.0).apply_if(should_hide, |s| s.hide())
            }),
            container(move || {
                svg(move || config.get().ui_svg(LapceIcons::REMOTE)).style(
                    move |s| {
                        s.size_px(26.0, 26.0).color(if is_local {
                            *config.get().get_color(LapceColor::LAPCE_REMOTE_LOCAL)
                        } else {
                            match proxy_status.get() {
                                Some(_) => Color::WHITE,
                                None => *config
                                    .get()
                                    .get_color(LapceColor::LAPCE_REMOTE_LOCAL),
                            }
                        })
                    },
                )
            })
            .popout_menu(move || {
                #[allow(unused_mut)]
                let mut menu = Menu::new("").entry(
                    MenuItem::new("Connect to SSH Host").action(move || {
                        workbench_command
                            .send(LapceWorkbenchCommand::ConnectSshHost);
                    }),
                );
                #[cfg(windows)]
                {
                    menu = menu.entry(MenuItem::new("Connect to WSL").action(
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::ConnectWsl);
                        },
                    ));
                }
                menu
            })
            .hover_style(move |s| {
                s.cursor(CursorStyle::Pointer).background(
                    *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                )
            })
            .active_style(move |s| {
                s.cursor(CursorStyle::Pointer).background(
                    *config
                        .get()
                        .get_color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                )
            })
            .style(move |s| {
                let config = config.get();
                let color =
                    if is_local {
                        Color::TRANSPARENT
                    } else {
                        match proxy_status.get() {
                            Some(ProxyStatus::Connected) => {
                                *config.get_color(LapceColor::LAPCE_REMOTE_CONNECTED)
                            }
                            Some(ProxyStatus::Connecting) => *config
                                .get_color(LapceColor::LAPCE_REMOTE_CONNECTING),
                            Some(ProxyStatus::Disconnected) => *config
                                .get_color(LapceColor::LAPCE_REMOTE_DISCONNECTED),
                            None => Color::TRANSPARENT,
                        }
                    };
                s.height_pct(100.0)
                    .padding_horiz_px(10.0)
                    .items_center()
                    .background(color)
            }),
        )
    })
    .style(move |s| {
        s.height_pct(100.0)
            .flex_basis(Dimension::Points(0.0))
            .flex_grow(1.0)
            .items_center()
    })
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
            config,
        )
        .style(move |s| s.margin_horiz_px(6.0))
    };
    let jump_forward = move || {
        clickable_icon(
            || LapceIcons::LOCATION_FORWARD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::JumpLocationForward);
            },
            || false,
            move || !can_jump_forward.get(),
            config,
        )
        .style(move |s| s.margin_right_px(6.0))
    };

    let open_folder = move || {
        clickable_icon(
            || LapceIcons::PALETTE_MENU,
            move || {},
            || false,
            || false,
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

    stack(move || {
        (
            stack(move || (jump_backward(), jump_forward())).style(|s| {
                s.flex_basis(Dimension::Points(0.0))
                    .flex_grow(1.0)
                    .justify_content(Some(JustifyContent::FlexEnd))
            }),
            container(|| {
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::SEARCH)).style(
                            move |s| {
                                let config = config.get();
                                let icon_size = config.ui.icon_size() as f32;
                                s.size_px(icon_size, icon_size).color(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                )
                            },
                        ),
                        label(move || {
                            if let Some(s) = local_workspace.display() {
                                s
                            } else {
                                "Open Folder".to_string()
                            }
                        })
                        .style(|s| s.padding_left_px(10.0).padding_right_px(5.0)),
                        open_folder(),
                    )
                })
                .style(|s| s.align_items(Some(AlignItems::Center)))
            })
            .on_event(EventListener::PointerDown, |_| true)
            .on_click(move |_| {
                if workspace.clone().path.is_some() {
                    workbench_command.send(LapceWorkbenchCommand::Palette);
                } else {
                    workbench_command.send(LapceWorkbenchCommand::PaletteWorkspace);
                }
                true
            })
            .style(move |s| {
                let config = config.get();
                s.flex_basis(Dimension::Points(0.0))
                    .flex_grow(10.0)
                    .min_width_px(200.0)
                    .max_width_px(500.0)
                    .height_px(26.0)
                    .justify_content(Some(JustifyContent::Center))
                    .align_items(Some(AlignItems::Center))
                    .border(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .border_radius(6.0)
                    .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            }),
            container(move || {
                clickable_icon(
                    || LapceIcons::START,
                    move || {
                        workbench_command
                            .send(LapceWorkbenchCommand::PaletteRunAndDebug)
                    },
                    || false,
                    || false,
                    config,
                )
                .style(move |s| s.margin_horiz_px(6.0))
            })
            .style(move |s| {
                s.flex_basis(Dimension::Points(0.0))
                    .flex_grow(1.0)
                    .justify_content(Some(JustifyContent::FlexStart))
            }),
        )
    })
    .style(|s| {
        s.flex_basis(Dimension::Points(0.0))
            .flex_grow(2.0)
            .align_items(Some(AlignItems::Center))
            .justify_content(Some(JustifyContent::Center))
    })
}

fn right(
    workbench_command: Listener<LapceWorkbenchCommand>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    update_in_progress: RwSignal<bool>,
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

    let has_update = move || latest_version.with(|v| v.is_some());

    container(move || {
        stack(|| {
            (
                clickable_icon(
                    || LapceIcons::SETTINGS,
                    || {},
                    || false,
                    || false,
                    config,
                )
                .popout_menu(move || {
                    Menu::new("")
                        .entry(MenuItem::new("Command Palette").action(move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::PaletteCommand)
                        }))
                        .separator()
                        .entry(MenuItem::new("Open Settings").action(move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenSettings)
                        }))
                        .entry(MenuItem::new("Open Keyboard Shortcuts").action(
                            move || {
                                workbench_command.send(
                                    LapceWorkbenchCommand::OpenKeyboardShortcuts,
                                )
                            },
                        ))
                        .separator()
                        .entry(if let Some(v) = latest_version.get_untracked() {
                            if update_in_progress.get_untracked() {
                                MenuItem::new(format!("Update in progress ({v})"))
                                    .enabled(false)
                            } else {
                                MenuItem::new(format!("Restart to update ({v})"))
                                    .action(move || {
                                        workbench_command.send(
                                            LapceWorkbenchCommand::RestartToUpdate,
                                        )
                                    })
                            }
                        } else {
                            MenuItem::new("No update available").enabled(false)
                        })
                        .separator()
                        .entry(MenuItem::new("About Lapce").action(move || {
                            workbench_command.send(LapceWorkbenchCommand::ShowAbout)
                        }))
                }),
                container(|| {
                    label(|| "1".to_string()).style(move |s| {
                        let config = config.get();
                        s.font_size(10.0)
                            .color(*config.get_color(LapceColor::EDITOR_BACKGROUND))
                            .border_radius(100.0)
                            .margin_left_px(5.0)
                            .margin_top_px(10.0)
                            .background(*config.get_color(LapceColor::EDITOR_CARET))
                    })
                })
                .style(move |s| {
                    let has_update = has_update();
                    s.absolute()
                        .size_pct(100.0, 100.0)
                        .justify_end()
                        .items_end()
                        .apply_if(!has_update, |s| s.hide())
                }),
            )
        })
        .style(move |s| s.margin_horiz_px(6.0))
    })
    .style(|s| {
        s.flex_basis(Dimension::Points(0.0))
            .flex_grow(1.0)
            .justify_content(Some(JustifyContent::FlexEnd))
    })
}

#[allow(clippy::too_many_arguments)]
pub fn title(
    workspace: Arc<LapceWorkspace>,
    main_split: MainSplitData,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    update_in_progress: RwSignal<bool>,
    proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
    title_height: RwSignal<f64>,
) -> impl View {
    let workbench_command = main_split.common.workbench_command;
    let config = main_split.common.config;
    handle_titlebar_area(|| {
        stack(move || {
            (
                left(
                    workspace.clone(),
                    workbench_command,
                    config,
                    proxy_status,
                    num_window_tabs,
                ),
                middle(workspace, main_split, workbench_command, config),
                right(
                    workbench_command,
                    latest_release,
                    update_in_progress,
                    config,
                ),
            )
        })
        .on_resize(move |rect| {
            let height = rect.height();
            if height != title_height.get_untracked() {
                title_height.set(height);
            }
        })
        .style(move |s| {
            let config = config.get();
            s.width_pct(100.0)
                .height_px(37.0)
                .items_center()
                .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
                .border_bottom(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
        })
    })
    .style(|s| s.width_pct(100.0))
}
