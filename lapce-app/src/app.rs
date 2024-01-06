#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufReader, Read, Write},
    ops::Range,
    process::Stdio,
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
};

use anyhow::{anyhow, Result};
use clap::Parser;
use crossbeam_channel::Sender;
use floem::{
    cosmic_text::{Style as FontStyle, Weight},
    event::{Event, EventListener},
    ext_event::create_signal_from_channel,
    menu::{Menu, MenuItem},
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, provide_context, use_context,
        ReadSignal, RwSignal, Scope,
    },
    style::{
        AlignItems, CursorStyle, Display, FlexDirection, JustifyContent, Position,
        Style,
    },
    unit::PxPctAuto,
    view::View,
    views::{
        clip, container, container_box, drag_resize_window_area, drag_window_area,
        dyn_stack, empty, label, rich_text, scroll::scroll, stack, svg, tab, text,
        virtual_stack, Decorators, VirtualDirection, VirtualItemSize, VirtualVector,
    },
    window::{ResizeDirection, WindowConfig, WindowId},
    EventPropagation,
};
use lapce_core::{
    command::{EditCommand, FocusCommand},
    directory::Directory,
    meta,
};
use lapce_rpc::{
    core::{CoreMessage, CoreNotification},
    file::PathObject,
    RpcMessage,
};
use lsp_types::{CompletionItemKind, MessageType, ShowMessageParams};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use tracing::{error, metadata::LevelFilter, trace};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{filter::Targets, reload::Handle};

use crate::{
    about, alert,
    code_action::CodeActionStatus,
    command::{
        CommandKind, InternalCommand, LapceCommand, LapceWorkbenchCommand,
        WindowCommand,
    },
    config::{
        color::LapceColor, icon::LapceIcons, watcher::ConfigWatcher, LapceConfig,
    },
    db::LapceDb,
    debug::RunDebugMode,
    editor::{
        diff::diff_show_more_section_view,
        location::{EditorLocation, EditorPosition},
        view::editor_container_view,
    },
    editor_tab::{EditorTabChild, EditorTabData},
    focus_text::focus_text,
    id::{EditorTabId, SplitId},
    keymap::keymap_view,
    keypress::keymap::KeyMap,
    listener::Listener,
    main_split::{SplitContent, SplitData, SplitDirection, SplitMoveDirection},
    markdown::MarkdownContent,
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteStatus,
    },
    panel::{position::PanelContainerPosition, view::panel_container_view},
    plugin::{plugin_info_view, PluginData},
    settings::{settings_view, theme_color_settings_view},
    status::status,
    text_input::text_input,
    title::{title, window_controls_view},
    update::ReleaseInfo,
    window::{TabsInfo, WindowData, WindowInfo},
    window_tab::{Focus, WindowTabData},
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

#[derive(Parser)]
#[clap(name = "Lapce")]
#[clap(version=meta::VERSION)]
#[derive(Debug)]
struct Cli {
    /// Launch new window even if Lapce is already running
    #[clap(short, long, action)]
    new: bool,
    /// Don't return instantly when opened in a terminal
    #[clap(short, long, action)]
    wait: bool,

    /// Paths to file(s) and/or folder(s) to open.
    /// When path is a file (that exists or not),
    /// it accepts `path:line:column` syntax
    /// to specify line and column at which it should open the file
    #[clap(value_parser = lapce_proxy::cli::parse_file_line_column)]
    #[clap(value_hint = clap::ValueHint::AnyPath)]
    paths: Vec<PathObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub windows: Vec<WindowInfo>,
}

#[derive(Clone)]
pub enum AppCommand {
    SaveApp,
    NewWindow,
    CloseWindow(WindowId),
    WindowGotFocus(WindowId),
    WindowClosed(WindowId),
}

#[derive(Clone)]
pub struct AppData {
    pub windows: RwSignal<im::HashMap<WindowId, WindowData>>,
    pub active_window: RwSignal<WindowId>,
    pub window_scale: RwSignal<f64>,
    pub app_command: Listener<AppCommand>,
    pub app_terminated: RwSignal<bool>,
    /// The latest release information
    pub latest_release: RwSignal<Arc<Option<ReleaseInfo>>>,
    pub watcher: Arc<notify::RecommendedWatcher>,
    pub tracing_handle: Handle<Targets>,
    pub config: RwSignal<Arc<LapceConfig>>,
}

impl AppData {
    pub fn reload_config(&self) {
        let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
        self.config.set(Arc::new(config));
        let windows = self.windows.get_untracked();
        for (_, window) in windows {
            window.reload_config();
        }
    }

    pub fn active_window_tab(&self) -> Option<Rc<WindowTabData>> {
        if let Some(window) = self.active_window() {
            return window.active_window_tab();
        }
        None
    }

    fn active_window(&self) -> Option<WindowData> {
        let windows = self.windows.get_untracked();
        let active_window = self.active_window.get_untracked();
        windows
            .get(&active_window)
            .cloned()
            .or_else(|| windows.iter().next().map(|(_, window)| window.clone()))
    }

    fn default_window_config(&self) -> WindowConfig {
        WindowConfig::default().themed(false).title("Lapce")
    }

    pub fn new_window(&self) {
        let config = self
            .active_window()
            .map(|window| {
                self.default_window_config()
                    .size(window.common.size.get_untracked())
                    .position(window.position.get_untracked() + (50.0, 50.0))
            })
            .or_else(|| {
                let db: Arc<LapceDb> = use_context().unwrap();
                db.get_window().ok().map(|info| {
                    self.default_window_config()
                        .size(info.size)
                        .position(info.pos)
                })
            })
            .unwrap_or_else(|| {
                self.default_window_config().size(Size::new(800.0, 600.0))
            });
        let config = if cfg!(target_os = "macos")
            || self.config.get_untracked().core.custom_titlebar
        {
            config.show_titlebar(false)
        } else {
            config
        };
        let app_data = self.clone();
        floem::new_window(
            move |window_id| {
                app_data.app_view(
                    window_id,
                    WindowInfo {
                        size: Size::ZERO,
                        pos: Point::ZERO,
                        maximised: false,
                        tabs: TabsInfo {
                            active_tab: 0,
                            workspaces: vec![LapceWorkspace::default()],
                        },
                    },
                )
            },
            Some(config),
        );
    }

    pub fn run_app_command(&self, cmd: AppCommand) {
        match cmd {
            AppCommand::SaveApp => {
                let db: Arc<LapceDb> = use_context().unwrap();
                let _ = db.save_app(self);
            }
            AppCommand::WindowClosed(window_id) => {
                if self.app_terminated.get_untracked() {
                    return;
                }
                let db: Arc<LapceDb> = use_context().unwrap();
                if self.windows.with_untracked(|w| w.len()) == 1 {
                    let _ = db.insert_app(self.clone());
                }
                let window_data = self
                    .windows
                    .try_update(|windows| windows.remove(&window_id))
                    .unwrap();
                if let Some(window_data) = window_data {
                    window_data.scope.dispose();
                }
                let _ = db.save_app(self);
            }
            AppCommand::CloseWindow(window_id) => {
                floem::close_window(window_id);
            }
            AppCommand::NewWindow => {
                self.new_window();
            }
            AppCommand::WindowGotFocus(window_id) => {
                self.active_window.set(window_id);
            }
        }
    }

    fn create_windows(
        &self,
        db: Arc<LapceDb>,
        paths: Vec<PathObject>,
    ) -> floem::Application {
        let mut app = floem::Application::new();

        // Split user input into known existing directors and
        // file paths that exist or not
        let (dirs, files): (Vec<&PathObject>, Vec<&PathObject>) =
            paths.iter().partition(|p| p.is_dir);

        if !dirs.is_empty() {
            // There were directories specified, so we'll load those as windows

            // Use the last opened window's size and position as the default
            let (size, mut pos) = db
                .get_window()
                .map(|i| (i.size, i.pos))
                .unwrap_or_else(|_| (Size::new(800.0, 600.0), Point::new(0.0, 0.0)));

            for dir in dirs {
                #[cfg(windows)]
                let workspace_type = if !std::env::var("WSL_DISTRO_NAME")
                    .unwrap_or_default()
                    .is_empty()
                    || !std::env::var("WSL_INTEROP").unwrap_or_default().is_empty()
                {
                    LapceWorkspaceType::RemoteWSL(crate::workspace::WslHost {
                        host: String::new(),
                    })
                } else {
                    LapceWorkspaceType::Local
                };
                #[cfg(not(windows))]
                let workspace_type = LapceWorkspaceType::Local;

                let info = WindowInfo {
                    size,
                    pos,
                    maximised: false,
                    tabs: TabsInfo {
                        active_tab: 0,
                        workspaces: vec![LapceWorkspace {
                            kind: workspace_type,
                            path: Some(dir.path.to_owned()),
                            last_open: 0,
                        }],
                    },
                };

                pos += (50.0, 50.0);

                let config = self
                    .default_window_config()
                    .size(info.size)
                    .position(info.pos);
                let config = if cfg!(target_os = "macos")
                    || self.config.get_untracked().core.custom_titlebar
                {
                    config.show_titlebar(false)
                } else {
                    config
                };
                let app_data = self.clone();
                app = app.window(
                    move |window_id| app_data.app_view(window_id, info),
                    Some(config),
                );
            }
        } else if files.is_empty() {
            // There were no dirs and no files specified, so we'll load the last windows
            if let Ok(app_info) = db.get_app() {
                for info in app_info.windows {
                    let config = self
                        .default_window_config()
                        .size(info.size)
                        .position(info.pos);
                    let config = if cfg!(target_os = "macos")
                        || self.config.get_untracked().core.custom_titlebar
                    {
                        config.show_titlebar(false)
                    } else {
                        config
                    };
                    let app_data = self.clone();
                    app = app.window(
                        move |window_id| app_data.app_view(window_id, info),
                        Some(config),
                    );
                }
            }
        }

        if self.windows.with_untracked(|windows| windows.is_empty()) {
            let mut info = db.get_window().unwrap_or_else(|_| WindowInfo {
                size: Size::new(800.0, 600.0),
                pos: Point::ZERO,
                maximised: false,
                tabs: TabsInfo {
                    active_tab: 0,
                    workspaces: vec![LapceWorkspace::default()],
                },
            });
            info.tabs = TabsInfo {
                active_tab: 0,
                workspaces: vec![LapceWorkspace::default()],
            };
            let config = self
                .default_window_config()
                .size(info.size)
                .position(info.pos);
            let config = if cfg!(target_os = "macos")
                || self.config.get_untracked().core.custom_titlebar
            {
                config.show_titlebar(false)
            } else {
                config
            };
            let app_data = self.clone();
            app = app.window(
                move |window_id| app_data.app_view(window_id, info),
                Some(config),
            );
        }

        // Open any listed files in the first window
        if let Some(window) = self.windows.with_untracked(|windows| {
            windows
                .iter()
                .next()
                .map(|(_, window_data)| window_data.clone())
        }) {
            let cur_window_tab = window.active.get_untracked();
            let (_, window_tab) =
                &window.window_tabs.get_untracked()[cur_window_tab];
            for file in files {
                let position = file.linecol.map(|pos| {
                    EditorPosition::Position(lsp_types::Position {
                        line: pos.line.saturating_sub(1) as u32,
                        character: pos.column.saturating_sub(1) as u32,
                    })
                });

                window_tab.run_internal_command(InternalCommand::GoToLocation {
                    location: EditorLocation {
                        path: file.path.clone(),
                        position,
                        scroll_offset: None,
                        // Create a new editor for the file, so we don't change any current unconfirmed
                        // editor
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    },
                });
            }
        }

        app
    }

    fn app_view(&self, window_id: WindowId, info: WindowInfo) -> impl View {
        let window_data = WindowData::new(
            window_id,
            info,
            self.window_scale,
            self.latest_release.read_only(),
            self.app_command,
        );
        self.windows.update(|windows| {
            windows.insert(window_id, window_data.clone());
        });
        let window_size = window_data.common.size;
        let position = window_data.position;
        let window_scale = window_data.window_scale;
        let app_command = window_data.app_command;
        let config = window_data.config;
        // The KeyDown and PointerDown event handlers both need ownership of a WindowData object.
        let key_down_window_data = window_data.clone();
        let view =
            stack((
                workspace_tab_header(window_data.clone()),
                window(window_data.clone()),
                stack((
                    drag_resize_window_area(ResizeDirection::West, empty())
                        .style(|s| s.absolute().width(4.0).height_full()),
                    drag_resize_window_area(ResizeDirection::North, empty())
                        .style(|s| s.absolute().width_full().height(4.0)),
                    drag_resize_window_area(ResizeDirection::East, empty()).style(
                        move |s| {
                            s.absolute()
                                .margin_left(window_size.get().width as f32 - 4.0)
                                .width(4.0)
                                .height_full()
                        },
                    ),
                    drag_resize_window_area(ResizeDirection::South, empty()).style(
                        move |s| {
                            s.absolute()
                                .margin_top(window_size.get().height as f32 - 4.0)
                                .width_full()
                                .height(4.0)
                        },
                    ),
                    drag_resize_window_area(ResizeDirection::NorthWest, empty())
                        .style(|s| s.absolute().width(20.0).height(4.0)),
                    drag_resize_window_area(ResizeDirection::NorthWest, empty())
                        .style(|s| s.absolute().width(4.0).height(20.0)),
                    drag_resize_window_area(ResizeDirection::NorthEast, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_left(window_size.get().width as f32 - 20.0)
                                .width(20.0)
                                .height(4.0)
                        }),
                    drag_resize_window_area(ResizeDirection::NorthEast, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_left(window_size.get().width as f32 - 4.0)
                                .width(4.0)
                                .height(20.0)
                        }),
                    drag_resize_window_area(ResizeDirection::SouthWest, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_top(window_size.get().height as f32 - 4.0)
                                .width(20.0)
                                .height(4.0)
                        }),
                    drag_resize_window_area(ResizeDirection::SouthWest, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_top(window_size.get().height as f32 - 20.0)
                                .width(4.0)
                                .height(20.0)
                        }),
                    drag_resize_window_area(ResizeDirection::SouthEast, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_left(window_size.get().width as f32 - 20.0)
                                .margin_top(window_size.get().height as f32 - 4.0)
                                .width(20.0)
                                .height(4.0)
                        }),
                    drag_resize_window_area(ResizeDirection::SouthEast, empty())
                        .style(move |s| {
                            s.absolute()
                                .margin_left(window_size.get().width as f32 - 4.0)
                                .margin_top(window_size.get().height as f32 - 20.0)
                                .width(4.0)
                                .height(20.0)
                        }),
                ))
                .style(move |s| {
                    s.absolute().size_full().apply_if(
                        cfg!(target_os = "macos")
                            || !config.get_untracked().core.custom_titlebar,
                        |s| s.hide(),
                    )
                }),
            ))
            .style(|s| s.flex_col().size_full());
        let view_id = view.id();
        view.window_scale(move || window_scale.get())
            .keyboard_navigatable()
            .on_event(EventListener::KeyDown, move |event| {
                if let Event::KeyDown(key_event) = event {
                    if key_down_window_data.key_down(key_event) {
                        view_id.request_focus();
                    }
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            })
            .on_event(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    window_data.key_down(pointer_event);
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            })
            .on_event_stop(EventListener::WindowResized, move |event| {
                if let Event::WindowResized(size) = event {
                    window_size.set(*size);
                }
            })
            .on_event_stop(EventListener::WindowMoved, move |event| {
                if let Event::WindowMoved(point) = event {
                    position.set(*point);
                }
            })
            .on_event_stop(EventListener::WindowGotFocus, move |_| {
                app_command.send(AppCommand::WindowGotFocus(window_id));
            })
            .on_event_stop(EventListener::WindowClosed, move |_| {
                app_command.send(AppCommand::WindowClosed(window_id));
            })
    }
}

fn editor_tab_header(
    window_tab_data: Rc<WindowTabData>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let plugin = window_tab_data.plugin.clone();
    let editors = window_tab_data.main_split.editors;
    let diff_editors = window_tab_data.main_split.diff_editors;
    let focus = window_tab_data.common.focus;
    let config = window_tab_data.common.config;
    let internal_command = window_tab_data.common.internal_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let editor_tab_id =
        editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

    let editor_tab_active =
        create_memo(move |_| editor_tab.with(|editor_tab| editor_tab.active));
    let items = move || {
        let editor_tab = editor_tab.get();
        for (i, (index, _, _)) in editor_tab.children.iter().enumerate() {
            if index.get_untracked() != i {
                index.set(i);
            }
        }
        editor_tab.children
    };
    let key = |(_, _, child): &(RwSignal<usize>, RwSignal<Rect>, EditorTabChild)| {
        child.id()
    };
    let is_focused = move || {
        if let Focus::Workbench = focus.get() {
            editor_tab.with_untracked(|e| Some(e.editor_tab_id))
                == active_editor_tab.get()
        } else {
            false
        }
    };

    let view_fn = move |(i, layout_rect, child): (
        RwSignal<usize>,
        RwSignal<Rect>,
        EditorTabChild,
    )| {
        let local_child = child.clone();
        let child_for_close = child.clone();
        let child_for_mouse_close = child.clone();
        let main_split = main_split.clone();
        let plugin = plugin.clone();
        let child_view = move || {
            let info = child.view_info(editors, diff_editors, plugin, config);
            let hovered = create_rw_signal(false);

            use crate::config::ui::TabCloseButton;
            let tab_close_button_style = config.get().ui.tab_close_button;

            let tab_icon = container({
                svg(move || info.with(|info| info.icon.clone())).style(move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size)
                        .apply_opt(info.with(|info| info.color), |s, c| s.color(c))
                })
            })
            .style(|s| s.padding_horiz(10.0));

            let tab_content = label(move || info.with(|info| info.path.clone()))
                .style(move |s| {
                    s.apply_if(
                        !info
                            .with(|info| info.confirmed)
                            .map(|confirmed| confirmed.get())
                            .unwrap_or(true),
                        |s| s.font_style(FontStyle::Italic),
                    )
                    .apply_if(tab_close_button_style == TabCloseButton::Off, |s| {
                        s.margin_right(15.0)
                    })
                });

            let tab_close_button = clickable_icon(
                move || {
                    if hovered.get() || info.with(|info| info.is_pristine) {
                        LapceIcons::CLOSE
                    } else {
                        LapceIcons::UNSAVED
                    }
                },
                move || {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);
                    internal_command.send(InternalCommand::EditorTabChildClose {
                        editor_tab_id,
                        child: child_for_close.clone(),
                    });
                },
                || false,
                || false,
                config,
            )
            .on_event_stop(EventListener::PointerDown, |_| {})
            .on_event_stop(EventListener::PointerEnter, move |_| {
                hovered.set(true);
            })
            .on_event_stop(EventListener::PointerLeave, move |_| {
                hovered.set(false);
            })
            .style(|s| s.margin_horiz(6.0));

            let tab_style = move |s: Style| {
                s.items_center()
                    .border_left(if i.get() == 0 { 1.0 } else { 0.0 })
                    .border_right(1.0)
                    .border_color(config.get().color(LapceColor::LAPCE_BORDER))
            };

            match tab_close_button_style {
                TabCloseButton::Left => container_box(
                    stack((tab_close_button, tab_content, tab_icon))
                        .style(tab_style),
                ),
                TabCloseButton::Right => container_box(
                    stack((tab_icon, tab_content, tab_close_button))
                        .style(tab_style),
                ),
                TabCloseButton::Off => {
                    container_box(stack((tab_icon, tab_content)).style(tab_style))
                }
            }
        };

        let confirmed = match local_child {
            EditorTabChild::Editor(editor_id) => editors.with_untracked(|editors| {
                editors
                    .get(&editor_id)
                    .map(|editor_data| editor_data.confirmed)
            }),
            EditorTabChild::DiffEditor(diff_editor_id) => diff_editors
                .with_untracked(|diff_editors| {
                    diff_editors
                        .get(&diff_editor_id)
                        .map(|diff_editor_data| diff_editor_data.confirmed)
                }),
            _ => None,
        };

        let header_content_size = create_rw_signal(Size::ZERO);
        let drag_over_left: RwSignal<Option<bool>> = create_rw_signal(None);
        stack((
            container(child_view())
                .on_double_click_stop(move |_| {
                    if let Some(confirmed) = confirmed {
                        confirmed.set(true);
                    }
                })
                .on_event(EventListener::PointerDown, move |event| {
                    if let Event::PointerDown(pointer_event) = event {
                        if pointer_event.button.is_auxiliary() {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(
                                InternalCommand::EditorTabChildClose {
                                    editor_tab_id,
                                    child: child_for_mouse_close.clone(),
                                },
                            );
                            EventPropagation::Stop
                        } else {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = i.get_untracked();
                            });
                            EventPropagation::Continue
                        }
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event_stop(EventListener::DragStart, move |_| {
                    dragging.set(Some((i, editor_tab_id)));
                })
                .on_event_stop(EventListener::DragEnd, move |_| {
                    dragging.set(None);
                })
                .on_event_stop(EventListener::DragOver, move |event| {
                    if dragging.with_untracked(|dragging| dragging.is_some()) {
                        if let Event::PointerMove(pointer_event) = event {
                            let new_left = pointer_event.pos.x
                                < header_content_size.get_untracked().width / 2.0;
                            if drag_over_left.get_untracked() != Some(new_left) {
                                drag_over_left.set(Some(new_left));
                            }
                        }
                    }
                })
                .on_event(EventListener::Drop, move |event| {
                    if let Some((from_index, from_editor_tab_id)) =
                        dragging.get_untracked()
                    {
                        drag_over_left.set(None);
                        if let Event::PointerUp(pointer_event) = event {
                            let left = pointer_event.pos.x
                                < header_content_size.get_untracked().width / 2.0;
                            let index = i.get_untracked();
                            let new_index = if left { index } else { index + 1 };
                            main_split.move_editor_tab_child(
                                from_editor_tab_id,
                                editor_tab_id,
                                from_index.get_untracked(),
                                new_index,
                            );
                        }
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event_stop(EventListener::DragLeave, move |_| {
                    drag_over_left.set(None);
                })
                .on_resize(move |rect| {
                    header_content_size.set(rect.size());
                })
                .draggable()
                .dragging_style(move |s| {
                    let config = config.get();
                    s.border(1.0)
                        .border_radius(6.0)
                        .background(
                            config
                                .color(LapceColor::PANEL_BACKGROUND)
                                .with_alpha_factor(0.7),
                        )
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                })
                .style(|s| s.align_items(Some(AlignItems::Center)).height_full()),
            container(empty().style(move |s| {
                s.size_full()
                    .border_bottom(if editor_tab_active.get() == i.get() {
                        2.0
                    } else {
                        0.0
                    })
                    .border_color(config.get().color(if is_focused() {
                        LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                    } else {
                        LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                    }))
            }))
            .style(|s| s.absolute().padding_horiz(3.0).size_full()),
            empty().style(move |s| {
                let i = i.get();
                let drag_over_left = drag_over_left.get();
                s.absolute()
                    .margin_left(if i == 0 { 0.0 } else { -2.0 })
                    .height_full()
                    .width(
                        header_content_size.get().width as f32
                            + if i == 0 { 1.0 } else { 3.0 },
                    )
                    .apply_if(drag_over_left.is_none(), |s| s.hide())
                    .apply_if(drag_over_left.is_some(), |s| {
                        if let Some(drag_over_left) = drag_over_left {
                            if drag_over_left {
                                s.border_left(3.0)
                            } else {
                                s.border_right(3.0)
                            }
                        } else {
                            s
                        }
                    })
                    .border_color(
                        config
                            .get()
                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                            .with_alpha_factor(0.5),
                    )
            }),
        ))
        .on_resize(move |rect| {
            layout_rect.set(rect);
        })
        .style(|s| s.height_full())
    };

    let content_size = create_rw_signal(Size::ZERO);
    let scroll_offset = create_rw_signal(Rect::ZERO);
    stack((
        stack({
            let size = create_rw_signal(Size::ZERO);
            (
                clip(empty().style(move |s| {
                    let config = config.get();
                    s.absolute()
                        .height_full()
                        .width(size.get().width as f32)
                        .background(config.color(LapceColor::PANEL_BACKGROUND))
                        .box_shadow_blur(3.0)
                        .box_shadow_color(
                            config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
                        )
                }))
                .style(move |s| {
                    let scroll_offset = scroll_offset.get();
                    s.absolute()
                        .width(size.get().width as f32 + 30.0)
                        .height_full()
                        .apply_if(scroll_offset.x0 == 0.0, |s| s.hide())
                }),
                stack((
                    clickable_icon(
                        || LapceIcons::TAB_PREVIOUS,
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::PreviousEditorTab);
                        },
                        || false,
                        || false,
                        config,
                    )
                    .style(|s| s.margin_horiz(6.0).margin_vert(7.0)),
                    clickable_icon(
                        || LapceIcons::TAB_NEXT,
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::NextEditorTab);
                        },
                        || false,
                        || false,
                        config,
                    )
                    .style(|s| s.margin_right(6.0)),
                ))
                .on_resize(move |rect| {
                    size.set(rect.size());
                })
                .style(move |s| s.items_center()),
            )
        }),
        container({
            scroll({
                dyn_stack(items, key, view_fn)
                    .on_resize(move |rect| {
                        let size = rect.size();
                        if content_size.get_untracked() != size {
                            content_size.set(size);
                        }
                    })
                    .style(|s| s.height_full().items_center())
            })
            .on_scroll(move |rect| {
                scroll_offset.set(rect);
            })
            .on_ensure_visible(move || {
                let active = editor_tab_active.get();
                editor_tab
                    .with_untracked(|editor_tab| editor_tab.children[active].1)
                    .get_untracked()
            })
            .hide_bar(|| true)
            .vertical_scroll_as_horizontal(|| true)
            .style(|s| {
                s.position(Position::Absolute)
                    .height_full()
                    .max_width_full()
            })
        })
        .style(|s| s.height_full().flex_grow(1.0).flex_basis(0.0)),
        stack({
            let size = create_rw_signal(Size::ZERO);
            (
                clip({
                    empty().style(move |s| {
                        let config = config.get();
                        s.absolute()
                            .height_full()
                            .margin_left(30.0)
                            .width(size.get().width as f32)
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                            .box_shadow_blur(3.0)
                            .box_shadow_color(
                                config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
                            )
                    })
                })
                .style(move |s| {
                    let content_size = content_size.get();
                    let scroll_offset = scroll_offset.get();
                    s.absolute()
                        .margin_left(-30.0)
                        .width(size.get().width as f32 + 30.0)
                        .height_full()
                        .apply_if(scroll_offset.x1 >= content_size.width, |s| {
                            s.hide()
                        })
                }),
                stack((
                    clickable_icon(
                        || LapceIcons::SPLIT_HORIZONTAL,
                        move || {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(InternalCommand::Split {
                                direction: SplitDirection::Vertical,
                                editor_tab_id,
                            });
                        },
                        || false,
                        || false,
                        config,
                    )
                    .style(|s| s.margin_left(6.0)),
                    clickable_icon(
                        || LapceIcons::CLOSE,
                        move || {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(InternalCommand::EditorTabClose {
                                editor_tab_id,
                            });
                        },
                        || false,
                        || false,
                        config,
                    )
                    .style(|s| s.margin_horiz(6.0)),
                ))
                .on_resize(move |rect| {
                    size.set(rect.size());
                })
                .style(|s| s.items_center().height_full()),
            )
        })
        .style(|s| s.height_full()),
    ))
    .style(move |s| {
        let config = config.get();
        s.items_center()
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}

fn editor_tab_content(
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let common = main_split.common.clone();
    let workspace = common.workspace.clone();
    let editors = main_split.editors;
    let diff_editors = main_split.diff_editors;
    let config = common.config;
    let focus = common.focus;
    let items = move || {
        editor_tab
            .get()
            .children
            .into_iter()
            .map(|(_, _, child)| child)
    };
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |child| {
        let common = common.clone();
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data = editors
                    .with_untracked(|editors| editors.get(&editor_id).cloned());
                if let Some(editor_data) = editor_data {
                    let editor_scope = editor_data.scope;
                    let editor_tab_id = editor_data.editor_tab_id;
                    let is_active = move |tracked: bool| {
                        editor_scope.track();
                        let focus = if tracked {
                            focus.get()
                        } else {
                            focus.get_untracked()
                        };
                        if let Focus::Workbench = focus {
                            let active_editor_tab = if tracked {
                                active_editor_tab.get()
                            } else {
                                active_editor_tab.get_untracked()
                            };
                            let editor_tab = if tracked {
                                editor_tab_id.get()
                            } else {
                                editor_tab_id.get_untracked()
                            };
                            editor_tab.is_some() && editor_tab == active_editor_tab
                        } else {
                            false
                        }
                    };
                    let editor_data = create_rw_signal(editor_data);
                    container_box(editor_container_view(
                        window_tab_data.clone(),
                        workspace.clone(),
                        is_active,
                        editor_data,
                    ))
                } else {
                    container_box(text("emtpy editor"))
                }
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor_data = diff_editors.with_untracked(|diff_editors| {
                    diff_editors.get(&diff_editor_id).cloned()
                });
                if let Some(diff_editor_data) = diff_editor_data {
                    let focus_right = diff_editor_data.focus_right;
                    let diff_editor_tab_id = diff_editor_data.editor_tab_id;
                    let diff_editor_scope = diff_editor_data.scope;
                    let is_active = move |tracked: bool| {
                        let focus = if tracked {
                            focus.get()
                        } else {
                            focus.get_untracked()
                        };
                        if let Focus::Workbench = focus {
                            let active_editor_tab = if tracked {
                                active_editor_tab.get()
                            } else {
                                active_editor_tab.get_untracked()
                            };
                            let diff_editor_tab_id = if tracked {
                                diff_editor_tab_id.get()
                            } else {
                                diff_editor_tab_id.get_untracked()
                            };
                            Some(diff_editor_tab_id) == active_editor_tab
                        } else {
                            false
                        }
                    };
                    let left_viewport = diff_editor_data.left.viewport;
                    let left_scroll_to = diff_editor_data.left.scroll_to;
                    let right_viewport = diff_editor_data.right.viewport;
                    let right_scroll_to = diff_editor_data.right.scroll_to;
                    create_effect(move |_| {
                        let left_viewport = left_viewport.get();
                        if right_viewport.get_untracked() != left_viewport {
                            right_scroll_to
                                .set(Some(left_viewport.origin().to_vec2()));
                        }
                    });
                    create_effect(move |_| {
                        let right_viewport = right_viewport.get();
                        if left_viewport.get_untracked() != right_viewport {
                            left_scroll_to
                                .set(Some(right_viewport.origin().to_vec2()));
                        }
                    });
                    let left_editor =
                        create_rw_signal(diff_editor_data.left.clone());
                    let right_editor =
                        create_rw_signal(diff_editor_data.right.clone());
                    container_box(
                        stack((
                            container(editor_container_view(
                                window_tab_data.clone(),
                                workspace.clone(),
                                move |track| {
                                    is_active(track)
                                        && if track {
                                            !focus_right.get()
                                        } else {
                                            !focus_right.get_untracked()
                                        }
                                },
                                left_editor,
                            ))
                            .on_event_cont(EventListener::PointerDown, move |_| {
                                focus_right.set(false);
                            })
                            .style(move |s| {
                                s.height_full()
                                    .flex_grow(1.0)
                                    .flex_basis(0.0)
                                    .border_right(1.0)
                                    .border_color(
                                        config.get().color(LapceColor::LAPCE_BORDER),
                                    )
                            }),
                            container(editor_container_view(
                                window_tab_data.clone(),
                                workspace.clone(),
                                move |track| {
                                    is_active(track)
                                        && if track {
                                            focus_right.get()
                                        } else {
                                            focus_right.get_untracked()
                                        }
                                },
                                right_editor,
                            ))
                            .on_event_cont(EventListener::PointerDown, move |_| {
                                focus_right.set(true);
                            })
                            .style(|s| {
                                s.height_full().flex_grow(1.0).flex_basis(0.0)
                            }),
                            diff_show_more_section_view(
                                diff_editor_data.left.clone(),
                                diff_editor_data.right.clone(),
                            ),
                        ))
                        .style(|s| s.size_full()),
                    )
                    .on_cleanup(move || {
                        diff_editor_scope.dispose();
                    })
                } else {
                    container_box(text("emtpy diff editor"))
                }
            }
            EditorTabChild::Settings(_) => {
                container_box(settings_view(plugin.installed, common))
            }
            EditorTabChild::ThemeColorSettings(_) => {
                container_box(theme_color_settings_view(common))
            }
            EditorTabChild::Keymap(_) => container_box(keymap_view(common)),
            EditorTabChild::Volt(_, id) => {
                container_box(plugin_info_view(plugin.clone(), id))
            }
        };
        child.style(|s| s.size_full())
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(active, items, key, view_fn).style(|s| s.size_full())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragOverPosition {
    Top,
    Bottom,
    Left,
    Right,
    Middle,
}

fn editor_tab(
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let common = main_split.common.clone();
    let editor_tabs = main_split.editor_tabs;
    let editor_tab_id =
        editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
    let config = common.config;
    let focus = common.focus;
    let internal_command = main_split.common.internal_command;
    let tab_size = create_rw_signal(Size::ZERO);
    let drag_over: RwSignal<Option<DragOverPosition>> = create_rw_signal(None);
    stack((
        editor_tab_header(
            window_tab_data.clone(),
            active_editor_tab,
            editor_tab,
            dragging,
        ),
        stack((
            editor_tab_content(
                window_tab_data.clone(),
                plugin.clone(),
                active_editor_tab,
                editor_tab,
            ),
            empty().style(move |s| {
                let pos = drag_over.get();
                let width = match pos {
                    Some(pos) => match pos {
                        DragOverPosition::Top => 100.0,
                        DragOverPosition::Bottom => 100.0,
                        DragOverPosition::Left => 50.0,
                        DragOverPosition::Right => 50.0,
                        DragOverPosition::Middle => 100.0,
                    },
                    None => 100.0,
                };
                let height = match pos {
                    Some(pos) => match pos {
                        DragOverPosition::Top => 50.0,
                        DragOverPosition::Bottom => 50.0,
                        DragOverPosition::Left => 100.0,
                        DragOverPosition::Right => 100.0,
                        DragOverPosition::Middle => 100.0,
                    },
                    None => 100.0,
                };
                let size = tab_size.get_untracked();
                let margin_left = match pos {
                    Some(pos) => match pos {
                        DragOverPosition::Top => 0.0,
                        DragOverPosition::Bottom => 0.0,
                        DragOverPosition::Left => 0.0,
                        DragOverPosition::Right => size.width / 2.0,
                        DragOverPosition::Middle => 0.0,
                    },
                    None => 0.0,
                };
                let margin_top = match pos {
                    Some(pos) => match pos {
                        DragOverPosition::Top => 0.0,
                        DragOverPosition::Bottom => size.height / 2.0,
                        DragOverPosition::Left => 0.0,
                        DragOverPosition::Right => 0.0,
                        DragOverPosition::Middle => 0.0,
                    },
                    None => 0.0,
                };
                s.absolute()
                    .size_pct(width, height)
                    .margin_top(margin_top as f32)
                    .margin_left(margin_left as f32)
                    .apply_if(pos.is_none(), |s| s.hide())
                    .background(
                        config.get().color(LapceColor::EDITOR_DRAG_DROP_BACKGROUND),
                    )
            }),
            empty()
                .on_event_stop(EventListener::DragOver, move |event| {
                    if dragging.with_untracked(|dragging| dragging.is_some()) {
                        if let Event::PointerMove(pointer_event) = event {
                            let size = tab_size.get_untracked();
                            let pos = pointer_event.pos;
                            let new_drag_over = if pos.x < size.width / 4.0 {
                                DragOverPosition::Left
                            } else if pos.x > size.width * 3.0 / 4.0 {
                                DragOverPosition::Right
                            } else if pos.y < size.height / 4.0 {
                                DragOverPosition::Top
                            } else if pos.y > size.height * 3.0 / 4.0 {
                                DragOverPosition::Bottom
                            } else {
                                DragOverPosition::Middle
                            };
                            if drag_over.get_untracked() != Some(new_drag_over) {
                                drag_over.set(Some(new_drag_over));
                            }
                        }
                    }
                })
                .on_event_stop(EventListener::DragLeave, move |_| {
                    drag_over.set(None);
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some((from_index, from_editor_tab_id)) =
                        dragging.get_untracked()
                    {
                        if let Some(pos) = drag_over.get_untracked() {
                            match pos {
                                DragOverPosition::Top => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Up,
                                    );
                                }
                                DragOverPosition::Bottom => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Down,
                                    );
                                }
                                DragOverPosition::Left => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Left,
                                    );
                                }
                                DragOverPosition::Right => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Right,
                                    );
                                }
                                DragOverPosition::Middle => {
                                    main_split.move_editor_tab_child(
                                        from_editor_tab_id,
                                        editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab.with_untracked(|editor_tab| {
                                            editor_tab.active + 1
                                        }),
                                    );
                                }
                            }
                        }
                        drag_over.set(None);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_resize(move |rect| {
                    tab_size.set(rect.size());
                })
                .style(|s| s.absolute().size_full()),
        ))
        .style(|s| s.size_full()),
    ))
    .on_event_cont(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Workbench {
            focus.set(Focus::Workbench);
        }
        let editor_tab_id = editor_tab.with_untracked(|t| t.editor_tab_id);
        internal_command.send(InternalCommand::FocusEditorTab { editor_tab_id });
    })
    .on_cleanup(move || {
        if editor_tabs
            .with_untracked(|editor_tabs| editor_tabs.contains_key(&editor_tab_id))
        {
            return;
        }
        editor_tab
            .with_untracked(|editor_tab| editor_tab.scope)
            .dispose();
    })
    .style(|s| s.flex_col().size_full())
}

fn split_resize_border(
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let content_rect = move |content: &SplitContent, tracked: bool| {
        if tracked {
            match content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data =
                        editor_tabs.with(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        editor_tab_data.with(|editor_tab| editor_tab.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with(|splits| splits.get(split_id).cloned())
                    {
                        split.with(|split| split.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
            }
        } else {
            match content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data = editor_tabs
                        .with_untracked(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        editor_tab_data
                            .with_untracked(|editor_tab| editor_tab.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with_untracked(|splits| splits.get(split_id).cloned())
                    {
                        split.with_untracked(|split| split.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
            }
        }
    };
    let direction = move |tracked: bool| {
        if tracked {
            split.with(|split| split.direction)
        } else {
            split.with_untracked(|split| split.direction)
        }
    };
    dyn_stack(
        move || {
            let data = split.get();
            data.children.into_iter().enumerate().skip(1)
        },
        |(index, (_, content))| (*index, content.id()),
        move |(index, (_, content))| {
            let drag_start: RwSignal<Option<Point>> = create_rw_signal(None);
            let view = empty();
            let view_id = view.id();
            view.on_event_stop(EventListener::PointerDown, move |event| {
                view_id.request_active();
                if let Event::PointerDown(pointer_event) = event {
                    drag_start.set(Some(pointer_event.pos));
                }
            })
            .on_event_stop(EventListener::PointerUp, move |_| {
                drag_start.set(None);
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    if let Some(drag_start_point) = drag_start.get_untracked() {
                        let rects = split.with_untracked(|split| {
                            split
                                .children
                                .iter()
                                .map(|(_, c)| content_rect(c, false))
                                .collect::<Vec<Rect>>()
                        });
                        let direction = direction(false);
                        match direction {
                            SplitDirection::Vertical => {
                                let left = rects[index - 1].width();
                                let right = rects[index].width();
                                let shift = pointer_event.pos.x - drag_start_point.x;
                                let left = left + shift;
                                let right = right - shift;
                                let total_width =
                                    rects.iter().map(|r| r.width()).sum::<f64>();
                                split.with_untracked(|split| {
                                    for (i, (size, _)) in
                                        split.children.iter().enumerate()
                                    {
                                        if i == index - 1 {
                                            size.set(left / total_width);
                                        } else if i == index {
                                            size.set(right / total_width);
                                        } else {
                                            size.set(rects[i].width() / total_width);
                                        }
                                    }
                                })
                            }
                            SplitDirection::Horizontal => {
                                let up = rects[index - 1].height();
                                let down = rects[index].height();
                                let shift = pointer_event.pos.y - drag_start_point.y;
                                let up = up + shift;
                                let down = down - shift;
                                let total_height =
                                    rects.iter().map(|r| r.height()).sum::<f64>();
                                split.with_untracked(|split| {
                                    for (i, (size, _)) in
                                        split.children.iter().enumerate()
                                    {
                                        if i == index - 1 {
                                            size.set(up / total_height);
                                        } else if i == index {
                                            size.set(down / total_height);
                                        } else {
                                            size.set(
                                                rects[i].height() / total_height,
                                            );
                                        }
                                    }
                                })
                            }
                        }
                    }
                }
            })
            .style(move |s| {
                let rect = content_rect(&content, true);
                let is_dragging = drag_start.get().is_some();
                let direction = direction(true);
                s.position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left(rect.x0 as f32 - 0.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 0.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => PxPctAuto::Px(4.0),
                        SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                        SplitDirection::Horizontal => PxPctAuto::Px(4.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .apply_if(is_dragging, |s| {
                        s.cursor(match direction {
                            SplitDirection::Vertical => CursorStyle::ColResize,
                            SplitDirection::Horizontal => CursorStyle::RowResize,
                        })
                        .background(config.get().color(LapceColor::EDITOR_CARET))
                    })
                    .hover(|s| {
                        s.cursor(match direction {
                            SplitDirection::Vertical => CursorStyle::ColResize,
                            SplitDirection::Horizontal => CursorStyle::RowResize,
                        })
                        .background(config.get().color(LapceColor::EDITOR_CARET))
                    })
            })
        },
    )
    .style(|s| s.position(Position::Absolute).size_full())
}

fn split_border(
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let direction = move || split.with(|split| split.direction);
    dyn_stack(
        move || split.get().children.into_iter().skip(1),
        |(_, content)| content.id(),
        move |(_, content)| {
            container(empty().style(move |s| {
                let direction = direction();
                s.width(match direction {
                    SplitDirection::Vertical => PxPctAuto::Px(1.0),
                    SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                })
                .height(match direction {
                    SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                    SplitDirection::Horizontal => PxPctAuto::Px(1.0),
                })
                .background(config.get().color(LapceColor::LAPCE_BORDER))
            }))
            .style(move |s| {
                let rect = match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        let editor_tab_data = editor_tabs
                            .with(|tabs| tabs.get(editor_tab_id).cloned());
                        if let Some(editor_tab_data) = editor_tab_data {
                            editor_tab_data.with(|editor_tab| editor_tab.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                    SplitContent::Split(split_id) => {
                        if let Some(split) =
                            splits.with(|splits| splits.get(split_id).cloned())
                        {
                            split.with(|split| split.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                };
                let direction = direction();
                s.position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left(rect.x0 as f32 - 2.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 2.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => PxPctAuto::Px(4.0),
                        SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                        SplitDirection::Horizontal => PxPctAuto::Px(4.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .justify_content(Some(JustifyContent::Center))
            })
        },
    )
    .style(|s| s.position(Position::Absolute).size_full())
}

fn split_list(
    split: ReadSignal<SplitData>,
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let editor_tabs = main_split.editor_tabs.read_only();
    let active_editor_tab = main_split.active_editor_tab.read_only();
    let splits = main_split.splits.read_only();
    let config = main_split.common.config;
    let split_id = split.with_untracked(|split| split.split_id);

    let direction = move || split.with(|split| split.direction);
    let items = move || split.get().children.into_iter().enumerate();
    let key = |(_index, (_, content)): &(usize, (RwSignal<f64>, SplitContent))| {
        content.id()
    };
    let view_fn = {
        let main_split = main_split.clone();
        let window_tab_data = window_tab_data.clone();
        move |(_index, (split_size, content)): (
            usize,
            (RwSignal<f64>, SplitContent),
        )| {
            let plugin = plugin.clone();
            let child = match &content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data = editor_tabs
                        .with_untracked(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        container_box(editor_tab(
                            window_tab_data.clone(),
                            plugin.clone(),
                            active_editor_tab,
                            editor_tab_data,
                            dragging,
                        ))
                    } else {
                        container_box(text("emtpy editor tab"))
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with(|splits| splits.get(split_id).cloned())
                    {
                        split_list(
                            split.read_only(),
                            window_tab_data.clone(),
                            plugin.clone(),
                            dragging,
                        )
                    } else {
                        container_box(text("emtpy split"))
                    }
                }
            };
            let local_main_split = main_split.clone();
            let local_local_main_split = main_split.clone();
            child
                .on_resize(move |rect| match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        local_main_split.editor_tab_update_layout(
                            editor_tab_id,
                            None,
                            Some(rect),
                        );
                    }
                    SplitContent::Split(split_id) => {
                        let split_data =
                            splits.with(|splits| splits.get(split_id).cloned());
                        if let Some(split_data) = split_data {
                            split_data.update(|split| {
                                split.layout_rect = rect;
                            });
                        }
                    }
                })
                .on_move(move |point| match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        local_local_main_split.editor_tab_update_layout(
                            editor_tab_id,
                            Some(point),
                            None,
                        );
                    }
                    SplitContent::Split(split_id) => {
                        let split_data =
                            splits.with(|splits| splits.get(split_id).cloned());
                        if let Some(split_data) = split_data {
                            split_data.update(|split| {
                                split.window_origin = point;
                            });
                        }
                    }
                })
                .style(move |s| s.flex_grow(split_size.get() as f32).flex_basis(0.0))
        }
    };
    container_box(
        stack((
            dyn_stack(items, key, view_fn).style(move |s| {
                s.flex_direction(match direction() {
                    SplitDirection::Vertical => FlexDirection::Row,
                    SplitDirection::Horizontal => FlexDirection::Column,
                })
                .size_full()
            }),
            split_border(splits, editor_tabs, split, config),
            split_resize_border(splits, editor_tabs, split, config),
        ))
        .style(|s| s.size_full()),
    )
    .on_cleanup(move || {
        if splits.with_untracked(|splits| splits.contains_key(&split_id)) {
            return;
        }
        split
            .with_untracked(|split_data| split_data.scope)
            .dispose();
    })
}

fn main_split(window_tab_data: Rc<WindowTabData>) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get_untracked()
        .get(&root_split)
        .unwrap()
        .read_only();
    let config = window_tab_data.main_split.common.config;
    let panel = window_tab_data.panel.clone();
    let plugin = window_tab_data.plugin.clone();
    let dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>> =
        create_rw_signal(None);
    split_list(
        root_split,
        window_tab_data.clone(),
        plugin.clone(),
        dragging,
    )
    .style(move |s| {
        let config = config.get();
        let is_hidden = panel.panel_bottom_maximized(true)
            && panel.is_container_shown(&PanelContainerPosition::Bottom, true);
        s.border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
            .apply_if(is_hidden, |s| s.display(Display::None))
            .width_full()
            .flex_grow(1.0)
            .flex_basis(0.0)
    })
}

pub fn clickable_icon(
    icon: impl Fn() -> &'static str + 'static,
    on_click: impl Fn() + 'static,
    active_fn: impl Fn() -> bool + 'static + Copy,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(
        container(
            svg(move || config.get().ui_svg(icon()))
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        .disabled(|s| {
                            s.color(config.color(LapceColor::LAPCE_ICON_INACTIVE))
                                .cursor(CursorStyle::Default)
                        })
                })
                .disabled(disabled_fn),
        )
        .on_click_stop(move |_| {
            on_click();
        })
        .disabled(disabled_fn)
        .style(move |s| {
            let config = config.get();
            s.padding(4.0)
                .border_radius(6.0)
                .border(1.0)
                .border_color(Color::TRANSPARENT)
                .apply_if(active_fn(), |s| {
                    s.border_color(config.color(LapceColor::EDITOR_CARET))
                })
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.background(
                        config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        }),
    )
}

fn workbench(window_tab_data: Rc<WindowTabData>) -> impl View {
    let workbench_size = window_tab_data.common.workbench_size;
    let main_split_width = window_tab_data.main_split.width;
    stack((
        panel_container_view(window_tab_data.clone(), PanelContainerPosition::Left),
        {
            let window_tab_data = window_tab_data.clone();
            stack((
                main_split(window_tab_data.clone()),
                panel_container_view(
                    window_tab_data,
                    PanelContainerPosition::Bottom,
                ),
            ))
            .on_resize(move |rect| {
                let width = rect.size().width;
                if main_split_width.get_untracked() != width {
                    main_split_width.set(width);
                }
            })
            .style(|s| s.flex_col().size_full())
        },
        panel_container_view(window_tab_data.clone(), PanelContainerPosition::Right),
        window_message_view(window_tab_data.messages, window_tab_data.common.config),
    ))
    .on_resize(move |rect| {
        let size = rect.size();
        if size != workbench_size.get_untracked() {
            workbench_size.set(size);
        }
    })
    .style(move |s| s.size_full())
}

fn palette_item(
    workspace: Arc<LapceWorkspace>,
    i: usize,
    item: PaletteItem,
    index: ReadSignal<usize>,
    palette_item_height: f64,
    config: ReadSignal<Arc<LapceConfig>>,
    keymap: Option<&KeyMap>,
) -> impl View {
    match &item.content {
        PaletteItemContent::File { path, .. }
        | PaletteItemContent::Reference { path, .. } => {
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            // let (file_name, _) = create_signal(cx.scope, file_name);
            let folder = path
                .parent()
                .unwrap_or("".as_ref())
                .to_string_lossy()
                .into_owned();
            // let (folder, _) = create_signal(cx.scope, folder);
            let folder_len = folder.len();

            let file_name_indices = item
                .indices
                .iter()
                .filter_map(|&i| {
                    if folder_len > 0 {
                        if i > folder_len {
                            Some(i - folder_len - 1)
                        } else {
                            None
                        }
                    } else {
                        Some(i)
                    }
                })
                .collect::<Vec<_>>();
            let folder_indices = item
                .indices
                .iter()
                .filter_map(|&i| if i < folder_len { Some(i) } else { None })
                .collect::<Vec<_>>();

            let path = path.to_path_buf();
            let style_path = path.clone();
            container_box(
                stack((
                    svg(move || config.get().file_svg(&path).0).style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        let color = config.file_svg(&style_path).1;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .apply_opt(color, Style::color)
                    }),
                    focus_text(
                        move || file_name.clone(),
                        move || file_name_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || folder.clone(),
                        move || folder_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(LapceColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::DocumentSymbol {
            kind,
            name,
            container_name,
            ..
        } => {
            let kind = *kind;
            let text = name.to_string();
            let hint = container_name.clone().unwrap_or_default();
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container_box(
                stack((
                    svg(move || {
                        let config = config.get();
                        config
                            .symbol_svg(&kind)
                            .unwrap_or_else(|| config.ui_svg(LapceIcons::FILE))
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(LapceColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::WorkspaceSymbol {
            kind,
            name,
            location,
            ..
        } => {
            let text = name.to_string();
            let kind = *kind;

            let path = location.path.clone();
            let full_path = location.path.clone();
            let path = if let Some(workspace_path) = workspace.path.as_ref() {
                path.strip_prefix(workspace_path)
                    .unwrap_or(&full_path)
                    .to_path_buf()
            } else {
                path
            };

            let hint = path.to_string_lossy().to_string();
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container_box(
                stack((
                    svg(move || {
                        let config = config.get();
                        config
                            .symbol_svg(&kind)
                            .unwrap_or_else(|| config.ui_svg(LapceIcons::FILE))
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(LapceColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::RunAndDebug {
            mode,
            config: run_config,
        } => {
            let mode = *mode;
            let text = format!("{mode} {}", run_config.name);
            let hint = format!(
                "{} {}",
                run_config.program,
                run_config.args.clone().unwrap_or_default().join(" ")
            );
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container_box(
                stack((
                    svg(move || {
                        let config = config.get();
                        match mode {
                            RunDebugMode::Run => config.ui_svg(LapceIcons::START),
                            RunDebugMode::Debug => config.ui_svg(LapceIcons::DEBUG),
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(LapceColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::PaletteHelp { .. }
        | PaletteItemContent::Command { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            let keys = if let Some(keymap) = keymap {
                keymap
                    .key
                    .iter()
                    .map(|key| key.label().trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            } else {
                vec![]
            };
            container_box(
                stack((
                    focus_text(
                        move || text.clone(),
                        move || indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|s| {
                        s.flex_row()
                            .flex_grow(1.0)
                            .align_items(Some(AlignItems::Center))
                    }),
                    stack((dyn_stack(
                        move || keys.clone(),
                        |k| k.clone(),
                        move |key| {
                            label(move || key.clone()).style(move |s| {
                                s.padding_horiz(5.0)
                                    .padding_vert(1.0)
                                    .margin_right(5.0)
                                    .border(1.0)
                                    .border_radius(3.0)
                                    .border_color(
                                        config.get().color(LapceColor::LAPCE_BORDER),
                                    )
                            })
                        },
                    ),)),
                ))
                .style(|s| s.width_full().items_center()),
            )
        }
        PaletteItemContent::Line { .. }
        | PaletteItemContent::Workspace { .. }
        | PaletteItemContent::SshHost { .. }
        | PaletteItemContent::Language { .. }
        | PaletteItemContent::ColorTheme { .. }
        | PaletteItemContent::SCMReference { .. }
        | PaletteItemContent::TerminalProfile { .. }
        | PaletteItemContent::IconTheme { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container_box(
                focus_text(
                    move || text.clone(),
                    move || indices.clone(),
                    move || config.get().color(LapceColor::EDITOR_FOCUS),
                )
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        #[cfg(windows)]
        PaletteItemContent::WslHost { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container_box(
                focus_text(
                    move || text.clone(),
                    move || indices.clone(),
                    move || config.get().color(LapceColor::EDITOR_FOCUS),
                )
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
    }
    .style(move |s| {
        s.width_full()
            .height(palette_item_height as f32)
            .padding_horiz(10.0)
            .apply_if(index.get() == i, |style| {
                style.background(
                    config.get().color(LapceColor::PALETTE_CURRENT_BACKGROUND),
                )
            })
    })
}

fn palette_input(window_tab_data: Rc<WindowTabData>) -> impl View {
    let editor = window_tab_data.palette.input_editor.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_focused = move || focus.get() == Focus::Palette;

    let input = text_input(editor, is_focused)
        .placeholder(move || window_tab_data.palette.placeholder_text().to_owned())
        .style(|s| s.width_full());

    container(container(input).style(move |s| {
        let config = config.get();
        s.width_full()
            .height(25.0)
            .items_center()
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    }))
    .style(|s| s.padding_bottom(5.0))
}

struct PaletteItems(im::Vector<PaletteItem>);

impl VirtualVector<(usize, PaletteItem)> for PaletteItems {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, PaletteItem)> {
        let start = range.start;
        Box::new(
            self.0
                .slice(range)
                .into_iter()
                .enumerate()
                .map(move |(i, item)| (i + start, item)),
        )
    }
}

fn palette_content(
    window_tab_data: Rc<WindowTabData>,
    layout_rect: ReadSignal<Rect>,
) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let keymaps = window_tab_data
        .palette
        .keypress
        .get_untracked()
        .command_keymaps;
    let index = window_tab_data.palette.index.read_only();
    let clicked_index = window_tab_data.palette.clicked_index.write_only();
    let config = window_tab_data.common.config;
    let run_id = window_tab_data.palette.run_id;
    let input = window_tab_data.palette.input.read_only();
    let palette_item_height = 25.0;
    let workspace = window_tab_data.workspace.clone();
    stack((
        scroll({
            let workspace = workspace.clone();
            virtual_stack(
                VirtualDirection::Vertical,
                VirtualItemSize::Fixed(Box::new(move || palette_item_height)),
                move || PaletteItems(items.get()),
                move |(i, _item)| {
                    (run_id.get_untracked(), *i, input.get_untracked().input)
                },
                move |(i, item)| {
                    let workspace = workspace.clone();
                    let keymap = {
                        let cmd_kind = match &item.content {
                            PaletteItemContent::PaletteHelp { cmd } => {
                                Some(CommandKind::Workbench(cmd.clone()))
                            }
                            PaletteItemContent::Command {
                                cmd: LapceCommand { kind, .. },
                            } => Some(kind.clone()),
                            _ => None,
                        };

                        cmd_kind
                            .and_then(|kind| keymaps.get(kind.str()))
                            .and_then(|maps| maps.first())
                    };
                    container(palette_item(
                        workspace,
                        i,
                        item,
                        index,
                        palette_item_height,
                        config,
                        keymap,
                    ))
                    .on_click_stop(move |_| {
                        clicked_index.set(Some(i));
                    })
                    .style(move |s| {
                        s.width_full().cursor(CursorStyle::Pointer).hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    })
                },
            )
            .style(|s| s.width_full().flex_col())
        })
        .on_ensure_visible(move || {
            Size::new(1.0, palette_item_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    index.get() as f64 * palette_item_height,
                ))
        })
        .style(|s| s.width_full().min_height(0.0)),
        text("No matching results").style(move |s| {
            s.display(if items.with(|items| items.is_empty()) {
                Display::Flex
            } else {
                Display::None
            })
            .padding_horiz(10.0)
            .align_items(Some(AlignItems::Center))
            .height(palette_item_height as f32)
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .min_height(0.0)
            .max_height((layout_rect.get().height() * 0.45 - 36.0).round() as f32)
            .padding_bottom(5.0)
            .padding_bottom(5.0)
    })
}

fn palette_preview(window_tab_data: Rc<WindowTabData>) -> impl View {
    let palette_data = window_tab_data.palette.clone();
    let workspace = palette_data.workspace.clone();
    let preview_editor = palette_data.preview_editor;
    let has_preview = palette_data.has_preview;
    let config = palette_data.common.config;
    let preview_editor = create_rw_signal(preview_editor);
    container(
        container(editor_container_view(
            window_tab_data,
            workspace,
            |_tracked: bool| true,
            preview_editor,
        ))
        .style(move |s| {
            let config = config.get();
            s.position(Position::Absolute)
                .border_top(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .size_full()
                .background(config.color(LapceColor::EDITOR_BACKGROUND))
        }),
    )
    .style(move |s| {
        s.display(if has_preview.get() {
            Display::Flex
        } else {
            Display::None
        })
        .flex_grow(1.0)
    })
}

fn palette(window_tab_data: Rc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.common.config;
    let has_preview = palette_data.has_preview.read_only();
    container(
        stack((
            palette_input(window_tab_data.clone()),
            palette_content(window_tab_data.clone(), layout_rect),
            palette_preview(window_tab_data.clone()),
        ))
        .on_event_stop(EventListener::PointerDown, move |_| {})
        .style(move |s| {
            let config = config.get();
            s.width(500.0)
                .max_width_full()
                .max_height(if has_preview.get() {
                    PxPctAuto::Auto
                } else {
                    PxPctAuto::Pct(100.0)
                })
                .height(if has_preview.get() {
                    PxPctAuto::Px(layout_rect.get().height() - 10.0)
                } else {
                    PxPctAuto::Auto
                })
                .margin_top(5.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .flex_col()
                .background(config.color(LapceColor::PALETTE_BACKGROUND))
        }),
    )
    .style(move |s| {
        s.display(if status.get() == PaletteStatus::Inactive {
            Display::None
        } else {
            Display::Flex
        })
        .position(Position::Absolute)
        .size_full()
        .flex_col()
        .items_center()
    })
}

fn window_message_view(
    messages: RwSignal<Vec<(String, ShowMessageParams)>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let view_fn =
        move |(i, (title, message)): (usize, (String, ShowMessageParams))| {
            stack((
                svg(move || {
                    if let MessageType::ERROR = message.typ {
                        config.get().ui_svg(LapceIcons::ERROR)
                    } else {
                        config.get().ui_svg(LapceIcons::WARNING)
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = if let MessageType::ERROR = message.typ {
                        config.color(LapceColor::LAPCE_ERROR)
                    } else {
                        config.color(LapceColor::LAPCE_WARN)
                    };
                    s.min_width(size)
                        .size(size, size)
                        .margin_right(10.0)
                        .margin_top(4.0)
                        .color(color)
                }),
                stack((
                    text(title.clone()).style(|s| {
                        s.min_width(0.0).line_height(1.6).font_weight(Weight::BOLD)
                    }),
                    text(message.message.clone()).style(|s| {
                        s.min_width(0.0).line_height(1.6).margin_top(5.0)
                    }),
                ))
                .style(move |s| {
                    s.flex_col().min_width(0.0).flex_basis(0.0).flex_grow(1.0)
                }),
                clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        messages.update(|messages| {
                            messages.remove(i);
                        });
                    },
                    || false,
                    || false,
                    config,
                )
                .style(|s| s.margin_left(6.0)),
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .items_start()
                    .padding(10.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
                    .apply_if(i > 0, |s| s.margin_top(10.0))
            })
        };

    let id = AtomicU64::new(0);
    container(
        container(
            container(
                scroll(
                    dyn_stack(
                        move || messages.get().into_iter().enumerate(),
                        move |_| {
                            id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        },
                        view_fn,
                    )
                    .style(|s| s.flex_col().width_full()),
                )
                .style(|s| {
                    s.absolute().width_full().min_height(0.0).max_height_full()
                }),
            )
            .style(|s| s.size_full()),
        )
        .style(|s| {
            s.width(360.0)
                .max_width_pct(80.0)
                .padding(10.0)
                .height_full()
        }),
    )
    .style(|s| s.absolute().size_full().justify_end())
}

struct VectorItems<V>(im::Vector<V>);

impl<V: Clone + 'static> VirtualVector<(usize, V)> for VectorItems<V> {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: Range<usize>) -> impl Iterator<Item = (usize, V)> {
        let start = range.start;
        self.0
            .slice(range)
            .into_iter()
            .enumerate()
            .map(move |(i, item)| (i + start, item))
    }
}

fn completion_kind_to_str(kind: CompletionItemKind) -> &'static str {
    match kind {
        CompletionItemKind::METHOD => "f",
        CompletionItemKind::FUNCTION => "f",
        CompletionItemKind::CLASS => "c",
        CompletionItemKind::STRUCT => "s",
        CompletionItemKind::VARIABLE => "v",
        CompletionItemKind::INTERFACE => "i",
        CompletionItemKind::ENUM => "e",
        CompletionItemKind::ENUM_MEMBER => "e",
        CompletionItemKind::FIELD => "v",
        CompletionItemKind::PROPERTY => "p",
        CompletionItemKind::CONSTANT => "d",
        CompletionItemKind::MODULE => "m",
        CompletionItemKind::KEYWORD => "k",
        CompletionItemKind::SNIPPET => "n",
        _ => "t",
    }
}

fn hover(window_tab_data: Rc<WindowTabData>) -> impl View {
    let hover_data = window_tab_data.common.hover.clone();
    let config = window_tab_data.common.config;
    let id = AtomicU64::new(0);
    let layout_rect = window_tab_data.common.hover.layout_rect;

    scroll(
        dyn_stack(
            move || hover_data.content.get(),
            move |_| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            move |content| match content {
                MarkdownContent::Text(text_layout) => container_box(
                    rich_text(move || text_layout.clone())
                        .style(|s| s.max_width(600.0)),
                )
                .style(|s| s.max_width_full()),
                MarkdownContent::Image { .. } => container_box(empty()),
                MarkdownContent::Separator => {
                    container_box(empty().style(move |s| {
                        s.width_full()
                            .margin_vert(5.0)
                            .height(1.0)
                            .background(config.get().color(LapceColor::LAPCE_BORDER))
                    }))
                }
            },
        )
        .style(|s| s.flex_col().padding_horiz(10.0).padding_vert(5.0)),
    )
    .on_resize(move |rect| {
        layout_rect.set(rect);
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .style(move |s| {
        let active = window_tab_data.common.hover.active.get();
        if !active {
            s.hide()
        } else {
            let config = config.get();
            if let Some(origin) = window_tab_data.hover_origin() {
                s.absolute()
                    .margin_left(origin.x as f32)
                    .margin_top(origin.y as f32)
                    .max_height(300.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            } else {
                s.hide()
            }
        }
    })
}

fn completion(window_tab_data: Rc<WindowTabData>) -> impl View {
    let completion_data = window_tab_data.common.completion;
    let config = window_tab_data.common.config;
    let active = completion_data.with_untracked(|c| c.active);
    let request_id =
        move || completion_data.with_untracked(|c| (c.request_id, c.input_id));
    scroll(
        virtual_stack(
            VirtualDirection::Vertical,
            VirtualItemSize::Fixed(Box::new(move || {
                config.get().editor.line_height() as f64
            })),
            move || completion_data.with(|c| VectorItems(c.filtered_items.clone())),
            move |(i, _item)| (request_id(), *i),
            move |(i, item)| {
                stack((
                    container(
                        text(
                            item.item.kind.map(completion_kind_to_str).unwrap_or(""),
                        )
                        .style(move |s| {
                            s.width_full()
                                .justify_content(Some(JustifyContent::Center))
                        }),
                    )
                    .style(move |s| {
                        let config = config.get();
                        let width = config.editor.line_height() as f32;
                        s.width(width)
                            .min_width(width)
                            .height_full()
                            .align_items(Some(AlignItems::Center))
                            .font_weight(Weight::BOLD)
                            .apply_opt(
                                config.completion_color(item.item.kind),
                                |s, c| {
                                    s.color(c).background(c.with_alpha_factor(0.3))
                                },
                            )
                    }),
                    focus_text(
                        move || item.item.label.clone(),
                        move || item.indices.clone(),
                        move || config.get().color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(5.0)
                            .min_width(0.0)
                            .align_items(Some(AlignItems::Center))
                            .size_full()
                            .apply_if(active.get() == i, |s| {
                                s.background(
                                    config.color(LapceColor::COMPLETION_CURRENT),
                                )
                            })
                    }),
                ))
                .style(move |s| {
                    s.align_items(Some(AlignItems::Center))
                        .width_full()
                        .height(config.get().editor.line_height() as f32)
                })
            },
        )
        .style(|s| {
            s.align_items(Some(AlignItems::Center))
                .width_full()
                .flex_col()
        }),
    )
    .on_ensure_visible(move || {
        let config = config.get();
        let active = active.get();
        Size::new(1.0, config.editor.line_height() as f64)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                active as f64 * config.editor.line_height() as f64,
            ))
    })
    .on_resize(move |rect| {
        completion_data.update(|c| {
            c.layout_rect = rect;
        });
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .style(move |s| {
        let config = config.get();
        let origin = window_tab_data.completion_origin();
        s.position(Position::Absolute)
            .width(400.0)
            .max_height(400.0)
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(config.color(LapceColor::COMPLETION_BACKGROUND))
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size() as f32)
            .border_radius(6.0)
    })
}

fn code_action(window_tab_data: Rc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let code_action = window_tab_data.code_action;
    let (status, active) = code_action
        .with_untracked(|code_action| (code_action.status, code_action.active));
    let request_id =
        move || code_action.with_untracked(|code_action| code_action.request_id);
    scroll(
        container(
            dyn_stack(
                move || {
                    code_action.with(|code_action| {
                        code_action.filtered_items.clone().into_iter().enumerate()
                    })
                },
                move |(i, _item)| (request_id(), *i),
                move |(i, item)| {
                    container(
                        text(item.title().replace('\n', " "))
                            .style(|s| s.text_ellipsis().min_width(0.0)),
                    )
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(10.0)
                            .align_items(Some(AlignItems::Center))
                            .min_width(0.0)
                            .width_full()
                            .line_height(1.6)
                            .apply_if(active.get() == i, |s| {
                                s.border_radius(6.0).background(
                                    config.color(LapceColor::COMPLETION_CURRENT),
                                )
                            })
                    })
                },
            )
            .style(|s| s.width_full().flex_col()),
        )
        .style(|s| s.width_full().padding_vert(4.0)),
    )
    .on_ensure_visible(move || {
        let config = config.get();
        let active = active.get();
        Size::new(1.0, config.editor.line_height() as f64)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                active as f64 * config.editor.line_height() as f64,
            ))
    })
    .on_resize(move |rect| {
        code_action.update(|c| {
            c.layout_rect = rect;
        });
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .style(move |s| {
        let origin = window_tab_data.code_action_origin();
        s.display(match status.get() {
            CodeActionStatus::Inactive => Display::None,
            CodeActionStatus::Active => Display::Flex,
        })
        .position(Position::Absolute)
        .width(400.0)
        .max_height(400.0)
        .margin_left(origin.x as f32)
        .margin_top(origin.y as f32)
        .background(config.get().color(LapceColor::COMPLETION_BACKGROUND))
        .border_radius(6.0)
    })
}

fn rename(window_tab_data: Rc<WindowTabData>) -> impl View {
    let editor = window_tab_data.rename.editor.clone();
    let active = window_tab_data.rename.active;
    let layout_rect = window_tab_data.rename.layout_rect;
    let config = window_tab_data.common.config;

    container(
        container(
            text_input(editor, move || active.get()).style(|s| s.width(150.0)),
        )
        .style(move |s| {
            let config = config.get();
            s.font_family(config.editor.font_family.clone())
                .font_size(config.editor.font_size() as f32)
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .background(config.color(LapceColor::EDITOR_BACKGROUND))
        }),
    )
    .on_resize(move |rect| {
        layout_rect.set(rect);
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        let origin = window_tab_data.rename_origin();
        s.position(Position::Absolute)
            .apply_if(!active.get(), |s| s.hide())
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(config.get().color(LapceColor::PANEL_BACKGROUND))
            .border_radius(6.0)
            .padding(6.0)
    })
}

fn window_tab(window_tab_data: Rc<WindowTabData>) -> impl View {
    let source_control = window_tab_data.source_control.clone();
    let window_origin = window_tab_data.common.window_origin;
    let layout_rect = window_tab_data.layout_rect;
    let config = window_tab_data.common.config;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_tab_scope = window_tab_data.scope;
    let hover_active = window_tab_data.common.hover.active;
    let status_height = window_tab_data.status_height;

    let view = stack((
        stack((
            title(window_tab_data.clone()),
            workbench(window_tab_data.clone()),
            status(
                window_tab_data.clone(),
                source_control,
                workbench_command,
                status_height,
                config,
            ),
        ))
        .on_resize(move |rect| {
            layout_rect.set(rect);
        })
        .on_move(move |point| {
            window_origin.set(point);
        })
        .style(|s| s.size_full().flex_col()),
        completion(window_tab_data.clone()),
        hover(window_tab_data.clone()),
        code_action(window_tab_data.clone()),
        rename(window_tab_data.clone()),
        palette(window_tab_data.clone()),
        about::about_popup(window_tab_data.clone()),
        alert::alert_box(window_tab_data.alert_data.clone()),
    ))
    .on_cleanup(move || {
        window_tab_scope.dispose();
    })
    .on_event_cont(EventListener::PointerMove, move |_| {
        if hover_active.get_untracked() {
            hover_active.set(false);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.size_full()
            .color(config.color(LapceColor::EDITOR_FOREGROUND))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
            .font_size(config.ui.font_size() as f32)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
            .class(floem::views::scroll::Handle, |s| {
                s.background(config.color(LapceColor::LAPCE_SCROLL_BAR))
            })
    });

    let view_id = view.id();
    window_tab_data.common.view_id.set(view_id);
    view
}

fn workspace_title(workspace: &LapceWorkspace) -> Option<String> {
    let p = workspace.path.as_ref()?;
    let dir = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
    Some(match &workspace.kind {
        LapceWorkspaceType::Local => format!("{dir}"),
        LapceWorkspaceType::RemoteSSH(remote) => format!("{dir} [{remote}]"),
        #[cfg(windows)]
        LapceWorkspaceType::RemoteWSL(remote) => format!("{dir} [{remote}]"),
    })
}

fn workspace_tab_header(window_data: WindowData) -> impl View {
    let tabs = window_data.window_tabs;
    let active = window_data.active;
    let config = window_data.config;
    let window_tab_header_height = window_data.common.window_tab_header_height;
    let available_width = create_rw_signal(0.0);
    let add_icon_width = create_rw_signal(0.0);
    let window_control_width = create_rw_signal(0.0);
    let window_maximized = window_data.common.window_maximized;
    let num_window_tabs = window_data.num_window_tabs;
    let window_command = window_data.common.window_command;

    let tab_width = create_memo(move |_| {
        let window_control_width = if !cfg!(target_os = "macos")
            && config.get_untracked().core.custom_titlebar
        {
            window_control_width.get()
        } else {
            0.0
        };
        let available_width = available_width.get()
            - add_icon_width.get()
            - if cfg!(target_os = "macos") { 75.0 } else { 0.0 }
            - window_control_width
            - 30.0;
        let tabs_len = tabs.with(|tabs| tabs.len());
        if tabs_len > 0 {
            (available_width / tabs_len as f64).min(200.0)
        } else {
            available_width
        }
    });

    let local_window_data = window_data.clone();
    let dragging_index: RwSignal<Option<RwSignal<usize>>> = create_rw_signal(None);
    let view_fn = move |(index, tab): (RwSignal<usize>, Rc<WindowTabData>)| {
        let drag_over_left = create_rw_signal(None);
        let window_data = local_window_data.clone();
        stack((
            container({
                stack((
                    stack((
                        text(
                            workspace_title(&tab.workspace)
                                .unwrap_or_else(|| String::from("New Tab")),
                        )
                        .style(|s| {
                            s.margin_left(10.0)
                                .min_width(0.0)
                                .flex_basis(0.0)
                                .flex_grow(1.0)
                                .text_ellipsis()
                        }),
                        {
                            let window_data = local_window_data.clone();
                            clickable_icon(
                                || LapceIcons::WINDOW_CLOSE,
                                move || {
                                    window_data.run_window_command(
                                        WindowCommand::CloseWorkspaceTab {
                                            index: Some(index.get_untracked()),
                                        },
                                    );
                                },
                                || false,
                                || false,
                                config.read_only(),
                            )
                            .style(|s| s.margin_horiz(6.0))
                        },
                    ))
                    .on_event_stop(EventListener::DragOver, move |event| {
                        if dragging_index.get_untracked().is_some() {
                            if let Event::PointerMove(pointer_event) = event {
                                let left = pointer_event.pos.x
                                    < tab_width.get_untracked() / 2.0;
                                if drag_over_left.get_untracked() != Some(left) {
                                    drag_over_left.set(Some(left));
                                }
                            }
                        }
                    })
                    .on_event(EventListener::Drop, move |event| {
                        if dragging_index.get_untracked().is_some() {
                            drag_over_left.set(None);
                            if let Event::PointerUp(pointer_event) = event {
                                let left = pointer_event.pos.x
                                    < tab_width.get_untracked() / 2.0;
                                let index = index.get_untracked();
                                let new_index = if left { index } else { index + 1 };
                                if let Some(from_index) =
                                    dragging_index.get_untracked()
                                {
                                    window_data.move_tab(
                                        from_index.get_untracked(),
                                        new_index,
                                    );
                                }
                                dragging_index.set(None);
                            }
                            EventPropagation::Stop
                        } else {
                            EventPropagation::Continue
                        }
                    })
                    .on_event_stop(EventListener::DragLeave, move |_| {
                        drag_over_left.set(None);
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .min_width(0.0)
                            .items_center()
                            .border_right(1.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .apply_if(
                                cfg!(target_os = "macos") && index.get() == 0,
                                |s| s.border_left(1.0),
                            )
                    }),
                    container(empty().style(move |s| {
                        s.size_full()
                            .apply_if(active.get() == index.get(), |s| {
                                s.border_bottom(2.0)
                            })
                            .border_color(
                                config
                                    .get()
                                    .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                            )
                    }))
                    .style(|s| {
                        s.position(Position::Absolute)
                            .padding_horiz(3.0)
                            .size_full()
                    }),
                ))
                .style(move |s| s.size_full().items_center())
            })
            .draggable()
            .on_event_stop(EventListener::DragStart, move |_| {
                dragging_index.set(Some(index));
            })
            .on_event_stop(EventListener::DragEnd, move |_| {
                dragging_index.set(None);
            })
            .dragging_style(move |s| {
                let config = config.get();
                s.border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .color(
                        config
                            .color(LapceColor::EDITOR_FOREGROUND)
                            .with_alpha_factor(0.7),
                    )
                    .background(
                        config
                            .color(LapceColor::PANEL_BACKGROUND)
                            .with_alpha_factor(0.7),
                    )
            })
            .on_click_stop(move |_| {
                active.set(index.get_untracked());
            })
            .style(move |s| s.size_full()),
            empty().style(move |s| {
                let index = index.get();
                s.absolute()
                    .margin_left(if index == 0 { 0.0 } else { -2.0 })
                    .width(
                        tab_width.get() as f32 + if index == 0 { 1.0 } else { 3.0 },
                    )
                    .height_full()
                    .border_color(
                        config.get().color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                    )
                    .apply_if(drag_over_left.get().is_some(), move |s| {
                        let drag_over_left = drag_over_left.get_untracked().unwrap();
                        if drag_over_left {
                            s.border_left(3.0)
                        } else {
                            s.border_right(3.0)
                        }
                    })
                    .apply_if(drag_over_left.get().is_none(), move |s| s.hide())
            }),
        ))
        .style(move |s| s.height_full().width(tab_width.get() as f32))
    };

    stack((
        empty().style(move |s| {
            let is_macos = cfg!(target_os = "macos");
            s.min_width(75.0)
                .width(75.0)
                .apply_if(!is_macos, |s| s.hide())
        }),
        dyn_stack(
            move || {
                let tabs = tabs.get();
                for (i, (index, _)) in tabs.iter().enumerate() {
                    if index.get_untracked() != i {
                        index.set(i);
                    }
                }
                tabs
            },
            |(_, tab)| tab.window_tab_id,
            view_fn,
        )
        .style(|s| s.height_full()),
        container(clickable_icon(
            || LapceIcons::ADD,
            move || {
                window_data.run_window_command(WindowCommand::NewWorkspaceTab {
                    workspace: LapceWorkspace::default(),
                    end: true,
                });
            },
            || false,
            || false,
            config.read_only(),
        ))
        .on_resize(move |rect| {
            let current = add_icon_width.get_untracked();
            if rect.width() != current {
                add_icon_width.set(rect.width());
            }
        })
        .style(|s| {
            s.height_full()
                .padding_left(10.0)
                .padding_right(10.0)
                .items_center()
        }),
        drag_window_area(empty())
            .style(|s| s.height_full().flex_basis(0.0).flex_grow(1.0)),
        window_controls_view(
            window_command,
            false,
            num_window_tabs,
            window_maximized,
            config.read_only(),
        )
        .on_resize(move |rect| {
            let width = rect.width();
            if window_control_width.get_untracked() != width {
                window_control_width.set(width);
            }
        }),
    ))
    .on_resize(move |rect| {
        let current = available_width.get_untracked();
        if rect.width() != current {
            available_width.set(rect.width());
        }
        window_tab_header_height.set(rect.height());
    })
    .style(move |s| {
        let config = config.get();
        s.border_bottom(1.0)
            .width_full()
            .height(37.0)
            .font_size(config.ui.font_size() as f32)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
            .apply_if(tabs.with(|tabs| tabs.len() < 2), |s| s.hide())
            .color(config.color(LapceColor::EDITOR_FOREGROUND))
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .items_center()
    })
}

fn window(window_data: WindowData) -> impl View {
    let window_tabs = window_data.window_tabs.read_only();
    let active = window_data.active.read_only();
    let items = move || window_tabs.get();
    let key = |(_, window_tab): &(RwSignal<usize>, Rc<WindowTabData>)| {
        window_tab.window_tab_id
    };
    let active = move || active.get();
    let window_focus = create_rw_signal(false);
    let ime_enabled = window_data.ime_enabled;
    let window_maximized = window_data.common.window_maximized;

    tab(active, items, key, |(_, window_tab_data)| {
        window_tab(window_tab_data)
    })
    .window_title(move || {
        let active = active();
        let window_tabs = window_tabs.get();
        let workspace = window_tabs
            .get(active)
            .or_else(|| window_tabs.last())
            .and_then(|(_, window_tab)| window_tab.workspace.display());
        match workspace {
            Some(workspace) => format!("{workspace} - Lapce"),
            None => "Lapce".to_string(),
        }
    })
    .on_event_stop(EventListener::ImeEnabled, move |_| {
        ime_enabled.set(true);
    })
    .on_event_stop(EventListener::ImeDisabled, move |_| {
        ime_enabled.set(false);
    })
    .on_event_cont(EventListener::WindowGotFocus, move |_| {
        window_focus.set(true);
    })
    .on_event_cont(EventListener::WindowMaximizeChanged, move |event| {
        if let Event::WindowMaximizeChanged(maximized) = event {
            window_maximized.set(*maximized);
        }
    })
    .window_menu(move || {
        window_focus.track();
        let active = active();
        let window_tabs = window_tabs.get();
        let window_tab = window_tabs.get(active).or_else(|| window_tabs.last());
        if let Some((_, window_tab)) = window_tab {
            window_tab.common.keypress.track();
            let workbench_command = window_tab.common.workbench_command;
            let lapce_command = window_tab.common.lapce_command;
            window_menu(lapce_command, workbench_command)
        } else {
            Menu::new("Lapce")
        }
    })
    .style(|s| s.size_full())
}

#[inline(always)]
fn logging() -> (Handle<Targets>, Option<WorkerGuard>) {
    use tracing_subscriber::{filter, fmt, prelude::*, reload};

    let (log_file, guard) = match Directory::logs_directory()
        .and_then(|dir| {
            tracing_appender::rolling::Builder::new()
                .max_log_files(10)
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("lapce")
                .filename_suffix("log")
                .build(dir)
                .ok()
        })
        .map(tracing_appender::non_blocking)
    {
        Some((log_file, guard)) => (Some(log_file), Some(guard)),
        None => (None, None),
    };

    let log_file_filter_targets = filter::Targets::new()
        .with_target("lapce_app", LevelFilter::DEBUG)
        .with_target("lapce_proxy", LevelFilter::DEBUG);
    let (log_file_filter, reload_handle) =
        reload::Subscriber::new(log_file_filter_targets);

    let console_filter_targets = std::env::var("LAPCE_LOG")
        .unwrap_or_default()
        .parse::<filter::Targets>()
        .unwrap_or_default();

    let registry = tracing_subscriber::registry();
    if let Some(log_file) = log_file {
        let file_layer = tracing_subscriber::fmt::subscriber()
            .with_ansi(false)
            .with_writer(log_file)
            .with_filter(log_file_filter);
        registry
            .with(file_layer)
            .with(fmt::Subscriber::default().with_filter(console_filter_targets))
            .init();
    } else {
        registry
            .with(fmt::Subscriber::default().with_filter(console_filter_targets))
            .init();
    };

    (reload_handle, guard)
}

pub fn launch() {
    let (reload_handle, _guard) = logging();
    tracing::info!("Starting up Lapce..");

    // if PWD is not set, then we are not being launched via a terminal
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if std::env::var("PWD").is_err() {
        load_shell_env();
    }

    let cli = Cli::parse();

    // small hack to unblock terminal if launched from it
    // launch it as a separate process that waits
    if !cli.wait {
        let mut args = std::env::args().collect::<Vec<_>>();
        args.push("--wait".to_string());
        let mut cmd = std::process::Command::new(&args[0]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Err(why) = cmd
            .args(&args[1..])
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
        {
            eprintln!("Failed to launch lapce: {why}");
        };
        return;
    }

    if !cli.new {
        if let Ok(socket) = get_socket() {
            if let Err(e) = try_open_in_existing_process(socket, &cli.paths) {
                error!("failed to open path(s): {e}");
            };
            return;
        }
    }

    std::thread::spawn(move || {
        if let Err(e) = fetch_grammars() {
            error!("failed to fetch grammars: {e}");
        }
    });

    #[cfg(feature = "updater")]
    crate::update::cleanup();

    let _ = lapce_proxy::register_lapce_path();
    let db = Arc::new(LapceDb::new().unwrap());
    let scope = Scope::new();
    provide_context(db.clone());

    let window_scale = scope.create_rw_signal(1.0);
    let latest_release = scope.create_rw_signal(Arc::new(None));
    let app_command = Listener::new_empty(scope);

    let (tx, rx) = crossbeam_channel::bounded(1);
    let mut watcher = notify::recommended_watcher(ConfigWatcher::new(tx)).unwrap();
    if let Some(path) = LapceConfig::settings_file() {
        let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
    }
    if let Some(path) = Directory::themes_directory() {
        let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
    }
    if let Some(path) = LapceConfig::keymaps_file() {
        let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
    }
    if let Some(path) = Directory::plugins_directory() {
        let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
    }

    let windows = scope.create_rw_signal(im::HashMap::new());
    let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
    let config = scope.create_rw_signal(Arc::new(config));
    let app_data = AppData {
        windows,
        active_window: scope.create_rw_signal(WindowId::from(0)),
        window_scale,
        app_terminated: scope.create_rw_signal(false),
        watcher: Arc::new(watcher),
        latest_release,
        app_command,
        tracing_handle: reload_handle,
        config,
    };

    let app = app_data.create_windows(db.clone(), cli.paths);

    {
        let app_data = app_data.clone();
        let notification = create_signal_from_channel(rx);
        create_effect(move |_| {
            if notification.get().is_some() {
                app_data.reload_config();
            }
        });
    }

    #[cfg(feature = "updater")]
    {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let notification = create_signal_from_channel(rx);
        let latest_release = app_data.latest_release;
        create_effect(move |_| {
            if let Some(release) = notification.get() {
                latest_release.set(Arc::new(Some(release)));
            }
        });
        std::thread::spawn(move || loop {
            if let Ok(release) = crate::update::get_latest_release() {
                let _ = tx.send(release);
            }
            std::thread::sleep(std::time::Duration::from_secs(60 * 60));
        });
    }

    {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let notification = create_signal_from_channel(rx);
        let app_data = app_data.clone();
        create_effect(move |_| {
            if let Some(CoreNotification::OpenPaths { paths }) = notification.get() {
                if let Some(window_tab) = app_data.active_window_tab() {
                    window_tab.open_paths(&paths);
                }
            }
        });
        std::thread::spawn(move || {
            let _ = listen_local_socket(tx);
        });
    }

    {
        let app_data = app_data.clone();
        app_data.app_command.listen(move |command| {
            app_data.run_app_command(command);
        });
    }

    app.on_event(move |event| match event {
        floem::AppEvent::WillTerminate => {
            app_data.app_terminated.set(true);
            let _ = db.insert_app(app_data.clone());
        }
        floem::AppEvent::Reopen {
            has_visible_windows,
        } => {
            if !has_visible_windows {
                app_data.new_window();
            }
        }
    })
    .run();
}

/// Uses a login shell to load the correct shell environment for the current user.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_shell_env() {
    use std::process::Command;

    let shell = match std::env::var("SHELL") {
        Ok(s) => s,
        Err(_) => {
            // Shell variable is not set, so we can't determine the correct shell executable.
            // Silently failing, since logger is not set up yet.
            return;
        }
    };

    let mut command = Command::new(shell);

    command.args(["--login"]).args(["-c", "printenv"]);

    let env = match command.output() {
        Ok(output) => String::from_utf8(output.stdout).unwrap_or_default(),

        Err(_) => {
            // sliently ignoring since logger is not yet available
            return;
        }
    };

    env.split('\n')
        .filter_map(|line| line.split_once('='))
        .for_each(|(key, value)| {
            std::env::set_var(key, value);
        })
}

pub fn get_socket() -> Result<interprocess::local_socket::LocalSocketStream> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let socket =
        interprocess::local_socket::LocalSocketStream::connect(local_socket)?;
    Ok(socket)
}

pub fn try_open_in_existing_process(
    mut socket: interprocess::local_socket::LocalSocketStream,
    paths: &[PathObject],
) -> Result<()> {
    let msg: CoreMessage = RpcMessage::Notification(CoreNotification::OpenPaths {
        paths: paths.to_vec(),
    });
    lapce_rpc::stdio::write_msg(&mut socket, msg)?;

    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let mut buf = [0; 100];
        let received = if let Ok(n) = socket.read(&mut buf) {
            &buf[..n] == b"received"
        } else {
            false
        };
        tx.send(received)
    });

    let received = rx.recv_timeout(std::time::Duration::from_millis(500))?;
    if !received {
        return Err(anyhow!("didn't receive response"));
    }

    Ok(())
}

fn listen_local_socket(tx: Sender<CoreNotification>) -> Result<()> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let _ = std::fs::remove_file(&local_socket);
    let socket =
        interprocess::local_socket::LocalSocketListener::bind(local_socket)?;

    for stream in socket.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || -> Result<()> {
            let mut reader = BufReader::new(stream);
            loop {
                let msg: CoreMessage = lapce_rpc::stdio::read_msg(&mut reader)?;

                if let RpcMessage::Notification(msg) = msg {
                    tx.send(msg)?;
                } else {
                    trace!("Unhandled message: {msg:?}");
                }

                let stream_ref = reader.get_mut();
                let _ = stream_ref.write_all(b"received");
                let _ = stream_ref.flush();
            }
        });
    }
    Ok(())
}

pub fn window_menu(
    lapce_command: Listener<LapceCommand>,
    workbench_command: Listener<LapceWorkbenchCommand>,
) -> Menu {
    Menu::new("Lapce")
        .entry({
            let mut menu = Menu::new("Lapce")
                .entry(MenuItem::new("About Lapce").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::ShowAbout)
                }))
                .separator()
                .entry(
                    Menu::new("Settings...")
                        .entry(MenuItem::new("Open Settings").action(move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenSettings);
                        }))
                        .entry(MenuItem::new("Open Keyboard Shortcuts").action(
                            move || {
                                workbench_command.send(
                                    LapceWorkbenchCommand::OpenKeyboardShortcuts,
                                );
                            },
                        )),
                )
                .separator()
                .entry(MenuItem::new("Quit Lapce").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::Quit);
                }));
            if cfg!(target_os = "macos") {
                menu = menu
                    .separator()
                    .entry(MenuItem::new("Hide Lapce"))
                    .entry(MenuItem::new("Hide Others"))
                    .entry(MenuItem::new("Show All"))
            }
            menu
        })
        .separator()
        .entry(
            Menu::new("File")
                .entry(MenuItem::new("New File").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::NewFile);
                }))
                .separator()
                .entry(MenuItem::new("Open").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::OpenFile);
                }))
                .entry(MenuItem::new("Open Folder").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::OpenFolder);
                }))
                .separator()
                .entry(MenuItem::new("Save").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::Save),
                        data: None,
                    });
                }))
                .entry(MenuItem::new("Save All").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::SaveAll);
                }))
                .separator()
                .entry(MenuItem::new("Close Folder").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::CloseFolder);
                }))
                .entry(MenuItem::new("Close Window").action(move || {
                    workbench_command.send(LapceWorkbenchCommand::CloseWindow);
                })),
        )
        .entry(
            Menu::new("Edit")
                .entry(MenuItem::new("Cut").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Edit(EditCommand::ClipboardCut),
                        data: None,
                    });
                }))
                .entry(MenuItem::new("Copy").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Edit(EditCommand::ClipboardCopy),
                        data: None,
                    });
                }))
                .entry(MenuItem::new("Paste").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Edit(EditCommand::ClipboardPaste),
                        data: None,
                    });
                }))
                .separator()
                .entry(MenuItem::new("Undo").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Edit(EditCommand::Undo),
                        data: None,
                    });
                }))
                .entry(MenuItem::new("Redo").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Edit(EditCommand::Redo),
                        data: None,
                    });
                }))
                .separator()
                .entry(MenuItem::new("Find").action(move || {
                    lapce_command.send(LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::Search),
                        data: None,
                    });
                })),
        )
}

fn fetch_grammars() -> Result<()> {
    let dir = Directory::grammars_directory()
        .ok_or_else(|| anyhow!("can't get grammars directory"))?;
    if !dir.exists() {
        let _ = std::fs::create_dir(&dir);
    }

    let url =
        "https://api.github.com/repos/lapce/tree-sitter-grammars/releases/latest";
    let resp = reqwest::blocking::ClientBuilder::new()
        .user_agent("Lapce")
        .build()?
        .get(url)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }
    let current_version =
        std::fs::read_to_string(dir.join("version")).unwrap_or_default();
    let release: ReleaseInfo = serde_json::from_str(&resp.text()?)?;
    if release.tag_name == current_version {
        return Ok(());
    }

    let file_name = format!(
        "grammars-{}-{}.zip",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    for asset in &release.assets {
        if asset.name == file_name {
            let mut resp = reqwest::blocking::get(&asset.browser_download_url)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }
            {
                let mut out = std::fs::File::create(dir.join(&file_name))?;
                resp.copy_to(&mut out)?;
            }

            let mut archive =
                zip::ZipArchive::new(std::fs::File::open(dir.join(&file_name))?)?;
            archive.extract(&dir)?;
            let _ = std::fs::remove_file(dir.join(&file_name));
            std::fs::write(dir.join("version"), release.tag_name)?;
            return Ok(());
        }
    }

    Err(anyhow!("can't find support grammars"))
}
