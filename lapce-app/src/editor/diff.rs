use std::sync::atomic;

use floem::{
    event::EventListener,
    ext_event::create_ext_action,
    reactive::{RwSignal, Scope},
    style::{CursorStyle},
    view::View,
    views::{clip, empty, label, list, stack, svg, Decorators},
};
use lapce_core::buffer::{
    diff::{expand_diff_lines, rope_diff, DiffExpand, DiffLines},
    rope_text::RopeText,
};
use lapce_rpc::{buffer::BufferId, proxy::ProxyResponse};
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};

use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    doc::{DocContent, Document},
    id::{DiffEditorId, EditorId, EditorTabId},
    main_split::MainSplitData,
    wave::wave_box,
    window_tab::CommonData,
};

use super::{EditorData, EditorViewKind};

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
        let cx = data.scope.create_child();

        let diff_editor_id = DiffEditorId::next();

        let new_doc = {
            let data = data.clone();
            let common = data.common.clone();
            move |content: &DocContent| match content {
                DocContent::File(path) => {
                    let (doc, _) = data.get_doc(path.clone());
                    doc
                }
                DocContent::Local => cx.create_rw_signal(Document::new_local(
                    cx,
                    common.find.clone(),
                    common.proxy.clone(),
                    common.config,
                )),
                DocContent::History(history) => {
                    let doc = Document::new_hisotry(
                        cx,
                        content.clone(),
                        common.find.clone(),
                        common.proxy.clone(),
                        common.config,
                    );
                    let doc = doc.scope.create_rw_signal(doc);

                    let send = create_ext_action(cx, move |result| {
                        if let Ok(ProxyResponse::BufferHeadResponse {
                            content,
                            ..
                        }) = result
                        {
                            doc.update(|doc| {
                                doc.init_content(Rope::from(content));
                            });
                        }
                    });
                    common.proxy.get_buffer_head(
                        history.path.clone(),
                        move |result| {
                            send(result);
                        },
                    );
                    doc
                }
                DocContent::Scratch { name, .. } => {
                    let doc_content = DocContent::Scratch {
                        id: BufferId::next(),
                        name: name.to_string(),
                    };
                    let doc = Document::new_content(
                        cx,
                        doc_content,
                        common.find.clone(),
                        common.proxy.clone(),
                        common.config,
                    );
                    let doc = doc.scope.create_rw_signal(doc);
                    data.scratch_docs.update(|scratch_docs| {
                        scratch_docs.insert(name.to_string(), doc);
                    });
                    doc
                }
            }
        };

        let left_doc = new_doc(&self.left_content);
        let right_doc = new_doc(&self.right_content);

        let diff_editor_data = DiffEditorData::new(
            cx,
            diff_editor_id,
            editor_tab_id,
            left_doc,
            right_doc,
            data.common.clone(),
        );

        data.diff_editors.update(|diff_editors| {
            diff_editors.insert(diff_editor_id, diff_editor_data.clone());
        });

        diff_editor_data
    }
}

#[derive(Clone)]
pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub editor_tab_id: RwSignal<EditorTabId>,
    pub scope: Scope,
    pub left: RwSignal<EditorData>,
    pub right: RwSignal<EditorData>,
    pub focus_right: RwSignal<bool>,
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
        let cx = cx.create_child();
        let left =
            EditorData::new(cx, None, EditorId::next(), left_doc, common.clone());
        let left = left.scope.create_rw_signal(left);
        let right = EditorData::new(cx, None, EditorId::next(), right_doc, common);
        let right = right.scope.create_rw_signal(right);

        let data = Self {
            id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            scope: cx,
            left,
            right,
            focus_right: cx.create_rw_signal(true),
        };

        data.listen_diff_changes();

        data
    }

    pub fn diff_editor_info(&self) -> DiffEditorInfo {
        DiffEditorInfo {
            left_content: self.left.get_untracked().view.doc.get_untracked().content,
            right_content: self
                .right
                .get_untracked()
                .view
                .doc
                .get_untracked()
                .content,
        }
    }

    pub fn copy(
        &self,
        cx: Scope,
        editor_tab_id: EditorTabId,
        diff_editor_id: EditorId,
    ) -> Self {
        let cx = cx.create_child();

        let diff_editor = DiffEditorData {
            scope: cx,
            id: diff_editor_id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            focus_right: cx.create_rw_signal(true),
            left: cx.create_rw_signal(self.left.get_untracked().copy(
                cx,
                None,
                EditorId::next(),
            )),
            right: cx.create_rw_signal(self.right.get_untracked().copy(
                cx,
                None,
                EditorId::next(),
            )),
        };

        diff_editor.listen_diff_changes();
        diff_editor
    }

    fn listen_diff_changes(&self) {
        let cx = self.scope;

        let left = self.left;
        let left_doc_rev = cx.create_memo(move |_| {
            let left_doc = left.with(|editor| editor.view.doc);
            left_doc.with(|doc| (doc.content.clone(), doc.rev()))
        });

        let right = self.right;
        let right_doc_rev = cx.create_memo(move |_| {
            let right_doc = right.with(|editor| editor.view.doc);
            right_doc.with(|doc| (doc.content.clone(), doc.rev()))
        });

        cx.create_effect(move |_| {
            let (_, left_rev) = left_doc_rev.get();
            let (left_editor_view, left_doc) =
                left.with_untracked(|editor| (editor.view.kind, editor.view.doc));
            let (left_atomic_rev, left_rope) = left_doc.with_untracked(|doc| {
                (doc.buffer().atomic_rev(), doc.buffer().text().clone())
            });

            let (_, right_rev) = right_doc_rev.get();
            let (right_editor_view, right_doc) =
                right.with_untracked(|editor| (editor.view.kind, editor.view.doc));
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
    left_actual_line: usize,
    right_actual_line: usize,
    visual_line: usize,
    lines: usize,
}

pub fn diff_show_more_section_view(
    left_editor: RwSignal<EditorData>,
    right_editor: RwSignal<EditorData>,
) -> impl View {
    let left_editor_view = left_editor.with_untracked(|editor| editor.view.kind);
    let (right_editor_view, viewport, config) =
        right_editor.with_untracked(|editor| {
            (editor.view.kind, editor.viewport, editor.common.config)
        });

    let each_fn = move || {
        let editor_view = right_editor.with(|editor| editor.view.kind);
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
                    DiffLines::Both(info) => {
                        if let Some(skip) = info.skip.as_ref() {
                            visual_line += skip.start;
                            if visual_line + 1 >= min_line {
                                sections.push(DiffShowMoreSection {
                                    left_actual_line: info.left.start,
                                    right_actual_line: info.right.start,
                                    visual_line,
                                    lines: skip.len(),
                                });
                            }
                            visual_line += 1;
                            visual_line += info.right.len() - skip.end;
                        } else {
                            visual_line += info.right.len();
                        }
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
                wave_box().style(move  |base| {
                     base
                        .absolute()
                        .size_pct(100.0, 100.0)
                        .color(*config.get().get_color(LapceColor::PANEL_BACKGROUND))
                }),
                label(move || format!("{} Hidden Lines", section.lines)),
                label(|| "|".to_string()).style( |base|  base.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD)).style(
                            move  |base| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                 base.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            },
                        ),
                        label(|| "Expand All".to_string())
                            .style( |base|  base.margin_left_px(6.0)),
                    )
                })
                .on_event(EventListener::PointerDown, move |_| true)
                .on_click(move |_event| {
                    left_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.left_actual_line,
                                DiffExpand::All,
                                false,
                            );
                        }
                    });
                    right_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.right_actual_line,
                                DiffExpand::All,
                                true,
                            );
                        }
                    });
                    true
                })
                .style( |base| {
                     base
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style( |base|  base.cursor(CursorStyle::Pointer)),
                label(|| "|".to_string()).style( |base|  base.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD_UP)).style(
                            move  |base| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                 base.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            },
                        ),
                        label(|| "Expand Up".to_string())
                            .style( |base|  base.margin_left_px(6.0)),
                    )
                })
                .on_event(EventListener::PointerDown, move |_| true)
                .on_click(move |_event| {
                    left_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.left_actual_line,
                                DiffExpand::Up(10),
                                false,
                            );
                        }
                    });
                    right_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.right_actual_line,
                                DiffExpand::Up(10),
                                true,
                            );
                        }
                    });
                    true
                })
                .style(move  |base| {
                     base
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style( |base|  base.cursor(CursorStyle::Pointer)),
                label(|| "|".to_string()).style( |base|  base.margin_left_px(10.0)),
                stack(|| {
                    (
                        svg(move || config.get().ui_svg(LapceIcons::FOLD_DOWN))
                            .style(move  |base| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                 base.size_px(size, size).color(
                                    *config.get_color(LapceColor::EDITOR_FOREGROUND),
                                )
                            }),
                        label(|| "Expand Down".to_string())
                            .style( |base|  base.margin_left_px(6.0)),
                    )
                })
                .on_event(EventListener::PointerDown, move |_| true)
                .on_click(move |_event| {
                    left_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.left_actual_line,
                                DiffExpand::Down(10),
                                false,
                            );
                        }
                    });
                    right_editor_view.update(|editor_view| {
                        if let EditorViewKind::Diff(diff_info) = editor_view {
                            expand_diff_lines(
                                &mut diff_info.changes,
                                section.right_actual_line,
                                DiffExpand::Down(10),
                                true,
                            );
                        }
                    });
                    true
                })
                .style(move  |base| {
                     base
                        .margin_left_px(10.0)
                        .height_pct(100.0)
                        .items_center()
                })
                .hover_style( |base|  base.cursor(CursorStyle::Pointer)),
            )
        })
        .style(move  |base| {
            let config = config.get();
             base
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
        .hover_style( |base|  base.cursor(CursorStyle::Default))
    };

    stack(move || {
        (
            empty().style(move  |base| {
                 base.height_px(config.get().editor.line_height() as f32 + 1.0)
            }),
            clip(|| {
                list(each_fn, key_fn, view_fn)
                    .style( |base|  base.flex_col().size_pct(100.0, 100.0))
            })
            .style( |base|  base.size_pct(100.0, 100.0)),
        )
    })
    .style( |base|  base.absolute().flex_col().size_pct(100.0, 100.0))
}
