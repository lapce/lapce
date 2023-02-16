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
        create_effect, create_signal, provide_context, use_context, ReadSignal,
        WriteSignal,
    },
    stack::stack,
    style::{
        AlignContent, AlignItems, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    view::View,
    views::{
        container, virtual_list, Decorators, VirtualListDirection,
        VirtualListItemSize,
    },
    views::{label, scroll},
};

use crate::{
    command::{CommandKind, LapceWorkbenchCommand},
    config::LapceConfig,
    db::LapceDb,
    keypress::{condition::Condition, DefaultKeyPress, KeyPressData, KeyPressFocus},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData,
    },
    proxy::{start_proxy, ProxyData},
    title::title,
    window_tab::{Focus, WindowTabData},
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

fn main_split(cx: AppContext) -> impl View {
    let (couter, set_couter) = create_signal(cx.scope, 0);
    stack(cx, move |cx| {
        (
            label(cx, move || "main".to_string()),
            button(cx, || "".to_string(), || {}).style(cx, || Style {
                width: Dimension::Percent(1.0),
                height: Dimension::Auto,
                flex_grow: 1.0,
                border: 2.0,
                border_radius: 24.0,
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: floem::style::Dimension::Percent(1.0),
        height: floem::style::Dimension::Auto,
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

fn status(cx: AppContext) -> impl View {
    label(cx, move || "status".to_string())
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
            let id = item.id;
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
                background: if index.get() == id {
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
                move |item| format!("{}", item.id),
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
    let palette = container(cx, |cx| {
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
    .style(cx, || Style {
        position: Position::Absolute,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        align_content: Some(AlignContent::Center),
        ..Default::default()
    })
    .event(EventListner::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            keypress.update(|keypress| {
                keypress.key_down(cx, key_event, &palette_data);
            });
        }
        true
    });

    let id = palette.id();
    let focus = window_tab_data.focus.read_only();
    create_effect(cx.scope, move |_| {
        if let Focus::Palette = focus.get() {
            cx.update_focus(id);
        }
    });

    palette
}

fn workbench(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let proxy_data = window_tab_data.proxy;
    let keypress = window_tab_data.keypress;
    let workbench = stack(cx, move |cx| {
        (title(cx, &proxy_data), main_split(cx), status(cx))
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
    .event(EventListner::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            keypress.update(|keypress| {
                keypress.key_down(cx, key_event, &mut DefaultKeyPress {});
            });
        }
        true
    });

    let id = workbench.id();
    cx.update_focus(id);
    workbench
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

    stack(cx, move |cx| {
        (
            workbench(cx, window_tab_data.clone()),
            palette(cx, window_tab_data.clone()),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        ..Default::default()
    })
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
