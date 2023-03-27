use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalSet, SignalWith, WriteSignal,
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
    source_control::SourceControlData,
    workspace::LapceWorkspace,
};

fn left(
    cx: AppContext,
    source_control: RwSignal<SourceControlData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let branch =
        move || source_control.with(|source_control| source_control.branch.clone());
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
                    label(cx, move || branch()).style(cx, || Style {
                        margin_left: Some(10.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, move || Style {
                display: if branch().is_empty() {
                    Display::None
                } else {
                    Display::Flex
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
    source_control: RwSignal<SourceControlData>,
    set_workbench_command: WriteSignal<Option<LapceWorkbenchCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack(cx, move |cx| {
        (
            left(cx, source_control, config),
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
