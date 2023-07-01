use std::{path::PathBuf, sync::atomic};

use floem::{
    event::EventListener,
    ext_event::create_ext_action,
    peniko::Color,
    reactive::{
        create_effect, create_memo, create_rw_signal, RwSignal, Scope, SignalGet,
        SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
        SignalWithUntracked,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{clip, empty, label, list, stack, svg, Decorators},
};
use lapce_core::buffer::{
    expand_diff_lines, rope_diff, rope_text::RopeText, DiffExpand, DiffLines,
};
use serde::{Deserialize, Serialize};

use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    doc::{DocContent, Document},
    id::{DiffEditorId, EditorId, EditorTabId},
    main_split::MainSplitData,
    wave::wave_box,
    window_tab::CommonData,
};

use super::{location::EditorLocation, EditorData, EditorViewKind};

#[derive(Clone)]
pub struct DiffInfo {
    pub is_right: bool,
    pub changes: Vec<DiffLines>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiffEditorInfo {
    pub left_content: DocContent,
    pub right_content: DocContent,
}

impl DiffEditorInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> DiffEditorData {
        let (cx, _) = data.scope.run_child_scope(|cx| cx);

        let diff_editor_id = DiffEditorId::next();

        let new_editor = {
            let data = data.clone();
            let common = data.common.clone();
            move |content: &DocContent| match content {
                DocContent::File(path) => {
                    let editor_id = EditorId::next();
                    let (doc, new_doc) = data.get_doc(path.clone());
                    let editor_data =
                        EditorData::new(cx, None, editor_id, doc, common.clone());
                    editor_data.go_to_location(
                        EditorLocation {
                            path: path.clone(),
                            position: None,
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        new_doc,
                        None,
                    );
                    editor_data
                }
                DocContent::Local => {
                    let editor_id = EditorId::next();
                    EditorData::new_local(data.scope, editor_id, common.clone())
                }
                DocContent::History(_) => {
                    let editor_id = EditorId::next();
                    EditorData::new_local(data.scope, editor_id, common.clone())
                }
            }
        };

        let left = new_editor(&self.left_content);
        let left = create_rw_signal(cx, left);
        let right = new_editor(&self.right_content);
        let right = create_rw_signal(cx, right);

        let diff_editor_data = DiffEditorData {
            id: diff_editor_id,
            editor_tab_id,
            scope: cx,
            left,
            right,
        };

        data.diff_editors.update(|diff_editors| {
            diff_editors.insert(diff_editor_id, diff_editor_data.clone());
        });

        diff_editor_data.listen_diff_changes();

        diff_editor_data
    }
}

#[derive(Clone)]
pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub editor_tab_id: EditorTabId,
    pub scope: Scope,
    pub left: RwSignal<EditorData>,
    pub right: RwSignal<EditorData>,
}

impl DiffEditorData {
    pub fn new(
        cx: Scope,
        id: DiffEditorId,
        editor_tab_id: EditorTabId,
        left_doc: RwSignal<Document>,
        right_doc: RwSignal<Document>,
        common: CommonData,
    ) -> Self {
        let (cx, _) = cx.run_child_scope(|cx| cx);
        let left =
            EditorData::new(cx, None, EditorId::next(), left_doc, common.clone());
        let left = create_rw_signal(left.scope, left);
        let right = EditorData::new(cx, None, EditorId::next(), right_doc, common);
        let right = create_rw_signal(right.scope, right);

        let data = Self {
            id,
            editor_tab_id,
            scope: cx,
            left,
            right,
        };

        data.listen_diff_changes();

        data
    }

    pub fn diff_editor_info(&self) -> DiffEditorInfo {
        DiffEditorInfo {
            left_content: self.left.get_untracked().doc.get_untracked().content,
            right_content: self.left.get_untracked().doc.get_untracked().content,
        }
    }

    pub fn copy(&self, cx: Scope, diff_editor_id: EditorId) -> Self {
        let (cx, _) = cx.run_child_scope(|cx| cx);
        let mut diff_editor = self.clone();
        diff_editor.scope = cx;
        diff_editor.id = diff_editor_id;
        diff_editor.left = create_rw_signal(
            cx,
            diff_editor
                .left
                .get_untracked()
                .copy(cx, None, EditorId::next()),
        );
        diff_editor.right = create_rw_signal(
            cx,
            diff_editor
                .right
                .get_untracked()
                .copy(cx, None, EditorId::next()),
        );
        diff_editor.listen_diff_changes();
        diff_editor
    }

    fn listen_diff_changes(&self) {
        let cx = self.scope;

        let left = self.left;
        let left_doc_rev = create_memo(cx, move |_| {
            let left_doc = left.with(|editor| editor.doc);
            left_doc.with(|doc| (doc.content.clone(), doc.rev()))
        });

        let right = self.right;
        let right_doc_rev = create_memo(cx, move |_| {
            let right_doc = right.with(|editor| editor.doc);
            right_doc.with(|doc| (doc.content.clone(), doc.rev()))
        });

        create_effect(cx, move |_| {
            let (_, left_rev) = left_doc_rev.get();
            let (left_editor_view, left_doc) =
                left.with_untracked(|editor| (editor.new_view, editor.doc));
            let (left_atomic_rev, left_rope) = left_doc.with_untracked(|doc| {
                (doc.buffer().atomic_rev(), doc.buffer().text().clone())
            });

            let (_, right_rev) = right_doc_rev.get();
            let (right_editor_view, right_doc) =
                right.with_untracked(|editor| (editor.new_view, editor.doc));
            let (right_atomic_rev, right_rope) = right_doc.with_untracked(|doc| {
                (doc.buffer().atomic_rev(), doc.buffer().text().clone())
            });

            let send = {
                let right_atomic_rev = right_atomic_rev.clone();
                create_ext_action(cx, move |changes: Option<Vec<DiffLines>>| {
                    let changes = if let Some(changes) = changes {
                        changes
                    } else {
                        return;
                    };

                    if left_atomic_rev.load(atomic::Ordering::Acquire) != left_rev {
                        return;
                    }

                    if right_atomic_rev.load(atomic::Ordering::Acquire) != right_rev
                    {
                        return;
                    }

                    left_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: false,
                        changes: changes.clone(),
                    }));
                    right_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: true,
                        changes,
                    }));
                })
            };

            rayon::spawn(move || {
                let changes = rope_diff(
                    left_rope,
                    right_rope,
                    right_rev,
                    right_atomic_rev.clone(),
                    Some(3),
                );
                send(changes);
            });
        });
    }
}

struct DiffShowMoreSection {
    actual_line: usize,
    visual_line: usize,
    lines: usize,
}

pub fn diff_show_more_section(editor: RwSignal<EditorData>) -> impl View {
    let (editor_view, viewport, config) = editor.with_untracked(|editor| {
        (editor.new_view, editor.viewport, editor.common.config)
    });

    let each_fn = move || {
        let editor_view = editor.with(|editor| editor.new_view);
        let editor_view = editor_view.get();
        if let EditorViewKind::Diff(diff_info) = editor_view {
            let viewport = viewport.get();
            let config = config.get_untracked();
            let line_height = config.editor.line_height() as f64;

            let min_line = (viewport.y0 / line_height).floor() as usize;
            let max_line = (viewport.y1 / line_height).ceil() as usize;

            let mut visual_line = 0;
            let mut last_change: Option<&DiffLines> = None;
            let mut changes = diff_info.changes.iter().peekable();
            let mut sections = Vec::new();
            while let Some(change) = changes.next() {
                match change {
                    DiffLines::Left(range) => {
                        if let Some(DiffLines::Right(_)) = changes.peek() {
                        } else {
                            let len = range.len();
                            visual_line += len;
                        }
                    }
                    DiffLines::Right(range) => {
                        let len = range.len();
                        visual_line += len;

                        if let Some(DiffLines::Left(r)) = last_change {
                            let len = r.len() - r.len().min(range.len());
                            if len > 0 {
                                visual_line += len;
                            }
                        };
                    }
                    DiffLines::Skip(_left, right) => {
                        if visual_line + 1 >= min_line {
                            sections.push(DiffShowMoreSection {
                                actual_line: right.start,
                                visual_line,
                                lines: right.len(),
                            });
                        }
                        visual_line += 1;
                    }
                    DiffLines::Both(_left, right) => {
                        let len = right.len();
                        visual_line += len;
                    }
                }
                if visual_line > max_line {
                    break;
                }
                last_change = Some(change);
            }
            sections
        } else {
            Vec::new()
        }
    };

    let key_fn =
        move |section: &DiffShowMoreSection| (section.visual_line, section.lines);

    let view_fn = move |section: DiffShowMoreSection| {
        stack(|| {
            (
                wave_box().style(move || {
                    Style::BASE
                        .absolute()
                        .size_pct(100.0, 100.0)
                        .color(*config.get().get_color(LapceColor::PANEL_BACKGROUND))
                }),
                label(move || format!("{} Hidden Lines", section.lines)),
                label(|| "|".to_string()).style(|| Style::BASE.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD_UP)).style(
                            move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            },
                        ),
                        label(|| "Expand Up".to_string())
                            .style(|| Style::BASE.margin_left_px(6.0)),
                    )
                })
                .style(|| {
                    Style::BASE
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style(|| Style::BASE.cursor(CursorStyle::Pointer)),
                label(|| "|".to_string()).style(|| Style::BASE.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD_DOWN))
                            .style(move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            }),
                        label(|| "Expand Down".to_string())
                            .style(|| Style::BASE.margin_left_px(6.0)),
                    )
                })
                .style(|| {
                    Style::BASE
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style(|| Style::BASE.cursor(CursorStyle::Pointer)),
                label(|| "|".to_string()).style(|| Style::BASE.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD)).style(
                            move || {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                Style::BASE.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            },
                        ),
                        label(|| "Expand All".to_string())
                            .style(|| Style::BASE.margin_left_px(6.0)),
                    )
                })
                .on_event(EventListener::PointerDown, move |_| true)
                .on_click(move |_event| {
                    editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.actual_line,
                                DiffExpand::All,
                            );
                        }
                    });
                    true
                })
                .style(|| {
                    Style::BASE
                        .border(1.0)
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style(|| Style::BASE.cursor(CursorStyle::Pointer)),
            )
        })
        .style(move || {
            let config = config.get();
            Style::BASE
                .absolute()
                .width_pct(100.0)
                .height_px(config.editor.line_height() as f32)
                .justify_center()
                .items_center()
                .margin_top_px(
                    (section.visual_line * config.editor.line_height()) as f32
                        - viewport.get().y0 as f32,
                )
        })
        .hover_style(|| Style::BASE.cursor(CursorStyle::Default))
    };

    stack(move || {
        (
            empty().style(move || {
                Style::BASE.height_px(config.get().editor.line_height() as f32 + 1.0)
            }),
            clip(|| {
                list(each_fn, key_fn, view_fn)
                    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0)),
        )
    })
    .style(|| Style::BASE.absolute().flex_col().size_pct(100.0, 100.0))
}
