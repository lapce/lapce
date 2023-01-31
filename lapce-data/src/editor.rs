use std::{
    cmp::Ordering,
    collections::HashMap,
    iter::Iterator,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{self, bounded};
use druid::{
    piet::{PietText, PietTextLayout, Svg},
    Color, Command, Env, EventCtx, ExtEventSink, FileDialogOptions, Modifiers,
    MouseEvent, Point, Rect, Target, Vec2, WidgetId,
};
use indexmap::IndexMap;
pub use lapce_core::syntax::Syntax;
use lapce_core::{
    buffer::{Buffer, DiffLines, InvalLines},
    command::{EditCommand, FocusCommand, MotionModeCommand, MultiSelectionCommand},
    editor::EditType,
    mode::{Mode, MotionMode},
    selection::{InsertDrift, Selection},
    syntax::edit::SyntaxEdit,
};
use lapce_rpc::{plugin::PluginId, proxy::ProxyResponse};
use lapce_xi_rope::{Rope, RopeDelta, Transformer};
use lsp_types::{
    request::GotoTypeDefinitionResponse, CodeAction, CodeActionOrCommand,
    CodeActionResponse, CompletionItem, CompletionTextEdit, DiagnosticSeverity,
    DocumentChangeOperation, DocumentChanges, GotoDefinitionResponse, Location,
    OneOf, Position, ResourceOp, TextEdit, Url, WorkspaceEdit,
};

use crate::{
    command::{
        CommandExecuted, CommandKind, EnsureVisiblePosition, InitBufferContent,
        InitBufferContentCb, LapceCommand, LapceUICommand, LAPCE_COMMAND,
        LAPCE_SAVE_FILE_AS, LAPCE_UI_COMMAND,
    },
    completion::{CompletionData, CompletionStatus, Snippet},
    config::LapceConfig,
    data::{
        EditorDiagnostic, EditorView, FocusArea, InlineFindDirection,
        LapceEditorData, LapceMainSplitData, SplitContent,
    },
    document::{BufferContent, Document, LocalBufferKind},
    find::Find,
    hover::{HoverData, HoverStatus},
    keypress::{KeyMap, KeyPressFocus},
    palette::PaletteData,
    proxy::{path_from_url, LapceProxy},
    rename::RenameData,
    selection_range::SelectionRangeDirection,
    signature::{SignatureData, SignatureStatus},
    source_control::SourceControlData,
    split::{SplitDirection, SplitMoveDirection},
};

pub struct LapceUI {}

#[derive(Copy, Clone)]
pub struct EditorCount(Option<usize>);

#[derive(Copy, Clone)]
pub enum EditorOperator {
    Delete(EditorCount),
    Yank(EditorCount),
}

pub trait EditorPosition: Sized {
    /// Convert the position to a utf8 offset
    fn to_utf8_offset(&self, buffer: &Buffer) -> usize;

    fn init_buffer_content_cmd(
        path: PathBuf,
        content: Rope,
        locations: Vec<(WidgetId, EditorLocation<Self>)>,
        edits: Option<Rope>,
        cb: Option<InitBufferContentCb>,
    ) -> LapceUICommand;
}

// Usize is always a utf8 offset
impl EditorPosition for usize {
    fn to_utf8_offset(&self, _buffer: &Buffer) -> usize {
        *self
    }

    fn init_buffer_content_cmd(
        path: PathBuf,
        content: Rope,
        locations: Vec<(WidgetId, EditorLocation<Self>)>,
        unsaved_buffers: Option<Rope>,
        cb: Option<InitBufferContentCb>,
    ) -> LapceUICommand {
        LapceUICommand::InitBufferContent(InitBufferContent {
            path,
            content,
            locations,
            edits: unsaved_buffers,
            cb,
        })
    }
}

/// Jump to first non blank character on a line
/// (If you want to jump to the very first character then use [`LineCol`] with column set to 0)
#[derive(Debug, Clone, Copy)]
pub struct Line(pub usize);

impl EditorPosition for Line {
    fn to_utf8_offset(&self, buffer: &Buffer) -> usize {
        buffer.first_non_blank_character_on_line(self.0.saturating_sub(1))
    }

    fn init_buffer_content_cmd(
        path: PathBuf,
        content: Rope,
        locations: Vec<(WidgetId, EditorLocation<Self>)>,
        edits: Option<Rope>,
        cb: Option<InitBufferContentCb>,
    ) -> LapceUICommand {
        LapceUICommand::InitBufferContentLine(InitBufferContent {
            path,
            content,
            locations,
            edits,
            cb,
        })
    }
}

/// UTF8 line and column-offset
#[derive(Debug, Clone, Copy)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}

impl EditorPosition for LineCol {
    fn to_utf8_offset(&self, buffer: &Buffer) -> usize {
        buffer.offset_of_line_col(self.line, self.column)
    }

    fn init_buffer_content_cmd(
        path: PathBuf,
        content: Rope,
        locations: Vec<(WidgetId, EditorLocation<Self>)>,
        edits: Option<Rope>,
        cb: Option<InitBufferContentCb>,
    ) -> LapceUICommand {
        LapceUICommand::InitBufferContentLineCol(InitBufferContent {
            path,
            content,
            locations,
            edits,
            cb,
        })
    }
}

impl EditorPosition for Position {
    fn to_utf8_offset(&self, buffer: &Buffer) -> usize {
        buffer.offset_of_position(self)
    }

    fn init_buffer_content_cmd(
        path: PathBuf,
        content: Rope,
        locations: Vec<(WidgetId, EditorLocation<Self>)>,
        edits: Option<Rope>,
        cb: Option<InitBufferContentCb>,
    ) -> LapceUICommand {
        LapceUICommand::InitBufferContentLsp(InitBufferContent {
            path,
            content,
            locations,
            edits,
            cb,
        })
    }
}

/// Used to specify a location with some path, and potentially position information.  
/// This is generic so that you can jump to utf8 offsets, line+column offsets, just the line,
/// or even utf16 offsets (such as those given by the LSP)
#[derive(Clone, Debug, PartialEq)]
pub struct EditorLocation<P: EditorPosition = usize> {
    pub path: PathBuf,
    pub position: Option<P>,
    pub scroll_offset: Option<Vec2>,
    /// Source control history name, ex: "head"
    pub history: Option<String>,
}

impl<P: EditorPosition> EditorLocation<P> {
    pub fn into_utf8_location(self, buffer: &Buffer) -> EditorLocation<usize> {
        EditorLocation {
            path: self.path,
            position: self.position.map(|p| p.to_utf8_offset(buffer)),
            scroll_offset: self.scroll_offset,
            history: self.history,
        }
    }
}

/// Temporary structure used to manipulate editors/buffers, before returning the data back to
/// [`LapceTabData`]. See [`LapceTabData::editor_view_content`] for more information on acquiring
/// the data and on returning it back.
pub struct LapceEditorBufferData {
    pub view_id: WidgetId,
    pub editor: Arc<LapceEditorData>,
    pub doc: Arc<Document>,
    // There are a variety of indirectly related data due to it being needed for the various utility
    // functions on the editor itself.
    pub completion: Arc<CompletionData>,
    pub signature: Arc<SignatureData>,
    pub hover: Arc<HoverData>,
    pub rename: Arc<RenameData>,
    pub main_split: LapceMainSplitData,
    pub focus_area: FocusArea,
    pub source_control: Arc<SourceControlData>,
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub proxy: Arc<LapceProxy>,
    pub command_keymaps: Arc<IndexMap<String, Vec<KeyMap>>>,
    pub config: Arc<LapceConfig>,
}

impl LapceEditorBufferData {
    fn doc_mut(&mut self) -> &mut Document {
        Arc::make_mut(&mut self.doc)
    }

    pub fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
        let cursor_offset = self.editor.cursor.offset();
        if self.doc.cursor_offset != cursor_offset
            || self.doc.scroll_offset != scroll_offset
        {
            let doc = self.doc_mut();
            doc.cursor_offset = cursor_offset;
            doc.scroll_offset = scroll_offset;
        }
    }

    /// Jump to the next/previous column on the line which matches the given text
    fn inline_find(
        &mut self,
        ctx: &mut EventCtx,
        direction: InlineFindDirection,
        c: &str,
    ) {
        let offset = self.editor.cursor.offset();
        let line = self.doc.buffer().line_of_offset(offset);
        let line_content = self.doc.buffer().line_content(line);
        let line_start_offset = self.doc.buffer().offset_of_line(line);
        let index = offset - line_start_offset;
        if let Some(new_index) = match direction {
            InlineFindDirection::Left => line_content[..index].rfind(c),
            InlineFindDirection::Right => {
                if index + 1 >= line_content.len() {
                    None
                } else {
                    let index = index
                        + self.doc.buffer().next_grapheme_offset(
                            offset,
                            1,
                            self.doc.buffer().offset_line_end(offset, false),
                        )
                        - offset;
                    line_content[index..].find(c).map(|i| i + index)
                }
            }
        } {
            self.run_move_command(
                ctx,
                &lapce_core::movement::Movement::Offset(
                    new_index + line_start_offset,
                ),
                None,
                Modifiers::empty(),
            );
        }
    }

    pub fn get_code_actions(&mut self, ctx: &mut EventCtx) {
        if !self.doc.loaded() {
            return;
        }

        if let BufferContent::File(path) = self.doc.content() {
            let path = path.clone();
            let offset = self.editor.cursor.offset();

            let exists = if self.doc.code_actions.contains_key(&offset) {
                true
            } else {
                Arc::make_mut(&mut self.doc)
                    .code_actions
                    .insert(offset, (PluginId(0), Vec::new()));
                false
            };
            if !exists {
                let position = self.doc.buffer().offset_to_position(offset);
                let rev = self.doc.rev();
                let event_sink = ctx.get_external_handle();

                // Get the diagnostics for the current line, which the LSP might use to inform
                // what code actions are available (such as fixes for the diagnostics).
                let diagnostics: &[EditorDiagnostic] = self
                    .doc
                    .diagnostics
                    .as_deref()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let diagnostics = diagnostics
                    .iter()
                    .map(|x| &x.diagnostic)
                    .filter(|x| {
                        x.range.start.line <= position.line
                            && x.range.end.line >= position.line
                    })
                    .cloned()
                    .collect();

                self.proxy.proxy_rpc.get_code_actions(
                    path.clone(),
                    position,
                    diagnostics,
                    move |result| {
                        if let Ok(ProxyResponse::GetCodeActionsResponse {
                            plugin_id,
                            resp,
                        }) = result
                        {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::UpdateCodeActions {
                                    path,
                                    plugin_id,
                                    rev,
                                    offset,
                                    resp,
                                },
                                Target::Auto,
                            );
                        } else {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::CodeActionsError {
                                    path,
                                    rev,
                                    offset,
                                },
                                Target::Auto,
                            );
                        }
                    },
                );
            }
        }
    }

    /// Update the positions of cursors in other editors which are editing the same document  
    /// Ex: You type at the start of the document, the cursor in the other editor (like a split)
    /// should be moved forward.
    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id
                && self.doc.content() == &editor.content
            {
                Arc::make_mut(editor).cursor.apply_delta(delta);
            }
        }
    }

    fn is_palette(&self) -> bool {
        self.editor.content == BufferContent::Local(LocalBufferKind::Palette)
    }

    fn is_rename(&self) -> bool {
        self.editor.content == BufferContent::Local(LocalBufferKind::Rename)
    }

    /// Check if there are completions that are being rendered
    fn has_completions(&self) -> bool {
        self.completion.status != CompletionStatus::Inactive
            && self.completion.len() > 0
    }

    /// Check if there are signatures that are being rendered
    fn has_signature(&self) -> bool {
        self.signature.status != SignatureStatus::Inactive
            && !self.signature.is_empty()
    }

    fn has_hover(&self) -> bool {
        self.hover.status != HoverStatus::Inactive && !self.hover.is_empty()
    }

    fn has_rename(&self) -> bool {
        self.rename.active
    }

    /// Perform a workspace edit, which are from the LSP (such as code actions, or symbol renaming)
    pub fn apply_workspace_edit(
        &mut self,
        ctx: &mut EventCtx,
        edit: &WorkspaceEdit,
    ) {
        // TODO: I think this probably has some issues if an operation created a file, and then the
        // workspace-edits after this are told to edit the created file. I think it would behave
        // correctly for a *new* file, but not if the created file overwrote an existing file we had
        // open.

        // If there's any operations, (such as creating files, renaming them, or deleting them),
        // then apply those.
        if let Some(DocumentChanges::Operations(op)) = edit.document_changes.as_ref()
        {
            op.iter()
                .flat_map(|op| match op {
                    DocumentChangeOperation::Op(op) => Some(op),
                    _ => None,
                })
                .flat_map(workspace_operation)
                .map(|cmd| Command::new(LAPCE_UI_COMMAND, cmd, Target::Auto))
                .for_each(|cmd| ctx.submit_command(cmd));
        }

        if let BufferContent::File(path) = &self.editor.content {
            if let Some(edits) = workspace_edits(edit) {
                for (url, edits) in edits {
                    if url_matches_path(path, &url) {
                        apply_edit(&mut self.main_split, path, &edits);
                    } else if let Ok(url_path) = url.to_file_path() {
                        // If it is not for the file we have open then we assume that
                        // we may have to load it
                        // So we jump to the location that the edits were at.
                        // TODO: url_matches_path checks if the url path 'goes back' to the original url
                        // Should we do that here?

                        // We choose to just jump to the start of the first edit. The edit function will jump
                        // appropriately when we actually apply the edits.
                        let position = edits.get(0).map(|edit| edit.range.start);
                        let location = EditorLocation {
                            path: url_path.clone(),
                            position,
                            scroll_offset: None,
                            history: None,
                        };

                        // Note: For some reason Rust is unsure about what type the arguments are if we don't specify them
                        // Perhaps this could be fixed by being very explicit about the lifetimes in the jump_to_location_cb fn?
                        let callback = move |_: &mut EventCtx, main_split: &mut LapceMainSplitData| {
                            // The file has been loaded, so we want to apply the edits now.
                            apply_edit(main_split, &url_path, &edits);
                        };
                        self.main_split.jump_to_location_cb(
                            ctx,
                            None,
                            false,
                            location,
                            &self.config,
                            Some(callback),
                        );
                    } else {
                        log::warn!("Text edits failed to apply to URL {url:?} because it was not found");
                    }
                }
            }
        }
    }

    pub fn run_code_action(
        &mut self,
        ctx: &mut EventCtx,
        action: &CodeActionOrCommand,
        plugin_id: &PluginId,
    ) {
        match action {
            CodeActionOrCommand::Command(_cmd) => {}
            CodeActionOrCommand::CodeAction(action) => {
                // If the action contains a workspace edit we can apply it right away
                // otherwise we need to use 'codeAction/resolve'
                // (see: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_codeAction)
                if let Some(edit) = action.edit.as_ref() {
                    self.apply_workspace_edit(ctx, edit);
                } else {
                    self.resolve_code_action(ctx, action, plugin_id)
                }
            }
        }
    }

    /// Resolve a code action and apply its held workspace edit
    fn resolve_code_action(
        &mut self,
        ctx: &mut EventCtx,
        action: &CodeAction,
        plugin_id: &PluginId,
    ) {
        let event_sink = ctx.get_external_handle();
        let view_id = self.view_id;
        self.proxy.proxy_rpc.code_action_resolve(
            action.clone(),
            *plugin_id,
            move |result| {
                if let Ok(ProxyResponse::CodeActionResolveResponse { item }) = result
                {
                    if let Some(edit) = item.edit {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ApplyWorkspaceEdit(edit),
                            Target::Widget(view_id),
                        );
                    }
                }
            },
        )
    }

    fn completion_do_edit(
        &mut self,
        selection: &Selection,
        edits: &[(impl AsRef<Selection>, &str)],
    ) {
        let old_cursor = self.editor.cursor.mode.clone();
        let doc = Arc::make_mut(&mut self.doc);
        let (delta, inval_lines, edits) =
            doc.do_raw_edit(edits, EditType::Completion);
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        Arc::make_mut(&mut self.editor)
            .cursor
            .update_selection(self.doc.buffer(), selection);

        let doc = Arc::make_mut(&mut self.doc);
        doc.buffer_mut().set_cursor_before(old_cursor);
        doc.buffer_mut()
            .set_cursor_after(self.editor.cursor.mode.clone());

        self.apply_deltas(&[(delta, inval_lines, edits)]);
    }

    pub fn apply_completion_item(&mut self, item: &CompletionItem) -> Result<()> {
        // Get all the edits which would be applied in places other than right where the cursor is
        let additional_edit: Vec<_> = item
            .additional_text_edits
            .as_ref()
            .into_iter()
            .flatten()
            .map(|edit| {
                let selection = lapce_core::selection::Selection::region(
                    self.doc.buffer().offset_of_position(&edit.range.start),
                    self.doc.buffer().offset_of_position(&edit.range.end),
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
                    let offset = self.editor.cursor.offset();
                    let start_offset = self.doc.buffer().prev_code_boundary(offset);
                    let end_offset = self.doc.buffer().next_code_boundary(offset);
                    let edit_start =
                        self.doc.buffer().offset_of_position(&edit.range.start);
                    let edit_end =
                        self.doc.buffer().offset_of_position(&edit.range.end);

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
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let old_cursor = self.editor.cursor.mode.clone();
                            let (delta, inval_lines, edits) =
                                Arc::make_mut(&mut self.doc).do_raw_edit(
                                    &[
                                        &[(selection.clone(), text.as_str())][..],
                                        &additional_edit[..],
                                    ]
                                    .concat(),
                                    EditType::Completion,
                                );

                            let selection = selection.apply_delta(
                                &delta,
                                true,
                                InsertDrift::Default,
                            );

                            let start_offset = additional_edit
                                .iter()
                                .map(|(selection, _)| selection.min_offset())
                                .min()
                                .map(|offset| {
                                    offset.min(start_offset).min(edit_start)
                                })
                                .unwrap_or(start_offset);

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer.transform(start_offset, false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.is_empty() {
                                Arc::make_mut(&mut self.editor)
                                    .cursor
                                    .update_selection(self.doc.buffer(), selection);

                                let doc = Arc::make_mut(&mut self.doc);
                                doc.buffer_mut().set_cursor_before(old_cursor);
                                doc.buffer_mut().set_cursor_after(
                                    self.editor.cursor.mode.clone(),
                                );

                                self.apply_deltas(&[(delta, inval_lines, edits)]);
                                return Ok(());
                            }

                            let mut selection =
                                lapce_core::selection::Selection::new();
                            let (_tab, (start, end)) = &snippet_tabs[0];
                            let region = lapce_core::selection::SelRegion::new(
                                *start, *end, None,
                            );
                            selection.add_region(region);
                            Arc::make_mut(&mut self.editor)
                                .cursor
                                .set_insert(selection);

                            let doc = Arc::make_mut(&mut self.doc);
                            doc.buffer_mut().set_cursor_before(old_cursor);
                            doc.buffer_mut()
                                .set_cursor_after(self.editor.cursor.mode.clone());

                            self.apply_deltas(&[(delta, inval_lines, edits)]);
                            Arc::make_mut(&mut self.editor)
                                .add_snippet_placeholders(snippet_tabs);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                CompletionTextEdit::InsertAndReplace(_) => (),
            }
        }

        let offset = self.editor.cursor.offset();
        let start_offset = self.doc.buffer().prev_code_boundary(offset);
        let end_offset = self.doc.buffer().next_code_boundary(offset);
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

    pub fn cancel_completion(&mut self) {
        if self.completion.status == CompletionStatus::Inactive {
            return;
        }
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
    }

    pub fn cancel_signature(&mut self) {
        if self.signature.status == SignatureStatus::Inactive {
            return;
        }
        let signature = Arc::make_mut(&mut self.signature);
        signature.cancel();
    }

    pub fn cancel_hover(&mut self) {
        let hover = Arc::make_mut(&mut self.hover);
        hover.cancel();
    }

    pub fn cancel_rename(&mut self, ctx: &mut EventCtx) {
        let rename = Arc::make_mut(&mut self.rename);
        rename.cancel();
        if self.focus_area == FocusArea::Rename {
            if let Some(active) = *self.main_split.active_tab {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(active),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(*self.main_split.split_id),
                ));
            }
        }
    }

    /// Select the current completion item, getting the data (potentially needing an LSP request)
    /// and then apply it.
    pub fn completion_item_select(&mut self, ctx: &mut EventCtx) {
        let item = if let Some(item) = self.completion.current_item() {
            item.to_owned()
        } else {
            // There was no selected item, this may be due to a bug in failing to ensure that the index was valid.
            return;
        };

        self.cancel_completion();
        if item.item.data.is_some() {
            let view_id = self.editor.view_id;
            let buffer_id = self.doc.id();
            let rev = self.doc.rev();
            let offset = self.editor.cursor.offset();
            let event_sink = ctx.get_external_handle();
            self.proxy.proxy_rpc.completion_resolve(
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
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ResolveCompletion {
                            id: buffer_id,
                            rev,
                            offset,
                            item: Box::new(item),
                        },
                        Target::Widget(view_id),
                    );
                },
            );
        } else {
            let _ = self.apply_completion_item(&item.item);
        }
    }

    /// Update the displayed autocompletion box
    /// Sends a request to the LSP for completion information
    fn update_completion(
        &mut self,
        _ctx: &mut EventCtx,
        display_if_empty_input: bool,
    ) {
        if self.get_mode() != Mode::Insert {
            self.cancel_completion();
            return;
        }
        if !self.doc.loaded() {
            return;
        }
        if !self.doc.content().is_file() {
            return;
        }
        let offset = self.editor.cursor.offset();
        let start_offset = self.doc.buffer().prev_code_boundary(offset);
        let end_offset = self.doc.buffer().next_code_boundary(offset);
        let input = self
            .doc
            .buffer()
            .slice_to_cow(start_offset..end_offset)
            .to_string();
        let char = if start_offset == 0 {
            "".to_string()
        } else {
            self.doc
                .buffer()
                .slice_to_cow(start_offset - 1..start_offset)
                .to_string()
        };
        let completion = Arc::make_mut(&mut self.completion);
        if !display_if_empty_input && input.is_empty() && char != "." && char != ":"
        {
            completion.cancel();
            return;
        }

        if completion.status != CompletionStatus::Inactive
            && completion.offset == start_offset
            && completion.buffer_id == self.doc.id()
        {
            completion.update_input(input.clone());

            if !completion.input_items.contains_key("") {
                let start_pos = self.doc.buffer().offset_to_position(start_offset);
                completion.request(
                    self.proxy.clone(),
                    self.doc.content().path().unwrap().into(),
                    "".to_string(),
                    start_pos,
                );
            }

            if !completion.input_items.contains_key(&input) {
                let position = self.doc.buffer().offset_to_position(offset);
                completion.request(
                    self.proxy.clone(),
                    self.doc.content().path().unwrap().into(),
                    input,
                    position,
                );
            }

            return;
        }

        completion.buffer_id = self.doc.id();
        completion.offset = start_offset;
        completion.input = input.clone();
        completion.status = CompletionStatus::Started;
        completion.input_items.clear();
        completion.request_id += 1;
        let start_pos = self.doc.buffer().offset_to_position(start_offset);
        completion.request(
            self.proxy.clone(),
            self.doc.content().path().unwrap().into(),
            "".to_string(),
            start_pos,
        );

        if !input.is_empty() {
            let position = self.doc.buffer().offset_to_position(offset);
            completion.request(
                self.proxy.clone(),
                self.doc.content().path().unwrap().into(),
                input,
                position,
            );
        }
    }

    fn update_signature(&mut self) {
        if self.get_mode() != Mode::Insert {
            self.cancel_signature();
            return;
        }
        if !self.doc.loaded() || !self.doc.content().is_file() {
            return;
        }

        let offset = self.editor.cursor.offset();

        let start_offset = match self
            .doc
            .syntax()
            .and_then(|syntax| syntax.find_enclosing_parentheses(offset))
        {
            Some((start, _)) => start,
            None => {
                self.cancel_signature();
                return;
            }
        };

        let signature = Arc::make_mut(&mut self.signature);

        signature.buffer_id = self.doc.id();
        signature.offset = start_offset;
        signature.status = SignatureStatus::Started;
        signature.request_id += 1;

        let pos = self.doc.buffer().offset_to_position(offset);
        signature.request(
            self.proxy.clone(),
            signature.request_id,
            self.doc.content().path().unwrap().into(),
            pos,
        );
    }

    /// return true if there's existing hover and it's not changed
    pub fn check_hover(
        &mut self,
        _ctx: &mut EventCtx,
        offset: usize,
        is_inside: bool,
        within_scroll: bool,
    ) -> bool {
        if self.hover.status != HoverStatus::Inactive {
            if !is_inside || !within_scroll {
                let hover = Arc::make_mut(&mut self.hover);
                hover.cancel();
                return false;
            }

            let start_offset = self.doc.buffer().prev_code_boundary(offset);
            if self.doc.id() == self.hover.buffer_id
                && start_offset == self.hover.offset
            {
                return true;
            }

            let hover = Arc::make_mut(&mut self.hover);
            hover.cancel();
            return false;
        }

        false
    }

    pub fn update_hover(&mut self, ctx: &mut EventCtx, offset: usize) {
        if !self.doc.loaded() {
            return;
        }

        if !self.doc.content().is_file() {
            return;
        }

        let start_offset = self.doc.buffer().prev_code_boundary(offset);
        let end_offset = self.doc.buffer().next_code_boundary(offset);
        let input = self.doc.buffer().slice_to_cow(start_offset..end_offset);
        if input.trim().is_empty() {
            return;
        }

        // Get the diagnostics for when we make the request
        let diagnostics = self.diagnostics().map(Arc::clone);

        let mut hover = Arc::make_mut(&mut self.hover);

        if hover.status != HoverStatus::Inactive
            && hover.offset == start_offset
            && hover.buffer_id == self.doc.id()
        {
            // We're hovering over the same location, but are trying to update
            return;
        }

        hover.buffer_id = self.doc.id();
        hover.editor_view_id = self.editor.view_id;
        hover.offset = start_offset;
        hover.status = HoverStatus::Started;
        Arc::make_mut(&mut hover.items).clear();
        hover.request_id += 1;

        let event_sink = ctx.get_external_handle();
        let start_pos = self.doc.buffer().offset_to_position(start_offset);
        hover.request(
            self.proxy.clone(),
            hover.request_id,
            self.doc.clone(),
            diagnostics,
            start_pos,
            hover.id,
            event_sink,
            self.config.clone(),
        );
    }

    fn update_snippet_offset(&mut self, delta: &RopeDelta) {
        if let Some(snippet) = &self.editor.snippet {
            let mut transformer = Transformer::new(delta);
            Arc::make_mut(&mut self.editor).snippet = Some(
                snippet
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
        }
    }
    fn diff_file_positions(&self) -> Vec<(PathBuf, Vec<usize>)> {
        let buffer = self.doc.buffer();
        let mut diff_files: Vec<(PathBuf, Vec<usize>)> = self
            .source_control
            .file_diffs
            .iter()
            .map(|(path, _)| {
                let mut positions = Vec::new();
                if let Some(doc) = self.main_split.open_docs.get(path) {
                    if let Some(history) = doc.get_history("head") {
                        for (i, change) in history.changes().iter().enumerate() {
                            match change {
                                DiffLines::Left(_) => {
                                    if let Some(next) = history.changes().get(i + 1)
                                    {
                                        match next {
                                            DiffLines::Right(_) => {}
                                            DiffLines::Left(_) => {}
                                            DiffLines::Both(_, r) => {
                                                let start =
                                                    buffer.offset_of_line(r.start);
                                                positions.push(start);
                                            }
                                            DiffLines::Skip(_, r) => {
                                                let start =
                                                    buffer.offset_of_line(r.start);
                                                positions.push(start);
                                            }
                                        }
                                    }
                                }
                                DiffLines::Both(_, _) => {}
                                DiffLines::Skip(_, _) => {}
                                DiffLines::Right(r) => {
                                    let start = buffer.offset_of_line(r.start);
                                    positions.push(start);
                                }
                            }
                        }
                    }
                }
                if positions.is_empty() {
                    positions.push(0);
                }
                (path.clone(), positions)
            })
            .collect();
        diff_files.sort();
        diff_files
    }

    fn prev_diff(&mut self, ctx: &mut EventCtx) {
        if let BufferContent::File(buffer_path) = self.doc.content() {
            if self.source_control.file_diffs.is_empty() {
                return;
            }

            let diff_files: Vec<(PathBuf, Vec<usize>)> = self.diff_file_positions();

            let offset = self.editor.cursor.offset();
            let (path, offset) =
                prev_in_file_diff_offset(offset, buffer_path, &diff_files);
            let location = EditorLocation {
                path: path.to_path_buf(),
                position: Some(offset),
                scroll_offset: None,
                history: Some("head".to_string()),
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location, true),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
    }

    fn next_diff(&mut self, ctx: &mut EventCtx) {
        if let BufferContent::File(buffer_path) = self.doc.content() {
            if self.source_control.file_diffs.is_empty() {
                return;
            }

            let diff_files: Vec<(PathBuf, Vec<usize>)> = self.diff_file_positions();

            let offset = self.editor.cursor.offset();
            let (path, offset) =
                next_in_file_diff_offset(offset, buffer_path, &diff_files);
            let location = EditorLocation {
                path: path.to_path_buf(),
                position: Some(offset),
                scroll_offset: None,
                history: Some("head".to_string()),
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location, true),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
    }

    fn next_error(&mut self, ctx: &mut EventCtx) {
        if let BufferContent::File(buffer_path) = self.doc.content() {
            let mut file_diagnostics: Vec<(&PathBuf, Vec<Position>)> = self
                .main_split
                .diagnostics_items(DiagnosticSeverity::ERROR)
                .into_iter()
                .map(|(p, d)| {
                    (p, d.iter().map(|d| d.diagnostic.range.start).collect())
                })
                .collect();
            if file_diagnostics.is_empty() {
                return;
            }
            file_diagnostics.sort_by(|a, b| a.0.cmp(b.0));

            let offset = self.editor.cursor.offset();
            let position = self.doc.buffer().offset_to_position(offset);
            let (path, position) =
                next_in_file_errors_offset(position, buffer_path, &file_diagnostics);
            let location = EditorLocation {
                path,
                position: Some(position),
                scroll_offset: None,
                history: None,
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLspLocation(None, location, true),
                Target::Auto,
            ));
        }
    }

    fn jump_location_forward(&mut self, ctx: &mut EventCtx) -> Option<()> {
        if self.main_split.locations.is_empty() {
            return None;
        }
        if self.main_split.current_location >= self.main_split.locations.len() - 1 {
            return None;
        }
        self.main_split.current_location += 1;
        let location =
            self.main_split.locations[self.main_split.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocation(
                None,
                location,
                !self.config.editor.show_tab,
            ),
            Target::Auto,
        ));
        None
    }

    fn jump_location_backward(&mut self, ctx: &mut EventCtx) -> Option<()> {
        if self.main_split.current_location < 1 {
            return None;
        }
        if self.main_split.current_location >= self.main_split.locations.len() {
            if let BufferContent::File(path) = &self.editor.content {
                self.main_split.save_jump_location(
                    path.to_path_buf(),
                    self.editor.cursor.offset(),
                    self.editor.scroll_offset,
                );
            }
            self.main_split.current_location -= 1;
        }
        self.main_split.current_location -= 1;
        let location =
            self.main_split.locations[self.main_split.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocation(
                None,
                location,
                !self.config.editor.show_tab,
            ),
            Target::Auto,
        ));
        None
    }

    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, mods: Modifiers) {
        let line_height = self.config.editor.line_height() as f64;
        let lines =
            (self.editor.size.borrow().height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.run_move_command(
            ctx,
            if down {
                &lapce_core::movement::Movement::Down
            } else {
                &lapce_core::movement::Movement::Up
            },
            Some(lines),
            mods,
        );
        let rect = Rect::ZERO
            .with_origin(
                self.editor.scroll_offset.to_point()
                    + Vec2::new(0.0, if down { distance } else { -distance }),
            )
            .with_size(*self.editor.size.borrow());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureRectVisible(rect),
            Target::Widget(self.editor.view_id),
        ));
    }

    fn scroll(
        &mut self,
        ctx: &mut EventCtx,
        down: bool,
        count: usize,
        mods: Modifiers,
    ) {
        let line_height = self.config.editor.line_height() as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, _col) = self.doc.buffer().offset_to_line_col(offset);
        let top = self.editor.scroll_offset.y + diff;
        let bottom = top + self.editor.size.borrow().height;

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

        match new_line.cmp(&line) {
            Ordering::Greater => {
                self.run_move_command(
                    ctx,
                    &lapce_core::movement::Movement::Down,
                    Some(new_line - line),
                    mods,
                );
            }
            Ordering::Less => {
                self.run_move_command(
                    ctx,
                    &lapce_core::movement::Movement::Up,
                    Some(line - new_line),
                    mods,
                );
            }
            _ => (),
        };

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((self.editor.scroll_offset.x, top)),
            Target::Widget(self.editor.view_id),
        ));
    }

    pub fn current_code_actions(&self) -> Option<&(PluginId, CodeActionResponse)> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.doc.buffer().prev_code_boundary(offset);
        self.doc.code_actions.get(&prev_offset)
    }

    pub fn diagnostics(&self) -> Option<&Arc<Vec<EditorDiagnostic>>> {
        self.doc.diagnostics.as_ref()
    }

    pub fn offset_of_mouse(
        &self,
        text: &mut PietText,
        pos: Point,
        config: &LapceConfig,
    ) -> usize {
        let (line, char_width) = if self.editor.is_code_lens() {
            let (line, font_size) = if let Some(syntax) = self.doc.syntax() {
                let line = syntax.lens.line_of_height(pos.y.floor() as usize);
                let line_height = syntax.lens.height_of_line(line + 1)
                    - syntax.lens.height_of_line(line);

                let font_size = if line_height < config.editor.line_height() {
                    config.editor.code_lens_font_size
                } else {
                    config.editor.font_size
                };

                (line, font_size)
            } else {
                (
                    (pos.y / config.editor.code_lens_font_size as f64).floor()
                        as usize,
                    config.editor.code_lens_font_size,
                )
            };

            (
                line,
                config.char_width(
                    text,
                    font_size as f64,
                    config.editor.font_family(),
                ),
            )
        } else if let Some(compare) = self.editor.compare.as_ref() {
            let line = (pos.y / config.editor.line_height() as f64).floor() as usize;
            let line = self.doc.history_actual_line_from_visual(compare, line);
            (line, config.editor_char_width(text))
        } else {
            let line = (pos.y / config.editor.line_height() as f64).floor() as usize;
            (line, config.editor_char_width(text))
        };

        let last_line = self.doc.buffer().last_line();
        let (line, col) = if line > last_line {
            (last_line, 0)
        } else {
            let line_end = self
                .doc
                .buffer()
                .line_end_col(line, self.editor.cursor.get_mode() != Mode::Normal);

            let col = (if self.editor.cursor.get_mode() == Mode::Insert {
                (pos.x / char_width).round() as usize
            } else {
                (pos.x / char_width).floor() as usize
            })
            .min(line_end);
            (line, col)
        };
        self.doc.buffer().offset_of_line_col(line, col)
    }

    pub fn single_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &LapceConfig,
    ) {
        let (new_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            &self.editor.view,
            config,
        );
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        cursor.set_offset(
            new_offset,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        );

        let mut go_to_definition = false;
        #[cfg(target_os = "macos")]
        if mouse_event.mods.meta() {
            go_to_definition = true;
        }
        #[cfg(not(target_os = "macos"))]
        if mouse_event.mods.ctrl() {
            go_to_definition = true;
        }

        if go_to_definition {
            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::GotoDefinition),
                    data: None,
                },
                Target::Widget(self.editor.view_id),
            ));
        } else if mouse_event.buttons.has_left() {
            ctx.set_active(true);
        }
    }

    pub fn double_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &LapceConfig,
    ) {
        ctx.set_active(true);
        let (mouse_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            &self.editor.view,
            config,
        );
        let (start, end) = self.doc.buffer().select_word(mouse_offset);
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        cursor.add_region(
            start,
            end,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        );
    }

    pub fn triple_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &LapceConfig,
    ) {
        ctx.set_active(true);
        let (mouse_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            &self.editor.view,
            config,
        );
        let line = self.doc.buffer().line_of_offset(mouse_offset);
        let start = self.doc.buffer().offset_of_line(line);
        let end = self.doc.buffer().offset_of_line(line + 1);
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        cursor.add_region(
            start,
            end,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        );
    }

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        for (delta, _, _) in deltas {
            self.inactive_apply_delta(delta);
            self.update_snippet_offset(delta);
        }
        self.update_signature();
    }

    fn save(&mut self, ctx: &mut EventCtx, exit: bool, allow_formatting: bool) {
        if self.doc.buffer().is_pristine() && self.doc.content().is_file() {
            if exit {
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::SplitClose),
                        data: None,
                    },
                    Target::Widget(self.editor.view_id),
                ));
            }
            return;
        }

        if let BufferContent::File(path) = self.doc.content() {
            let format_on_save =
                allow_formatting && self.config.editor.format_on_save;
            let path = path.clone();
            let proxy = self.proxy.clone();
            let rev = self.doc.rev();
            let event_sink = ctx.get_external_handle();
            let view_id = self.editor.view_id;
            let tab_id = self.main_split.tab_id.clone();
            let exit = if exit { Some(view_id) } else { None };

            if format_on_save {
                let (sender, receiver) = bounded(1);
                thread::spawn(move || {
                    proxy.proxy_rpc.get_document_formatting(
                        path.clone(),
                        Box::new(move |result| {
                            let _ = sender.send(result);
                        }),
                    );

                    let result =
                        receiver.recv_timeout(Duration::from_secs(1)).map_or_else(
                            |e| Err(anyhow!("{}", e)),
                            |v| {
                                v.map_err(|e| anyhow!("{:?}", e)).and_then(|r| {
                                    if let ProxyResponse::GetDocumentFormatting {
                                        edits,
                                    } = r
                                    {
                                        Ok(edits)
                                    } else {
                                        Err(anyhow!("wrong response"))
                                    }
                                })
                            },
                        );

                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::DocumentFormatAndSave {
                            path,
                            rev,
                            result,
                            exit,
                        },
                        Target::Widget(*tab_id),
                    );
                });
            } else {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::DocumentSave { path, exit },
                    Target::Widget(*tab_id),
                );
            }
        } else if let BufferContent::Scratch(..) = self.doc.content() {
            let content = self.doc.content().clone();
            let view_id = self.editor.view_id;
            self.main_split.current_save_as =
                Some(Arc::new((content, view_id, exit)));
            let options =
                FileDialogOptions::new().accept_command(LAPCE_SAVE_FILE_AS);
            ctx.submit_command(druid::commands::SHOW_SAVE_PANEL.with(options));
        }
    }

    fn run_move_command(
        &mut self,
        ctx: &mut EventCtx,
        movement: &lapce_core::movement::Movement,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        if movement.is_jump() && movement != &self.editor.last_movement_new {
            if let BufferContent::File(path) = &self.editor.content {
                self.main_split.save_jump_location(
                    path.to_path_buf(),
                    self.editor.cursor.offset(),
                    self.editor.scroll_offset,
                );
            }
        }
        Arc::make_mut(&mut self.editor).last_movement_new = movement.clone();

        let register = Arc::make_mut(&mut self.main_split.register);
        let doc = Arc::make_mut(&mut self.doc);
        let view = self.editor.view.clone();
        doc.move_cursor(
            ctx.text(),
            &mut Arc::make_mut(&mut self.editor).cursor,
            movement,
            count.unwrap_or(1),
            mods.shift(),
            &view,
            register,
            &self.config,
        );
        if let Some(snippet) = self.editor.snippet.as_ref() {
            let offset = self.editor.cursor.offset();
            let mut within_region = false;
            for (_, (start, end)) in snippet {
                if offset >= *start && offset <= *end {
                    within_region = true;
                    break;
                }
            }
            if !within_region {
                Arc::make_mut(&mut self.editor).snippet = None;
            }
        }
        self.cancel_completion();
        self.update_signature();
        self.cancel_hover();
        CommandExecuted::Yes
    }

    fn run_edit_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &EditCommand,
    ) -> CommandExecuted {
        let modal = self.config.core.modal && !self.editor.content.is_input();
        let doc = Arc::make_mut(&mut self.doc);
        let doc_before_edit = doc.buffer().text().clone();
        let register = Arc::make_mut(&mut self.main_split.register);
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        let yank_data =
            if let lapce_core::cursor::CursorMode::Visual { .. } = &cursor.mode {
                Some(cursor.yank(doc.buffer()))
            } else {
                None
            };

        let deltas = doc.do_edit(cursor, cmd, modal, register);

        if !deltas.is_empty() {
            if let Some(data) = yank_data {
                register.add_delete(data);
            }
        }

        if show_completion(cmd, &doc_before_edit, &deltas) {
            self.update_completion(ctx, false);
        } else {
            self.cancel_completion();
        }
        self.apply_deltas(&deltas);
        if let EditCommand::NormalMode = cmd {
            Arc::make_mut(&mut self.editor).snippet = None;
        }

        CommandExecuted::Yes
    }

    fn run_focus_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &FocusCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        use FocusCommand::*;
        match cmd {
            ModalClose => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ModalClose),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                }
                if self.has_completions() {
                    self.cancel_completion();
                }
                if self.has_signature() {
                    self.cancel_signature();
                }
                if self.has_hover() {
                    self.cancel_hover();
                }
                if self.is_rename() {
                    self.cancel_rename(ctx);
                }
            }
            SplitVertical => {
                self.main_split.split_editor(
                    ctx,
                    self.editor.view_id,
                    SplitDirection::Vertical,
                    &self.config,
                );
            }
            SplitHorizontal => {
                self.main_split.split_editor(
                    ctx,
                    self.editor.view_id,
                    SplitDirection::Horizontal,
                    &self.config,
                );
            }
            SplitExchange => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split
                        .split_exchange(ctx, SplitContent::EditorTab(*widget_id));
                }
            }
            SplitLeft => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Left,
                    );
                }
            }
            SplitRight => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Right,
                    );
                }
            }
            SplitUp => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Up,
                    );
                }
            }
            SplitDown => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Down,
                    );
                }
            }
            SplitClose => {
                self.main_split.editor_close(ctx, self.view_id, false);
            }
            ForceExit => {
                self.main_split.editor_close(ctx, self.view_id, true);
            }
            SearchWholeWordForward => {
                Arc::make_mut(&mut self.find).visual = true;
                let offset = self.editor.cursor.offset();
                let (start, end) = self.doc.buffer().select_word(offset);
                let word = self.doc.buffer().slice_to_cow(start..end).to_string();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSearchInput(word.clone()),
                    Target::Widget(*self.main_split.tab_id),
                ));

                Arc::make_mut(&mut self.find).set_find(&word, false, true);
                let next =
                    self.find
                        .next(self.doc.buffer().text(), offset, false, true);
                if let Some((start, _end)) = next {
                    self.run_move_command(
                        ctx,
                        &lapce_core::movement::Movement::Offset(start),
                        None,
                        mods,
                    );
                }
            }
            SearchForward => {
                if self.editor.content.is_search() {
                    if let Some(parent_view_id) = self.editor.parent_view_id {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(
                                    FocusCommand::SearchForward,
                                ),
                                data: None,
                            },
                            Target::Widget(parent_view_id),
                        ));
                    }
                } else {
                    Arc::make_mut(&mut self.find).visual = true;
                    let offset = self.editor.cursor.offset();
                    let next = self.find.next(
                        self.doc.buffer().text(),
                        offset,
                        false,
                        true,
                    );
                    if let Some((start, _end)) = next {
                        self.run_move_command(
                            ctx,
                            &lapce_core::movement::Movement::Offset(start),
                            None,
                            mods,
                        );
                    }
                }
            }
            SearchBackward => {
                if self.editor.content.is_search() {
                    if let Some(parent_view_id) = self.editor.parent_view_id {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(
                                    FocusCommand::SearchBackward,
                                ),
                                data: None,
                            },
                            Target::Widget(parent_view_id),
                        ));
                    }
                } else {
                    Arc::make_mut(&mut self.find).visual = true;
                    let offset = self.editor.cursor.offset();
                    let next =
                        self.find.next(self.doc.buffer().text(), offset, true, true);
                    if let Some((start, _end)) = next {
                        self.run_move_command(
                            ctx,
                            &lapce_core::movement::Movement::Offset(start),
                            None,
                            mods,
                        );
                    }
                }
            }
            ToggleCaseSensitive => {
                let tab_id = *self.main_split.tab_id;
                let find = Arc::make_mut(&mut self.find);
                let case_sensitive = find.toggle_case_sensitive();
                let pattern = find.search_string.clone().unwrap_or_default();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSearch(pattern, Some(case_sensitive)),
                    Target::Widget(tab_id),
                ));
                return CommandExecuted::No;
            }
            GlobalSearchRefresh => {
                let tab_id = *self.main_split.tab_id;
                let pattern = self.doc.buffer().to_string();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSearch(pattern, None),
                    Target::Widget(tab_id),
                ));
            }
            ClearSearch => {
                Arc::make_mut(&mut self.find).visual = false;
                let view_id =
                    if let Some(parent_view_id) = self.editor.parent_view_id {
                        parent_view_id
                    } else if self.editor.content.is_search() {
                        (*self.main_split.active).unwrap_or(self.editor.view_id)
                    } else {
                        self.editor.view_id
                    };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(view_id),
                ));
            }
            SearchInView => {
                let start_line = ((self.editor.scroll_offset.y
                    / self.config.editor.line_height() as f64)
                    .ceil() as usize)
                    .max(self.doc.buffer().last_line());
                let end_line = ((self.editor.scroll_offset.y
                    + self.editor.size.borrow().height
                        / self.config.editor.line_height() as f64)
                    .ceil() as usize)
                    .max(self.doc.buffer().last_line());
                let end_offset = self.doc.buffer().offset_of_line(end_line + 1);

                let offset = self.editor.cursor.offset();
                let line = self.doc.buffer().line_of_offset(offset);
                let offset = self.doc.buffer().offset_of_line(line);
                let next =
                    self.find
                        .next(self.doc.buffer().text(), offset, false, false);

                if let Some(start) = next
                    .map(|(start, _)| start)
                    .filter(|start| *start < end_offset)
                {
                    self.run_move_command(
                        ctx,
                        &lapce_core::movement::Movement::Offset(start),
                        None,
                        mods,
                    );
                } else {
                    let start_offset = self.doc.buffer().offset_of_line(start_line);
                    if let Some((start, _)) = self.find.next(
                        self.doc.buffer().text(),
                        start_offset,
                        false,
                        true,
                    ) {
                        self.run_move_command(
                            ctx,
                            &lapce_core::movement::Movement::Offset(start),
                            None,
                            mods,
                        );
                    }
                }
            }
            ListSelect => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ListSelect),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                } else {
                    let completion = Arc::make_mut(&mut self.completion);
                    completion.run_focus_command(
                        &self.editor,
                        &mut self.doc,
                        &self.config,
                        ctx,
                        cmd,
                    );
                }
            }
            ListNext => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ListNext),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                } else {
                    let completion = Arc::make_mut(&mut self.completion);
                    completion.run_focus_command(
                        &self.editor,
                        &mut self.doc,
                        &self.config,
                        ctx,
                        cmd,
                    );
                }
            }
            ListNextPage => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ListNextPage),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                } else {
                    let completion = Arc::make_mut(&mut self.completion);
                    completion.run_focus_command(
                        &self.editor,
                        &mut self.doc,
                        &self.config,
                        ctx,
                        cmd,
                    );
                }
            }
            ListPrevious => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ListPrevious),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                } else {
                    let completion = Arc::make_mut(&mut self.completion);
                    completion.run_focus_command(
                        &self.editor,
                        &mut self.doc,
                        &self.config,
                        ctx,
                        cmd,
                    );
                }
            }
            ListPreviousPage => {
                if self.is_palette() {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ListPreviousPage),
                            data: None,
                        },
                        Target::Widget(self.palette.widget_id),
                    ));
                } else {
                    let completion = Arc::make_mut(&mut self.completion);
                    completion.run_focus_command(
                        &self.editor,
                        &mut self.doc,
                        &self.config,
                        ctx,
                        cmd,
                    );
                }
            }
            JumpToNextSnippetPlaceholder => {
                if let Some(snippet) = self.editor.snippet.as_ref() {
                    let mut current = 0;
                    let offset = self.editor.cursor.offset();
                    for (i, (_, (start, end))) in snippet.iter().enumerate() {
                        if *start <= offset && offset <= *end {
                            current = i;
                            break;
                        }
                    }

                    let last_placeholder = current + 1 >= snippet.len() - 1;

                    if let Some((_, (start, end))) = snippet.get(current + 1) {
                        let mut selection = lapce_core::selection::Selection::new();
                        let region = lapce_core::selection::SelRegion::new(
                            *start, *end, None,
                        );
                        selection.add_region(region);
                        Arc::make_mut(&mut self.editor).cursor.set_insert(selection);
                    }

                    if last_placeholder {
                        Arc::make_mut(&mut self.editor).snippet = None;
                    }
                    self.update_signature();
                    self.cancel_completion();
                }
            }
            JumpToPrevSnippetPlaceholder => {
                if let Some(snippet) = self.editor.snippet.as_ref() {
                    let mut current = 0;
                    let offset = self.editor.cursor.offset();
                    for (i, (_, (start, end))) in snippet.iter().enumerate() {
                        if *start <= offset && offset <= *end {
                            current = i;
                            break;
                        }
                    }

                    if current > 0 {
                        if let Some((_, (start, end))) = snippet.get(current - 1) {
                            let mut selection =
                                lapce_core::selection::Selection::new();
                            let region = lapce_core::selection::SelRegion::new(
                                *start, *end, None,
                            );
                            selection.add_region(region);
                            Arc::make_mut(&mut self.editor)
                                .cursor
                                .set_insert(selection);
                        }
                        self.update_signature();
                        self.cancel_completion();
                    }
                }
            }
            PageUp => {
                self.page_move(ctx, false, mods);
            }
            PageDown => {
                self.page_move(ctx, true, mods);
            }
            ScrollUp => {
                self.scroll(ctx, false, count.unwrap_or(1), mods);
            }
            ScrollDown => {
                self.scroll(ctx, true, count.unwrap_or(1), mods);
            }
            CenterOfWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorPosition(
                        EnsureVisiblePosition::CenterOfWindow,
                    ),
                    Target::Widget(self.editor.view_id),
                ));
            }
            TopOfWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorPosition(
                        EnsureVisiblePosition::TopOfWindow,
                    ),
                    Target::Widget(self.editor.view_id),
                ));
            }
            BottomOfWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorPosition(
                        EnsureVisiblePosition::BottomOfWindow,
                    ),
                    Target::Widget(self.editor.view_id),
                ));
            }
            ShowCodeActions => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowCodeActions(None),
                    Target::Widget(self.editor.editor_id),
                ));
            }
            GetCompletion => {
                // we allow empty inputs to allow for cases where the user wants to get the autocompletion beforehand
                self.update_completion(ctx, true);
            }
            GetSignature => {
                self.update_signature();
            }
            GotoDefinition => {
                if let BufferContent::File(path) = self.doc.content() {
                    let offset = self.editor.cursor.offset();
                    let start_offset = self.doc.buffer().prev_code_boundary(offset);
                    let start_position =
                        self.doc.buffer().offset_to_position(start_offset);
                    let event_sink = ctx.get_external_handle();
                    let position = self.doc.buffer().offset_to_position(offset);
                    let proxy = self.proxy.clone();
                    let editor_view_id = self.editor.view_id;
                    let path = path.clone();
                    self.proxy.proxy_rpc.get_definition(
                        offset,
                        path.clone(),
                        position,
                        move |result| {
                            if let Ok(ProxyResponse::GetDefinitionResponse {
                                          definition,
                                          ..
                                      }) = result
                            {
                                if let Some(location) = match definition {
                                    GotoDefinitionResponse::Scalar(location) => {
                                        Some(location)
                                    }
                                    GotoDefinitionResponse::Array(locations) => {
                                        if !locations.is_empty() {
                                            Some(locations[0].clone())
                                        } else {
                                            None
                                        }
                                    }
                                    GotoDefinitionResponse::Link(
                                        location_links,
                                    ) => {
                                        let location_link = location_links[0].clone();
                                        Some(Location { uri: location_link.target_uri, range:location_link.target_selection_range  })
                                    },
                                } {
                                    if location.range.start == start_position {
                                        proxy.proxy_rpc.get_references(
                                            path.clone(),
                                            position,
                                            move |result| {
                                                if let Ok(ProxyResponse::GetReferencesResponse { references }) = result {
                                                    process_get_references(
                                                        offset, references, event_sink,
                                                    );
                                                }
                                            },
                                        );
                                    } else {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition {
                                                editor_view_id,
                                                offset,
                                                location: EditorLocation {
                                                    path: path_from_url(
                                                        &location.uri,
                                                    ),
                                                    position: Some(
                                                        location.range.start,
                                                    ),
                                                    scroll_offset: None,
                                                    history: None,
                                                },
                                            },
                                            Target::Auto,
                                        );
                                    }
                                }
                            }
                        },
                    );
                }
            }
            GotoTypeDefinition => {
                if let BufferContent::File(path) = self.doc.content() {
                    let offset = self.editor.cursor.offset();
                    let event_sink = ctx.get_external_handle();
                    let position = self.doc.buffer().offset_to_position(offset);
                    let editor_view_id = self.editor.view_id;
                    self.proxy.proxy_rpc.get_type_definition(
                        offset,
                        path.clone(),
                        position,
                        move |result| {
                            if let Ok(ProxyResponse::GetTypeDefinition {
                                          definition,
                                          ..
                                      }) = result
                            {
                                match definition {
                                    GotoTypeDefinitionResponse::Scalar(location) => {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition {
                                                editor_view_id,
                                                offset,
                                                location: EditorLocation {
                                                    path: path_from_url(
                                                        &location.uri,
                                                    ),
                                                    position: Some(
                                                        location.range.start,
                                                    ),
                                                    scroll_offset: None,
                                                    history: None,
                                                },
                                            },
                                            Target::Auto,
                                        );
                                    }
                                    GotoTypeDefinitionResponse::Array(locations) => {
                                        let len = locations.len();
                                        match len {
                                            1 => {
                                                let _ = event_sink.submit_command(
                                                    LAPCE_UI_COMMAND,
                                                    LapceUICommand::GotoDefinition {
                                                        editor_view_id,
                                                        offset,
                                                        location: EditorLocation {
                                                            path: path_from_url(
                                                                &locations[0].uri,
                                                            ),
                                                            position: Some(
                                                                locations[0]
                                                                    .range
                                                                    .start,
                                                            ),
                                                            scroll_offset: None,
                                                            history: None,
                                                        },
                                                    },
                                                    Target::Auto,
                                                );
                                            }
                                            _ if len > 1 => {
                                                let _ = event_sink.submit_command(
                                                    LAPCE_UI_COMMAND,
                                                    LapceUICommand::PaletteReferences(
                                                        offset, locations,
                                                    ),
                                                    Target::Auto,
                                                );
                                            }
                                            _ => (),
                                        }
                                    }
                                    GotoTypeDefinitionResponse::Link(
                                        location_links,
                                    ) => {
                                        let location_link = location_links[0].clone();
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition {
                                                editor_view_id,
                                                offset,
                                                location: EditorLocation {
                                                    path: path_from_url(
                                                        &location_link.target_uri,
                                                    ),
                                                    position: Some(
                                                        location_link.target_selection_range.start
                                                    ),
                                                    scroll_offset: None,
                                                    history: None,
                                                },
                                            },
                                            Target::Auto,
                                        );
                                    }
                                }
                            }
                        },
                    );
                }
            }
            ShowHover => {
                let offset = self.editor.cursor.offset();
                self.update_hover(ctx, offset);
            }
            JumpLocationBackward => {
                self.jump_location_backward(ctx);
            }
            JumpLocationForward => {
                self.jump_location_forward(ctx);
            }
            NextError => {
                self.next_error(ctx);
            }
            PreviousDiff => {
                self.prev_diff(ctx);
            }
            NextDiff => {
                self.next_diff(ctx);
            }
            ToggleCodeLens => {
                let editor = Arc::make_mut(&mut self.editor);
                editor.view = match editor.view {
                    EditorView::Normal => EditorView::Lens,
                    EditorView::Lens => EditorView::Normal,
                    EditorView::Diff(_) => return CommandExecuted::Yes,
                };
            }
            ToggleHistory => {
                let editor = Arc::make_mut(&mut self.editor);
                (editor.view, editor.compare) = match editor.view {
                    EditorView::Normal => (
                        EditorView::Diff(String::from("head")),
                        Some(String::from("head")),
                    ),
                    EditorView::Diff(_) => (EditorView::Normal, None),
                    EditorView::Lens => return CommandExecuted::Yes,
                };
            }
            FormatDocument => {
                if let BufferContent::File(path) = self.doc.content() {
                    let path = path.clone();
                    let proxy = self.proxy.clone();
                    let rev = self.doc.rev();
                    let event_sink = ctx.get_external_handle();
                    let (sender, receiver) = bounded(1);
                    let tab_id = self.main_split.tab_id.clone();
                    thread::spawn(move || {
                        proxy.proxy_rpc.get_document_formatting(
                            path.clone(),
                            Box::new(move |result| {
                                let _ = sender.send(result);
                            }),
                        );

                        let result = receiver
                            .recv_timeout(Duration::from_secs(1))
                            .map_or_else(
                                |e| Err(anyhow!("{}", e)),
                                |v| {
                                    v.map_err(|e| anyhow!("{:?}", e)).and_then(|r| {
                                        if let ProxyResponse::GetDocumentFormatting {
                                            edits,
                                        } = r
                                        {
                                            Ok(edits)
                                        } else {
                                            Err(anyhow!("wrong response"))
                                        }
                                    })
                                },
                            );
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::DocumentFormat { path, rev, result },
                            Target::Widget(*tab_id),
                        );
                    });
                }
            }
            Search => {
                Arc::make_mut(&mut self.find).visual = true;
                let region = match &self.editor.cursor.mode {
                    lapce_core::cursor::CursorMode::Normal(offset) => {
                        lapce_core::selection::SelRegion::caret(*offset)
                    }
                    lapce_core::cursor::CursorMode::Visual {
                        start,
                        end,
                        mode: _,
                    } => lapce_core::selection::SelRegion::new(
                        *start.min(end),
                        self.doc.buffer().next_grapheme_offset(
                            *start.max(end),
                            1,
                            self.doc.buffer().len(),
                        ),
                        None,
                    ),
                    lapce_core::cursor::CursorMode::Insert(selection) => {
                        *selection.last_inserted().unwrap()
                    }
                };
                let pattern = if region.is_caret() {
                    let (start, end) = self.doc.buffer().select_word(region.start);
                    self.doc.buffer().slice_to_cow(start..end).to_string()
                } else {
                    self.doc
                        .buffer()
                        .slice_to_cow(region.min()..region.max())
                        .to_string()
                };
                if !pattern.contains('\n') {
                    Arc::make_mut(&mut self.find).set_find(&pattern, false, false);
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSearchInput(pattern),
                        Target::Widget(*self.main_split.tab_id),
                    ));
                }
                if let Some((find_view_id, _)) = self.editor.find_view_id {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::MultiSelection(
                                MultiSelectionCommand::SelectAll,
                            ),
                            data: None,
                        },
                        Target::Widget(find_view_id),
                    ));
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(find_view_id),
                    ));
                }
            }
            InlineFindLeft => {
                Arc::make_mut(&mut self.editor).inline_find =
                    Some(InlineFindDirection::Left);
            }
            InlineFindRight => {
                Arc::make_mut(&mut self.editor).inline_find =
                    Some(InlineFindDirection::Right);
            }
            RepeatLastInlineFind => {
                if let Some((direction, c)) = self.editor.last_inline_find.clone() {
                    self.inline_find(ctx, direction, &c);
                }
            }
            SaveAndExit => {
                self.save(ctx, true, true);
            }
            Save => {
                self.save(ctx, false, true);
            }
            SaveWithoutFormatting => {
                self.save(ctx, false, false);
            }
            Rename => {
                if let BufferContent::File(path) = self.doc.content() {
                    let offset = self.editor.cursor.offset();
                    let buffer = self.doc.buffer().clone();
                    let rev = self.doc.rev();
                    let path = path.to_path_buf();
                    let tab_id = *self.main_split.tab_id;
                    let event_sink = ctx.get_external_handle();

                    let position = self.doc.buffer().offset_to_position(offset);

                    Arc::make_mut(&mut self.rename).update(
                        path.clone(),
                        rev,
                        offset,
                        position,
                        self.editor.view_id,
                    );
                    self.proxy.proxy_rpc.prepare_rename(
                        path.clone(),
                        position,
                        move |result| {
                            if let Ok(ProxyResponse::PrepareRename { resp }) = result
                            {
                                RenameData::prepare_rename(
                                    tab_id, path, offset, rev, buffer, resp,
                                    event_sink,
                                );
                            }
                        },
                    );
                }
            }
            ConfirmRename => {
                let new_name = self
                    .main_split
                    .local_docs
                    .get(&LocalBufferKind::Rename)
                    .unwrap()
                    .buffer()
                    .text()
                    .to_string();
                let new_name = new_name.trim();
                if !new_name.is_empty() {
                    let event_sink = ctx.get_external_handle();
                    let view_id = self.rename.from_editor;
                    self.proxy.proxy_rpc.rename(
                        self.rename.path.clone(),
                        self.rename.position,
                        new_name.to_string(),
                        move |result| {
                            if let Ok(ProxyResponse::Rename { edit }) = result {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::ApplyWorkspaceEdit(edit),
                                    Target::Widget(view_id),
                                );
                            }
                        },
                    );
                }
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::ModalClose),
                        data: None,
                    },
                    Target::Widget(self.rename.view_id),
                ));
            }
            SelectNextSyntaxItem => {
                self.run_selection_range_command(ctx, SelectionRangeDirection::Next)
            }
            SelectPreviousSyntaxItem => self
                .run_selection_range_command(ctx, SelectionRangeDirection::Previous),
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn run_selection_range_command(
        &mut self,
        ctx: &mut EventCtx,
        direction: SelectionRangeDirection,
    ) {
        let offset = self.editor.cursor.offset();
        if let BufferContent::File(path) = self.doc.content() {
            let rev = self.doc.buffer().rev();
            let buffer_id = self.doc.id();
            let event_sink = ctx.get_external_handle();
            let current_selection = self.editor.cursor.get_selection();

            match &self.doc.syntax_selection_range {
                // If the cached selection range match current revision, no need to call the
                // LSP server, we ca apply it right now
                Some(selection_range)
                    if selection_range.match_request(
                        buffer_id,
                        rev,
                        current_selection,
                    ) =>
                {
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ApplySelectionRange {
                            rev,
                            buffer_id,
                            direction,
                        },
                        Target::Auto,
                    );
                }
                // Otherwise, ask the LSP server for `textDocument/selectionRange`
                _ => {
                    let position = self.doc.buffer().offset_to_position(offset);
                    self.proxy.proxy_rpc.get_selection_range(
                        path.to_owned(),
                        vec![position],
                        move |result| {
                            if let Ok(ProxyResponse::GetSelectionRange { ranges }) =
                                result
                            {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::StoreSelectionRangeAndApply {
                                        ranges,
                                        rev,
                                        buffer_id,
                                        direction,
                                        current_selection,
                                    },
                                    Target::Auto,
                                );
                            }
                        },
                    )
                }
            }
        }
    }

    fn run_motion_mode_command(
        &mut self,
        _ctx: &mut EventCtx,
        cmd: &MotionModeCommand,
    ) -> CommandExecuted {
        let motion_mode = match cmd {
            MotionModeCommand::MotionModeDelete => MotionMode::Delete,
            MotionModeCommand::MotionModeIndent => MotionMode::Indent,
            MotionModeCommand::MotionModeOutdent => MotionMode::Outdent,
            MotionModeCommand::MotionModeYank => MotionMode::Yank,
        };
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        let doc = Arc::make_mut(&mut self.doc);
        let register = Arc::make_mut(&mut self.main_split.register);
        doc.do_motion_mode(cursor, motion_mode, register);
        CommandExecuted::Yes
    }

    fn run_multi_selection_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &MultiSelectionCommand,
    ) -> CommandExecuted {
        let view = self.editor.view.clone();
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        self.doc
            .do_multi_selection(ctx.text(), cursor, cmd, &view, &self.config);
        self.cancel_signature();
        self.cancel_completion();
        CommandExecuted::Yes
    }
}

impl KeyPressFocus for LapceEditorBufferData {
    fn get_mode(&self) -> Mode {
        self.editor.cursor.get_mode()
    }

    fn focus_only(&self) -> bool {
        self.editor.content.is_settings()
    }

    fn expect_char(&self) -> bool {
        self.editor.inline_find.is_some()
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "search_active" => {
                if self.config.core.modal && !self.editor.cursor.is_normal() {
                    false
                } else {
                    self.find.visual
                }
            }
            "search_focus" => {
                self.editor.content == BufferContent::Local(LocalBufferKind::Search)
                    && self.editor.parent_view_id.is_some()
            }
            "global_search_focus" => {
                self.editor.content == BufferContent::Local(LocalBufferKind::Search)
                    && self.editor.parent_view_id.is_none()
            }
            "input_focus" => self.editor.content.is_input(),
            "editor_focus" => match self.editor.content {
                BufferContent::File(_) => true,
                BufferContent::Scratch(..) => true,
                BufferContent::Local(_) => false,
                BufferContent::SettingsValue(..) => false,
            },
            "diff_focus" => self.editor.compare.is_some(),
            "source_control_focus" => {
                self.editor.content
                    == BufferContent::Local(LocalBufferKind::SourceControl)
            }
            "in_snippet" => self.editor.snippet.is_some(),
            "completion_focus" => self.has_completions(),
            "hover_focus" => self.has_hover(),
            "list_focus" => self.has_completions() || self.is_palette(),
            "rename_focus" => self.has_rename(),
            "modal_focus" => {
                (self.has_completions() && !self.config.core.modal)
                    || self.has_hover()
                    || self.is_palette()
                    || self.has_rename()
            }
            _ => false,
        }
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            let doc = Arc::make_mut(&mut self.doc);
            let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
            let deltas = doc.do_insert(cursor, c, &self.config);

            if !c
                .chars()
                .all(|c| c.is_whitespace() || c.is_ascii_whitespace())
            {
                self.update_completion(ctx, false);
            } else {
                self.cancel_completion();
            }
            self.cancel_hover();
            self.apply_deltas(&deltas);
        } else if let Some(direction) = self.editor.inline_find.clone() {
            self.inline_find(ctx, direction.clone(), c);
            let editor = Arc::make_mut(&mut self.editor);
            editor.last_inline_find = Some((direction, c.to_string()));
            editor.inline_find = None;
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        let old_doc = self.doc.clone();
        let executed = match &command.kind {
            CommandKind::Edit(cmd) => self.run_edit_command(ctx, cmd),
            CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                self.run_move_command(ctx, &movement, count, mods)
            }
            CommandKind::Focus(cmd) => self.run_focus_command(ctx, cmd, count, mods),
            CommandKind::MotionMode(cmd) => self.run_motion_mode_command(ctx, cmd),
            CommandKind::MultiSelection(cmd) => {
                self.run_multi_selection_command(ctx, cmd)
            }
            CommandKind::Workbench(_) => CommandExecuted::No,
        };
        let doc = self.doc.clone();
        if doc.content() != old_doc.content() || doc.rev() != old_doc.rev() {
            Arc::make_mut(&mut self.editor)
                .cursor
                .history_selections
                .clear();
        }

        executed
    }
}

#[derive(Clone)]
pub struct TabRect {
    pub svg: Svg,
    pub svg_color: Option<Color>,
    pub rect: Rect,
    pub close_rect: Rect,
    pub text_layout: PietTextLayout,
    pub path_layout: Option<PietTextLayout>,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

fn prev_in_file_diff_offset<'a>(
    offset: usize,
    path: &Path,
    file_diffs: &'a [(PathBuf, Vec<usize>)],
) -> (&'a Path, usize) {
    for (current_path, offsets) in file_diffs.iter().rev() {
        if path == current_path {
            for diff_offset in offsets.iter().rev() {
                if *diff_offset < offset {
                    return (current_path.as_ref(), *diff_offset);
                }
            }
        }
        if current_path < path {
            return (current_path.as_ref(), offsets[0]);
        }
    }
    (file_diffs[0].0.as_ref(), file_diffs[0].1[0])
}

fn next_in_file_diff_offset<'a>(
    offset: usize,
    path: &Path,
    file_diffs: &'a [(PathBuf, Vec<usize>)],
) -> (&'a Path, usize) {
    for (current_path, offsets) in file_diffs {
        if path == current_path {
            for diff_offset in offsets {
                if *diff_offset > offset {
                    return (current_path.as_ref(), *diff_offset);
                }
            }
        }
        if current_path > path {
            return (current_path.as_ref(), offsets[0]);
        }
    }
    (file_diffs[0].0.as_ref(), file_diffs[0].1[0])
}

fn next_in_file_errors_offset(
    position: Position,
    path: &Path,
    file_diagnostics: &[(&PathBuf, Vec<Position>)],
) -> (PathBuf, Position) {
    for (current_path, positions) in file_diagnostics {
        if &path == current_path {
            for error_position in positions {
                if error_position.line > position.line
                    || (error_position.line == position.line
                        && error_position.character > position.character)
                {
                    return ((*current_path).clone(), *error_position);
                }
            }
        }
        if current_path > &path {
            return ((*current_path).clone(), positions[0]);
        }
    }
    ((*file_diagnostics[0].0).clone(), file_diagnostics[0].1[0])
}

fn process_get_references(
    offset: usize,
    locations: Vec<Location>,
    event_sink: ExtEventSink,
) {
    if locations.is_empty() {
        return;
    }
    if locations.len() == 1 {
        // If there's only a single location then just jump directly to it
        let location = &locations[0];
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::JumpToLspLocation(
                None,
                EditorLocation {
                    path: path_from_url(&location.uri),
                    position: Some(location.range.start),
                    scroll_offset: None,
                    history: None,
                },
                true,
            ),
            Target::Auto,
        );
    } else {
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::PaletteReferences(offset, locations),
            Target::Auto,
        );
    }
}

fn workspace_edits(edit: &WorkspaceEdit) -> Option<HashMap<Url, Vec<TextEdit>>> {
    if let Some(changes) = edit.changes.as_ref() {
        return Some(changes.clone());
    }

    let changes = edit.document_changes.as_ref()?;
    let edits = match changes {
        DocumentChanges::Edits(edits) => edits
            .iter()
            .map(|e| {
                (
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
        DocumentChanges::Operations(ops) => ops
            .iter()
            .filter_map(|o| match o {
                DocumentChangeOperation::Op(_op) => None,
                DocumentChangeOperation::Edit(e) => Some((
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )),
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
    };
    Some(edits)
}

fn workspace_operation(op: &ResourceOp) -> Option<LapceUICommand> {
    Some(match op {
        ResourceOp::Create(p) => LapceUICommand::CreateFileOpen {
            path: p.uri.to_file_path().ok()?,
        },
        ResourceOp::Rename(p) => LapceUICommand::RenamePath {
            from: p.old_uri.to_file_path().ok()?,
            to: p.new_uri.to_file_path().ok()?,
        },
        ResourceOp::Delete(p) => LapceUICommand::TrashPath {
            path: p.uri.to_file_path().ok()?,
        },
    })
}

/// Check if a [`Url`] matches the path
fn url_matches_path(path: &Path, url: &Url) -> bool {
    // TODO: Neither of these methods work for paths
    // on different filesystems (i.e. windows and linux),
    // as pathbuf is meant to represent a path on the host
    let mut matches = false;
    // This handles windows drive letters, which rust-url doesn't do.
    if let Ok(url_path) = url.to_file_path() {
        matches |= url_path == path;
    }
    // This is the previous check, to ensure this isn't a regression
    if let Ok(path_url) = Url::from_file_path(path) {
        matches |= &path_url == url;
    }

    matches
}

fn apply_edit(main_split: &mut LapceMainSplitData, path: &Path, edits: &[TextEdit]) {
    let doc = match main_split.open_docs.get(path) {
        Some(doc) => doc,
        None => return,
    };

    let edits = edits
        .iter()
        .map(|edit| {
            let selection = lapce_core::selection::Selection::region(
                doc.buffer().offset_of_position(&edit.range.start),
                doc.buffer().offset_of_position(&edit.range.end),
            );
            (selection, edit.new_text.as_str())
        })
        .collect::<Vec<_>>();

    main_split.edit(path, &edits, lapce_core::editor::EditType::Other);
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
