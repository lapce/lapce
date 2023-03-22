use std::sync::Arc;

use floem::{
    app::AppContext,
    peniko::Color,
    reactive::{
        create_signal, ReadSignal, SignalGet, SignalSet, SignalWith, WriteSignal,
    },
    stack::stack,
    style::{AlignItems, Dimension, Display, JustifyContent, Style},
    view::View,
    views::{click, container, Decorators},
    views::{label, svg},
};

use crate::{
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    proxy::ProxyData,
    workspace::LapceWorkspace,
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
            stack(cx, move |cx| {
                (
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
                    ),
                    label(cx, move || head().unwrap_or_default()).style(cx, || {
                        Style {
                            margin_left: Some(10.0),
                            ..Default::default()
                        }
                    }),
                )
            })
            .style(cx, move || Style {
                display: if diff_info.with(|diff_info| diff_info.is_some()) {
                    Display::Flex
                } else {
                    Display::None
                },
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

fn middle(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    set_workbench_command: WriteSignal<Option<LapceWorkbenchCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let local_workspace = workspace.clone();
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
            click(
                cx,
                |cx| {
                    stack(cx, |cx| {
                        (
                            svg(cx, move || config.get().ui_svg(LapceIcons::SEARCH))
                                .style(cx, move || {
                                    let icon_size =
                                        config.get().ui.icon_size() as f32;
                                    Style {
                                        width: Dimension::Points(icon_size),
                                        height: Dimension::Points(icon_size),
                                        ..Default::default()
                                    }
                                }),
                            label(cx, move || {
                                if let Some(s) = local_workspace.display() {
                                    s
                                } else {
                                    "Open Folder".to_string()
                                }
                            })
                            .style(cx, || Style {
                                padding_left: 10.0,
                                padding_right: 5.0,
                                ..Default::default()
                            }),
                            click(
                                cx,
                                move |cx| {
                                    svg(cx, move || {
                                        config.get().ui_svg(LapceIcons::PALETTE_MENU)
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let icon_size =
                                                config.get().ui.icon_size() as f32;
                                            Style {
                                                width: Dimension::Points(icon_size),
                                                height: Dimension::Points(icon_size),
                                                ..Default::default()
                                            }
                                        },
                                    )
                                },
                                || {},
                            )
                            .style(cx, move || Style {
                                padding_left: 5.0,
                                padding_right: 5.0,
                                padding_top: 5.0,
                                padding_bottom: 5.0,
                                ..Default::default()
                            }),
                        )
                    })
                    .style(cx, || Style {
                        align_items: Some(AlignItems::Center),
                        ..Default::default()
                    })
                },
                move || {
                    if workspace.clone().path.is_some() {
                        set_workbench_command
                            .set(Some(LapceWorkbenchCommand::Palette));
                    } else {
                        set_workbench_command
                            .set(Some(LapceWorkbenchCommand::PaletteWorkspace));
                    }
                },
            )
            .style(cx, move || Style {
                flex_basis: Dimension::Points(0.0),
                flex_grow: 10.0,
                min_width: Dimension::Points(200.0),
                max_width: Dimension::Points(500.0),
                height: Dimension::Points(26.0),
                justify_content: Some(JustifyContent::Center),
                align_items: Some(AlignItems::Center),
                border: 1.0,
                border_radius: 6.0,
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
    workspace: Arc<LapceWorkspace>,
    proxy_data: &ProxyData,
    set_workbench_command: WriteSignal<Option<LapceWorkbenchCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    stack(cx, move |cx| {
        (
            left(cx, proxy_data, config),
            middle(cx, workspace, set_workbench_command, config),
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
