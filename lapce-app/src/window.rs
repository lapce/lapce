use std::{path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    ViewId,
    action::TimerToken,
    peniko::kurbo::{Point, Size},
    reactive::{
        Memo, ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith,
        use_context,
    },
    window::WindowId,
};
use serde::{Deserialize, Serialize};

use crate::{
    app::AppCommand,
    command::{InternalCommand, WindowCommand},
    config::LapceConfig,
    db::LapceDb,
    keypress::EventRef,
    listener::Listener,
    update::ReleaseInfo,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabsInfo {
    pub active_tab: usize,
    pub workspaces: Vec<LapceWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub size: Size,
    pub pos: Point,
    pub maximised: bool,
    pub tabs: TabsInfo,
}

#[derive(Clone)]
pub struct WindowCommonData {
    pub window_command: Listener<WindowCommand>,
    pub window_scale: RwSignal<f64>,
    pub size: RwSignal<Size>,
    pub num_window_tabs: Memo<usize>,
    pub window_maximized: RwSignal<bool>,
    pub window_tab_header_height: RwSignal<f64>,
    pub latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    pub ime_allowed: RwSignal<bool>,
    pub cursor_blink_timer: RwSignal<TimerToken>,
    // the value to be update by curosr blinking
    pub hide_cursor: RwSignal<bool>,
    pub app_view_id: RwSignal<ViewId>,
    pub extra_plugin_paths: Arc<Vec<PathBuf>>,
}

/// `WindowData` is the application model for a top-level window.
///
/// A top-level window can be independently moved around and
/// resized using your window manager. Normally Lapce has only one
/// top-level window, but new ones can be created using the "New Window"
/// command.
///
/// Each window has its own collection of "window tabs" (again, there is
/// normally only one window tab), size, position etc.
#[derive(Clone)]
pub struct WindowData {
    pub window_id: WindowId,
    pub scope: Scope,
    /// The set of tabs within the window. These tabs are high-level
    /// constructs for workspaces, in particular they are not **editor tabs**.
    pub window_tabs: RwSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    pub num_window_tabs: Memo<usize>,
    /// The index of the active window tab.
    pub active: RwSignal<usize>,
    pub app_command: Listener<AppCommand>,
    pub position: RwSignal<Point>,
    pub root_view_id: RwSignal<ViewId>,
    pub window_scale: RwSignal<f64>,
    pub config: RwSignal<Arc<LapceConfig>>,
    pub ime_enabled: RwSignal<bool>,
    pub common: Rc<WindowCommonData>,
}

impl WindowData {
    pub fn new(
        window_id: WindowId,
        app_view_id: RwSignal<ViewId>,
        info: WindowInfo,
        window_scale: RwSignal<f64>,
        latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
        extra_plugin_paths: Arc<Vec<PathBuf>>,
        app_command: Listener<AppCommand>,
    ) -> Self {
        let cx = Scope::new();
        let config =
            LapceConfig::load(&LapceWorkspace::default(), &[], &extra_plugin_paths);
        let config = cx.create_rw_signal(Arc::new(config));
        let root_view_id = cx.create_rw_signal(ViewId::new());

        let window_tabs = cx.create_rw_signal(im::Vector::new());
        let num_window_tabs =
            cx.create_memo(move |_| window_tabs.with(|tabs| tabs.len()));
        let active = info.tabs.active_tab;
        let window_command = Listener::new_empty(cx);
        let ime_allowed = cx.create_rw_signal(false);
        let window_maximized = cx.create_rw_signal(false);
        let size = cx.create_rw_signal(Size::ZERO);
        let window_tab_header_height = cx.create_rw_signal(0.0);
        let cursor_blink_timer = cx.create_rw_signal(TimerToken::INVALID);
        let hide_cursor = cx.create_rw_signal(false);

        let common = Rc::new(WindowCommonData {
            window_command,
            window_scale,
            size,
            num_window_tabs,
            window_maximized,
            window_tab_header_height,
            latest_release,
            ime_allowed,
            cursor_blink_timer,
            hide_cursor,
            app_view_id,
            extra_plugin_paths,
        });

        for w in info.tabs.workspaces {
            let window_tab =
                Rc::new(WindowTabData::new(cx, Arc::new(w), common.clone()));
            window_tabs.update(|window_tabs| {
                window_tabs.push_back((cx.create_rw_signal(0), window_tab));
            });
        }

        if window_tabs.with_untracked(|window_tabs| window_tabs.is_empty()) {
            let window_tab = Rc::new(WindowTabData::new(
                cx,
                Arc::new(LapceWorkspace::default()),
                common.clone(),
            ));
            window_tabs.update(|window_tabs| {
                window_tabs.push_back((cx.create_rw_signal(0), window_tab));
            });
        }

        let active = cx.create_rw_signal(active);
        let position = cx.create_rw_signal(info.pos);

        let window_data = Self {
            window_id,
            scope: cx,
            window_tabs,
            num_window_tabs,
            active,
            position,
            root_view_id,
            window_scale,
            app_command,
            config,
            ime_enabled: cx.create_rw_signal(false),
            common,
        };

        {
            let window_data = window_data.clone();
            window_data.common.window_command.listen(move |cmd| {
                window_data.run_window_command(cmd);
            });
        }

        {
            cx.create_effect(move |_| {
                let active = active.get();
                let tab = window_tabs
                    .with(|tabs| tabs.get(active).map(|(_, tab)| tab.clone()));
                if let Some(tab) = tab {
                    tab.common
                        .internal_command
                        .send(InternalCommand::ResetBlinkCursor);
                }
            })
        }

        window_data
    }

    pub fn reload_config(&self) {
        let config = LapceConfig::load(
            &LapceWorkspace::default(),
            &[],
            &self.common.extra_plugin_paths,
        );
        self.config.set(Arc::new(config));
        let window_tabs = self.window_tabs.get_untracked();
        for (_, window_tab) in window_tabs {
            window_tab.reload_config();
        }
    }

    pub fn run_window_command(&self, cmd: WindowCommand) {
        match cmd {
            WindowCommand::SetWorkspace { workspace } => {
                let db: Arc<LapceDb> = use_context().unwrap();
                if let Err(err) = db.update_recent_workspace(&workspace) {
                    tracing::error!("{:?}", err);
                }

                let active = self.active.get_untracked();
                self.window_tabs.with_untracked(|window_tabs| {
                    if !window_tabs.is_empty() {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        if let Err(err) =
                            db.insert_window_tab(window_tabs[active].1.clone())
                        {
                            tracing::error!("{:?}", err);
                        }
                    }
                });

                let window_tab = Rc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.common.clone(),
                ));
                self.window_tabs.update(|window_tabs| {
                    if window_tabs.is_empty() {
                        window_tabs
                            .push_back((self.scope.create_rw_signal(0), window_tab));
                    } else {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        let (_, old_window_tab) = window_tabs.set(
                            active,
                            (self.scope.create_rw_signal(0), window_tab),
                        );
                        old_window_tab.proxy.shutdown();
                    }
                })
            }
            WindowCommand::NewWorkspaceTab { workspace, end } => {
                let db: Arc<LapceDb> = use_context().unwrap();
                if let Err(err) = db.update_recent_workspace(&workspace) {
                    tracing::error!("{:?}", err);
                }

                let window_tab = Rc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.common.clone(),
                ));
                let active = self.active.get_untracked();
                let active = self
                    .window_tabs
                    .try_update(|tabs| {
                        if end || tabs.is_empty() {
                            tabs.push_back((
                                self.scope.create_rw_signal(0),
                                window_tab,
                            ));
                            tabs.len() - 1
                        } else {
                            let index = tabs.len().min(active + 1);
                            tabs.insert(
                                index,
                                (self.scope.create_rw_signal(0), window_tab),
                            );
                            index
                        }
                    })
                    .unwrap();
                self.active.set(active);
            }
            WindowCommand::CloseWorkspaceTab { index } => {
                let active = self.active.get_untracked();
                let index = index.unwrap_or(active);
                self.window_tabs.update(|window_tabs| {
                    if window_tabs.len() < 2 {
                        return;
                    }

                    if index < window_tabs.len() {
                        let (_, old_window_tab) = window_tabs.remove(index);
                        old_window_tab.proxy.shutdown();
                        let db: Arc<LapceDb> = use_context().unwrap();
                        if let Err(err) = db.save_window_tab(old_window_tab) {
                            tracing::error!("{:?}", err);
                        }
                    }
                });

                let tabs_len = self.window_tabs.with_untracked(|tabs| tabs.len());

                if active > index && active > 0 {
                    self.active.set(active - 1);
                } else if active >= tabs_len.saturating_sub(1) {
                    self.active.set(tabs_len.saturating_sub(1));
                }
            }
            WindowCommand::NextWorkspaceTab => {
                let active = self.active.get_untracked();
                let tabs_len = self.window_tabs.with_untracked(|tabs| tabs.len());
                if tabs_len > 1 {
                    let active = if active >= tabs_len - 1 {
                        0
                    } else {
                        active + 1
                    };
                    self.active.set(active);
                }
            }
            WindowCommand::PreviousWorkspaceTab => {
                let active = self.active.get_untracked();
                let tabs_len = self.window_tabs.with_untracked(|tabs| tabs.len());
                if tabs_len > 1 {
                    let active = if active == 0 {
                        tabs_len - 1
                    } else {
                        active - 1
                    };
                    self.active.set(active);
                }
            }
            WindowCommand::NewWindow => {
                self.app_command
                    .send(AppCommand::NewWindow { folder: None });
            }
            WindowCommand::CloseWindow => {
                self.app_command
                    .send(AppCommand::CloseWindow(self.window_id));
            }
        }
        self.app_command.send(AppCommand::SaveApp);
    }

    pub fn key_down<'a>(&self, event: impl Into<EventRef<'a>> + Copy) -> bool {
        let active = self.active.get_untracked();
        let window_tab = self.window_tabs.with_untracked(|window_tabs| {
            window_tabs
                .get(active)
                .or_else(|| window_tabs.last())
                .cloned()
        });
        if let Some((_, window_tab)) = window_tab {
            window_tab.key_down(event)
        } else {
            false
        }
    }

    pub fn info(&self) -> WindowInfo {
        let workspaces: Vec<LapceWorkspace> = self
            .window_tabs
            .get_untracked()
            .iter()
            .map(|(_, t)| (*t.workspace).clone())
            .collect();
        WindowInfo {
            size: self.common.size.get_untracked(),
            pos: self.position.get_untracked(),
            maximised: false,
            tabs: TabsInfo {
                active_tab: self.active.get_untracked(),
                workspaces,
            },
        }
    }

    pub fn active_window_tab(&self) -> Option<Rc<WindowTabData>> {
        let window_tabs = self.window_tabs.get_untracked();
        let active = self
            .active
            .get_untracked()
            .min(window_tabs.len().saturating_sub(1));
        window_tabs.get(active).map(|(_, tab)| tab.clone())
    }

    pub fn move_tab(&self, from_index: usize, to_index: usize) {
        if from_index == to_index {
            return;
        }

        let to_index = if from_index < to_index {
            to_index - 1
        } else {
            to_index
        };
        self.window_tabs.update(|tabs| {
            let tab = tabs.remove(from_index);
            tabs.insert(to_index, tab);
        });
        self.active.set(to_index);
    }
}
