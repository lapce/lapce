use std::{
    ops::Range,
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc},
};

use floem::{
    app::AppContext,
    cosmic_text::{Attrs, AttrsList, Style as FontStyle, TextLayout, Weight},
    event::{Event, EventListner},
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_memo, create_rw_signal, provide_context, use_context, ReadSignal,
        RwSignal, SignalGet, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWith, SignalWithUntracked,
    },
    stack::stack,
    style::{
        AlignContent, AlignItems, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    view::View,
    views::{click, double_click, svg, VirtualListVector},
    views::{
        clip, container, container_box, list, tab, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize,
    },
    views::{label, rich_text, scroll},
};
use lapce_core::{
    cursor::{ColPosition, Cursor, CursorMode},
    mode::{Mode, VisualMode},
    selection::Selection,
};
use lsp_types::{CompletionItemKind, DiagnosticSeverity};

use crate::{
    code_action::CodeActionStatus,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    db::LapceDb,
    doc::{DocContent, DocLine, Document},
    editor::{self, EditorData},
    editor_tab::{EditorTabChild, EditorTabData},
    focus_text::focus_text,
    id::{EditorId, EditorTabId, SplitId},
    main_split::{MainSplitData, SplitContent, SplitData, SplitDirection},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData, PaletteStatus,
    },
    panel::{
        kind::PanelKind,
        position::{PanelContainerPosition, PanelPosition},
    },
    terminal::{
        data::TerminalData, panel::TerminalPanelData, tab::TerminalTabData,
        view::terminal_view,
    },
    title::title,
    window::WindowData,
    window_tab::{Focus, WindowTabData},
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

#[derive(Clone, Debug)]
enum CursorRender {
    CurrentLine { line: usize },
    Selection { x: f64, width: f64, line: usize },
    Caret { x: f64, width: f64, line: usize },
}

fn cursor_caret(doc: &Document, offset: usize, block: bool) -> CursorRender {
    let (line, col) = doc.buffer().offset_to_line_col(offset);
    let phantom_text = doc.line_phantom_text(line);
    let col = phantom_text.col_after(col, block);
    let x0 = doc.line_point_of_line_col(line, col, 12).x;
    if block {
        let right_offset = doc.buffer().move_right(offset, Mode::Insert, 1);
        let (_, right_col) = doc.buffer().offset_to_line_col(right_offset);
        let x1 = doc.line_point_of_line_col(line, right_col, 12).x;

        let width = if x1 > x0 { x1 - x0 } else { 7.0 };
        CursorRender::Caret { x: x0, width, line }
    } else {
        CursorRender::Caret {
            x: x0 - 1.0,
            width: 2.0,
            line,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn visual_cursor(
    doc: &Document,
    start: usize,
    end: usize,
    mode: &VisualMode,
    horiz: Option<&ColPosition>,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let (start_line, start_col) = doc.buffer().offset_to_line_col(start.min(end));
    let (end_line, end_col) = doc.buffer().offset_to_line_col(start.max(end));
    let (cursor_line, _) = doc.buffer().offset_to_line_col(end);

    let mut renders = Vec::new();

    for line in min_line..max_line + 1 {
        if line < start_line {
            continue;
        }

        if line > end_line {
            break;
        }

        let left_col = match mode {
            VisualMode::Normal => {
                if start_line == line {
                    start_col
                } else {
                    0
                }
            }
            VisualMode::Linewise => 0,
            VisualMode::Blockwise => {
                let max_col = doc.buffer().line_end_col(line, false);
                let left = start_col.min(end_col);
                if left > max_col {
                    continue;
                }
                left
            }
        };

        let (right_col, line_end) = match mode {
            VisualMode::Normal => {
                if line == end_line {
                    let max_col = doc.buffer().line_end_col(line, true);

                    let end_offset =
                        doc.buffer().move_right(start.max(end), Mode::Visual, 1);
                    let (_, end_col) = doc.buffer().offset_to_line_col(end_offset);

                    (end_col.min(max_col), false)
                } else {
                    (doc.buffer().line_end_col(line, true), true)
                }
            }
            VisualMode::Linewise => (doc.buffer().line_end_col(line, true), true),
            VisualMode::Blockwise => {
                let max_col = doc.buffer().line_end_col(line, true);
                let right = match horiz.as_ref() {
                    Some(&ColPosition::End) => max_col,
                    _ => {
                        let end_offset =
                            doc.buffer().move_right(start.max(end), Mode::Visual, 1);
                        let (_, end_col) =
                            doc.buffer().offset_to_line_col(end_offset);
                        end_col.max(start_col).min(max_col)
                    }
                };
                (right, false)
            }
        };

        let phantom_text = doc.line_phantom_text(line);
        let left_col = phantom_text.col_after(left_col, false);
        let right_col = phantom_text.col_after(right_col, false);
        let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
        let mut x1 = doc.line_point_of_line_col(line, right_col, 12).x;
        if line_end {
            x1 += char_width;
        }

        renders.push(CursorRender::Selection {
            x: x0,
            width: x1 - x0,
            line,
        });

        if is_active && line == cursor_line {
            let caret = cursor_caret(doc, end, true);
            renders.push(caret);
        }
    }

    renders
}

fn insert_cursor(
    doc: &Document,
    selection: &Selection,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let start = doc.buffer().offset_of_line(min_line);
    let end = doc.buffer().offset_of_line(max_line + 1);
    let regions = selection.regions_in_range(start, end);

    let mut renders = Vec::new();

    for region in regions {
        let cursor_offset = region.end;
        let (cursor_line, _) = doc.buffer().offset_to_line_col(cursor_offset);
        let start = region.start;
        let end = region.end;
        let (start_line, start_col) =
            doc.buffer().offset_to_line_col(start.min(end));
        let (end_line, end_col) = doc.buffer().offset_to_line_col(start.max(end));
        for line in min_line..max_line + 1 {
            if line < start_line {
                continue;
            }

            if line > end_line {
                break;
            }

            let left_col = match line {
                _ if line == start_line => start_col,
                _ => 0,
            };
            let (right_col, line_end) = match line {
                _ if line == end_line => {
                    let max_col = doc.buffer().line_end_col(line, true);
                    (end_col.min(max_col), false)
                }
                _ => (doc.buffer().line_end_col(line, true), true),
            };

            // Shift it by the inlay hints
            let phantom_text = doc.line_phantom_text(line);
            let left_col = phantom_text.col_after(left_col, false);
            let right_col = phantom_text.col_after(right_col, false);

            let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
            let mut x1 = doc.line_point_of_line_col(line, right_col, 12).x;
            if line_end {
                x1 += char_width;
            }

            if line == cursor_line {
                renders.push(CursorRender::CurrentLine { line });
            }

            if start != end {
                renders.push(CursorRender::Selection {
                    x: x0,
                    width: x1 - x0,
                    line,
                });
            }

            if is_active && line == cursor_line {
                let caret = cursor_caret(doc, cursor_offset, false);
                renders.push(caret);
            }
        }
    }
    renders
}

fn editor_gutter(
    cx: AppContext,
    editor: RwSignal<EditorData>,
    is_active: impl Fn() -> bool + 'static + Copy,
) -> impl View {
    let (cursor, viewport, scroll_delta, config) = editor.with(|editor| {
        (
            editor.cursor,
            editor.viewport,
            editor.scroll_delta,
            editor.common.config,
        )
    });

    let padding_left = 10.0;
    let padding_right = 30.0;

    let code_action_line = create_memo(cx.scope, move |_| {
        if is_active() {
            let doc = editor.with(|editor| editor.doc);
            let offset = cursor.with(|cursor| cursor.offset());
            doc.with(|doc| {
                let line = doc.buffer().line_of_offset(offset);
                let has_code_actions = doc
                    .code_actions
                    .get(&offset)
                    .map(|c| !c.1.is_empty())
                    .unwrap_or(false);
                if has_code_actions {
                    Some(line)
                } else {
                    None
                }
            })
        } else {
            None
        }
    });

    let current_line = create_memo(cx.scope, move |_| {
        let doc = editor.with(|editor| editor.doc);
        let (offset, mode) =
            cursor.with(|cursor| (cursor.offset(), cursor.get_mode()));
        let line = doc.with(|doc| {
            let line = doc.buffer().line_of_offset(offset);
            line
        });
        (line, mode)
    });

    stack(cx, |cx| {
        (
            stack(cx, |cx| {
                (
                    label(cx, || "".to_string())
                        .style(cx, move || Style::default().width_pt(padding_left)),
                    label(cx, move || {
                        editor
                            .get()
                            .doc
                            .with(|doc| (doc.buffer().last_line() + 1).to_string())
                    })
                    .style(cx, || Style {
                        ..Default::default()
                    }),
                    label(cx, || "".to_string())
                        .style(cx, move || Style::default().width_pt(padding_right)),
                )
            })
            .style(cx, || Style::default().height_pct(1.0)),
            scroll(cx, |cx| {
                virtual_list(
                    cx,
                    VirtualListDirection::Vertical,
                    move || {
                        let editor = editor.get();
                        current_line.get();
                        editor.doc.get()
                    },
                    move |line: &DocLine| (line.line, current_line.get_untracked()),
                    move |cx, line: DocLine| {
                        let line_number = {
                            let config = config.get_untracked();
                            let (current_line, mode) = current_line.get_untracked();
                            if config.core.modal
                                && config.editor.modal_mode_relative_line_numbers
                                && mode != Mode::Insert
                            {
                                if line.line == current_line {
                                    line.line + 1
                                } else {
                                    line.line.abs_diff(current_line)
                                }
                            } else {
                                line.line + 1
                            }
                        };

                        stack(cx, move |cx| {
                            (
                                label(cx, || "".to_string()).style(cx, move || {
                                    Style::default().width_pt(padding_left)
                                }),
                                label(cx, move || line_number.to_string()).style(
                                    cx,
                                    move || {
                                        let config = config.get();
                                        let (current_line, _) =
                                            current_line.get_untracked();
                                        Style::default()
                                            .flex_grow(1.0)
                                            .apply_if(
                                                current_line != line.line,
                                                move |s| {
                                                    s.color(*config.get_color(
                                                        LapceColor::EDITOR_DIM,
                                                    ))
                                                },
                                            )
                                            .justify_content(Some(
                                                JustifyContent::End,
                                            ))
                                    },
                                ),
                                container(cx, |cx| {
                                    container(cx, |cx| {
                                        click(
                                            cx,
                                            |cx| {
                                                svg(cx, move || {
                                                    config.get().ui_svg(
                                                        LapceIcons::LIGHTBULB,
                                                    )
                                                })
                                                .style(cx, move || {
                                                    let config = config.get();
                                                    let size =
                                                        config.ui.icon_size() as f32;
                                                    Style::default()
                                                        .dimension_pt(size, size)
                                                        .color(*config.get_color(
                                                            LapceColor::LAPCE_WARN,
                                                        ))
                                                })
                                            },
                                            move || {
                                                editor.with_untracked(|editor| {
                                                    editor
                                                        .show_code_actions(cx, true);
                                                });
                                            },
                                        )
                                        .style(cx, move || {
                                            Style::default().display(
                                                if code_action_line.get()
                                                    == Some(line.line)
                                                {
                                                    Display::Flex
                                                } else {
                                                    Display::None
                                                },
                                            )
                                        })
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            Style::default()
                                                .justify_content(Some(
                                                    JustifyContent::Center,
                                                ))
                                                .align_content(Some(
                                                    AlignContent::Center,
                                                ))
                                                .width_pt(
                                                    padding_right - padding_left,
                                                )
                                        },
                                    )
                                })
                                .style(
                                    cx,
                                    move || {
                                        Style::default()
                                            .justify_content(Some(
                                                JustifyContent::End,
                                            ))
                                            .width_pt(padding_right)
                                    },
                                ),
                            )
                        })
                        .style(cx, move || {
                            let config = config.get_untracked();
                            let line_height = config.editor.line_height();
                            Style::default()
                                .align_content(Some(AlignContent::Center))
                                .height_pt(line_height as f32)
                        })
                    },
                    VirtualListItemSize::Fixed(
                        config.get_untracked().editor.line_height() as f64,
                    ),
                )
                .style(cx, move || {
                    let config = config.get();
                    let padding_bottom = if config.editor.scroll_beyond_last_line {
                        viewport.get().height() as f32
                            - config.editor.line_height() as f32
                    } else {
                        0.0
                    };
                    Style::default()
                        .flex_col()
                        .width_pct(1.0)
                        .padding_bottom(padding_bottom)
                })
            })
            .hide_bar()
            .on_event(EventListner::MouseWheel, move |event| {
                if let Event::MouseWheel(wheel) = event {
                    scroll_delta.set(wheel.wheel_delta);
                }
                true
            })
            .on_scroll_to(cx, move || {
                let viewport = viewport.get();
                Some(viewport.origin())
            })
            .style(cx, move || {
                Style::default()
                    .position(Position::Absolute)
                    .background(
                        *config.get().get_color(LapceColor::EDITOR_BACKGROUND),
                    )
                    .dimension_pct(1.0, 1.0)
            }),
        )
    })
    .style(cx, move || {
        let config = config.get();
        Style::default()
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size as f32)
    })
}

fn editor_cursor(
    cx: AppContext,
    editor: RwSignal<EditorData>,
    cursor: ReadSignal<Cursor>,
    viewport: ReadSignal<Rect>,
    is_active: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let cursor = move || {
        let viewport = viewport.get();
        let config = config.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        let is_active = is_active();
        let doc = editor.get().doc;
        doc.with(|doc| {
            cursor.with(|cursor| match &cursor.mode {
                CursorMode::Normal(offset) => {
                    let line = doc.buffer().line_of_offset(*offset);
                    let mut renders =
                        vec![(viewport, CursorRender::CurrentLine { line })];
                    if is_active {
                        let caret = cursor_caret(doc, *offset, true);
                        renders.push((viewport, caret));
                    }
                    renders
                }
                CursorMode::Visual { start, end, mode } => visual_cursor(
                    doc, *start, *end, mode, None, min_line, max_line, 7.5,
                    is_active,
                )
                .into_iter()
                .map(|render| (viewport, render))
                .collect(),
                CursorMode::Insert(selection) => {
                    insert_cursor(doc, selection, min_line, max_line, 7.5, is_active)
                        .into_iter()
                        .map(|render| (viewport, render))
                        .collect()
                }
            })
        })
    };
    let id = AtomicU64::new(0);
    clip(cx, |cx| {
        list(
            cx,
            cursor,
            move |(viewport, cursor)| {
                id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            move |cx, (viewport, cursor)| {
                label(cx, || "".to_string()).style(cx, move || {
                    let config = config.get_untracked();
                    let line_height = config.editor.line_height();

                    let (width, margin_left, line, background) = match &cursor {
                        CursorRender::CurrentLine { line } => (
                            Dimension::Percent(1.0),
                            0.0,
                            *line,
                            *config.get_color(LapceColor::EDITOR_CURRENT_LINE),
                        ),
                        CursorRender::Selection { x, width, line } => (
                            Dimension::Points(*width as f32),
                            (*x - viewport.x0) as f32,
                            *line,
                            *config.get_color(LapceColor::EDITOR_SELECTION),
                        ),
                        CursorRender::Caret { x, width, line } => (
                            Dimension::Points(*width as f32),
                            (*x - viewport.x0) as f32,
                            *line,
                            *config.get_color(LapceColor::EDITOR_CARET),
                        ),
                    };

                    Style::default()
                        .absolute()
                        .width(width)
                        .height_pt(line_height as f32)
                        .margin_left(margin_left)
                        .margin_top((line * line_height) as f32 - viewport.y0 as f32)
                        .background(background)
                })
            },
        )
        .style(cx, move || Style::default().dimension_pct(1.0, 1.0))
    })
    .style(cx, move || {
        Style::default().absolute().dimension_pct(1.0, 1.0)
    })
}

fn editor_extra_style(
    cx: AppContext,
    editor: RwSignal<EditorData>,
    viewport: ReadSignal<Rect>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let list_items = move || {
        let config = config.get();
        let mut doc = editor.get().doc.get();
        let viewport = viewport.get();
        let min = viewport.y0;
        let max = viewport.y1;
        let line_height = config.editor.line_height() as f64;
        let total_len = doc.total_len();
        let start = (min / line_height).floor() as usize;
        let end = ((max / line_height).ceil() as usize).min(total_len);
        doc.slice(start..end)
    };

    clip(cx, |cx| {
        list(
            cx,
            list_items,
            move |line| {
                (
                    editor
                        .with_untracked(|editor| editor.doc)
                        .with_untracked(|doc| doc.content.clone()),
                    line.rev,
                    line.style_rev,
                    line.line,
                )
            },
            move |cx, line| {
                let extra_styles = line.text.extra_style.clone();
                list(
                    cx,
                    move || extra_styles.clone(),
                    |extra| 0,
                    move |cx, extra| {
                        container(cx, |cx| {
                            label(cx, || " ".to_string()).style(cx, move || {
                                let config = config.get();
                                Style::default()
                                    .font_family(config.editor.font_family.clone())
                                    .font_size(config.editor.font_size as f32)
                                    .width_pct(1.0)
                                    .apply_opt(extra.bg_color, Style::background)
                            })
                        })
                        .style(cx, move || {
                            let viewport = viewport.get();
                            let line_height = config.get().editor.line_height();
                            let y = (line.line * line_height) as f64 - viewport.y0;
                            Style::default()
                                .position(Position::Absolute)
                                .height_pt(line_height as f32)
                                .width(match extra.width {
                                    Some(width) => Dimension::Points(width as f32),
                                    None => Dimension::Percent(1.0),
                                })
                                .apply_if(extra.width.is_some(), |s| {
                                    s.margin_left((extra.x - viewport.x0) as f32)
                                })
                                .margin_top(y as f32)
                                .align_items(Some(AlignItems::Center))
                        })
                    },
                )
                .style(cx, || {
                    Style::default()
                        .position(Position::Absolute)
                        .dimension_pct(1.0, 1.0)
                })
            },
        )
        .style(cx, || Style::default().dimension_pct(1.0, 1.0))
    })
    .style(cx, move || {
        Style::default()
            .position(Position::Absolute)
            .dimension_pct(1.0, 1.0)
    })
}

fn editor(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    is_active: impl Fn() -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (cursor, scroll_delta, scroll_to, window_origin, viewport, config) = editor
        .with(|editor| {
            (
                editor.cursor.read_only(),
                editor.scroll_delta.read_only(),
                editor.scroll_to,
                editor.window_origin,
                editor.viewport,
                editor.common.config,
            )
        });

    let key_fn = move |line: &DocLine| {
        (
            editor
                .with_untracked(|editor| editor.doc)
                .with_untracked(|doc| doc.content.clone()),
            line.rev,
            line.style_rev,
            line.line,
        )
    };
    let view_fn = move |cx, line: DocLine| {
        container(cx, |cx| {
            rich_text(cx, move || line.text.clone().text.clone())
        })
        .style(cx, move || {
            let config = config.get_untracked();
            let line_height = config.editor.line_height();
            Style::default()
                .align_content(Some(AlignContent::Center))
                .height_pt(line_height as f32)
        })
    };

    stack(cx, move |cx| {
        (
            stack(cx, move |cx| {
                (
                    label(cx, || " ".to_string())
                        .style(cx, || Style::default().margin_vert(5.0)),
                    scroll(cx, move |cx| {
                        let workspace = workspace.clone();
                        list(
                            cx,
                            move || {
                                let doc = editor.with(|editor| editor.doc);
                                let full_path = doc
                                    .with_untracked(|doc| {
                                        doc.content.path().cloned()
                                    })
                                    .unwrap_or_default();
                                let mut path = full_path.clone();
                                if let Some(workspace_path) =
                                    workspace.clone().path.as_ref()
                                {
                                    path = path
                                        .strip_prefix(workspace_path)
                                        .unwrap_or(&full_path)
                                        .to_path_buf();
                                }
                                path.ancestors()
                                    .collect::<Vec<_>>()
                                    .iter()
                                    .rev()
                                    .filter_map(|path| {
                                        Some(path.file_name()?.to_str()?.to_string())
                                    })
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .enumerate()
                            },
                            |(i, section)| (*i, section.to_string()),
                            move |cx, (i, section)| {
                                stack(cx, move |cx| {
                                    (
                                        svg(cx, move || {
                                            config.get().ui_svg(
                                                LapceIcons::BREADCRUMB_SEPARATOR,
                                            )
                                        })
                                        .style(cx, move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::default()
                                                .display(if i == 0 {
                                                    Display::None
                                                } else {
                                                    Display::Flex
                                                })
                                                .dimension_pt(size, size)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        }),
                                        label(cx, move || section.clone()),
                                    )
                                })
                                .style(cx, || {
                                    Style::default()
                                        .align_items(Some(AlignItems::Center))
                                })
                            },
                        )
                        .style(cx, || Style::default().padding_horiz(10.0))
                    })
                    .on_scroll_to(cx, move || {
                        editor.with(|_editor| ());
                        Some(Point::new(3000.0, 0.0))
                    })
                    .hide_bar()
                    .style(cx, move || {
                        Style::default()
                            .position(Position::Absolute)
                            .dimension_pct(1.0, 1.0)
                            .border_bottom(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                            .align_items(Some(AlignItems::Center))
                    }),
                )
            })
            .style(cx, || Style::default().width_pct(1.0)),
            container(cx, |cx| {
                stack(cx, |cx| {
                    (
                        editor_gutter(cx, editor, is_active),
                        stack(cx, move |cx| {
                            (
                                editor_cursor(
                                    cx,
                                    editor,
                                    cursor,
                                    viewport.read_only(),
                                    is_active,
                                    config,
                                ),
                                editor_extra_style(
                                    cx,
                                    editor,
                                    viewport.read_only(),
                                    config,
                                ),
                                scroll(cx, |cx| {
                                    let focus =
                                        editor.with_untracked(|e| e.common.focus);
                                    click(
                                        cx,
                                        |cx| {
                                            let line_height = config
                                                .get_untracked()
                                                .editor
                                                .line_height();
                                            virtual_list(
                                                cx,
                                                VirtualListDirection::Vertical,
                                                move || editor.get().doc.get(),
                                                key_fn,
                                                view_fn,
                                                VirtualListItemSize::Fixed(
                                                    line_height as f64,
                                                ),
                                            )
                                            .style(cx, move || {
                                                let config = config.get();
                                                let padding_bottom = if config
                                                    .editor
                                                    .scroll_beyond_last_line
                                                {
                                                    viewport.get().height() as f32
                                                        - config.editor.line_height()
                                                            as f32
                                                } else {
                                                    0.0
                                                };
                                                Style::default()
                                                    .flex_col()
                                                    .padding_bottom(padding_bottom)
                                            })
                                        },
                                        move || {
                                            focus.set(Focus::Workbench);
                                        },
                                    )
                                })
                                .scroll_bar_color(cx, move || {
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_SCROLL_BAR)
                                })
                                .on_resize(move |point, rect| {
                                    window_origin.set(point);
                                })
                                .onscroll(move |rect| {
                                    viewport.set(rect);
                                })
                                .on_scroll_to(cx, move || {
                                    scroll_to.get().map(|s| s.to_point())
                                })
                                .on_scroll_delta(cx, move || scroll_delta.get())
                                .on_ensure_visible(cx, move || {
                                    let cursor = cursor.get();
                                    let offset = cursor.offset();
                                    let editor = editor.get_untracked();
                                    let doc = editor.doc;
                                    let caret = doc.with_untracked(|doc| {
                                        cursor_caret(
                                            doc,
                                            offset,
                                            !cursor.is_insert(),
                                        )
                                    });
                                    let config = config.get_untracked();
                                    let line_height = config.editor.line_height();
                                    if let CursorRender::Caret { x, width, line } =
                                        caret
                                    {
                                        let rect =
                                            Size::new(width, line_height as f64)
                                                .to_rect()
                                                .with_origin(Point::new(
                                                    x,
                                                    (line * line_height) as f64,
                                                ));

                                        let viewport = viewport.get_untracked();
                                        let smallest_distance = (viewport.y0
                                            - rect.y0)
                                            .abs()
                                            .min((viewport.y1 - rect.y0).abs())
                                            .min((viewport.y0 - rect.y1).abs())
                                            .min((viewport.y1 - rect.y1).abs());
                                        let biggest_distance = (viewport.y0
                                            - rect.y0)
                                            .abs()
                                            .max((viewport.y1 - rect.y0).abs())
                                            .max((viewport.y0 - rect.y1).abs())
                                            .max((viewport.y1 - rect.y1).abs());
                                        let jump_to_middle = biggest_distance
                                            > viewport.height()
                                            && smallest_distance
                                                > viewport.height() / 2.0;

                                        if jump_to_middle {
                                            rect.inflate(
                                                0.0,
                                                viewport.height() / 2.0,
                                            )
                                        } else {
                                            rect.inflate(
                                                0.0,
                                                (config
                                                    .editor
                                                    .cursor_surrounding_lines
                                                    * line_height)
                                                    as f64,
                                            )
                                        }
                                    } else {
                                        Rect::ZERO
                                    }
                                })
                                .style(cx, || {
                                    Style::default()
                                        .position(Position::Absolute)
                                        .dimension_pct(1.0, 1.0)
                                }),
                            )
                        })
                        .style(cx, || {
                            Style::default().flex_grow(1.0).height_pct(1.0)
                        }),
                    )
                })
                .style(cx, || {
                    Style::default()
                        .position(Position::Absolute)
                        .dimension_pct(1.0, 1.0)
                })
            })
            .style(cx, || Style::default().flex_grow(1.0).width_pct(1.0)),
        )
    })
    .style(cx, || Style::default().flex_col().dimension_pct(1.0, 1.0))
}

fn editor_tab_header(
    cx: AppContext,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let items = move || {
        let editor_tab = editor_tab.get();
        for (i, (index, _)) in editor_tab.children.iter().enumerate() {
            if index.get_untracked() != i {
                index.set(i);
            }
        }
        editor_tab.children
    };
    let key = |(_, child): &(RwSignal<usize>, EditorTabChild)| child.id();
    let active = move || editor_tab.with(|editor_tab| editor_tab.active);
    let is_focused = move || {
        if let Focus::Workbench = focus.get() {
            editor_tab.with_untracked(|e| Some(e.editor_tab_id))
                == active_editor_tab.get()
        } else {
            false
        }
    };

    let view_fn = move |cx, (i, child): (RwSignal<usize>, EditorTabChild)| {
        let local_child = child.clone();
        let child_view = move |cx: AppContext| match child {
            EditorTabChild::Editor(editor_id) => {
                #[derive(PartialEq)]
                struct Info {
                    icon: String,
                    color: Option<Color>,
                    path: String,
                    confirmed: RwSignal<bool>,
                    is_pristine: bool,
                }

                let info = create_memo(cx.scope, move |_| {
                    let config = config.get();
                    let editor_data =
                        editors.with(|editors| editors.get(&editor_id).cloned());
                    let path = if let Some(editor_data) = editor_data {
                        let ((content, is_pristine), confirmed) =
                            editor_data.with(|editor_data| {
                                (
                                    editor_data.doc.with(|doc| {
                                        (
                                            doc.content.clone(),
                                            doc.buffer().is_pristine(),
                                        )
                                    }),
                                    editor_data.confirmed,
                                )
                            });
                        match content {
                            DocContent::File(path) => {
                                Some((path, confirmed, is_pristine))
                            }
                            DocContent::Local => None,
                        }
                    } else {
                        None
                    };
                    let (icon, color, path, confirmed, is_pristine) = match path {
                        Some((path, confirmed, is_pritine)) => {
                            let (svg, color) = config.file_svg(&path);
                            (
                                svg,
                                color.cloned(),
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_str()
                                    .unwrap_or_default()
                                    .to_string(),
                                confirmed,
                                is_pritine,
                            )
                        }
                        None => (
                            config.ui_svg(LapceIcons::FILE),
                            Some(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE)),
                            "local".to_string(),
                            create_rw_signal(cx.scope, true),
                            true,
                        ),
                    };
                    Info {
                        icon,
                        color,
                        path,
                        confirmed,
                        is_pristine,
                    }
                });

                stack(cx, |cx| {
                    (
                        container(cx, |cx| {
                            svg(cx, move || info.with(|info| info.icon.clone()))
                                .style(cx, move || {
                                    let size = config.get().ui.icon_size() as f32;
                                    Style::default()
                                        .dimension_pt(size, size)
                                        .apply_opt(
                                            info.with(|info| info.color),
                                            |s, c| s.color(c),
                                        )
                                })
                        })
                        .style(cx, || Style::default().padding_horiz(5.0)),
                        label(cx, move || info.with(|info| info.path.clone()))
                            .style(cx, move || {
                                Style::default().apply_if(
                                    !info.with(|info| info.confirmed).get(),
                                    |s| s.font_style(FontStyle::Italic),
                                )
                            }),
                        container(cx, |cx| {
                            svg(cx, move || {
                                config.get().ui_svg(
                                    if info.with(|info| info.is_pristine) {
                                        LapceIcons::CLOSE
                                    } else {
                                        LapceIcons::UNSAVED
                                    },
                                )
                            })
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::default().dimension_pt(size, size).color(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                )
                            })
                        })
                        .style(cx, || Style::default().padding_horiz(5.0)),
                    )
                })
                .style(cx, move || {
                    Style::default()
                        .align_items(Some(AlignItems::Center))
                        .border_left(if i.get() == 0 { 1.0 } else { 0.0 })
                        .border_right(1.0)
                        .border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                })
            }
        };

        let confirmed = match local_child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                editor_data.map(|editor_data| editor_data.get().confirmed)
            }
        };

        stack(cx, |cx| {
            (
                click(
                    cx,
                    |cx| {
                        double_click(cx, child_view, move || {
                            if let Some(confirmed) = confirmed {
                                confirmed.set(true);
                            }
                        })
                        .style(cx, move || {
                            Style::default()
                                .align_items(Some(AlignItems::Center))
                                .height_pct(1.0)
                        })
                    },
                    move || {
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = i.get_untracked();
                        });
                    },
                )
                .style(cx, move || {
                    Style::default()
                        .align_items(Some(AlignItems::Center))
                        .height_pct(1.0)
                }),
                container(cx, |cx| {
                    label(cx, || "".to_string()).style(cx, move || {
                        Style::default()
                            .dimension_pct(1.0, 1.0)
                            .border_bottom(if active() == i.get() {
                                2.0
                            } else {
                                0.0
                            })
                            .border_color(*config.get().get_color(if is_focused() {
                                LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                            } else {
                                LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                            }))
                    })
                })
                .style(cx, || {
                    Style::default()
                        .position(Position::Absolute)
                        .padding_horiz(3.0)
                        .dimension_pct(1.0, 1.0)
                }),
            )
        })
        .style(cx, || Style::default().height_pct(1.0))
    };

    stack(cx, |cx| {
        (
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::TAB_PREVIOUS)).style(
                    cx,
                    move || {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        Style::default()
                            .dimension_pt(size, size)
                            .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                    },
                )
            })
            .style(cx, || Style::default().padding_horiz(5.0)),
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::TAB_NEXT)).style(
                    cx,
                    move || {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        Style::default()
                            .dimension_pt(size, size)
                            .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                    },
                )
            })
            .style(cx, || Style::default().padding_horiz(5.0)),
            container(cx, |cx| {
                scroll(cx, |cx| {
                    list(cx, items, key, view_fn).style(cx, || {
                        Style::default().height_pct(1.0).items_center()
                    })
                })
                .hide_bar()
                .style(cx, || {
                    Style::default()
                        .position(Position::Absolute)
                        .height_pct(1.0)
                        .max_width_pct(1.0)
                })
            })
            .style(cx, || Style::default().height_pct(1.0).flex_grow(1.0)),
            container(cx, |cx| {
                svg(cx, move || {
                    config.get().ui_svg(LapceIcons::SPLIT_HORIZONTAL)
                })
                .style(cx, move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::default()
                        .dimension_pt(size, size)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                })
            })
            .style(cx, || Style::default().padding_horiz(5.0)),
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::CLOSE)).style(
                    cx,
                    move || {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        Style::default()
                            .dimension_pt(size, size)
                            .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                    },
                )
            })
            .style(cx, || Style::default().padding_horiz(5.0)),
        )
    })
    .style(cx, move || {
        let config = config.get();
        Style::default()
            .height_pt(config.ui.header_height() as f32)
            .items_center()
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
    })
}

fn editor_tab_content(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
) -> impl View {
    let items = move || {
        editor_tab
            .get()
            .children
            .into_iter()
            .map(|(_, child)| child)
    };
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |cx, child| {
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                if let Some(editor_data) = editor_data {
                    let is_active = move || {
                        let focus = focus.get();
                        if let Focus::Workbench = focus {
                            let active_editor_tab = active_editor_tab.get();
                            let editor_tab =
                                editor_data.with(|editor| editor.editor_tab_id);
                            editor_tab.is_some() && editor_tab == active_editor_tab
                        } else {
                            false
                        }
                    };
                    container_box(cx, |cx| {
                        Box::new(editor(
                            cx,
                            workspace.clone(),
                            is_active,
                            editor_data,
                        ))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style::default().flex_grow(1.0).flex_col())
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(cx, active, items, key, view_fn)
        .style(cx, || Style::default().flex_grow(1.0).flex_col())
}

fn editor_tab(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    focus: ReadSignal<Focus>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack(cx, |cx| {
        (
            editor_tab_header(
                cx,
                active_editor_tab,
                editor_tab,
                editors,
                focus,
                config,
            ),
            editor_tab_content(
                cx,
                workspace.clone(),
                active_editor_tab,
                editor_tab,
                editors,
                focus,
            ),
        )
    })
    .style(cx, || Style::default().flex_col().flex_grow(1.0))
}

fn split_border(
    cx: AppContext,
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let direction = move || split.with(|split| split.direction);
    list(
        cx,
        move || split.get().children.into_iter().skip(1),
        |content| content.id(),
        move |cx, content| {
            container(cx, |cx| {
                label(cx, || "".to_string()).style(cx, move || {
                    let direction = direction();
                    Style::default()
                        .width(match direction {
                            SplitDirection::Vertical => Dimension::Points(1.0),
                            SplitDirection::Horizontal => Dimension::Percent(1.0),
                        })
                        .height(match direction {
                            SplitDirection::Vertical => Dimension::Percent(1.0),
                            SplitDirection::Horizontal => Dimension::Points(1.0),
                        })
                        .background(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                })
            })
            .style(cx, move || {
                let rect = match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        let editor_tab_data = editor_tabs
                            .with(|tabs| tabs.get(editor_tab_id).cloned());
                        if let Some(editor_tab_data) = editor_tab_data {
                            editor_tab_data.with(|editor_tab| editor_tab.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                    SplitContent::Split(split_id) => {
                        if let Some(split) =
                            splits.with(|splits| splits.get(split_id).cloned())
                        {
                            split.with(|split| split.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                };
                let direction = direction();
                Style::default()
                    .position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left(rect.x0 as f32 - 1.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 1.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => Dimension::Points(5.0),
                        SplitDirection::Horizontal => Dimension::Percent(1.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => Dimension::Percent(1.0),
                        SplitDirection::Horizontal => Dimension::Points(5.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .justify_content(Some(JustifyContent::Center))
            })
        },
    )
    .style(cx, || {
        Style::default()
            .position(Position::Absolute)
            .dimension_pct(1.0, 1.0)
    })
}

fn split_list(
    cx: AppContext,
    workspace: Arc<LapceWorkspace>,
    split: ReadSignal<SplitData>,
    main_split: MainSplitData,
) -> impl View {
    let editor_tabs = main_split.editor_tabs.read_only();
    let active_editor_tab = main_split.active_editor_tab.read_only();
    let editors = main_split.editors.read_only();
    let splits = main_split.splits.read_only();
    let config = main_split.common.config;
    let focus = main_split.common.focus.read_only();

    let direction = move || split.with(|split| split.direction);
    let items = move || split.get().children.into_iter().enumerate();
    let key = |(index, content): &(usize, SplitContent)| content.id();
    let view_fn = move |cx, (index, content), main_split: MainSplitData| {
        let child = match &content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    editor_tabs.with(|tabs| tabs.get(editor_tab_id).cloned());
                if let Some(editor_tab_data) = editor_tab_data {
                    container_box(cx, |cx| {
                        Box::new(editor_tab(
                            cx,
                            workspace.clone(),
                            active_editor_tab,
                            editor_tab_data,
                            editors,
                            focus,
                            config,
                        ))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor tab".to_string()))
                    })
                }
            }
            SplitContent::Split(split_id) => {
                if let Some(split) =
                    splits.with(|splits| splits.get(split_id).cloned())
                {
                    split_list(
                        cx,
                        workspace.clone(),
                        split.read_only(),
                        main_split.clone(),
                    )
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy split".to_string()))
                    })
                }
            }
        };
        child
            .on_resize(move |window_origin, rect| match &content {
                SplitContent::EditorTab(editor_tab_id) => {
                    main_split.editor_tab_update_layout(
                        editor_tab_id,
                        window_origin,
                        rect,
                    );
                }
                SplitContent::Split(split_id) => {
                    let split_data =
                        splits.with(|splits| splits.get(split_id).cloned());
                    if let Some(split_data) = split_data {
                        split_data.update(|split| {
                            split.window_origin = window_origin;
                            split.layout_rect = rect;
                        });
                    }
                }
            })
            .style(cx, move || {
                Style::default()
                    .flex_grow(1.0)
                    .flex_basis(Dimension::Points(1.0))
            })
    };
    container_box(cx, move |cx| {
        Box::new(
            stack(cx, move |cx| {
                (
                    list(cx, items, key, move |cx, (index, content)| {
                        view_fn(cx, (index, content), main_split.clone())
                    })
                    .style(cx, move || {
                        Style::default()
                            .flex_direction(match direction() {
                                SplitDirection::Vertical => FlexDirection::Row,
                                SplitDirection::Horizontal => FlexDirection::Column,
                            })
                            .flex_grow(1.0)
                            .flex_basis(Dimension::Points(1.0))
                    }),
                    split_border(cx, splits, editor_tabs, split, config),
                )
            })
            .style(cx, || {
                Style::default()
                    .flex_grow(1.0)
                    .flex_basis(Dimension::Points(1.0))
            }),
        )
    })
}

fn main_split(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get()
        .get(&root_split)
        .unwrap()
        .read_only();
    let splits = window_tab_data.main_split.splits.read_only();
    let active_editor_tab = window_tab_data.main_split.active_editor_tab.read_only();
    let editor_tabs = window_tab_data.main_split.editor_tabs.read_only();
    let editors = window_tab_data.main_split.editors.read_only();
    let config = window_tab_data.main_split.common.config;
    let workspace = window_tab_data.workspace.clone();
    let panel = window_tab_data.panel.clone();
    split_list(
        cx,
        workspace,
        root_split,
        window_tab_data.main_split.clone(),
    )
    .style(cx, move || {
        let is_hidden = panel.panel_bottom_maximized(true)
            && panel.is_container_shown(&PanelContainerPosition::Bottom, true);
        Style::default()
            .background(*config.get().get_color(LapceColor::EDITOR_BACKGROUND))
            .apply_if(is_hidden, |s| s.display(Display::None))
            .flex_grow(1.0)
    })
}

fn terminal_tab_header(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let active = move || terminal.tab_info.with(|info| info.active);

    list(
        cx,
        move || {
            let tabs = terminal.tab_info.with(|info| info.tabs.clone());
            for (i, (index, _)) in tabs.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            tabs
        },
        |(_, tab)| tab.terminal_tab_id,
        move |cx, (index, tab)| {
            let title = move || {
                let terminal = tab.active_terminal(true);
                let title = terminal.map(|t| t.title);
                let title = title.map(|t| t.get());
                title.unwrap_or_default()
            };
            stack(cx, |cx| {
                (
                    container(cx, |cx| {
                        stack(cx, |cx| {
                            (
                                container(cx, |cx| {
                                    svg(cx, move || {
                                        config.get().ui_svg(LapceIcons::TERMINAL)
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::default()
                                                .dimension_pt(size, size)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        },
                                    )
                                })
                                .style(cx, || Style::default().padding_horiz(5.0)),
                                label(cx, title).style(cx, || {
                                    Style::default().min_width_pt(0.0).flex_grow(1.0)
                                }),
                                container(cx, |cx| {
                                    svg(cx, move || {
                                        config.get().ui_svg(LapceIcons::CLOSE)
                                    })
                                    .style(
                                        cx,
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;
                                            Style::default()
                                                .dimension_pt(size, size)
                                                .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                        },
                                    )
                                })
                                .style(cx, || Style::default().padding_horiz(5.0)),
                            )
                        })
                        .style(cx, move || {
                            Style::default()
                                .items_center()
                                .width_pt(200.0)
                                .border_right(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                    })
                    .style(cx, || Style::default().items_center()),
                    container(cx, |cx| {
                        label(cx, || "".to_string()).style(cx, move || {
                            Style::default()
                                .dimension_pct(1.0, 1.0)
                                .border_bottom(if active() == index.get() {
                                    2.0
                                } else {
                                    0.0
                                })
                                .border_color(*config.get().get_color(
                                    if focus.get()
                                        == Focus::Panel(PanelKind::Terminal)
                                    {
                                        LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE
                                    } else {
                                        LapceColor::LAPCE_TAB_INACTIVE_UNDERLINE
                                    },
                                ))
                        })
                    })
                    .style(cx, || {
                        Style::default()
                            .position(Position::Absolute)
                            .padding_horiz(3.0)
                            .dimension_pct(1.0, 1.0)
                    }),
                )
            })
        },
    )
    .style(cx, move || {
        let config = config.get();
        Style::default()
            .height_pt(config.ui.header_height() as f32)
            .width_pct(1.0)
            .border_bottom(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
    })
}

fn terminal_tab_split(
    cx: AppContext,
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
) -> impl View {
    let config = terminal_panel_data.common.config;
    list(
        cx,
        move || {
            let terminals = terminal_tab_data.terminals.get();
            for (i, (index, _)) in terminals.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            terminals
        },
        |(_, terminal)| terminal.term_id,
        move |cx, (index, terminal)| {
            let focus = terminal.common.focus;
            let terminal_panel_data = terminal_panel_data.clone();
            click(
                cx,
                move |cx| {
                    terminal_view(
                        cx,
                        terminal.term_id,
                        terminal.raw.clone(),
                        terminal.mode.read_only(),
                        terminal_panel_data,
                    )
                    .on_event(EventListner::MouseWheel, move |event| {
                        if let Event::MouseWheel(mouse_event) = event {
                            terminal.clone().wheel_scroll(mouse_event.wheel_delta.y);
                            true
                        } else {
                            false
                        }
                    })
                    .style(cx, || Style::default().dimension_pct(1.0, 1.0))
                },
                move || {
                    focus.set(Focus::Panel(PanelKind::Terminal));
                },
            )
            .style(cx, move || {
                Style::default()
                    .dimension_pct(1.0, 1.0)
                    .padding_horiz(5.0)
                    .apply_if(index.get() > 0, |s| {
                        s.border_left(1.0).border_color(
                            *config.get().get_color(LapceColor::LAPCE_BORDER),
                        )
                    })
            })
        },
    )
    .style(cx, || {
        Style::default()
            .flex_grow(1.0)
            .flex_basis(Dimension::Points(1.0))
    })
}

fn terminal_tab_content(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        cx,
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |cx, (_, tab)| terminal_tab_split(cx, terminal.clone(), tab),
    )
    .style(cx, || Style::default().width_pct(1.0).flex_grow(1.0))
}

fn terminal_panel(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    stack(cx, |cx| {
        (
            terminal_tab_header(cx, window_tab_data.clone()),
            terminal_tab_content(cx, window_tab_data),
        )
    })
    .style(cx, || Style::default().dimension_pct(1.0, 1.0).flex_col())
}

fn panel_view(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        cx,
        active_fn,
        panels,
        |p| *p,
        move |cx, kind| {
            let view = match kind {
                PanelKind::Terminal => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::FileExplorer => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::SourceControl => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::Plugin => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::Search => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::Problem => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
                PanelKind::Debug => container_box(cx, |cx| {
                    Box::new(terminal_panel(cx, window_tab_data.clone()))
                }),
            };
            view.style(cx, || Style::default().dimension_pct(1.0, 1.0))
        },
    )
    .style(cx, move || {
        Style::default()
            .dimension_pct(1.0, 1.0)
            .apply_if(!panel.is_position_shown(&position, true), |s| {
                s.display(Display::None)
            })
    })
}

fn bottom_panel(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    stack(cx, |cx| {
        (
            panel_view(cx, window_tab_data.clone(), PanelPosition::BottomLeft),
            panel_view(cx, window_tab_data, PanelPosition::BottomRight),
        )
    })
    .style(cx, move || {
        let height = panel.size.with(|s| s.bottom);
        let is_maximized = panel.panel_bottom_maximized(true);
        Style::default()
            .apply_if(
                !panel.is_container_shown(&PanelContainerPosition::Bottom, true),
                |s| s.display(Display::None),
            )
            .apply_if(!is_maximized, |s| s.height_pt(height as f32))
            .apply_if(is_maximized, |s| s.flex_grow(1.0))
            .width_pct(1.0)
            .border_top(1.0)
            .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
    })
}

fn workbench(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.main_split.common.config;
    stack(cx, move |cx| {
        (
            label(cx, move || "left".to_string()).style(cx, move || {
                let config = config.get();
                Style::default()
                    .width_pt(250.0)
                    .padding(20.0)
                    .border_right(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            }),
            stack(cx, move |cx| {
                (
                    main_split(cx, window_tab_data.clone()),
                    bottom_panel(cx, window_tab_data),
                )
            })
            .style(cx, || {
                Style::default().flex_col().flex_grow(1.0).min_width_pt(0.0)
            }),
            label(cx, move || "right".to_string()).style(cx, move || {
                let config = config.get();
                Style::default()
                    .padding(20.0)
                    .border_left(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .min_width_pt(0.0)
                    .display(Display::None)
                    .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
            }),
        )
    })
    .style(cx, || Style::default().width_pct(1.0).flex_grow(1.0))
}

fn status(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let diagnostic_count = create_memo(cx.scope, move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.iter() {
                if let Some(severity) = diagnostic.diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });

    let mode = create_memo(cx.scope, move |_| window_tab_data.mode());

    stack(cx, |cx| {
        (
            label(cx, move || match mode.get() {
                Mode::Normal => "Normal".to_string(),
                Mode::Insert => "Insert".to_string(),
                Mode::Visual => "Visual".to_string(),
                Mode::Terminal => "Terminal".to_string(),
            })
            .style(cx, move || {
                let config = config.get();
                let display = if config.core.modal {
                    Display::Flex
                } else {
                    Display::None
                };

                let (bg, fg) = match mode.get() {
                    Mode::Normal => (
                        LapceColor::STATUS_MODAL_NORMAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_NORMAL_FOREGROUND,
                    ),
                    Mode::Insert => (
                        LapceColor::STATUS_MODAL_INSERT_BACKGROUND,
                        LapceColor::STATUS_MODAL_INSERT_FOREGROUND,
                    ),
                    Mode::Visual => (
                        LapceColor::STATUS_MODAL_VISUAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_VISUAL_FOREGROUND,
                    ),
                    Mode::Terminal => (
                        LapceColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                    ),
                };

                let bg = *config.get_color(bg);
                let fg = *config.get_color(fg);

                Style::default()
                    .display(display)
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(1.0)
                    .align_items(Some(AlignItems::Center))
            }),
            svg(cx, move || config.get().ui_svg(LapceIcons::ERROR)).style(
                cx,
                move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::default()
                        .dimension_pt(size, size)
                        .margin_left(10.0)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                },
            ),
            label(cx, move || diagnostic_count.get().0.to_string())
                .style(cx, || Style::default().margin_left(5.0)),
            svg(cx, move || config.get().ui_svg(LapceIcons::WARNING)).style(
                cx,
                move || {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    Style::default()
                        .dimension_pt(size, size)
                        .margin_left(5.0)
                        .color(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE))
                },
            ),
            label(cx, move || diagnostic_count.get().1.to_string())
                .style(cx, || Style::default().margin_left(5.0)),
        )
    })
    .style(cx, move || {
        let config = config.get();
        Style::default()
            .border_top(1.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::STATUS_BACKGROUND))
            .height_pt(config.ui.status_height() as f32)
            .align_items(Some(AlignItems::Center))
    })
}

fn palette_item(
    cx: AppContext,
    i: usize,
    item: PaletteItem,
    index: ReadSignal<usize>,
    palette_item_height: f64,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    match &item.content {
        PaletteItemContent::File { path, .. }
        | PaletteItemContent::Reference { path, .. } => {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            // let (file_name, _) = create_signal(cx.scope, file_name);
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            // let (folder, _) = create_signal(cx.scope, folder);
            let folder_len = folder.len();

            let file_name_indices = item
                .indices
                .iter()
                .filter_map(|&i| {
                    if folder_len > 0 {
                        if i > folder_len {
                            Some(i - folder_len - 1)
                        } else {
                            None
                        }
                    } else {
                        Some(i)
                    }
                })
                .collect::<Vec<_>>();
            let folder_indices = item
                .indices
                .iter()
                .filter_map(|&i| if i < folder_len { Some(i) } else { None })
                .collect::<Vec<_>>();

            let path = path.to_path_buf();
            let style_path = path.clone();
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || config.get().file_svg(&path).0).style(
                                cx,
                                move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color =
                                        config.file_svg(&style_path).1.copied();
                                    Style::default()
                                        .min_width_pt(size)
                                        .dimension_pt(size, size)
                                        .margin_right(5.0)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            focus_text(
                                cx,
                                move || file_name.clone(),
                                move || file_name_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::default().margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || folder.clone(),
                                move || folder_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
                                Style::default()
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_pt(0.0)
                            }),
                        )
                    })
                    .style(cx, || {
                        Style::default()
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
                    }),
                )
            })
        }
        PaletteItemContent::DocumentSymbol {
            kind,
            name,
            container_name,
            ..
        } => {
            let kind = *kind;
            let text = name.to_string();
            let hint = container_name.clone().unwrap_or_default();
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container_box(cx, move |cx| {
                Box::new(
                    stack(cx, move |cx| {
                        (
                            svg(cx, move || {
                                let config = config.get();
                                config.symbol_svg(&kind).unwrap_or_else(|| {
                                    config.ui_svg(LapceIcons::FILE)
                                })
                            })
                            .style(cx, move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::default()
                                    .min_width_pt(size)
                                    .dimension_pt(size, size)
                                    .margin_right(5.0)
                                    .color(
                                        *config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ),
                                    )
                            }),
                            focus_text(
                                cx,
                                move || text.clone(),
                                move || text_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, || {
                                Style::default().margin_right(6.0).max_width_pct(1.0)
                            }),
                            focus_text(
                                cx,
                                move || hint.clone(),
                                move || hint_indices.clone(),
                                move || {
                                    *config.get().get_color(LapceColor::EDITOR_FOCUS)
                                },
                            )
                            .style(cx, move || {
                                Style::default()
                                    .color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width_pt(0.0)
                            }),
                        )
                    })
                    .style(cx, || {
                        Style::default()
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
                    }),
                )
            })
        }
        PaletteItemContent::Command { .. }
        | PaletteItemContent::Line { .. }
        | PaletteItemContent::Workspace { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container_box(cx, move |cx| {
                Box::new(
                    focus_text(
                        cx,
                        move || text.clone(),
                        move || indices.clone(),
                        move || *config.get().get_color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(cx, || {
                        Style::default()
                            .align_items(Some(AlignItems::Center))
                            .max_width_pct(1.0)
                    }),
                )
            })
        }
    }
    .style(cx, move || {
        Style::default()
            .width_pct(1.0)
            .height_pt(palette_item_height as f32)
            .padding_horiz(10.0)
            .apply_if(index.get() == i, |style| {
                style.background(
                    *config
                        .get()
                        .get_color(LapceColor::PALETTE_CURRENT_BACKGROUND),
                )
            })
    })
}

fn palette_input(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let doc = window_tab_data.palette.input_editor.doc.read_only();
    let cursor = window_tab_data.palette.input_editor.cursor.read_only();
    let config = window_tab_data.common.config;
    let cursor_x = create_memo(cx.scope, move |_| {
        let offset = cursor.get().offset();
        let config = config.get();
        doc.with(|doc| {
            let (_, col) = doc.buffer().offset_to_line_col(offset);

            let line_content = doc.buffer().line_content(0);

            let families = config.ui.font_family();
            let attrs = Attrs::new()
                .font_size(config.ui.font_size() as f32)
                .family(&families);
            let attrs_list = AttrsList::new(attrs);
            let mut text_layout = TextLayout::new();
            text_layout.set_text(&line_content[..col], attrs_list);

            text_layout.size().width as f32
        })
    });
    container(cx, move |cx| {
        container(cx, move |cx| {
            scroll(cx, move |cx| {
                stack(cx, move |cx| {
                    (
                        label(cx, move || {
                            doc.with(|doc| doc.buffer().text().to_string())
                        }),
                        label(cx, move || "".to_string()).style(cx, move || {
                            Style::default()
                                .position(Position::Absolute)
                                .width_pt(2.0)
                                .height_pct(1.0)
                                .margin_left(cursor_x.get() - 0.5)
                                .background(
                                    *config
                                        .get()
                                        .get_color(LapceColor::EDITOR_CARET),
                                )
                        }),
                    )
                })
            })
            .scroll_bar_color(cx, move || {
                *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
            })
            .on_ensure_visible(cx, move || {
                Size::new(20.0, 0.0)
                    .to_rect()
                    .with_origin(Point::new(cursor_x.get() as f64 - 10.0, 0.0))
            })
            .style(cx, || {
                Style::default()
                    .flex_grow(1.0)
                    .min_width_pt(0.0)
                    .height_pt(24.0)
                    .align_items(Some(AlignItems::Center))
            })
        })
        .style(cx, move || {
            let config = config.get();
            Style::default()
                .flex_grow(1.0)
                .min_width_pt(0.0)
                .border_bottom(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
                .padding_horiz(5.0)
        })
    })
    .style(cx, || Style::default().padding_bottom(5.0))
}

struct PaletteItems(im::Vector<PaletteItem>);

impl VirtualListVector<(usize, PaletteItem)> for PaletteItems {
    type ItemIterator = Box<dyn Iterator<Item = (usize, PaletteItem)>>;

    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator {
        let start = range.start;
        Box::new(
            self.0
                .slice(range)
                .into_iter()
                .enumerate()
                .map(move |(i, item)| (i + start, item)),
        )
    }
}

fn palette_content(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    layout_rect: ReadSignal<Rect>,
) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let index = window_tab_data.palette.index.read_only();
    let clicked_index = window_tab_data.palette.clicked_index.write_only();
    let config = window_tab_data.common.config;
    let run_id = window_tab_data.palette.run_id;
    let input = window_tab_data.palette.input.read_only();
    let palette_item_height = 25.0;
    stack(cx, |cx| {
        (
            scroll(cx, |cx| {
                virtual_list(
                    cx,
                    VirtualListDirection::Vertical,
                    move || PaletteItems(items.get()),
                    move |(i, _item)| {
                        (run_id.get_untracked(), *i, input.get_untracked().input)
                    },
                    move |cx, (i, item)| {
                        click(
                            cx,
                            move |cx| {
                                palette_item(
                                    cx,
                                    i,
                                    item,
                                    index,
                                    palette_item_height,
                                    config,
                                )
                            },
                            move || {
                                clicked_index.set(Some(i));
                            },
                        )
                        .style(cx, || Style::default().width_pct(1.0))
                    },
                    VirtualListItemSize::Fixed(palette_item_height),
                )
                .style(cx, || Style::default().width_pct(1.0).flex_col())
            })
            .scroll_bar_color(cx, move || {
                *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
            })
            .on_ensure_visible(cx, move || {
                Size::new(1.0, palette_item_height).to_rect().with_origin(
                    Point::new(0.0, index.get() as f64 * palette_item_height),
                )
            })
            .style(cx, || Style::default().width_pct(1.0).min_height_pt(0.0)),
            label(cx, || "No matching results".to_string()).style(cx, move || {
                Style::default()
                    .display(if items.with(|items| items.is_empty()) {
                        Display::Flex
                    } else {
                        Display::None
                    })
                    .padding_horiz(10.0)
                    .align_items(Some(AlignItems::Center))
                    .height_pt(palette_item_height as f32)
            }),
        )
    })
    .style(cx, move || {
        Style::default()
            .flex_col()
            .width_pct(1.0)
            .min_height_pt(0.0)
            .max_height_pt((layout_rect.get().height() * 0.45 - 36.0).round() as f32)
            .padding_bottom(5.0)
            .padding_bottom(5.0)
    })
}

fn palette_preview(cx: AppContext, palette_data: PaletteData) -> impl View {
    let workspace = palette_data.workspace.clone();
    let preview_editor = palette_data.preview_editor;
    let has_preview = palette_data.has_preview;
    let config = palette_data.common.config;
    container(cx, |cx| {
        container(cx, |cx| editor(cx, workspace, || true, preview_editor)).style(
            cx,
            move || {
                let config = config.get();
                Style::default()
                    .position(Position::Absolute)
                    .border_top(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .dimension_pct(1.0, 1.0)
                    .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
            },
        )
    })
    .style(cx, move || {
        Style::default()
            .display(if has_preview.get() {
                Display::Flex
            } else {
                Display::None
            })
            .flex_grow(1.0)
    })
}

fn palette(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.common.config;
    let has_preview = palette_data.has_preview.read_only();
    container(cx, |cx| {
        stack(cx, |cx| {
            (
                palette_input(cx, window_tab_data.clone()),
                palette_content(cx, window_tab_data.clone(), layout_rect),
                palette_preview(cx, palette_data),
            )
        })
        .style(cx, move || {
            let config = config.get();
            Style::default()
                .width_pt(500.0)
                .max_width_pct(0.9)
                .max_height(if has_preview.get() {
                    Dimension::Auto
                } else {
                    Dimension::Percent(1.0)
                })
                .height(if has_preview.get() {
                    Dimension::Points(layout_rect.get().height() as f32 - 10.0)
                } else {
                    Dimension::Auto
                })
                .margin_top(5.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .flex_col()
                .background(*config.get_color(LapceColor::PALETTE_BACKGROUND))
        })
    })
    .style(cx, move || {
        Style::default()
            .display(if status.get() == PaletteStatus::Inactive {
                Display::None
            } else {
                Display::Flex
            })
            .position(Position::Absolute)
            .dimension_pct(1.0, 1.0)
            .flex_col()
            .align_content(Some(AlignContent::Center))
    })
}

struct VectorItems<V>(im::Vector<V>);

impl<V: Clone + 'static> VirtualListVector<(usize, V)> for VectorItems<V> {
    type ItemIterator = Box<dyn Iterator<Item = (usize, V)>>;

    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: Range<usize>) -> Self::ItemIterator {
        let start = range.start;
        Box::new(
            self.0
                .slice(range)
                .into_iter()
                .enumerate()
                .map(move |(i, item)| (i + start, item)),
        )
    }
}

fn completion_kind_to_str(kind: CompletionItemKind) -> &'static str {
    match kind {
        CompletionItemKind::METHOD => "f",
        CompletionItemKind::FUNCTION => "f",
        CompletionItemKind::CLASS => "c",
        CompletionItemKind::STRUCT => "s",
        CompletionItemKind::VARIABLE => "v",
        CompletionItemKind::INTERFACE => "i",
        CompletionItemKind::ENUM => "e",
        CompletionItemKind::ENUM_MEMBER => "e",
        CompletionItemKind::FIELD => "v",
        CompletionItemKind::PROPERTY => "p",
        CompletionItemKind::CONSTANT => "d",
        CompletionItemKind::MODULE => "m",
        CompletionItemKind::KEYWORD => "k",
        CompletionItemKind::SNIPPET => "n",
        _ => "t",
    }
}

fn completion(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let completion_data = window_tab_data.common.completion;
    let config = window_tab_data.common.config;
    let active = completion_data.with_untracked(|c| c.active);
    let request_id =
        move || completion_data.with_untracked(|c| (c.request_id, c.input_id));
    scroll(cx, move |cx| {
        virtual_list(
            cx,
            VirtualListDirection::Vertical,
            move || completion_data.with(|c| VectorItems(c.filtered_items.clone())),
            move |(i, item)| (request_id(), *i),
            move |cx, (i, item)| {
                stack(cx, |cx| {
                    (
                        container(cx, move |cx| {
                            label(cx, move || {
                                item.item
                                    .kind
                                    .map(completion_kind_to_str)
                                    .unwrap_or("")
                                    .to_string()
                            })
                            .style(cx, move || {
                                Style::default()
                                    .width_pct(1.0)
                                    .justify_content(Some(JustifyContent::Center))
                            })
                        })
                        .style(cx, move || {
                            let config = config.get();
                            Style::default()
                                .width_pt(config.editor.line_height() as f32)
                                .height_pct(1.0)
                                .align_items(Some(AlignItems::Center))
                                .font_weight(Weight::BOLD)
                                .apply_opt(
                                    config.completion_color(item.item.kind),
                                    |s, c| {
                                        s.color(c)
                                            .background(c.with_alpha_factor(0.3))
                                    },
                                )
                        }),
                        focus_text(
                            cx,
                            move || item.item.label.clone(),
                            move || item.indices.clone(),
                            move || {
                                *config.get().get_color(LapceColor::EDITOR_FOCUS)
                            },
                        )
                        .style(cx, move || {
                            let config = config.get();
                            Style::default()
                                .padding_horiz(5.0)
                                .align_items(Some(AlignItems::Center))
                                .dimension_pct(1.0, 1.0)
                                .apply_if(active.get() == i, |s| {
                                    s.background(
                                        *config.get_color(
                                            LapceColor::COMPLETION_CURRENT,
                                        ),
                                    )
                                })
                        }),
                    )
                })
                .style(cx, move || {
                    Style::default()
                        .align_items(Some(AlignItems::Center))
                        .width_pct(1.0)
                        .height_pt(config.get().editor.line_height() as f32)
                })
            },
            VirtualListItemSize::Fixed(config.get().editor.line_height() as f64),
        )
        .style(cx, || {
            Style::default()
                .align_items(Some(AlignItems::Center))
                .width_pct(1.0)
                .flex_col()
        })
    })
    .scroll_bar_color(cx, move || {
        *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
    })
    .on_ensure_visible(cx, move || {
        let config = config.get();
        let active = active.get();
        Size::new(1.0, config.editor.line_height() as f64)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                active as f64 * config.editor.line_height() as f64,
            ))
    })
    .on_resize(move |_, rect| {
        completion_data.update(|c| {
            c.layout_rect = rect;
        });
    })
    .style(cx, move || {
        let config = config.get();
        let origin = window_tab_data.completion_origin();
        Style::default()
            .position(Position::Absolute)
            .width_pt(400.0)
            .max_height_pt(400.0)
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(*config.get_color(LapceColor::COMPLETION_BACKGROUND))
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size as f32)
            .border_radius(6.0)
    })
}

fn code_action(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let code_action = window_tab_data.code_action;
    let (status, active) = code_action
        .with_untracked(|code_action| (code_action.status, code_action.active));
    let request_id =
        move || code_action.with_untracked(|code_action| code_action.request_id);
    scroll(cx, move |cx| {
        list(
            cx,
            move || {
                code_action.with(|code_action| {
                    code_action.filtered_items.clone().into_iter().enumerate()
                })
            },
            move |(i, _item)| (request_id(), *i),
            move |cx, (i, item)| {
                container(cx, move |cx| {
                    label(cx, move || item.title().to_string()).style(
                        cx,
                        move || Style {
                            ..Default::default()
                        },
                    )
                })
                .style(cx, move || {
                    let config = config.get();
                    Style::default()
                        .padding_horiz(10.0)
                        .align_items(Some(AlignItems::Center))
                        .min_width_pt(0.0)
                        .width_pct(1.0)
                        .height_pt(config.editor.line_height() as f32)
                        .apply_if(active.get() == i, |s| {
                            s.background(
                                *config.get_color(LapceColor::COMPLETION_CURRENT),
                            )
                        })
                })
            },
        )
        .style(cx, || Style::default().width_pct(1.0).flex_col())
    })
    .scroll_bar_color(cx, move || {
        *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
    })
    .on_ensure_visible(cx, move || {
        let config = config.get();
        let active = active.get();
        Size::new(1.0, config.editor.line_height() as f64)
            .to_rect()
            .with_origin(Point::new(
                0.0,
                active as f64 * config.editor.line_height() as f64,
            ))
    })
    .on_resize(move |_, rect| {
        code_action.update(|c| {
            c.layout_rect = rect;
        });
    })
    .style(cx, move || {
        let origin = window_tab_data.code_action_origin();
        Style::default()
            .display(match status.get() {
                CodeActionStatus::Inactive => Display::None,
                CodeActionStatus::Active => Display::Flex,
            })
            .position(Position::Absolute)
            .width_pt(400.0)
            .max_height_pt(400.0)
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .background(*config.get().get_color(LapceColor::COMPLETION_BACKGROUND))
            .border_radius(20.0)
    })
}

fn window_tab(cx: AppContext, window_tab_data: Arc<WindowTabData>) -> impl View {
    let proxy_data = window_tab_data.proxy.clone();
    let source_control = window_tab_data.source_control;
    let window_origin = window_tab_data.window_origin;
    let layout_rect = window_tab_data.layout_rect;
    let config = window_tab_data.common.config;
    let workspace = window_tab_data.workspace.clone();
    let set_workbench_command =
        window_tab_data.common.workbench_command.write_only();

    stack(cx, |cx| {
        (
            stack(cx, |cx| {
                (
                    title(
                        cx,
                        workspace,
                        source_control,
                        set_workbench_command,
                        config,
                    ),
                    workbench(cx, window_tab_data.clone()),
                    status(cx, window_tab_data.clone()),
                )
            })
            .on_resize(move |point, rect| {
                window_origin.set(point);
                layout_rect.set(rect);
            })
            .style(cx, || Style::default().dimension_pct(1.0, 1.0).flex_col()),
            completion(cx, window_tab_data.clone()),
            code_action(cx, window_tab_data.clone()),
            palette(cx, window_tab_data.clone()),
        )
    })
    .style(cx, move || {
        Style::default()
            .dimension_pct(1.0, 1.0)
            .color(*config.get().get_color(LapceColor::EDITOR_FOREGROUND))
            .background(*config.get().get_color(LapceColor::EDITOR_BACKGROUND))
            .font_size(config.get().ui.font_size() as f32)
    })
}

fn window(cx: AppContext, window_data: WindowData) -> impl View {
    let db: Arc<LapceDb> = use_context(cx.scope).unwrap();

    let window_tabs = window_data.window_tabs.read_only();
    let active = window_data.active.read_only();
    let items = move || window_tabs.get();
    let key = |window_tab: &Arc<WindowTabData>| window_tab.window_tab_id;
    let active = move || active.get();
    let window_size = window_data.size;

    let local_window_data = window_data.clone();
    let window_view = tab(cx, active, items, key, window_tab)
        .style(cx, || Style::default().dimension_pct(1.0, 1.0))
        .on_event(EventListner::KeyDown, move |event| {
            if let Event::KeyDown(key_event) = event {
                window_data.key_down(cx, key_event);
                true
            } else {
                false
            }
        })
        .on_event(EventListner::WindowClosed, move |_| {
            println!("window closed");
            let _ = db.save_window(local_window_data.clone());
            true
        })
        .on_event(EventListner::WindowResized, move |event| {
            if let Event::WindowResized(size) = event {
                window_size.set(*size);
            }
            true
        });

    let id = window_view.id();
    cx.update_focus(id);

    window_view
}

fn app_logic(cx: AppContext) -> impl View {
    let db = Arc::new(LapceDb::new().unwrap());
    provide_context(cx.scope, db);

    let mut args = std::env::args_os();
    // Skip executable name
    args.next();
    let path = args
        .next()
        .map(|it| PathBuf::from(it).canonicalize().ok())
        .flatten()
        .or_else(|| Some(PathBuf::from("/Users/dz/lapce-rust")));

    let workspace = Arc::new(LapceWorkspace {
        kind: LapceWorkspaceType::Local,
        path,
        last_open: 0,
    });

    let window_data = WindowData::new(cx);
    window(cx, window_data)
}

pub fn launch() {
    floem::launch(app_logic);
}
