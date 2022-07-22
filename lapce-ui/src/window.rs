#[cfg(any(target_os = "macos", target_os = "windows"))]
use druid::WindowConfig;
use druid::{
    kurbo::Line,
    piet::{PietText, PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    widget::{LensWrap, WidgetExt},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, Region, RenderContext, Size, Target,
    Widget, WidgetId, WidgetPod, WindowConfig, WindowId, WindowState,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{LapceTabData, LapceTabLens, LapceWindowData, LapceWorkspace},
};
use std::cmp::Ordering;
use std::sync::Arc;

use crate::{
    svg::get_svg,
    tab::{LapceTab, LapceTabHeader},
};

pub struct LapceWindow {
    // pub title: WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>,
    pub tabs: Vec<WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>>,
    tab_headers: Vec<
        WidgetPod<
            LapceWindowData,
            LensWrap<LapceWindowData, LapceTabData, LapceTabLens, LapceTabHeader>,
        >,
    >,
    dragable_area: Region,
}

impl LapceWindow {
    pub fn new(data: &mut LapceWindowData) -> Self {
        let tabs = data
            .tabs_order
            .iter()
            .map(|tab_id| {
                let data = data.tabs.get_mut(tab_id).unwrap();
                let tab = LapceTab::new(data);
                let tab = tab.lens(LapceTabLens(*tab_id));
                WidgetPod::new(tab.boxed())
            })
            .collect();
        let tab_headers = data
            .tabs_order
            .iter()
            .map(|tab_id| {
                let tab_header = LapceTabHeader::new().lens(LapceTabLens(*tab_id));
                WidgetPod::new(tab_header)
            })
            .collect();
        Self {
            dragable_area: Region::EMPTY,
            tabs,
            tab_headers,
        }
    }

    pub fn new_tab(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        workspace: LapceWorkspace,
        replace_current: bool,
    ) {
        if replace_current {
            let tab = data.tabs.get(&data.active_id).unwrap();
            let _ = tab.db.save_workspace(tab);
        }
        let tab_id = WidgetId::next();
        let mut tab_data = LapceTabData::new(
            data.window_id,
            tab_id,
            workspace,
            data.db.clone(),
            data.keypress.clone(),
            data.panel_orders.clone(),
            ctx.get_external_handle(),
        );
        let tab = LapceTab::new(&mut tab_data).lens(LapceTabLens(tab_id));
        let tab_header = LapceTabHeader::new().lens(LapceTabLens(tab_id));
        data.tabs.insert(tab_id, tab_data);
        if replace_current {
            self.tabs[data.active] = WidgetPod::new(tab.boxed());
            self.tab_headers[data.active] = WidgetPod::new(tab_header);
            if let Some(tab) = data.tabs.remove(&data.active_id) {
                tab.proxy.stop();
            }
            data.active_id = tab_id;
        } else {
            self.tabs
                .insert(data.active + 1, WidgetPod::new(tab.boxed()));
            self.tab_headers
                .insert(data.active + 1, WidgetPod::new(tab_header));
            data.active += 1;
            data.active_id = tab_id;
        }
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(data.active_id),
        ));
        data.tabs_order = Arc::new(self.tabs.iter().map(|t| t.id()).collect());
        let _ = data.db.save_tabs_async(data);
        ctx.children_changed();
        ctx.set_handled();
        ctx.request_layout();
    }

    pub fn close_index_tab(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        index: usize,
    ) {
        if data.tabs.len() == 1 {
            return;
        }

        let id = self.tabs[index].id();
        self.tabs.remove(index);
        self.tab_headers.remove(index);
        if let Some(tab) = data.tabs.remove(&id) {
            let _ = tab.db.save_workspace(&tab);
            tab.proxy.stop();
        }

        match data.active.cmp(&index) {
            Ordering::Greater => data.active -= 1,
            Ordering::Equal => {
                if data.active >= self.tabs.len() {
                    data.active = self.tabs.len() - 1;
                }
                data.active_id = self.tabs[data.active].id();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(data.active_id),
                ));
            }
            _ => (),
        }

        data.tabs_order = Arc::new(self.tabs.iter().map(|t| t.id()).collect());
        let _ = data.db.save_tabs_async(data);
        ctx.children_changed();
        ctx.set_handled();
        ctx.request_layout();
    }

    pub fn close_tab_id(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        tab_id: WidgetId,
    ) {
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab_id == tab.id() {
                self.close_index_tab(ctx, data, i);
                return;
            }
        }
    }

    pub fn close_tab(&mut self, ctx: &mut EventCtx, data: &mut LapceWindowData) {
        self.close_index_tab(ctx, data, data.active);
    }
}

impl Widget<LapceWindowData> for LapceWindow {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceWindowData,
        env: &Env,
    ) {
        match event {
            Event::WindowPosition(pos) => {
                ctx.set_handled();
                data.pos = *pos;
            }
            Event::WindowSize(size) => {
                ctx.set_handled();
                data.size = *size;
                data.maximised = matches!(
                    ctx.window().get_window_state(),
                    WindowState::Maximized
                );
            }
            Event::WindowConnected => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(data.active_id),
                ));
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Event::MouseUp(mouse_event) => {
                if (cfg!(target_os = "macos") || data.config.ui.custom_titlebar())
                    && data.tabs.len() > 1
                    && mouse_event.count >= 2
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
                    ctx.submit_command(
                        druid::commands::CONFIGURE_WINDOW
                            .with(WindowConfig::default().set_window_state(state))
                            .to(Target::Window(data.window_id)),
                    )
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdatePluginDescriptions(plugins) => {
                        data.plugins = Arc::new(plugins.to_owned());
                    }
                    LapceUICommand::Focus => {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.active_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadConfig => {
                        data.config = Arc::new(
                            Config::load(&LapceWorkspace::default())
                                .unwrap_or_default(),
                        );
                        for (_, tab) in data.tabs.iter_mut() {
                            tab.config = Arc::new(
                                Config::load(&tab.workspace.clone())
                                    .unwrap_or_default(),
                            );
                        }
                        Arc::make_mut(&mut data.keypress)
                            .update_keymaps(&data.config);
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadWindow => {
                        let tab = data.tabs.get(&data.active_id).unwrap();

                        let workspace = (*tab.workspace).clone();
                        self.new_tab(ctx, data, workspace, true);
                        return;
                    }
                    LapceUICommand::SetWorkspace(workspace) => {
                        let mut workspaces =
                            Config::recent_workspaces().unwrap_or_default();

                        let mut exits = false;
                        for w in workspaces.iter_mut() {
                            if w.path == workspace.path && w.kind == workspace.kind {
                                w.last_open = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();
                                exits = true;
                            }
                        }
                        if !exits {
                            workspaces.push(workspace.clone());
                        }
                        workspaces.sort_by_key(|w| -(w.last_open as i64));
                        Config::update_recent_workspaces(workspaces);

                        self.new_tab(ctx, data, workspace.clone(), true);
                        return;
                    }
                    LapceUICommand::SetTheme(theme, preview) => {
                        let config = Arc::make_mut(&mut data.config);
                        config.set_theme(theme, *preview);
                        if *preview {
                            for (_, tab) in data.tabs.iter_mut() {
                                Arc::make_mut(&mut tab.config)
                                    .set_theme(theme, true);
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::NewTab => {
                        self.new_tab(ctx, data, LapceWorkspace::default(), false);
                        return;
                    }
                    LapceUICommand::CloseTab => {
                        self.close_tab(ctx, data);
                        return;
                    }
                    LapceUICommand::CloseTabId(tab_id) => {
                        self.close_tab_id(ctx, data, *tab_id);
                        return;
                    }
                    LapceUICommand::FocusTabId(tab_id) => {
                        for (i, tab) in self.tabs.iter().enumerate() {
                            if tab_id == &tab.id() {
                                if i != data.active {
                                    data.active = i;
                                    data.active_id = tab.id();
                                    let _ = data.db.save_tabs_async(data);
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::Focus,
                                        Target::Widget(data.active_id),
                                    ));
                                }
                                return;
                            }
                        }
                        return;
                    }
                    LapceUICommand::SwapTab(index) => {
                        self.tabs.swap(data.active, *index);
                        self.tab_headers.swap(data.active, *index);
                        data.active = *index;
                        data.tabs_order =
                            Arc::new(self.tabs.iter().map(|t| t.id()).collect());
                        let _ = data.db.save_tabs_async(data);
                        return;
                    }
                    LapceUICommand::NextTab => {
                        let new_index = if data.active >= self.tabs.len() - 1 {
                            0
                        } else {
                            data.active + 1
                        };
                        let _ = data.db.save_workspace_async(
                            data.tabs.get(&data.active_id).unwrap(),
                        );
                        data.active = new_index;
                        data.active_id = self.tabs[new_index].id();
                        let _ = data.db.save_tabs_async(data);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.active_id),
                        ));
                        ctx.request_layout();
                        ctx.set_handled();
                    }
                    LapceUICommand::PreviousTab => {
                        let new_index = if data.active == 0 {
                            self.tabs.len() - 1
                        } else {
                            data.active - 1
                        };
                        let _ = data.db.save_workspace_async(
                            data.tabs.get(&data.active_id).unwrap(),
                        );
                        data.active = new_index;
                        data.active_id = self.tabs[new_index].id();
                        let _ = data.db.save_tabs_async(data);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.active_id),
                        ));
                        ctx.request_layout();
                        ctx.set_handled();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.tabs[data.active].event(ctx, event, data, env);
        match event {
            Event::MouseDown(_)
            | Event::MouseUp(_)
            | Event::MouseMove(_)
            | Event::Wheel(_)
            | Event::AnimFrame(_)
            | Event::KeyDown(_)
            | Event::KeyUp(_) => {}
            _ => {
                for (i, tab) in self.tabs.iter_mut().enumerate() {
                    if i != data.active {
                        tab.event(ctx, event, data, env);
                    }
                }
            }
        }
        for tab_header in self.tab_headers.iter_mut() {
            tab_header.event(ctx, event, data, env);
        }
        // self.title.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceWindowData,
        env: &Env,
    ) {
        // self.title.lifecycle(ctx, event, data, env);
        for tab in self.tabs.iter_mut() {
            tab.lifecycle(ctx, event, data, env);
        }
        for tab_header in self.tab_headers.iter_mut() {
            tab_header.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceWindowData,
        data: &LapceWindowData,
        env: &Env,
    ) {
        // self.title.update(ctx, data, env);

        if old_data.active != data.active {
            ctx.request_layout();
        }
        let old_tab = old_data.tabs.get(&old_data.active_id).unwrap();
        let tab = data.tabs.get(&data.active_id).unwrap();
        if old_tab.workspace != tab.workspace {
            ctx.request_layout();
        }
        for tab in self.tabs.iter_mut() {
            tab.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceWindowData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();

        let (tab_size, tab_origin) = if self.tabs.len() > 1 {
            let tab_header_height = 36.0;
            let tab_header_padding = 0.0;
            let tab_size = Size::new(
                self_size.width,
                self_size.height - tab_header_height - tab_header_padding,
            );
            let tab_origin = Point::new(0.0, tab_header_height + tab_header_padding);

            #[cfg(not(target_os = "macos"))]
            let left_padding = 0.0;
            #[cfg(target_os = "macos")]
            let left_padding = if ctx.window().is_fullscreen() {
                0.0
            } else {
                78.0
            };

            let right_padding = 100.0;

            let total_width = self_size.width - left_padding - right_padding;
            let width =
                ((total_width / self.tab_headers.len() as f64).min(200.0)).round();

            let mut x = left_padding;
            let mut drag = None;
            let bc = BoxConstraints::tight(Size::new(width, tab_header_height));
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let size = tab_header.layout(ctx, &bc, data, env);
                let mut origin = Point::new(x, tab_header_padding);
                let header = tab_header.widget().child();
                if let Some(o) = header.origin() {
                    origin = Point::new(o.x, tab_header_padding);
                    drag = Some((i, header.mouse_pos));
                }
                tab_header.set_origin(ctx, data, env, origin);
                x += size.width;
            }
            x += 36.0;

            let mut region = Region::EMPTY;
            region.add_rect(
                Size::new(self_size.width - x, 36.0)
                    .to_rect()
                    .with_origin(Point::new(x, 0.0)),
            );
            if left_padding > 0.0 {
                region.add_rect(
                    Size::new(left_padding, 36.0)
                        .to_rect()
                        .with_origin(Point::new(0.0, 0.0)),
                );
            }

            self.dragable_area.clear();
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if cfg!(target_os = "macos") || data.config.ui.custom_titlebar() {
                ctx.window().set_dragable_area(region.clone());
                self.dragable_area = region;
            }

            if let Some((index, mouse_pos)) = drag {
                for (i, tab_header) in self.tab_headers.iter().enumerate() {
                    if i != index {
                        let rect = tab_header.layout_rect();
                        if rect.x0 <= mouse_pos.x && rect.x1 >= mouse_pos.x {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::SwapTab(i),
                                Target::Auto,
                            ));
                            break;
                        }
                    }
                }
            }

            (tab_size, tab_origin)
        } else {
            for (_i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let bc = BoxConstraints::tight(Size::new(self_size.width, 0.0));
                tab_header.layout(ctx, &bc, data, env);
                tab_header.set_origin(ctx, data, env, Point::new(0.0, 0.0));
            }
            let tab_size = self_size;

            (tab_size, Point::ZERO)
        };

        let bc = BoxConstraints::tight(tab_size);
        self.tabs[data.active].layout(ctx, &bc, data, env);
        self.tabs[data.active].set_origin(ctx, data, env, tab_origin);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceWindowData, env: &Env) {
        let _start = std::time::SystemTime::now();
        self.tabs[data.active].paint(ctx, data, env);

        // let title_height = self.title.layout_rect().height();

        let tab_height = 36.0;
        let size = ctx.size();
        if self.tabs.len() > 1 {
            let rect = Size::new(size.width, tab_height).to_rect();
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                tab_header.paint(ctx, data, env);
                let rect = tab_header.layout_rect();
                if i == 0 {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0, rect.y0 + 8.0),
                            Point::new(rect.x0, rect.y1 - 8.0),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
                ctx.stroke(
                    Line::new(
                        Point::new(rect.x1, rect.y0 + 8.0),
                        Point::new(rect.x1, rect.y1 - 8.0),
                    ),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.blurred_rect(
                    self.tabs[data.active].layout_rect(),
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            } else {
                ctx.stroke(
                    self.tabs[data.active].layout_rect(),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }

            let rect = self.tab_headers[data.active].layout_rect();
            ctx.stroke(
                Line::new(
                    Point::new(rect.x0, rect.y1 - 2.0),
                    Point::new(rect.x1, rect.y1 - 2.0),
                ),
                data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                2.0,
            );
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn window_controls(
    window_id: WindowId,
    window_state: &WindowState,
    piet_text: &mut PietText,
    x: f64,
    width: f64,
    config: &Config,
) -> (
    Vec<(Rect, Command)>,
    Vec<(Svg, Rect)>,
    Vec<(PietTextLayout, Point)>,
) {
    let mut commands = Vec::new();

    let minimise_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x, 0.0));
    commands.push((
        minimise_rect,
        Command::new(
            druid::commands::CONFIGURE_WINDOW,
            WindowConfig::default().set_window_state(WindowState::Minimized),
            Target::Window(window_id),
        ),
    ));

    let max_res_state = if window_state == &WindowState::Restored {
        WindowState::Maximized
    } else {
        WindowState::Restored
    };

    let max_res_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x + width, 0.0));
    commands.push((
        max_res_rect,
        Command::new(
            druid::commands::CONFIGURE_WINDOW,
            WindowConfig::default().set_window_state(max_res_state),
            Target::Window(window_id),
        ),
    ));

    let close_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x + 2.0 * width, 0.0));

    commands.push((
        close_rect,
        Command::new(druid::commands::QUIT_APP, (), Target::Global),
    ));

    let mut svgs = Vec::new();
    if cfg!(target_os = "linux") {
        let minimize_rect = Size::new(width, width)
            .to_rect()
            .with_origin(Point::new(x, 0.0))
            .inflate(-12.0, -12.0);
        svgs.push((get_svg("chrome-minimize.svg").unwrap(), minimize_rect));

        let max_res_rect = Size::new(width, width)
            .to_rect()
            .with_origin(Point::new(x + width, 0.0))
            .inflate(-10.0, -10.0);
        let max_res_svg = if window_state == &WindowState::Restored {
            get_svg("chrome-maximize.svg").unwrap()
        } else {
            get_svg("chrome-restore.svg").unwrap()
        };
        svgs.push((max_res_svg, max_res_rect));

        let close_rect = Size::new(width, width)
            .to_rect()
            .with_origin(Point::new(x + 2.0 * width, 0.0))
            .inflate(-10.0, -10.0);
        svgs.push((get_svg("chrome-close.svg").unwrap(), close_rect));
    }

    let mut text_layouts = Vec::new();
    if cfg!(target_os = "windows") {
        let texts = vec![
            "\u{E949}",
            if window_state == &WindowState::Restored {
                "\u{E739}"
            } else {
                "\u{E923}"
            },
            "\u{E106}",
        ];
        let font_size = 10.0;
        let font_family = "Segoe MDL2 Assets";
        for (i, text_layout) in texts
            .iter()
            .map(|text| {
                piet_text
                    .new_text_layout(text.to_string())
                    .font(piet_text.font_family(font_family).unwrap(), font_size)
                    .text_color(
                        config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap()
            })
            .enumerate()
        {
            let point = Point::new(
                x + i as f64 * width + ((text_layout.size().width + 5.0) / 2.0),
                0.0,
            );
            text_layouts.push((text_layout, point));
        }
    }

    (commands, svgs, text_layouts)
}
