use std::{rc::Rc, sync::Arc};

use floem::{
    glazier::KeyEvent,
    id::WindowId,
    peniko::kurbo::{Point, Size},
    reactive::{use_context, Memo, ReadSignal, RwSignal, Scope},
};
use serde::{Deserialize, Serialize};

use crate::{
    app::AppCommand, command::WindowCommand, config::LapceConfig, db::LapceDb,
    listener::Listener, update::ReleaseInfo, window_tab::WindowTabData,
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
    pub window_command: Listener<WindowCommand>,
    pub app_command: Listener<AppCommand>,
    pub size: RwSignal<Size>,
    pub position: RwSignal<Point>,
    pub root_view_id: RwSignal<floem::id::Id>,
    pub window_scale: RwSignal<f64>,
    pub latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    pub config: RwSignal<Arc<LapceConfig>>,
}

impl WindowData {
    pub fn new(
        window_id: WindowId,
        info: WindowInfo,
        window_scale: RwSignal<f64>,
        latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
        app_command: Listener<AppCommand>,
    ) -> Self {
        let cx = Scope::new();
        let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
        let config = cx.create_rw_signal(Arc::new(config));
        let root_view_id = cx.create_rw_signal(floem::id::Id::next());

        let window_tabs = cx.create_rw_signal(im::Vector::new());
        let num_window_tabs =
            cx.create_memo(move |_| window_tabs.with(|tabs| tabs.len()));
        let active = info.tabs.active_tab;

        let window_command = Listener::new_empty(cx);

        for w in info.tabs.workspaces {
            let window_tab = Rc::new(WindowTabData::new(
                cx,
                Arc::new(w),
                window_command,
                window_scale,
                latest_release,
                num_window_tabs,
            ));
            window_tabs.update(|window_tabs| {
                window_tabs.push_back((cx.create_rw_signal(0), window_tab));
            });
        }

        if window_tabs.with_untracked(|window_tabs| window_tabs.is_empty()) {
            let window_tab = Rc::new(WindowTabData::new(
                cx,
                Arc::new(LapceWorkspace::default()),
                window_command,
                window_scale,
                latest_release,
                num_window_tabs,
            ));
            window_tabs.update(|window_tabs| {
                window_tabs.push_back((cx.create_rw_signal(0), window_tab));
            });
        }

        let active = cx.create_rw_signal(active);
        let size = cx.create_rw_signal(Size::ZERO);
        let position = cx.create_rw_signal(info.pos);

        let window_data = Self {
            window_id,
            scope: cx,
            window_tabs,
            num_window_tabs,
            active,
            window_command,
            size,
            position,
            root_view_id,
            window_scale,
            latest_release,
            app_command,
            config,
        };

        {
            let window_data = window_data.clone();
            window_data.window_command.listen(move |cmd| {
                window_data.run_window_command(cmd);
            });
        }

        window_data
    }

    pub fn reload_config(&self) {
        let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
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
                let _ = db.update_recent_workspace(&workspace);

                let active = self.active.get_untracked();
                self.window_tabs.with_untracked(|window_tabs| {
                    if !window_tabs.is_empty() {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        let _ = db.insert_window_tab(window_tabs[active].1.clone());
                    }
                });

                let window_tab = Rc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.window_command,
                    self.window_scale,
                    self.latest_release,
                    self.num_window_tabs,
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
                let _ = db.update_recent_workspace(&workspace);

                let window_tab = Rc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.window_command,
                    self.window_scale,
                    self.latest_release,
                    self.num_window_tabs,
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
                        let _ = db.save_window_tab(old_window_tab);
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
                self.app_command.send(AppCommand::NewWindow);
            }
            WindowCommand::CloseWindow => {
                self.app_command
                    .send(AppCommand::CloseWindow(self.window_id));
            }
        }
        self.app_command.send(AppCommand::SaveApp);
    }

    pub fn key_down(&self, key_event: &KeyEvent) {
        let active = self.active.get_untracked();
        let window_tab = self.window_tabs.with_untracked(|window_tabs| {
            window_tabs
                .get(active)
                .or_else(|| window_tabs.last())
                .cloned()
        });
        if let Some((_, window_tab)) = window_tab {
            window_tab.key_down(key_event);
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
            size: self.size.get_untracked(),
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
