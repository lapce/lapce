use std::sync::Arc;

use floem::{
    glazier::KeyEvent,
    peniko::kurbo::{Point, Size},
    reactive::{
        create_rw_signal, use_context, ReadSignal, RwSignal, Scope,
        SignalGetUntracked, SignalSet, SignalUpdate, SignalWithUntracked,
    },
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
    pub scope: Scope,
    /// The set of tabs within the window. These tabs are high-level
    /// constructs for workspaces, in particular they are not **editor tabs**.
    pub window_tabs: RwSignal<im::Vector<(RwSignal<usize>, Arc<WindowTabData>)>>,
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
        cx: Scope,
        info: WindowInfo,
        window_scale: RwSignal<f64>,
        latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
        app_command: Listener<AppCommand>,
    ) -> Self {
        let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
        let config = create_rw_signal(cx, Arc::new(config));
        let root_view_id = create_rw_signal(cx, floem::id::Id::next());

        let mut window_tabs = im::Vector::new();
        let active = info.tabs.active_tab;

        let window_command = Listener::new_empty(cx);

        for w in info.tabs.workspaces {
            let window_tab = Arc::new(WindowTabData::new(
                cx,
                Arc::new(w),
                window_command,
                window_scale,
                latest_release,
            ));
            window_tabs.push_back((create_rw_signal(cx, 0), window_tab));
        }

        if window_tabs.is_empty() {
            let window_tab = Arc::new(WindowTabData::new(
                cx,
                Arc::new(LapceWorkspace::default()),
                window_command,
                window_scale,
                latest_release,
            ));
            window_tabs.push_back((create_rw_signal(cx, 0), window_tab));
        }

        let window_tabs = create_rw_signal(cx, window_tabs);
        let active = create_rw_signal(cx, active);
        let size = create_rw_signal(cx, Size::ZERO);
        let position = create_rw_signal(cx, info.pos);

        let window_data = Self {
            scope: cx,
            window_tabs,
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
                let db: Arc<LapceDb> = use_context(self.scope).unwrap();
                let _ = db.update_recent_workspace(&workspace);

                let active = self.active.get_untracked();
                self.window_tabs.with_untracked(|window_tabs| {
                    if !window_tabs.is_empty() {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        let _ = db.insert_window_tab(window_tabs[active].1.clone());
                    }
                });

                let window_tab = Arc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.window_command,
                    self.window_scale,
                    self.latest_release,
                ));
                self.window_tabs.update(|window_tabs| {
                    if window_tabs.is_empty() {
                        window_tabs.push_back((
                            create_rw_signal(self.scope, 0),
                            window_tab,
                        ));
                    } else {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        let (_, old_window_tab) = window_tabs.set(
                            active,
                            (create_rw_signal(self.scope, 0), window_tab),
                        );
                        old_window_tab.proxy.shutdown();
                    }
                })
            }
            WindowCommand::NewWorkspaceTab { workspace, end } => {
                let db: Arc<LapceDb> = use_context(self.scope).unwrap();
                let _ = db.update_recent_workspace(&workspace);

                let window_tab = Arc::new(WindowTabData::new(
                    self.scope,
                    Arc::new(workspace),
                    self.window_command,
                    self.window_scale,
                    self.latest_release,
                ));
                let active = self.active.get_untracked();
                let active = self
                    .window_tabs
                    .try_update(|tabs| {
                        if end || tabs.is_empty() {
                            tabs.push_back((
                                create_rw_signal(self.scope, 0),
                                window_tab,
                            ));
                            tabs.len() - 1
                        } else {
                            let index = tabs.len().min(active + 1);
                            tabs.insert(
                                index,
                                (create_rw_signal(self.scope, 0), window_tab),
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
                        let db: Arc<LapceDb> = use_context(self.scope).unwrap();
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

    pub fn active_window_tab(&self) -> Option<Arc<WindowTabData>> {
        let window_tabs = self.window_tabs.get_untracked();
        let active = self
            .active
            .get_untracked()
            .min(window_tabs.len().saturating_sub(1));
        window_tabs.get(active).map(|(_, tab)| tab.clone())
    }
}
