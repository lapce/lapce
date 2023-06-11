#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufReader, Read, Write},
    ops::Range,
    process::Stdio,
    sync::Arc,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use crossbeam_channel::Sender;
use floem::{
    cosmic_text::{Style as FontStyle, Weight},
    event::{Event, EventListener},
    ext_event::{create_ext_action, create_signal_from_channel},
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, on_cleanup, provide_context,
        use_context, ReadSignal, RwSignal, Scope, SignalGet, SignalGetUntracked,
        SignalSet, SignalUpdate, SignalWith, SignalWithUntracked,
    },
    style::{
        AlignItems, CursorStyle, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    view::View,
    views::{
        container, container_box, empty, label, list, scroll, stack, svg, tab,
        virtual_list, Decorators, VirtualListDirection, VirtualListItemSize,
        VirtualListVector,
    },
    window::WindowConfig,
    ViewContext,
};
use lapce_core::{directory::Directory, meta, mode::Mode};
use lapce_rpc::{
    core::{CoreMessage, CoreNotification},
    file::PathObject,
    RpcMessage,
};
use lsp_types::{CompletionItemKind, DiagnosticSeverity};
use notify::Watcher;
use serde::{Deserialize, Serialize};

use crate::{
    code_action::CodeActionStatus,
    command::{InternalCommand, WindowCommand},
    config::{
        color::LapceColor, icon::LapceIcons, watcher::ConfigWatcher, LapceConfig,
    },
    db::LapceDb,
    debug::RunDebugMode,
    doc::DocContent,
    editor::{
        location::{EditorLocation, EditorPosition},
        view::editor_container_view,
        EditorData,
    },
    editor_tab::{EditorTabChild, EditorTabData},
    focus_text::focus_text,
    id::{EditorId, EditorTabId, SplitId},
    listener::Listener,
    main_split::{MainSplitData, SplitContent, SplitData, SplitDirection},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData, PaletteStatus,
    },
    panel::{
        kind::PanelKind, position::PanelContainerPosition,
        view::panel_container_view,
    },
    settings::settings_view,
    text_input::text_input,
    title::title,
    update::ReleaseInfo,
    window::{TabsInfo, WindowData, WindowInfo},
    window_tab::{CommonData, Focus, WindowTabData},
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
}

#[derive(Clone)]
pub struct AppData {
    pub scope: Scope,
    pub windows: RwSignal<im::Vector<WindowData>>,
    pub window_scale: RwSignal<f64>,
    pub app_command: Listener<AppCommand>,
    /// The latest release information
    pub latest_release: RwSignal<Arc<Option<ReleaseInfo>>>,
    pub watcher: Arc<notify::RecommendedWatcher>,
}

impl AppData {
    pub fn reload_config(&self) {
        let windows = self.windows.get_untracked();
        for window in windows {
            window.reload_config();
        }
    }

    pub fn active_window_tab(&self) -> Option<Arc<WindowTabData>> {
        let windows = self.windows.get_untracked();
        if let Some(window) = windows.iter().next() {
            return window.active_window_tab();
        }
        None
    }

    pub fn run_app_command(&self, cmd: AppCommand) {
        match cmd {
            AppCommand::SaveApp => {
                let db: Arc<LapceDb> = use_context(self.scope).unwrap();
                let _ = db.save_app(self);
            }
        }
    }
}

fn editor_tab_header(
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    common: CommonData,
) -> impl View {
    let focus = common.focus;
    let config = common.config;
    let internal_command = common.internal_command;

    let items = move || {
        let editor_tab = editor_tab.get();
        for (i, (index, _)) in editor_tab.children.iter().enumerate() {
            if index.get_untracked() != i {
                index.set(i);
            }
        }
        editor_tab.children
    };
    let key = |(_, child): &(RwSignal<usize>, EditorTabChild)| child.id();
    let active = move || editor_tab.with(|editor_tab| editor_tab.active);
    let is_focused = move || {
        if let Focus::Workbench = focus.get() {
            editor_tab.with_untracked(|e| Some(e.editor_tab_id))
                == active_editor_tab.get()
        } else {
            false
        }
    };

    let view_fn = move |(i, child): (RwSignal<usize>, EditorTabChild)| {
        let local_child = child.clone();
        let child_for_close = child.clone();
        let child_view = move || {
            #[derive(PartialEq)]
            struct Info {
                icon: String,
                color: Option<Color>,
                path: String,
                confirmed: Option<RwSignal<bool>>,
                is_pristine: bool,
            }

            let cx = ViewContext::get_current();
            let info = match child {
                EditorTabChild::Editor(editor_id) => {
                    create_memo(cx.scope, move |_| {
                        let config = config.get();
                        let editor_data =
                            editors.with(|editors| editors.get(&editor_id).cloned());
                        let path = if let Some(editor_data) = editor_data {
                            let ((content, is_pristine), confirmed) = editor_data
                                .with(|editor_data| {
                                    (
                                        editor_data.doc.with(|doc| {
                                            (
                                                doc.content.clone(),
                                                doc.buffer().is_pristine(),
                                            )
                                        }),
                                        editor_data.confirmed,
                                    )
                                });
                            match content {
                                DocContent::File(path) => {
                                    Some((path, confirmed, is_pristine))
                                }
                                DocContent::Local => None,
                            }
                        } else {
                            None
                        };
                        let (icon, color, path, confirmed, is_pristine) = match path
                        {
                            Some((path, confirmed, is_pritine)) => {
                                let (svg, color) = config.file_svg(&path);
                                (
                                    svg,
                                    color.cloned(),
                                    path.file_name()
                                        .unwrap_or_default()
                                        .to_str()
                                        .unwrap_or_default()
                                        .to_string(),
                                    confirmed,
                                    is_pritine,
                                )
                            }
                            None => (
                                config.ui_svg(LapceIcons::FILE),
                                Some(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                ),
                                "local".to_string(),
                                create_rw_signal(cx.scope, true),
                                true,
                            ),
                        };
                        Info {
                            icon,
                            color,
                            path,
                            confirmed: Some(confirmed),
                            is_pristine,
                        }
                    })
                }
                EditorTabChild::Settings(_) => create_memo(cx.scope, move |_| {
                    let config = config.get();
                    Info {
                        icon: config.ui_svg(LapceIcons::SETTINGS),
                        color: Some(
                            *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                        ),
                        path: "Settings".to_string(),
                        confirmed: None,
                        is_pristine: true,
                    }
                }),
            };

            stack(|| {
                (
                    container(|| {
                        svg(move || info.with(|info| info.icon.clone())).style(
                            move || {
                                let size = config.get().ui.icon_size() as f32;
                                Style::BASE.size_px(size, size).apply_opt(
                                    info.with(|info| info.color),
                                    |s, c| s.color(c),
                                )
                            },
                        )
                    })
                    .style(|| Style::BASE.padding_horiz_px(10.0)),
                    label(move || info.with(|info| info.path.clone())).style(
                        move || {
                            Style::BASE.apply_if(
                                !info
                                    .with(|info| info.confirmed)
                                    .map(|confirmed| confirmed.get())
                                    .unwrap_or(true),
                                |s| s.font_style(FontStyle::Italic),
                            )
                        },
                    ),
                    clickable_icon(
                        move || {
                            if info.with(|info| info.is_pristine) {
                                LapceIcons::CLOSE
                            } else {
                                LapceIcons::UNSAVED
                            }
                        },
                        move || {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(
                                InternalCommand::EditorTabChildClose {
                                    editor_tab_id,
                                    child: child_for_close.clone(),
                                },
                            );
                        },
                        || false,
                        || false,
                        config,
                    )
                    .on_event(EventListener::PointerDown, |_| true)
                    .style(|| Style::BASE.margin_horiz_px(6.0)),
                )
            })
            .style(move || {
                Style::BASE
                    .items_center()
                    .border_left(if i.get() == 0 { 1.0 } else { 0.0 })
                    .border_right(1.0)
                    .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
            })
        };

        let confirmed = match local_child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                editor_data.map(|editor_data| editor_data.get().confirmed)
            }
            _ => None,
        };

        stack(|| {
            (
                container(child_view)
                    .on_double_click(move |_| {
                        if let Some(confirmed) = confirmed {
                            confirmed.set(true);
                        }
                        true
                    })
                    .on_event(EventListener::PointerDown, move |_| {
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = i.get_untracked();
                        });
                        false
                    })
                    .draggable()
                    .dragging_style(move || {
                        let config = config.get();
                        Style::BASE
                            .border(1.0)
                            .border_radius(6.0)
                            .background(
                                config
                                    .get_color(LapceColor::PANEL_BACKGROUND)
                                    .with_alpha_factor(0.7),
                            )
                            .border_color(
                                *config.get_color(LapceColor::LAPCE_BORDER),
                            )
                    })
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .height_pct(100.0)
                    }),
                container(|| {
                    empty().style(move || {
                        Style::BASE
                            .size_pct(100.0, 100.0)
                            .border_bottom(if active() == i.get() {
                                2.0
                            } else {
                                0.0
                            })
                            .border_color(*config.get().get_color(if is_focused() {
                                LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                            } else {
                                LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                            }))
                    })
                })
                .style(|| {
                    Style::BASE
                        .position(Position::Absolute)
                        .padding_horiz_px(3.0)
                        .size_pct(100.0, 100.0)
                }),
            )
        })
        .style(|| Style::BASE.height_pct(100.0))
    };

    stack(|| {
        (
            clickable_icon(
                || LapceIcons::TAB_PREVIOUS,
                || {},
                || false,
                || false,
                config,
            )
            .style(|| Style::BASE.margin_horiz_px(6.0).margin_vert_px(7.0)),
            clickable_icon(
                || LapceIcons::TAB_NEXT,
                || {},
                || false,
                || false,
                config,
            )
            .style(|| Style::BASE.margin_right_px(6.0)),
            container(|| {
                scroll(|| {
                    list(items, key, view_fn)
                        .style(|| Style::BASE.height_pct(100.0).items_center())
                })
                .hide_bar(|| true)
                .style(|| {
                    Style::BASE
                        .position(Position::Absolute)
                        .height_pct(100.0)
                        .max_width_pct(100.0)
                })
            })
            .style(|| {
                Style::BASE
                    .height_pct(100.0)
                    .flex_grow(1.0)
                    .flex_basis_px(0.0)
            }),
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
            .style(|| Style::BASE.margin_left_px(6.0)),
            clickable_icon(
                || LapceIcons::CLOSE,
                move || {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);
                    internal_command
                        .send(InternalCommand::EditorTabClose { editor_tab_id });
                },
                || false,
                || false,
                config,
            )
            .style(|| Style::BASE.margin_horiz_px(6.0)),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .items_center()
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
    })
}

fn editor_tab_content(
    main_split: MainSplitData,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let common = main_split.common.clone();
    let focus = common.focus;
    let items = move || {
        editor_tab
            .get()
            .children
            .into_iter()
            .map(|(_, child)| child)
    };
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |child| {
        let common = common.clone();
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                if let Some(editor_data) = editor_data {
                    let is_active = move || {
                        let focus = focus.get();
                        if let Focus::Workbench = focus {
                            let active_editor_tab = active_editor_tab.get();
                            let editor_tab =
                                editor_data.with(|editor| editor.editor_tab_id);
                            editor_tab.is_some() && editor_tab == active_editor_tab
                        } else {
                            false
                        }
                    };
                    container_box(|| {
                        Box::new(editor_container_view(
                            main_split.clone(),
                            workspace.clone(),
                            is_active,
                            editor_data,
                        ))
                    })
                } else {
                    container_box(|| Box::new(label(|| "emtpy editor".to_string())))
                }
            }
            EditorTabChild::Settings(_) => {
                container_box(move || Box::new(settings_view(common)))
            }
        };
        child.style(|| Style::BASE.size_pct(100.0, 100.0))
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(active, items, key, view_fn).style(|| Style::BASE.size_pct(100.0, 100.0))
}

fn editor_tab(
    main_split: MainSplitData,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let (editor_tab_id, editor_tab_scope) =
        editor_tab.with_untracked(|e| (e.editor_tab_id, e.scope));
    let editor_tabs = main_split.editor_tabs;
    on_cleanup(ViewContext::get_current().scope, move || {
        let exits =
            editor_tabs.with_untracked(|tabs| tabs.contains_key(&editor_tab_id));
        if !exits {
            let send = create_ext_action(editor_tab_scope, move |_| {
                editor_tab_scope.dispose();
            });
            std::thread::spawn(move || {
                send(());
            });
        }
    });

    let common = main_split.common.clone();
    let focus = common.focus;
    let internal_command = main_split.common.internal_command;
    stack(|| {
        (
            editor_tab_header(active_editor_tab, editor_tab, editors, common),
            editor_tab_content(
                main_split.clone(),
                workspace.clone(),
                active_editor_tab,
                editor_tab,
                editors,
            ),
        )
    })
    .on_event(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Workbench {
            focus.set(Focus::Workbench);
        }
        let editor_tab_id = editor_tab.with_untracked(|t| t.editor_tab_id);
        internal_command.send(InternalCommand::FocusEditorTab { editor_tab_id });
        false
    })
    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
}

fn split_border(
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let direction = move || split.with(|split| split.direction);
    list(
        move || split.get().children.into_iter().skip(1),
        |content| content.id(),
        move |content| {
            container(|| {
                empty().style(move || {
                    let direction = direction();
                    Style::BASE
                        .width(match direction {
                            SplitDirection::Vertical => Dimension::Points(1.0),
                            SplitDirection::Horizontal => Dimension::Percent(1.0),
                        })
                        .height(match direction {
                            SplitDirection::Vertical => Dimension::Percent(1.0),
                            SplitDirection::Horizontal => Dimension::Points(1.0),
                        })
                        .background(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                })
            })
            .style(move || {
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
                Style::BASE
                    .position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left_px(rect.x0 as f32 - 2.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top_px(rect.y0 as f32 - 2.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => Dimension::Points(4.0),
                        SplitDirection::Horizontal => Dimension::Percent(1.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => Dimension::Percent(1.0),
                        SplitDirection::Horizontal => Dimension::Points(4.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .justify_content(Some(JustifyContent::Center))
            })
        },
    )
    .style(|| {
        Style::BASE
            .position(Position::Absolute)
            .size_pct(100.0, 100.0)
    })
}

fn split_list(
    workspace: Arc<LapceWorkspace>,
    split: ReadSignal<SplitData>,
    main_split: MainSplitData,
) -> impl View {
    let editor_tabs = main_split.editor_tabs.read_only();
    let active_editor_tab = main_split.active_editor_tab.read_only();
    let editors = main_split.editors.read_only();
    let splits = main_split.splits.read_only();
    let config = main_split.common.config;
    let (split_id, split_scope) =
        split.with_untracked(|split| (split.split_id, split.scope));
    on_cleanup(ViewContext::get_current().scope, move || {
        let exits = splits.with_untracked(|splits| splits.contains_key(&split_id));
        if !exits {
            let send = create_ext_action(split_scope, move |_| {
                split_scope.dispose();
            });
            std::thread::spawn(move || {
                send(());
            });
        }
    });

    let direction = move || split.with(|split| split.direction);
    let items = move || split.get().children.into_iter().enumerate();
    let key = |(_index, content): &(usize, SplitContent)| content.id();
    let view_fn = move |(_index, content), main_split: MainSplitData| {
        let child = match &content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    editor_tabs.with(|tabs| tabs.get(editor_tab_id).cloned());
                if let Some(editor_tab_data) = editor_tab_data {
                    container_box(|| {
                        Box::new(editor_tab(
                            main_split.clone(),
                            workspace.clone(),
                            active_editor_tab,
                            editor_tab_data,
                            editors,
                        ))
                    })
                } else {
                    container_box(|| {
                        Box::new(label(|| "emtpy editor tab".to_string()))
                    })
                }
            }
            SplitContent::Split(split_id) => {
                if let Some(split) =
                    splits.with(|splits| splits.get(split_id).cloned())
                {
                    split_list(
                        workspace.clone(),
                        split.read_only(),
                        main_split.clone(),
                    )
                } else {
                    container_box(|| Box::new(label(|| "emtpy split".to_string())))
                }
            }
        };
        child
            .on_resize(move |window_origin, rect| match &content {
                SplitContent::EditorTab(editor_tab_id) => {
                    main_split.editor_tab_update_layout(
                        editor_tab_id,
                        window_origin,
                        rect,
                    );
                }
                SplitContent::Split(split_id) => {
                    let split_data =
                        splits.with(|splits| splits.get(split_id).cloned());
                    if let Some(split_data) = split_data {
                        split_data.update(|split| {
                            split.window_origin = window_origin;
                            split.layout_rect = rect;
                        });
                    }
                }
            })
            .style(move || {
                Style::BASE
                    .flex_grow(1.0)
                    .flex_basis(Dimension::Points(1.0))
            })
    };
    container_box(move || {
        Box::new(
            stack(move || {
                (
                    list(items, key, move |(index, content)| {
                        view_fn((index, content), main_split.clone())
                    })
                    .style(move || {
                        Style::BASE
                            .flex_direction(match direction() {
                                SplitDirection::Vertical => FlexDirection::Row,
                                SplitDirection::Horizontal => FlexDirection::Column,
                            })
                            .size_pct(100.0, 100.0)
                    }),
                    split_border(splits, editor_tabs, split, config),
                )
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0)),
        )
    })
}

fn main_split(window_tab_data: Arc<WindowTabData>) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get()
        .get(&root_split)
        .unwrap()
        .read_only();
    let config = window_tab_data.main_split.common.config;
    let workspace = window_tab_data.workspace.clone();
    let panel = window_tab_data.panel.clone();
    split_list(workspace, root_split, window_tab_data.main_split.clone()).style(
        move || {
            let config = config.get();
            let is_hidden = panel.panel_bottom_maximized(true)
                && panel.is_container_shown(&PanelContainerPosition::Bottom, true);
            Style::BASE
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
                .apply_if(is_hidden, |s| s.display(Display::None))
                .flex_grow(1.0)
        },
    )
}

pub fn clickable_icon(
    icon: impl Fn() -> &'static str + 'static,
    on_click: impl Fn() + 'static,
    active_fn: impl Fn() -> bool + 'static + Copy,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(|| {
        container(|| {
            svg(move || config.get().ui_svg(icon()))
                .style(move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::BASE
                        .size_px(size, size)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                })
                .disabled(disabled_fn)
                .disabled_style(move || {
                    Style::BASE
                        .color(
                            *config.get().get_color(LapceColor::LAPCE_ICON_INACTIVE),
                        )
                        .cursor(CursorStyle::Default)
                })
        })
        .on_click(move |_| {
            on_click();
            true
        })
        .disabled(disabled_fn)
        .style(move || {
            Style::BASE
                .padding_px(4.0)
                .border_radius(6.0)
                .border(1.0)
                .border_color(Color::TRANSPARENT)
                .apply_if(active_fn(), |s| {
                    s.border_color(*config.get().get_color(LapceColor::EDITOR_CARET))
                })
        })
        .hover_style(move || {
            Style::BASE.cursor(CursorStyle::Pointer).background(
                *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
            )
        })
        .active_style(move || {
            Style::BASE.background(
                *config
                    .get()
                    .get_color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
            )
        })
    })
}

fn workbench(window_tab_data: Arc<WindowTabData>) -> impl View {
    stack(move || {
        (
            panel_container_view(
                window_tab_data.clone(),
                PanelContainerPosition::Left,
            ),
            {
                let window_tab_data = window_tab_data.clone();
                stack(move || {
                    (
                        main_split(window_tab_data.clone()),
                        panel_container_view(
                            window_tab_data,
                            PanelContainerPosition::Bottom,
                        ),
                    )
                })
                .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
            },
            panel_container_view(window_tab_data, PanelContainerPosition::Right),
        )
    })
    .style(|| Style::BASE.size_pct(100.0, 100.0))
}

fn status(window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let panel = window_tab_data.panel.clone();
    let cx = ViewContext::get_current();
    let diagnostic_count = create_memo(cx.scope, move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.diagnostics.get().iter() {
                if let Some(severity) = diagnostic.diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });

    let mode = create_memo(cx.scope, move |_| window_tab_data.mode());

    stack(|| {
        (
            stack(|| {
                (
                    label(move || match mode.get() {
                        Mode::Normal => "Normal".to_string(),
                        Mode::Insert => "Insert".to_string(),
                        Mode::Visual => "Visual".to_string(),
                        Mode::Terminal => "Terminal".to_string(),
                    })
                    .style(move || {
                        let config = config.get();
                        let display = if config.core.modal {
                            Display::Flex
                        } else {
                            Display::None
                        };

                        let (bg, fg) = match mode.get() {
                            Mode::Normal => (
                                LapceColor::STATUS_MODAL_NORMAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_NORMAL_FOREGROUND,
                            ),
                            Mode::Insert => (
                                LapceColor::STATUS_MODAL_INSERT_BACKGROUND,
                                LapceColor::STATUS_MODAL_INSERT_FOREGROUND,
                            ),
                            Mode::Visual => (
                                LapceColor::STATUS_MODAL_VISUAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_VISUAL_FOREGROUND,
                            ),
                            Mode::Terminal => (
                                LapceColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                                LapceColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                            ),
                        };

                        let bg = *config.get_color(bg);
                        let fg = *config.get_color(fg);

                        Style::BASE
                            .display(display)
                            .padding_horiz_px(10.0)
                            .color(fg)
                            .background(bg)
                            .height_pct(100.0)
                            .align_items(Some(AlignItems::Center))
                    }),
                    {
                        let panel = panel.clone();
                        stack(|| {
                            (
                                svg(move || config.get().ui_svg(LapceIcons::ERROR))
                                    .style(move || {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        Style::BASE.size_px(size, size).color(
                                            *config.get_color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                    }),
                                label(move || diagnostic_count.get().0.to_string())
                                    .style(|| Style::BASE.margin_left_px(5.0)),
                                svg(move || {
                                    config.get().ui_svg(LapceIcons::WARNING)
                                })
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    Style::BASE
                                        .size_px(size, size)
                                        .margin_left_px(5.0)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                }),
                                label(move || diagnostic_count.get().1.to_string())
                                    .style(|| Style::BASE.margin_left_px(5.0)),
                            )
                        })
                        .on_click(move |_| {
                            panel.show_panel(&PanelKind::Problem);
                            true
                        })
                        .style(|| {
                            Style::BASE
                                .height_pct(100.0)
                                .padding_horiz_px(10.0)
                                .items_center()
                        })
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    },
                )
            })
            .style(|| {
                Style::BASE
                    .height_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
                    .items_center()
            }),
            stack(move || {
                (
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Left,
                                    true,
                                ) {
                                    LapceIcons::SIDEBAR_LEFT
                                } else {
                                    LapceIcons::SIDEBAR_LEFT_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Left,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Bottom,
                                    true,
                                ) {
                                    LapceIcons::LAYOUT_PANEL
                                } else {
                                    LapceIcons::LAYOUT_PANEL_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Bottom,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                    {
                        let panel = panel.clone();
                        let icon = {
                            let panel = panel.clone();
                            move || {
                                if panel.is_container_shown(
                                    &PanelContainerPosition::Right,
                                    true,
                                ) {
                                    LapceIcons::SIDEBAR_RIGHT
                                } else {
                                    LapceIcons::SIDEBAR_RIGHT_OFF
                                }
                            }
                        };
                        clickable_icon(
                            icon,
                            move || {
                                panel.toggle_container_visual(
                                    &PanelContainerPosition::Right,
                                )
                            },
                            || false,
                            || false,
                            config,
                        )
                    },
                )
            })
            .style(|| Style::BASE.height_pct(100.0).items_center()),
            label(|| "".to_string()).style(|| {
                Style::BASE
                    .height_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
            }),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .border_top(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::STATUS_BACKGROUND))
            .height_px(config.ui.status_height() as f32)
            .align_items(Some(AlignItems::Center))
    })
}

fn palette_item(
    workspace: Arc<LapceWorkspace>,
    i: usize,
    item: PaletteItem,
    index: ReadSignal<usize>,
    palette_item_height: f64,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    match &item.content {
        PaletteItemContent::File { path, .. }
        | PaletteItemContent::Reference { path, .. } => {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            // let (file_name, _) = create_signal(cx.scope, file_name);
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
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
            container_box(move || {
                Box::new(
                    stack(move || {
                        (
                            svg(move || config.get().file_svg(&path).0).style(
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color =
                                        config.file_svg(&style_path).1.copied();
                                    Style::BASE
                                        .min_width_px(size)
                                        .size_px(size, size)
                                        .margin_right_px(5.0)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            focus_text(
                                move || file_name.clone(),
                                move || file_name_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(|| {
                                Style::BASE.margin_right_px(6.0).max_width_pct(100.0)
                            }),
                            focus_text(
                                move || folder.clone(),
                                move || folder_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(move || {
                                Style::BASE
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                        )
                    })
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(100.0)
                    }),
                )
            })
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
            container_box(move || {
                Box::new(
                    stack(move || {
                        (
                            svg(move || {
                                let config = config.get();
                                config.symbol_svg(&kind).unwrap_or_else(|| {
                                    config.ui_svg(LapceIcons::FILE)
                                })
                            })
                            .style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .size_px(size, size)
                                    .margin_right_px(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(|| {
                                Style::BASE.margin_right_px(6.0).max_width_pct(100.0)
                            }),
                            focus_text(
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(move || {
                                Style::BASE
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                        )
                    })
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(100.0)
                    }),
                )
            })
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
            container_box(move || {
                Box::new(
                    stack(move || {
                        (
                            svg(move || {
                                let config = config.get();
                                config.symbol_svg(&kind).unwrap_or_else(|| {
                                    config.ui_svg(LapceIcons::FILE)
                                })
                            })
                            .style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .size_px(size, size)
                                    .margin_right_px(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(|| {
                                Style::BASE.margin_right_px(6.0).max_width_pct(100.0)
                            }),
                            focus_text(
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(move || {
                                Style::BASE
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                        )
                    })
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(100.0)
                    }),
                )
            })
        }
        PaletteItemContent::RunAndDebug {
            mode,
            config: run_config,
        } => {
            let mode = *mode;
            let text = format!("{mode} {}", run_config.name);
            let hint =
                format!("{} {}", run_config.program, run_config.args.join(" "));
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
            container_box(move || {
                Box::new(
                    stack(move || {
                        (
                            svg(move || {
                                let config = config.get();
                                match mode {
                                    RunDebugMode::Run => {
                                        config.ui_svg(LapceIcons::START)
                                    }
                                    RunDebugMode::Debug => {
                                        config.ui_svg(LapceIcons::DEBUG)
                                    }
                                }
                            })
                            .style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .size_px(size, size)
                                    .margin_right_px(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(|| {
                                Style::BASE.margin_right_px(6.0).max_width_pct(100.0)
                            }),
                            focus_text(
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(move || {
                                Style::BASE
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                        )
                    })
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(100.0)
                    }),
                )
            })
        }
        PaletteItemContent::Command { .. }
        | PaletteItemContent::Line { .. }
        | PaletteItemContent::Workspace { .. }
        | PaletteItemContent::SshHost { .. }
        | PaletteItemContent::ColorTheme { .. }
        | PaletteItemContent::IconTheme { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container_box(move || {
                Box::new(
                    focus_text(
                        move || text.clone(),
                        move || indices.clone(),
                        move || *config.get().get_color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(|| {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(100.0)
                    }),
                )
            })
        }
    }
    .style(move || {
        Style::BASE
            .width_pct(100.0)
            .height_px(palette_item_height as f32)
            .padding_horiz_px(10.0)
            .apply_if(index.get() == i, |style| {
                style.background(
                    *config
                        .get()
                        .get_color(LapceColor::PALETTE_CURRENT_BACKGROUND),
                )
            })
    })
}

fn palette_input(window_tab_data: Arc<WindowTabData>) -> impl View {
    let editor = window_tab_data.palette.input_editor.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_focused = move || focus.get() == Focus::Palette;
    container(move || {
        container(move || {
            text_input(editor, is_focused).style(|| Style::BASE.width_pct(100.0))
        })
        .style(move || {
            let config = config.get();
            Style::BASE
                .width_pct(100.0)
                .height_px(25.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
        })
    })
    .style(|| Style::BASE.padding_bottom_px(5.0))
}

struct PaletteItems(im::Vector<PaletteItem>);

impl VirtualListVector<(usize, PaletteItem)> for PaletteItems {
    type ItemIterator = Box<dyn Iterator<Item = (usize, PaletteItem)>>;

    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator {
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
    window_tab_data: Arc<WindowTabData>,
    layout_rect: ReadSignal<Rect>,
) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let index = window_tab_data.palette.index.read_only();
    let clicked_index = window_tab_data.palette.clicked_index.write_only();
    let config = window_tab_data.common.config;
    let run_id = window_tab_data.palette.run_id;
    let input = window_tab_data.palette.input.read_only();
    let palette_item_height = 25.0;
    let workspace = window_tab_data.workspace.clone();
    stack(move || {
        (
            scroll(move || {
                let workspace = workspace.clone();
                virtual_list(
                    VirtualListDirection::Vertical,
                    VirtualListItemSize::Fixed(Box::new(move || {
                        palette_item_height
                    })),
                    move || PaletteItems(items.get()),
                    move |(i, _item)| {
                        (run_id.get_untracked(), *i, input.get_untracked().input)
                    },
                    move |(i, item)| {
                        let workspace = workspace.clone();
                        container(move || {
                            palette_item(
                                workspace,
                                i,
                                item,
                                index,
                                palette_item_height,
                                config,
                            )
                        })
                        .on_click(move |_| {
                            clicked_index.set(Some(i));
                            true
                        })
                        .style(|| {
                            Style::BASE.width_pct(100.0).cursor(CursorStyle::Pointer)
                        })
                        .hover_style(move || {
                            Style::BASE.background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    },
                )
                .style(|| Style::BASE.width_pct(100.0).flex_col())
            })
            .scroll_bar_color(move || {
                *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
            })
            .on_ensure_visible(move || {
                Size::new(1.0, palette_item_height).to_rect().with_origin(
                    Point::new(0.0, index.get() as f64 * palette_item_height),
                )
            })
            .style(|| Style::BASE.width_pct(100.0).min_height_px(0.0)),
            label(|| "No matching results".to_string()).style(move || {
                Style::BASE
                    .display(if items.with(|items| items.is_empty()) {
                        Display::Flex
                    } else {
                        Display::None
                    })
                    .padding_horiz_px(10.0)
                    .align_items(Some(AlignItems::Center))
                    .height_px(palette_item_height as f32)
            }),
        )
    })
    .style(move || {
        Style::BASE
            .flex_col()
            .width_pct(100.0)
            .min_height_px(0.0)
            .max_height_px((layout_rect.get().height() * 0.45 - 36.0).round() as f32)
            .padding_bottom_px(5.0)
            .padding_bottom_px(5.0)
    })
}

fn palette_preview(palette_data: PaletteData) -> impl View {
    let workspace = palette_data.workspace.clone();
    let preview_editor = palette_data.preview_editor;
    let has_preview = palette_data.has_preview;
    let config = palette_data.common.config;
    let main_split = palette_data.main_split;
    container(|| {
        container(|| {
            editor_container_view(main_split, workspace, || true, preview_editor)
        })
        .style(move || {
            let config = config.get();
            Style::BASE
                .position(Position::Absolute)
                .border_top(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .size_pct(100.0, 100.0)
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
        })
    })
    .style(move || {
        Style::BASE
            .display(if has_preview.get() {
                Display::Flex
            } else {
                Display::None
            })
            .flex_grow(1.0)
    })
}

fn palette(window_tab_data: Arc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.common.config;
    let has_preview = palette_data.has_preview.read_only();
    container(|| {
        stack(|| {
            (
                palette_input(window_tab_data.clone()),
                palette_content(window_tab_data.clone(), layout_rect),
                palette_preview(palette_data),
            )
        })
        .on_event(EventListener::PointerDown, move |_| true)
        .style(move || {
            let config = config.get();
            Style::BASE
                .width_px(500.0)
                .max_width_pct(90.0)
                .max_height(if has_preview.get() {
                    Dimension::Auto
                } else {
                    Dimension::Percent(1.0)
                })
                .height(if has_preview.get() {
                    Dimension::Points(layout_rect.get().height() as f32 - 10.0)
                } else {
                    Dimension::Auto
                })
                .margin_top_px(5.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .flex_col()
                .background(*config.get_color(LapceColor::PALETTE_BACKGROUND))
        })
    })
    .style(move || {
        Style::BASE
            .display(if status.get() == PaletteStatus::Inactive {
                Display::None
            } else {
                Display::Flex
            })
            .position(Position::Absolute)
            .size_pct(100.0, 100.0)
            .flex_col()
            .items_center()
    })
}

struct VectorItems<V>(im::Vector<V>);

impl<V: Clone + 'static> VirtualListVector<(usize, V)> for VectorItems<V> {
    type ItemIterator = Box<dyn Iterator<Item = (usize, V)>>;

    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator {
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

fn completion(window_tab_data: Arc<WindowTabData>) -> impl View {
    let completion_data = window_tab_data.common.completion;
    let config = window_tab_data.common.config;
    let active = completion_data.with_untracked(|c| c.active);
    let request_id =
        move || completion_data.with_untracked(|c| (c.request_id, c.input_id));
    scroll(move || {
        virtual_list(
            VirtualListDirection::Vertical,
            VirtualListItemSize::Fixed(Box::new(move || {
                config.get().editor.line_height() as f64
            })),
            move || completion_data.with(|c| VectorItems(c.filtered_items.clone())),
            move |(i, _item)| (request_id(), *i),
            move |(i, item)| {
                stack(|| {
                    (
                        container(move || {
                            label(move || {
                                item.item
                                    .kind
                                    .map(completion_kind_to_str)
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .style(move || {
                                Style::BASE
                                    .width_pct(100.0)
                                    .justify_content(Some(JustifyContent::Center))
                            })
                        })
                        .style(move || {
                            let config = config.get();
                            Style::BASE
                                .width_px(config.editor.line_height() as f32)
                                .height_pct(100.0)
                                .align_items(Some(AlignItems::Center))
                                .font_weight(Weight::BOLD)
                                .apply_opt(
                                    config.completion_color(item.item.kind),
                                    |s, c| {
                                        s.color(c)
                                            .background(c.with_alpha_factor(0.3))
                                    },
                                )
                        }),
                        focus_text(
                            move || item.item.label.clone(),
                            move || item.indices.clone(),
                            move || {
                                *config.get().get_color(LapceColor::EDITOR_FOCUS)
                            },
                        )
                        .style(move || {
                            let config = config.get();
                            Style::BASE
                                .padding_horiz_px(5.0)
                                .align_items(Some(AlignItems::Center))
                                .size_pct(100.0, 100.0)
                                .apply_if(active.get() == i, |s| {
                                    s.background(
                                        *config.get_color(
                                            LapceColor::COMPLETION_CURRENT,
                                        ),
                                    )
                                })
                        }),
                    )
                })
                .style(move || {
                    Style::BASE
                        .align_items(Some(AlignItems::Center))
                        .width_pct(100.0)
                        .height_px(config.get().editor.line_height() as f32)
                })
            },
        )
        .style(|| {
            Style::BASE
                .align_items(Some(AlignItems::Center))
                .width_pct(100.0)
                .flex_col()
        })
    })
    .scroll_bar_color(move || *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR))
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
    .on_resize(move |_, rect| {
        completion_data.update(|c| {
            c.layout_rect = rect;
        });
    })
    .style(move || {
        let config = config.get();
        let origin = window_tab_data.completion_origin();
        Style::BASE
            .position(Position::Absolute)
            .width_px(400.0)
            .max_height_px(400.0)
            .margin_left_px(origin.x as f32)
            .margin_top_px(origin.y as f32)
            .background(*config.get_color(LapceColor::COMPLETION_BACKGROUND))
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size() as f32)
            .border_radius(10.0)
    })
}

fn code_action(window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let code_action = window_tab_data.code_action;
    let (status, active) = code_action
        .with_untracked(|code_action| (code_action.status, code_action.active));
    let request_id =
        move || code_action.with_untracked(|code_action| code_action.request_id);
    scroll(move || {
        container(|| {
            list(
                move || {
                    code_action.with(|code_action| {
                        code_action.filtered_items.clone().into_iter().enumerate()
                    })
                },
                move |(i, _item)| (request_id(), *i),
                move |(i, item)| {
                    container(move || {
                        label(move || item.title().replace('\n', " "))
                            .style(|| Style::BASE.text_ellipsis().min_width_px(0.0))
                    })
                    .style(move || {
                        let config = config.get();
                        Style::BASE
                            .padding_horiz_px(10.0)
                            .align_items(Some(AlignItems::Center))
                            .min_width_px(0.0)
                            .width_pct(100.0)
                            .line_height(1.6)
                            .apply_if(active.get() == i, |s| {
                                s.border_radius(6.0).background(
                                    *config
                                        .get_color(LapceColor::COMPLETION_CURRENT),
                                )
                            })
                    })
                },
            )
            .style(|| Style::BASE.width_pct(100.0).flex_col())
        })
        .style(|| Style::BASE.width_pct(100.0).padding_vert_px(4.0))
    })
    .scroll_bar_color(move || *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR))
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
    .on_resize(move |_, rect| {
        code_action.update(|c| {
            c.layout_rect = rect;
        });
    })
    .style(move || {
        let origin = window_tab_data.code_action_origin();
        Style::BASE
            .display(match status.get() {
                CodeActionStatus::Inactive => Display::None,
                CodeActionStatus::Active => Display::Flex,
            })
            .position(Position::Absolute)
            .width_px(400.0)
            .max_height_px(400.0)
            .margin_left_px(origin.x as f32)
            .margin_top_px(origin.y as f32)
            .background(*config.get().get_color(LapceColor::COMPLETION_BACKGROUND))
            .border_radius(10.0)
    })
}

fn rename(window_tab_data: Arc<WindowTabData>) -> impl View {
    let editor = window_tab_data.rename.editor.clone();
    let active = window_tab_data.rename.active;
    let layout_rect = window_tab_data.rename.layout_rect;
    let config = window_tab_data.common.config;

    container(|| {
        container(move || {
            text_input(editor, move || active.get())
                .style(|| Style::BASE.width_px(150.0))
        })
        .style(move || {
            let config = config.get();
            Style::BASE
                .font_family(config.editor.font_family.clone())
                .font_size(config.editor.font_size() as f32)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
        })
    })
    .on_resize(move |_, rect| {
        layout_rect.set(rect);
    })
    .style(move || {
        let origin = window_tab_data.rename_origin();
        Style::BASE
            .position(Position::Absolute)
            .apply_if(!active.get(), |s| s.hide())
            .margin_left_px(origin.x as f32)
            .margin_top_px(origin.y as f32)
            .background(*config.get().get_color(LapceColor::PANEL_BACKGROUND))
            .border_radius(6.0)
            .padding_px(6.0)
    })
}

pub fn dispose_on_ui_cleanup(scope: Scope) {
    on_cleanup(ViewContext::get_current().scope, move || {
        let send = create_ext_action(scope, move |_| {
            scope.dispose();
        });
        std::thread::spawn(move || {
            send(());
        });
    });
}

fn window_tab(window_tab_data: Arc<WindowTabData>) -> impl View {
    dispose_on_ui_cleanup(window_tab_data.scope);
    let source_control = window_tab_data.source_control.clone();
    let window_origin = window_tab_data.window_origin;
    let layout_rect = window_tab_data.layout_rect;
    let latest_release = window_tab_data.latest_release;
    let update_in_progress = window_tab_data.update_in_progress;
    let config = window_tab_data.common.config;
    let workspace = window_tab_data.workspace.clone();
    let workbench_command = window_tab_data.common.workbench_command;
    let main_split = window_tab_data.main_split.clone();

    let view = stack(|| {
        (
            stack(|| {
                (
                    title(
                        workspace,
                        main_split,
                        source_control,
                        workbench_command,
                        latest_release,
                        update_in_progress,
                        config,
                    ),
                    workbench(window_tab_data.clone()),
                    status(window_tab_data.clone()),
                )
            })
            .on_resize(move |point, rect| {
                window_origin.set(point);
                layout_rect.set(rect);
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0).flex_col()),
            completion(window_tab_data.clone()),
            code_action(window_tab_data.clone()),
            rename(window_tab_data.clone()),
            palette(window_tab_data.clone()),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .size_pct(100.0, 100.0)
            .color(*config.get_color(LapceColor::EDITOR_FOREGROUND))
            .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            .font_size(config.ui.font_size() as f32)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
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
        LapceWorkspaceType::RemoteSSH(ssh) => format!("{dir} [{ssh}]"),
        #[cfg(windows)]
        LapceWorkspaceType::RemoteWSL => format!("{dir} [wsl]"),
    })
}

fn workspace_tab_header(window_data: WindowData) -> impl View {
    let tabs = window_data.window_tabs;
    let active = window_data.active;
    let config = window_data.config;
    let cx = ViewContext::get_current();
    let available_width = create_rw_signal(cx.scope, 0.0);
    let add_icon_width = create_rw_signal(cx.scope, 0.0);

    let tab_width = create_memo(cx.scope, move |_| {
        let available_width = available_width.get() - add_icon_width.get();
        let tabs_len = tabs.with(|tabs| tabs.len());
        if tabs_len > 0 {
            (available_width / tabs_len as f64).min(200.0)
        } else {
            available_width
        }
    });

    let local_window_data = window_data.clone();
    stack(|| {
        (
            list(
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
                move |(index, tab)| {
                    container(|| {
                        stack(|| {
                            (
                                stack(|| {
                                    let window_data = local_window_data.clone();
                                    (
                                        label(move || {
                                            workspace_title(&tab.workspace)
                                                .unwrap_or_else(|| {
                                                    String::from("New Tab")
                                                })
                                        })
                                        .style(|| {
                                            Style::BASE
                                                .margin_left_px(10.0)
                                                .min_width_px(0.0)
                                                .flex_basis_px(0.0)
                                                .flex_grow(1.0)
                                                .text_ellipsis()
                                        }),
                                        clickable_icon(
                                            || LapceIcons::WINDOW_CLOSE,
                                            move || {
                                                window_data.run_window_command(
                                                WindowCommand::CloseWorkspaceTab {
                                                    index: Some(
                                                        index.get_untracked(),
                                                    ),
                                                },
                                            );
                                            },
                                            || false,
                                            || false,
                                            config.read_only(),
                                        )
                                        .style(|| Style::BASE.margin_horiz_px(6.0)),
                                    )
                                })
                                .style(move || {
                                    let config = config.get();
                                    Style::BASE
                                        .width_pct(100.0)
                                        .min_width_px(0.0)
                                        .items_center()
                                        .border_right(1.0)
                                        .border_color(
                                            *config
                                                .get_color(LapceColor::LAPCE_BORDER),
                                        )
                                }),
                                container(|| {
                                    label(|| "".to_string()).style(move || {
                                        Style::BASE
                                        .size_pct(100.0, 100.0)
                                        .apply_if(active.get() == index.get(), |s| {
                                            s.border_bottom(2.0)
                                        })
                                        .border_color(*config.get().get_color(
                                            LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE,
                                        ))
                                    })
                                })
                                .style(|| {
                                    Style::BASE
                                        .position(Position::Absolute)
                                        .padding_horiz_px(3.0)
                                        .size_pct(100.0, 100.0)
                                }),
                            )
                        })
                        .style(move || {
                            Style::BASE.size_pct(100.0, 100.0).items_center()
                        })
                    })
                    .on_click(move |_| {
                        active.set(index.get_untracked());
                        true
                    })
                    .style(move || {
                        Style::BASE
                            .height_pct(100.0)
                            .width_px(tab_width.get() as f32)
                    })
                },
            )
            .style(|| Style::BASE.height_pct(100.0)),
            clickable_icon(
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
            )
            .on_resize(move |_, rect| {
                let current = add_icon_width.get_untracked();
                if rect.width() != current {
                    add_icon_width.set(rect.width());
                }
            })
            .style(|| Style::BASE.padding_left_px(10.0).padding_right_px(20.0)),
        )
    })
    .on_resize(move |_, rect| {
        let current = available_width.get_untracked();
        if rect.width() != current {
            available_width.set(rect.width());
        }
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .border_bottom(1.0)
            .width_pct(100.0)
            .height_px(37.0)
            .font_size(config.ui.font_size() as f32)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
            .apply_if(tabs.with(|tabs| tabs.len() < 2), |s| s.hide())
            .color(*config.get_color(LapceColor::EDITOR_FOREGROUND))
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            .items_center()
    })
}

fn window(window_data: WindowData) -> impl View {
    let window_tabs = window_data.window_tabs.read_only();
    let active = window_data.active.read_only();
    let items = move || window_tabs.get();
    let key = |(_, window_tab): &(RwSignal<usize>, Arc<WindowTabData>)| {
        window_tab.window_tab_id
    };
    let active = move || active.get();

    tab(active, items, key, |(_, window_tab_data)| {
        window_tab(window_tab_data)
    })
    .style(|| Style::BASE.size_pct(100.0, 100.0))
}

fn app_view(window_data: WindowData) -> impl View {
    // let window_data = WindowData::new(cx);
    let window_size = window_data.size;
    let position = window_data.position;
    let window_scale = window_data.window_scale;
    stack(|| {
        (
            workspace_tab_header(window_data.clone()),
            window(window_data.clone()),
        )
    })
    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
    .window_scale(move || window_scale.get())
    .keyboard_navigatable()
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            window_data.key_down(key_event);
            true
        } else {
            false
        }
    })
    .on_event(EventListener::WindowResized, move |event| {
        if let Event::WindowResized(size) = event {
            window_size.set(*size);
        }
        true
    })
    .on_event(EventListener::WindowMoved, move |event| {
        if let Event::WindowMoved(point) = event {
            position.set(*point);
        }
        true
    })
}

pub fn launch() {
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
                log::error!("failed to open path(s): {e}");
            };
            return;
        }
    }

    #[cfg(feature = "updater")]
    crate::update::cleanup();

    let _ = lapce_proxy::register_lapce_path();
    let db = Arc::new(LapceDb::new().unwrap());
    let mut app = floem::Application::new();
    let scope = app.scope();
    provide_context(scope, db.clone());

    let window_scale = create_rw_signal(scope, 1.0);
    let latest_release = create_rw_signal(scope, Arc::new(None));
    let app_command = Listener::new_empty(scope);

    let mut windows = im::Vector::new();

    app = create_windows(
        scope,
        db.clone(),
        app,
        cli.paths,
        &mut windows,
        window_scale,
        latest_release.read_only(),
        app_command,
    );

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

    let windows = create_rw_signal(scope, windows);
    let app_data = AppData {
        scope,
        windows,
        window_scale,
        watcher: Arc::new(watcher),
        latest_release,
        app_command,
    };

    {
        let app_data = app_data.clone();
        let notification = create_signal_from_channel(scope, rx);
        create_effect(scope, move |_| {
            if notification.get().is_some() {
                app_data.reload_config();
            }
        });
    }

    #[cfg(feature = "updater")]
    {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let notification = create_signal_from_channel(scope, rx);
        let latest_release = app_data.latest_release;
        create_effect(scope, move |_| {
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
        let notification = create_signal_from_channel(scope, rx);
        let app_data = app_data.clone();
        create_effect(scope, move |_| {
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
            let _ = db.insert_app(app_data.clone());
        }
    })
    .run();
}

#[allow(clippy::too_many_arguments)]
fn create_windows(
    scope: floem::reactive::Scope,
    db: Arc<LapceDb>,
    mut app: floem::Application,
    paths: Vec<PathObject>,
    windows: &mut im::Vector<WindowData>,
    window_scale: RwSignal<f64>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    app_command: Listener<AppCommand>,
) -> floem::Application {
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
                LapceWorkspaceType::RemoteWSL
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

            let config = WindowConfig::default().size(info.size).position(info.pos);
            let window_data = WindowData::new(
                scope,
                info,
                window_scale,
                latest_release,
                app_command,
            );
            windows.push_back(window_data.clone());
            app = app.window(move || app_view(window_data), Some(config));
        }
    } else if files.is_empty() {
        // There were no dirs and no files specified, so we'll load the last windows
        if let Ok(app_info) = db.get_app() {
            for info in app_info.windows {
                let config =
                    WindowConfig::default().size(info.size).position(info.pos);
                let window_data = WindowData::new(
                    scope,
                    info,
                    window_scale,
                    latest_release,
                    app_command,
                );
                windows.push_back(window_data.clone());
                app = app.window(move || app_view(window_data), Some(config));
            }
        }
    }

    if windows.is_empty() {
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
        let config = WindowConfig::default().size(info.size).position(info.pos);
        let window_data =
            WindowData::new(scope, info, window_scale, latest_release, app_command);
        windows.push_back(window_data.clone());
        app = app.window(|| app_view(window_data), Some(config));
    }

    // Open any listed files in the first window
    if let Some(window) = windows.iter().next() {
        let cur_window_tab = window.active.get_untracked();
        let (_, window_tab) = &window.window_tabs.get_untracked()[cur_window_tab];
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
                    log::trace!("Unhandled message: {msg:?}");
                }

                let stream_ref = reader.get_mut();
                let _ = stream_ref.write_all(b"received");
                let _ = stream_ref.flush();
            }
        });
    }
    Ok(())
}
