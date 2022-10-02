use std::sync::Arc;

use druid::{
    kurbo::{Circle, Line},
    piet::{PietText, PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, InternalEvent, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, Region,
    RenderContext, Size, Target, Widget, WidgetExt, WidgetId, WidgetPod,
    WindowConfig, WindowState,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{FocusArea, LapceTabData, LapceWorkspaceType},
    list::ListData,
    menu::{MenuItem, MenuKind},
    palette::PaletteStatus,
    proxy::{ProxyStatus, VERSION},
};

#[cfg(not(target_os = "macos"))]
use crate::window::window_controls;
use crate::{list::List, palette::Palette, svg::get_svg};

pub struct Title {
    widget_id: WidgetId,
    mouse_pos: Point,
    menus: Vec<(Rect, Command)>,
    window_controls: Vec<(Rect, Command)>,
    holding_click_rect: Option<Rect>,
    svgs: Vec<(Svg, Rect, Option<Color>, Option<Color>)>,
    text_layouts: Vec<(PietTextLayout, Point)>,
    borders: Vec<Line>,
    rects: Vec<(Rect, Color)>,
    circles: Vec<(Circle, Color)>,
    palette: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    branch_list: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    dragable_area: Region,
    hover_rect: Option<Rect>,
}

impl Title {
    pub fn new(data: &LapceTabData) -> Self {
        let palette = Palette::new(data);
        let branch_list = SourceControlBranches::new();
        Self {
            widget_id: data.title.widget_id,
            mouse_pos: Point::ZERO,
            menus: Vec::new(),
            window_controls: Vec::new(),
            holding_click_rect: None,
            svgs: Vec::new(),
            text_layouts: Vec::new(),
            borders: Vec::new(),
            rects: Vec::new(),
            circles: Vec::new(),
            palette: WidgetPod::new(palette.boxed()),
            branch_list: WidgetPod::new(branch_list.boxed()),
            dragable_area: Region::EMPTY,
            hover_rect: None,
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
        self.menus.clear();
        self.window_controls.clear();
        self.svgs.clear();
        self.text_layouts.clear();
        self.borders.clear();
        self.rects.clear();
        self.circles.clear();

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
                None,
            ));
            x += size.height;
        }

        let padding = 15.0;
        x = self.update_remote(data, size, padding, x);
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
                .inflate(-6.0, -6.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    .clone(),
            ),
            None,
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

        #[cfg(target_os = "windows")]
        {
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

        self.menus.push((
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
                folder_rect.inflate(-8.5, -8.5),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                ),
                None,
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
            let branches = data.source_control.branches.clone();
            self.menus.push((
                command_rect,
                Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowGitBranches {
                        origin: Point::new(command_rect.x0, command_rect.y1),
                        branches,
                    },
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
        piet_text: &mut PietText,
        size: Size,
        _padding: f64,
        x: f64,
    ) -> f64 {
        let mut x = x;
        if cfg!(target_os = "macos")
            || data.multiple_tab
            || !data.config.core.custom_titlebar
        {
            x -= size.height;
        } else {
            x = size.width - (size.height * 4.0);
        }

        let offset = x;

        let hover_color = {
            let (r, g, b, a) = data
                .config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND)
                .to_owned()
                .as_rgba8();
            // TODO: hacky way to detect "lightness" of colour, should be fixed once we have dark/light themes
            if r < 128 || g < 128 || b < 128 {
                Color::rgba8(
                    r.saturating_add(25),
                    g.saturating_add(25),
                    b.saturating_add(25),
                    a,
                )
            } else {
                Color::rgba8(
                    r.saturating_sub(30),
                    g.saturating_sub(30),
                    b.saturating_sub(30),
                    a,
                )
            }
        };

        let settings_rect = Size::new(size.height, size.height)
            .to_rect()
            .with_origin(Point::new(x, 0.0));
        let settings_svg = get_svg("settings.svg").unwrap();
        self.svgs.push((
            settings_svg,
            settings_rect.inflate(-10.0, -10.0),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            ),
            Some(hover_color),
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
                        format!("Restart to update ({})", latest_version.unwrap())
                    } else {
                        "No update available".to_string()
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
            MenuKind::Separator,
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::ShowAbout),
                    data: None,
                },
                enabled: true,
            }),
        ];
        if latest_version.is_some() && latest_version != Some(*VERSION) {
            let text_layout = piet_text
                .new_text_layout("1")
                .font(data.config.ui.font_family(), 10.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let size = text_layout.size();
            let point = settings_rect.center() + (5.0, 3.0);
            let circle = Circle::new(
                Point::new(point.x + size.width / 2.0, point.y + size.height / 2.0),
                ((size.width / 2.0).powi(2) + (size.height / 2.0).powi(2)).sqrt(),
            );
            self.circles.push((
                circle,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_CARET)
                    .clone(),
            ));
            self.text_layouts.push((text_layout, point));
        }
        self.menus.push((
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
        if !data.multiple_tab && data.config.core.custom_titlebar {
            x += size.height;
            let (window_controls, svgs) = window_controls(
                *data.window_id,
                window_state,
                x,
                size.height,
                &data.config,
            );

            for command in window_controls {
                self.window_controls.push(command);
            }

            for (svg, rect, color) in svgs {
                self.svgs.push((
                    svg,
                    rect,
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    ),
                    Some(color),
                ));
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
            None,
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
            None,
        ));
        self.menus.push((
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

    fn icon_hit_test(&mut self, mouse_event: &MouseEvent) -> bool {
        for (rect, _) in self.menus.iter() {
            if rect.contains(mouse_event.pos) {
                self.hover_rect = Some(*rect);
                return true;
            }
        }
        for (rect, _) in self.window_controls.iter() {
            if rect.contains(mouse_event.pos) {
                self.hover_rect = Some(*rect);
                return true;
            }
        }
        false
    }

    fn mouse_down(&mut self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for (rect, command) in self.menus.iter() {
            if rect.contains(mouse_event.pos) {
                self.holding_click_rect = Some(*rect);
                ctx.submit_command(command.clone());
                ctx.set_handled();
                return;
            }
        }
        for (rect, _command) in self.window_controls.iter() {
            if rect.contains(mouse_event.pos) {
                self.holding_click_rect = Some(*rect);
                ctx.set_handled();
                return;
            }
        }
    }

    fn mouse_up(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for (rect, command) in self.window_controls.iter() {
            if rect.contains(mouse_event.pos)
                && self.holding_click_rect.eq(&Some(*rect))
            {
                ctx.submit_command(command.clone());
                ctx.set_handled();
                return;
            }
        }
    }
}

impl Widget<LapceTabData> for Title {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Internal(InternalEvent::MouseLeave) => {
                self.mouse_pos = Point::ZERO;
                ctx.request_paint();
            }
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                let hover_rect = self.hover_rect;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.set_handled();
                } else {
                    self.hover_rect = None;
                    ctx.clear_cursor();

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
                if hover_rect != self.hover_rect {
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                if mouse_event.button.is_left() {
                    self.mouse_down(ctx, mouse_event);
                }
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
                            .to(Target::Window(*data.window_id)),
                    );
                    return;
                }

                if mouse_event.button.is_left() {
                    self.mouse_up(ctx, mouse_event);
                }

                self.holding_click_rect = None;
            }
            _ => {}
        }
        self.palette.event(ctx, event, data, env);

        if event.should_propagate_to_hidden() || data.title.branches.active {
            self.branch_list.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.palette.lifecycle(ctx, event, data, env);
        self.branch_list.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.main_split.can_jump_location_forward()
            != old_data.main_split.can_jump_location_forward()
        {
            ctx.request_layout();
        }
        if data.main_split.can_jump_location_backward()
            != old_data.main_split.can_jump_location_backward()
        {
            ctx.request_layout();
        }
        if data.source_control.branch != old_data.source_control.branch {
            ctx.request_layout();
        }
        self.palette.update(ctx, data, env);
        self.branch_list.update(ctx, data, env);
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
            - 36.0 * 4.0;

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

        // TODO: Let this be configurable
        let bl_width = 200.0;
        let bl_height = 500.0;
        let bl_bc = BoxConstraints::tight(Size::new(bl_width, bl_height));
        let _bl_size = self.branch_list.layout(ctx, &bl_bc, data, env);
        self.branch_list
            .set_origin(ctx, data, env, data.title.branches.origin);

        let target = if let Some(active) = *data.main_split.active {
            Target::Widget(active)
        } else {
            Target::Auto
        };
        let arrow_left_rect = Size::new(36.0, 36.0)
            .to_rect()
            .with_origin(Point::new(palette_origin.x - 36.0 - 36.0, 0.0));
        let (arrow_left_svg_color, arrow_left_svg_hover_color) = if !data
            .main_split
            .can_jump_location_backward()
        {
            (
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone(),
                ),
                None,
            )
        } else {
            self.menus.push((
                arrow_left_rect,
                Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::JumpLocationBackward),
                        data: None,
                    },
                    target,
                ),
            ));
            (
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                ),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_CURRENT)
                        .clone(),
                ),
            )
        };
        self.svgs.push((
            get_svg("arrow-left.svg").unwrap(),
            arrow_left_rect.inflate(-10.5, -10.5),
            arrow_left_svg_color,
            arrow_left_svg_hover_color,
        ));

        let arrow_right_rect = Size::new(36.0, 36.0)
            .to_rect()
            .with_origin(Point::new(palette_origin.x - 36.0, 0.0));
        let (arrow_right_svg_color, arrow_right_svg_hover_color) = if !data
            .main_split
            .can_jump_location_forward()
        {
            (
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone(),
                ),
                None,
            )
        } else {
            self.menus.push((
                arrow_right_rect,
                Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::JumpLocationForward),
                        data: None,
                    },
                    target,
                ),
            ));
            (
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                ),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_CURRENT)
                        .clone(),
                ),
            )
        };
        self.svgs.push((
            get_svg("arrow-right.svg").unwrap(),
            arrow_right_rect.inflate(-10.5, -10.5),
            arrow_right_svg_color,
            arrow_right_svg_hover_color,
        ));

        self.dragable_area.clear();
        if !data.multiple_tab {
            self.dragable_area.add_rect(Rect::new(
                remaining_rect.x0,
                0.0,
                palette_rect.x0 - 36.0 * 2.0,
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

        for (svg, rect, color, bg_color) in self.svgs.iter() {
            let hover_rect = rect.inflate(10.0, 10.0);
            if hover_rect.contains(self.mouse_pos)
                && bg_color.is_some()
                && (self.holding_click_rect.is_none()
                    || self.holding_click_rect.unwrap().contains(self.mouse_pos))
            {
                let bg_color = bg_color.to_owned().unwrap();
                ctx.fill(hover_rect, &bg_color);

                // TODO: hacky way to detect close button, should be fixed once we have dark/light themes
                if bg_color.as_rgba8() == (210, 16, 33, 255) {
                    ctx.draw_svg(svg, *rect, Some(&Color::WHITE));
                } else {
                    ctx.draw_svg(svg, *rect, color.as_ref());
                }
            } else {
                ctx.draw_svg(svg, *rect, color.as_ref());
            }
        }

        for (circle, color) in self.circles.iter() {
            ctx.fill(circle, color);
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

        if data.title.branches.active {
            self.branch_list.paint(ctx, data, env);
        }
    }
}

// TODO: Implement an input for filtering the branches
pub struct SourceControlBranches {
    widget_id: WidgetId,
    list: WidgetPod<ListData<String, ()>, List<String, ()>>,
}
impl SourceControlBranches {
    fn new() -> Self {
        let widget_id = WidgetId::next();
        let scroll_id = WidgetId::next();
        Self {
            widget_id,
            list: WidgetPod::new(List::new(scroll_id)),
        }
    }

    fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        ctx.request_focus();
        data.focus_area = FocusArea::BranchPicker;
        data.focus = Arc::new(self.widget_id);
    }
}
impl Widget<LapceTabData> for SourceControlBranches {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        let title = Arc::make_mut(&mut data.title);
        title.branches.list.update_data(data.config.clone());
        self.list.event(ctx, event, &mut title.branches.list, env);

        match event {
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let title = Arc::make_mut(&mut data.title);
                Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    &mut title.branches,
                    env,
                );
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Hide => {
                        Arc::make_mut(&mut data.title).branches.active = false;
                    }
                    LapceUICommand::Focus => {
                        self.request_focus(ctx, data);
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowGitBranches { origin, branches } => {
                        let title = Arc::make_mut(&mut data.title);
                        title.branches.list.clear_items();

                        title.branches.list.items = branches.clone();
                        title.branches.origin = *origin;

                        // Make so the default selected entry is the current branch
                        let current_branch = &data.source_control.branch;
                        let current_item_index = title
                            .branches
                            .list
                            .items
                            .iter()
                            .enumerate()
                            .find(|(_, branch)| *branch == current_branch)
                            .map(|(i, _)| i);
                        title.branches.list.selected_index =
                            current_item_index.unwrap_or(0);

                        title.branches.active = true;

                        self.request_focus(ctx, data);
                        ctx.set_handled();
                    }
                    LapceUICommand::ListItemSelected => {
                        let title = Arc::make_mut(&mut data.title);
                        if let Some(branch) =
                            title.branches.list.current_selected_item()
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Workbench(
                                        LapceWorkbenchCommand::CheckoutBranch,
                                    ),
                                    data: Some(serde_json::json!(branch.clone())),
                                },
                                Target::Auto,
                            ));
                        }

                        title.branches.active = false;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::FocusChanged(focus) = event {
            if !focus {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Hide,
                    Target::Widget(self.widget_id),
                ));
            }
        }
        self.list.lifecycle(
            ctx,
            event,
            &data.title.branches.list.clone_with(data.config.clone()),
            env,
        );
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.title.branches.active != old_data.title.branches.active {
            ctx.request_layout();
        }

        self.list.update(
            ctx,
            &data.title.branches.list.clone_with(data.config.clone()),
            env,
        );
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let list_data = &data.title.branches.list.clone_with(data.config.clone());
        let size = self.list.layout(ctx, bc, list_data, env);
        // The moving of the origin is handled by the title widget which contains this
        self.list.set_origin(ctx, list_data, env, Point::ZERO);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        self.list.paint(
            ctx,
            &data.title.branches.list.clone_with(data.config.clone()),
            env,
        )
    }
}
