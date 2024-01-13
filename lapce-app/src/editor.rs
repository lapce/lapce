use std::{
    cmp::Ordering, collections::HashMap, rc::Rc, str::FromStr, sync::Arc,
    time::Duration,
};

use anyhow::Result;
use floem::{
    action::{exec_after, show_context_menu, TimerToken},
    ext_event::create_ext_action,
    keyboard::ModifiersState,
    menu::{Menu, MenuItem},
    peniko::kurbo::{Point, Rect, Vec2},
    pointer::{PointerButton, PointerInputEvent, PointerMoveEvent},
    reactive::{batch, use_context, ReadSignal, RwSignal, Scope},
};
use lapce_core::{
    buffer::{diff::DiffLines, rope_text::RopeText, InvalLines},
    command::{EditCommand, FocusCommand, MotionModeCommand, MultiSelectionCommand},
    cursor::{Cursor, CursorMode},
    editor::EditType,
    mode::{Mode, MotionMode},
    movement::Movement,
    selection::{InsertDrift, Selection},
    syntax::edit::SyntaxEdit,
};
use lapce_rpc::{buffer::BufferId, plugin::PluginId, proxy::ProxyResponse};
use lapce_xi_rope::{Rope, RopeDelta, Transformer};
use lsp_types::{
    CompletionItem, CompletionTextEdit, GotoDefinitionResponse, HoverContents,
    InlineCompletionTriggerKind, Location, MarkedString, MarkupKind, TextEdit,
};
use serde::{Deserialize, Serialize};

use crate::{
    command::{
        CommandExecuted, CommandKind, InternalCommand, LapceCommand,
        LapceWorkbenchCommand,
    },
    completion::CompletionStatus,
    config::LapceConfig,
    db::LapceDb,
    doc::{DocContent, Document, DocumentExt},
    editor::{
        location::{EditorLocation, EditorPosition},
        visual_line::Lines,
    },
    editor_tab::EditorTabChild,
    id::{DiffEditorId, EditorId, EditorTabId},
    inline_completion::{InlineCompletionItem, InlineCompletionStatus},
    keypress::{condition::Condition, KeyPressFocus},
    main_split::{MainSplitData, SplitDirection, SplitMoveDirection},
    markdown::{
        from_marked_string, from_plaintext, parse_markdown, MarkdownContent,
    },
    proxy::path_from_url,
    snippet::Snippet,
    window_tab::{CommonData, Focus, WindowTabData},
};

use self::{
    view::{DiffSection, DiffSectionKind, LineInfo, ScreenLines, ScreenLinesBase},
    view_data::{EditorViewData, EditorViewKind},
    visual_line::{TextLayoutProvider, VLine, VLineInfo},
};

pub mod diff;
pub mod gutter;
pub mod location;
pub mod movement;
pub mod view;
pub mod view_data;
pub mod visual_line;

const CHAR_WIDTH: f64 = 7.5;

#[derive(Clone, Debug)]
pub enum InlineFindDirection {
    Left,
    Right,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorInfo {
    pub content: DocContent,
    // pub unsaved: Option<String>,
    pub offset: usize,
    pub scroll_offset: (f64, f64),
}

impl EditorInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> Rc<EditorData> {
        let editor_id = EditorId::next();
        let editor_data = match &self.content {
            DocContent::File { path, .. } => {
                let (doc, new_doc) = data.get_doc(path.clone());
                let editor_data = EditorData::new(
                    data.scope,
                    Some(editor_tab_id),
                    None,
                    editor_id,
                    doc,
                    None,
                    data.common,
                );
                editor_data.go_to_location(
                    EditorLocation {
                        path: path.clone(),
                        position: Some(EditorPosition::Offset(self.offset)),
                        scroll_offset: Some(Vec2::new(
                            self.scroll_offset.0,
                            self.scroll_offset.1,
                        )),
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                    new_doc,
                    None,
                );
                editor_data
            }
            DocContent::Local => {
                EditorData::new_local(data.scope, editor_id, data.common)
            }
            DocContent::History(_) => {
                EditorData::new_local(data.scope, editor_id, data.common)
            }
            DocContent::Scratch { name, .. } => {
                let doc = data
                    .scratch_docs
                    .try_update(|scratch_docs| {
                        if let Some(doc) = scratch_docs.get(name) {
                            return doc.clone();
                        }
                        let content = DocContent::Scratch {
                            id: BufferId::next(),
                            name: name.to_string(),
                        };
                        let doc = Document::new_content(
                            data.scope,
                            content,
                            data.common.clone(),
                        );
                        let doc = Rc::new(doc);
                        scratch_docs.insert(name.to_string(), doc.clone());
                        doc
                    })
                    .unwrap();

                EditorData::new(
                    data.scope,
                    Some(editor_tab_id),
                    None,
                    editor_id,
                    doc,
                    None,
                    data.common,
                )
            }
        };
        let editor_data = Rc::new(editor_data);
        data.editors.update(|editors| {
            editors.insert(editor_id, editor_data.clone());
        });
        editor_data
    }
}

pub type SnippetIndex = Vec<(usize, (usize, usize))>;

#[derive(Clone)]
pub struct EditorData {
    pub scope: Scope,
    pub editor_id: EditorId,
    pub editor_tab_id: RwSignal<Option<EditorTabId>>,
    pub diff_editor_id: RwSignal<Option<(EditorTabId, DiffEditorId)>>,
    pub view: EditorViewData,
    pub confirmed: RwSignal<bool>,
    pub cursor: RwSignal<Cursor>,
    pub window_origin: RwSignal<Point>,
    pub viewport: RwSignal<Rect>,
    pub scroll_delta: RwSignal<Vec2>,
    pub scroll_to: RwSignal<Option<Vec2>>,
    pub snippet: RwSignal<Option<SnippetIndex>>,
    pub last_movement: RwSignal<Movement>,
    pub inline_find: RwSignal<Option<InlineFindDirection>>,
    pub last_inline_find: RwSignal<Option<(InlineFindDirection, String)>>,
    pub find_focus: RwSignal<bool>,
    pub active: RwSignal<bool>,
    pub sticky_header_height: RwSignal<f64>,
    pub common: Rc<CommonData>,
}

impl PartialEq for EditorData {
    fn eq(&self, other: &Self) -> bool {
        self.editor_id == other.editor_id
    }
}

impl EditorData {
    pub fn new(
        cx: Scope,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        editor_id: EditorId,
        doc: Rc<Document>,
        confirmed: Option<RwSignal<bool>>,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = cx.create_child();

        let is_local = doc.content.with_untracked(|content| content.is_local());
        let viewport = cx.create_rw_signal(Rect::ZERO);
        let modal = common.config.with_untracked(|c| c.core.modal);
        let cursor = Cursor::new(
            if modal && !is_local {
                CursorMode::Normal(0)
            } else {
                CursorMode::Insert(Selection::caret(0))
            },
            None,
            None,
        );
        let cursor = cx.create_rw_signal(cursor);
        let view = EditorViewData::new(
            cx,
            doc,
            EditorViewKind::Normal,
            viewport,
            common.config,
        );
        {
            let internal_comamnd = common.internal_command;
            cx.create_effect(move |_| {
                cursor.track();
                internal_comamnd.send(InternalCommand::ResetBlinkCursor);
            });
        }
        let confirmed = confirmed.unwrap_or_else(|| cx.create_rw_signal(false));
        Self {
            scope: cx,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            diff_editor_id: cx.create_rw_signal(diff_editor_id),
            editor_id,
            view,
            cursor,
            confirmed,
            snippet: cx.create_rw_signal(None),
            window_origin: cx.create_rw_signal(Point::ZERO),
            viewport,
            scroll_delta: cx.create_rw_signal(Vec2::ZERO),
            scroll_to: cx.create_rw_signal(None),
            last_movement: cx.create_rw_signal(Movement::Left),
            inline_find: cx.create_rw_signal(None),
            last_inline_find: cx.create_rw_signal(None),
            find_focus: cx.create_rw_signal(false),
            active: cx.create_rw_signal(false),
            sticky_header_height: cx.create_rw_signal(0.0),
            common,
        }
    }

    pub fn new_local(
        cx: Scope,
        editor_id: EditorId,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = cx.create_child();
        let doc = Rc::new(Document::new_local(cx, common.clone()));
        Self::new(cx, None, None, editor_id, doc, None, common)
    }

    pub fn editor_info(&self, _data: &WindowTabData) -> EditorInfo {
        let offset = self.cursor.get_untracked().offset();
        let scroll_offset = self.viewport.get_untracked().origin();
        EditorInfo {
            content: self.view.doc.get_untracked().content.get_untracked(),
            offset,
            scroll_offset: (scroll_offset.x, scroll_offset.y),
        }
    }

    /// Swap out the document this editor is for.
    pub fn update_doc(&self, doc: Rc<Document>) {
        self.view.update_doc(doc);
    }

    pub fn copy(
        &self,
        cx: Scope,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        editor_id: EditorId,
        confirmed: Option<RwSignal<bool>>,
    ) -> Self {
        let cx = cx.create_child();
        let cursor = cx.create_rw_signal(self.cursor.get_untracked());
        {
            let internal_comamnd = self.common.internal_command;
            cx.create_effect(move |_| {
                cursor.track();
                internal_comamnd.send(InternalCommand::ResetBlinkCursor);
            });
        }
        let viewport = cx.create_rw_signal(self.viewport.get_untracked());
        let confirmed = confirmed.unwrap_or_else(|| cx.create_rw_signal(true));

        EditorData {
            scope: cx,
            editor_id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            diff_editor_id: cx.create_rw_signal(diff_editor_id),
            view: self.view.duplicate(cx, viewport),
            cursor,
            viewport,
            scroll_delta: cx.create_rw_signal(Vec2::ZERO),
            scroll_to: cx.create_rw_signal(Some(
                self.viewport.get_untracked().origin().to_vec2(),
            )),
            window_origin: cx.create_rw_signal(Point::ZERO),
            confirmed,
            snippet: cx.create_rw_signal(None),
            last_movement: cx.create_rw_signal(self.last_movement.get_untracked()),
            inline_find: cx.create_rw_signal(None),
            last_inline_find: cx.create_rw_signal(None),
            find_focus: cx.create_rw_signal(false),
            active: cx.create_rw_signal(false),
            sticky_header_height: cx.create_rw_signal(0.0),
            common: self.common.clone(),
        }
    }

    fn run_edit_command(&self, cmd: &EditCommand) -> CommandExecuted {
        let doc = self.view.doc.get_untracked();
        let modal = self
            .common
            .config
            .with_untracked(|config| config.core.modal)
            && !doc.content.with_untracked(|content| content.is_local());
        let smart_tab = self
            .common
            .config
            .with_untracked(|config| config.editor.smart_tab);
        let doc_before_edit =
            doc.buffer.with_untracked(|buffer| buffer.text().clone());
        let mut cursor = self.cursor.get_untracked();
        let mut register = self.common.register.get_untracked();

        let yank_data =
            if let lapce_core::cursor::CursorMode::Visual { .. } = &cursor.mode {
                Some(
                    self.view
                        .doc
                        .get_untracked()
                        .buffer
                        .with_untracked(|buffer| cursor.yank(buffer)),
                )
            } else {
                None
            };

        let deltas =
            batch(|| doc.do_edit(&mut cursor, cmd, modal, &mut register, smart_tab));

        if !deltas.is_empty() {
            if let Some(data) = yank_data {
                register.add_delete(data);
            }
        }

        self.cursor.set(cursor);
        self.common.register.set(register);

        if show_completion(cmd, &doc_before_edit, &deltas) {
            self.update_completion(false);
        } else {
            self.cancel_completion();
        }

        if *cmd == EditCommand::InsertNewLine {
            // Cancel so that there's no flickering
            self.cancel_inline_completion();
            self.update_inline_completion(InlineCompletionTriggerKind::Automatic);
        } else if show_inline_completion(cmd) {
            self.update_inline_completion(InlineCompletionTriggerKind::Automatic);
        } else {
            self.cancel_inline_completion();
        }

        self.apply_deltas(&deltas);
        if let EditCommand::NormalMode = cmd {
            self.snippet.set(None);
        }

        CommandExecuted::Yes
    }

    fn run_motion_mode_command(
        &self,
        cmd: &MotionModeCommand,
        count: Option<usize>,
    ) -> CommandExecuted {
        let count = count.unwrap_or(1);
        let motion_mode = match cmd {
            MotionModeCommand::MotionModeDelete => MotionMode::Delete { count },
            MotionModeCommand::MotionModeIndent => MotionMode::Indent,
            MotionModeCommand::MotionModeOutdent => MotionMode::Outdent,
            MotionModeCommand::MotionModeYank => MotionMode::Yank { count },
        };
        let mut cursor = self.cursor.get_untracked();
        let mut register = self.common.register.get_untracked();

        movement::do_motion_mode(
            &self.view.doc.get_untracked(),
            &mut cursor,
            motion_mode,
            &mut register,
        );

        self.cursor.set(cursor);
        self.common.register.set(register);

        CommandExecuted::Yes
    }

    fn run_multi_selection_command(
        &self,
        cmd: &MultiSelectionCommand,
    ) -> CommandExecuted {
        let mut cursor = self.cursor.get_untracked();
        movement::do_multi_selection(&self.view, &mut cursor, cmd);
        self.cursor.set(cursor);
        // self.cancel_signature();
        self.cancel_completion();
        self.cancel_inline_completion();
        CommandExecuted::Yes
    }

    fn run_move_command(
        &self,
        movement: &lapce_core::movement::Movement,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        if movement.is_jump() && movement != &self.last_movement.get_untracked() {
            let path = self
                .view
                .doc
                .get_untracked()
                .content
                .with_untracked(|content| content.path().cloned());
            if let Some(path) = path {
                let offset = self.cursor.with_untracked(|c| c.offset());
                let scroll_offset = self.viewport.get_untracked().origin().to_vec2();
                self.common.internal_command.send(
                    InternalCommand::SaveJumpLocation {
                        path,
                        offset,
                        scroll_offset,
                    },
                );
            }
        }
        self.last_movement.set(movement.clone());

        let mut cursor = self.cursor.get_untracked();
        self.common.register.update(|register| {
            movement::move_cursor(
                &self.view,
                &mut cursor,
                movement,
                count.unwrap_or(1),
                mods.shift_key(),
                register,
            )
        });

        self.cursor.set(cursor);

        if self.snippet.with_untracked(|s| s.is_some()) {
            self.snippet.update(|snippet| {
                let offset = self.cursor.get_untracked().offset();
                let mut within_region = false;
                for (_, (start, end)) in snippet.as_mut().unwrap() {
                    if offset >= *start && offset <= *end {
                        within_region = true;
                        break;
                    }
                }
                if !within_region {
                    *snippet = None;
                }
            })
        }
        self.cancel_completion();
        CommandExecuted::Yes
    }

    pub fn run_focus_command(
        &self,
        cmd: &FocusCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        // TODO(minor): Evaluate whether we should split this into subenums,
        // such as actions specific to the actual editor pane, movement, and list movement.
        let prev_completion_index = self
            .common
            .completion
            .with_untracked(|c| c.active.get_untracked());

        match cmd {
            FocusCommand::ModalClose => {
                self.cancel_completion();
            }
            FocusCommand::SplitVertical => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common.internal_command.send(InternalCommand::Split {
                        direction: SplitDirection::Vertical,
                        editor_tab_id,
                    });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common.internal_command.send(InternalCommand::Split {
                        direction: SplitDirection::Vertical,
                        editor_tab_id,
                    });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitHorizontal => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common.internal_command.send(InternalCommand::Split {
                        direction: SplitDirection::Horizontal,
                        editor_tab_id,
                    });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common.internal_command.send(InternalCommand::Split {
                        direction: SplitDirection::Horizontal,
                        editor_tab_id,
                    });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitRight => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Right,
                            editor_tab_id,
                        });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Right,
                            editor_tab_id,
                        });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitLeft => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Left,
                            editor_tab_id,
                        });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Left,
                            editor_tab_id,
                        });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitUp => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Up,
                            editor_tab_id,
                        });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Up,
                            editor_tab_id,
                        });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitDown => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Down,
                            editor_tab_id,
                        });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitMove {
                            direction: SplitMoveDirection::Down,
                            editor_tab_id,
                        });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitExchange => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitExchange { editor_tab_id });
                } else if let Some((editor_tab_id, _)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common
                        .internal_command
                        .send(InternalCommand::SplitExchange { editor_tab_id });
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::SplitClose => {
                if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
                    self.common.internal_command.send(
                        InternalCommand::EditorTabChildClose {
                            editor_tab_id,
                            child: EditorTabChild::Editor(self.editor_id),
                        },
                    );
                } else if let Some((editor_tab_id, diff_editor_id)) =
                    self.diff_editor_id.get_untracked()
                {
                    self.common.internal_command.send(
                        InternalCommand::EditorTabChildClose {
                            editor_tab_id,
                            child: EditorTabChild::DiffEditor(diff_editor_id),
                        },
                    );
                } else {
                    return CommandExecuted::No;
                }
            }
            FocusCommand::PageUp => {
                self.page_move(false, mods);
            }
            FocusCommand::PageDown => {
                self.page_move(true, mods);
            }
            FocusCommand::ScrollUp => {
                self.scroll(false, count.unwrap_or(1), mods);
            }
            FocusCommand::ScrollDown => {
                self.scroll(true, count.unwrap_or(1), mods);
            }
            FocusCommand::ListNext => {
                self.common.completion.update(|c| {
                    c.next();
                });
            }
            FocusCommand::ListPrevious => {
                self.common.completion.update(|c| {
                    c.previous();
                });
            }
            FocusCommand::ListNextPage => {
                self.common.completion.update(|c| {
                    c.next_page();
                });
            }
            FocusCommand::ListPreviousPage => {
                self.common.completion.update(|c| {
                    c.previous_page();
                });
            }
            FocusCommand::ListSelect => {
                self.select_completion();
                self.cancel_inline_completion();
            }
            FocusCommand::JumpToNextSnippetPlaceholder => {
                self.snippet.update(|snippet| {
                    if let Some(snippet_mut) = snippet.as_mut() {
                        let mut current = 0;
                        let offset = self.cursor.get_untracked().offset();
                        for (i, (_, (start, end))) in snippet_mut.iter().enumerate()
                        {
                            if *start <= offset && offset <= *end {
                                current = i;
                                break;
                            }
                        }

                        let last_placeholder = current + 1 >= snippet_mut.len() - 1;

                        if let Some((_, (start, end))) = snippet_mut.get(current + 1)
                        {
                            let mut selection =
                                lapce_core::selection::Selection::new();
                            let region = lapce_core::selection::SelRegion::new(
                                *start, *end, None,
                            );
                            selection.add_region(region);
                            self.cursor.update(|cursor| {
                                cursor.set_insert(selection);
                            });
                        }

                        if last_placeholder {
                            *snippet = None;
                        }
                        // self.update_signature();
                        self.cancel_completion();
                        self.cancel_inline_completion();
                    }
                });
            }
            FocusCommand::JumpToPrevSnippetPlaceholder => {
                self.snippet.update(|snippet| {
                    if let Some(snippet_mut) = snippet.as_mut() {
                        let mut current = 0;
                        let offset = self.cursor.get_untracked().offset();
                        for (i, (_, (start, end))) in snippet_mut.iter().enumerate()
                        {
                            if *start <= offset && offset <= *end {
                                current = i;
                                break;
                            }
                        }

                        if current > 0 {
                            if let Some((_, (start, end))) =
                                snippet_mut.get(current - 1)
                            {
                                let mut selection =
                                    lapce_core::selection::Selection::new();
                                let region = lapce_core::selection::SelRegion::new(
                                    *start, *end, None,
                                );
                                selection.add_region(region);
                                self.cursor.update(|cursor| {
                                    cursor.set_insert(selection);
                                });
                            }
                            // self.update_signature();
                            self.cancel_completion();
                            self.cancel_inline_completion();
                        }
                    }
                });
            }
            FocusCommand::GotoDefinition => {
                self.go_to_definition();
            }
            FocusCommand::ShowCodeActions => {
                self.show_code_actions(false);
            }
            FocusCommand::SearchWholeWordForward => {
                self.search_whole_word_forward(mods);
            }
            FocusCommand::SearchForward => {
                self.search_forward(mods);
            }
            FocusCommand::SearchBackward => {
                self.search_backward(mods);
            }
            FocusCommand::Save => {
                self.save(true, || {});
            }
            FocusCommand::SaveWithoutFormatting => {
                self.save(false, || {});
            }
            FocusCommand::FormatDocument => {
                self.format();
            }
            FocusCommand::InlineFindLeft => {
                self.inline_find.set(Some(InlineFindDirection::Left));
            }
            FocusCommand::InlineFindRight => {
                self.inline_find.set(Some(InlineFindDirection::Right));
            }
            FocusCommand::RepeatLastInlineFind => {
                if let Some((direction, c)) = self.last_inline_find.get_untracked() {
                    self.inline_find(direction, &c);
                }
            }
            FocusCommand::Rename => {
                self.rename();
            }
            FocusCommand::ClearSearch => {
                self.clear_search();
            }
            FocusCommand::Search => {
                self.search();
            }
            FocusCommand::FocusFindEditor => {
                self.common.find.replace_focus.set(false);
            }
            FocusCommand::FocusReplaceEditor => {
                if self.common.find.replace_active.get_untracked() {
                    self.common.find.replace_focus.set(true);
                }
            }
            FocusCommand::InlineCompletionSelect => {
                self.select_inline_completion();
            }
            FocusCommand::InlineCompletionNext => {
                self.next_inline_completion();
            }
            FocusCommand::InlineCompletionPrevious => {
                self.previous_inline_completion();
            }
            FocusCommand::InlineCompletionCancel => {
                self.cancel_inline_completion();
            }
            FocusCommand::InlineCompletionInvoke => {
                self.update_inline_completion(InlineCompletionTriggerKind::Invoked);
            }
            _ => {}
        }

        let current_completion_index = self
            .common
            .completion
            .with_untracked(|c| c.active.get_untracked());

        if prev_completion_index != current_completion_index {
            self.common.completion.with_untracked(|c| {
                let cursor_offset = self.cursor.with_untracked(|c| c.offset());
                c.update_document_completion(&self.view, cursor_offset);
            });
        }

        CommandExecuted::Yes
    }

    /// Jump to the next/previous column on the line which matches the given text
    fn inline_find(&self, direction: InlineFindDirection, c: &str) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let doc = self.view.doc.get_untracked();
        let (line_content, line_start_offset) =
            doc.buffer.with_untracked(|buffer| {
                let line = buffer.line_of_offset(offset);
                let line_content = buffer.line_content(line);
                let line_start_offset = buffer.offset_of_line(line);
                (line_content.to_string(), line_start_offset)
            });
        let index = offset - line_start_offset;
        if let Some(new_index) = match direction {
            InlineFindDirection::Left => line_content[..index].rfind(c),
            InlineFindDirection::Right => {
                if index + 1 >= line_content.len() {
                    None
                } else {
                    let index = index
                        + doc.buffer.with_untracked(|buffer| {
                            buffer.next_grapheme_offset(
                                offset,
                                1,
                                buffer.offset_line_end(offset, false),
                            )
                        })
                        - offset;
                    line_content[index..].find(c).map(|i| i + index)
                }
            }
        } {
            self.run_move_command(
                &lapce_core::movement::Movement::Offset(
                    new_index + line_start_offset,
                ),
                None,
                ModifiersState::empty(),
            );
        }
    }

    fn go_to_definition(&self) {
        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let (start_position, position) = doc.buffer.with_untracked(|buffer| {
            let start_offset = buffer.prev_code_boundary(offset);
            let start_position = buffer.offset_to_position(start_offset);
            let position = buffer.offset_to_position(offset);
            (start_position, position)
        });

        enum DefinitionOrReferece {
            Location(EditorLocation),
            References(Vec<Location>),
        }

        let internal_command = self.common.internal_command;
        let cursor = self.cursor.read_only();
        let send = create_ext_action(self.scope, move |d| {
            let current_offset = cursor.with_untracked(|c| c.offset());
            if current_offset != offset {
                return;
            }

            match d {
                DefinitionOrReferece::Location(location) => {
                    internal_command
                        .send(InternalCommand::JumpToLocation { location });
                }
                DefinitionOrReferece::References(locations) => {
                    internal_command.send(InternalCommand::PaletteReferences {
                        references: locations
                            .into_iter()
                            .map(|l| EditorLocation {
                                path: path_from_url(&l.uri),
                                position: Some(EditorPosition::Position(
                                    l.range.start,
                                )),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            })
                            .collect(),
                    });
                }
            }
        });
        let proxy = self.common.proxy.clone();
        self.common.proxy.get_definition(
            offset,
            path.clone(),
            position,
            move |result| {
                if let Ok(ProxyResponse::GetDefinitionResponse {
                    definition, ..
                }) = result
                {
                    if let Some(location) = match definition {
                        GotoDefinitionResponse::Scalar(location) => Some(location),
                        GotoDefinitionResponse::Array(locations) => {
                            if !locations.is_empty() {
                                Some(locations[0].clone())
                            } else {
                                None
                            }
                        }
                        GotoDefinitionResponse::Link(location_links) => {
                            let location_link = location_links[0].clone();
                            Some(Location {
                                uri: location_link.target_uri,
                                range: location_link.target_selection_range,
                            })
                        }
                    } {
                        if location.range.start == start_position {
                            proxy.get_references(
                                path.clone(),
                                position,
                                move |result| {
                                    if let Ok(
                                        ProxyResponse::GetReferencesResponse {
                                            references,
                                        },
                                    ) = result
                                    {
                                        if references.is_empty() {
                                            return;
                                        }
                                        if references.len() == 1 {
                                            let location = &references[0];
                                            send(DefinitionOrReferece::Location(
                                                EditorLocation {
                                                    path: path_from_url(
                                                        &location.uri,
                                                    ),
                                                    position: Some(
                                                        EditorPosition::Position(
                                                            location.range.start,
                                                        ),
                                                    ),
                                                    scroll_offset: None,
                                                    ignore_unconfirmed: false,
                                                    same_editor_tab: false,
                                                },
                                            ));
                                        } else {
                                            send(DefinitionOrReferece::References(
                                                references,
                                            ));
                                        }
                                    }
                                },
                            );
                        } else {
                            let path = path_from_url(&location.uri);
                            send(DefinitionOrReferece::Location(EditorLocation {
                                path,
                                position: Some(EditorPosition::Position(
                                    location.range.start,
                                )),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            }));
                        }
                    }
                }
            },
        );
    }

    fn page_move(&self, down: bool, mods: ModifiersState) {
        let config = self.common.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let lines = (viewport.height() / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.scroll_delta
            .set(Vec2::new(0.0, if down { distance } else { -distance }));
        self.run_move_command(
            if down {
                &lapce_core::movement::Movement::Down
            } else {
                &lapce_core::movement::Movement::Up
            },
            Some(lines),
            mods,
        );
    }

    fn scroll(&self, down: bool, count: usize, mods: ModifiersState) {
        let config = self.common.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.cursor.with_untracked(|cursor| cursor.offset());
        let (line, _col) = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.offset_to_line_col(offset));
        let top = viewport.y0 + diff + self.sticky_header_height.get_untracked();
        let bottom = viewport.y0 + diff + viewport.height();

        let new_line = if (line + 1) as f64 * line_height + line_height > bottom {
            let line = (bottom / line_height).floor() as usize;
            if line > 2 {
                line - 2
            } else {
                0
            }
        } else if line as f64 * line_height - line_height < top {
            let line = (top / line_height).ceil() as usize;
            line + 1
        } else {
            line
        };

        self.scroll_delta.set(Vec2::new(0.0, diff));

        match new_line.cmp(&line) {
            Ordering::Greater => {
                self.run_move_command(
                    &lapce_core::movement::Movement::Down,
                    Some(new_line - line),
                    mods,
                );
            }
            Ordering::Less => {
                self.run_move_command(
                    &lapce_core::movement::Movement::Up,
                    Some(line - new_line),
                    mods,
                );
            }
            _ => (),
        };
    }

    fn select_inline_completion(&self) {
        if self
            .common
            .inline_completion
            .with_untracked(|c| c.status == InlineCompletionStatus::Inactive)
        {
            return;
        }

        let data = self
            .common
            .inline_completion
            .with_untracked(|c| (c.current_item().cloned(), c.start_offset));
        self.cancel_inline_completion();

        let (Some(item), start_offset) = data else {
            return;
        };

        let _ = item.apply(self, start_offset);
    }

    fn next_inline_completion(&self) {
        if self
            .common
            .inline_completion
            .with_untracked(|c| c.status == InlineCompletionStatus::Inactive)
        {
            return;
        }

        self.common.inline_completion.update(|c| {
            c.next();
        });
    }

    fn previous_inline_completion(&self) {
        if self
            .common
            .inline_completion
            .with_untracked(|c| c.status == InlineCompletionStatus::Inactive)
        {
            return;
        }

        self.common.inline_completion.update(|c| {
            c.previous();
        });
    }

    fn cancel_inline_completion(&self) {
        if self
            .common
            .inline_completion
            .with_untracked(|c| c.status == InlineCompletionStatus::Inactive)
        {
            return;
        }

        self.common.inline_completion.update(|c| {
            c.cancel();
        });

        self.view.doc.get_untracked().clear_inline_completion();
    }

    /// Update the current inline completion
    fn update_inline_completion(&self, trigger_kind: InlineCompletionTriggerKind) {
        if self.get_mode() != Mode::Insert {
            self.cancel_inline_completion();
            return;
        }

        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let line = doc
            .buffer
            .with_untracked(|buffer| buffer.line_of_offset(offset));
        let position = doc
            .buffer
            .with_untracked(|buffer| buffer.offset_to_position(offset));

        let inline_completion = self.common.inline_completion;
        let doc = self.view.doc.get_untracked();

        // Update the inline completion's text if it's already active to avoid flickering
        let has_relevant = inline_completion.with_untracked(|completion| {
            let c_line = doc.buffer.with_untracked(|buffer| {
                buffer.line_of_offset(completion.start_offset)
            });
            completion.status != InlineCompletionStatus::Inactive
                && line == c_line
                && completion.path == path
        });
        if has_relevant {
            let config = self.common.config.get_untracked();
            inline_completion.update(|completion| {
                completion.update_inline_completion(&config, &doc, offset);
            });
        }

        let path2 = path.clone();
        let send = create_ext_action(
            self.scope,
            move |items: Vec<lsp_types::InlineCompletionItem>| {
                let items = doc.buffer.with_untracked(|buffer| {
                    items
                        .into_iter()
                        .map(|item| InlineCompletionItem::from_lsp(buffer, item))
                        .collect()
                });
                inline_completion.update(|c| {
                    c.set_items(items, offset, path2);
                    c.update_doc(&doc, offset);
                });
            },
        );

        inline_completion.update(|c| c.status = InlineCompletionStatus::Started);

        self.common.proxy.get_inline_completions(
            path,
            position,
            trigger_kind,
            move |res| {
                if let Ok(ProxyResponse::GetInlineCompletions {
                    completions: items,
                }) = res
                {
                    let items = match items {
                        lsp_types::InlineCompletionResponse::Array(items) => items,
                        // Currently does not have any relevant extra fields
                        lsp_types::InlineCompletionResponse::List(items) => {
                            items.items
                        }
                    };
                    send(items);
                }
            },
        );
    }

    /// Check if there are inline completions that are being rendered
    fn has_inline_completions(&self) -> bool {
        self.common.inline_completion.with_untracked(|completion| {
            completion.status != InlineCompletionStatus::Inactive
                && !completion.items.is_empty()
        })
    }

    fn select_completion(&self) {
        let item = self
            .common
            .completion
            .with_untracked(|c| c.current_item().cloned());
        self.cancel_completion();
        let doc = self.view.doc.get_untracked();
        if let Some(item) = item {
            if item.item.data.is_some() {
                let editor = self.clone();
                let rev = doc.buffer.with_untracked(|buffer| buffer.rev());
                let path = doc.content.with_untracked(|c| c.path().cloned());
                let offset = self.cursor.with_untracked(|c| c.offset());
                let buffer = doc.buffer;
                let content = doc.content;
                let send = create_ext_action(self.scope, move |item| {
                    if editor.cursor.with_untracked(|c| c.offset() != offset) {
                        return;
                    }
                    if buffer.with_untracked(|b| b.rev()) != rev
                        || content.with_untracked(|content| {
                            content.path() != path.as_ref()
                        })
                    {
                        return;
                    }
                    let _ = editor.apply_completion_item(&item);
                });
                self.common.proxy.completion_resolve(
                    item.plugin_id,
                    item.item.clone(),
                    move |result| {
                        let item =
                            if let Ok(ProxyResponse::CompletionResolveResponse {
                                item,
                            }) = result
                            {
                                *item
                            } else {
                                item.item.clone()
                            };
                        send(item);
                    },
                );
            } else {
                let _ = self.apply_completion_item(&item.item);
            }
        }
    }

    pub fn cancel_completion(&self) {
        if self.common.completion.with_untracked(|c| c.status)
            == CompletionStatus::Inactive
        {
            return;
        }
        self.common.completion.update(|c| {
            c.cancel();
        });

        self.view
            .doc
            .with_untracked(|doc| doc.clear_completion_lens());
    }

    /// Update the displayed autocompletion box
    /// Sends a request to the LSP for completion information
    fn update_completion(&self, display_if_empty_input: bool) {
        if self.get_mode() != Mode::Insert {
            self.cancel_completion();
            return;
        }

        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let (start_offset, input, char) = doc.buffer.with_untracked(|buffer| {
            let start_offset = buffer.prev_code_boundary(offset);
            let end_offset = buffer.next_code_boundary(offset);
            let input = buffer.slice_to_cow(start_offset..end_offset).to_string();
            let char = if start_offset == 0 {
                "".to_string()
            } else {
                buffer
                    .slice_to_cow(start_offset - 1..start_offset)
                    .to_string()
            };
            (start_offset, input, char)
        });
        if !display_if_empty_input && input.is_empty() && char != "." && char != ":"
        {
            self.cancel_completion();
            return;
        }

        if self.common.completion.with_untracked(|completion| {
            completion.status != CompletionStatus::Inactive
                && completion.offset == start_offset
                && completion.path == path
        }) {
            self.common.completion.update(|completion| {
                completion.update_input(input.clone());

                if !completion.input_items.contains_key("") {
                    let start_pos = doc.buffer.with_untracked(|buffer| {
                        buffer.offset_to_position(start_offset)
                    });
                    completion.request(
                        self.editor_id,
                        &self.common.proxy,
                        path.clone(),
                        "".to_string(),
                        start_pos,
                    );
                }

                if !completion.input_items.contains_key(&input) {
                    let position = doc
                        .buffer
                        .with_untracked(|buffer| buffer.offset_to_position(offset));
                    completion.request(
                        self.editor_id,
                        &self.common.proxy,
                        path,
                        input,
                        position,
                    );
                }
            });
            let cursor_offset = self.cursor.with_untracked(|c| c.offset());
            self.common
                .completion
                .get_untracked()
                .update_document_completion(&self.view, cursor_offset);

            return;
        }

        let doc = self.view.doc.get_untracked();
        self.common.completion.update(|completion| {
            completion.path = path.clone();
            completion.offset = start_offset;
            completion.input = input.clone();
            completion.status = CompletionStatus::Started;
            completion.input_items.clear();
            completion.request_id += 1;
            let start_pos = doc
                .buffer
                .with_untracked(|buffer| buffer.offset_to_position(start_offset));
            completion.request(
                self.editor_id,
                &self.common.proxy,
                path.clone(),
                "".to_string(),
                start_pos,
            );

            if !input.is_empty() {
                let position = doc
                    .buffer
                    .with_untracked(|buffer| buffer.offset_to_position(offset));
                completion.request(
                    self.editor_id,
                    &self.common.proxy,
                    path,
                    input,
                    position,
                );
            }
        });
    }

    /// Check if there are completions that are being rendered
    fn has_completions(&self) -> bool {
        self.common.completion.with_untracked(|completion| {
            completion.status != CompletionStatus::Inactive
                && !completion.filtered_items.is_empty()
        })
    }

    fn apply_completion_item(&self, item: &CompletionItem) -> Result<()> {
        let doc = self.view.doc.get_untracked();
        let buffer = doc.buffer.get_untracked();
        let cursor = self.cursor.get_untracked();
        // Get all the edits which would be applied in places other than right where the cursor is
        let additional_edit: Vec<_> = item
            .additional_text_edits
            .as_ref()
            .into_iter()
            .flatten()
            .map(|edit| {
                let selection = lapce_core::selection::Selection::region(
                    buffer.offset_of_position(&edit.range.start),
                    buffer.offset_of_position(&edit.range.end),
                );
                (selection, edit.new_text.as_str())
            })
            .collect::<Vec<(lapce_core::selection::Selection, &str)>>();

        let text_format = item
            .insert_text_format
            .unwrap_or(lsp_types::InsertTextFormat::PLAIN_TEXT);
        if let Some(edit) = &item.text_edit {
            match edit {
                CompletionTextEdit::Edit(edit) => {
                    let offset = cursor.offset();
                    let start_offset = buffer.prev_code_boundary(offset);
                    let end_offset = buffer.next_code_boundary(offset);
                    let edit_start = buffer.offset_of_position(&edit.range.start);
                    let edit_end = buffer.offset_of_position(&edit.range.end);

                    let selection = lapce_core::selection::Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PLAIN_TEXT => {
                            self.do_edit(
                                &selection,
                                &[
                                    &[(selection.clone(), edit.new_text.as_str())][..],
                                    &additional_edit[..],
                                ]
                                .concat(),
                            );
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::SNIPPET => {
                            self.completion_apply_snippet(
                                &edit.new_text,
                                &selection,
                                additional_edit,
                                start_offset,
                            )?;
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                CompletionTextEdit::InsertAndReplace(_) => (),
            }
        }

        let offset = cursor.offset();
        let start_offset = buffer.prev_code_boundary(offset);
        let end_offset = buffer.next_code_boundary(offset);
        let selection = Selection::region(start_offset, end_offset);

        self.do_edit(
            &selection,
            &[
                &[(
                    selection.clone(),
                    item.insert_text.as_deref().unwrap_or(item.label.as_str()),
                )][..],
                &additional_edit[..],
            ]
            .concat(),
        );
        Ok(())
    }

    pub fn completion_apply_snippet(
        &self,
        snippet: &str,
        selection: &Selection,
        additional_edit: Vec<(Selection, &str)>,
        start_offset: usize,
    ) -> Result<()> {
        let snippet = Snippet::from_str(snippet)?;
        let text = snippet.text();
        let mut cursor = self.cursor.get_untracked();
        let old_cursor = cursor.mode.clone();
        let (delta, inval_lines, edits) = self
            .view
            .doc
            .get_untracked()
            .do_raw_edit(
                &[
                    &[(selection.clone(), text.as_str())][..],
                    &additional_edit[..],
                ]
                .concat(),
                EditType::Completion,
            )
            .ok_or_else(|| anyhow::anyhow!("not edited"))?;

        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);

        let mut transformer = Transformer::new(&delta);
        let offset = transformer.transform(start_offset, false);
        let snippet_tabs = snippet.tabs(offset);

        let doc = self.view.doc.get_untracked();
        if snippet_tabs.is_empty() {
            doc.buffer.update(|buffer| {
                cursor.update_selection(buffer, selection);
                buffer.set_cursor_before(old_cursor);
                buffer.set_cursor_after(cursor.mode.clone());
            });
            self.cursor.set(cursor);
            self.apply_deltas(&[(delta, inval_lines, edits)]);
            return Ok(());
        }

        let mut selection = lapce_core::selection::Selection::new();
        let (_tab, (start, end)) = &snippet_tabs[0];
        let region = lapce_core::selection::SelRegion::new(*start, *end, None);
        selection.add_region(region);
        cursor.set_insert(selection);

        doc.buffer.update(|buffer| {
            buffer.set_cursor_before(old_cursor);
            buffer.set_cursor_after(cursor.mode.clone());
        });
        self.cursor.set(cursor);
        self.apply_deltas(&[(delta, inval_lines, edits)]);
        self.add_snippet_placeholders(snippet_tabs);
        Ok(())
    }

    fn add_snippet_placeholders(
        &self,
        new_placeholders: Vec<(usize, (usize, usize))>,
    ) {
        self.snippet.update(|snippet| {
            if snippet.is_none() {
                if new_placeholders.len() > 1 {
                    *snippet = Some(new_placeholders);
                }
                return;
            }

            let placeholders = snippet.as_mut().unwrap();

            let mut current = 0;
            let offset = self.cursor.get_untracked().offset();
            for (i, (_, (start, end))) in placeholders.iter().enumerate() {
                if *start <= offset && offset <= *end {
                    current = i;
                    break;
                }
            }

            let v = placeholders.split_off(current);
            placeholders.extend_from_slice(&new_placeholders);
            placeholders.extend_from_slice(&v[1..]);
        });
    }

    pub fn do_edit(
        &self,
        selection: &Selection,
        edits: &[(impl AsRef<Selection>, &str)],
    ) {
        let mut cursor = self.cursor.get_untracked();
        let doc = self.view.doc.get_untracked();
        let (delta, inval_lines, edits) =
            match doc.do_raw_edit(edits, EditType::Completion) {
                Some(e) => e,
                None => return,
            };
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        let old_cursor = cursor.mode.clone();
        doc.buffer.update(|buffer| {
            cursor.update_selection(buffer, selection);
            buffer.set_cursor_before(old_cursor);
            buffer.set_cursor_after(cursor.mode.clone());
        });
        self.cursor.set(cursor);

        self.apply_deltas(&[(delta, inval_lines, edits)]);
    }

    pub fn do_text_edit(&self, edits: &[TextEdit]) {
        let (selection, edits) = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| {
                let selection = self.cursor.get_untracked().edit_selection(buffer);
                let edits = edits
                    .iter()
                    .map(|edit| {
                        let selection = lapce_core::selection::Selection::region(
                            buffer.offset_of_position(&edit.range.start),
                            buffer.offset_of_position(&edit.range.end),
                        );
                        (selection, edit.new_text.as_str())
                    })
                    .collect::<Vec<_>>();
                (selection, edits)
            });

        self.do_edit(&selection, &edits);
    }

    fn apply_deltas(&self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        if !deltas.is_empty() && !self.confirmed.get_untracked() {
            self.confirmed.set(true);
        }
        for (delta, _, _) in deltas {
            // self.inactive_apply_delta(delta);
            self.update_snippet_offset(delta);
            // self.update_breakpoints(delta);
        }
        // self.update_signature();
    }

    fn update_snippet_offset(&self, delta: &RopeDelta) {
        if self.snippet.with_untracked(|s| s.is_some()) {
            self.snippet.update(|snippet| {
                let mut transformer = Transformer::new(delta);
                *snippet = Some(
                    snippet
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|(tab, (start, end))| {
                            (
                                *tab,
                                (
                                    transformer.transform(*start, false),
                                    transformer.transform(*end, true),
                                ),
                            )
                        })
                        .collect(),
                );
            });
        }
    }

    fn do_go_to_location(
        &self,
        location: EditorLocation,
        edits: Option<Vec<TextEdit>>,
    ) {
        if let Some(position) = location.position {
            self.go_to_position(position, location.scroll_offset, edits);
        } else if let Some(edits) = edits.as_ref() {
            self.do_text_edit(edits);
        } else {
            let db: Arc<LapceDb> = use_context().unwrap();
            if let Ok(info) = db.get_doc_info(&self.common.workspace, &location.path)
            {
                self.go_to_position(
                    EditorPosition::Offset(info.cursor_offset),
                    Some(Vec2::new(info.scroll_offset.0, info.scroll_offset.1)),
                    edits,
                );
            }
        }
    }

    pub fn go_to_location(
        &self,
        location: EditorLocation,
        new_doc: bool,
        edits: Option<Vec<TextEdit>>,
    ) {
        if !new_doc {
            self.do_go_to_location(location, edits);
        } else {
            let loaded = self.view.doc.with_untracked(|d| d.loaded);
            let editor = self.clone();
            self.scope.create_effect(move |prev_loaded| {
                if prev_loaded == Some(true) {
                    return true;
                }

                let loaded = loaded.get();
                if loaded {
                    editor.do_go_to_location(location.clone(), edits.clone());
                }
                loaded
            });
        }
    }

    pub fn go_to_position(
        &self,
        position: EditorPosition,
        scroll_offset: Option<Vec2>,
        edits: Option<Vec<TextEdit>>,
    ) {
        let offset = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| position.to_offset(buffer));
        let config = self.common.config.get_untracked();
        self.cursor.set(if config.core.modal {
            Cursor::new(CursorMode::Normal(offset), None, None)
        } else {
            Cursor::new(CursorMode::Insert(Selection::caret(offset)), None, None)
        });
        if let Some(scroll_offset) = scroll_offset {
            self.scroll_to.set(Some(scroll_offset));
        }
        if let Some(edits) = edits.as_ref() {
            self.do_text_edit(edits);
        }
    }

    pub fn get_code_actions(&self) {
        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let exists = doc
            .code_actions()
            .with_untracked(|c| c.contains_key(&offset));

        if exists {
            return;
        }

        // insert some empty data, so that we won't make the request again
        doc.code_actions().update(|c| {
            c.insert(offset, Arc::new((PluginId(0), Vec::new())));
        });

        let (position, rev, diagnostics) = doc.buffer.with_untracked(|buffer| {
            let position = buffer.offset_to_position(offset);
            let rev = doc.rev();

            // Get the diagnostics for the current line, which the LSP might use to inform
            // what code actions are available (such as fixes for the diagnostics).
            let diagnostics = doc
                .diagnostics()
                .diagnostics
                .get_untracked()
                .iter()
                .map(|x| &x.diagnostic)
                .filter(|x| {
                    x.range.start.line <= position.line
                        && x.range.end.line >= position.line
                })
                .cloned()
                .collect();

            (position, rev, diagnostics)
        });

        let send = create_ext_action(self.scope, move |resp| {
            if doc.rev() == rev {
                doc.code_actions().update(|c| {
                    c.insert(offset, Arc::new(resp));
                });
            }
        });

        self.common.proxy.get_code_actions(
            path,
            position,
            diagnostics,
            move |result| {
                if let Ok(ProxyResponse::GetCodeActionsResponse {
                    plugin_id,
                    resp,
                }) = result
                {
                    send((plugin_id, resp))
                }
            },
        );
    }

    pub fn show_code_actions(&self, mouse_click: bool) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let doc = self.view.doc.get_untracked();
        let code_actions = doc
            .code_actions()
            .with_untracked(|c| c.get(&offset).cloned());
        if let Some(code_actions) = code_actions {
            if !code_actions.1.is_empty() {
                self.common.internal_command.send(
                    InternalCommand::ShowCodeActions {
                        offset,
                        mouse_click,
                        code_actions,
                    },
                );
            }
        }
    }

    fn do_save(&self, after_action: impl Fn() + 'static) {
        self.view.doc.get_untracked().save(after_action);
    }

    pub fn save(
        &self,
        allow_formatting: bool,
        after_action: impl Fn() + 'static + Copy,
    ) {
        let doc = self.view.doc.get_untracked();
        let rev = doc.rev();
        let is_pristine = doc.is_pristine();
        let content = doc.content.get_untracked();

        if let DocContent::Scratch { .. } = &content {
            self.common
                .internal_command
                .send(InternalCommand::SaveScratchDoc { doc });
            return;
        }

        if content.path().is_some() && is_pristine {
            return;
        }

        let config = self.common.config.get_untracked();
        if let DocContent::File { path, .. } = content {
            let format_on_save = allow_formatting && config.editor.format_on_save;
            if format_on_save {
                let editor = self.clone();
                let send = create_ext_action(self.scope, move |result| {
                    if let Ok(Ok(ProxyResponse::GetDocumentFormatting { edits })) =
                        result
                    {
                        let current_rev =
                            editor.view.doc.with_untracked(|doc| doc.rev());
                        if current_rev == rev {
                            editor.do_text_edit(&edits);
                        }
                    }
                    editor.do_save(after_action);
                });

                let (tx, rx) = crossbeam_channel::bounded(1);
                let proxy = self.common.proxy.clone();
                std::thread::spawn(move || {
                    proxy.get_document_formatting(path, move |result| {
                        let _ = tx.send(result);
                    });
                    let result = rx.recv_timeout(std::time::Duration::from_secs(1));
                    send(result);
                });
            } else {
                self.do_save(after_action);
            }
        }
    }

    pub fn format(&self) {
        let doc = self.view.doc.get_untracked();
        let rev = doc.rev();
        let content = doc.content.get_untracked();

        if let DocContent::File { path, .. } = content {
            let editor = self.clone();
            let send = create_ext_action(self.scope, move |result| {
                if let Ok(Ok(ProxyResponse::GetDocumentFormatting { edits })) =
                    result
                {
                    let current_rev =
                        editor.view.doc.with_untracked(|doc| doc.rev());
                    if current_rev == rev {
                        editor.do_text_edit(&edits);
                    }
                }
            });

            let (tx, rx) = crossbeam_channel::bounded(1);
            let proxy = self.common.proxy.clone();
            std::thread::spawn(move || {
                proxy.get_document_formatting(path, move |result| {
                    let _ = tx.send(result);
                });
                let result = rx.recv_timeout(std::time::Duration::from_secs(1));
                send(result);
            });
        }
    }

    fn search_whole_word_forward(&self, mods: ModifiersState) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let (word, buffer) =
            self.view
                .doc
                .get_untracked()
                .buffer
                .with_untracked(|buffer| {
                    let (start, end) = buffer.select_word(offset);
                    (buffer.slice_to_cow(start..end).to_string(), buffer.clone())
                });
        self.common.internal_command.send(InternalCommand::Search {
            pattern: Some(word),
        });
        let next = self.common.find.next(buffer.text(), offset, false, true);

        if let Some((start, _end)) = next {
            self.run_move_command(
                &lapce_core::movement::Movement::Offset(start),
                None,
                mods,
            );
        }
    }

    fn search_forward(&self, mods: ModifiersState) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let text = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.text().clone());
        let next = self.common.find.next(&text, offset, false, true);

        if let Some((start, _end)) = next {
            self.run_move_command(
                &lapce_core::movement::Movement::Offset(start),
                None,
                mods,
            );
        }
    }

    fn search_backward(&self, mods: ModifiersState) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let text = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.text().clone());
        let next = self.common.find.next(&text, offset, true, true);

        if let Some((start, _end)) = next {
            self.run_move_command(
                &lapce_core::movement::Movement::Offset(start),
                None,
                mods,
            );
        }
    }

    fn replace_next(&self, text: &str) {
        let offset = self.cursor.with_untracked(|c| c.offset());
        let buffer = self
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.clone());
        let next = self.common.find.next(buffer.text(), offset, false, true);

        if let Some((start, end)) = next {
            let selection = Selection::region(start, end);
            self.do_edit(&selection, &[(selection.clone(), text)]);
        }
    }

    fn replace_all(&self, text: &str) {
        let offset = self.cursor.with_untracked(|c| c.offset());

        self.view.update_find();

        let edits: Vec<(Selection, &str)> = self
            .view
            .find_result()
            .occurrences
            .get_untracked()
            .regions()
            .iter()
            .map(|region| (Selection::region(region.start, region.end), text))
            .collect();
        if !edits.is_empty() {
            self.do_edit(&Selection::caret(offset), &edits);
        }
    }

    pub fn save_doc_position(&self) {
        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let cursor_offset = self.cursor.with_untracked(|c| c.offset());
        let scroll_offset = self.viewport.with_untracked(|v| v.origin().to_vec2());

        let db: Arc<LapceDb> = use_context().unwrap();
        db.save_doc_position(
            &self.common.workspace,
            path,
            cursor_offset,
            scroll_offset,
        );
    }

    fn rename(&self) {
        let doc = self.view.doc.get_untracked();
        let path = match if doc.loaded() {
            doc.content.with_untracked(|c| c.path().cloned())
        } else {
            None
        } {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let (position, rev) = doc
            .buffer
            .with_untracked(|buffer| (buffer.offset_to_position(offset), doc.rev()));

        let cursor = self.cursor;
        let buffer = doc.buffer;
        let internal_command = self.common.internal_command;
        let local_path = path.clone();
        let send = create_ext_action(self.scope, move |result| {
            if let Ok(ProxyResponse::PrepareRename { resp }) = result {
                if buffer.with_untracked(|buffer| buffer.rev()) != rev {
                    return;
                }

                if cursor.with_untracked(|c| c.offset()) != offset {
                    return;
                }

                let (start, _end, position, placeholder) =
                    buffer.with_untracked(|buffer| match resp {
                        lsp_types::PrepareRenameResponse::Range(range) => (
                            buffer.offset_of_position(&range.start),
                            buffer.offset_of_position(&range.end),
                            range.start,
                            None,
                        ),
                        lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                            range,
                            placeholder,
                        } => (
                            buffer.offset_of_position(&range.start),
                            buffer.offset_of_position(&range.end),
                            range.start,
                            Some(placeholder),
                        ),
                        lsp_types::PrepareRenameResponse::DefaultBehavior {
                            ..
                        } => {
                            let start = buffer.prev_code_boundary(offset);
                            let position = buffer.offset_to_position(start);
                            (
                                start,
                                buffer.next_code_boundary(offset),
                                position,
                                None,
                            )
                        }
                    });
                let placeholder = placeholder.unwrap_or_else(|| {
                    buffer.with_untracked(|buffer| {
                        let (start, end) = buffer.select_word(offset);
                        buffer.slice_to_cow(start..end).to_string()
                    })
                });
                internal_command.send(InternalCommand::StartRename {
                    path: local_path.clone(),
                    placeholder,
                    start,
                    position,
                });
            }
        });
        self.common
            .proxy
            .prepare_rename(path, position, move |result| {
                send(result);
            });
    }

    pub fn word_at_cursor(&self) -> String {
        let region = self.cursor.with_untracked(|c| match &c.mode {
            lapce_core::cursor::CursorMode::Normal(offset) => {
                lapce_core::selection::SelRegion::caret(*offset)
            }
            lapce_core::cursor::CursorMode::Visual {
                start,
                end,
                mode: _,
            } => lapce_core::selection::SelRegion::new(
                *start.min(end),
                self.view
                    .doc
                    .get_untracked()
                    .buffer
                    .with_untracked(|buffer| {
                        buffer.next_grapheme_offset(*start.max(end), 1, buffer.len())
                    }),
                None,
            ),
            lapce_core::cursor::CursorMode::Insert(selection) => {
                *selection.last_inserted().unwrap()
            }
        });

        if region.is_caret() {
            self.view
                .doc
                .get_untracked()
                .buffer
                .with_untracked(|buffer| {
                    let (start, end) = buffer.select_word(region.start);
                    buffer.slice_to_cow(start..end).to_string()
                })
        } else {
            self.view
                .doc
                .get_untracked()
                .buffer
                .with_untracked(|buffer| {
                    buffer.slice_to_cow(region.min()..region.max()).to_string()
                })
        }
    }

    pub fn clear_search(&self) {
        self.common.find.visual.set(false);
        self.find_focus.set(false);
    }

    fn search(&self) {
        let pattern = self.word_at_cursor();

        let pattern = if pattern.contains('\n') || pattern.is_empty() {
            None
        } else {
            Some(pattern)
        };

        self.common
            .internal_command
            .send(InternalCommand::Search { pattern });
        self.common.find.visual.set(true);
        self.find_focus.set(true);
        self.common.find.replace_focus.set(false);
    }

    pub fn pointer_down(&self, pointer_event: &PointerInputEvent) {
        if let Some(editor_tab_id) = self.editor_tab_id.get_untracked() {
            self.common
                .internal_command
                .send(InternalCommand::FocusEditorTab { editor_tab_id });
        }
        if self
            .view
            .doc
            .get_untracked()
            .content
            .with_untracked(|content| !content.is_local())
        {
            self.common.focus.set(Focus::Workbench);
            self.find_focus.set(false);
        }
        match pointer_event.button {
            PointerButton::Primary => {
                self.active.set(true);
                self.left_click(pointer_event);
            }
            PointerButton::Secondary => {
                self.right_click(pointer_event);
            }
            _ => {}
        }
    }

    fn left_click(&self, pointer_event: &PointerInputEvent) {
        match pointer_event.count {
            1 => {
                self.single_click(pointer_event);
            }
            2 => {
                self.double_click(pointer_event);
            }
            3 => {
                self.triple_click(pointer_event);
            }
            _ => {}
        }
    }

    fn single_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (new_offset, _) = self.view.offset_of_point(mode, pointer_event.pos);
        self.cursor.update(|cursor| {
            cursor.set_offset(
                new_offset,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    fn double_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (mouse_offset, _) = self.view.offset_of_point(mode, pointer_event.pos);
        let (start, end) = self.view.select_word(mouse_offset);

        self.cursor.update(|cursor| {
            cursor.add_region(
                start,
                end,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    fn triple_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (mouse_offset, _) = self.view.offset_of_point(mode, pointer_event.pos);
        let line = self.view.line_of_offset(mouse_offset);
        let start = self.view.offset_of_line(line);
        let end = self.view.offset_of_line(line + 1);

        self.cursor.update(|cursor| {
            cursor.add_region(
                start,
                end,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    pub fn pointer_move(&self, pointer_event: &PointerMoveEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (offset, is_inside) = self.view.offset_of_point(mode, pointer_event.pos);
        if self.active.get_untracked()
            && self.cursor.with_untracked(|c| c.offset()) != offset
        {
            self.cursor.update(|cursor| {
                cursor.set_offset(offset, true, pointer_event.modifiers.alt_key())
            });
        }
        if self.common.hover.active.get_untracked() {
            let hover_editor_id = self.common.hover.editor_id.get_untracked();
            if hover_editor_id != self.editor_id {
                self.common.hover.active.set(false);
            } else {
                let current_offset = self.common.hover.offset.get_untracked();
                let start_offset = self
                    .view
                    .doc
                    .get_untracked()
                    .buffer
                    .with_untracked(|buffer| buffer.prev_code_boundary(offset));
                if current_offset != start_offset {
                    self.common.hover.active.set(false);
                }
            }
        }
        let hover_delay = self.common.config.get_untracked().editor.hover_delay;
        if hover_delay > 0 {
            if is_inside {
                let start_offset = self
                    .view
                    .doc
                    .get_untracked()
                    .buffer
                    .with_untracked(|buffer| buffer.prev_code_boundary(offset));

                let editor = self.clone();
                let mouse_hover_timer = self.common.mouse_hover_timer;
                let timer_token =
                    exec_after(Duration::from_millis(hover_delay), move |token| {
                        if mouse_hover_timer.try_get_untracked() == Some(token)
                            && editor.editor_tab_id.try_get_untracked().is_some()
                        {
                            editor.update_hover(start_offset);
                        }
                    });
                mouse_hover_timer.set(timer_token);
            } else {
                self.common.mouse_hover_timer.set(TimerToken::INVALID);
            }
        }
    }

    pub fn pointer_up(&self, _pointer_event: &PointerInputEvent) {
        self.active.set(false);
    }

    pub fn pointer_leave(&self) {
        self.common.mouse_hover_timer.set(TimerToken::INVALID);
    }

    fn right_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (offset, _) = self.view.offset_of_point(mode, pointer_event.pos);
        let doc = self.view.doc.get_untracked();
        let pointer_inside_selection = doc.buffer.with_untracked(|buffer| {
            self.cursor
                .with_untracked(|c| c.edit_selection(buffer).contains(offset))
        });
        if !pointer_inside_selection {
            // move cursor to pointer position if outside current selection
            self.single_click(pointer_event);
        }

        let is_file = doc.content.with_untracked(|content| content.is_file());
        let mut menu = Menu::new("");
        let cmds = if is_file {
            vec![
                Some(CommandKind::Focus(FocusCommand::GotoDefinition)),
                Some(CommandKind::Focus(FocusCommand::GotoTypeDefinition)),
                None,
                Some(CommandKind::Focus(FocusCommand::Rename)),
                None,
                Some(CommandKind::Edit(EditCommand::ClipboardCut)),
                Some(CommandKind::Edit(EditCommand::ClipboardCopy)),
                Some(CommandKind::Edit(EditCommand::ClipboardPaste)),
                None,
                Some(CommandKind::Workbench(
                    LapceWorkbenchCommand::PaletteCommand,
                )),
            ]
        } else {
            vec![
                Some(CommandKind::Edit(EditCommand::ClipboardCut)),
                Some(CommandKind::Edit(EditCommand::ClipboardCopy)),
                Some(CommandKind::Edit(EditCommand::ClipboardPaste)),
                None,
                Some(CommandKind::Workbench(
                    LapceWorkbenchCommand::PaletteCommand,
                )),
            ]
        };
        let lapce_command = self.common.lapce_command;
        for cmd in cmds {
            if let Some(cmd) = cmd {
                menu = menu.entry(
                    MenuItem::new(cmd.desc().unwrap_or_else(|| cmd.str())).action(
                        move || {
                            lapce_command.send(LapceCommand {
                                kind: cmd.clone(),
                                data: None,
                            })
                        },
                    ),
                );
            } else {
                menu = menu.separator();
            }
        }
        show_context_menu(menu, None);
    }

    fn update_hover(&self, offset: usize) {
        let doc = self.view.doc.get_untracked();
        let path = doc
            .content
            .with_untracked(|content| content.path().cloned());
        let position = doc
            .buffer
            .with_untracked(|buffer| buffer.offset_to_position(offset));
        let path = match path {
            Some(path) => path,
            None => return,
        };
        let config = self.common.config;
        let hover_data = self.common.hover.clone();
        let editor_id = self.editor_id;
        let send = create_ext_action(self.scope, move |resp| {
            if let Ok(ProxyResponse::HoverResponse { hover, .. }) = resp {
                let content = parse_hover_resp(hover, &config.get_untracked());
                hover_data.content.set(content);
                hover_data.offset.set(offset);
                hover_data.editor_id.set(editor_id);
                hover_data.active.set(true);
            }
        });
        self.common.proxy.get_hover(0, path, position, |resp| {
            send(resp);
        });
    }

    // reset the doc inside and move cursor back
    pub fn reset(&self) {
        let doc = self.view.doc.get_untracked();
        doc.reload(Rope::from(""), true);
        self.cursor
            .update(|cursor| cursor.set_offset(0, false, false));
    }

    /// Get the line information for lines on the screen.  
    pub fn screen_lines(&self) -> RwSignal<ScreenLines> {
        self.view.screen_lines
    }
}

impl KeyPressFocus for EditorData {
    fn get_mode(&self) -> Mode {
        if self.common.find.visual.get_untracked() && self.find_focus.get_untracked()
        {
            Mode::Insert
        } else {
            self.cursor.with_untracked(|c| c.get_mode())
        }
    }

    fn check_condition(&self, condition: Condition) -> bool {
        match condition {
            Condition::InputFocus => {
                self.common.find.visual.get_untracked()
                    && self.find_focus.get_untracked()
            }
            Condition::ListFocus => self.has_completions(),
            Condition::CompletionFocus => self.has_completions(),
            Condition::InlineCompletionVisible => self.has_inline_completions(),
            Condition::InSnippet => self.snippet.with_untracked(|s| s.is_some()),
            Condition::EditorFocus => self
                .view
                .doc
                .get_untracked()
                .content
                .with_untracked(|content| !content.is_local()),
            Condition::SearchFocus => {
                self.common.find.visual.get_untracked()
                    && self.find_focus.get_untracked()
                    && !self.common.find.replace_focus.get_untracked()
            }
            Condition::ReplaceFocus => {
                self.common.find.visual.get_untracked()
                    && self.find_focus.get_untracked()
                    && self.common.find.replace_focus.get_untracked()
            }
            Condition::SearchActive => {
                if self.common.config.get_untracked().core.modal
                    && self.cursor.with_untracked(|c| !c.is_normal())
                {
                    false
                } else {
                    self.common.find.visual.get_untracked()
                }
            }
            _ => false,
        }
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> crate::command::CommandExecuted {
        if self.common.find.visual.get_untracked() && self.find_focus.get_untracked()
        {
            match &command.kind {
                CommandKind::Edit(_)
                | CommandKind::Move(_)
                | CommandKind::MultiSelection(_) => {
                    if self.common.find.replace_focus.get_untracked() {
                        self.common.internal_command.send(
                            InternalCommand::ReplaceEditorCommand {
                                command: command.clone(),
                                count,
                                mods,
                            },
                        );
                    } else {
                        self.common.internal_command.send(
                            InternalCommand::FindEditorCommand {
                                command: command.clone(),
                                count,
                                mods,
                            },
                        );
                    }
                    return CommandExecuted::Yes;
                }
                _ => {}
            }
        }

        match &command.kind {
            crate::command::CommandKind::Workbench(_) => CommandExecuted::No,
            crate::command::CommandKind::Edit(cmd) => self.run_edit_command(cmd),
            crate::command::CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                self.run_move_command(&movement, count, mods)
            }
            crate::command::CommandKind::Focus(cmd) => {
                if self
                    .view
                    .doc
                    .get_untracked()
                    .content
                    .with_untracked(|content| content.is_local())
                {
                    return CommandExecuted::No;
                }
                self.run_focus_command(cmd, count, mods)
            }
            crate::command::CommandKind::MotionMode(cmd) => {
                self.run_motion_mode_command(cmd, count)
            }
            crate::command::CommandKind::MultiSelection(cmd) => {
                self.run_multi_selection_command(cmd)
            }
        }
    }

    fn expect_char(&self) -> bool {
        if self.common.find.visual.get_untracked() && self.find_focus.get_untracked()
        {
            false
        } else {
            self.inline_find.with_untracked(|f| f.is_some())
        }
    }

    fn receive_char(&self, c: &str) {
        if self.common.find.visual.get_untracked() && self.find_focus.get_untracked()
        {
            // find/relace editor receive char
            if self.common.find.replace_focus.get_untracked() {
                self.common.internal_command.send(
                    InternalCommand::ReplaceEditorReceiveChar { s: c.to_string() },
                );
            } else {
                self.common.internal_command.send(
                    InternalCommand::FindEditorReceiveChar { s: c.to_string() },
                );
            }
        } else {
            // normal editor receive char
            if self.get_mode() == Mode::Insert {
                let mut cursor = self.cursor.get_untracked();
                let deltas = self.view.doc.get_untracked().do_insert(&mut cursor, c);
                self.cursor.set(cursor);

                if !c
                    .chars()
                    .all(|c| c.is_whitespace() || c.is_ascii_whitespace())
                {
                    self.update_completion(false);
                } else {
                    self.cancel_completion();
                }

                self.update_inline_completion(
                    InlineCompletionTriggerKind::Automatic,
                );

                self.apply_deltas(&deltas);
            } else if let Some(direction) = self.inline_find.get_untracked() {
                self.inline_find(direction.clone(), c);
                self.last_inline_find.set(Some((direction, c.to_string())));
                self.inline_find.set(None);
            }
        }
    }
}

/// Checks if completion should be triggered if the received command
/// is one that inserts whitespace or deletes whitespace
fn show_completion(
    cmd: &EditCommand,
    doc: &Rope,
    deltas: &[(RopeDelta, InvalLines, SyntaxEdit)],
) -> bool {
    let show_completion = match cmd {
        EditCommand::DeleteBackward
        | EditCommand::DeleteForward
        | EditCommand::DeleteWordBackward
        | EditCommand::DeleteWordForward
        | EditCommand::DeleteForwardAndInsert => {
            let start = match deltas.first().and_then(|delta| delta.0.els.first()) {
                Some(lapce_xi_rope::DeltaElement::Copy(_, start)) => *start,
                _ => 0,
            };

            let end = match deltas.first().and_then(|delta| delta.0.els.get(1)) {
                Some(lapce_xi_rope::DeltaElement::Copy(end, _)) => *end,
                _ => 0,
            };

            if start > 0 && end > start {
                !doc.slice_to_cow(start..end)
                    .chars()
                    .all(|c| c.is_whitespace() || c.is_ascii_whitespace())
            } else {
                true
            }
        }
        _ => false,
    };

    show_completion
}

fn show_inline_completion(cmd: &EditCommand) -> bool {
    matches!(
        cmd,
        EditCommand::DeleteBackward
            | EditCommand::DeleteForward
            | EditCommand::DeleteWordBackward
            | EditCommand::DeleteWordForward
            | EditCommand::DeleteForwardAndInsert
            | EditCommand::IndentLine
            | EditCommand::InsertMode
    )
}

// TODO(minor): Should we just put this on view, since it only requires those values?
fn compute_screen_lines(
    config: ReadSignal<Arc<LapceConfig>>,
    base: RwSignal<ScreenLinesBase>,
    view_kind: ReadSignal<EditorViewKind>,
    doc: ReadSignal<Rc<Document>>,
    lines: &Lines,
    text_prov: impl TextLayoutProvider + Clone,
) -> ScreenLines {
    // TODO: this should probably be a get since we need to depend on line-height
    let config = config.get();
    let line_height = config.editor.line_height();

    let (y0, y1) = base
        .with_untracked(|base| (base.active_viewport.y0, base.active_viewport.y1));
    // Get the start and end (visual) lines that are visible in the viewport
    let min_vline = VLine((y0 / line_height as f64).floor() as usize);
    let max_vline = VLine((y1 / line_height as f64).ceil() as usize);

    let (cache_rev, content, loaded) =
        doc.with(|doc| (doc.cache_rev, doc.content, doc.loaded));

    cache_rev.track();
    // TODO(minor): we don't really need to depend on various subdetails that aren't affecting how
    // the screen lines are set up, like the title of a scratch document.
    content.track();
    loaded.track();

    let min_info = once_cell::sync::Lazy::new(|| {
        lines
            .iter_vlines(text_prov.clone(), false, min_vline)
            .next()
    });
    // TODO: if you need the max vline you probably need the min vline too and so you could grab
    // both in one iter call, which would be more efficient than two iterations
    let max_info = once_cell::sync::Lazy::new(|| {
        lines
            .iter_vlines(text_prov.clone(), false, max_vline)
            .next()
    });

    match view_kind.get() {
        EditorViewKind::Normal => {
            let mut rvlines = Vec::new();
            let mut info = HashMap::new();

            let Some(min_info) = *min_info else {
                return ScreenLines {
                    lines: Rc::new(rvlines),
                    info: Rc::new(info),
                    diff_sections: None,
                    base,
                };
            };

            // TODO: the original was min_line..max_line + 1, are we iterating too little now?
            // the iterator is from min_vline..max_vline
            let count = max_vline.get() - min_vline.get();
            let iter = lines
                .iter_rvlines_init(text_prov, config.id, min_info.rvline, false)
                .take(count);

            for (i, vline_info) in iter.enumerate() {
                rvlines.push(vline_info.rvline);

                let y_idx = min_vline.get() + i;
                let vline_y = y_idx * line_height;
                let line_y = vline_y - vline_info.rvline.line_index * line_height;

                // Add the information to make it cheap to get in the future.
                // This y positions are shifted by the baseline y0
                info.insert(
                    vline_info.rvline,
                    LineInfo {
                        y: line_y as f64 - y0,
                        vline_y: vline_y as f64 - y0,
                        vline_info,
                    },
                );
            }

            ScreenLines {
                lines: Rc::new(rvlines),
                info: Rc::new(info),
                diff_sections: None,
                base,
            }
        }
        EditorViewKind::Diff(diff_info) => {
            // TODO: let lines in diff view be wrapped, possibly screen_lines should be impl'd
            // on DiffEditorData

            let mut y_idx = 0;
            let mut rvlines = Vec::new();
            let mut info = HashMap::new();
            let mut diff_sections = Vec::new();
            let mut last_change: Option<&DiffLines> = None;
            let mut changes = diff_info.changes.iter().peekable();
            let is_right = diff_info.is_right;

            let line_y = |info: VLineInfo<()>, vline_y: usize| -> usize {
                vline_y - info.rvline.line_index * line_height
            };

            while let Some(change) = changes.next() {
                match (is_right, change) {
                    (true, DiffLines::Left(range)) => {
                        if let Some(DiffLines::Right(_)) = changes.peek() {
                        } else {
                            let len = range.len();
                            diff_sections.push(DiffSection {
                                y_idx,
                                height: len,
                                kind: DiffSectionKind::NoCode,
                            });
                            y_idx += len;
                        }
                    }
                    (false, DiffLines::Right(range)) => {
                        let len = if let Some(DiffLines::Left(r)) = last_change {
                            range.len() - r.len().min(range.len())
                        } else {
                            range.len()
                        };
                        if len > 0 {
                            diff_sections.push(DiffSection {
                                y_idx,
                                height: len,
                                kind: DiffSectionKind::NoCode,
                            });
                            y_idx += len;
                        }
                    }
                    (true, DiffLines::Right(range))
                    | (false, DiffLines::Left(range)) => {
                        // TODO: count vline count in the range instead
                        let height = range.len();

                        diff_sections.push(DiffSection {
                            y_idx,
                            height,
                            kind: if is_right {
                                DiffSectionKind::Added
                            } else {
                                DiffSectionKind::Removed
                            },
                        });

                        let initial_y_idx = y_idx;
                        // Mopve forward by the count given.
                        y_idx += height;

                        if y_idx < min_vline.get() {
                            if is_right {
                                if let Some(DiffLines::Left(r)) = last_change {
                                    // TODO: count vline count in the other editor since this is skipping an amount dependent on those vlines
                                    let len = r.len() - r.len().min(range.len());
                                    if len > 0 {
                                        diff_sections.push(DiffSection {
                                            y_idx,
                                            height: len,
                                            kind: DiffSectionKind::NoCode,
                                        });
                                        y_idx += len;
                                    }
                                };
                            }
                            last_change = Some(change);
                            continue;
                        }

                        let Some(min_info) = *min_info else {
                            // TODO(minor): What is the proper behavior here?
                            break;
                        };

                        let Some(max_info) = *max_info else {
                            // TODO(minor): What is the proper behavior here?
                            break;
                        };

                        let start_rvline =
                            lines.rvline_of_line(&text_prov, range.start);

                        // TODO: this wouldn't need to produce vlines if screen lines didn't
                        // require them.
                        let iter = lines
                            .iter_rvlines(&text_prov, false, start_rvline)
                            .take_while(|vline_info| {
                                vline_info.rvline.line < range.end
                            })
                            .enumerate();
                        for (i, rvline_info) in iter {
                            let rvline = rvline_info.rvline;
                            if rvline < min_info.rvline {
                                continue;
                            }

                            rvlines.push(rvline);
                            let vline_y = (initial_y_idx + i) * line_height;
                            info.insert(
                                rvline,
                                LineInfo {
                                    y: line_y(rvline_info, vline_y) as f64 - y0,
                                    vline_y: vline_y as f64 - y0,
                                    vline_info: rvline_info,
                                },
                            );

                            if rvline > max_info.rvline {
                                break;
                            }
                        }

                        if is_right {
                            if let Some(DiffLines::Left(r)) = last_change {
                                // TODO: count vline count in the other editor since this is skipping an amount dependent on those vlines
                                let len = r.len() - r.len().min(range.len());
                                if len > 0 {
                                    diff_sections.push(DiffSection {
                                        y_idx,
                                        height: len,
                                        kind: DiffSectionKind::NoCode,
                                    });
                                    y_idx += len;
                                }
                            };
                        }
                    }
                    (_, DiffLines::Both(bothinfo)) => {
                        let start = if is_right {
                            bothinfo.right.start
                        } else {
                            bothinfo.left.start
                        };
                        let len = bothinfo.right.len();
                        let diff_height = len
                            - bothinfo
                                .skip
                                .as_ref()
                                .map(|skip| skip.len().saturating_sub(1))
                                .unwrap_or(0);
                        if y_idx + diff_height < min_vline.get() {
                            y_idx += diff_height;
                            last_change = Some(change);
                            continue;
                        }

                        let start_rvline = lines.rvline_of_line(&text_prov, start);

                        let mut iter = lines
                            .iter_rvlines_init(
                                &text_prov,
                                config.id,
                                start_rvline,
                                false,
                            )
                            .take_while(|info| info.rvline.line < start + len);
                        while let Some(rvline_info) = iter.next() {
                            let line = rvline_info.rvline.line;

                            // Skip over the lines
                            if let Some(skip) = bothinfo.skip.as_ref() {
                                if Some(skip.start) == line.checked_sub(start) {
                                    y_idx += 1;
                                    // Skip by `skip` count, which is skip - 1 because we will
                                    // go to the next vline on the next iter
                                    let _ = iter.nth(skip.len().saturating_sub(1));
                                    continue;
                                }
                            }

                            // Add the vline if it is within view
                            if y_idx >= min_vline.get() {
                                rvlines.push(rvline_info.rvline);
                                let vline_y = y_idx * line_height;
                                info.insert(
                                    rvline_info.rvline,
                                    LineInfo {
                                        y: line_y(rvline_info, vline_y) as f64 - y0,
                                        vline_y: vline_y as f64 - y0,
                                        vline_info: rvline_info,
                                    },
                                );
                            }

                            y_idx += 1;

                            if y_idx - 1 > max_vline.get() {
                                break;
                            }
                        }
                    }
                }
                last_change = Some(change);
            }
            ScreenLines {
                lines: Rc::new(rvlines),
                info: Rc::new(info),
                diff_sections: Some(Rc::new(diff_sections)),
                base,
            }
        }
    }
}

fn parse_hover_resp(
    hover: lsp_types::Hover,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    match hover.contents {
        HoverContents::Scalar(text) => match text {
            MarkedString::String(text) => parse_markdown(&text, 1.5, config),
            MarkedString::LanguageString(code) => parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                1.5,
                config,
            ),
        },
        HoverContents::Array(array) => {
            let entries = array
                .into_iter()
                .map(|t| from_marked_string(t, config))
                .rev();

            // TODO: It'd be nice to avoid this vec
            itertools::Itertools::intersperse(
                entries,
                vec![MarkdownContent::Separator],
            )
            .flatten()
            .collect()
        }
        HoverContents::Markup(content) => match content.kind {
            MarkupKind::PlainText => from_plaintext(&content.value, 1.5, config),
            MarkupKind::Markdown => parse_markdown(&content.value, 1.5, config),
        },
    }
}
