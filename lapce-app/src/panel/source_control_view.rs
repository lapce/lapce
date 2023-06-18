use std::{path::PathBuf, sync::Arc};

use floem::{
    event::{Event, EventListener},
    menu::{Menu, MenuItem},
    peniko::kurbo::{Point, Rect, Size},
    reactive::{
        create_memo, create_rw_signal, SignalGet, SignalGetUntracked, SignalSet,
        SignalUpdate, SignalWith,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{container, label, list, scroll, stack, svg, Decorators},
    ViewContext,
};
use lapce_core::buffer::rope_text::RopeText;
use lapce_rpc::source_control::FileDiff;

use super::{kind::PanelKind, position::PanelPosition, view::panel_header};
use crate::{
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand},
    config::{color::LapceColor, icon::LapceIcons},
    editor::view::{cursor_caret, editor_view, CursorRender},
    settings::checkbox,
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};

pub fn source_control_panel(
    window_tab_data: Arc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let focus = source_control.common.focus;
    let editor = source_control.editor.clone();
    let doc = editor.doc;
    let cursor = editor.cursor;
    let viewport = editor.viewport;
    let cx = ViewContext::get_current();
    let editor = create_rw_signal(cx.scope, editor);
    let is_active = move || focus.get() == Focus::Panel(PanelKind::SourceControl);
    let is_empty =
        create_memo(cx.scope, move |_| doc.with(|doc| doc.buffer().len() == 0));

    stack(|| {
        (
            stack(|| {
                (
                    container(|| {
                        scroll(|| {
                            let view = stack(|| {
                                (
                                    editor_view(editor, is_active).style(|| {
                                        Style::BASE.min_size_pct(100.0, 100.0)
                                    }),
                                    label(|| "Commit Message".to_string()).style(
                                        move || {
                                            let config = config.get();
                                            Style::BASE
                                                .absolute()
                                                .items_center()
                                                .height_px(
                                                    config.editor.line_height()
                                                        as f32,
                                                )
                                                .color(*config.get_color(
                                                    LapceColor::EDITOR_DIM,
                                                ))
                                                .apply_if(!is_empty.get(), |s| {
                                                    s.hide()
                                                })
                                        },
                                    ),
                                )
                            })
                            .style(|| {
                                Style::BASE
                                    .min_size_pct(100.0, 100.0)
                                    .padding_left_px(10.0)
                                    .padding_vert_px(6.0)
                            });
                            let id = view.id();
                            view.on_event(EventListener::PointerDown, move |event| {
                                let event = event.clone().offset((10.0, 6.0));
                                if let Event::PointerDown(pointer_event) = event {
                                    id.request_active();
                                    let editor = editor.get_untracked();
                                    editor.pointer_down(&pointer_event);
                                }
                                false
                            })
                            .on_event(EventListener::PointerMove, move |event| {
                                let event = event.clone().offset((10.0, 6.0));
                                if let Event::PointerMove(pointer_event) = event {
                                    let editor = editor.get_untracked();
                                    editor.pointer_move(&pointer_event);
                                }
                                true
                            })
                            .on_event(
                                EventListener::PointerUp,
                                move |event| {
                                    let event = event.clone().offset((10.0, 6.0));
                                    if let Event::PointerUp(pointer_event) = event {
                                        let editor = editor.get_untracked();
                                        editor.pointer_up(&pointer_event);
                                    }
                                    true
                                },
                            )
                        })
                        .on_scroll(move |rect| {
                            viewport.set(rect);
                        })
                        .scroll_bar_color(move || {
                            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
                        })
                        .on_ensure_visible(move || {
                            let cursor = cursor.get();
                            let offset = cursor.offset();
                            let view = editor.with(|e| e.view.clone());
                            let caret =
                                cursor_caret(&view, offset, !cursor.is_insert());
                            let config = config.get_untracked();
                            let line_height = config.editor.line_height();
                            if let CursorRender::Caret { x, width, line } = caret {
                                Size::new(width, line_height as f64)
                                    .to_rect()
                                    .with_origin(Point::new(
                                        x,
                                        (line * line_height) as f64,
                                    ))
                                    .inflate(30.0, 10.0)
                            } else {
                                Rect::ZERO
                            }
                        })
                        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
                    })
                    .style(move || {
                        let config = config.get();
                        Style::BASE
                            .width_pct(100.0)
                            .height_px(120.0)
                            .border(1.0)
                            .padding_px(-1.0)
                            .border_radius(6.0)
                            .border_color(
                                *config.get_color(LapceColor::LAPCE_BORDER),
                            )
                            .background(
                                *config.get_color(LapceColor::EDITOR_BACKGROUND),
                            )
                    }),
                    {
                        let source_control = source_control.clone();
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
                                source_control.commit();
                                true
                            })
                            .hover_style(move || {
                                Style::BASE.cursor(CursorStyle::Pointer).background(
                                    *config.get().get_color(
                                        LapceColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                            .active_style(move || {
                                Style::BASE.background(*config.get().get_color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            })
                    },
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
    .on_event(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::SourceControl) {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        }
        true
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
    let lapce_command = source_control.common.lapce_command;

    let view_fn = move |(path, (diff, checked)): (PathBuf, (FileDiff, bool))| {
        let diff_for_style = diff.clone();
        let full_path = path.clone();
        let diff_for_menu = diff.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
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
                            if let Some((_, checked)) = diffs.get_mut(&full_path) {
                                *checked = !*checked;
                            }
                        });
                        true
                    })
                    .hover_style(|| Style::BASE.cursor(CursorStyle::Pointer)),
                svg(move || config.get().file_svg(&path).0).style(move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = config.file_svg(&style_path).1.copied();
                    Style::BASE
                        .min_width_px(size)
                        .size_px(size, size)
                        .margin_px(6.0)
                        .apply_opt(color, Style::color)
                }),
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
                        .flex_grow(1.0)
                        .flex_basis_px(0.0)
                        .color(*config.get().get_color(LapceColor::EDITOR_DIM))
                        .min_width_px(0.0)
                }),
                container(|| {
                    svg(move || {
                        let svg = match &diff {
                            FileDiff::Modified(_) => LapceIcons::SCM_DIFF_MODIFIED,
                            FileDiff::Added(_) => LapceIcons::SCM_DIFF_ADDED,
                            FileDiff::Deleted(_) => LapceIcons::SCM_DIFF_REMOVED,
                            FileDiff::Renamed(_, _) => LapceIcons::SCM_DIFF_RENAMED,
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
                            FileDiff::Added(_) => LapceColor::SOURCE_CONTROL_ADDED,
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
        .on_event(EventListener::PointerDown, move |event| {
            let diff_for_menu = diff_for_menu.clone();

            let discard = move || {
                lapce_command.send(LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges,
                    ),
                    data: Some(serde_json::json!(diff_for_menu.clone())),
                });
            };

            if let Event::PointerDown(pointer_event) = event {
                if pointer_event.button.is_right() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Discard Changes").action(discard));
                    cx.id.show_context_menu(menu, Point::ZERO);
                }
            }
            false
        })
        .style(move || {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            Style::BASE
                .padding_left_px(10.0)
                .padding_right_px(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
        })
        .hover_style(move || {
            Style::BASE.background(
                *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
            )
        })
    };

    container(|| {
        scroll(|| {
            list(
                move || file_diffs.get(),
                |(path, (diff, checked))| {
                    (path.to_path_buf(), diff.clone(), *checked)
                },
                view_fn,
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
