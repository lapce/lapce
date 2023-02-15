use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    button::button,
    event::{Event, EventListner},
    peniko::Color,
    reactive::{
        create_effect, create_signal, provide_context, use_context, WriteSignal,
    },
    stack::stack,
    style::{
        AlignContent, AlignItems, Dimension, FlexDirection, JustifyContent,
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
    window_tab::WindowTabData,
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

fn palette_item(cx: AppContext, item: PaletteItem) -> impl View {
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
            .style(cx, || Style {
                height: Dimension::Points(20.0),
                padding_left: 10.0,
                padding_right: 10.0,
                ..Default::default()
            })
        }
    }
}

fn palette_content(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let items = window_tab_data.palette.items.read_only();
    scroll(cx, |cx| {
        virtual_list(
            cx,
            VirtualListDirection::Vertical,
            move || items.get(),
            move |item| format!("{}", item.id),
            palette_item,
            VirtualListItemSize::Fixed(20.0),
        )
        .style(cx, || Style {
            width: Dimension::Percent(1.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        flex_grow: 1.0,
        border: 1.0,
        // height: Dimension::Points(900.0),
        ..Default::default()
    })
}

fn palette(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    container(cx, move |cx| {
        container(cx, move |cx| {
            stack(cx, move |cx| {
                (
                    label(cx, move || "palette".to_string()),
                    palette_content(cx, window_tab_data),
                )
            })
            .style(cx, || Style {
                width: Dimension::Percent(1.0),
                border: 1.0,
                border_radius: 5.0,
                flex_direction: FlexDirection::Column,
                ..Default::default()
            })
        })
        .style(cx, || Style {
            width: Dimension::Points(200.0),
            // min_height: Dimension::Points(0.0),
            max_height: Dimension::Percent(0.5),
            background: Some(Color::rgba8(0, 0, 0, 255)),
            // border: 1.0,
            // border_radius: 2.0,
            ..Default::default()
        })
    })
    .style(cx, || Style {
        position: Position::Absolute,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        justify_content: Some(AlignContent::Center),
        ..Default::default()
    })
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
            println!("got key event {key_event:?}");
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
