use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    button::button,
    event::{Event, EventListner},
    reactive::{
        create_effect, create_signal, provide_context, use_context, WriteSignal,
    },
    stack::stack,
    style::{AlignItems, Dimension, FlexDirection, Style},
    view::View,
    views::label,
    views::Decorators,
};

use crate::{
    command::{CommandKind, LapceWorkbenchCommand},
    config::LapceConfig,
    db::LapceDb,
    keypress::{condition::Condition, DefaultKeyPress, KeyPressData, KeyPressFocus},
    palette::PaletteData,
    proxy::start_proxy,
    title::title,
    window_tab::WindowTabData,
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

fn workbench(cx: AppContext) -> impl View {
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

fn app_logic(cx: AppContext) -> impl View {
    let db = Arc::new(LapceDb::new().unwrap());
    provide_context(cx.scope, db.clone());

    let workspace = Arc::new(LapceWorkspace {
        kind: LapceWorkspaceType::Local,
        path: Some(PathBuf::from("/Users/Lulu/lapce")),
        last_open: 0,
    });

    let window_tab = WindowTabData::new(cx, workspace);
    let workbench_command = window_tab.workbench_command;

    {
        let window_tab = window_tab.clone();
        create_effect(cx.scope, move |_| {
            if let Some(cmd) = workbench_command.get() {
                window_tab.run_workbench_command(cmd);
            }
        });
    }

    let proxy_data = window_tab.proxy.clone();
    let keypress = window_tab.keypress;
    let app = stack(cx, move |cx| {
        (title(cx, &proxy_data), workbench(cx), status(cx))
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
    let id = app.id();
    cx.update_focus(id);
    app
}

pub fn launch() {
    floem::launch(app_logic);
}
