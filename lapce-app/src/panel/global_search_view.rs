use std::{path::PathBuf, sync::Arc};

use floem::{
    peniko::kurbo::{Point, Size},
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, SignalGet, SignalGetUntracked,
        SignalSet, SignalUpdate,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{container, label, list, scroll, stack, svg, Decorators},
    AppContext,
};
use indexmap::IndexMap;

use crate::{
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    editor::location::{EditorLocation, EditorPosition},
    global_search::SearchMatchData,
    text_input::text_input,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

use super::position::PanelPosition;

pub fn global_search_panel(
    window_tab_data: Arc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let global_search = window_tab_data.global_search.clone();
    let doc = global_search.editor.doc;
    let cursor = global_search.editor.cursor;
    let config = global_search.common.config;
    let workspace = global_search.common.workspace.clone();
    let result = global_search.search_result;
    let internal_command = global_search.common.internal_command;

    let cx = AppContext::get_current();
    let cursor_x = create_rw_signal(cx.scope, 0.0);

    stack(|| {
        (
            container(|| {
                container(|| {
                    scroll(|| {
                        text_input(doc, cursor, config).on_cursor_pos(move |point| {
                            cursor_x.set(point.x);
                        })
                    })
                    .hide_bar(|| true)
                    .on_ensure_visible(move || {
                        Size::new(20.0, 0.0)
                            .to_rect()
                            .with_origin(Point::new(cursor_x.get() - 10.0, 0.0))
                    })
                    .style(|| Style::BASE.width_pct(100.0))
                })
                .style(move || {
                    Style::BASE
                        .width_pct(100.0)
                        .padding_px(6.0)
                        .border(1.0)
                        .border_radius(6.0)
                        .border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                })
            })
            .style(|| Style::BASE.width_pct(100.0).padding_px(10.0)),
            search_result(workspace, result, internal_command, config),
        )
    })
    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0).flex_col())
}

fn search_result(
    workspace: Arc<LapceWorkspace>,
    result: RwSignal<IndexMap<PathBuf, SearchMatchData>>,
    internal_command: RwSignal<Option<InternalCommand>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    scroll(move || {
        list(
            move || result.get(),
            move |(path, _)| path.to_owned(),
            move |(path, match_data)| {
                let full_path = path.clone();
                let path = if let Some(workspace_path) = workspace.path.as_ref() {
                    path.strip_prefix(workspace_path)
                        .unwrap_or(&full_path)
                        .to_path_buf()
                } else {
                    path
                };
                let style_path = path.clone();

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

                let expanded = match_data.expanded;

                stack(|| {
                    (
                        stack(|| {
                            (
                                svg(move || {
                                    config.get().ui_svg(if expanded.get() {
                                        LapceIcons::ITEM_OPENED
                                    } else {
                                        LapceIcons::ITEM_CLOSED
                                    })
                                })
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    Style::BASE
                                        .margin_left_px(10.0)
                                        .margin_right_px(6.0)
                                        .size_px(size, size)
                                        .min_size_px(size, size)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                }),
                                svg(move || config.get().file_svg(&path).0).style(
                                    move || {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        let color =
                                            config.file_svg(&style_path).1.copied();
                                        Style::BASE
                                            .margin_right_px(6.0)
                                            .size_px(size, size)
                                            .min_size_px(size, size)
                                            .apply_opt(color, Style::color)
                                    },
                                ),
                                stack(|| {
                                    (
                                        label(move || file_name.clone()).style(
                                            || {
                                                Style::BASE
                                                    .margin_right_px(6.0)
                                                    .max_width_pct(100.0)
                                                    .text_ellipsis()
                                            },
                                        ),
                                        label(move || folder.clone()).style(
                                            move || {
                                                Style::BASE
                                                    .color(*config.get().get_color(
                                                        LapceColor::EDITOR_DIM,
                                                    ))
                                                    .min_width_px(0.0)
                                                    .text_ellipsis()
                                            },
                                        ),
                                    )
                                })
                                .style(move || {
                                    Style::BASE.min_width_px(0.0).items_center()
                                }),
                            )
                        })
                        .on_click(move |_| {
                            expanded.update(|expanded| *expanded = !*expanded);
                            true
                        })
                        .style(move || {
                            Style::BASE
                                .width_pct(100.0)
                                .min_width_pct(100.0)
                                .items_center()
                        })
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        }),
                        list(
                            move || {
                                if expanded.get() {
                                    match_data.matches.get()
                                } else {
                                    im::Vector::new()
                                }
                            },
                            |m| m.line,
                            move |m| {
                                let path = full_path.clone();
                                let line_number = m.line;
                                stack(|| {
                                    (label(move || {
                                        let config = config.get();
                                        let content = if config
                                            .ui
                                            .trim_search_results_whitespace
                                        {
                                            m.line_content.trim()
                                        } else {
                                            &m.line_content
                                        };
                                        format!("{}: {content}", m.line + 1,)
                                    })
                                    .style(
                                        move || {
                                            let config = config.get();
                                            let icon_size =
                                                config.ui.icon_size() as f32;
                                            Style::BASE.margin_left_px(
                                                10.0 + icon_size + 6.0,
                                            )
                                        },
                                    ),)
                                })
                                .on_click(move |_| {
                                    internal_command.set(Some(
                                        InternalCommand::JumpToLocation {
                                            location: EditorLocation {
                                                path: path.clone(),
                                                position: Some(
                                                    EditorPosition::Line(
                                                        line_number,
                                                    ),
                                                ),
                                                scroll_offset: None,
                                                ignore_unconfirmed: false,
                                                same_editor_tab: false,
                                            },
                                        },
                                    ));
                                    true
                                })
                                .hover_style(
                                    move || {
                                        Style::BASE
                                            .cursor(CursorStyle::Pointer)
                                            .background(*config.get().get_color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            ))
                                    },
                                )
                            },
                        )
                        .style(|| Style::BASE.flex_col()),
                    )
                })
                .style(|| Style::BASE.flex_col())
            },
        )
        .style(|| Style::BASE.flex_col().min_width_pct(100.0).line_height(1.6))
    })
    .scroll_bar_color(move || *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR))
    .style(|| Style::BASE.size_pct(100.0, 100.0))
}
