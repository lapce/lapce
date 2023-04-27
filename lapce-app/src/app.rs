use std::{ops::Range, sync::Arc};

use floem::{
    cosmic_text::{Attrs, AttrsList, Style as FontStyle, TextLayout, Weight},
    event::{Event, EventListner},
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_memo, create_rw_signal, provide_context, ReadSignal, RwSignal,
        SignalGet, SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
        SignalWithUntracked,
    },
    style::{
        AlignItems, CursorStyle, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    view::View,
    views::{
        click, container, container_box, double_click, label, list, scroll, stack,
        svg, tab, virtual_list, Decorators, VirtualListDirection,
        VirtualListItemSize, VirtualListVector,
    },
    window::WindowConfig,
    AppContext,
};
use lapce_core::mode::Mode;
use lapce_rpc::{
    dap_types::{DapId, ThreadId},
    terminal::TermId,
};
use lsp_types::{CompletionItemKind, DiagnosticSeverity};
use serde::{Deserialize, Serialize};

use crate::{
    code_action::CodeActionStatus,
    command::{InternalCommand, WindowCommand},
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    db::LapceDb,
    debug::{RunDebugMode, StackTraceData},
    doc::DocContent,
    editor::{
        location::{EditorLocation, EditorPosition},
        view::editor_view,
        EditorData,
    },
    editor_tab::{EditorTabChild, EditorTabData},
    focus_text::focus_text,
    id::{EditorId, EditorTabId, SplitId},
    main_split::{MainSplitData, SplitContent, SplitData, SplitDirection},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData, PaletteStatus,
    },
    panel::{
        kind::PanelKind,
        position::{PanelContainerPosition, PanelPosition},
    },
    terminal::{
        panel::TerminalPanelData, tab::TerminalTabData, view::terminal_view,
    },
    title::title,
    window::{TabsInfo, WindowData, WindowInfo},
    window_tab::{Focus, WindowTabData},
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub windows: Vec<WindowInfo>,
}

#[derive(Clone)]
pub struct AppData {
    pub windows: RwSignal<im::Vector<WindowData>>,
}

fn editor_tab_header(
    cx: AppContext,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
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

    let view_fn = move |cx, (i, child): (RwSignal<usize>, EditorTabChild)| {
        let local_child = child.clone();
        let child_view = move |cx: AppContext| match child {
            EditorTabChild::Editor(editor_id) => {
                #[derive(PartialEq)]
                struct Info {
                    icon: String,
                    color: Option<Color>,
                    path: String,
                    confirmed: RwSignal<bool>,
                    is_pristine: bool,
                }

                let info = create_memo(cx.scope, move |_| {
                    let config = config.get();
                    let editor_data =
                        editors.with(|editors| editors.get(&editor_id).cloned());
                    let path = if let Some(editor_data) = editor_data {
                        let ((content, is_pristine), confirmed) =
                            editor_data.with(|editor_data| {
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
                    let (icon, color, path, confirmed, is_pristine) = match path {
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
                            Some(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE)),
                            "local".to_string(),
                            create_rw_signal(cx.scope, true),
                            true,
                        ),
                    };
                    Info {
                        icon,
                        color,
                        path,
                        confirmed,
                        is_pristine,
                    }
                });

                stack(cx, |cx| {
                    (
                        container(cx, |cx| {
                            svg(cx, move || info.with(|info| info.icon.clone()))
                                .style(cx, move || {
                                    let size = config.get().ui.icon_size() as f32;
                                    Style::BASE.dimension_px(size, size).apply_opt(
                                        info.with(|info| info.color),
                                        |s, c| s.color(c),
                                    )
                                })
                        })
                        .style(cx, || Style::BASE.padding_horiz(10.0)),
                        label(cx, move || info.with(|info| info.path.clone()))
                            .style(cx, move || {
                                Style::BASE.apply_if(
                                    !info.with(|info| info.confirmed).get(),
                                    |s| s.font_style(FontStyle::Italic),
                                )
                            }),
                        container(cx, |cx| {
                            svg(cx, move || {
                                config.get().ui_svg(
                                    if info.with(|info| info.is_pristine) {
                                        LapceIcons::CLOSE
                                    } else {
                                        LapceIcons::UNSAVED
                                    },
                                )
                            })
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE.dimension_px(size, size).color(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                )
                            })
                        })
                        .style(cx, || {
                            Style::BASE
                                .border_radius(6.0)
                                .padding(4.0)
                                .margin_horiz(6.0)
                                .cursor(CursorStyle::Pointer)
                        })
                        .hover_style(cx, move || {
                            Style::BASE.background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        }),
                    )
                })
                .style(cx, move || {
                    Style::BASE
                        .align_items(Some(AlignItems::Center))
                        .border_left(if i.get() == 0 { 1.0 } else { 0.0 })
                        .border_right(1.0)
                        .border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                })
            }
        };

        let confirmed = match local_child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                editor_data.map(|editor_data| editor_data.get().confirmed)
            }
        };

        stack(cx, |cx| {
            (
                click(
                    cx,
                    |cx| {
                        double_click(cx, child_view, move || {
                            if let Some(confirmed) = confirmed {
                                confirmed.set(true);
                            }
                        })
                        .style(cx, move || {
                            Style::BASE
                                .align_items(Some(AlignItems::Center))
                                .height_pct(1.0)
                        })
                    },
                    move || {
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = i.get_untracked();
                        });
                    },
                )
                .style(cx, move || {
                    Style::BASE
                        .align_items(Some(AlignItems::Center))
                        .height_pct(1.0)
                }),
                container(cx, |cx| {
                    label(cx, || "".to_string()).style(cx, move || {
                        Style::BASE
                            .dimension_pct(1.0, 1.0)
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
                .style(cx, || {
                    Style::BASE
                        .position(Position::Absolute)
                        .padding_horiz(3.0)
                        .dimension_pct(1.0, 1.0)
                }),
            )
        })
        .style(cx, || Style::BASE.height_pct(1.0))
    };

    stack(cx, |cx| {
        (
            clickable_icon(cx, LapceIcons::TAB_PREVIOUS, || {}, || false, config)
                .style(cx, || Style::BASE.margin_horiz(6.0)),
            clickable_icon(cx, LapceIcons::TAB_NEXT, || {}, || false, config)
                .style(cx, || Style::BASE.margin_right(6.0)),
            container(cx, |cx| {
                scroll(cx, |cx| {
                    list(cx, items, key, view_fn)
                        .style(cx, || Style::BASE.height_pct(1.0).items_center())
                })
                .hide_bar(cx, || true)
                .style(cx, || {
                    Style::BASE
                        .position(Position::Absolute)
                        .height_pct(1.0)
                        .max_width_pct(1.0)
                })
            })
            .style(cx, || {
                Style::BASE
                    .height_pct(1.0)
                    .flex_grow(1.0)
                    .flex_basis_px(0.0)
            }),
            clickable_icon(
                cx,
                LapceIcons::SPLIT_HORIZONTAL,
                || {},
                || false,
                config,
            )
            .style(cx, || Style::BASE.margin_left(6.0)),
            clickable_icon(cx, LapceIcons::CLOSE, || {}, || false, config)
                .style(cx, || Style::BASE.margin_horiz(6.0)),
        )
    })
    .style(cx, move || {
        let config = config.get();
        Style::BASE
            .height_px(config.ui.header_height() as f32)
            .items_center()
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
    })
}

fn editor_tab_content(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
) -> impl View {
    let items = move || {
        editor_tab
            .get()
            .children
            .into_iter()
            .map(|(_, child)| child)
    };
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |cx, child| {
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
                    container_box(cx, |cx| {
                        Box::new(editor_view(
                            cx,
                            workspace.clone(),
                            is_active,
                            editor_data,
                        ))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(cx, active, items, key, view_fn)
        .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
}

fn editor_tab(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack(cx, |cx| {
        (
            editor_tab_header(
                cx,
                active_editor_tab,
                editor_tab,
                editors,
                focus,
                config,
            ),
            editor_tab_content(
                cx,
                workspace.clone(),
                active_editor_tab,
                editor_tab,
                editors,
                focus,
            ),
        )
    })
    .style(cx, || Style::BASE.flex_col().dimension_pct(1.0, 1.0))
}

fn split_border(
    cx: AppContext,
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let direction = move || split.with(|split| split.direction);
    list(
        cx,
        move || split.get().children.into_iter().skip(1),
        |content| content.id(),
        move |cx, content| {
            container(cx, |cx| {
                label(cx, || "".to_string()).style(cx, move || {
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
            .style(cx, move || {
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
                        style.margin_left(rect.x0 as f32 - 1.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 1.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => Dimension::Points(5.0),
                        SplitDirection::Horizontal => Dimension::Percent(1.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => Dimension::Percent(1.0),
                        SplitDirection::Horizontal => Dimension::Points(5.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .justify_content(Some(JustifyContent::Center))
            })
        },
    )
    .style(cx, || {
        Style::BASE
            .position(Position::Absolute)
            .dimension_pct(1.0, 1.0)
    })
}

fn split_list(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    split: ReadSignal<SplitData>,
    main_split: MainSplitData,
) -> impl View {
    let editor_tabs = main_split.editor_tabs.read_only();
    let active_editor_tab = main_split.active_editor_tab.read_only();
    let editors = main_split.editors.read_only();
    let splits = main_split.splits.read_only();
    let config = main_split.common.config;
    let focus = main_split.common.focus.read_only();

    let direction = move || split.with(|split| split.direction);
    let items = move || split.get().children.into_iter().enumerate();
    let key = |(_index, content): &(usize, SplitContent)| content.id();
    let view_fn = move |cx, (_index, content), main_split: MainSplitData| {
        let child = match &content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    editor_tabs.with(|tabs| tabs.get(editor_tab_id).cloned());
                if let Some(editor_tab_data) = editor_tab_data {
                    container_box(cx, |cx| {
                        Box::new(editor_tab(
                            cx,
                            workspace.clone(),
                            active_editor_tab,
                            editor_tab_data,
                            editors,
                            focus,
                            config,
                        ))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor tab".to_string()))
                    })
                }
            }
            SplitContent::Split(split_id) => {
                if let Some(split) =
                    splits.with(|splits| splits.get(split_id).cloned())
                {
                    split_list(
                        cx,
                        workspace.clone(),
                        split.read_only(),
                        main_split.clone(),
                    )
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy split".to_string()))
                    })
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
            .style(cx, move || {
                Style::BASE
                    .flex_grow(1.0)
                    .flex_basis(Dimension::Points(1.0))
            })
    };
    container_box(cx, move |cx| {
        Box::new(
            stack(cx, move |cx| {
                (
                    list(cx, items, key, move |cx, (index, content)| {
                        view_fn(cx, (index, content), main_split.clone())
                    })
                    .style(cx, move || {
                        Style::BASE
                            .flex_direction(match direction() {
                                SplitDirection::Vertical => FlexDirection::Row,
                                SplitDirection::Horizontal => FlexDirection::Column,
                            })
                            .dimension_pct(1.0, 1.0)
                    }),
                    split_border(cx, splits, editor_tabs, split, config),
                )
            })
            .style(cx, || Style::BASE.dimension_pct(1.0, 1.0)),
        )
    })
}

fn main_split(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
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
    split_list(
        cx,
        workspace,
        root_split,
        window_tab_data.main_split.clone(),
    )
    .style(cx, move || {
        let is_hidden = panel.panel_bottom_maximized(true)
            && panel.is_container_shown(&PanelContainerPosition::Bottom, true);
        Style::BASE
            .background(*config.get().get_color(LapceColor::EDITOR_BACKGROUND))
            .apply_if(is_hidden, |s| s.display(Display::None))
            .flex_grow(1.0)
    })
}

fn terminal_tab_header(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let active = move || terminal.tab_info.with(|info| info.active);

    list(
        cx,
        move || {
            let tabs = terminal.tab_info.with(|info| info.tabs.clone());
            for (i, (index, _)) in tabs.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            tabs
        },
        |(_, tab)| tab.terminal_tab_id,
        move |cx, (index, tab)| {
            let title = {
                let tab = tab.clone();
                move || {
                    let terminal = tab.active_terminal(true);
                    let run_debug = terminal.as_ref().map(|t| t.run_debug);
                    if let Some(run_debug) = run_debug {
                        if let Some(name) = run_debug.with(|run_debug| {
                            run_debug.as_ref().map(|r| r.config.name.clone())
                        }) {
                            return name;
                        }
                    }

                    let title = terminal.map(|t| t.title);
                    let title = title.map(|t| t.get());
                    title.unwrap_or_default()
                }
            };

            let svg_string = move || {
                let terminal = tab.active_terminal(true);
                let run_debug = terminal.as_ref().map(|t| t.run_debug);
                if let Some(run_debug) = run_debug {
                    if let Some((mode, stopped)) = run_debug.with(|run_debug| {
                        run_debug.as_ref().map(|r| (r.mode, r.stopped))
                    }) {
                        let svg = match (mode, stopped) {
                            (RunDebugMode::Run, false) => LapceIcons::START,
                            (RunDebugMode::Run, true) => LapceIcons::RUN_ERRORS,
                            (RunDebugMode::Debug, false) => LapceIcons::DEBUG,
                            (RunDebugMode::Debug, true) => {
                                LapceIcons::DEBUG_DISCONNECT
                            }
                        };
                        return svg;
                    }
                }
                LapceIcons::TERMINAL
            };
            stack(cx, |cx| {
                (
                    container(cx, |cx| {
                        stack(cx, |cx| {
                            (
                                container(cx, |cx| {
                                    svg(cx, move || {
                                        config.get().ui_svg(svg_string())
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::BASE
                                                .dimension_px(size, size)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        },
                                    )
                                })
                                .style(cx, || Style::BASE.padding_horiz(10.0)),
                                label(cx, title).style(cx, || {
                                    Style::BASE
                                        .min_width_px(0.0)
                                        .flex_basis_px(0.0)
                                        .flex_grow(1.0)
                                }),
                                clickable_icon(
                                    cx,
                                    LapceIcons::CLOSE,
                                    || {},
                                    || false,
                                    config,
                                )
                                .style(cx, || Style::BASE.margin_horiz(6.0)),
                            )
                        })
                        .style(cx, move || {
                            Style::BASE
                                .items_center()
                                .width_px(200.0)
                                .border_right(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                    })
                    .style(cx, || Style::BASE.items_center()),
                    container(cx, |cx| {
                        label(cx, || "".to_string()).style(cx, move || {
                            Style::BASE
                                .dimension_pct(1.0, 1.0)
                                .border_bottom(if active() == index.get() {
                                    2.0
                                } else {
                                    0.0
                                })
                                .border_color(*config.get().get_color(
                                    if focus.get()
                                        == Focus::Panel(PanelKind::Terminal)
                                    {
                                        LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                                    } else {
                                        LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                                    },
                                ))
                        })
                    })
                    .style(cx, || {
                        Style::BASE
                            .position(Position::Absolute)
                            .padding_horiz(3.0)
                            .dimension_pct(1.0, 1.0)
                    }),
                )
            })
        },
    )
    .style(cx, move || {
        let config = config.get();
        Style::BASE
            .height_px(config.ui.header_height() as f32)
            .width_pct(1.0)
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
    })
}

pub fn clickable_icon(
    cx: AppContext,
    icon: &'static str,
    on_click: impl Fn() + 'static,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(cx, |cx| {
        click(
            cx,
            |cx| {
                svg(cx, move || config.get().ui_svg(icon))
                    .style(cx, move || {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        Style::BASE
                            .dimension_px(size, size)
                            .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                    })
                    .disabled(cx, disabled_fn)
                    .disabled_style(cx, move || {
                        Style::BASE
                            .color(
                                *config
                                    .get()
                                    .get_color(LapceColor::LAPCE_ICON_INACTIVE),
                            )
                            .cursor(CursorStyle::Default)
                    })
            },
            on_click,
        )
        .disabled(cx, disabled_fn)
        .style(cx, || {
            Style::BASE
                .padding(4.0)
                .border_radius(6.0)
                .cursor(CursorStyle::Pointer)
        })
        .hover_style(cx, move || {
            Style::BASE.background(
                *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
            )
        })
        .active_style(cx, move || {
            Style::BASE.background(
                *config
                    .get()
                    .get_color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
            )
        })
    })
}

fn terminal_tab_split(
    cx: AppContext,
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
) -> impl View {
    let config = terminal_panel_data.common.config;
    list(
        cx,
        move || {
            let terminals = terminal_tab_data.terminals.get();
            for (i, (index, _)) in terminals.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            terminals
        },
        |(_, terminal)| terminal.term_id,
        move |cx, (index, terminal)| {
            let focus = terminal.common.focus;
            let terminal_panel_data = terminal_panel_data.clone();
            click(
                cx,
                move |cx| {
                    terminal_view(
                        cx,
                        terminal.term_id,
                        terminal.raw.read_only(),
                        terminal.mode.read_only(),
                        terminal.run_debug.read_only(),
                        terminal_panel_data,
                    )
                    .on_event(EventListner::MouseWheel, move |event| {
                        if let Event::MouseWheel(mouse_event) = event {
                            terminal.clone().wheel_scroll(mouse_event.wheel_delta.y);
                            true
                        } else {
                            false
                        }
                    })
                    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
                },
                move || {
                    focus.set(Focus::Panel(PanelKind::Terminal));
                },
            )
            .style(cx, move || {
                Style::BASE
                    .dimension_pct(1.0, 1.0)
                    .padding_horiz(10.0)
                    .apply_if(index.get() > 0, |s| {
                        s.border_left(1.0).border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                    })
            })
        },
    )
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
}

fn terminal_tab_content(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        cx,
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |cx, (_, tab)| terminal_tab_split(cx, terminal.clone(), tab),
    )
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
}

fn blank_panel(cx: AppContext) -> impl View {
    label(cx, || "blank".to_string())
}

fn terminal_panel(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    stack(cx, |cx| {
        (
            terminal_tab_header(cx, window_tab_data.clone()),
            terminal_tab_content(cx, window_tab_data),
        )
    })
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0).flex_col())
}

fn panel_header(
    cx: AppContext,
    header: String,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(cx, |cx| label(cx, move || header.clone())).style(cx, move || {
        Style::BASE
            .padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(1.0)
            .background(*config.get().get_color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn debug_process_icons(
    cx: AppContext,
    terminal: TerminalPanelData,
    term_id: TermId,
    dap_id: DapId,
    mode: RunDebugMode,
    stopped: bool,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let paused = move || {
        let stopped = terminal
            .debug
            .daps
            .with_untracked(|daps| daps.get(&dap_id).map(|dap| dap.stopped));
        stopped.map(|stopped| stopped.get()).unwrap_or(false)
    };
    match mode {
        RunDebugMode::Run => container_box(cx, |cx| {
            Box::new(stack(cx, |cx| {
                (
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_RESTART,
                            move || {
                                terminal.restart_run_debug(term_id);
                            },
                            || false,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_horiz(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_STOP,
                            move || {
                                terminal.stop_run_debug(term_id);
                            },
                            move || stopped,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::CLOSE,
                            move || {
                                terminal.close_terminal(&term_id);
                            },
                            || false,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                )
            }))
        }),
        RunDebugMode::Debug => container_box(cx, |cx| {
            Box::new(stack(cx, |cx| {
                (
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_CONTINUE,
                            move || {
                                terminal.dap_continue(term_id);
                            },
                            move || !paused() || stopped,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_horiz(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_PAUSE,
                            move || {
                                terminal.dap_pause(term_id);
                            },
                            move || paused() || stopped,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_RESTART,
                            move || {
                                terminal.restart_run_debug(term_id);
                            },
                            || false,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::DEBUG_STOP,
                            move || {
                                terminal.stop_run_debug(term_id);
                            },
                            move || stopped,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                    {
                        let terminal = terminal.clone();
                        clickable_icon(
                            cx,
                            LapceIcons::CLOSE,
                            move || {
                                terminal.close_terminal(&term_id);
                            },
                            || false,
                            config,
                        )
                        .style(cx, || Style::BASE.margin_right(6.0))
                    },
                )
            }))
        }),
    }
}

fn debug_processes(
    cx: AppContext,
    terminal: TerminalPanelData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    scroll(cx, move |cx| {
        let terminal = terminal.clone();
        let local_terminal = terminal.clone();
        list(
            cx,
            move || local_terminal.run_debug_process(true),
            |(term_id, p)| (*term_id, p.stopped),
            move |cx, (term_id, p)| {
                let terminal = terminal.clone();
                let is_active =
                    move || terminal.debug.active_term.get() == Some(term_id);
                let local_terminal = terminal.clone();
                click(
                    cx,
                    |cx| {
                        stack(cx, move |cx| {
                            (
                                {
                                    let svg_str = match (&p.mode, p.stopped) {
                                        (RunDebugMode::Run, false) => {
                                            LapceIcons::START
                                        }
                                        (RunDebugMode::Run, true) => {
                                            LapceIcons::RUN_ERRORS
                                        }
                                        (RunDebugMode::Debug, false) => {
                                            LapceIcons::DEBUG
                                        }
                                        (RunDebugMode::Debug, true) => {
                                            LapceIcons::DEBUG_DISCONNECT
                                        }
                                    };
                                    svg(cx, move || config.get().ui_svg(svg_str))
                                        .style(cx, move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::BASE
                                                .dimension_px(size, size)
                                                .margin_horiz(10.0)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        })
                                },
                                label(cx, move || p.config.name.clone()).style(
                                    cx,
                                    || {
                                        Style::BASE
                                            .flex_grow(1.0)
                                            .flex_basis_px(0.0)
                                            .min_width_px(0.0)
                                    },
                                ),
                                debug_process_icons(
                                    cx,
                                    terminal.clone(),
                                    term_id,
                                    p.config.dap_id,
                                    p.mode,
                                    p.stopped,
                                    config,
                                ),
                            )
                        })
                        .style(cx, move || {
                            Style::BASE
                                .padding_vert(6.0)
                                .width_pct(1.0)
                                .items_center()
                                .apply_if(is_active(), |s| {
                                    s.background(*config.get().get_color(
                                        LapceColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                })
                        })
                        .hover_style(cx, move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                (*config.get().get_color(
                                    LapceColor::PANEL_HOVERED_BACKGROUND,
                                ))
                                .with_alpha_factor(0.3),
                            )
                        })
                    },
                    move || {
                        local_terminal.debug.active_term.set(Some(term_id));
                        local_terminal.focus_terminal(term_id);
                    },
                )
            },
        )
        .style(cx, || Style::BASE.width_pct(1.0).flex_col())
    })
}

fn debug_stack_frames(
    cx: AppContext,
    thread_id: ThreadId,
    stack_trace: StackTraceData,
    stopped: RwSignal<bool>,
    internal_command: RwSignal<Option<InternalCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let expanded = stack_trace.expanded;
    stack(cx, move |cx| {
        (
            click(
                cx,
                |cx| label(cx, move || thread_id.to_string()),
                move || {
                    expanded.update(|expanded| {
                        *expanded = !*expanded;
                    });
                },
            )
            .style(cx, || Style::BASE.padding_horiz(10.0).min_width_pct(1.0))
            .hover_style(cx, move || {
                Style::BASE.cursor(CursorStyle::Pointer).background(
                    *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                )
            }),
            list(
                cx,
                move || {
                    let expanded = stack_trace.expanded.get() && stopped.get();
                    if expanded {
                        stack_trace.frames.get()
                    } else {
                        im::Vector::new()
                    }
                },
                |frame| frame.id,
                move |cx, frame| {
                    let full_path =
                        frame.source.as_ref().and_then(|s| s.path.clone());
                    let line = frame.line.saturating_sub(1);
                    let col = frame.column.saturating_sub(1);

                    let source_path = frame
                        .source
                        .as_ref()
                        .and_then(|s| s.path.as_ref())
                        .and_then(|p| p.file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let has_source = !source_path.is_empty();
                    let source_path = format!("{source_path}:{}", frame.line);

                    click(
                        cx,
                        |cx| {
                            stack(cx, |cx| {
                                (
                                    label(cx, move || frame.name.clone())
                                        .hover_style(cx, move || {
                                            Style::BASE
                                                .background(*config.get().get_color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            ))
                                        }),
                                    label(cx, move || source_path.clone()).style(
                                        cx,
                                        move || {
                                            Style::BASE
                                                .margin_left(10.0)
                                                .color(*config.get().get_color(
                                                    LapceColor::EDITOR_DIM,
                                                ))
                                                .font_style(FontStyle::Italic)
                                                .apply_if(!has_source, |s| {
                                                    s.display(Display::None)
                                                })
                                        },
                                    ),
                                )
                            })
                        },
                        move || {
                            if let Some(path) = full_path.clone() {
                                internal_command.set(Some(
                                    InternalCommand::JumpToLocation {
                                        location: EditorLocation {
                                            path,
                                            position: Some(
                                                EditorPosition::Position(
                                                    lsp_types::Position {
                                                        line: line as u32,
                                                        character: col as u32,
                                                    },
                                                ),
                                            ),
                                            scroll_offset: None,
                                            ignore_unconfirmed: false,
                                            same_editor_tab: false,
                                        },
                                    },
                                ));
                            }
                        },
                    )
                    .style(cx, move || {
                        Style::BASE
                            .padding_left(20.0)
                            .padding_right(10.0)
                            .min_width_pct(1.0)
                            .apply_if(!has_source, |s| {
                                s.color(
                                    *config.get().get_color(LapceColor::EDITOR_DIM),
                                )
                            })
                    })
                    .hover_style(cx, move || {
                        Style::BASE
                            .background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                            .apply_if(has_source, |s| s.cursor(CursorStyle::Pointer))
                    })
                },
            )
            .style(cx, || Style::BASE.flex_col().min_width_pct(1.0)),
        )
    })
    .style(cx, || Style::BASE.flex_col().min_width_pct(1.0))
}

fn debug_stack_traces(
    cx: AppContext,
    terminal: TerminalPanelData,
    internal_command: RwSignal<Option<InternalCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(cx, move |cx| {
        scroll(cx, move |cx| {
            let local_terminal = terminal.clone();
            list(
                cx,
                move || {
                    let dap = local_terminal.get_active_dap(true);
                    if let Some(dap) = dap {
                        let process_stopped = local_terminal
                            .get_terminal(&dap.term_id)
                            .and_then(|t| {
                                t.run_debug.with(|r| r.as_ref().map(|r| r.stopped))
                            })
                            .unwrap_or(true);
                        if process_stopped {
                            return Vec::new();
                        }
                        let main_thread = dap.thread_id.get();
                        let stack_traces = dap.stack_traces.get();
                        let mut traces = stack_traces
                            .into_iter()
                            .map(|(thread_id, stack_trace)| {
                                (dap.dap_id, dap.stopped, thread_id, stack_trace)
                            })
                            .collect::<Vec<_>>();
                        traces.sort_by_key(|(_, _, id, _)| main_thread != Some(*id));
                        traces
                    } else {
                        Vec::new()
                    }
                },
                |(dap_id, stopped, thread_id, _)| {
                    (*dap_id, *thread_id, stopped.get_untracked())
                },
                move |cx, (_, stopped, thread_id, stack_trace)| {
                    debug_stack_frames(
                        cx,
                        thread_id,
                        stack_trace,
                        stopped,
                        internal_command,
                        config,
                    )
                },
            )
            .style(cx, || Style::BASE.flex_col().min_width_pct(1.0))
        })
        .scroll_bar_color(cx, move || {
            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
        })
        .style(cx, || Style::BASE.absolute().dimension_pct(1.0, 1.0))
    })
    .style(cx, || {
        Style::BASE
            .width_pct(1.0)
            .line_height(1.6)
            .flex_grow(1.0)
            .flex_basis_px(0.0)
    })
}

fn debug_panel(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let terminal = window_tab_data.terminal.clone();
    let internal_command = window_tab_data.common.internal_command;

    stack(cx, move |cx| {
        (
            {
                let terminal = terminal.clone();
                stack(cx, move |cx| {
                    (
                        panel_header(cx, "Processes".to_string(), config),
                        debug_processes(cx, terminal, config),
                    )
                })
                .style(cx, || {
                    Style::BASE.width_pct(1.0).flex_col().height_px(200.0)
                })
            },
            stack(cx, move |cx| {
                (
                    panel_header(cx, "Stack Frames".to_string(), config),
                    debug_stack_traces(cx, terminal, internal_command, config),
                )
            })
            .style(cx, || {
                Style::BASE
                    .width_pct(1.0)
                    .flex_grow(1.0)
                    .flex_basis_px(0.0)
                    .flex_col()
            }),
        )
    })
    .style(cx, move || {
        Style::BASE
            .width_pct(1.0)
            .apply_if(!position.is_bottom(), |s| s.flex_col())
    })
}

fn panel_view(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        cx,
        active_fn,
        panels,
        |p| *p,
        move |cx, kind| {
            let view = match kind {
                PanelKind::Terminal => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::FileExplorer => container_box(cx, |cx| {
                    Box::new(debug_panel(cx, window_tab_data.clone(), position))
                }),
                PanelKind::SourceControl => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Plugin => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Search => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Problem => {
                    container_box(cx, |cx| Box::new(blank_panel(cx)))
                }
                PanelKind::Debug => container_box(cx, |cx| {
                    Box::new(debug_panel(cx, window_tab_data.clone(), position))
                }),
            };
            view.style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
        },
    )
    .style(cx, move || {
        Style::BASE
            .dimension_pct(1.0, 1.0)
            .apply_if(!panel.is_position_shown(&position, true), |s| {
                s.display(Display::None)
            })
    })
}

fn panel_container_view(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelContainerPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    stack(cx, |cx| {
        (
            panel_view(cx, window_tab_data.clone(), position.first()),
            panel_view(cx, window_tab_data, position.second()),
        )
    })
    .style(cx, move || {
        let size = panel.size.with(|s| match position {
            PanelContainerPosition::Left => s.left,
            PanelContainerPosition::Bottom => s.bottom,
            PanelContainerPosition::Right => s.right,
        });
        let is_maximized = panel.panel_bottom_maximized(true);
        let config = config.get();
        Style::BASE
            .apply_if(!panel.is_container_shown(&position, true), |s| {
                s.display(Display::None)
            })
            .apply_if(position == PanelContainerPosition::Bottom, |s| {
                s.border_top(1.0)
                    .width_pct(1.0)
                    .apply_if(!is_maximized, |s| s.height_px(size as f32))
                    .apply_if(is_maximized, |s| s.flex_grow(1.0))
            })
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.border_right(1.0)
                    .width_px(size as f32)
                    .height_pct(1.0)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.border_left(1.0)
                    .width_px(size as f32)
                    .height_pct(1.0)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            })
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .color(*config.get_color(LapceColor::PANEL_FOREGROUND))
    })
}

fn workbench(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.main_split.common.config;
    stack(cx, move |cx| {
        (
            panel_container_view(
                cx,
                window_tab_data.clone(),
                PanelContainerPosition::Left,
            ),
            stack(cx, move |cx| {
                (
                    main_split(cx, window_tab_data.clone()),
                    panel_container_view(
                        cx,
                        window_tab_data,
                        PanelContainerPosition::Bottom,
                    ),
                )
            })
            .style(cx, || Style::BASE.flex_col().dimension_pct(1.0, 1.0)),
            label(cx, move || "right".to_string()).style(cx, move || {
                let config = config.get();
                Style::BASE
                    .padding(20.0)
                    .border_left(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .min_width_px(0.0)
                    .display(Display::None)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            }),
        )
    })
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
}

fn status(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let diagnostic_count = create_memo(cx.scope, move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.iter() {
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

    stack(cx, |cx| {
        (
            label(cx, move || match mode.get() {
                Mode::Normal => "Normal".to_string(),
                Mode::Insert => "Insert".to_string(),
                Mode::Visual => "Visual".to_string(),
                Mode::Terminal => "Terminal".to_string(),
            })
            .style(cx, move || {
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
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(1.0)
                    .align_items(Some(AlignItems::Center))
            }),
            svg(cx, move || config.get().ui_svg(LapceIcons::ERROR)).style(
                cx,
                move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::BASE
                        .dimension_px(size, size)
                        .margin_left(10.0)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                },
            ),
            label(cx, move || diagnostic_count.get().0.to_string())
                .style(cx, || Style::BASE.margin_left(5.0)),
            svg(cx, move || config.get().ui_svg(LapceIcons::WARNING)).style(
                cx,
                move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::BASE
                        .dimension_px(size, size)
                        .margin_left(5.0)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                },
            ),
            label(cx, move || diagnostic_count.get().1.to_string())
                .style(cx, || Style::BASE.margin_left(5.0)),
        )
    })
    .style(cx, move || {
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
    cx: AppContext,
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
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || config.get().file_svg(&path).0).style(
                                cx,
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color =
                                        config.file_svg(&style_path).1.copied();
                                    Style::BASE
                                        .min_width_px(size)
                                        .dimension_px(size, size)
                                        .margin_right(5.0)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            focus_text(
                                cx,
                                move || file_name.clone(),
                                move || file_name_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::BASE.margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || folder.clone(),
                                move || folder_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
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
                    .style(cx, || {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
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
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || {
                                let config = config.get();
                                config.symbol_svg(&kind).unwrap_or_else(|| {
                                    config.ui_svg(LapceIcons::FILE)
                                })
                            })
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .dimension_px(size, size)
                                    .margin_right(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                cx,
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::BASE.margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
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
                    .style(cx, || {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
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
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || {
                                let config = config.get();
                                config.symbol_svg(&kind).unwrap_or_else(|| {
                                    config.ui_svg(LapceIcons::FILE)
                                })
                            })
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .dimension_px(size, size)
                                    .margin_right(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                cx,
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::BASE.margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
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
                    .style(cx, || {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
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
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || {
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
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE
                                    .min_width_px(size)
                                    .dimension_px(size, size)
                                    .margin_right(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                cx,
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::BASE.margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
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
                    .style(cx, || {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
                    }),
                )
            })
        }
        PaletteItemContent::Command { .. }
        | PaletteItemContent::Line { .. }
        | PaletteItemContent::Workspace { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container_box(cx, move |cx| {
                Box::new(
                    focus_text(
                        cx,
                        move || text.clone(),
                        move || indices.clone(),
                        move || *config.get().get_color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(cx, || {
                        Style::BASE
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
                    }),
                )
            })
        }
    }
    .style(cx, move || {
        Style::BASE
            .width_pct(1.0)
            .height_px(palette_item_height as f32)
            .padding_horiz(10.0)
            .apply_if(index.get() == i, |style| {
                style.background(
                    *config
                        .get()
                        .get_color(LapceColor::PALETTE_CURRENT_BACKGROUND),
                )
            })
    })
}

fn palette_input(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let doc = window_tab_data.palette.input_editor.doc.read_only();
    let cursor = window_tab_data.palette.input_editor.cursor.read_only();
    let config = window_tab_data.common.config;
    let cursor_x = create_memo(cx.scope, move |_| {
        let offset = cursor.get().offset();
        let config = config.get();
        doc.with(|doc| {
            let (_, col) = doc.buffer().offset_to_line_col(offset);

            let line_content = doc.buffer().line_content(0);

            let families = config.ui.font_family();
            let attrs = Attrs::new()
                .font_size(config.ui.font_size() as f32)
                .family(&families);
            let attrs_list = AttrsList::new(attrs);
            let mut text_layout = TextLayout::new();
            text_layout.set_text(&line_content[..col], attrs_list);

            text_layout.size().width as f32
        })
    });
    container(cx, move |cx| {
        container(cx, move |cx| {
            scroll(cx, move |cx| {
                stack(cx, move |cx| {
                    (
                        label(cx, move || {
                            doc.with(|doc| doc.buffer().text().to_string())
                        }),
                        label(cx, move || "".to_string()).style(cx, move || {
                            Style::BASE
                                .position(Position::Absolute)
                                .width_px(2.0)
                                .height_pct(1.0)
                                .margin_left(cursor_x.get() - 0.5)
                                .background(
                                    *config
                                        .get()
                                        .get_color(LapceColor::EDITOR_CARET),
                                )
                        }),
                    )
                })
            })
            .scroll_bar_color(cx, move || {
                *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
            })
            .on_ensure_visible(cx, move || {
                Size::new(20.0, 0.0)
                    .to_rect()
                    .with_origin(Point::new(cursor_x.get() as f64 - 10.0, 0.0))
            })
            .style(cx, || {
                Style::BASE.min_width_px(0.0).height_px(24.0).items_center()
            })
        })
        .style(cx, move || {
            let config = config.get();
            Style::BASE
                .width_pct(1.0)
                .min_width_px(0.0)
                .border_bottom(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
                .padding_horiz(10.0)
                .cursor(CursorStyle::Text)
        })
    })
    .style(cx, || Style::BASE.padding_bottom(5.0))
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
    cx: AppContext,
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
    stack(cx, move |cx| {
        (
            scroll(cx, move |cx| {
                let workspace = workspace.clone();
                virtual_list(
                    cx,
                    VirtualListDirection::Vertical,
                    move || PaletteItems(items.get()),
                    move |(i, _item)| {
                        (run_id.get_untracked(), *i, input.get_untracked().input)
                    },
                    move |cx, (i, item)| {
                        let workspace = workspace.clone();
                        click(
                            cx,
                            move |cx| {
                                palette_item(
                                    cx,
                                    workspace,
                                    i,
                                    item,
                                    index,
                                    palette_item_height,
                                    config,
                                )
                            },
                            move || {
                                clicked_index.set(Some(i));
                            },
                        )
                        .style(cx, || {
                            Style::BASE.width_pct(1.0).cursor(CursorStyle::Pointer)
                        })
                        .hover_style(cx, move || {
                            Style::BASE.background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    },
                    VirtualListItemSize::Fixed(palette_item_height),
                )
                .style(cx, || Style::BASE.width_pct(1.0).flex_col())
            })
            .scroll_bar_color(cx, move || {
                *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
            })
            .on_ensure_visible(cx, move || {
                Size::new(1.0, palette_item_height).to_rect().with_origin(
                    Point::new(0.0, index.get() as f64 * palette_item_height),
                )
            })
            .style(cx, || Style::BASE.width_pct(1.0).min_height_px(0.0)),
            label(cx, || "No matching results".to_string()).style(cx, move || {
                Style::BASE
                    .display(if items.with(|items| items.is_empty()) {
                        Display::Flex
                    } else {
                        Display::None
                    })
                    .padding_horiz(10.0)
                    .align_items(Some(AlignItems::Center))
                    .height_px(palette_item_height as f32)
            }),
        )
    })
    .style(cx, move || {
        Style::BASE
            .flex_col()
            .width_pct(1.0)
            .min_height_px(0.0)
            .max_height_px((layout_rect.get().height() * 0.45 - 36.0).round() as f32)
            .padding_bottom(5.0)
            .padding_bottom(5.0)
    })
}

fn palette_preview(cx: AppContext, palette_data: PaletteData) -> impl View {
    let workspace = palette_data.workspace.clone();
    let preview_editor = palette_data.preview_editor;
    let has_preview = palette_data.has_preview;
    let config = palette_data.common.config;
    container(cx, |cx| {
        container(cx, |cx| editor_view(cx, workspace, || true, preview_editor))
            .style(cx, move || {
                let config = config.get();
                Style::BASE
                    .position(Position::Absolute)
                    .border_top(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .dimension_pct(1.0, 1.0)
                    .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            })
    })
    .style(cx, move || {
        Style::BASE
            .display(if has_preview.get() {
                Display::Flex
            } else {
                Display::None
            })
            .flex_grow(1.0)
    })
}

fn palette(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.common.config;
    let has_preview = palette_data.has_preview.read_only();
    container(cx, |cx| {
        stack(cx, |cx| {
            (
                palette_input(cx, window_tab_data.clone()),
                palette_content(cx, window_tab_data.clone(), layout_rect),
                palette_preview(cx, palette_data),
            )
        })
        .style(cx, move || {
            let config = config.get();
            Style::BASE
                .width_px(500.0)
                .max_width_pct(0.9)
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
                .margin_top(5.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .flex_col()
                .background(*config.get_color(LapceColor::PALETTE_BACKGROUND))
        })
    })
    .style(cx, move || {
        Style::BASE
            .display(if status.get() == PaletteStatus::Inactive {
                Display::None
            } else {
                Display::Flex
            })
            .position(Position::Absolute)
            .dimension_pct(1.0, 1.0)
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

fn completion(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let completion_data = window_tab_data.common.completion;
    let config = window_tab_data.common.config;
    let active = completion_data.with_untracked(|c| c.active);
    let request_id =
        move || completion_data.with_untracked(|c| (c.request_id, c.input_id));
    scroll(cx, move |cx| {
        virtual_list(
            cx,
            VirtualListDirection::Vertical,
            move || completion_data.with(|c| VectorItems(c.filtered_items.clone())),
            move |(i, _item)| (request_id(), *i),
            move |cx, (i, item)| {
                stack(cx, |cx| {
                    (
                        container(cx, move |cx| {
                            label(cx, move || {
                                item.item
                                    .kind
                                    .map(completion_kind_to_str)
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .style(cx, move || {
                                Style::BASE
                                    .width_pct(1.0)
                                    .justify_content(Some(JustifyContent::Center))
                            })
                        })
                        .style(cx, move || {
                            let config = config.get();
                            Style::BASE
                                .width_px(config.editor.line_height() as f32)
                                .height_pct(1.0)
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
                            cx,
                            move || item.item.label.clone(),
                            move || item.indices.clone(),
                            move || {
                                *config.get().get_color(LapceColor::EDITOR_FOCUS)
                            },
                        )
                        .style(cx, move || {
                            let config = config.get();
                            Style::BASE
                                .padding_horiz(5.0)
                                .align_items(Some(AlignItems::Center))
                                .dimension_pct(1.0, 1.0)
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
                .style(cx, move || {
                    Style::BASE
                        .align_items(Some(AlignItems::Center))
                        .width_pct(1.0)
                        .height_px(config.get().editor.line_height() as f32)
                })
            },
            VirtualListItemSize::Fixed(config.get().editor.line_height() as f64),
        )
        .style(cx, || {
            Style::BASE
                .align_items(Some(AlignItems::Center))
                .width_pct(1.0)
                .flex_col()
        })
    })
    .scroll_bar_color(cx, move || {
        *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
    })
    .on_ensure_visible(cx, move || {
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
    .style(cx, move || {
        let config = config.get();
        let origin = window_tab_data.completion_origin();
        Style::BASE
            .position(Position::Absolute)
            .width_px(400.0)
            .max_height_px(400.0)
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(*config.get_color(LapceColor::COMPLETION_BACKGROUND))
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size as f32)
            .border_radius(6.0)
    })
}

fn code_action(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let code_action = window_tab_data.code_action;
    let (status, active) = code_action
        .with_untracked(|code_action| (code_action.status, code_action.active));
    let request_id =
        move || code_action.with_untracked(|code_action| code_action.request_id);
    scroll(cx, move |cx| {
        list(
            cx,
            move || {
                code_action.with(|code_action| {
                    code_action.filtered_items.clone().into_iter().enumerate()
                })
            },
            move |(i, _item)| (request_id(), *i),
            move |cx, (i, item)| {
                container(cx, move |cx| label(cx, move || item.title().to_string()))
                    .style(cx, move || {
                        let config = config.get();
                        Style::BASE
                            .padding_horiz(10.0)
                            .align_items(Some(AlignItems::Center))
                            .min_width_px(0.0)
                            .width_pct(1.0)
                            .height_px(config.editor.line_height() as f32)
                            .apply_if(active.get() == i, |s| {
                                s.background(
                                    *config
                                        .get_color(LapceColor::COMPLETION_CURRENT),
                                )
                            })
                    })
            },
        )
        .style(cx, || Style::BASE.width_pct(1.0).flex_col())
    })
    .scroll_bar_color(cx, move || {
        *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
    })
    .on_ensure_visible(cx, move || {
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
    .style(cx, move || {
        let origin = window_tab_data.code_action_origin();
        Style::BASE
            .display(match status.get() {
                CodeActionStatus::Inactive => Display::None,
                CodeActionStatus::Active => Display::Flex,
            })
            .position(Position::Absolute)
            .width_px(400.0)
            .max_height_px(400.0)
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(*config.get().get_color(LapceColor::COMPLETION_BACKGROUND))
            .border_radius(20.0)
    })
}

fn window_tab(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let source_control = window_tab_data.source_control;
    let window_origin = window_tab_data.window_origin;
    let layout_rect = window_tab_data.layout_rect;
    let config = window_tab_data.common.config;
    let workspace = window_tab_data.workspace.clone();
    let set_workbench_command =
        window_tab_data.common.workbench_command.write_only();

    stack(cx, |cx| {
        (
            stack(cx, |cx| {
                (
                    title(
                        cx,
                        workspace,
                        source_control,
                        set_workbench_command,
                        config,
                    ),
                    workbench(cx, window_tab_data.clone()),
                    status(cx, window_tab_data.clone()),
                )
            })
            .on_resize(move |point, rect| {
                window_origin.set(point);
                layout_rect.set(rect);
            })
            .style(cx, || Style::BASE.dimension_pct(1.0, 1.0).flex_col()),
            completion(cx, window_tab_data.clone()),
            code_action(cx, window_tab_data.clone()),
            palette(cx, window_tab_data.clone()),
        )
    })
    .style(cx, move || {
        let config = config.get();
        Style::BASE
            .dimension_pct(1.0, 1.0)
            .color(*config.get_color(LapceColor::EDITOR_FOREGROUND))
            .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            .font_size(config.ui.font_size() as f32)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
    })
}

fn workspace_title(workspace: &LapceWorkspace) -> Option<String> {
    let p = workspace.path.as_ref()?;
    let dir = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
    Some(match &workspace.kind {
        LapceWorkspaceType::Local => format!("{dir}"),
        LapceWorkspaceType::RemoteSSH(ssh) => format!("{dir} [{ssh}]"),
        LapceWorkspaceType::RemoteWSL => format!("{dir} [wsl]"),
    })
}

fn workspace_tab_header(cx: AppContext, window_data: WindowData) -> impl View {
    let tabs = window_data.window_tabs;
    let active = window_data.active;
    let config = window_data.config;
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
    stack(cx, |cx| {
        (
            list(
                cx,
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
                move |cx, (index, tab)| {
                    click(
                        cx,
                        |cx| {
                            stack(cx, |cx| {
                                (
                                    stack(cx, |cx| {
                                        let window_data = local_window_data.clone();
                                        (
                                            label(cx, move || {
                                                workspace_title(&tab.workspace)
                                                    .unwrap_or_else(|| {
                                                        String::from("New Tab")
                                                    })
                                            })
                                            .style(cx, || {
                                                Style::BASE
                                                    .margin_left(10.0)
                                                    .min_width_px(0.0)
                                                    .flex_basis_px(0.0)
                                                    .flex_grow(1.0)
                                            }),
                                            clickable_icon(
                                                cx,
                                                LapceIcons::WINDOW_CLOSE,
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
                                                config.read_only(),
                                            )
                                            .style(cx, || {
                                                Style::BASE.margin_horiz(6.0)
                                            }),
                                        )
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let config = config.get();
                                            Style::BASE
                                                .width_pct(1.0)
                                                .min_width_px(0.0)
                                                .items_center()
                                                .border_right(1.0)
                                                .border_color(*config.get_color(
                                                    LapceColor::LAPCE_BORDER,
                                                ))
                                        },
                                    ),
                                    container(cx, |cx| {
                                        label(cx, || "".to_string()).style(
                                            cx,
                                            move || {
                                                Style::BASE
                                        .dimension_pct(1.0, 1.0)
                                        .apply_if(active.get() == index.get(), |s| {
                                            s.border_bottom(2.0)
                                        })
                                        .border_color(*config.get().get_color(
                                            LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE,
                                        ))
                                            },
                                        )
                                    })
                                    .style(
                                        cx,
                                        || {
                                            Style::BASE
                                                .position(Position::Absolute)
                                                .padding_horiz(3.0)
                                                .dimension_pct(1.0, 1.0)
                                        },
                                    ),
                                )
                            })
                            .style(cx, move || {
                                Style::BASE.dimension_pct(1.0, 1.0).items_center()
                            })
                        },
                        move || {
                            active.set(index.get_untracked());
                        },
                    )
                    .style(cx, move || {
                        Style::BASE.height_pct(1.0).width_px(tab_width.get() as f32)
                    })
                },
            )
            .style(cx, || Style::BASE.height_pct(1.0)),
            clickable_icon(
                cx,
                LapceIcons::ADD,
                move || {
                    window_data.run_window_command(WindowCommand::NewWorkspaceTab {
                        workspace: LapceWorkspace::default(),
                        end: true,
                    });
                },
                || false,
                config.read_only(),
            )
            .on_resize(move |_, rect| {
                let current = add_icon_width.get_untracked();
                if rect.width() != current {
                    add_icon_width.set(rect.width());
                }
            })
            .style(cx, || Style::BASE.padding_left(10.0).padding_right(20.0)),
        )
    })
    .on_resize(move |_, rect| {
        let current = available_width.get_untracked();
        if rect.width() != current {
            available_width.set(rect.width());
        }
    })
    .style(cx, move || {
        let config = config.get();
        Style::BASE
            .border_bottom(1.0)
            .width_pct(1.0)
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

fn window(cx: AppContext, window_data: WindowData) -> impl View {
    let window_tabs = window_data.window_tabs.read_only();
    let active = window_data.active.read_only();
    let items = move || window_tabs.get();
    let key = |(_, window_tab): &(RwSignal<usize>, Arc<WindowTabData>)| {
        window_tab.window_tab_id
    };
    let active = move || active.get();

    let window_view = tab(cx, active, items, key, |cx, (_, window_tab_data)| {
        window_tab(cx, window_tab_data)
    })
    .style(cx, || Style::BASE.dimension_pct(1.0, 1.0))
    .on_event(EventListner::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            window_data.key_down(key_event);
            true
        } else {
            false
        }
    });

    let id = window_view.id();
    cx.update_focus(id);

    window_view
}

fn app_view(cx: AppContext, window_data: WindowData) -> impl View {
    // let window_data = WindowData::new(cx);
    let window_size = window_data.size;
    let position = window_data.position;
    stack(cx, |cx| {
        (
            workspace_tab_header(cx, window_data.clone()),
            window(cx, window_data.clone()),
        )
    })
    .style(cx, || Style::BASE.flex_col().dimension_pct(1.0, 1.0))
    .on_event(EventListner::WindowResized, move |event| {
        if let Event::WindowResized(size) = event {
            window_size.set(*size);
        }
        true
    })
    .on_event(EventListner::WindowMoved, move |event| {
        if let Event::WindowMoved(point) = event {
            position.set(*point);
        }
        true
    })
}

pub fn launch() {
    let db = Arc::new(LapceDb::new().unwrap());
    let mut app = floem::Application::new();
    let scope = app.scope();
    provide_context(scope, db.clone());

    let mut windows = im::Vector::new();

    if let Ok(app_info) = db.get_app() {
        for info in app_info.windows {
            let config = WindowConfig::default().size(info.size).position(info.pos);
            let window_data = WindowData::new(scope, info);
            windows.push_back(window_data.clone());
            app = app.window(move |cx| app_view(cx, window_data), Some(config));
        }
    }

    if windows.is_empty() {
        let info = db.get_window().unwrap_or_else(|_| WindowInfo {
            size: Size::new(800.0, 600.0),
            pos: Point::ZERO,
            maximised: false,
            tabs: TabsInfo {
                active_tab: 0,
                workspaces: vec![LapceWorkspace::default()],
            },
        });
        let config = WindowConfig::default().size(info.size).position(info.pos);
        let window_data = WindowData::new(scope, info);
        windows.push_back(window_data.clone());
        app = app.window(|cx| app_view(cx, window_data), Some(config));
    }

    let windows = create_rw_signal(scope, windows);
    let app_data = AppData { windows };

    app.on_event(move |event| match event {
        floem::AppEvent::WillTerminate => {
            let _ = db.save_app(app_data.clone());
        }
    })
    .run();
}
