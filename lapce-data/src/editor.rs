use crate::command::LapceCommand;
use crate::command::LAPCE_COMMAND;
use crate::command::LAPCE_SAVE_FILE_AS;
use crate::command::{CommandExecuted, CommandKind};
use crate::completion::{CompletionData, CompletionStatus, Snippet};
use crate::config::Config;
use crate::data::{
    EditorDiagnostic, InlineFindDirection, LapceEditorData, LapceMainSplitData,
    SplitContent,
};
use crate::document::BufferContent;
use crate::document::Document;
use crate::document::LocalBufferKind;
use crate::hover::HoverData;
use crate::hover::HoverStatus;
use crate::keypress::KeyMap;
use crate::keypress::KeyPressFocus;
use crate::palette::PaletteData;
use crate::proxy::path_from_url;
use crate::{
    command::{EnsureVisiblePosition, LapceUICommand, LAPCE_UI_COMMAND},
    split::SplitMoveDirection,
};
use crate::{find::Find, split::SplitDirection};
use crate::{proxy::LapceProxy, source_control::SourceControlData};
use anyhow::{anyhow, Result};
use crossbeam_channel::{self, bounded};
use druid::piet::PietTextLayout;
use druid::piet::Svg;
use druid::FileDialogOptions;
use druid::Modifiers;
use druid::{
    piet::PietText, Command, Env, EventCtx, Point, Rect, Target, Vec2, WidgetId,
};
use druid::{ExtEventSink, MouseEvent};
use indexmap::IndexMap;
use lapce_core::buffer::{DiffLines, InvalLines};
use lapce_core::command::{
    EditCommand, FocusCommand, MotionModeCommand, MultiSelectionCommand,
};
use lapce_core::mode::{Mode, MotionMode};
pub use lapce_core::syntax::Syntax;
use lsp_types::CodeActionOrCommand;
use lsp_types::CompletionTextEdit;
use lsp_types::DocumentChangeOperation;
use lsp_types::DocumentChanges;
use lsp_types::OneOf;
use lsp_types::TextEdit;
use lsp_types::Url;
use lsp_types::WorkspaceEdit;
use lsp_types::{
    CodeActionResponse, CompletionItem, DiagnosticSeverity, GotoDefinitionResponse,
    Location, Position,
};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::thread;
use std::{collections::HashMap, sync::Arc};
use std::{iter::Iterator, path::PathBuf};
use std::{str::FromStr, time::Duration};
use xi_rope::{RopeDelta, Transformer};

pub struct LapceUI {}

#[derive(Copy, Clone)]
pub struct EditorCount(Option<usize>);

#[derive(Copy, Clone)]
pub enum EditorOperator {
    Delete(EditorCount),
    Yank(EditorCount),
}

#[derive(Clone, Debug)]
pub struct EditorLocation {
    pub path: PathBuf,
    pub position: Option<Position>,
    pub scroll_offset: Option<Vec2>,
    pub history: Option<String>,
}

pub struct LapceEditorBufferData {
    pub view_id: WidgetId,
    pub editor: Arc<LapceEditorData>,
    pub doc: Arc<Document>,
    pub completion: Arc<CompletionData>,
    pub hover: Arc<HoverData>,
    pub main_split: LapceMainSplitData,
    pub source_control: Arc<SourceControlData>,
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub proxy: Arc<LapceProxy>,
    pub command_keymaps: Arc<IndexMap<String, Vec<KeyMap>>>,
    pub config: Arc<Config>,
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

    pub fn get_code_actions(&self, ctx: &mut EventCtx) {
        if !self.doc.loaded() {
            return;
        }
        if !self.doc.content().is_file() {
            return;
        }
        if let BufferContent::File(path) = self.doc.content() {
            let path = path.clone();
            let offset = self.editor.cursor.offset();
            let prev_offset = self.doc.buffer().prev_code_boundary(offset);
            if self.doc.code_actions.get(&prev_offset).is_none() {
                let buffer_id = self.doc.id();
                let position = self.doc.buffer().offset_to_position(prev_offset);
                let rev = self.doc.rev();
                let event_sink = ctx.get_external_handle();
                self.proxy.get_code_actions(
                    buffer_id,
                    position,
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<CodeActionResponse>(res)
                            {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::UpdateCodeActions(
                                        path,
                                        rev,
                                        prev_offset,
                                        resp,
                                    ),
                                    Target::Auto,
                                );
                            }
                        }
                    }),
                );
            }
        }
    }

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

    /// Check if there are completions that are being rendered
    fn has_completions(&self) -> bool {
        self.completion.status != CompletionStatus::Inactive
            && self.completion.len() > 0
    }

    fn has_hover(&self) -> bool {
        self.hover.status != HoverStatus::Inactive && !self.hover.is_empty()
    }

    pub fn run_code_action(&mut self, action: &CodeActionOrCommand) {
        if let BufferContent::File(path) = &self.editor.content {
            match action {
                CodeActionOrCommand::Command(_cmd) => {}
                CodeActionOrCommand::CodeAction(action) => {
                    if let Some(edit) = action.edit.as_ref() {
                        if let Some(edits) = workspace_edits(edit) {
                            for (url, edits) in edits {
                                // TODO: Neither of these methods work for paths
                                // on different filesystems (i.e. windows and linux),
                                // as pathbuf is meant to represent a path on the host
                                let mut matches = false;
                                // This handles windows drive letters, which rust-url doesn't do.
                                if let Ok(url_path) = url.to_file_path() {
                                    matches |= &url_path == path;
                                }
                                // This is the previous check, to ensure this isn't a regression
                                if let Ok(path_url) = Url::from_file_path(path) {
                                    matches |= path_url == url;
                                }
                                if matches {
                                    let path = path.clone();
                                    let doc = self
                                        .main_split
                                        .open_docs
                                        .get_mut(&path)
                                        .unwrap();
                                    let edits: Vec<(
                                        lapce_core::selection::Selection,
                                        &str,
                                    )> = edits
                                        .iter()
                                        .map(|edit| {
                                            let selection =
                                            lapce_core::selection::Selection::region(
                                                doc.buffer().offset_of_position(
                                                    &edit.range.start,
                                                ),
                                                doc.buffer().offset_of_position(
                                                    &edit.range.end,
                                                ),
                                            );
                                            (selection, edit.new_text.as_str())
                                        })
                                        .collect();
                                    self.main_split.edit(
                                        &path,
                                        &edits,
                                        lapce_core::editor::EditType::Other,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn apply_completion_item(&mut self, item: &CompletionItem) -> Result<()> {
        let additional_edit: Option<Vec<_>> =
            item.additional_text_edits.as_ref().map(|edits| {
                edits
                    .iter()
                    .map(|edit| {
                        let selection = lapce_core::selection::Selection::region(
                            self.doc.buffer().offset_of_position(&edit.range.start),
                            self.doc.buffer().offset_of_position(&edit.range.end),
                        );
                        (selection, edit.new_text.as_str())
                    })
                    .collect::<Vec<(lapce_core::selection::Selection, &str)>>()
            });
        let additional_edit: Option<Vec<_>> =
            additional_edit.as_ref().map(|edits| {
                edits.iter().map(|(selection, c)| (selection, *c)).collect()
            });

        let text_format = item
            .insert_text_format
            .unwrap_or(lsp_types::InsertTextFormat::PlainText);
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
                        lsp_types::InsertTextFormat::PlainText => {
                            let (delta, inval_lines) = Arc::make_mut(&mut self.doc)
                                .do_raw_edit(
                                    &[
                                        &[(&selection, edit.new_text.as_str())][..],
                                        &additional_edit.unwrap_or_default()[..],
                                    ]
                                    .concat(),
                                    lapce_core::editor::EditType::InsertChars,
                                );
                            let selection = selection.apply_delta(
                                &delta,
                                true,
                                lapce_core::selection::InsertDrift::Default,
                            );
                            Arc::make_mut(&mut self.editor)
                                .cursor
                                .update_selection(self.doc.buffer(), selection);
                            self.apply_deltas(&[(delta, inval_lines)]);
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::Snippet => {
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let (delta, inval_lines) = Arc::make_mut(&mut self.doc)
                                .do_raw_edit(
                                    &[
                                        &[(&selection, text.as_str())][..],
                                        &additional_edit.unwrap_or_default()[..],
                                    ]
                                    .concat(),
                                    lapce_core::editor::EditType::InsertChars,
                                );
                            let selection = selection.apply_delta(
                                &delta,
                                true,
                                lapce_core::selection::InsertDrift::Default,
                            );

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer
                                .transform(start_offset.min(edit_start), false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.is_empty() {
                                Arc::make_mut(&mut self.editor)
                                    .cursor
                                    .update_selection(self.doc.buffer(), selection);
                                self.apply_deltas(&[(delta, inval_lines)]);
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
                            self.apply_deltas(&[(delta, inval_lines)]);
                            Arc::make_mut(&mut self.editor)
                                .add_snippet_placeholders(snippet_tabs);
                            return Ok(());
                        }
                    }
                }
                CompletionTextEdit::InsertAndReplace(_) => (),
            }
        }

        let offset = self.editor.cursor.offset();
        let start_offset = self.doc.buffer().prev_code_boundary(offset);
        let end_offset = self.doc.buffer().next_code_boundary(offset);
        let selection =
            lapce_core::selection::Selection::region(start_offset, end_offset);

        let (delta, inval_lines) = Arc::make_mut(&mut self.doc).do_raw_edit(
            &[
                &[(
                    &selection,
                    item.insert_text.as_deref().unwrap_or(item.label.as_str()),
                )][..],
                &additional_edit.unwrap_or_default()[..],
            ]
            .concat(),
            lapce_core::editor::EditType::InsertChars,
        );
        let selection = selection.apply_delta(
            &delta,
            true,
            lapce_core::selection::InsertDrift::Default,
        );
        Arc::make_mut(&mut self.editor)
            .cursor
            .update_selection(self.doc.buffer(), selection);
        self.apply_deltas(&[(delta, inval_lines)]);
        Ok(())
    }

    pub fn cancel_completion(&mut self) {
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
    }

    pub fn cancel_hover(&mut self) {
        let hover = Arc::make_mut(&mut self.hover);
        hover.cancel();
    }

    /// Update the displayed autocompletion box
    /// Sends a request to the LSP for completion information
    fn update_completion(
        &mut self,
        ctx: &mut EventCtx,
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
                let event_sink = ctx.get_external_handle();
                completion.request(
                    self.proxy.clone(),
                    completion.request_id,
                    self.doc.id(),
                    "".to_string(),
                    self.doc.buffer().offset_to_position(start_offset),
                    completion.id,
                    event_sink,
                );
            }

            if !completion.input_items.contains_key(&input) {
                let event_sink = ctx.get_external_handle();
                completion.request(
                    self.proxy.clone(),
                    completion.request_id,
                    self.doc.id(),
                    input,
                    self.doc.buffer().offset_to_position(offset),
                    completion.id,
                    event_sink,
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
        let event_sink = ctx.get_external_handle();
        completion.request(
            self.proxy.clone(),
            completion.request_id,
            self.doc.id(),
            "".to_string(),
            self.doc.buffer().offset_to_position(start_offset),
            completion.id,
            event_sink.clone(),
        );
        if !input.is_empty() {
            completion.request(
                self.proxy.clone(),
                completion.request_id,
                self.doc.id(),
                input,
                self.doc.buffer().offset_to_position(offset),
                completion.id,
                event_sink,
            );
        }
    }

    /// return true if there's existing hover and it's not changed
    pub fn check_hover(
        &mut self,
        _ctx: &mut EventCtx,
        offset: usize,
        is_inside: bool,
        within_scroll: bool,
    ) -> bool {
        let hover = Arc::make_mut(&mut self.hover);

        if hover.status != HoverStatus::Inactive {
            if !is_inside || !within_scroll {
                hover.cancel();
                return false;
            }

            let start_offset = self.doc.buffer().prev_code_boundary(offset);
            if self.doc.id() == hover.buffer_id && start_offset == hover.offset {
                return true;
            }

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
        hover.request(
            self.proxy.clone(),
            hover.request_id,
            self.doc.clone(),
            diagnostics,
            self.doc.buffer().offset_to_position(start_offset),
            hover.id,
            event_sink,
            self.config.clone(),
        );
    }

    fn initiate_diagnostics_offset(&mut self) {
        let doc = self.doc.clone();
        if let Some(diagnostics) = self.diagnostics_mut() {
            for diagnostic in diagnostics.iter_mut() {
                if diagnostic.range.is_none() {
                    diagnostic.range = Some((
                        doc.buffer()
                            .offset_of_position(&diagnostic.diagnostic.range.start),
                        doc.buffer()
                            .offset_of_position(&diagnostic.diagnostic.range.end),
                    ));
                }
            }
        }
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

    fn update_diagnostics_offset(&mut self, delta: &RopeDelta) {
        let doc = self.doc.clone();
        if let Some(diagnostics) = self.diagnostics_mut() {
            for diagnostic in diagnostics.iter_mut() {
                let mut transformer = Transformer::new(delta);
                let (start, end) = diagnostic.range.unwrap();
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );
                diagnostic.range = Some((new_start, new_end));
                if start != new_start {
                    diagnostic.diagnostic.range.start =
                        doc.buffer().offset_to_position(new_start);
                }
                if end != new_end {
                    diagnostic.diagnostic.range.end =
                        doc.buffer().offset_to_position(new_end);
                }
            }
        }
    }

    fn next_diff(&mut self, ctx: &mut EventCtx) {
        if let BufferContent::File(buffer_path) = self.doc.content() {
            if self.source_control.file_diffs.is_empty() {
                return;
            }
            let mut diff_files: Vec<(PathBuf, Vec<Position>)> = self
                .source_control
                .file_diffs
                .iter()
                .map(|(diff, _)| {
                    let path = diff.path();
                    let mut positions = Vec::new();
                    if let Some(doc) = self.main_split.open_docs.get(path) {
                        if let Some(history) = doc.get_history("head") {
                            for (i, change) in history.changes().iter().enumerate() {
                                match change {
                                    DiffLines::Left(_) => {
                                        if let Some(next) =
                                            history.changes().get(i + 1)
                                        {
                                            match next {
                                                DiffLines::Right(_) => {}
                                                DiffLines::Left(_) => {}
                                                DiffLines::Both(_, r) => {
                                                    positions.push(Position {
                                                        line: r.start as u32,
                                                        character: 0,
                                                    });
                                                }
                                                DiffLines::Skip(_, r) => {
                                                    positions.push(Position {
                                                        line: r.start as u32,
                                                        character: 0,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    DiffLines::Both(_, _) => {}
                                    DiffLines::Skip(_, _) => {}
                                    DiffLines::Right(r) => {
                                        positions.push(Position {
                                            line: r.start as u32,
                                            character: 0,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    if positions.is_empty() {
                        positions.push(Position {
                            line: 0,
                            character: 0,
                        });
                    }
                    (path.clone(), positions)
                })
                .collect();
            diff_files.sort();

            let offset = self.editor.cursor.offset();
            let position = self.doc.buffer().offset_to_position(offset);
            let (path, position) =
                next_in_file_diff_offset(position, buffer_path, &diff_files);
            let location = EditorLocation {
                path,
                position: Some(position),
                scroll_offset: None,
                history: Some("head".to_string()),
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
    }

    fn next_error(&mut self, ctx: &mut EventCtx) {
        if let BufferContent::File(buffer_path) = self.doc.content() {
            let mut file_diagnostics = self
                .main_split
                .diagnostics
                .iter()
                .filter_map(|(path, diagnostics)| {
                    //let buffer = self.get_buffer_from_path(ctx, ui_state, path);
                    let mut errors: Vec<Position> = diagnostics
                        .iter()
                        .filter_map(|d| {
                            let severity = d
                                .diagnostic
                                .severity
                                .unwrap_or(DiagnosticSeverity::Hint);
                            if severity != DiagnosticSeverity::Error {
                                return None;
                            }
                            Some(d.diagnostic.range.start)
                        })
                        .collect();
                    if errors.is_empty() {
                        None
                    } else {
                        errors.sort();
                        Some((path, errors))
                    }
                })
                .collect::<Vec<(&PathBuf, Vec<Position>)>>();
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
                LapceUICommand::JumpToLocation(None, location),
                Target::Auto,
            ));
        }
    }

    fn jump_location_forward(&mut self, ctx: &mut EventCtx) -> Option<()> {
        if self.editor.locations.is_empty() {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() - 1 {
            return None;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location += 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    fn jump_location_backward(&mut self, ctx: &mut EventCtx) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.doc);
            editor.current_location -= 1;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location -= 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, mods: Modifiers) {
        let line_height = self.config.editor.line_height as f64;
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
        let line_height = self.config.editor.line_height as f64;
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

    pub fn current_code_actions(&self) -> Option<&CodeActionResponse> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.doc.buffer().prev_code_boundary(offset);
        self.doc.code_actions.get(&prev_offset)
    }

    pub fn diagnostics(&self) -> Option<&Arc<Vec<EditorDiagnostic>>> {
        if let BufferContent::File(path) = self.doc.content() {
            self.main_split.diagnostics.get(path)
        } else {
            None
        }
    }

    pub fn diagnostics_mut(&mut self) -> Option<&mut Vec<EditorDiagnostic>> {
        if let BufferContent::File(path) = self.doc.content() {
            self.main_split.diagnostics.get_mut(path).map(Arc::make_mut)
        } else {
            None
        }
    }

    pub fn offset_of_mouse(
        &self,
        text: &mut PietText,
        pos: Point,
        config: &Config,
    ) -> usize {
        let (line, char_width) = if self.editor.code_lens {
            let (line, font_size) = if let Some(syntax) = self.doc.syntax() {
                let line = syntax.lens.line_of_height(pos.y.floor() as usize);
                let line_height = syntax.lens.height_of_line(line + 1)
                    - syntax.lens.height_of_line(line);

                let font_size = if line_height < config.editor.line_height {
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

            (line, config.char_width(text, font_size as f64))
        } else if let Some(compare) = self.editor.compare.as_ref() {
            let line = (pos.y / config.editor.line_height as f64).floor() as usize;
            let line = self.doc.history_actual_line_from_visual(compare, line);
            (line, config.editor_char_width(text))
        } else {
            let line = (pos.y / config.editor.line_height as f64).floor() as usize;
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
        config: &Config,
    ) {
        let (new_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            config.editor.font_size,
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
        config: &Config,
    ) {
        ctx.set_active(true);
        let (mouse_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            config.editor.font_size,
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
        config: &Config,
    ) {
        ctx.set_active(true);
        let (mouse_offset, _) = self.doc.offset_of_point(
            ctx.text(),
            self.get_mode(),
            mouse_event.pos,
            config.editor.font_size,
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

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines)]) {
        for (delta, _) in deltas {
            self.inactive_apply_delta(delta);
            self.update_snippet_offset(delta);
            self.update_diagnostics_offset(delta);
        }
    }

    fn save(&mut self, ctx: &mut EventCtx, exit: bool) {
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
            let format_on_save = self.config.editor.format_on_save;
            let path = path.clone();
            let proxy = self.proxy.clone();
            let buffer_id = self.doc.id();
            let rev = self.doc.rev();
            let event_sink = ctx.get_external_handle();
            let view_id = self.editor.view_id;
            let (sender, receiver) = bounded(1);
            thread::spawn(move || {
                proxy.get_document_formatting(
                    buffer_id,
                    Box::new(move |result| {
                        let _ = sender.send(result);
                    }),
                );

                let result =
                    receiver.recv_timeout(Duration::from_secs(1)).map_or_else(
                        |e| Err(anyhow!("{}", e)),
                        |v| v.map_err(|e| anyhow!("{:?}", e)),
                    );

                let exit = if exit { Some(view_id) } else { None };
                let cmd = if format_on_save {
                    LapceUICommand::DocumentFormatAndSave(path, rev, result, exit)
                } else {
                    LapceUICommand::DocumentSave(path, exit)
                };

                let _ =
                    event_sink.submit_command(LAPCE_UI_COMMAND, cmd, Target::Auto);
            });
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
            Arc::make_mut(&mut self.editor).save_jump_location(&self.doc);
        }
        Arc::make_mut(&mut self.editor).last_movement_new = movement.clone();

        let register = Arc::make_mut(&mut self.main_split.register);
        let doc = Arc::make_mut(&mut self.doc);
        doc.move_cursor(
            ctx.text(),
            &mut Arc::make_mut(&mut self.editor).cursor,
            movement,
            count.unwrap_or(1),
            mods.shift(),
            self.config.editor.font_size,
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
        self.cancel_hover();
        CommandExecuted::Yes
    }

    fn run_edit_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &EditCommand,
    ) -> CommandExecuted {
        let modal = self.config.lapce.modal && !self.editor.content.is_input();
        let doc = Arc::make_mut(&mut self.doc);
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

        self.update_completion(ctx, false);
        self.apply_deltas(&deltas);

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
                if self.has_hover() {
                    self.cancel_hover();
                }
            }
            SplitVertical => {
                self.main_split.split_editor(
                    ctx,
                    Arc::make_mut(&mut self.editor),
                    SplitDirection::Vertical,
                    &self.config,
                );
            }
            SplitHorizontal => {
                self.main_split.split_editor(
                    ctx,
                    Arc::make_mut(&mut self.editor),
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
                Arc::make_mut(&mut self.find).set_find(&word, false, false, true);
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
            GlobalSearchRefresh => {
                let tab_id = *self.main_split.tab_id;
                let pattern = self.doc.buffer().text().to_string();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSearch(pattern),
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
                    / self.config.editor.line_height as f64)
                    .ceil() as usize)
                    .max(self.doc.buffer().last_line());
                let end_line = ((self.editor.scroll_offset.y
                    + self.editor.size.borrow().height
                        / self.config.editor.line_height as f64)
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
                    let item = self.completion.current_item().to_owned();
                    self.cancel_completion();
                    if item.data.is_some() {
                        let view_id = self.editor.view_id;
                        let buffer_id = self.doc.id();
                        let rev = self.doc.rev();
                        let offset = self.editor.cursor.offset();
                        let event_sink = ctx.get_external_handle();
                        self.proxy.completion_resolve(
                            buffer_id,
                            item.clone(),
                            Box::new(move |result| {
                                let mut item = item.clone();
                                if let Ok(res) = result {
                                    if let Ok(i) =
                                        serde_json::from_value::<CompletionItem>(res)
                                    {
                                        item = i;
                                    }
                                };
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::ResolveCompletion(
                                        buffer_id,
                                        rev,
                                        offset,
                                        Box::new(item),
                                    ),
                                    Target::Widget(view_id),
                                );
                            }),
                        );
                    } else {
                        let _ = self.apply_completion_item(&item);
                    }
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
                    completion.next();
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
                    completion.previous();
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
            GotoDefinition => {
                let offset = self.editor.cursor.offset();
                let start_offset = self.doc.buffer().prev_code_boundary(offset);
                let start_position =
                    self.doc.buffer().offset_to_position(start_offset);
                let event_sink = ctx.get_external_handle();
                let buffer_id = self.doc.id();
                let position = self.doc.buffer().offset_to_position(offset);
                let proxy = self.proxy.clone();
                let editor_view_id = self.editor.view_id;
                self.proxy.get_definition(
                    offset,
                    buffer_id,
                    position,
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<GotoDefinitionResponse>(res)
                            {
                                if let Some(location) = match resp {
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
                                        _location_links,
                                    ) => None,
                                } {
                                    if location.range.start == start_position {
                                        proxy.get_references(
                                            buffer_id,
                                            position,
                                            Box::new(move |result| {
                                                let _ = process_get_references(
                                                    editor_view_id,
                                                    offset,
                                                    result,
                                                    event_sink,
                                                );
                                            }),
                                        );
                                    } else {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition(
                                                editor_view_id,
                                                offset,
                                                EditorLocation {
                                                    path: path_from_url(
                                                        &location.uri,
                                                    ),
                                                    position: Some(
                                                        location.range.start,
                                                    ),
                                                    scroll_offset: None,
                                                    history: None,
                                                },
                                            ),
                                            Target::Auto,
                                        );
                                    }
                                }
                            }
                        }
                    }),
                );
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
            NextDiff => {
                self.next_diff(ctx);
            }
            ToggleCodeLens => {
                let editor = Arc::make_mut(&mut self.editor);
                editor.code_lens = !editor.code_lens;
            }
            FormatDocument => {
                if let BufferContent::File(path) = self.doc.content() {
                    let path = path.clone();
                    let proxy = self.proxy.clone();
                    let buffer_id = self.doc.id();
                    let rev = self.doc.rev();
                    let event_sink = ctx.get_external_handle();
                    let (sender, receiver) = bounded(1);
                    thread::spawn(move || {
                        proxy.get_document_formatting(
                            buffer_id,
                            Box::new(move |result| {
                                let _ = sender.send(result);
                            }),
                        );

                        let result = receiver
                            .recv_timeout(Duration::from_secs(1))
                            .map_or_else(
                                |e| Err(anyhow!("{}", e)),
                                |v| v.map_err(|e| anyhow!("{:?}", e)),
                            );
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::DocumentFormat(path, rev, result),
                            Target::Auto,
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
                    Arc::make_mut(&mut self.find)
                        .set_find(&pattern, false, false, false);
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
                self.save(ctx, true);
            }
            Save => {
                self.save(ctx, false);
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
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
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
        self.doc
            .do_multi_selection(ctx.text(), cursor, cmd, &self.config);
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
            "modal_focus" => {
                (self.has_completions() && !self.config.lapce.modal)
                    || self.has_hover()
                    || self.is_palette()
            }
            _ => false,
        }
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            self.initiate_diagnostics_offset();
            let doc = Arc::make_mut(&mut self.doc);
            let cursor = &mut Arc::make_mut(&mut self.editor).cursor;
            let deltas = doc.do_insert(cursor, c);

            self.update_completion(ctx, false);
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
        self.initiate_diagnostics_offset();
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
    pub rect: Rect,
    pub close_rect: Rect,
    pub text_layout: PietTextLayout,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

fn next_in_file_diff_offset(
    position: Position,
    path: &Path,
    file_diffs: &[(PathBuf, Vec<Position>)],
) -> (PathBuf, Position) {
    for (current_path, positions) in file_diffs {
        if path == current_path {
            for diff_position in positions {
                if diff_position.line > position.line
                    || (diff_position.line == position.line
                        && diff_position.character > position.character)
                {
                    return ((*current_path).clone(), *diff_position);
                }
            }
        }
        if current_path > path {
            return ((*current_path).clone(), positions[0]);
        }
    }
    ((file_diffs[0].0).clone(), file_diffs[0].1[0])
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
    editor_view_id: WidgetId,
    offset: usize,
    result: Result<Value, Value>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let res = result.map_err(|e| anyhow!("{:?}", e))?;
    let locations: Vec<Location> = serde_json::from_value(res)?;
    if locations.is_empty() {
        return Ok(());
    }
    if locations.len() == 1 {
        let location = &locations[0];
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::GotoReference(
                editor_view_id,
                offset,
                EditorLocation {
                    path: path_from_url(&location.uri),
                    position: Some(location.range.start),
                    scroll_offset: None,
                    history: None,
                },
            ),
            Target::Auto,
        );
    }
    let _ = event_sink.submit_command(
        LAPCE_UI_COMMAND,
        LapceUICommand::PaletteReferences(offset, locations),
        Target::Auto,
    );
    Ok(())
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
