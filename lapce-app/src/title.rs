use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{create_signal, ReadSignal, SignalGet},
    stack::stack,
    style::{AlignItems, Dimension, JustifyContent, Style},
    view::View,
    views::{container, Decorators},
    views::{label, svg},
};

use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    proxy::ProxyData,
};

fn left(
    cx: AppContext,
    proxy_data: &ProxyData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    let (read_svg, set_svg) = create_signal(cx.scope, r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"> <path fill-rule="evenodd" clip-rule="evenodd" d="M2.99999 9.00004L5.14644 11.1465L4.43933 11.8536L1.43933 8.85359V8.14649L4.43933 5.14648L5.14644 5.85359L2.99999 8.00004H13L10.8535 5.85359L11.5606 5.14648L14.5606 8.14648V8.85359L11.5606 11.8536L10.8535 11.1465L13 9.00004H2.99999Z" fill="\#C5C5C5"/></svg>"#.to_string());
    stack(cx, move |cx| {
        (
            container(cx, move |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::REMOTE)).style(
                    cx,
                    move || Style {
                        width: Dimension::Points(26.0),
                        height: Dimension::Points(26.0),
                        ..Default::default()
                    },
                )
            })
            .style(cx, move || Style {
                height: Dimension::Percent(1.0),
                padding_left: 10.0,
                padding_right: 10.0,
                align_items: Some(AlignItems::Center),
                background: Some(
                    *config.get().get_color(LapceColor::LAPCE_REMOTE_LOCAL),
                ),
                ..Default::default()
            }),
            container(cx, move |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::SCM)).style(
                    cx,
                    move || {
                        let icon_size = config.get().ui.icon_size() as f32;
                        Style {
                            width: Dimension::Points(icon_size),
                            height: Dimension::Points(icon_size),
                            ..Default::default()
                        }
                    },
                )
            })
            .style(cx, move || Style {
                padding_left: 10.0,
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
        )
    })
    .style(cx, move || Style {
        height: Dimension::Percent(1.0),
        flex_basis: Dimension::Points(0.0),
        flex_grow: 1.0,
        justify_content: Some(JustifyContent::FlexStart),
        align_items: Some(AlignItems::Center),
        ..Default::default()
    })
}

fn middle(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> impl View {
    stack(cx, move |cx| {
        (
            stack(cx, move |cx| {
                (
                    container(cx, move |cx| {
                        svg(cx, move || {
                            config.get().ui_svg(LapceIcons::LOCATION_BACKWARD)
                        })
                        .style(cx, move || {
                            let icon_size = config.get().ui.icon_size() as f32;
                            Style {
                                width: Dimension::Points(icon_size),
                                height: Dimension::Points(icon_size),
                                ..Default::default()
                            }
                        })
                    })
                    .style(cx, move || Style {
                        padding_left: 10.0,
                        padding_right: 10.0,
                        ..Default::default()
                    }),
                    container(cx, move |cx| {
                        svg(cx, move || {
                            config.get().ui_svg(LapceIcons::LOCATION_FORWARD)
                        })
                        .style(cx, move || {
                            let icon_size = config.get().ui.icon_size() as f32;
                            Style {
                                width: Dimension::Points(icon_size),
                                height: Dimension::Points(icon_size),
                                ..Default::default()
                            }
                        })
                    })
                    .style(cx, move || Style {
                        padding_right: 10.0,
                        ..Default::default()
                    }),
                )
            })
            .style(cx, || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexEnd),
                ..Default::default()
            }),
            stack(cx, move |cx| {
                (
                    svg(cx, move || config.get().ui_svg(LapceIcons::SEARCH)).style(
                        cx,
                        move || {
                            let icon_size = config.get().ui.icon_size() as f32;
                            Style {
                                width: Dimension::Points(icon_size),
                                height: Dimension::Points(icon_size),
                                ..Default::default()
                            }
                        },
                    ),
                    label(cx, move || "middle".to_string()).style(cx, || Style {
                        padding_left: 10.0,
                        padding_right: 10.0,
                        ..Default::default()
                    }),
                    svg(cx, move || config.get().ui_svg(LapceIcons::PALETTE_MENU))
                        .style(cx, move || {
                            let icon_size = config.get().ui.icon_size() as f32;
                            Style {
                                width: Dimension::Points(icon_size),
                                height: Dimension::Points(icon_size),
                                ..Default::default()
                            }
                        }),
                )
            })
            .style(cx, move || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 10.0,
                min_width: Dimension::Points(200.0),
                max_width: Dimension::Points(500.0),
                height: Dimension::Points(26.0),
                justify_content: Some(JustifyContent::Center),
                align_items: Some(AlignItems::Center),
                border: 1.0,
                border_radius: 2.0,
                background: Some(
                    *config.get().get_color(LapceColor::EDITOR_BACKGROUND),
                ),
                ..Default::default()
            }),
            container(cx, move |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::START)).style(
                    cx,
                    move || {
                        let icon_size = config.get().ui.icon_size() as f32;
                        Style {
                            width: Dimension::Points(icon_size),
                            height: Dimension::Points(icon_size),
                            ..Default::default()
                        }
                    },
                )
            })
            .style(cx, move || Style {
                padding_left: 10.0,
                padding_right: 10.0,
                flex_basis: Dimension::Points(0.0),
                flex_grow: 1.0,
                justify_content: Some(JustifyContent::FlexStart),
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        flex_basis: Dimension::Points(0.0),
        flex_grow: 2.0,
        align_items: Some(AlignItems::Center),
        justify_content: Some(JustifyContent::Center),
        ..Default::default()
    })
}

fn right(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> impl View {
    container(cx, move |cx| {
        svg(cx, move || config.get().ui_svg(LapceIcons::SETTINGS)).style(
            cx,
            move || {
                let icon_size = config.get().ui.icon_size() as f32;
                Style {
                    width: Dimension::Points(icon_size),
                    height: Dimension::Points(icon_size),
                    ..Default::default()
                }
            },
        )
    })
    .style(cx, || Style {
        flex_basis: Dimension::Points(0.0),
        flex_grow: 1.0,
        padding_left: 10.0,
        padding_right: 10.0,
        justify_content: Some(JustifyContent::FlexEnd),
        ..Default::default()
    })
}

pub fn title(
    cx: AppContext,
    proxy_data: &ProxyData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    stack(cx, move |cx| {
        (
            left(cx, proxy_data, config),
            middle(cx, config),
            right(cx, config),
        )
    })
    .style(cx, move || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Points(37.0),
        align_items: Some(AlignItems::Center),
        background: Some(*config.get().get_color(LapceColor::PANEL_BACKGROUND)),
        border_bottom: 1.0,
        ..Default::default()
    })
}
