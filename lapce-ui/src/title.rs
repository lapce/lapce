use std::sync::Arc;

use crate::svg::get_svg;
use druid::{
    kurbo::Line,
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use druid::{WindowConfig, WindowState};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{LapceTabData, LapceWorkspaceType},
    menu::{MenuItem, MenuKind},
    proxy::ProxyStatus,
};
use serde_json::json;

pub struct Title {
    mouse_pos: Point,
    commands: Vec<(Rect, Command)>,
}

impl Title {
    pub fn new() -> Self {
        Self {
            mouse_pos: Point::ZERO,
            commands: Vec::new(),
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for (rect, _) in self.commands.iter() {
            if rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for (rect, command) in self.commands.iter() {
            if rect.contains(mouse_event.pos) {
                ctx.submit_command(command.clone());
            }
        }
    }
}

impl Default for Title {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for Title {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        data: &mut LapceTabData,
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();

                    #[cfg(target_os = "windows")]
                    // ! Currently implemented on Windows only
                    ctx.window().handle_titlebar(true);
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Event::MouseUp(mouse_event) => {
                if (cfg!(target_os = "macos") || data.config.ui.custom_titlebar())
                    && mouse_event.count >= 2
                {
                    let state = match ctx.window().get_window_state() {
                        WindowState::Maximized => WindowState::Restored,
                        WindowState::Restored => WindowState::Maximized,
                        WindowState::Minimized => WindowState::Maximized,
                    };
                    ctx.submit_command(
                        druid::commands::CONFIGURE_WINDOW
                            .with(WindowConfig::default().set_window_state(state))
                            .to(Target::Window(data.window_id)),
                    )
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        #[cfg(target_os = "windows")] ctx: &mut UpdateCtx,
        #[cfg(not(target_os = "windows"))] _ctx: &mut UpdateCtx,
        #[cfg(target_os = "windows")] old_data: &LapceTabData,
        #[cfg(not(target_os = "windows"))] _old_data: &LapceTabData,
        #[cfg(target_os = "windows")] data: &LapceTabData,
        #[cfg(not(target_os = "windows"))] _data: &LapceTabData,
        _env: &Env,
    ) {
        #[cfg(target_os = "windows")]
        if old_data.config.ui.custom_titlebar() != data.config.ui.custom_titlebar() {
            ctx.submit_command(
                druid::commands::CONFIGURE_WINDOW
                    .with(
                        WindowConfig::default()
                            .show_titlebar(!data.config.ui.custom_titlebar()),
                    )
                    .to(Target::Window(data.window_id)),
            )
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        #[cfg(not(target_os = "windows"))]
        {
            Size::new(bc.max().width, 36.0)
        }

        #[cfg(target_os = "windows")]
        {
            Size::new(bc.max().width, 32.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );

        self.commands.clear();

        #[cfg(not(target_os = "macos"))]
        let mut x = 0.0;
        #[cfg(target_os = "macos")]
        let mut x = 78.0;

        let padding = 15.0;

        #[cfg(target_os = "windows")]
        {
            let logo_rect = Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x, 0.0));
            let logo_svg = crate::svg::logo_svg();
            ctx.draw_svg(
                &logo_svg,
                logo_rect.inflate(-5.0, -5.0),
                Some(
                    &data
                        .config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone()
                        .with_alpha(0.5),
                ),
            );

            x += size.height;
        }

        let command_rect = Size::ZERO.to_rect().with_origin(Point::new(x, 0.0));
        let remote_text = match &data.workspace.kind {
            LapceWorkspaceType::Local => None,
            LapceWorkspaceType::RemoteSSH(_, host) => {
                let text = match *data.proxy_status {
                    ProxyStatus::Connecting => {
                        format!("Connecting to SSH: {host} ...")
                    }
                    ProxyStatus::Connected => format!("SSH: {host}"),
                    ProxyStatus::Disconnected => {
                        format!("Disconnected SSH: {host}")
                    }
                };
                let text_layout = ctx
                    .text()
                    .new_text_layout(text)
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                Some(text_layout)
            }
            LapceWorkspaceType::RemoteWSL => {
                let text = match *data.proxy_status {
                    ProxyStatus::Connecting => "Connecting to WSL ...".to_string(),
                    ProxyStatus::Connected => "WSL".to_string(),
                    ProxyStatus::Disconnected => "Disconnected WSL".to_string(),
                };
                let text_layout = ctx
                    .text()
                    .new_text_layout(text)
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                Some(text_layout)
            }
        };

        let remote_rect = Size::new(
            size.height
                + 10.0
                + remote_text
                    .as_ref()
                    .map(|t| t.size().width.round() + padding - 5.0)
                    .unwrap_or(0.0),
            size.height,
        )
        .to_rect()
        .with_origin(Point::new(x, 0.0));
        let color = match &data.workspace.kind {
            LapceWorkspaceType::Local => Color::rgb8(64, 120, 242),
            LapceWorkspaceType::RemoteSSH(_, _) | LapceWorkspaceType::RemoteWSL => {
                match *data.proxy_status {
                    ProxyStatus::Connecting => Color::rgb8(193, 132, 1),
                    ProxyStatus::Connected => Color::rgb8(80, 161, 79),
                    ProxyStatus::Disconnected => Color::rgb8(228, 86, 73),
                }
            }
        };
        ctx.fill(remote_rect, &color);
        let remote_svg = get_svg("remote.svg").unwrap();
        ctx.draw_svg(
            &remote_svg,
            Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x + 5.0, 0.0))
                .inflate(-8.0, -8.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            ),
        );
        if let Some(text_layout) = remote_text.as_ref() {
            ctx.draw_text(
                text_layout,
                Point::new(
                    x + size.height + 5.0,
                    (size.height - text_layout.size().height) / 2.0,
                ),
            );
        }
        x += remote_rect.width();
        let command_rect =
            command_rect.with_size(Size::new(x - command_rect.x0, size.height));

        let mut menu_items = vec![MenuKind::Item(MenuItem {
            desc: None,
            command: LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::ConnectSshHost),
                data: None,
            },
        })];

        if cfg!(target_os = "windows") {
            menu_items.push(MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::ConnectWsl),
                    data: None,
                },
            }));
        }

        if data.workspace.kind.is_remote() {
            menu_items.push(MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::DisconnectRemote,
                    ),
                    data: None,
                },
            }));
        }

        self.commands.push((
            command_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    ctx.to_window(Point::new(command_rect.x0, command_rect.y1)),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));

        let command_rect = Size::ZERO.to_rect().with_origin(Point::new(x, 0.0));

        x += 5.0;
        let folder_svg = get_svg("default_folder.svg").unwrap();
        let folder_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x, 0.0));
        ctx.draw_svg(
            &folder_svg,
            folder_rect.inflate(-9.0, -9.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );
        x += size.height;
        let text = if let Some(workspace_path) = tab.workspace.path.as_ref() {
            workspace_path
                .file_name()
                .unwrap_or(workspace_path.as_os_str())
                .to_string_lossy()
                .to_string()
        } else {
            "Open Folder".to_string()
        };
        let text_layout = ctx
            .text()
            .new_text_layout(text)
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(
            &text_layout,
            Point::new(x, (size.height - text_layout.size().height) / 2.0),
        );
        x += text_layout.size().width.round() + padding;
        let menu_items = vec![
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::OpenFolder),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::PaletteWorkspace,
                    ),
                    data: None,
                },
            }),
        ];
        let command_rect =
            command_rect.with_size(Size::new(x - command_rect.x0, size.height));
        self.commands.push((
            command_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    ctx.to_window(Point::new(command_rect.x0, command_rect.y1)),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));

        let line_color = data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER);
        let line = Line::new(Point::new(x, 0.0), Point::new(x, size.height));
        ctx.stroke(line, line_color, 1.0);

        if !tab.source_control.branch.is_empty() {
            let command_rect = Size::ZERO.to_rect().with_origin(Point::new(x, 0.0));

            x += 5.0;
            let folder_svg = get_svg("git-icon.svg").unwrap();
            let folder_rect = Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x, 0.0));
            ctx.draw_svg(
                &folder_svg,
                folder_rect.inflate(-10.5, -10.5),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
            x += size.height;

            let mut branch = tab.source_control.branch.clone();
            if !tab.source_control.file_diffs.is_empty() {
                branch += "*";
            }
            let text_layout = ctx
                .text()
                .new_text_layout(branch)
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(x, (size.height - text_layout.size().height) / 2.0),
            );
            x += text_layout.size().width.round() + padding;

            let command_rect =
                command_rect.with_size(Size::new(x - command_rect.x0, size.height));
            let menu_items = tab
                .source_control
                .branches
                .iter()
                .map(|b| {
                    MenuKind::Item(MenuItem {
                        desc: Some(b.to_string()),
                        command: LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::CheckoutBranch,
                            ),
                            data: Some(json!(b.to_string())),
                        },
                    })
                })
                .collect();
            self.commands.push((
                command_rect,
                Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowMenu(
                        ctx.to_window(Point::new(command_rect.x0, command_rect.y1)),
                        Arc::new(menu_items),
                    ),
                    Target::Auto,
                ),
            ));

            let line_color =
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER);
            let line = Line::new(Point::new(x, 0.0), Point::new(x, size.height));
            ctx.stroke(line, line_color, 1.0);
        }

        #[cfg(target_os = "windows")]
        {
            let title_layout = ctx
                .text()
                .new_text_layout(String::from("Lapce"))
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &title_layout,
                Point::new(
                    (size.width - title_layout.size().width) / 2.0,
                    (size.height - title_layout.size().height) / 2.0,
                ),
            );

            if data.config.ui.custom_titlebar() {
                x = size.width - (size.height * 4.0);
            }
        }

        if cfg!(not(target_os = "windows")) || !data.config.ui.custom_titlebar() {
            x = size.width - size.height;
        }

        let settings_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x, 0.0));
        let settings_svg = get_svg("settings.svg").unwrap();
        ctx.draw_svg(
            &settings_svg,
            settings_rect.inflate(-10.5, -10.5),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );
        let menu_items = vec![
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::PaletteCommand,
                    ),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::OpenSettings,
                    ),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::OpenKeyboardShortcuts,
                    ),
                    data: None,
                },
            }),
        ];
        self.commands.push((
            settings_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    ctx.to_window(Point::new(
                        size.width - size.height,
                        settings_rect.y1,
                    )),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));

        #[cfg(target_os = "windows")]
        if data.config.ui.custom_titlebar() {
            let font_size = 10.0;
            let font_family = "Segoe MDL2 Assets";

            #[derive(strum_macros::Display)]
            enum WindowControls {
                Minimise,
                Maximise,
                Restore,
                Close,
            }

            impl WindowControls {
                fn as_str(&self) -> &'static str {
                    match self {
                        WindowControls::Minimise => "\u{E949}",
                        WindowControls::Maximise => "\u{E739}",
                        WindowControls::Restore => "\u{E923}",
                        WindowControls::Close => "\u{E106}",
                    }
                }
            }

            x += size.height;
            let minimise_text = ctx
                .text()
                .new_text_layout(WindowControls::Minimise.as_str())
                .font(ctx.text().font_family(font_family).unwrap(), font_size)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &minimise_text,
                Point::new(
                    x + ((minimise_text.size().width + 5.0) / 2.0),
                    (size.height - minimise_text.size().height) / 2.0,
                ),
            );
            let minimise_rect = Size::new(
                size.height
                    + Some(minimise_text)
                        .as_ref()
                        .map(|t| t.size().width.round() + padding - 5.0)
                        .unwrap_or(0.0),
                size.height,
            )
            .to_rect()
            .with_origin(Point::new(x, 0.0));

            self.commands.push((
                minimise_rect,
                Command::new(
                    druid::commands::CONFIGURE_WINDOW,
                    WindowConfig::default().set_window_state(WindowState::Minimized),
                    Target::Window(data.window_id),
                ),
            ));

            x += size.height;

            let max_res_icon;
            let max_res_state;

            if ctx.window().get_window_state() == WindowState::Restored {
                max_res_icon = WindowControls::Maximise;
                max_res_state = WindowState::Maximized;
            } else {
                max_res_icon = WindowControls::Restore;
                max_res_state = WindowState::Restored;
            };

            let max_res_text = ctx
                .text()
                .new_text_layout(max_res_icon.as_str())
                .font(ctx.text().font_family(font_family).unwrap(), font_size)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &max_res_text,
                Point::new(
                    x + ((max_res_text.size().width + 5.0) / 2.0),
                    (size.height - max_res_text.size().height) / 2.0,
                ),
            );

            let max_res_rect = Size::new(
                size.height
                    + Some(max_res_text)
                        .as_ref()
                        .map(|t| t.size().width.round() + padding - 5.0)
                        .unwrap_or(0.0),
                size.height,
            )
            .to_rect()
            .with_origin(Point::new(x, 0.0));
            self.commands.push((
                max_res_rect,
                Command::new(
                    druid::commands::CONFIGURE_WINDOW,
                    WindowConfig::default().set_window_state(max_res_state),
                    Target::Window(data.window_id),
                ),
            ));

            x += size.height;
            let close_text = ctx
                .text()
                .new_text_layout(WindowControls::Close.as_str())
                .font(ctx.text().font_family(font_family).unwrap(), font_size)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &close_text,
                Point::new(
                    x + ((close_text.size().width + 5.0) / 2.0),
                    (size.height - close_text.size().height) / 2.0,
                ),
            );
            let close_rect = Size::new(
                size.height
                    + Some(close_text)
                        .as_ref()
                        .map(|t| t.size().width.round() + padding + 5.0)
                        .unwrap_or(0.0),
                size.height,
            )
            .to_rect()
            .with_origin(Point::new(x, 0.0));

            self.commands.push((
                close_rect,
                Command::new(druid::commands::QUIT_APP, (), Target::Global),
            ));
        }
    }
}
