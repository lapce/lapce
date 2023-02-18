use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    button::button,
    event::{Event, EventListner},
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_selector, create_selector_with_fn, create_signal,
        provide_context, use_context, ReadSignal, RwSignal, UntrackedGettableSignal,
        WriteSignal,
    },
    stack::stack,
    style::{
        AlignContent, AlignItems, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    view::View,
    views::{
        container, container_box, list, tab, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize,
    },
    views::{label, scroll},
};

use crate::{
    command::{CommandKind, LapceWorkbenchCommand},
    config::LapceConfig,
    db::LapceDb,
    doc::DocContent,
    editor::EditorData,
    editor_tab::{EditorTabChild, EditorTabData},
    id::{EditorId, EditorTabId, SplitId},
    keypress::{condition::Condition, DefaultKeyPress, KeyPressData, KeyPressFocus},
    main_split::{SplitContent, SplitData},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData, PaletteStatus,
    },
    proxy::{start_proxy, ProxyData},
    title::title,
    window_tab::{Focus, WindowTabData},
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

fn editor(cx: AppContext, editor: ReadSignal<EditorData>) -> impl View {
    let doc = move || editor.with(|editor| editor.doc).get();
    let key_fn = |(line, content): &(usize, String)| format!("{line}{content}");
    let view_fn = |cx, (line, line_content): (usize, String)| {
        label(cx, move || line_content.clone()).style(cx, || Style {
            height: Dimension::Points(20.0),
            ..Default::default()
        })
    };
    scroll(cx, |cx| {
        virtual_list(
            cx,
            VirtualListDirection::Vertical,
            doc,
            key_fn,
            view_fn,
            VirtualListItemSize::Fixed(20.0),
        )
        .style(cx, || Style {
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })
    })
    .style(cx, || Style {
        position: Position::Absolute,
        border: 1.0,
        border_radius: 10.0,
        // flex_grow: 1.0,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        ..Default::default()
    })
}

fn editor_tab_header(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let items = move || editor_tab.get().children.into_iter().enumerate();
    let key = |(_, child): &(usize, EditorTabChild)| child.id();

    let view_fn = move |cx, (i, child)| {
        button(
            cx,
            |cx| match child {
                EditorTabChild::Editor(editor_id) => {
                    let editor_data =
                        editors.with(|editors| editors.get(&editor_id).cloned());
                    if let Some(editor_data) = editor_data {
                        let content = editor_data.with(|editor_data| {
                            editor_data.doc.with(|doc| doc.content.clone())
                        });
                        match content {
                            DocContent::File(path) => {
                                label(cx, move || path.to_str().unwrap().to_string())
                            }
                            DocContent::Local => {
                                label(cx, || "local editor".to_string())
                            }
                        }
                    } else {
                        label(cx, || "emtpy editor".to_string())
                    }
                }
            },
            move || {
                editor_tab.update(|editor_tab| {
                    editor_tab.active = i;
                });
            },
        )
        .style(cx, || Style {
            border_radius: 10.0,
            border: 1.0,
            ..Default::default()
        })
    };

    container(cx, |cx| {
        scroll(cx, |cx| {
            list(cx, items, key, view_fn).style(cx, || Style {
                height: Dimension::Percent(1.0),
                align_content: Some(AlignContent::Center),
                ..Default::default()
            })
        })
        .style(cx, || Style {
            position: Position::Absolute,
            border: 1.0,
            height: Dimension::Percent(1.0),
            max_width: Dimension::Percent(1.0),
            ..Default::default()
        })
    })
    .style(cx, || Style {
        height: Dimension::Points(50.0),
        ..Default::default()
    })
}

fn editor_tab_content(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let items = move || editor_tab.get().children;
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |cx, child| {
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                if let Some(editor_data) = editor_data {
                    container_box(cx, |cx| {
                        Box::new(editor(cx, editor_data.read_only()))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(cx, active, items, key, view_fn).style(cx, || Style {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

fn editor_tab(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    stack(cx, |cx| {
        (
            editor_tab_header(cx, editor_tab, editors),
            editor_tab_content(cx, editor_tab, editors),
        )
    })
    .style(cx, || Style {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        ..Default::default()
    })
}

fn split_list(
    cx: AppContext,
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    split: ReadSignal<SplitData>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let items = move || split.get().children;
    let key = |content: &SplitContent| content.id();
    let view_fn = move |cx, content| {
        let child = match content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    editor_tabs.with(|tabs| tabs.get(&editor_tab_id).cloned());
                if let Some(editor_tab_data) = editor_tab_data {
                    container_box(cx, |cx| {
                        Box::new(editor_tab(cx, editor_tab_data, editors))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor tab".to_string()))
                    })
                }
            }
            SplitContent::Split(split_id) => {
                if let Some(split) =
                    splits.with(|splits| splits.get(&split_id).cloned())
                {
                    split_list(cx, splits, split.read_only(), editor_tabs, editors)
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy split".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style {
            height: Dimension::Percent(1.0),
            border: 5.0,
            flex_grow: 1.0,
            ..Default::default()
        })
    };
    container_box(cx, |cx| {
        Box::new(list(cx, items, key, view_fn).style(cx, || Style {
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            ..Default::default()
        }))
    })
}

fn main_split(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get()
        .get(&root_split)
        .unwrap()
        .read_only();
    let splits = window_tab_data.main_split.splits.read_only();
    let editor_tabs = window_tab_data.main_split.editor_tabs.read_only();
    let editors = window_tab_data.main_split.editors.read_only();
    split_list(cx, splits, root_split, editor_tabs, editors).style(cx, || Style {
        flex_grow: 1.0,
        ..Default::default()
    })
}

fn workbench(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    stack(cx, move |cx| {
        (
            label(cx, move || "left".to_string()).style(cx, || Style {
                padding: 20.0,
                border_right: 1.0,
                ..Default::default()
            }),
            stack(cx, move |cx| {
                (
                    main_split(cx, window_tab_data),
                    label(cx, move || "bottom".to_string()).style(cx, || Style {
                        padding: 20.0,
                        border_top: 1.0,
                        min_width: Dimension::Points(0.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, || Style {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_width: Dimension::Points(0.0),
                ..Default::default()
            }),
            label(cx, move || "right".to_string()).style(cx, || Style {
                padding: 20.0,
                border_left: 1.0,
                min_width: Dimension::Points(0.0),
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        flex_grow: 1.0,
        flex_direction: FlexDirection::Row,
        ..Default::default()
    })
}

fn status(cx: AppContext) -> impl View {
    label(cx, move || "status".to_string()).style(cx, || Style {
        border_top: 1.0,
        ..Default::default()
    })
}

fn palette_item(
    cx: AppContext,
    item: PaletteItem,
    index: ReadSignal<usize>,
) -> impl View {
    match item.content {
        PaletteItemContent::File { path, full_path } => {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let (file_name, _) = create_signal(cx.scope, file_name);
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let (folder, _) = create_signal(cx.scope, folder);
            let item_index = item.index;
            stack(cx, |cx| {
                (
                    label(cx, move || file_name.get()).style(cx, || Style {
                        max_width: Dimension::Percent(1.0),
                        ..Default::default()
                    }),
                    label(cx, move || folder.get()).style(cx, || Style {
                        margin_left: 6.0,
                        min_width: Dimension::Points(0.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, move || Style {
                height: Dimension::Points(20.0),
                padding_left: 10.0,
                padding_right: 10.0,
                background: if index.get() == item_index {
                    Some(Color::rgb8(180, 0, 0))
                } else {
                    None
                },
                ..Default::default()
            })
        }
    }
}

fn palette_input(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let doc = window_tab_data.palette.editor.doc.read_only();
    let cursor = window_tab_data.palette.editor.cursor.read_only();
    let config = window_tab_data.palette.config;
    let cursor_x = move || {
        let offset = cursor.get().offset();
        let config = config.get();
        doc.with(|doc| doc.line_point_of_offset(offset, 12, &config).x)
    };
    container(cx, |cx| {
        container(cx, |cx| {
            scroll(cx, |cx| {
                stack(cx, |cx| {
                    (
                        label(cx, move || {
                            doc.with(|doc| doc.buffer().text().to_string())
                        }),
                        label(cx, move || "".to_string()).style(cx, move || Style {
                            position: Position::Absolute,
                            margin_left: cursor_x() as f32 - 1.0,
                            border_left: 2.0,
                            ..Default::default()
                        }),
                    )
                })
            })
            .style(cx, || Style {
                flex_grow: 1.0,
                min_width: Dimension::Points(0.0),
                padding_top: 5.0,
                padding_bottom: 5.0,
                ..Default::default()
            })
        })
        .style(cx, || Style {
            flex_grow: 1.0,
            min_width: Dimension::Points(0.0),
            border: 1.0,
            border_radius: 2.0,
            padding_left: 5.0,
            padding_right: 5.0,
            ..Default::default()
        })
    })
    .style(cx, || Style {
        padding: 5.0,
        ..Default::default()
    })
}

fn palette_content(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let index = window_tab_data.palette.index.read_only();
    container(cx, |cx| {
        scroll(cx, |cx| {
            virtual_list(
                cx,
                VirtualListDirection::Vertical,
                move || items.get(),
                move |item| format!("{}{}", item.id, item.index),
                move |cx, item| palette_item(cx, item, index),
                VirtualListItemSize::Fixed(20.0),
            )
            .style(cx, || Style {
                width: Dimension::Percent(1.0),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            })
        })
        .on_ensure_visible(cx, move || {
            Size::new(1.0, 20.0)
                .to_rect()
                .with_origin(Point::new(0.0, index.get() as f64 * 20.0))
        })
        .style(cx, || Style {
            width: Dimension::Percent(1.0),
            min_height: Dimension::Points(0.0),
            ..Default::default()
        })
    })
    .style(cx, move || Style {
        display: if items.with(|items| items.is_empty()) {
            Display::None
        } else {
            Display::Flex
        },
        width: Dimension::Percent(1.0),
        min_height: Dimension::Points(0.0),
        padding_bottom: 5.0,
        ..Default::default()
    })
}

fn palette(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let keypress = window_tab_data.keypress.write_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    container(cx, |cx| {
        stack(cx, |cx| {
            (
                palette_input(cx, window_tab_data.clone()),
                palette_content(cx, window_tab_data.clone()),
            )
        })
        .style(cx, || Style {
            width: Dimension::Points(500.0),
            max_width: Dimension::Percent(0.9),
            min_height: Dimension::Points(0.0),
            max_height: Dimension::Percent(0.5),
            border: 1.0,
            border_radius: 5.0,
            flex_direction: FlexDirection::Column,
            background: Some(Color::rgb8(0, 0, 0)),
            ..Default::default()
        })
    })
    .style(cx, move || Style {
        display: if status.get() == PaletteStatus::Inactive {
            Display::None
        } else {
            Display::Flex
        },
        position: Position::Absolute,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        align_content: Some(AlignContent::Center),
        ..Default::default()
    })
}

fn window_tab(cx: AppContext, workspace: Arc<LapceWorkspace>) -> impl View {
    let window_tab_data = WindowTabData::new(cx, workspace);
    let workbench_command = window_tab_data.workbench_command;

    {
        let window_tab_data = window_tab_data.clone();
        create_effect(cx.scope, move |_| {
            if let Some(cmd) = workbench_command.get() {
                window_tab_data.run_workbench_command(cx, cmd);
            }
        });
    }

    {
        let window_tab_data = window_tab_data.clone();
        let internal_command = window_tab_data.internal_command;
        create_effect(cx.scope, move |_| {
            if let Some(cmd) = internal_command.get() {
                println!("get internal command");
                window_tab_data.run_internal_command(cx, cmd);
            }
        });
    }

    let proxy_data = window_tab_data.proxy.clone();
    let keypress = window_tab_data.keypress;
    let window_tab_view = stack(cx, |cx| {
        (
            stack(cx, |cx| {
                (
                    title(cx, &proxy_data),
                    workbench(cx, window_tab_data.clone()),
                    status(cx),
                )
            })
            .style(cx, || Style {
                width: Dimension::Percent(1.0),
                height: Dimension::Percent(1.0),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            }),
            palette(cx, window_tab_data.clone()),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        ..Default::default()
    })
    .event(EventListner::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            window_tab_data.key_down(cx, key_event);
            true
        } else {
            false
        }
    });

    let id = window_tab_view.id();
    cx.update_focus(id);

    window_tab_view
}

fn app_logic(cx: AppContext) -> impl View {
    let db = Arc::new(LapceDb::new().unwrap());
    provide_context(cx.scope, db);

    let workspace = Arc::new(LapceWorkspace {
        kind: LapceWorkspaceType::Local,
        path: Some(PathBuf::from("/Users/dz/lapce-rust")),
        last_open: 0,
    });

    window_tab(cx, workspace)
}

pub fn launch() {
    floem::launch(app_logic);
}
