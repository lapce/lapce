use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalSet, SignalWith, WriteSignal,
    },
    style::{AlignItems, CursorStyle, Dimension, Display, JustifyContent, Style},
    view::View,
    views::{click, container, stack, Decorators},
    views::{label, svg},
};

use crate::{
    app::clickable_icon,
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
    let branch = move || {
        source_control.with(|source_control| {
            format!(
                "{}{}",
                source_control.branch,
                if source_control.file_diffs.is_empty() {
                    ""
                } else {
                    "*"
                }
            )
        })
    };
    stack(cx, move |cx| {
        (
            container(cx, move |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::REMOTE)).style(
                    cx,
                    move || {
                        Style::default().dimension_px(26.0, 26.0).color(
                            *config.get().get_color(LapceColor::LAPCE_REMOTE_ICON),
                        )
                    },
                )
            })
            .style(cx, move || {
                Style::default()
                    .height_pct(1.0)
                    .padding_horiz(10.0)
                    .items_center()
                    .background(
                        *config.get().get_color(LapceColor::LAPCE_REMOTE_LOCAL),
                    )
            }),
            stack(cx, move |cx| {
                (
                    svg(cx, move || config.get().ui_svg(LapceIcons::SCM)).style(
                        cx,
                        move || {
                            let config = config.get();
                            let icon_size = config.ui.icon_size() as f32;
                            Style::default()
                                .dimension_px(icon_size, icon_size)
                                .color(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                )
                        },
                    ),
                    label(cx, move || branch())
                        .style(cx, || Style::default().margin_left(10.0)),
                )
            })
            .style(cx, move || {
                Style::default()
                    .display(if branch().is_empty() {
                        Display::None
                    } else {
                        Display::Flex
                    })
                    .height_pct(1.0)
                    .padding_horiz(10.0)
                    .border_right(1.0)
                    .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
                    .align_items(Some(AlignItems::Center))
            }),
        )
    })
    .style(cx, move || {
        Style::default()
            .height_pct(1.0)
            .flex_basis(Dimension::Points(0.0))
            .flex_grow(1.0)
            .items_center()
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
                    clickable_icon(
                        cx,
                        LapceIcons::LOCATION_BACKWARD,
                        || {},
                        || false,
                        config,
                    )
                    .style(cx, move || Style::default().margin_horiz(6.0)),
                    clickable_icon(
                        cx,
                        LapceIcons::LOCATION_FORWARD,
                        || {},
                        || false,
                        config,
                    )
                    .style(cx, move || Style::default().margin_right(6.0)),
                )
            })
            .style(cx, || {
                Style::default()
                    .flex_basis(Dimension::Points(0.0))
                    .flex_grow(1.0)
                    .justify_content(Some(JustifyContent::FlexEnd))
            }),
            click(
                cx,
                |cx| {
                    stack(cx, |cx| {
                        (
                            svg(cx, move || config.get().ui_svg(LapceIcons::SEARCH))
                                .style(cx, move || {
                                    let config = config.get();
                                    let icon_size = config.ui.icon_size() as f32;
                                    Style::default()
                                        .dimension_px(icon_size, icon_size)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                }),
                            label(cx, move || {
                                if let Some(s) = local_workspace.display() {
                                    s
                                } else {
                                    "Open Folder".to_string()
                                }
                            })
                            .style(cx, || {
                                Style::default()
                                    .padding_left(10.0)
                                    .padding_right(5.0)
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
                                            let config = config.get();
                                            let icon_size =
                                                config.ui.icon_size() as f32;
                                            Style::default()
                                                .dimension_px(icon_size, icon_size)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        },
                                    )
                                },
                                || {},
                            )
                            .style(cx, move || Style::default().padding(5.0)),
                        )
                    })
                    .style(cx, || {
                        Style::default().align_items(Some(AlignItems::Center))
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
            .style(cx, move || {
                let config = config.get();
                Style::default()
                    .flex_basis(Dimension::Points(0.0))
                    .flex_grow(10.0)
                    .min_width_px(200.0)
                    .max_width_px(500.0)
                    .height_px(26.0)
                    .justify_content(Some(JustifyContent::Center))
                    .align_items(Some(AlignItems::Center))
                    .border(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .border_radius(6.0)
                    .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            }),
            container(cx, move |cx| {
                clickable_icon(
                    cx,
                    LapceIcons::START,
                    move || {
                        set_workbench_command
                            .set(Some(LapceWorkbenchCommand::PaletteRunAndDebug))
                    },
                    || false,
                    config,
                )
                .style(cx, move || Style::default().margin_horiz(6.0))
            })
            .style(cx, move || {
                Style::default()
                    .flex_basis(Dimension::Points(0.0))
                    .flex_grow(1.0)
                    .justify_content(Some(JustifyContent::FlexStart))
            }),
        )
    })
    .style(cx, || {
        Style::default()
            .flex_basis(Dimension::Points(0.0))
            .flex_grow(2.0)
            .align_items(Some(AlignItems::Center))
            .justify_content(Some(JustifyContent::Center))
    })
}

fn right(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> impl View {
    container(cx, move |cx| {
        clickable_icon(cx, LapceIcons::SETTINGS, || {}, || false, config)
            .style(cx, move || Style::default().margin_horiz(6.0))
    })
    .style(cx, || {
        Style::default()
            .flex_basis(Dimension::Points(0.0))
            .flex_grow(1.0)
            .justify_content(Some(JustifyContent::FlexEnd))
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
    .style(cx, move || {
        let config = config.get();
        Style::default()
            .width_pct(1.0)
            .height_px(37.0)
            .items_center()
            .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
    })
}
