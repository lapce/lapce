use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    container::LapceContainer,
    editor::EditorUIState,
    explorer::FileExplorer,
    split::LapceSplit,
    state::{
        LapceTabState, LapceUIState, LapceWindowState, LapceWorkspaceType,
        LAPCE_APP_STATE,
    },
    status::LapceStatus,
    theme::LapceTheme,
};
use druid::{
    kurbo::Line, theme, widget::IdentityWrapper, widget::WidgetExt, BoxConstraints,
    Env, Event, EventCtx, FontDescriptor, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, TextLayout, UpdateCtx,
    Widget, WidgetId, WidgetPod, WindowId,
};
use std::sync::Arc;

pub struct LapceTab {
    window_id: WindowId,
    tab_id: WidgetId,
    main_split: WidgetPod<LapceUIState, LapceSplit>,
    status: WidgetPod<LapceUIState, LapceStatus>,
}

impl LapceTab {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> LapceTab {
        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let file_explorer_widget_id = {
            let file_explorer = state.file_explorer.lock();
            file_explorer.widget_id
        };
        let container_id = WidgetId::next();
        let container = LapceContainer::new(window_id.clone(), tab_id.clone())
            .with_id(container_id.clone());
        let main_split = LapceSplit::new(window_id, tab_id, true)
            .with_child(
                FileExplorer::new(window_id.clone(), tab_id.clone())
                    .with_id(file_explorer_widget_id),
                300.0,
            )
            .with_flex_child(container, 1.0);
        let status = LapceStatus::new(window_id.clone(), tab_id.clone());
        LapceTab {
            window_id,
            tab_id,
            main_split: WidgetPod::new(main_split),
            status: WidgetPod::new(status),
        }
    }
}

impl Drop for LapceTab {
    fn drop(&mut self) {
        println!("now drop tab");
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        state.stop();
        LAPCE_APP_STATE
            .get_window_state(&self.window_id)
            .states
            .lock()
            .remove(&self.tab_id);
    }
}

impl Widget<LapceUIState> for LapceTab {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut LapceUIState,
        env: &druid::Env,
    ) {
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(druid::commands::OPEN_FILE) => {
                    let command = cmd.get_unchecked(druid::commands::OPEN_FILE);
                    let state =
                        LAPCE_APP_STATE.get_active_tab_state(&self.window_id);
                    state.open(ctx, data, command.path());
                    println!("got open file command {:?}", command);
                }
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        LapceUICommand::UpdateLineChanges(buffer_id) => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
                            let buffer =
                                editor_split.buffers.get_mut(buffer_id).unwrap();
                            let buffer_ui_state = data.get_buffer_mut(buffer_id);
                            buffer_ui_state.line_changes =
                                buffer.line_changes.clone();
                            ctx.set_handled();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
        self.main_split.event(ctx, event, data, env);
        self.status.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        self.main_split.lifecycle(ctx, event, data, env);
        self.status.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        self.main_split.update(ctx, data, env);
        self.status.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceUIState,
        env: &druid::Env,
    ) -> druid::Size {
        let self_size = bc.max();
        let status_size = self.status.layout(ctx, bc, data, env);
        let main_split_size =
            Size::new(self_size.width, self_size.height - status_size.height);
        let main_split_bc = BoxConstraints::new(Size::ZERO, main_split_size);
        self.main_split.layout(ctx, &main_split_bc, data, env);
        self.main_split
            .set_layout_rect(ctx, data, env, main_split_size.to_rect());
        self.status.set_layout_rect(
            ctx,
            data,
            env,
            Rect::from_origin_size(
                Point::new(0.0, main_split_size.height),
                status_size,
            ),
        );
        self_size
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
        self.main_split.paint(ctx, data, env);
        self.status.paint(ctx, data, env);
    }

    fn id(&self) -> Option<WidgetId> {
        Some(self.tab_id)
    }
}

pub struct LapceWindow {
    window_id: WindowId,
    pub tabs: Vec<WidgetPod<LapceUIState, LapceTab>>,
}

impl LapceWindow {
    pub fn new(window_id: WindowId) -> LapceWindow {
        let tab_id = LAPCE_APP_STATE
            .states
            .lock()
            .get(&window_id)
            .unwrap()
            .active
            .lock()
            .clone();
        let window = LapceTab::new(window_id.clone(), tab_id);
        LapceWindow {
            window_id,
            tabs: vec![WidgetPod::new(window)],
        }
    }
}

impl Widget<LapceUIState> for LapceWindow {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        ctx.request_focus();
        match event {
            Event::KeyDown(key_event) => {
                LAPCE_APP_STATE
                    .get_active_tab_state(&self.window_id)
                    .keypress
                    .lock()
                    .key_down(ctx, data, key_event, env);
                ctx.set_handled();
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::CloseBuffers(buffer_ids) => {
                            for buffer_id in buffer_ids {
                                Arc::make_mut(&mut data.buffers).remove(buffer_id);
                            }
                        }
                        LapceUICommand::NewTab => {
                            let state = LapceTabState::new(self.window_id.clone());
                            for (_, editor) in
                                state.editor_split.lock().editors.iter()
                            {
                                let editor_ui_state = EditorUIState::new();
                                Arc::make_mut(&mut data.editors)
                                    .insert(editor.view_id, editor_ui_state);
                            }
                            let tab_id = state.tab_id.clone();
                            let window_state =
                                LAPCE_APP_STATE.get_window_state(&self.window_id);
                            window_state.states.lock().insert(tab_id, state.clone());
                            let active = { window_state.active.lock().clone() };
                            let mut index = 0;
                            for (i, window) in self.tabs.iter_mut().enumerate() {
                                if window.id() == active {
                                    index = i;
                                }
                            }
                            let tab = LapceTab::new(self.window_id.clone(), tab_id);
                            self.tabs.insert(index + 1, WidgetPod::new(tab));
                            *window_state.active.lock() = tab_id;
                            ctx.request_layout();
                        }
                        LapceUICommand::CloseTab => {
                            if self.tabs.len() <= 1 {
                                return;
                            }
                            let window_state =
                                LAPCE_APP_STATE.get_window_state(&self.window_id);
                            let mut active = window_state.active.lock();
                            let mut index = 0;
                            for (i, window) in self.tabs.iter_mut().enumerate() {
                                if window.id() == *active {
                                    index = i;
                                }
                            }
                            let new_index = if index >= self.tabs.len() - 1 {
                                index - 1
                            } else {
                                index + 1
                            };
                            let new_active = self.tabs[new_index].widget().tab_id;
                            self.tabs.remove(index);
                            //window_state.states.lock().remove(&active);
                            *active = new_active;
                            ctx.request_layout();
                        }
                        LapceUICommand::NextTab => {
                            let window_state =
                                LAPCE_APP_STATE.get_window_state(&self.window_id);
                            let mut active = window_state.active.lock();
                            let mut index = 0;
                            for (i, window) in self.tabs.iter_mut().enumerate() {
                                if window.id() == *active {
                                    index = i;
                                }
                            }
                            let new_index = if index >= self.tabs.len() - 1 {
                                0
                            } else {
                                index + 1
                            };
                            *active = self.tabs[new_index].id();
                            ctx.request_layout();
                        }
                        LapceUICommand::PreviousTab => {
                            let window_state =
                                LAPCE_APP_STATE.get_window_state(&self.window_id);
                            let mut active = window_state.active.lock();
                            let mut index = 0;
                            for (i, window) in self.tabs.iter_mut().enumerate() {
                                if window.id() == *active {
                                    index = i;
                                }
                            }
                            let new_index = if index == 0 {
                                self.tabs.len() - 1
                            } else {
                                index - 1
                            };
                            *active = self.tabs[new_index].id();
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
        for tab in self.tabs.iter_mut() {
            tab.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        for tab in self.tabs.iter_mut() {
            tab.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        let active = LAPCE_APP_STATE
            .get_window_state(&self.window_id)
            .active
            .lock()
            .clone();
        for tab in self.tabs.iter_mut() {
            if tab.widget().tab_id == active {
                tab.update(ctx, data, env);
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let (window_size, window_rect) = if self.tabs.len() > 1 {
            let tab_height = 25.0;
            let window_size =
                Size::new(self_size.width, self_size.height - tab_height);
            let window_rect = Rect::ZERO
                .with_origin(Point::new(0.0, tab_height))
                .with_size(window_size);
            (window_size, window_rect)
        } else {
            (self_size.clone(), self_size.to_rect())
        };
        let active = LAPCE_APP_STATE
            .get_window_state(&self.window_id)
            .active
            .lock()
            .clone();
        for window in self.tabs.iter_mut() {
            if window.widget().tab_id == active {
                window.layout(
                    ctx,
                    &BoxConstraints::new(Size::ZERO, window_size),
                    data,
                    env,
                );
                window.set_layout_rect(ctx, data, env, window_rect);
            }
        }
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            ctx.fill(rect, &env.get(LapceTheme::EDITOR_BACKGROUND));
        }
        let size = ctx.size();
        let tab_height = 25.0;
        let active = {
            LAPCE_APP_STATE
                .get_window_state(&self.window_id)
                .active
                .lock()
                .clone()
        };
        if self.tabs.len() > 1 {
            ctx.fill(
                Size::new(size.width, tab_height).to_rect(),
                &env.get(LapceTheme::EDITOR_SELECTION_COLOR),
            );
            let color = env.get(theme::BORDER_LIGHT);
            let num = self.tabs.len();
            let section = size.width / num as f64;
            for (i, tab) in self.tabs.iter().enumerate() {
                let tab_id = tab.id();
                if tab_id == active {
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(section * i as f64, 0.0))
                            .with_size(Size::new(section, tab_height)),
                        &env.get(LapceTheme::EDITOR_BACKGROUND),
                    );
                }
                let workspace = LAPCE_APP_STATE
                    .get_window_state(&self.window_id)
                    .states
                    .lock()
                    .get(&tab_id)
                    .unwrap()
                    .workspace
                    .lock()
                    .clone();
                let dir = workspace.path.file_name().unwrap().to_str().unwrap();
                let dir = match &workspace.kind {
                    LapceWorkspaceType::Local => dir.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("{} [{}@{}]", dir, user, host)
                    }
                };
                let mut text_layout = TextLayout::<String>::from_text(dir);
                text_layout.set_font(
                    FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0),
                );
                text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
                text_layout.rebuild_if_needed(ctx.text(), env);
                let text_width = text_layout.size().width;
                let x = (section - text_width) / 2.0 + section * i as f64;
                text_layout.draw(ctx, Point::new(x, 3.0));
            }
            for i in 1..num {
                let line = Line::new(
                    Point::new(i as f64 * section, 0.0),
                    Point::new(i as f64 * section, tab_height),
                );
                ctx.stroke(line, &color, 1.0);
            }
            ctx.fill(
                Rect::ZERO
                    .with_origin(Point::new(0.0, tab_height - 1.0))
                    .with_size(Size::new(size.width, 1.0)),
                &color,
            );
        }
        for window in self.tabs.iter_mut() {
            if window.id() == active {
                window.paint(ctx, data, env);
            }
        }
    }
}
