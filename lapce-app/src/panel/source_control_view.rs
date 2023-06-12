use std::sync::Arc;

use floem::{
    peniko::kurbo::Rect,
    reactive::{create_memo, create_rw_signal, SignalGet, SignalSet, SignalUpdate},
    style::{CursorStyle, Style},
    view::View,
    views::{container, label, list, scroll, stack, svg, Decorators},
    ViewContext,
};
use lapce_rpc::source_control::FileDiff;

use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    editor::EditorData,
    id::EditorId,
    settings::checkbox,
    source_control::SourceControlData,
    text_area::text_area,
    window_tab::WindowTabData,
};

use super::{position::PanelPosition, view::panel_header};

pub fn source_control_panel(
    window_tab_data: Arc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let cx = ViewContext::get_current();
    let editor = EditorData::new_local(
        cx.scope,
        EditorId::next(),
        window_tab_data.common.clone(),
    );

    stack(|| {
        (
            stack(|| {
                (
                    text_area(editor)
                        .style(|| Style::BASE.width_pct(100.0).height_px(120.0)),
                    label(|| "Commit".to_string())
                        .style(move || {
                            Style::BASE
                                .margin_top_px(10.0)
                                .line_height(1.6)
                                .width_pct(100.0)
                                .justify_center()
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                        .on_click(move |_| {
                            // on_click();
                            true
                        })
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .active_style(move || {
                            Style::BASE.background(*config.get().get_color(
                                LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                            ))
                        }),
                )
            })
            .style(|| Style::BASE.flex_col().width_pct(100.0).padding_px(10.0)),
            stack(|| {
                (
                    panel_header("Changes".to_string(), config),
                    file_diffs_view(source_control),
                )
            })
            .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0)),
        )
    })
    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
}

fn file_diffs_view(source_control: SourceControlData) -> impl View {
    let file_diffs = source_control.file_diffs;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace;
    let cx = ViewContext::get_current();
    let panel_rect = create_rw_signal(cx.scope, Rect::ZERO);
    let panel_width = create_memo(cx.scope, move |_| panel_rect.get().width());
    container(|| {
        scroll(|| {
            list(
                move || file_diffs.get(),
                |(path, (diff, checked))| {
                    (path.to_path_buf(), diff.clone(), *checked)
                },
                move |(path, (diff, checked))| {
                    let diff_for_style = diff.clone();
                    let full_path = path.clone();
                    let path = if let Some(workspace_path) = workspace.path.as_ref()
                    {
                        path.strip_prefix(workspace_path)
                            .unwrap_or(&full_path)
                            .to_path_buf()
                    } else {
                        path
                    };
                    let file_name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let folder = path
                        .parent()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let style_path = path.clone();
                    stack(|| {
                        (
                            checkbox(move || checked, config)
                                .on_click(move |_| {
                                    file_diffs.update(|diffs| {
                                        if let Some((_, checked)) =
                                            diffs.get_mut(&full_path)
                                        {
                                            *checked = !*checked;
                                        }
                                    });
                                    true
                                })
                                .hover_style(|| {
                                    Style::BASE.cursor(CursorStyle::Pointer)
                                }),
                            svg(move || config.get().file_svg(&path).0).style(
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color =
                                        config.file_svg(&style_path).1.copied();
                                    Style::BASE
                                        .min_width_px(size)
                                        .size_px(size, size)
                                        .margin_px(6.0)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            label(move || file_name.clone()).style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                let max_width = panel_width.get() as f32
                                    - 10.0
                                    - size
                                    - 6.0
                                    - size
                                    - 6.0
                                    - 10.0
                                    - size
                                    - 6.0;
                                Style::BASE
                                    .text_ellipsis()
                                    .margin_right_px(6.0)
                                    .max_width_px(max_width)
                            }),
                            label(move || folder.clone()).style(move || {
                                Style::BASE
                                    .text_ellipsis()
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_px(0.0)
                            }),
                            container(|| {
                                svg(move || {
                                    let svg = match &diff {
                                        FileDiff::Modified(_) => {
                                            LapceIcons::SCM_DIFF_MODIFIED
                                        }
                                        FileDiff::Added(_) => {
                                            LapceIcons::SCM_DIFF_ADDED
                                        }
                                        FileDiff::Deleted(_) => {
                                            LapceIcons::SCM_DIFF_REMOVED
                                        }
                                        FileDiff::Renamed(_, _) => {
                                            LapceIcons::SCM_DIFF_RENAMED
                                        }
                                    };
                                    config.get().ui_svg(svg)
                                })
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color = match &diff_for_style {
                                        FileDiff::Modified(_) => {
                                            LapceColor::SOURCE_CONTROL_MODIFIED
                                        }
                                        FileDiff::Added(_) => {
                                            LapceColor::SOURCE_CONTROL_ADDED
                                        }
                                        FileDiff::Deleted(_) => {
                                            LapceColor::SOURCE_CONTROL_REMOVED
                                        }
                                        FileDiff::Renamed(_, _) => {
                                            LapceColor::SOURCE_CONTROL_MODIFIED
                                        }
                                    };
                                    let color = config.get_color(color);
                                    Style::BASE
                                        .min_width_px(size)
                                        .size_px(size, size)
                                        .color(*color)
                                })
                            })
                            .style(|| {
                                Style::BASE
                                    .absolute()
                                    .size_pct(100.0, 100.0)
                                    .padding_right_px(20.0)
                                    .items_center()
                                    .justify_end()
                            }),
                        )
                    })
                    .on_click(move |_| true)
                    .style(move || {
                        Style::BASE
                            .padding_left_px(10.0)
                            .padding_right_px(10.0)
                            .width_pct(100.0)
                            .items_center()
                    })
                    .hover_style(move || {
                        Style::BASE.background(
                            *config
                                .get()
                                .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
                },
            )
            .style(|| Style::BASE.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |_, rect| {
        panel_rect.set(rect);
    })
    .style(|| Style::BASE.size_pct(100.0, 100.0))
}
