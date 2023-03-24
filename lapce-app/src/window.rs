use std::sync::Arc;

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    peniko::kurbo::{Point, Size},
    reactive::{
        create_effect, create_rw_signal, use_context, RwSignal, SignalGet,
        SignalGetUntracked, SignalUpdate, SignalWithUntracked,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    command::WindowCommand, db::LapceDb, window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct TabsInfo {
    pub active_tab: usize,
    pub workspaces: Vec<LapceWorkspace>,
}

#[derive(Clone, Serialize, Deserialize)]
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
    /// The set of tabs within the window. These tabs are high-level
    /// constructs for workspaces, in particular they are not **editor tabs**.
    pub window_tabs: RwSignal<Vec<Arc<WindowTabData>>>,
    /// The index of the active window tab.
    pub active: RwSignal<usize>,
    pub window_command: RwSignal<Option<WindowCommand>>,
    pub size: RwSignal<Size>,
}

impl WindowData {
    pub fn new(cx: AppContext) -> Self {
        let db: Arc<LapceDb> = use_context(cx.scope).unwrap();

        let info = db.get_window().unwrap_or_else(|_| WindowInfo {
            size: Size::new(800.0, 600.0),
            pos: Point::ZERO,
            maximised: false,
            tabs: TabsInfo {
                active_tab: 0,
                workspaces: vec![LapceWorkspace::default()],
            },
        });

        let mut window_tabs = Vec::new();
        let active = info.tabs.active_tab;

        let window_command = create_rw_signal(cx.scope, None);

        for w in info.tabs.workspaces {
            let window_tab = Arc::new(WindowTabData::new(
                cx,
                Arc::new(w),
                window_command.write_only(),
            ));
            window_tabs.push(window_tab);
        }

        if window_tabs.is_empty() {
            let window_tab = Arc::new(WindowTabData::new(
                cx,
                Arc::new(LapceWorkspace::default()),
                window_command.write_only(),
            ));
            window_tabs.push(window_tab);
        }

        let window_tabs = create_rw_signal(cx.scope, window_tabs);
        let active = create_rw_signal(cx.scope, active);
        let size = create_rw_signal(cx.scope, Size::ZERO);

        let window_data = Self {
            window_tabs,
            active,
            window_command,
            size,
        };

        {
            let window_data = window_data.clone();
            let window_command = window_data.window_command.read_only();
            create_effect(cx.scope, move |_| {
                if let Some(cmd) = window_command.get() {
                    window_data.run_window_command(cx, cmd);
                }
            });
        }

        window_data
    }

    pub fn run_window_command(&self, cx: AppContext, cmd: WindowCommand) {
        match cmd {
            WindowCommand::SetWorkspace { workspace } => {
                let window_tab = Arc::new(WindowTabData::new(
                    cx,
                    Arc::new(workspace),
                    self.window_command.write_only(),
                ));
                let active = self.active.get_untracked();
                self.window_tabs.update(|window_tabs| {
                    if window_tabs.is_empty() {
                        window_tabs.push(window_tab);
                    } else {
                        let active = window_tabs.len().saturating_sub(1).min(active);
                        window_tabs[active] = window_tab;
                    }
                })
            }
        }
    }

    pub fn key_down(&self, cx: AppContext, key_event: &KeyEvent) {
        let active = self.active.get_untracked();
        let window_tab = self.window_tabs.with_untracked(|window_tabs| {
            window_tabs
                .get(active)
                .or_else(|| window_tabs.last())
                .cloned()
        });
        if let Some(window_tab) = window_tab {
            window_tab.key_down(cx, key_event);
        }
    }

    pub fn info(&self) -> WindowInfo {
        let workspaces: Vec<LapceWorkspace> = self
            .window_tabs
            .get_untracked()
            .iter()
            .map(|t| (*t.workspace).clone())
            .collect();
        WindowInfo {
            size: self.size.get_untracked(),
            pos: Point::ZERO,
            maximised: false,
            tabs: TabsInfo {
                active_tab: self.active.get_untracked(),
                workspaces,
            },
        }
    }
}
