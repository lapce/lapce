use std::{path::PathBuf, rc::Rc};

use floem::{
    action::show_context_menu,
    event::{Event, EventListener},
    menu::{Menu, MenuItem},
    peniko::kurbo::Rect,
    reactive::{create_memo, create_rw_signal},
    style::{CursorStyle, Style},
    view::View,
    views::{container, dyn_stack, label, scroll, stack, svg, Decorators},
};
use lapce_core::buffer::rope_text::RopeText;
use lapce_rpc::source_control::FileDiff;

use super::{kind::PanelKind, position::PanelPosition, view::panel_header};
use crate::{
    command::{CommandKind, InternalCommand, LapceCommand, LapceWorkbenchCommand},
    config::{color::LapceColor, icon::LapceIcons},
    editor::view::{cursor_caret, editor_view, LineRegion},
    settings::checkbox,
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};

pub fn source_control_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let focus = source_control.common.focus;
    let editor = source_control.editor.clone();
    let doc = editor.view.doc;
    let cursor = editor.cursor;
    let viewport = editor.viewport;
    let window_origin = editor.window_origin;
    let editor = create_rw_signal(editor);
    let is_active = move |tracked| {
        let focus = if tracked {
            focus.get()
        } else {
            focus.get_untracked()
        };
        focus == Focus::Panel(PanelKind::SourceControl)
    };
    let is_empty = create_memo(move |_| {
        let doc = doc.get();
        doc.buffer.with(|b| b.len() == 0)
    });
    let debug_breakline = create_memo(move |_| None);

    stack((
        stack((
            container({
                scroll({
                    let view = stack((
                        editor_view(
                            editor.get_untracked(),
                            debug_breakline,
                            is_active,
                        )
                        .style(|s| s.min_size_pct(100.0, 100.0)),
                        label(|| "Commit Message".to_string()).style(move |s| {
                            let config = config.get();
                            s.absolute()
                                .items_center()
                                .height(config.editor.line_height() as f32)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .apply_if(!is_empty.get(), |s| s.hide())
                        }),
                    ))
                    .style(|s| {
                        s.absolute()
                            .min_size_pct(100.0, 100.0)
                            .padding_left(10.0)
                            .padding_vert(6.0)
                            .hover(|s| s.cursor(CursorStyle::Text))
                    });
                    let id = view.id();
                    view.on_event_cont(EventListener::PointerDown, move |event| {
                        let event = event.clone().offset((10.0, 6.0));
                        if let Event::PointerDown(pointer_event) = event {
                            id.request_active();
                            editor.get_untracked().pointer_down(&pointer_event);
                        }
                    })
                    .on_event_stop(EventListener::PointerMove, move |event| {
                        let event = event.clone().offset((10.0, 6.0));
                        if let Event::PointerMove(pointer_event) = event {
                            editor.get_untracked().pointer_move(&pointer_event);
                        }
                    })
                    .on_event_stop(
                        EventListener::PointerUp,
                        move |event| {
                            let event = event.clone().offset((10.0, 6.0));
                            if let Event::PointerUp(pointer_event) = event {
                                editor.get_untracked().pointer_up(&pointer_event);
                            }
                        },
                    )
                })
                .on_move(move |pos| {
                    window_origin.set(pos + (10.0, 6.0));
                })
                .on_scroll(move |rect| {
                    viewport.set(rect);
                })
                .on_ensure_visible(move || {
                    let cursor = cursor.get();
                    let offset = cursor.offset();
                    let editor = editor.get_untracked();
                    let editor_view = editor.view.clone();
                    editor_view.doc.track();
                    editor_view.kind.track();
                    let LineRegion { x, width, rvline } = cursor_caret(
                        &editor_view,
                        offset,
                        !cursor.is_insert(),
                        cursor.affinity,
                    );
                    let config = config.get_untracked();
                    let line_height = config.editor.line_height();
                    // TODO: is there a way to avoid the calculation of the vline here?
                    let vline = editor.view.vline_of_rvline(rvline);
                    Rect::from_origin_size(
                        (x, (vline.get() * line_height) as f64),
                        (width, line_height as f64),
                    )
                    .inflate(30.0, 10.0)
                })
                .style(|s| s.absolute().size_pct(100.0, 100.0))
            })
            .style(move |s| {
                let config = config.get();
                s.width_pct(100.0)
                    .height(120.0)
                    .border(1.0)
                    .padding(-1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
            }),
            {
                let source_control = source_control.clone();
                label(|| "Commit".to_string())
                    .on_click_stop(move |_| {
                        source_control.commit();
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.margin_top(10.0)
                            .line_height(1.6)
                            .width_pct(100.0)
                            .justify_center()
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                )
                            })
                            .active(|s| {
                                s.background(config.color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            })
                    })
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0).padding(10.0)),
        stack((
            panel_header("Changes".to_string(), config),
            file_diffs_view(source_control),
        ))
        .style(|s| s.flex_col().size_pct(100.0, 100.0)),
    ))
    .on_event_stop(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::SourceControl) {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        }
    })
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

fn file_diffs_view(source_control: SourceControlData) -> impl View {
    let file_diffs = source_control.file_diffs;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let lapce_command = source_control.common.lapce_command;
    let internal_command = source_control.common.internal_command;

    let view_fn = move |(path, (diff, checked)): (PathBuf, (FileDiff, bool))| {
        let diff_for_style = diff.clone();
        let full_path = path.clone();
        let diff_for_menu = diff.clone();
        let path_for_click = full_path.clone();

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
        stack((
            checkbox(move || checked, config)
                .style(|s| s.hover(|s| s.cursor(CursorStyle::Pointer)))
                .on_click_stop(move |_| {
                    file_diffs.update(|diffs| {
                        if let Some((_, checked)) = diffs.get_mut(&full_path) {
                            *checked = !*checked;
                        }
                    });
                }),
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
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
                s.text_ellipsis().margin_right(6.0).max_width(max_width)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0)
                    .flex_basis(0.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(0.0)
            }),
            container({
                svg(move || {
                    let svg = match &diff {
                        FileDiff::Modified(_) => LapceIcons::SCM_DIFF_MODIFIED,
                        FileDiff::Added(_) => LapceIcons::SCM_DIFF_ADDED,
                        FileDiff::Deleted(_) => LapceIcons::SCM_DIFF_REMOVED,
                        FileDiff::Renamed(_, _) => LapceIcons::SCM_DIFF_RENAMED,
                    };
                    config.get().ui_svg(svg)
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = match &diff_for_style {
                        FileDiff::Modified(_) => LapceColor::SOURCE_CONTROL_MODIFIED,
                        FileDiff::Added(_) => LapceColor::SOURCE_CONTROL_ADDED,
                        FileDiff::Deleted(_) => LapceColor::SOURCE_CONTROL_REMOVED,
                        FileDiff::Renamed(_, _) => {
                            LapceColor::SOURCE_CONTROL_MODIFIED
                        }
                    };
                    let color = config.color(color);
                    s.min_width(size).size(size, size).color(color)
                })
            })
            .style(|s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .padding_right(20.0)
                    .items_center()
                    .justify_end()
            }),
        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .on_event_cont(EventListener::PointerDown, move |event| {
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
                if pointer_event.button.is_secondary() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Discard Changes").action(discard));
                    show_context_menu(menu, None);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.padding_left(10.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        scroll({
            dyn_stack(
                move || file_diffs.get(),
                |(path, (diff, checked))| {
                    (path.to_path_buf(), diff.clone(), *checked)
                },
                view_fn,
            )
            .style(|s| s.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}
