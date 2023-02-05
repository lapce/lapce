use floem::{
    app::AppContext,
    reactive::create_signal,
    stack::stack,
    style::{AlignItems, Dimension, JustifyContent, Style},
    view::View,
    views::{container, Decorators},
    views::{label, svg},
};

use crate::proxy::ProxyData;

fn left(cx: AppContext, proxy_data: &ProxyData) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    let (read_svg, set_svg) = create_signal(cx.scope, r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"> <path fill-rule="evenodd" clip-rule="evenodd" d="M2.99999 9.00004L5.14644 11.1465L4.43933 11.8536L1.43933 8.85359V8.14649L4.43933 5.14648L5.14644 5.85359L2.99999 8.00004H13L10.8535 5.85359L11.5606 5.14648L14.5606 8.14648V8.85359L11.5606 11.8536L10.8535 11.1465L13 9.00004H2.99999Z" fill="\#C5C5C5"/></svg>"#.to_string());
    stack(cx, move |cx| {
        (
            container(cx, move |cx| {
                svg(cx, move || read_svg.get()).style(cx, || Style {
                    width: Dimension::Points(20.0),
                    height: Dimension::Points(20.0),
                    ..Default::default()
                })
            })
            .style(cx, || Style {
                height: Dimension::Percent(1.0),
                padding: 20.0,
                border_right: 1.0,
                align_items: Some(AlignItems::Center),
                ..Default::default()
            }),
            svg(cx, move || read_svg.get()).style(cx, || Style {
                width: Dimension::Points(20.0),
                height: Dimension::Points(20.0),
                ..Default::default()
            }),
            label(cx, move || head().unwrap_or_default()).style(cx, || Style {
                height: Dimension::Percent(1.0),
                padding_left: 10.0,
                padding_right: 10.0,
                border_right: 1.0,
                align_items: Some(AlignItems::Center),
                ..Default::default()
            }),
            label(cx, move || {
                println!("connected got new value");
                if connected.get() {
                    "connected".to_string()
                } else {
                    "disconnected".to_string()
                }
            }),
        )
    })
}

fn middle(cx: AppContext) -> impl View {
    let (read_svg, set_svg) = create_signal(cx.scope, r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"> <path fill-rule="evenodd" clip-rule="evenodd" d="M2.99999 9.00004L5.14644 11.1465L4.43933 11.8536L1.43933 8.85359V8.14649L4.43933 5.14648L5.14644 5.85359L2.99999 8.00004H13L10.8535 5.85359L11.5606 5.14648L14.5606 8.14648V8.85359L11.5606 11.8536L10.8535 11.1465L13 9.00004H2.99999Z" fill="\#C5C5C5"/></svg>"#.to_string());
    stack(cx, move |cx| {
        (
            stack(cx, move |cx| {
                (
                    svg(cx, move || read_svg.get()).style(cx, || Style {
                        width: Dimension::Points(20.0),
                        height: Dimension::Points(20.0),
                        ..Default::default()
                    }),
                    svg(cx, move || read_svg.get()).style(cx, || Style {
                        width: Dimension::Points(20.0),
                        height: Dimension::Points(20.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexEnd),
                border: 1.0,
                ..Default::default()
            }),
            label(cx, move || "middle".to_string()).style(cx, || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 10.0,
                min_width: Dimension::Points(200.0),
                max_width: Dimension::Points(500.0),
                justify_content: Some(JustifyContent::Center),
                border: 1.0,
                ..Default::default()
            }),
            svg(cx, move || read_svg.get()).style(cx, || Style {
                width: Dimension::Points(20.0),
                height: Dimension::Points(20.0),
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexStart),
                border: 1.0,
                ..Default::default()
            }),
        )
    })
}

fn right(cx: AppContext) -> impl View {
    let (read_svg, set_svg) = create_signal(cx.scope, r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"> <path fill-rule="evenodd" clip-rule="evenodd" d="M2.99999 9.00004L5.14644 11.1465L4.43933 11.8536L1.43933 8.85359V8.14649L4.43933 5.14648L5.14644 5.85359L2.99999 8.00004H13L10.8535 5.85359L11.5606 5.14648L14.5606 8.14648V8.85359L11.5606 11.8536L10.8535 11.1465L13 9.00004H2.99999Z" fill="\#C5C5C5"/></svg>"#.to_string());
    container(cx, move |cx| {
        svg(cx, move || read_svg.get()).style(cx, || Style {
            width: Dimension::Points(20.0),
            height: Dimension::Points(20.0),
            ..Default::default()
        })
    })
}

pub fn title(cx: AppContext, proxy_data: &ProxyData) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    let (read_svg, set_svg) = create_signal(cx.scope, r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"> <path fill-rule="evenodd" clip-rule="evenodd" d="M2.99999 9.00004L5.14644 11.1465L4.43933 11.8536L1.43933 8.85359V8.14649L4.43933 5.14648L5.14644 5.85359L2.99999 8.00004H13L10.8535 5.85359L11.5606 5.14648L14.5606 8.14648V8.85359L11.5606 11.8536L10.8535 11.1465L13 9.00004H2.99999Z" fill="\#C5C5C5"/></svg>"#.to_string());
    stack(cx, move |cx| {
        (
            left(cx, proxy_data).style(cx, || Style {
                height: Dimension::Percent(1.0),
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexStart),
                border: 1.0,
                ..Default::default()
            }),
            middle(cx).style(cx, || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 2.0,
                justify_content: Some(JustifyContent::Center),
                border: 1.0,
                ..Default::default()
            }),
            right(cx).style(cx, || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexEnd),
                border: 1.0,
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Points(100.0),
        align_items: Some(AlignItems::Center),
        border_bottom: 10.0,
        ..Default::default()
    })
}
