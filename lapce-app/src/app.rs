use floem::{
    app::AppContext,
    button::button,
    label,
    stack::stack,
    style::{Dimension, FlexDirection, Style},
    view::View,
    Decorators,
};

fn title(cx: AppContext) -> impl View {
    stack(cx, move |cx| {
        (
            label(cx, move || "title".to_string()),
            label(cx, move || "title".to_string()),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Auto,
        padding: 10.0,
        border: 2.0,
        border_radius: 24.0,
        ..Default::default()
    })
}

fn workbench(cx: AppContext) -> impl View {
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
    stack(cx, |cx| (title(cx), workbench(cx), status(cx))).style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

pub fn launch() {
    floem::launch(app_logic);
}
