use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, Arc},
};

use floem::{
    app::AppContext,
    event::{Event, EventListner},
    parley::style::{FontFamily, FontStack, StyleProperty},
    peniko::{
        kurbo::{Point, Rect, Size},
        Brush, Color,
    },
    reactive::{
        create_effect, create_memo, create_signal, provide_context, use_context,
        ReadSignal, RwSignal, UntrackedGettableSignal, WriteSignal,
    },
    stack::stack,
    style::{
        AlignContent, AlignItems, Dimension, Display, FlexDirection, JustifyContent,
        Position, Style,
    },
    text::ParleyBrush,
    view::View,
    views::{click, svg},
    views::{
        container, container_box, list, tab, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize,
    },
    views::{label, scroll, text_layout},
};
use lapce_core::{
    cursor::{ColPosition, Cursor, CursorMode},
    mode::{Mode, VisualMode},
    selection::Selection,
};

use crate::{
    command::{CommandKind, LapceWorkbenchCommand},
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    db::LapceDb,
    doc::{DocContent, DocLine, Document, TextLayoutLine},
    editor::EditorData,
    editor_tab::{EditorTabChild, EditorTabData},
    focus_text::focus_text,
    id::{EditorId, EditorTabId, SplitId},
    keypress::{condition::Condition, DefaultKeyPress, KeyPressData, KeyPressFocus},
    main_split::{SplitContent, SplitData},
    palette::{
        item::{PaletteItem, PaletteItemContent},
        PaletteData, PaletteStatus,
    },
    proxy::{start_proxy, ProxyData},
    title::title,
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

        if line == cursor_line {
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

            if line == cursor_line {
                let caret = cursor_caret(doc, cursor_offset, false);
                renders.push(caret);
            }
        }
    }
    renders
}

fn editor_cursor(
    cx: AppContext,
    doc: ReadSignal<Document>,
    cursor: ReadSignal<Cursor>,
    viewport: ReadSignal<Rect>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let cursor = move || {
        let viewport = viewport.get();
        let config = config.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        doc.with_untracked(|doc| {
            cursor.with(|cursor| match &cursor.mode {
                CursorMode::Normal(offset) => {
                    let line = doc.buffer().line_of_offset(*offset);
                    let caret = cursor_caret(doc, *offset, true);
                    vec![
                        (viewport, CursorRender::CurrentLine { line }),
                        (viewport, caret),
                    ]
                }
                CursorMode::Visual { start, end, mode } => visual_cursor(
                    doc, *start, *end, mode, None, min_line, max_line, 7.5,
                )
                .into_iter()
                .map(|render| (viewport, render))
                .collect(),
                CursorMode::Insert(selection) => {
                    insert_cursor(doc, selection, min_line, max_line, 7.5)
                        .into_iter()
                        .map(|render| (viewport, render))
                        .collect()
                }
            })
        })
    };
    let id = AtomicU64::new(0);
    list(
        cx,
        move || cursor(),
        move |(viewport, cursor)| {
            id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        },
        move |cx, (viewport, curosr)| {
            label(cx, || "".to_string()).style(cx, move || {
                let config = config.get_untracked();
                let line_height = config.editor.line_height();

                Style {
                    width: match &curosr {
                        CursorRender::CurrentLine { .. } => Dimension::Percent(1.0),
                        CursorRender::Selection { width, .. } => {
                            Dimension::Points(*width as f32)
                        }
                        CursorRender::Caret { width, .. } => {
                            Dimension::Points(*width as f32)
                        }
                    },
                    margin_left: Some(match &curosr {
                        CursorRender::CurrentLine { .. } => 0.0,
                        CursorRender::Selection { x, .. } => {
                            (*x - viewport.x0) as f32
                        }
                        CursorRender::Caret { x, .. } => (*x - viewport.x0) as f32,
                    }),
                    margin_top: Some({
                        let line = match &curosr {
                            CursorRender::CurrentLine { line } => *line,
                            CursorRender::Selection { line, .. } => *line,
                            CursorRender::Caret { line, .. } => *line,
                        };
                        (line * line_height) as f32 - viewport.y0 as f32
                    }),
                    height: Dimension::Points(line_height as f32),
                    position: Position::Absolute,
                    background: match &curosr {
                        CursorRender::CurrentLine { .. } => {
                            Some(*config.get_color(LapceColor::EDITOR_CURRENT_LINE))
                        }
                        CursorRender::Selection { .. } => {
                            Some(*config.get_color(LapceColor::EDITOR_SELECTION))
                        }
                        CursorRender::Caret { .. } => {
                            Some(*config.get_color(LapceColor::EDITOR_CARET))
                        }
                    },
                    ..Default::default()
                }
            })
        },
    )
    .style(cx, move || Style {
        position: Position::Absolute,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        ..Default::default()
    })
}

fn editor(cx: AppContext, editor: ReadSignal<EditorData>) -> impl View {
    let (doc, cursor, config) = editor.with(|editor| {
        (
            editor.doc.read_only(),
            editor.cursor.read_only(),
            editor.config,
        )
    });

    let (viewport, set_viewport) = create_signal(cx.scope, Rect::ZERO);

    let key_fn = |line: &DocLine| format!("{}{}", line.rev, line.line);
    let view_fn =
        move |cx, line: DocLine| {
            stack(cx, move |cx| {
                (text_layout(cx, move || line.text.text.clone()).style(
                    cx,
                    move || {
                        let config = config.get_untracked();
                        let line_height = config.editor.line_height();
                        Style {
                            align_content: Some(AlignContent::Center),
                            height: Dimension::Points(line_height as f32),
                            ..Default::default()
                        }
                    },
                ),)
            })
        };

    stack(cx, |cx| {
        (
            editor_cursor(cx, doc, cursor, viewport, config),
            scroll(cx, |cx| {
                let config = config.get_untracked();
                let line_height = config.editor.line_height();
                virtual_list(
                    cx,
                    VirtualListDirection::Vertical,
                    move || doc.get(),
                    key_fn,
                    view_fn,
                    VirtualListItemSize::Fixed(line_height as f64),
                )
                .style(cx, || Style {
                    flex_direction: FlexDirection::Column,
                    ..Default::default()
                })
            })
            .onscroll(move |rect| {
                set_viewport.set(rect);
            })
            .on_ensure_visible(cx, move || {
                let cursor = cursor.get();
                let offset = cursor.offset();
                let caret = doc.with_untracked(|doc| {
                    cursor_caret(doc, offset, !cursor.is_insert())
                });
                let config = config.get_untracked();
                let line_height = config.editor.line_height();
                if let CursorRender::Caret { x, width, line } = caret {
                    Size::new(width, line_height as f64)
                        .to_rect()
                        .with_origin(Point::new(x, (line * line_height) as f64))
                } else {
                    Rect::ZERO
                }
            })
            .style(cx, || Style {
                position: Position::Absolute,
                width: Dimension::Percent(1.0),
                height: Dimension::Percent(1.0),
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        ..Default::default()
    })

    // scroll(cx, |cx| {
    //     stack(cx, |cx| {
    //         (
    //             editor_cursor(cx, doc, cursor, viewport, config),
    //             virtual_list(
    //                 cx,
    //                 VirtualListDirection::Vertical,
    //                 move || doc.get(),
    //                 key_fn,
    //                 view_fn,
    //                 VirtualListItemSize::Fixed(20.0),
    //             )
    //             .style(cx, || Style {
    //                 flex_direction: FlexDirection::Column,
    //                 min_width: Dimension::Percent(1.0),
    //                 min_height: Dimension::Percent(1.0),
    //                 border: 1.0,
    //                 ..Default::default()
    //             }),
    //         )
    //     })
    //     .style(cx, || Style {
    //         min_width: Dimension::Percent(1.0),
    //         min_height: Dimension::Percent(1.0),
    //         flex_direction: FlexDirection::Column,
    //         ..Default::default()
    //     })
    // })
    // .onscroll(move |rect| {
    //     set_viewport.set(rect);
    // })
    // .style(cx, || Style {
    //     position: Position::Absolute,
    //     border: 1.0,
    //     border_radius: 10.0,
    //     // flex_grow: 1.0,
    //     width: Dimension::Percent(1.0),
    //     height: Dimension::Percent(1.0),
    //     ..Default::default()
    // })
}

fn editor_tab_header(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let items = move || editor_tab.get().children.into_iter().enumerate();
    let key = |(_, child): &(usize, EditorTabChild)| child.id();
    let active = move || editor_tab.with(|editor_tab| editor_tab.active);

    let view_fn = move |cx, (i, child)| {
        let child = move |cx: AppContext| match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                let path = if let Some(editor_data) = editor_data {
                    let content = editor_data.with(|editor_data| {
                        editor_data.doc.with(|doc| doc.content.clone())
                    });
                    match content {
                        DocContent::File(path) => Some(path),
                        DocContent::Local => None,
                    }
                } else {
                    None
                };

                let (icon, color, path) = {
                    let config = config.get();
                    match path {
                        Some(path) => {
                            let (svg, color) = config.file_svg(&path);
                            (
                                svg,
                                color.cloned(),
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_str()
                                    .unwrap_or_default()
                                    .to_string(),
                            )
                        }
                        None => (
                            config.ui_svg(LapceIcons::FILE),
                            Some(*config.get_color(LapceColor::LAPCE_ICON_ACTIVE)),
                            "local".to_string(),
                        ),
                    }
                };
                stack(cx, |cx| {
                    (
                        container(cx, |cx| {
                            svg(cx, move || icon.clone()).style(cx, move || {
                                let size = config.get().ui.icon_size() as f32;
                                Style {
                                    width: Dimension::Points(size),
                                    height: Dimension::Points(size),
                                    ..Default::default()
                                }
                            })
                        })
                        .style(cx, || Style {
                            padding_left: 5.0,
                            padding_right: 5.0,
                            ..Default::default()
                        }),
                        label(cx, move || path.clone()),
                        container(cx, |cx| {
                            svg(cx, move || config.get().ui_svg(LapceIcons::CLOSE))
                                .style(cx, move || {
                                    let size = config.get().ui.icon_size() as f32;
                                    Style {
                                        width: Dimension::Points(size),
                                        height: Dimension::Points(size),
                                        ..Default::default()
                                    }
                                })
                        })
                        .style(cx, || Style {
                            padding_left: 5.0,
                            padding_right: 5.0,
                            ..Default::default()
                        }),
                    )
                })
                .style(cx, move || Style {
                    border_left: if i == 0 { 1.0 } else { 0.0 },
                    border_right: 1.0,
                    ..Default::default()
                })
            }
        };
        stack(cx, |cx| {
            (
                click(cx, child, move || {
                    editor_tab.update(|editor_tab| {
                        editor_tab.active = i;
                    });
                })
                .style(cx, move || Style {
                    align_items: Some(AlignItems::Center),
                    height: Dimension::Percent(1.0),
                    ..Default::default()
                }),
                container(cx, |cx| {
                    label(cx, || "".to_string()).style(cx, move || Style {
                        width: Dimension::Percent(1.0),
                        height: Dimension::Percent(1.0),
                        border_bottom: if active() == i { 2.0 } else { 0.0 },
                        ..Default::default()
                    })
                })
                .style(cx, || Style {
                    position: Position::Absolute,
                    padding_left: 3.0,
                    padding_right: 3.0,
                    width: Dimension::Percent(1.0),
                    height: Dimension::Percent(1.0),
                    ..Default::default()
                }),
            )
        })
        .style(cx, || Style {
            height: Dimension::Percent(1.0),
            ..Default::default()
        })
    };

    stack(cx, |cx| {
        (
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::TAB_PREVIOUS)).style(
                    cx,
                    move || {
                        let size = config.get().ui.icon_size() as f32;
                        Style {
                            width: Dimension::Points(size),
                            height: Dimension::Points(size),
                            ..Default::default()
                        }
                    },
                )
            })
            .style(cx, || Style {
                padding_left: 5.0,
                padding_right: 5.0,
                ..Default::default()
            }),
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::TAB_NEXT)).style(
                    cx,
                    move || {
                        let size = config.get().ui.icon_size() as f32;
                        Style {
                            width: Dimension::Points(size),
                            height: Dimension::Points(size),
                            ..Default::default()
                        }
                    },
                )
            })
            .style(cx, || Style {
                padding_left: 5.0,
                padding_right: 5.0,
                ..Default::default()
            }),
            container(cx, |cx| {
                scroll(cx, |cx| {
                    list(cx, items, key, view_fn).style(cx, || Style {
                        height: Dimension::Percent(1.0),
                        align_content: Some(AlignContent::Center),
                        ..Default::default()
                    })
                })
                .hide_bar()
                .style(cx, || Style {
                    position: Position::Absolute,
                    height: Dimension::Percent(1.0),
                    max_width: Dimension::Percent(1.0),
                    ..Default::default()
                })
            })
            .style(cx, || Style {
                height: Dimension::Percent(1.0),
                flex_grow: 1.0,
                ..Default::default()
            }),
            container(cx, |cx| {
                svg(cx, move || {
                    config.get().ui_svg(LapceIcons::SPLIT_HORIZONTAL)
                })
                .style(cx, move || {
                    let size = config.get().ui.icon_size() as f32;
                    Style {
                        width: Dimension::Points(size),
                        height: Dimension::Points(size),
                        ..Default::default()
                    }
                })
            })
            .style(cx, || Style {
                padding_left: 5.0,
                padding_right: 5.0,
                ..Default::default()
            }),
            container(cx, |cx| {
                svg(cx, move || config.get().ui_svg(LapceIcons::CLOSE)).style(
                    cx,
                    move || {
                        let size = config.get().ui.icon_size() as f32;
                        Style {
                            width: Dimension::Points(size),
                            height: Dimension::Points(size),
                            ..Default::default()
                        }
                    },
                )
            })
            .style(cx, || Style {
                padding_left: 5.0,
                padding_right: 5.0,
                ..Default::default()
            }),
        )
    })
    .style(cx, move || Style {
        height: Dimension::Points(config.get().ui.header_height() as f32),
        align_items: Some(AlignItems::Center),
        border_bottom: 1.0,
        background: Some(*config.get().get_color(LapceColor::PANEL_BACKGROUND)),
        ..Default::default()
    })
}

fn editor_tab_content(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
) -> impl View {
    let items = move || editor_tab.get().children;
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |cx, child| {
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                if let Some(editor_data) = editor_data {
                    container_box(cx, |cx| {
                        Box::new(editor(cx, editor_data.read_only()))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(cx, active, items, key, view_fn).style(cx, || Style {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

fn editor_tab(
    cx: AppContext,
    editor_tab: RwSignal<EditorTabData>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack(cx, |cx| {
        (
            editor_tab_header(cx, editor_tab, editors, config),
            editor_tab_content(cx, editor_tab, editors),
        )
    })
    .style(cx, || Style {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        ..Default::default()
    })
}

fn split_list(
    cx: AppContext,
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    split: ReadSignal<SplitData>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    editors: ReadSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let items = move || split.get().children;
    let key = |content: &SplitContent| content.id();
    let view_fn = move |cx, content| {
        let child = match content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    editor_tabs.with(|tabs| tabs.get(&editor_tab_id).cloned());
                if let Some(editor_tab_data) = editor_tab_data {
                    container_box(cx, |cx| {
                        Box::new(editor_tab(cx, editor_tab_data, editors, config))
                    })
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy editor tab".to_string()))
                    })
                }
            }
            SplitContent::Split(split_id) => {
                if let Some(split) =
                    splits.with(|splits| splits.get(&split_id).cloned())
                {
                    split_list(
                        cx,
                        splits,
                        split.read_only(),
                        editor_tabs,
                        editors,
                        config,
                    )
                } else {
                    container_box(cx, |cx| {
                        Box::new(label(cx, || "emtpy split".to_string()))
                    })
                }
            }
        };
        child.style(cx, || Style {
            height: Dimension::Percent(1.0),
            flex_grow: 1.0,
            ..Default::default()
        })
    };
    container_box(cx, |cx| {
        Box::new(list(cx, items, key, view_fn).style(cx, || Style {
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            ..Default::default()
        }))
    })
}

fn main_split(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get()
        .get(&root_split)
        .unwrap()
        .read_only();
    let splits = window_tab_data.main_split.splits.read_only();
    let editor_tabs = window_tab_data.main_split.editor_tabs.read_only();
    let editors = window_tab_data.main_split.editors.read_only();
    let config = window_tab_data.main_split.config;
    split_list(cx, splits, root_split, editor_tabs, editors, config).style(
        cx,
        move || Style {
            background: Some(*config.get().get_color(LapceColor::EDITOR_BACKGROUND)),
            flex_grow: 1.0,
            ..Default::default()
        },
    )
}

fn workbench(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let config = window_tab_data.main_split.config;
    stack(cx, move |cx| {
        (
            label(cx, move || "left".to_string()).style(cx, move || Style {
                padding: 20.0,
                border_right: 1.0,
                background: Some(
                    *config.get().get_color(LapceColor::PANEL_BACKGROUND),
                ),
                ..Default::default()
            }),
            stack(cx, move |cx| {
                (
                    main_split(cx, window_tab_data),
                    label(cx, move || "bottom".to_string()).style(cx, || Style {
                        padding: 20.0,
                        border_top: 1.0,
                        min_width: Dimension::Points(0.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, || Style {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_width: Dimension::Points(0.0),
                ..Default::default()
            }),
            label(cx, move || "right".to_string()).style(cx, move || Style {
                padding: 20.0,
                border_left: 1.0,
                min_width: Dimension::Points(0.0),
                background: Some(
                    *config.get().get_color(LapceColor::PANEL_BACKGROUND),
                ),
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        flex_grow: 1.0,
        flex_direction: FlexDirection::Row,
        ..Default::default()
    })
}

fn status(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> impl View {
    label(cx, move || "status".to_string()).style(cx, move || Style {
        border_top: 1.0,
        background: Some(*config.get().get_color(LapceColor::STATUS_BACKGROUND)),
        ..Default::default()
    })
}

fn palette_item(
    cx: AppContext,
    item: PaletteItem,
    index: ReadSignal<usize>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    match &item.content {
        PaletteItemContent::File { path, full_path } => {
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

            let item_index = item.index;

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

            stack(cx, move |cx| {
                (
                    focus_text(
                        cx,
                        move || file_name.clone(),
                        move || file_name_indices.clone(),
                        move || *config.get().get_color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(cx, || Style {
                        max_width: Dimension::Percent(1.0),
                        ..Default::default()
                    }),
                    focus_text(
                        cx,
                        move || folder.clone(),
                        move || folder_indices.clone(),
                        move || *config.get().get_color(LapceColor::EDITOR_FOCUS),
                    )
                    .style(cx, || Style {
                        margin_left: Some(6.0),
                        min_width: Dimension::Points(0.0),
                        ..Default::default()
                    }),
                )
            })
            .style(cx, move || Style {
                height: Dimension::Points(20.0),
                padding_left: 10.0,
                padding_right: 10.0,
                background: if index.get() == item_index {
                    Some(
                        *config
                            .get()
                            .get_color(LapceColor::PALETTE_CURRENT_BACKGROUND),
                    )
                } else {
                    None
                },
                ..Default::default()
            })
        }
    }
}

fn palette_input(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let doc = window_tab_data.palette.editor.doc.read_only();
    let cursor = window_tab_data.palette.editor.cursor.read_only();
    let config = window_tab_data.palette.config;
    let cursor_x = create_memo(cx.scope, move |_| {
        let offset = cursor.get().offset();
        let config = config.get();
        doc.with(|doc| {
            let (_, col) = doc.buffer().offset_to_line_col(offset);

            let line_content = doc.buffer().line_content(0);
            let mut text_layout_builder =
                floem::parley::LayoutContext::builder(&line_content[..col], 1.0);
            text_layout_builder.push_default(
                &floem::parley::style::StyleProperty::Brush(ParleyBrush(
                    Brush::Solid(Color::rgb8(0, 0, 0)),
                )),
            );
            text_layout_builder.push_default(&StyleProperty::FontSize(
                config.ui.font_size() as f32,
            ));
            let families = config.ui.font_family();
            text_layout_builder
                .push_default(&StyleProperty::FontStack(FontStack::List(&families)));
            let mut text_layout = text_layout_builder.build();
            text_layout
                .break_all_lines(None, floem::parley::layout::Alignment::Start);

            text_layout.width()
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
                            Style {
                                position: Position::Absolute,
                                margin_left: Some(cursor_x.get() - 1.0),
                                width: Dimension::Points(2.0),
                                background: Some(
                                    *config
                                        .get()
                                        .get_color(LapceColor::EDITOR_CARET),
                                ),
                                // border_left: 2.0,
                                ..Default::default()
                            }
                        }),
                    )
                })
            })
            .on_ensure_visible(cx, move || {
                Size::new(20.0, 0.0)
                    .to_rect()
                    .with_origin(Point::new(cursor_x.get() as f64 - 10.0, 0.0))
            })
            .style(cx, || Style {
                flex_grow: 1.0,
                min_width: Dimension::Points(0.0),
                height: Dimension::Points(24.0),
                align_items: Some(AlignItems::Center),
                ..Default::default()
            })
        })
        .style(cx, move || Style {
            flex_grow: 1.0,
            min_width: Dimension::Points(0.0),
            border: 1.0,
            border_radius: 2.0,
            background: Some(*config.get().get_color(LapceColor::EDITOR_BACKGROUND)),
            padding_left: 5.0,
            padding_right: 5.0,
            ..Default::default()
        })
    })
    .style(cx, || Style {
        padding: 5.0,
        ..Default::default()
    })
}

fn palette_content(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let index = window_tab_data.palette.index.read_only();
    let config = window_tab_data.palette.config;
    container(cx, |cx| {
        scroll(cx, |cx| {
            virtual_list(
                cx,
                VirtualListDirection::Vertical,
                move || items.get(),
                move |item| (item.id, item.index, item.indices.clone()),
                move |cx, item| palette_item(cx, item, index, config),
                VirtualListItemSize::Fixed(20.0),
            )
            .style(cx, || Style {
                width: Dimension::Percent(1.0),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            })
        })
        .on_ensure_visible(cx, move || {
            Size::new(1.0, 20.0)
                .to_rect()
                .with_origin(Point::new(0.0, index.get() as f64 * 20.0))
        })
        .style(cx, || Style {
            width: Dimension::Percent(1.0),
            min_height: Dimension::Points(0.0),
            ..Default::default()
        })
    })
    .style(cx, move || Style {
        display: if items.with(|items| items.is_empty()) {
            Display::None
        } else {
            Display::Flex
        },
        width: Dimension::Percent(1.0),
        min_height: Dimension::Points(0.0),
        padding_bottom: 5.0,
        ..Default::default()
    })
}

fn palette(cx: AppContext, window_tab_data: WindowTabData) -> impl View {
    let keypress = window_tab_data.keypress.write_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.config;
    container(cx, |cx| {
        stack(cx, |cx| {
            (
                palette_input(cx, window_tab_data.clone()),
                palette_content(cx, window_tab_data.clone()),
            )
        })
        .style(cx, move || Style {
            width: Dimension::Points(512.0),
            max_width: Dimension::Percent(0.9),
            min_height: Dimension::Points(0.0),
            max_height: Dimension::Percent(0.5),
            margin_top: Some(-1.0),
            border: 1.0,
            border_radius: 5.0,
            flex_direction: FlexDirection::Column,
            background: Some(
                *config.get().get_color(LapceColor::PALETTE_BACKGROUND),
            ),
            ..Default::default()
        })
    })
    .style(cx, move || Style {
        display: if status.get() == PaletteStatus::Inactive {
            Display::None
        } else {
            Display::Flex
        },
        position: Position::Absolute,
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        align_content: Some(AlignContent::Center),
        ..Default::default()
    })
}

fn window_tab(cx: AppContext, workspace: Arc<LapceWorkspace>) -> impl View {
    let window_tab_data = WindowTabData::new(cx, workspace);
    let workbench_command = window_tab_data.workbench_command;

    {
        let window_tab_data = window_tab_data.clone();
        create_effect(cx.scope, move |_| {
            if let Some(cmd) = workbench_command.get() {
                window_tab_data.run_workbench_command(cx, cmd);
            }
        });
    }

    {
        let window_tab_data = window_tab_data.clone();
        let internal_command = window_tab_data.internal_command;
        create_effect(cx.scope, move |_| {
            if let Some(cmd) = internal_command.get() {
                println!("get internal command");
                window_tab_data.run_internal_command(cx, cmd);
            }
        });
    }

    let proxy_data = window_tab_data.proxy.clone();
    let keypress = window_tab_data.keypress;
    let config = window_tab_data.main_split.config;
    let window_tab_view = stack(cx, |cx| {
        (
            stack(cx, |cx| {
                (
                    title(cx, &proxy_data, config),
                    workbench(cx, window_tab_data.clone()),
                    status(cx, window_tab_data.main_split.config),
                )
            })
            .style(cx, || Style {
                width: Dimension::Percent(1.0),
                height: Dimension::Percent(1.0),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            }),
            palette(cx, window_tab_data.clone()),
        )
    })
    .style(cx, move || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        color: Some(*config.get().get_color(LapceColor::EDITOR_FOREGROUND)),
        background: Some(*config.get().get_color(LapceColor::EDITOR_BACKGROUND)),
        font_size: Some(config.get().ui.font_size() as f32),
        ..Default::default()
    })
    .event(EventListner::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            window_tab_data.key_down(cx, key_event);
            true
        } else {
            false
        }
    });

    let id = window_tab_view.id();
    cx.update_focus(id);

    window_tab_view
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

    window_tab(cx, workspace)
}

pub fn launch() {
    floem::launch(app_logic);
}
