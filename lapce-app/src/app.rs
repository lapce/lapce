use floem::{
    app::AppContext,
    button::button,
    event::{Event, EventListner},
    reactive::create_signal,
    stack::stack,
    style::{AlignItems, Dimension, FlexDirection, Style},
    view::View,
    views::label,
    views::Decorators,
};

use crate::{proxy::start_proxy, title::title};

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
    let proxy_data = start_proxy(cx);
    stack(cx, move |cx| {
        (title(cx, &proxy_data), workbench(cx), status(cx))
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
    .event(EventListner::KeyDown, |event| {
        if let Event::KeyDown = event {}
        true
    })
}

pub fn launch() {
    floem::launch(app_logic);
}
