use std::{cmp::Ordering, sync::Arc};

use druid::{
    kurbo::Line,
    widget::{LensWrap, WidgetExt},
    BoxConstraints, Command, Data, Env, Event, EventCtx, InternalEvent, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Region, RenderContext,
    SingleUse, Size, Target, Widget, WidgetId, WidgetPod, WindowConfig, WindowState,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceTheme},
    data::{LapceTabData, LapceTabLens, LapceWindowData, LapceWorkspace},
};
use lapce_rpc::plugin::VoltID;

use crate::tab::{LapceTab, LapceTabHeader, LapceTabMeta, LAPCE_TAB_META};

pub struct LapceWindow {
    pub mouse_pos: Point,
    // pub title: WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>,
    pub tabs: Vec<WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>>,
    pub tab_headers: Vec<
        WidgetPod<
            LapceWindowData,
            LensWrap<LapceWindowData, LapceTabData, LapceTabLens, LapceTabHeader>,
        >,
    >,
    pub draggable_area: Region,
    pub tab_header_cmds: Vec<(Rect, Command)>,
    pub mouse_down_cmd: Option<(Rect, Command)>,
    #[cfg(not(target_os = "macos"))]
    pub holding_click_rect: Option<Rect>,
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
            mouse_pos: Point::ZERO,
            draggable_area: Region::EMPTY,
            tabs,
            tab_headers,
            tab_header_cmds: Vec::new(),
            mouse_down_cmd: None,
            #[cfg(not(target_os = "macos"))]
            holding_click_rect: None,
        }
    }

    pub fn new_tab(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        workspace: LapceWorkspace,
        replace_current: bool,
    ) {
        let _ = data.db.update_recent_workspace(workspace.clone());

        let current_panels = {
            let tab = data.tabs.get(&data.active_id).unwrap();
            if replace_current {
                let _ = tab.db.save_workspace(tab);
            }
            (*tab.panel).clone()
        };

        let tab_id = WidgetId::next();
        let mut tab_data = LapceTabData::new(
            data.window_id,
            tab_id,
            workspace,
            data.db.clone(),
            data.keypress.clone(),
            data.latest_release.clone(),
            data.update_in_progress,
            data.log_file.clone(),
            Some(current_panels),
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
            data.active_id = Arc::new(tab_id);
        } else {
            self.tabs
                .insert(data.active + 1, WidgetPod::new(tab.boxed()));
            self.tab_headers
                .insert(data.active + 1, WidgetPod::new(tab_header));
            data.active += 1;
            data.active_id = Arc::new(tab_id);
        }
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(*data.active_id),
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
        stop_proxy: bool,
    ) -> Option<LapceTabMeta> {
        let mut removed_tab = None;
        if data.tabs.len() == 1 {
            return removed_tab;
        }

        let id = self.tabs[index].id();
        let tab_widget = self.tabs.remove(index);
        self.tab_headers.remove(index);
        if let Some(tab) = data.tabs.remove(&id) {
            let _ = tab.db.save_workspace(&tab);
            if stop_proxy {
                tab.proxy.stop();
            }
            removed_tab = Some(LapceTabMeta {
                data: tab,
                widget: tab_widget,
            });
        }

        match data.active.cmp(&index) {
            Ordering::Greater => data.active -= 1,
            Ordering::Equal => {
                if data.active >= self.tabs.len() {
                    data.active = self.tabs.len() - 1;
                }
                data.active_id = Arc::new(self.tabs[data.active].id());
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(*data.active_id),
                ));
            }
            _ => (),
        }

        data.tabs_order = Arc::new(self.tabs.iter().map(|t| t.id()).collect());
        let _ = data.db.save_tabs_async(data);
        ctx.children_changed();
        ctx.set_handled();
        ctx.request_layout();

        removed_tab
    }

    pub fn close_tab_id(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        tab_id: WidgetId,
        stop_proxy: bool,
    ) -> Option<LapceTabMeta> {
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab_id == tab.id() {
                return self.close_index_tab(ctx, data, i, stop_proxy);
            }
        }
        None
    }

    pub fn close_tab(&mut self, ctx: &mut EventCtx, data: &mut LapceWindowData) {
        self.close_index_tab(ctx, data, data.active, true);
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
                    Target::Widget(*data.active_id),
                ));
            }
            Event::Internal(InternalEvent::MouseLeave) => {
                self.mouse_pos = Point::ZERO;
                ctx.request_paint();
            }
            Event::MouseMove(mouse_event) => {
                ctx.clear_cursor();
                self.mouse_pos = mouse_event.pos;

                #[cfg(not(target_os = "macos"))]
                if data.tabs.len() > 1 && mouse_event.count < 2 {
                    for (rect, _) in self.tab_header_cmds.iter() {
                        if rect.contains(mouse_event.pos) {
                            ctx.set_cursor(&druid::Cursor::Pointer);
                            ctx.request_paint();
                            break;
                        }
                    }
                }

                #[cfg(target_os = "windows")]
                if data.tabs.len() > 1
                    && self
                        .draggable_area
                        .rects()
                        .iter()
                        .any(|r| r.contains(mouse_event.pos))
                {
                    ctx.window().handle_titlebar(true);
                }
            }
            Event::MouseDown(_mouse_event) => {
                self.mouse_down_cmd = None;
                #[cfg(not(target_os = "macos"))]
                if (data.tabs.len() > 1 && _mouse_event.count == 1)
                    || data.config.core.custom_titlebar
                {
                    for (rect, cmd) in self.tab_header_cmds.iter() {
                        if rect.contains(_mouse_event.pos) {
                            self.mouse_down_cmd = Some((*rect, cmd.clone()));
                            self.holding_click_rect = Some(*rect);
                            break;
                        }
                    }
                }
            }
            Event::MouseUp(mouse_event) => {
                if data.tabs.len() > 1
                    && mouse_event.count == 2
                    && self
                        .draggable_area
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

                #[cfg(not(target_os = "macos"))]
                if (data.tabs.len() > 1 && mouse_event.count < 2)
                    || data.config.core.custom_titlebar
                {
                    if let Some((rect, cmd)) = self.mouse_down_cmd.as_ref() {
                        if rect.contains(mouse_event.pos) {
                            ctx.submit_command(cmd.clone());
                        }
                    }

                    for (rect, cmd) in self.tab_header_cmds.iter() {
                        if let Some(click_rect) = self.holding_click_rect {
                            if rect.contains(mouse_event.pos)
                                && click_rect.contains(mouse_event.pos)
                            {
                                ctx.submit_command(cmd.clone());
                                ctx.set_handled();
                            }
                        }
                    }

                    self.holding_click_rect = None;
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(*data.active_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadConfig => {
                        data.config = Arc::new(LapceConfig::load(
                            &LapceWorkspace::default(),
                            &[],
                        ));
                        for (_, tab) in data.tabs.iter_mut() {
                            let mut disabled_volts: Vec<VoltID> =
                                tab.plugin.disabled.clone().into_iter().collect();
                            disabled_volts.append(
                                &mut tab
                                    .plugin
                                    .workspace_disabled
                                    .clone()
                                    .into_iter()
                                    .collect(),
                            );
                            tab.config = Arc::new(LapceConfig::load(
                                &tab.workspace.clone(),
                                &disabled_volts,
                            ));
                            tab.proxy
                                .proxy_rpc
                                .update_plugin_configs(data.config.plugins.clone());
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
                        self.new_tab(ctx, data, workspace.clone(), true);
                        return;
                    }
                    LapceUICommand::SetColorTheme { theme, preview } => {
                        let config = Arc::make_mut(&mut data.config);
                        config.set_color_theme(
                            &LapceWorkspace::default(),
                            theme,
                            *preview,
                        );
                        for (_, tab) in data.tabs.iter_mut() {
                            Arc::make_mut(&mut tab.config).set_color_theme(
                                &tab.workspace,
                                theme,
                                true,
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::SetIconTheme { theme, preview } => {
                        let config = Arc::make_mut(&mut data.config);
                        config.set_icon_theme(
                            &LapceWorkspace::default(),
                            theme,
                            *preview,
                        );
                        for (_, tab) in data.tabs.iter_mut() {
                            Arc::make_mut(&mut tab.config).set_icon_theme(
                                &tab.workspace,
                                theme,
                                true,
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowWindow => {
                        ctx.set_handled();
                        ctx.submit_command(druid::commands::SHOW_WINDOW);
                        return;
                    }
                    LapceUICommand::NewTab(workspace) => {
                        ctx.set_handled();
                        self.new_tab(
                            ctx,
                            data,
                            workspace.clone().unwrap_or_default(),
                            false,
                        );
                        return;
                    }
                    LapceUICommand::CloseTab => {
                        self.close_tab(ctx, data);
                        return;
                    }
                    LapceUICommand::CloseTabId(tab_id) => {
                        self.close_tab_id(ctx, data, *tab_id, true);
                        return;
                    }
                    LapceUICommand::TabToWindow(_, tab_id) => {
                        if let Some(meta) =
                            self.close_tab_id(ctx, data, *tab_id, false)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_TAB_META,
                                SingleUse::new(meta),
                                Target::Global,
                            ))
                        }
                        return;
                    }
                    LapceUICommand::FocusTabId(tab_id) => {
                        for (i, tab) in self.tabs.iter().enumerate() {
                            if tab_id == &tab.id() {
                                if i != data.active {
                                    data.active = i;
                                    data.active_id = Arc::new(tab.id());
                                    let _ = data.db.save_tabs_async(data);
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::Focus,
                                        Target::Widget(*data.active_id),
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
                        data.active_id = Arc::new(self.tabs[new_index].id());
                        let _ = data.db.save_tabs_async(data);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(*data.active_id),
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
                        data.active_id = Arc::new(self.tabs[new_index].id());
                        let _ = data.db.save_tabs_async(data);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(*data.active_id),
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
        #[cfg(not(platform_os = "macos"))]
        if data.config.core.custom_titlebar != old_data.config.core.custom_titlebar {
            ctx.window()
                .show_titlebar(!data.config.core.custom_titlebar);
        }
        let old_tab = old_data.tabs.get(&old_data.active_id).unwrap();
        let tab = data.tabs.get(&data.active_id).unwrap();
        if old_tab.workspace != tab.workspace {
            ctx.request_layout();
        }
        for tab in self.tabs.iter_mut() {
            tab.update(ctx, data, env);
        }

        if !old_data.latest_release.same(&data.latest_release) {
            ctx.request_layout();
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
            let tab_size = Size::new(
                self_size.width,
                self_size.height - TAB_HEADER_HEIGHT - TAB_HEADER_PADDING,
            );
            let tab_origin = Point::new(0.0, TAB_HEADER_HEIGHT + TAB_HEADER_PADDING);

            #[cfg(not(target_os = "macos"))]
            let left_padding = 0.0;
            #[cfg(target_os = "macos")]
            let left_padding = if ctx.window().is_fullscreen()
                || !data.config.core.custom_titlebar
            {
                0.0
            } else {
                78.0
            };

            let right_buttons_width = if cfg!(target_os = "macos") {
                0.0
            } else {
                WINDOW_CONTROL_BUTTON_WIDTH * 3.0
            };
            let right_padding = RIGHT_PADDING + right_buttons_width;

            let total_width = self_size.width - left_padding - right_padding;
            let width =
                ((total_width / self.tab_headers.len() as f64).min(200.0)).round();

            let mut x = left_padding;
            let mut drag = None;
            let bc = BoxConstraints::tight(Size::new(width, TAB_HEADER_HEIGHT));
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let size = tab_header.layout(ctx, &bc, data, env);
                let header = tab_header.widget().child();

                let origin = header.origin();

                if origin.is_some() {
                    drag = Some(i);
                }

                let origin_x = origin.map_or(x, |origin| origin.x);
                let origin = Point::new(origin_x, TAB_HEADER_PADDING);

                tab_header.set_origin(ctx, data, env, origin);
                x += size.width;
            }

            self.draggable_area = if data.config.core.custom_titlebar {
                // Area right of tabs, but left of the window control buttons
                let mut draggable_area = Region::from(
                    Size::new(
                        self_size.width - x - right_buttons_width,
                        TAB_HEADER_HEIGHT,
                    )
                    .to_rect()
                    .with_origin(Point::new(x, 0.0)),
                );

                // Area left of the tabs
                draggable_area.add_rect(
                    Size::new(left_padding, TAB_HEADER_HEIGHT)
                        .to_rect()
                        .with_origin(Point::ZERO),
                );

                draggable_area
            } else {
                Region::EMPTY
            };

            ctx.window().set_dragable_area(self.draggable_area.clone());

            if let Some(dragged_tab_idx) = drag {
                // Because tab is opaque, use the tab's center point instead of the mouse position.
                let dragged_tab_center =
                    self.tab_headers[dragged_tab_idx].layout_rect().center();

                if let Some((swapped_idx, _)) = self
                    .tab_headers
                    .iter()
                    .enumerate()
                    .find(|&(idx, tab_header)| {
                        idx != dragged_tab_idx
                            && tab_header.layout_rect().contains(dragged_tab_center)
                    })
                {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SwapTab(swapped_idx),
                        Target::Auto,
                    ));
                }
            }

            (tab_size, tab_origin)
        } else {
            for tab_header in self.tab_headers.iter_mut() {
                let bc = BoxConstraints::tight(Size::new(self_size.width, 0.0));
                tab_header.layout(ctx, &bc, data, env);
                tab_header.set_origin(ctx, data, env, Point::ZERO);
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

        let size = ctx.size();
        if self.tabs.len() > 1 {
            let rect = Size::new(size.width, TAB_HEADER_HEIGHT).to_rect();
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );

            let mut dragged = None;
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                // Store index and skip if tab is being dragged.
                if tab_header.widget().child().origin().is_some() {
                    dragged = Some(i);
                    continue;
                }

                tab_header.paint(ctx, data, env);
            }

            // Draw the dragged tab last, above the others
            if let Some(dragged_tab_idx) = dragged {
                let tab_header = &mut self.tab_headers[dragged_tab_idx];

                tab_header.paint(ctx, data, env);
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

            self.tab_header_cmds.clear();
            #[cfg(not(target_os = "macos"))]
            if data.config.core.custom_titlebar {
                let (cmds, svgs) = window_controls(
                    data.window_id,
                    &ctx.window().get_window_state(),
                    size.width - WINDOW_CONTROL_BUTTON_WIDTH * 3.0,
                    WINDOW_CONTROL_BUTTON_WIDTH,
                    &data.config,
                );
                self.tab_header_cmds = cmds;

                for (svg, rect, color) in svgs {
                    let hover_rect = rect.inflate(10.0, 10.0);
                    if hover_rect.contains(self.mouse_pos)
                        && (self.holding_click_rect.is_none()
                            || self
                                .holding_click_rect
                                .unwrap()
                                .contains(self.mouse_pos))
                    {
                        ctx.fill(hover_rect, &color);
                        ctx.stroke(
                            Line::new(
                                Point::new(hover_rect.x0, hover_rect.y1),
                                Point::new(hover_rect.x1, hover_rect.y1),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                            1.0,
                        );
                    }

                    ctx.draw_svg(
                        &svg,
                        rect,
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)]
#[cfg(not(target_os = "macos"))]
pub fn window_controls(
    window_id: druid::WindowId,
    window_state: &WindowState,
    x: f64,
    width: f64,
    config: &LapceConfig,
) -> (
    Vec<(Rect, Command)>,
    Vec<(druid::piet::Svg, Rect, druid::Color)>,
) {
    use druid::Color;
    use lapce_data::config::LapceIcons;

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
        Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::CloseWindow(window_id),
            Target::Auto,
        ),
    ));

    let hover_color = config
        .get_hover_color(config.get_color_unchecked(LapceTheme::PANEL_BACKGROUND));

    let mut svgs = Vec::new();

    let minimize_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x, 0.0))
        .inflate(-10.0, -10.0);
    svgs.push((
        config.ui_svg(LapceIcons::WINDOW_MINIMIZE),
        minimize_rect,
        hover_color.clone(),
    ));

    let max_res_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x + width, 0.0))
        .inflate(-10.0, -10.0);
    let max_res_svg = if window_state == &WindowState::Restored {
        config.ui_svg(LapceIcons::WINDOW_MAXIMIZE)
    } else {
        config.ui_svg(LapceIcons::WINDOW_RESTORE)
    };
    svgs.push((max_res_svg, max_res_rect, hover_color));

    let close_rect = Size::new(width, width)
        .to_rect()
        .with_origin(Point::new(x + 2.0 * width, 0.0))
        .inflate(-10.0, -10.0);
    svgs.push((
        config.ui_svg(LapceIcons::WINDOW_CLOSE),
        close_rect,
        Color::rgb8(210, 16, 33),
    ));

    (commands, svgs)
}

// TODO(dbuga): find a better place for these
const TAB_HEADER_HEIGHT: f64 = 36.0;
const TAB_HEADER_PADDING: f64 = 0.0;
const RIGHT_PADDING: f64 = 100.0;
const WINDOW_CONTROL_BUTTON_WIDTH: f64 = 36.0;
