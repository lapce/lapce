use std::{cmp::Ordering, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Result;
use floem::{
    app::AppContext,
    ext_event::create_ext_action,
    glazier::Modifiers,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, UntrackedGettableSignal, WriteSignal,
    },
};
use lapce_core::{
    buffer::InvalLines,
    command::{EditCommand, FocusCommand},
    cursor::{Cursor, CursorMode},
    editor::EditType,
    mode::Mode,
    register::Register,
    selection::{InsertDrift, Selection},
    syntax::edit::SyntaxEdit,
};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};
use lapce_xi_rope::{Rope, RopeDelta, Transformer};
use lsp_types::{CompletionItem, CompletionTextEdit};

use crate::{
    command::{CommandExecuted, InternalCommand},
    completion::{CompletionData, CompletionStatus},
    config::LapceConfig,
    doc::Document,
    editor_tab::EditorTabChild,
    id::{EditorId, EditorTabId},
    keypress::{condition::Condition, KeyPressFocus},
    main_split::{SplitDirection, SplitMoveDirection},
    snippet::Snippet,
};

#[derive(Clone)]
pub struct EditorData {
    pub editor_tab_id: Option<EditorTabId>,
    pub editor_id: EditorId,
    pub doc: RwSignal<Document>,
    pub cursor: RwSignal<Cursor>,
    register: RwSignal<Register>,
    completion: RwSignal<CompletionData>,
    internal_command: WriteSignal<Option<InternalCommand>>,
    pub window_origin: RwSignal<Point>,
    pub viewport: RwSignal<Rect>,
    pub gutter_viewport: RwSignal<Rect>,
    pub scroll: RwSignal<Vec2>,
    pub snippet: RwSignal<Option<Vec<(usize, (usize, usize))>>>,
    proxy: ProxyRpcHandler,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl EditorData {
    pub fn new(
        cx: AppContext,
        editor_tab_id: Option<EditorTabId>,
        editor_id: EditorId,
        doc: RwSignal<Document>,
        register: RwSignal<Register>,
        completion: RwSignal<CompletionData>,
        internal_command: WriteSignal<Option<InternalCommand>>,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None);
        let cursor = create_rw_signal(cx.scope, cursor);
        let scroll = create_rw_signal(cx.scope, Vec2::ZERO);
        let snippet = create_rw_signal(cx.scope, None);
        let window_origin = create_rw_signal(cx.scope, Point::ZERO);
        let viewport = create_rw_signal(cx.scope, Rect::ZERO);
        let gutter_viewport = create_rw_signal(cx.scope, Rect::ZERO);
        Self {
            editor_tab_id,
            editor_id,
            doc,
            cursor,
            snippet,
            register,
            completion,
            internal_command,
            window_origin,
            viewport,
            gutter_viewport,
            scroll,
            proxy,
            config,
        }
    }

    pub fn new_local(
        cx: AppContext,
        editor_id: EditorId,
        register: RwSignal<Register>,
        completion: RwSignal<CompletionData>,
        internal_command: WriteSignal<Option<InternalCommand>>,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let doc = Document::new_local(cx, proxy.clone(), config);
        let doc = create_rw_signal(cx.scope, doc);
        Self::new(
            cx,
            None,
            editor_id,
            doc,
            register,
            completion,
            internal_command,
            proxy,
            config,
        )
    }

    pub fn copy(
        &self,
        cx: AppContext,
        editor_tab_id: Option<EditorTabId>,
        editor_id: EditorId,
    ) -> Self {
        let mut editor = self.clone();
        editor.cursor = create_rw_signal(cx.scope, editor.cursor.get_untracked());
        editor.viewport =
            create_rw_signal(cx.scope, editor.viewport.get_untracked());
        editor.gutter_viewport =
            create_rw_signal(cx.scope, editor.gutter_viewport.get_untracked());
        editor.scroll = create_rw_signal(cx.scope, Vec2::ZERO);
        editor.window_origin = create_rw_signal(cx.scope, Point::ZERO);
        editor.editor_tab_id = editor_tab_id;
        editor.editor_id = editor_id;
        editor
    }

    fn run_edit_command(
        &self,
        cx: AppContext,
        cmd: &EditCommand,
    ) -> CommandExecuted {
        let modal = self.config.with_untracked(|config| config.core.modal)
            && !self.doc.with_untracked(|doc| doc.content.is_local());
        let doc_before_edit =
            self.doc.with_untracked(|doc| doc.buffer().text().clone());
        let mut cursor = self.cursor.get_untracked();
        let mut register = self.register.get_untracked();

        let yank_data =
            if let lapce_core::cursor::CursorMode::Visual { .. } = &cursor.mode {
                Some(self.doc.with_untracked(|doc| cursor.yank(doc.buffer())))
            } else {
                None
            };

        let deltas = self
            .doc
            .update_returning(|doc| {
                doc.do_edit(&mut cursor, cmd, modal, &mut register)
            })
            .unwrap();

        if !deltas.is_empty() {
            if let Some(data) = yank_data {
                register.add_delete(data);
            }
        }

        self.cursor.set(cursor);
        self.register.set(register);

        if show_completion(cmd, &doc_before_edit, &deltas) {
            self.update_completion(false);
        } else {
            self.cancel_completion();
        }
        self.apply_deltas(&deltas);
        if let EditCommand::NormalMode = cmd {
            self.snippet.set(None);
        }

        CommandExecuted::Yes
    }

    fn run_move_command(
        &self,
        cx: AppContext,
        movement: &lapce_core::movement::Movement,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        let mut cursor = self.cursor.get_untracked();
        let config = self.config.get_untracked();
        self.doc.update(|doc| {
            self.register.update(|register| {
                doc.move_cursor(
                    &mut cursor,
                    movement,
                    count.unwrap_or(1),
                    mods.shift(),
                    register,
                    &config,
                );
            });
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

    fn run_focus_command(
        &self,
        cx: AppContext,
        cmd: &FocusCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match cmd {
            FocusCommand::SplitVertical => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::Split {
                        direction: SplitDirection::Vertical,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitHorizontal => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::Split {
                        direction: SplitDirection::Horizontal,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitRight => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Right,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitLeft => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Left,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitUp => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Up,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitDown => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Down,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitExchange => {
                println!("split exchage");
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command
                        .set(Some(InternalCommand::SplitExchange { editor_tab_id }));
                }
            }
            FocusCommand::SplitClose => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(
                        InternalCommand::EditorTabChildClose {
                            editor_tab_id,
                            child: EditorTabChild::Editor(self.editor_id),
                        },
                    ));
                }
            }
            FocusCommand::PageUp => {
                self.page_move(cx, false, mods);
            }
            FocusCommand::PageDown => {
                self.page_move(cx, true, mods);
            }
            FocusCommand::ScrollUp => {
                self.scroll(cx, false, count.unwrap_or(1), mods);
            }
            FocusCommand::ScrollDown => {
                self.scroll(cx, true, count.unwrap_or(1), mods);
            }
            FocusCommand::ListNext => {
                self.completion.update(|c| {
                    c.next();
                });
            }
            FocusCommand::ListPrevious => {
                self.completion.update(|c| {
                    c.previous();
                });
            }
            FocusCommand::ListNextPage => {
                self.completion.update(|c| {
                    c.next_page();
                });
            }
            FocusCommand::ListPreviousPage => {
                self.completion.update(|c| {
                    c.previous_page();
                });
            }
            FocusCommand::ListSelect => {
                let item = self
                    .completion
                    .with_untracked(|c| c.current_item().cloned());
                self.cancel_completion();
                if let Some(item) = item {
                    if item.item.data.is_some() {
                        let editor = self.clone();
                        let (rev, path) = self.doc.with_untracked(|doc| {
                            (doc.rev(), doc.content.path().cloned())
                        });
                        let offset = self.cursor.with_untracked(|c| c.offset());
                        let send = create_ext_action(cx, move |item| {
                            if editor.cursor.with_untracked(|c| c.offset() != offset)
                            {
                                return;
                            }
                            if editor.doc.with_untracked(|doc| {
                                doc.rev() != rev
                                    || doc.content.path() != path.as_ref()
                            }) {
                                return;
                            }
                            let _ = editor.apply_completion_item(&item);
                        });
                        self.proxy.completion_resolve(
                            item.plugin_id,
                            item.item.clone(),
                            move |result| {
                                let item = if let Ok(
                                    ProxyResponse::CompletionResolveResponse {
                                        item,
                                    },
                                ) = result
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
                        }
                    }
                });
            }
            _ => {}
        }
        CommandExecuted::Yes
    }

    fn page_move(&self, cx: AppContext, down: bool, mods: Modifiers) {
        let config = self.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let lines = (viewport.height() / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.scroll
            .set(Vec2::new(0.0, if down { distance } else { -distance }));
        self.run_move_command(
            cx,
            if down {
                &lapce_core::movement::Movement::Down
            } else {
                &lapce_core::movement::Movement::Up
            },
            Some(lines),
            mods,
        );
    }

    fn scroll(&self, cx: AppContext, down: bool, count: usize, mods: Modifiers) {
        let config = self.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.cursor.with_untracked(|cursor| cursor.offset());
        let (line, _col) = self
            .doc
            .with_untracked(|doc| doc.buffer().offset_to_line_col(offset));
        let top = viewport.y0 + diff;
        let bottom = top + viewport.height();

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

        self.scroll.set(Vec2::new(0.0, diff));

        match new_line.cmp(&line) {
            Ordering::Greater => {
                self.run_move_command(
                    cx,
                    &lapce_core::movement::Movement::Down,
                    Some(new_line - line),
                    mods,
                );
            }
            Ordering::Less => {
                self.run_move_command(
                    cx,
                    &lapce_core::movement::Movement::Up,
                    Some(line - new_line),
                    mods,
                );
            }
            _ => (),
        };
    }

    pub fn cancel_completion(&self) {
        if self.completion.with_untracked(|c| c.status) == CompletionStatus::Inactive
        {
            return;
        }
        self.completion.update(|c| {
            c.cancel();
        });
    }

    /// Update the displayed autocompletion box
    /// Sends a request to the LSP for completion information
    fn update_completion(&self, display_if_empty_input: bool) {
        if self.get_mode() != Mode::Insert {
            self.cancel_completion();
            return;
        }

        let path = match self.doc.with_untracked(|doc| {
            if doc.loaded() {
                doc.content.path().cloned()
            } else {
                None
            }
        }) {
            Some(path) => path,
            None => return,
        };

        let offset = self.cursor.with_untracked(|c| c.offset());
        let (start_offset, input, char) = self.doc.with_untracked(|doc| {
            let start_offset = doc.buffer().prev_code_boundary(offset);
            let end_offset = doc.buffer().next_code_boundary(offset);
            let input = doc
                .buffer()
                .slice_to_cow(start_offset..end_offset)
                .to_string();
            let char = if start_offset == 0 {
                "".to_string()
            } else {
                doc.buffer()
                    .slice_to_cow(start_offset - 1..start_offset)
                    .to_string()
            };
            (start_offset, input, char)
        });
        if !display_if_empty_input && input.is_empty() && char != "." && char != ":"
        {
            self.completion.update(|c| {
                c.cancel();
            });
            return;
        }
        println!("offset {offset} start offset {start_offset} input {input}");

        if self.completion.with_untracked(|completion| {
            completion.status != CompletionStatus::Inactive
                && completion.offset == start_offset
                && completion.path == path
        }) {
            self.completion.update(|completion| {
                completion.update_input(input.clone());

                if !completion.input_items.contains_key("") {
                    let start_pos = self.doc.with_untracked(|doc| {
                        doc.buffer().offset_to_position(start_offset)
                    });
                    completion.request(
                        &self.proxy,
                        path.clone(),
                        "".to_string(),
                        start_pos,
                    );
                }

                if !completion.input_items.contains_key(&input) {
                    let position = self.doc.with_untracked(|doc| {
                        doc.buffer().offset_to_position(offset)
                    });
                    completion.request(&self.proxy, path, input, position);
                }
            });
            return;
        }

        self.completion.update(|completion| {
            println!("new completion");
            completion.path = path.clone();
            completion.offset = start_offset;
            completion.input = input.clone();
            completion.status = CompletionStatus::Started;
            completion.input_items.clear();
            completion.request_id += 1;
            let start_pos = self
                .doc
                .with_untracked(|doc| doc.buffer().offset_to_position(start_offset));
            completion.request(&self.proxy, path.clone(), "".to_string(), start_pos);

            if !input.is_empty() {
                let position = self
                    .doc
                    .with_untracked(|doc| doc.buffer().offset_to_position(offset));
                completion.request(&self.proxy, path, input, position);
            }
        });
    }

    /// Check if there are completions that are being rendered
    fn has_completions(&self) -> bool {
        self.completion.with_untracked(|completion| {
            completion.status != CompletionStatus::Inactive
        })
    }

    fn apply_completion_item(&self, item: &CompletionItem) -> Result<()> {
        let doc = self.doc.get_untracked();
        let cursor = self.cursor.get_untracked();
        // Get all the edits which would be applied in places other than right where the cursor is
        let additional_edit: Vec<_> = item
            .additional_text_edits
            .as_ref()
            .into_iter()
            .flatten()
            .map(|edit| {
                let selection = lapce_core::selection::Selection::region(
                    doc.buffer().offset_of_position(&edit.range.start),
                    doc.buffer().offset_of_position(&edit.range.end),
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
                    let start_offset = doc.buffer().prev_code_boundary(offset);
                    let end_offset = doc.buffer().next_code_boundary(offset);
                    let edit_start =
                        doc.buffer().offset_of_position(&edit.range.start);
                    let edit_end = doc.buffer().offset_of_position(&edit.range.end);

                    let selection = lapce_core::selection::Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PLAIN_TEXT => {
                            self.completion_do_edit(
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
                                edit_start,
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
        let start_offset = doc.buffer().prev_code_boundary(offset);
        let end_offset = doc.buffer().next_code_boundary(offset);
        let selection = Selection::region(start_offset, end_offset);

        self.completion_do_edit(
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

    fn completion_apply_snippet(
        &self,
        snippet: &str,
        selection: &Selection,
        additional_edit: Vec<(Selection, &str)>,
        start_offset: usize,
        edit_start: usize,
    ) -> Result<()> {
        let snippet = Snippet::from_str(snippet)?;
        let text = snippet.text();
        let mut cursor = self.cursor.get_untracked();
        let old_cursor = cursor.mode.clone();
        let (delta, inval_lines, edits) = self
            .doc
            .update_returning(|doc| {
                doc.do_raw_edit(
                    &[
                        &[(selection.clone(), text.as_str())][..],
                        &additional_edit[..],
                    ]
                    .concat(),
                    EditType::Completion,
                )
            })
            .unwrap();

        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);

        let start_offset = additional_edit
            .iter()
            .map(|(selection, _)| selection.min_offset())
            .min()
            .map(|offset| offset.min(start_offset).min(edit_start))
            .unwrap_or(start_offset);

        let mut transformer = Transformer::new(&delta);
        let offset = transformer.transform(start_offset, false);
        let snippet_tabs = snippet.tabs(offset);

        if snippet_tabs.is_empty() {
            self.doc.update(|doc| {
                cursor.update_selection(doc.buffer(), selection);
                doc.buffer_mut().set_cursor_before(old_cursor);
                doc.buffer_mut().set_cursor_after(cursor.mode.clone());
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

        self.doc.update(|doc| {
            doc.buffer_mut().set_cursor_before(old_cursor);
            doc.buffer_mut().set_cursor_after(cursor.mode.clone());
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

    fn completion_do_edit(
        &self,
        selection: &Selection,
        edits: &[(impl AsRef<Selection>, &str)],
    ) {
        let mut cursor = self.cursor.get_untracked();
        let (delta, inval_lines, edits) = self
            .doc
            .update_returning(|doc| {
                let (delta, inval_lines, edits) =
                    doc.do_raw_edit(edits, EditType::Completion);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                let old_cursor = cursor.mode.clone();
                cursor.update_selection(doc.buffer(), selection);
                doc.buffer_mut().set_cursor_before(old_cursor);
                doc.buffer_mut().set_cursor_after(cursor.mode.clone());
                (delta, inval_lines, edits)
            })
            .unwrap();
        self.cursor.set(cursor);

        self.apply_deltas(&[(delta, inval_lines, edits)]);
    }

    fn apply_deltas(&self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
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
}

impl KeyPressFocus for EditorData {
    fn get_mode(&self) -> lapce_core::mode::Mode {
        self.cursor.with_untracked(|c| c.get_mode())
    }

    fn check_condition(
        &self,
        condition: crate::keypress::condition::Condition,
    ) -> bool {
        match condition {
            Condition::ListFocus => self.has_completions(),
            Condition::CompletionFocus => self.has_completions(),
            Condition::InSnippet => self.snippet.with_untracked(|s| s.is_some()),
            _ => false,
        }
    }

    fn run_command(
        &self,
        cx: AppContext,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            crate::command::CommandKind::Workbench(_) => CommandExecuted::No,
            crate::command::CommandKind::Edit(cmd) => self.run_edit_command(cx, cmd),
            crate::command::CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                self.run_move_command(cx, &movement, count, mods)
            }
            crate::command::CommandKind::Focus(cmd) => {
                self.run_focus_command(cx, cmd, count, mods)
            }
            crate::command::CommandKind::MotionMode(_) => CommandExecuted::No,
            crate::command::CommandKind::MultiSelection(_) => CommandExecuted::No,
        }
    }

    fn receive_char(&self, cx: AppContext, c: &str) {
        if self.get_mode() == Mode::Insert {
            let mut cursor = self.cursor.get_untracked();
            let config = self.config.get_untracked();
            let deltas = self
                .doc
                .update_returning(|doc| doc.do_insert(&mut cursor, c, &config))
                .unwrap();
            self.cursor.set(cursor);

            if !c
                .chars()
                .all(|c| c.is_whitespace() || c.is_ascii_whitespace())
            {
                self.update_completion(false);
            } else {
                self.cancel_completion();
            }
            self.apply_deltas(&deltas);
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
            let start = match deltas.get(0).and_then(|delta| delta.0.els.get(0)) {
                Some(lapce_xi_rope::DeltaElement::Copy(_, start)) => *start,
                _ => 0,
            };

            let end = match deltas.get(0).and_then(|delta| delta.0.els.get(1)) {
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
