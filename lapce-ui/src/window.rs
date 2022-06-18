use druid::{
    kurbo::Line,
    widget::{LensWrap, WidgetExt},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target, Widget, WidgetId,
    WidgetPod, WindowState,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{LapceTabData, LapceTabLens, LapceWindowData, LapceWorkspace},
};
use std::cmp::Ordering;
use std::sync::Arc;

use crate::{
    tab::{LapceTab, LapceTabHeader},
    title::Title,
};

pub struct LapceWindow {
    pub title: WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>,
    pub tabs: Vec<WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>>,
    tab_headers: Vec<
        WidgetPod<
            LapceWindowData,
            LensWrap<LapceWindowData, LapceTabData, LapceTabLens, LapceTabHeader>,
        >,
    >,
}

impl LapceWindow {
    pub fn new(data: &LapceWindowData) -> Self {
        let title = WidgetPod::new(Title::new().boxed());
        let tabs = data
            .tabs_order
            .iter()
            .map(|tab_id| {
                let data = data.tabs.get(tab_id).unwrap();
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
            title,
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
        let tab_data = LapceTabData::new(
            data.window_id,
            tab_id,
            workspace,
            data.db.clone(),
            data.keypress.clone(),
            ctx.get_external_handle(),
        );
        let tab = LapceTab::new(&tab_data).lens(LapceTabLens(tab_id));
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
        self.title.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceWindowData,
        env: &Env,
    ) {
        self.title.lifecycle(ctx, event, data, env);
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
        self.title.update(ctx, data, env);

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

        let title_size = self.title.layout(ctx, bc, data, env);
        self.title.set_origin(ctx, data, env, Point::ZERO);

        let (tab_size, tab_origin) = if self.tabs.len() > 1 {
            let tab_height = 25.0;
            let tab_size = Size::new(
                self_size.width,
                self_size.height - tab_height - title_size.height,
            );
            let tab_origin = Point::new(0.0, tab_height + title_size.height);

            let num = self.tabs.len();
            let section = self_size.width / num as f64;

            let mut drag = None;
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let bc = BoxConstraints::tight(Size::new(section, tab_height));
                tab_header.layout(ctx, &bc, data, env);
                let mut origin = Point::new(section * i as f64, title_size.height);
                let header = tab_header.widget().child();
                if let Some(o) = header.origin() {
                    origin = Point::new(o.x, title_size.height);
                    drag = Some((i, header.mouse_pos));
                }
                tab_header.set_origin(ctx, data, env, origin);
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
                tab_header.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(0.0, title_size.height),
                );
            }
            let tab_size =
                Size::new(self_size.width, self_size.height - title_size.height);
            (tab_size, Point::new(0.0, title_size.height))
        };

        let bc = BoxConstraints::tight(tab_size);
        self.tabs[data.active].layout(ctx, &bc, data, env);
        self.tabs[data.active].set_origin(ctx, data, env, tab_origin);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceWindowData, env: &Env) {
        let _start = std::time::SystemTime::now();

        let title_height = self.title.layout_rect().height();

        let tab_height = 25.0;
        let size = ctx.size();
        if self.tabs.len() > 1 {
            ctx.fill(
                Size::new(size.width, tab_height)
                    .to_rect()
                    .with_origin(Point::new(0.0, title_height)),
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );
            for tab_header in self.tab_headers.iter_mut() {
                tab_header.paint(ctx, data, env);
            }
        }

        ctx.with_save(|ctx| {
            ctx.clip(self.tabs[data.active].layout_rect());
            self.tabs[data.active].paint(ctx, data, env);
        });

        let line_color = data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER);
        if self.tabs.len() > 1 {
            let num = self.tabs.len();
            let section = ctx.size().width / num as f64;
            for i in 1..num {
                let line = Line::new(
                    Point::new(i as f64 * section, title_height),
                    Point::new(i as f64 * section, title_height + tab_height),
                );
                ctx.stroke(line, line_color, 1.0);
            }

            let rect = self.tab_headers[data.active].layout_rect();
            let clip_rect = Size::new(size.width, tab_height)
                .to_rect()
                .with_origin(Point::new(0.0, title_height));
            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                let shadow_width = data.config.ui.drop_shadow_width() as f64;
                if shadow_width > 0.0 {
                    ctx.blurred_rect(
                        rect,
                        shadow_width,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                }
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                );
            });
            self.tab_headers[data.active].paint(ctx, data, env);

            let line = Line::new(
                Point::new(0.0, title_height + tab_height - 0.5),
                Point::new(size.width, title_height + tab_height - 0.5),
            );
            ctx.stroke(line, line_color, 1.0);
        }

        self.title.paint(ctx, data, env);
        let line = Line::new(
            Point::new(0.0, title_height - 0.5),
            Point::new(size.width, title_height - 0.5),
        );
        ctx.stroke(line, line_color, 1.0);
    }
}
