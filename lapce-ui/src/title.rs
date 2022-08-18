use std::sync::Arc;

#[cfg(not(target_os = "macos"))]
use crate::window::window_controls;
use crate::{palette::Palette, svg::get_svg};
use druid::WindowConfig;
use druid::{
    kurbo::Line,
    piet::{PietText, PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, Region, RenderContext, Size,
    Target, Widget, WidgetExt, WidgetPod, WindowState,
};
use lapce_data::proxy::VERSION;
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{LapceTabData, LapceWorkspaceType},
    menu::{MenuItem, MenuKind},
    palette::PaletteStatus,
    proxy::ProxyStatus,
};
use serde_json::json;

pub struct Title {
    mouse_pos: Point,
    commands: Vec<(Rect, Command)>,
    svgs: Vec<(Svg, Rect, Option<Color>)>,
    text_layouts: Vec<(PietTextLayout, Point)>,
    borders: Vec<Line>,
    rects: Vec<(Rect, Color)>,
    palette: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    dragable_area: Region,
}

impl Title {
    pub fn new(data: &LapceTabData) -> Self {
        let palette = Palette::new(data);
        Self {
            mouse_pos: Point::ZERO,
            commands: Vec::new(),
            svgs: Vec::new(),
            text_layouts: Vec::new(),
            borders: Vec::new(),
            rects: Vec::new(),
            palette: WidgetPod::new(palette.boxed()),
            dragable_area: Region::EMPTY,
        }
    }

    fn update_content(
        &mut self,
        data: &LapceTabData,
        window_state: &WindowState,
        #[cfg(not(target_os = "macos"))] _is_fullscreen: bool,
        #[cfg(target_os = "macos")] is_fullscreen: bool,
        piet_text: &mut PietText,
        size: Size,
    ) -> Rect {
        self.commands.clear();
        self.svgs.clear();
        self.text_layouts.clear();
        self.borders.clear();
        self.rects.clear();

        #[cfg(not(target_os = "macos"))]
        let mut x = 0.0;
        #[cfg(target_os = "macos")]
        let mut x = if data.multiple_tab || is_fullscreen {
            0.0
        } else {
            78.0
        };

        #[cfg(not(target_os = "macos"))]
        {
            let logo_rect = Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x + 2.0, 0.0))
                .inflate(-9.0, -9.0);
            let logo_svg = crate::svg::logo_svg();
            self.svgs.push((
                logo_svg,
                logo_rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone()
                        .with_alpha(0.5),
                ),
            ));
            x += size.height;
        }

        let padding = 15.0;
        x = self.update_remote(data, piet_text, size, padding, x);
        x = self.update_source_control(data, piet_text, size, padding, x);

        let mut region = Region::EMPTY;

        if data.palette.status == PaletteStatus::Inactive {
            self.update_folder(data, piet_text, size);
        }

        let right_x = size.width;
        let right_x = self.update_settings(
            data,
            window_state,
            piet_text,
            size,
            padding,
            right_x,
        );

        if !data.multiple_tab {
            region.add_rect(
                Size::new(right_x - x, size.height)
                    .to_rect()
                    .with_origin(Point::new(x, 0.0)),
            );
        }

        self.dragable_area = region;

        Size::new(right_x - x, size.height)
            .to_rect()
            .with_origin(Point::new(x, 0.0))
    }

    fn update_remote(
        &mut self,
        data: &LapceTabData,
        _piet_text: &mut PietText,
        size: Size,
        _padding: f64,
        x: f64,
    ) -> f64 {
        let command_rect = Size::ZERO.to_rect().with_origin(Point::new(x, 0.0));

        let remote_rect = Size::new(size.height + 10.0, size.height)
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
        self.rects.push((remote_rect, color));
        let remote_svg = get_svg("remote.svg").unwrap();
        self.svgs.push((
            remote_svg,
            Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x + 5.0, 0.0))
                .inflate(-8.0, -8.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    .clone(),
            ),
        ));
        let x = x + remote_rect.width();
        let command_rect =
            command_rect.with_size(Size::new(x - command_rect.x0, size.height));

        let mut menu_items = vec![MenuKind::Item(MenuItem {
            desc: None,
            command: LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::ConnectSshHost),
                data: None,
            },
            enabled: true,
        })];

        if cfg!(target_os = "windows") {
            menu_items.push(MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::ConnectWsl),
                    data: None,
                },
                enabled: true,
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
                enabled: true,
            }));
        }

        self.commands.push((
            command_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    Point::new(
                        command_rect.x0,
                        command_rect.y1 + if data.multiple_tab { 36.0 } else { 0.0 },
                    ),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));
        x
    }

    fn update_source_control(
        &mut self,
        data: &LapceTabData,
        piet_text: &mut PietText,
        size: Size,
        padding: f64,
        x: f64,
    ) -> f64 {
        let mut x = x;
        if !data.source_control.branch.is_empty() {
            let command_rect = Size::ZERO.to_rect().with_origin(Point::new(x, 0.0));

            x += 5.0;
            let folder_svg = get_svg("git-icon.svg").unwrap();
            let folder_rect = Size::new(size.height, size.height)
                .to_rect()
                .with_origin(Point::new(x, 0.0));
            self.svgs.push((
                folder_svg,
                folder_rect.inflate(-10.5, -10.5),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                ),
            ));
            x += size.height;

            let mut branch = data.source_control.branch.clone();
            if !data.source_control.file_diffs.is_empty() {
                branch += "*";
            }
            let text_layout = piet_text
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
            let point =
                Point::new(x, (size.height - text_layout.size().height) / 2.0);
            x += text_layout.size().width.round() + padding;
            self.text_layouts.push((text_layout, point));

            let command_rect =
                command_rect.with_size(Size::new(x - command_rect.x0, size.height));
            let menu_items = data
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
                        enabled: true,
                    })
                })
                .collect();
            self.commands.push((
                command_rect,
                Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowMenu(
                        Point::new(
                            command_rect.x0,
                            command_rect.y1
                                + if data.multiple_tab { 36.0 } else { 0.0 },
                        ),
                        Arc::new(menu_items),
                    ),
                    Target::Auto,
                ),
            ));

            self.borders.push(Line::new(
                Point::new(command_rect.x1, command_rect.y0),
                Point::new(command_rect.x1, command_rect.y1),
            ));

            x = command_rect.x1
        }
        x
    }

    fn update_settings(
        &mut self,
        data: &LapceTabData,
        #[cfg(not(target_os = "macos"))] window_state: &WindowState,
        #[cfg(target_os = "macos")] _window_state: &WindowState,
        #[cfg(not(target_os = "macos"))] piet_text: &mut PietText,
        #[cfg(target_os = "macos")] _piet_text: &mut PietText,
        size: Size,
        _padding: f64,
        x: f64,
    ) -> f64 {
        let mut x = x;
        if cfg!(target_os = "macos") || data.multiple_tab {
            x -= size.height;
        } else {
            x = size.width - (size.height * 4.0);
        }

        let offset = x;

        let settings_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x, 0.0));
        let settings_svg = get_svg("settings.svg").unwrap();
        self.svgs.push((
            settings_svg,
            settings_rect.inflate(-10.5, -10.5),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            ),
        ));
        let latest_version = data
            .latest_release
            .as_ref()
            .as_ref()
            .map(|r| r.version.as_str());
        let menu_items = vec![
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::PaletteCommand,
                    ),
                    data: None,
                },
                enabled: true,
            }),
            MenuKind::Separator,
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::OpenSettings,
                    ),
                    data: None,
                },
                enabled: true,
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::OpenKeyboardShortcuts,
                    ),
                    data: None,
                },
                enabled: true,
            }),
            MenuKind::Separator,
            MenuKind::Item(MenuItem {
                desc: Some(
                    if latest_version.is_some() && latest_version != Some(*VERSION) {
                        format!("Restart to Update ({})", latest_version.unwrap())
                    } else {
                        "Restart to Update".to_string()
                    },
                ),
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::RestartToUpdate,
                    ),
                    data: None,
                },
                enabled: latest_version.is_some()
                    && latest_version != Some(*VERSION),
            }),
        ];
        self.commands.push((
            settings_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    Point::new(
                        settings_rect.x0,
                        settings_rect.y1
                            + if data.multiple_tab { 36.0 } else { 0.0 },
                    ),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));

        #[cfg(not(target_os = "macos"))]
        if !data.multiple_tab {
            x += size.height;
            let (commands, svgs, text_layouts) = window_controls(
                data.window_id,
                window_state,
                piet_text,
                x,
                size.height,
                &data.config,
            );

            for command in commands {
                self.commands.push(command);
            }

            for (svg, rect) in svgs {
                self.svgs.push((
                    svg,
                    rect,
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    ),
                ));
            }

            for text_layout in text_layouts {
                self.text_layouts.push(text_layout);
            }
        }

        offset
    }

    fn update_folder(
        &mut self,
        data: &LapceTabData,
        piet_text: &mut PietText,
        size: Size,
    ) {
        let path = if let Some(workspace_path) = data.workspace.path.as_ref() {
            workspace_path
                .file_name()
                .unwrap_or(workspace_path.as_os_str())
                .to_string_lossy()
                .to_string()
        } else {
            "Open Folder".to_string()
        };
        let remote = match &data.workspace.kind {
            LapceWorkspaceType::Local => "".to_string(),
            LapceWorkspaceType::RemoteSSH(_, host) => {
                format!(" [SSH: {host}]")
            }
            LapceWorkspaceType::RemoteWSL => " [WSL]".to_string(),
        };
        let text = format!("{path}{remote}");
        let text_layout = piet_text
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
        let text_size = text_layout.size();
        let x = (size.width - text_size.width) / 2.0;
        let point = Point::new(x, (size.height - text_layout.size().height) / 2.0);
        self.text_layouts.push((text_layout, point));

        let folder_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x - size.height, 0.0));
        let (folder_svg, folder_rect) = if data.workspace.path.is_none() {
            (
                get_svg("default_folder.svg").unwrap(),
                folder_rect.inflate(-9.0, -9.0),
            )
        } else {
            (
                get_svg("search.svg").unwrap(),
                folder_rect.inflate(-12.0, -12.0),
            )
        };

        self.svgs.push((
            folder_svg,
            folder_rect,
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            ),
        ));
        let menu_items = vec![
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::OpenFolder),
                    data: None,
                },
                enabled: true,
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::PaletteWorkspace,
                    ),
                    data: None,
                },
                enabled: true,
            }),
        ];
        let command_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x + text_size.width - 8.0, 0.0));
        self.svgs.push((
            get_svg("chevron-down.svg").unwrap(),
            command_rect.inflate(-12.0, -12.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            ),
        ));
        self.commands.push((
            command_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowMenu(
                    Point::new(
                        x,
                        size.height + if data.multiple_tab { 36.0 } else { 0.0 },
                    ),
                    Arc::new(menu_items),
                ),
                Target::Auto,
            ),
        ));
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
                ctx.set_handled();
                return;
            }
        }
    }
}

impl Widget<LapceTabData> for Title {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                    ctx.set_handled();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();

                    #[cfg(target_os = "windows")]
                    // ! Currently implemented on Windows only
                    if !data.multiple_tab
                        && self
                            .dragable_area
                            .rects()
                            .iter()
                            .any(|r| r.contains(mouse_event.pos))
                    {
                        ctx.window().handle_titlebar(true);
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            Event::MouseUp(mouse_event) => {
                if !data.multiple_tab
                    && mouse_event.count == 2
                    && self
                        .dragable_area
                        .rects()
                        .iter()
                        .any(|r| r.contains(mouse_event.pos))
                {
                    let state = match ctx.window().get_window_state() {
                        WindowState::Maximized => WindowState::Restored,
                        WindowState::Restored => WindowState::Maximized,
                        WindowState::Minimized => WindowState::Maximized,
                    };
                    ctx.set_handled();
                    ctx.submit_command(
                        druid::commands::CONFIGURE_WINDOW
                            .with(WindowConfig::default().set_window_state(state))
                            .to(Target::Window(data.window_id)),
                    );
                }
            }
            _ => {}
        }
        self.palette.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.palette.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.palette.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let window_state = ctx.window().get_window_state();
        let remaining_rect = self.update_content(
            data,
            &window_state,
            ctx.window().is_fullscreen(),
            ctx.text(),
            Size::new(bc.max().width, 36.0),
        );

        let remaining = bc.max().width
            - (remaining_rect.x0.max(bc.max().width - remaining_rect.x1)) * 2.0
            - 80.0;

        let min_palette_width = if data.palette.status == PaletteStatus::Inactive {
            100.0
        } else {
            300.0
        };
        let palette_width = remaining.min(500.0).max(min_palette_width);
        let palette_size = self.palette.layout(
            ctx,
            &BoxConstraints::tight(Size::new(palette_width, bc.max().height)),
            data,
            env,
        );
        let palette_origin =
            Point::new((bc.max().width - palette_size.width) / 2.0, 0.0);
        self.palette.set_origin(ctx, data, env, palette_origin);
        let palette_rect = self.palette.layout_rect();

        self.dragable_area.clear();
        if !data.multiple_tab {
            self.dragable_area.add_rect(Rect::new(
                remaining_rect.x0,
                0.0,
                palette_rect.x0,
                36.0,
            ));
            self.dragable_area.add_rect(Rect::new(
                palette_rect.x1,
                0.0,
                remaining_rect.x1,
                36.0,
            ));
            ctx.window().set_dragable_area(self.dragable_area.clone());
        }

        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = Size::new(ctx.size().width, 36.0);
        let rect = size.to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        ctx.stroke(
            Line::new(
                Point::new(rect.x0, rect.y1 + 0.5),
                Point::new(rect.x1, rect.y1 + 0.5),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        if data.palette.status == PaletteStatus::Inactive {
            self.palette.paint(ctx, data, env);
        }

        for (rect, color) in self.rects.iter() {
            ctx.fill(rect, color);
        }

        for (svg, rect, color) in self.svgs.iter() {
            ctx.draw_svg(svg, *rect, color.as_ref());
        }

        for (text_layout, point) in self.text_layouts.iter() {
            ctx.draw_text(text_layout, *point);
        }

        for line in self.borders.iter() {
            ctx.stroke(
                line,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }

        if data.palette.status != PaletteStatus::Inactive {
            self.palette.paint(ctx, data, env);
        }
    }
}
