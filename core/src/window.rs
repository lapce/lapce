use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    config::{Config, LapceTheme},
    data::{LapceTabData, LapceTabLens, LapceWindowData},
    editor::EditorUIState,
    explorer::{FileExplorer, FileExplorerState},
    panel::{LapcePanel, PanelPosition, PanelProperty},
    state::{LapceWorkspace, LapceWorkspaceType},
    tab::{LapceTabHeader, LapceTabNew},
    theme::OldLapceTheme,
};
use druid::{
    kurbo::Line,
    piet::{Text, TextLayout, TextLayoutBuilder},
    theme,
    widget::IdentityWrapper,
    widget::{LensWrap, WidgetExt},
    BoxConstraints, Command, Env, Event, EventCtx, FontDescriptor, FontFamily,
    LayoutCtx, Lens, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext,
    Size, Target, UpdateCtx, Widget, WidgetId, WidgetPod, WindowId,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Index, sync::Arc};

pub struct LapceWindowNew {
    pub tabs: Vec<WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>>,
    tab_headers: Vec<
        WidgetPod<
            LapceWindowData,
            LensWrap<LapceWindowData, LapceTabData, LapceTabLens, LapceTabHeader>,
        >,
    >,
}

impl LapceWindowNew {
    pub fn new(data: &LapceWindowData) -> Self {
        let tabs = data
            .tabs
            .iter()
            .map(|(tab_id, data)| {
                let tab = LapceTabNew::new(data);
                let tab = tab.lens(LapceTabLens(*tab_id));
                WidgetPod::new(tab.boxed())
            })
            .collect();
        let tab_headers = data
            .tabs
            .iter()
            .map(|(tab_id, data)| {
                let tab_header = LapceTabHeader::new().lens(LapceTabLens(*tab_id));
                WidgetPod::new(tab_header)
            })
            .collect();
        Self { tabs, tab_headers }
    }

    pub fn new_tab(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceWindowData,
        workspace: Option<LapceWorkspace>,
        replace_current: bool,
    ) {
        let tab_id = WidgetId::next();
        let mut tab_data = LapceTabData::new(
            tab_id,
            data.keypress.clone(),
            data.theme.clone(),
            Some(ctx.get_external_handle()),
        );
        tab_data.workspace = workspace.map(|w| Arc::new(w));
        let tab = LapceTabNew::new(&tab_data).lens(LapceTabLens(tab_id));
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
            data.active = data.active + 1;
            data.active_id = tab_id;
        }
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::FocusTab,
            Target::Auto,
        ));
        ctx.children_changed();
        ctx.set_handled();
        ctx.request_layout();
        return;
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
            tab.proxy.stop();
        }

        if data.active > index {
            data.active -= 1;
        } else if data.active == index {
            if data.active >= self.tabs.len() {
                data.active = self.tabs.len() - 1;
            }
            data.active_id = self.tabs[data.active].id();
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::FocusTab,
                Target::Auto,
            ));
        }

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

impl Widget<LapceWindowData> for LapceWindowNew {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceWindowData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::ReloadConfig => {
                        data.config =
                            Arc::new(Config::load(None).unwrap_or_default());
                        for (_, tab) in data.tabs.iter_mut() {
                            tab.config = Arc::new(
                                Config::load(
                                    tab.workspace.clone().map(|w| (*w).clone()),
                                )
                                .unwrap_or_default(),
                            );
                        }
                        Arc::make_mut(&mut data.keypress).update_keymaps();
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadWindow => {
                        let tab = data.tabs.get(&data.active_id).unwrap();
                        let workspace =
                            tab.workspace.as_ref().map(|w| (*w.clone()).clone());
                        self.new_tab(ctx, data, workspace, true);
                        return;
                    }
                    LapceUICommand::SetWorkspace(workspace) => {
                        let mut workspaces =
                            Config::recent_workspaces().unwrap_or(Vec::new());

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

                        self.new_tab(ctx, data, Some(workspace.clone()), true);
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
                        self.new_tab(ctx, data, None, false);
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
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::FocusTab,
                                        Target::Auto,
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
                        return;
                    }
                    LapceUICommand::NextTab => {
                        let new_index = if data.active >= self.tabs.len() - 1 {
                            0
                        } else {
                            data.active + 1
                        };
                        data.active = new_index;
                        data.active_id = self.tabs[new_index].id();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::FocusTab,
                            Target::Auto,
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
                        data.active = new_index;
                        data.active_id = self.tabs[new_index].id();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::FocusTab,
                            Target::Auto,
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
        for tab_header in self.tab_headers.iter_mut() {
            tab_header.event(ctx, event, data, env);
        }
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
        let start = std::time::SystemTime::now();

        if old_data.active != data.active {
            ctx.request_layout();
        }
        let old_tab = old_data.tabs.get(&old_data.active_id).unwrap();
        let tab = data.tabs.get(&data.active_id).unwrap();
        if old_tab.workspace != tab.workspace {
            ctx.request_layout();
        }
        self.tabs[data.active].update(ctx, data, env);

        // println!(
        //     "update took {}",
        //     std::time::SystemTime::now()
        //         .duration_since(start)
        //         .unwrap()
        //         .as_micros() as f64
        //         / 1000.0
        // );
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
            let tab_height = 25.0;
            let tab_size = Size::new(self_size.width, self_size.height - tab_height);
            let tab_origin = Point::new(0.0, tab_height);

            let num = self.tabs.len();
            let section = self_size.width / num as f64;

            let mut drag = None;
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let bc = BoxConstraints::tight(Size::new(section, tab_height));
                tab_header.layout(ctx, &bc, data, env);
                let mut origin = Point::new(section * i as f64, 0.0);
                let header = tab_header.widget().child();
                if let Some(o) = header.origin() {
                    origin = Point::new(o.x, 0.0);
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
            for (i, tab_header) in self.tab_headers.iter_mut().enumerate() {
                let bc = BoxConstraints::tight(Size::new(self_size.width, 0.0));
                tab_header.layout(ctx, &bc, data, env);
                tab_header.set_origin(ctx, data, env, Point::ZERO);
            }
            (self_size.clone(), Point::ZERO)
        };

        let start = std::time::SystemTime::now();
        let tab = &mut self.tabs[data.active];
        let bc = BoxConstraints::tight(tab_size);
        tab.layout(ctx, &bc, data, env);
        tab.set_origin(ctx, data, env, tab_origin);
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("layout took {}", duration as f64 / 1000.0);
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateWindowOrigin,
            Target::Auto,
        ));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceWindowData, env: &Env) {
        let start = std::time::SystemTime::now();

        let tab_height = 25.0;
        let size = ctx.size();
        if self.tabs.len() > 1 {
            ctx.fill(
                Size::new(size.width, tab_height).to_rect(),
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_INACTIVE_TAB),
            );
            for tab_header in self.tab_headers.iter_mut() {
                tab_header.paint(ctx, data, env);
            }
        }

        self.tabs[data.active].paint(ctx, data, env);

        let line_color = data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER);
        if self.tabs.len() > 1 {
            let num = self.tabs.len();
            let section = ctx.size().width / num as f64;
            for i in 1..num {
                let line = Line::new(
                    Point::new(i as f64 * section, 0.0),
                    Point::new(i as f64 * section, tab_height),
                );
                ctx.stroke(line, line_color, 1.0);
            }

            let rect = self.tab_headers[data.active].layout_rect();
            let clip_rect = Size::new(size.width, tab_height).to_rect();
            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                ctx.blurred_rect(
                    rect,
                    5.0,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ACTIVE_TAB),
                );
            });
            self.tab_headers[data.active].paint(ctx, data, env);

            let line = Line::new(
                Point::new(0.0, tab_height - 0.5),
                Point::new(size.width, tab_height - 0.5),
            );
            ctx.stroke(line, line_color, 1.0);
        }
    }
}
