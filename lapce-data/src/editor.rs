use crate::buffer::get_word_property;
use crate::buffer::matching_char;
use crate::buffer::{
    has_unmatched_pair, BufferContent, DiffLines, EditType, LocalBufferKind,
};
use crate::buffer::{matching_pair_direction, Buffer};
use crate::command::CommandExecuted;
use crate::command::CommandTarget;
use crate::command::LapceCommandNew;
use crate::command::LAPCE_NEW_COMMAND;
use crate::completion::{CompletionData, CompletionStatus, Snippet};
use crate::config::Config;
use crate::data::MotionMode;
use crate::data::Register;
use crate::data::RegisterKind;
use crate::data::{
    EditorDiagnostic, InlineFindDirection, LapceEditorData, LapceMainSplitData,
    RegisterData, SplitContent,
};
use crate::hover::HoverData;
use crate::hover::HoverStatus;
use crate::movement::InsertDrift;
use crate::proxy::path_from_url;
use crate::{buffer::WordProperty, movement::CursorMode};
use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    movement::{Movement, SelRegion, Selection},
    split::SplitMoveDirection,
    state::Mode,
    state::VisualMode,
};
use crate::{find::Find, split::SplitDirection};
use crate::{keypress::KeyPressFocus, movement::Cursor};
use crate::{proxy::LapceProxy, source_control::SourceControlData};
use anyhow::{anyhow, Result};
use crossbeam_channel::{self, bounded};
use druid::piet::PietTextLayout;
use druid::piet::Svg;
use druid::Modifiers;
use druid::{
    piet::PietText, Command, Env, EventCtx, Point, Rect, Size, Target, Vec2,
    WidgetId,
};
use druid::{Application, ExtEventSink, MouseEvent};
pub use lapce_core::syntax::Syntax;
use lapce_rpc::buffer::BufferId;
use lsp_types::CompletionTextEdit;
use lsp_types::{
    CodeActionResponse, CompletionItem, DiagnosticSeverity, GotoDefinitionResponse,
    Location, Position,
};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashSet;
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

#[derive(Clone)]
pub struct EditorUIState {
    pub buffer_id: BufferId,
    pub cursor: (usize, usize),
    pub mode: Mode,
    pub visual_mode: VisualMode,
    pub selection: Selection,
    pub selection_start_line: usize,
    pub selection_end_line: usize,
}

#[derive(Clone)]
pub struct EditorState {
    pub editor_id: WidgetId,
    pub view_id: WidgetId,
    pub split_id: WidgetId,
    pub tab_id: WidgetId,
    pub buffer_id: Option<BufferId>,
    pub char_width: f64,
    pub width: f64,
    pub height: f64,
    pub selection: Selection,
    pub scroll_offset: Vec2,
    pub scroll_size: Size,
    pub view_size: Size,
    pub gutter_width: f64,
    pub header_height: f64,
    pub locations: Vec<EditorLocation>,
    pub current_location: usize,
    pub saved_buffer_id: BufferId,
    pub saved_selection: Selection,
    pub saved_scroll_offset: Vec2,

    #[allow(dead_code)]
    last_movement: Movement,
}

#[derive(Clone, Debug)]
pub struct EditorLocationNew {
    pub path: PathBuf,
    pub position: Option<Position>,
    pub scroll_offset: Option<Vec2>,
    pub hisotry: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EditorLocation {
    pub path: String,
    pub offset: usize,
    pub scroll_offset: Option<Vec2>,
}

pub struct LapceEditorBufferData {
    pub view_id: WidgetId,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<Buffer>,
    pub completion: Arc<CompletionData>,
    pub hover: Arc<HoverData>,
    pub main_split: LapceMainSplitData,
    pub source_control: Arc<SourceControlData>,
    pub find: Arc<Find>,
    pub proxy: Arc<LapceProxy>,
    pub config: Arc<Config>,
    pub register: Arc<Register>,
}

impl LapceEditorBufferData {
    pub fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
        let cursor_offset = self.editor.cursor.offset();
        if self.buffer.cursor_offset != cursor_offset
            || self.buffer.scroll_offset != scroll_offset
        {
            let buffer = Arc::make_mut(&mut self.buffer);
            buffer.cursor_offset = cursor_offset;
            buffer.scroll_offset = scroll_offset;
        }
    }

    fn inline_find(&mut self, direction: InlineFindDirection, c: &str) {
        let offset = self.editor.cursor.offset();
        let line = self.buffer.line_of_offset(offset);
        let line_content = self.buffer.line_content(line);
        let line_start_offset = self.buffer.offset_of_line(line);
        let index = offset - line_start_offset;
        if let Some(new_index) = match direction {
            InlineFindDirection::Left => line_content[..index].rfind(c),
            InlineFindDirection::Right => {
                if index + 1 >= line_content.len() {
                    None
                } else {
                    let index = index
                        + self.buffer.next_grapheme_offset(
                            offset,
                            1,
                            self.buffer.offset_line_end(offset, false),
                        )
                        - offset;
                    line_content[index..].find(c).map(|i| i + index)
                }
            }
        } {
            self.do_move(
                &Movement::Offset(new_index + line_start_offset),
                1,
                Modifiers::empty(),
            );
        }
    }

    pub fn get_code_actions(&self, ctx: &mut EventCtx) {
        if !self.buffer.loaded {
            return;
        }
        if self.buffer.local {
            return;
        }
        if let BufferContent::File(path) = &self.buffer.content {
            let path = path.clone();
            let offset = self.editor.cursor.offset();
            let prev_offset = self.buffer.prev_code_boundary(offset);
            if self.buffer.code_actions.get(&prev_offset).is_none() {
                let buffer_id = self.buffer.id;
                let position = self
                    .buffer
                    .offset_to_position(prev_offset, self.config.editor.tab_width);
                let rev = self.buffer.rev;
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

    fn set_motion_mode(&mut self, mode: MotionMode) {
        if let Some(m) = &self.editor.motion_mode {
            if m == &mode {
                let offset = self.editor.cursor.offset();
                self.execute_motion_mode(offset, offset, true);
            }
            Arc::make_mut(&mut self.editor).motion_mode = None;
        } else {
            Arc::make_mut(&mut self.editor).motion_mode = Some(mode);
        }
    }

    fn format_start_end(
        &self,
        start: usize,
        end: usize,
        is_vertical: bool,
    ) -> (usize, usize) {
        if is_vertical {
            let start_line = self.buffer.line_of_offset(start.min(end));
            let end_line = self.buffer.line_of_offset(end.max(start));
            let start = self.buffer.offset_of_line(start_line);
            let end = self.buffer.offset_of_line(end_line + 1);
            (start, end)
        } else {
            let s = start.min(end);
            let e = start.max(end);
            (s, e)
        }
    }

    fn add_register(
        &mut self,
        start: usize,
        end: usize,
        is_vertical: bool,
        kind: RegisterKind,
    ) {
        let content = self.buffer.slice_to_cow(start..end).to_string();
        let data = RegisterData {
            content,
            mode: if is_vertical {
                VisualMode::Linewise
            } else {
                VisualMode::Normal
            },
        };
        let register = Arc::make_mut(&mut self.register);
        register.add(kind, data);
    }

    fn execute_motion_mode(&mut self, start: usize, end: usize, is_vertical: bool) {
        if let Some(mode) = &self.editor.motion_mode {
            match mode {
                MotionMode::Delete => {
                    let (start, end) =
                        self.format_start_end(start, end, is_vertical);
                    self.add_register(start, end, is_vertical, RegisterKind::Yank);
                    let selection = Selection::region(start, end);
                    let delta =
                        self.edit(&[(&selection, "")], true, EditType::Delete);
                    Arc::make_mut(&mut self.editor).cursor.apply_delta(&delta);
                }
                MotionMode::Yank => {
                    let (start, end) =
                        self.format_start_end(start, end, is_vertical);
                    self.add_register(start, end, is_vertical, RegisterKind::Yank);
                }
                MotionMode::Indent => {
                    let selection = Selection::region(start, end);
                    self.indent_line(selection);
                }
                MotionMode::Outdent => {
                    let selection = Selection::region(start, end);
                    self.outdent_line(selection);
                }
            }
        }
    }

    fn do_move(&mut self, movement: &Movement, count: usize, mods: Modifiers) {
        if movement.is_jump() && movement != &self.editor.last_movement {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer, self.config.editor.tab_width);
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.last_movement = movement.clone();
        let compare = editor.compare.clone();
        match &self.editor.cursor.mode {
            &CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    offset,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                    self.editor.code_lens,
                    compare,
                    &self.config,
                );

                if self.editor.motion_mode.is_some() {
                    let (start, end) = match movement {
                        Movement::EndOfLine | Movement::WordEndForward => {
                            let (end, _) = self.buffer.move_offset(
                                new_offset,
                                None,
                                1,
                                &Movement::Right,
                                Mode::Insert,
                                false,
                                None,
                                &self.config,
                            );
                            (offset, end)
                        }
                        Movement::MatchPairs => {
                            if new_offset > offset {
                                let (end, _) = self.buffer.move_offset(
                                    new_offset,
                                    None,
                                    1,
                                    &Movement::Right,
                                    Mode::Insert,
                                    false,
                                    None,
                                    &self.config,
                                );
                                (offset, end)
                            } else {
                                let (start, _) = self.buffer.move_offset(
                                    offset,
                                    None,
                                    1,
                                    &Movement::Right,
                                    Mode::Insert,
                                    false,
                                    None,
                                    &self.config,
                                );
                                (start, new_offset)
                            }
                        }
                        _ => (offset, new_offset),
                    };
                    self.execute_motion_mode(start, end, movement.is_vertical());
                } else {
                    let editor = Arc::make_mut(&mut self.editor);
                    editor.cursor.mode = CursorMode::Normal(new_offset);
                    editor.cursor.horiz = Some(horiz);
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    *end,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                    self.editor.code_lens,
                    compare,
                    &self.config,
                );
                let start = *start;
                let mode = *mode;
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Insert(selection) => {
                let selection = self.buffer.update_selection(
                    selection,
                    count,
                    movement,
                    Mode::Insert,
                    mods.shift(),
                    self.editor.code_lens,
                    compare,
                    &self.config,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    fn indent_line(&mut self, selection: Selection) {
        let indent = self.buffer.indent_unit();
        let mut edits = Vec::new();

        let mut lines = HashSet::new();
        for region in selection.regions() {
            let start_line = self.buffer.line_of_offset(region.min());
            let mut end_line = self.buffer.line_of_offset(region.max());
            if end_line > start_line {
                let end_line_start = self.buffer.offset_of_line(end_line);
                if end_line_start == region.max() {
                    end_line -= 1;
                }
            }
            for line in start_line..end_line + 1 {
                if lines.contains(&line) {
                    continue;
                }
                lines.insert(line);
                let line_content = self.buffer.line_content(line);
                if line_content == "\n" || line_content == "\r\n" {
                    continue;
                }
                let nonblank = self.buffer.first_non_blank_character_on_line(line);
                if indent.starts_with('\t') {
                    edits.push((Selection::caret(nonblank), indent.to_string()));
                } else {
                    let (_, col) = self
                        .buffer
                        .offset_to_line_col(nonblank, self.config.editor.tab_width);
                    let indent = " ".repeat(indent.len() - col % indent.len());
                    edits.push((Selection::caret(nonblank), indent));
                }
            }
        }

        let edits = edits
            .iter()
            .map(|(selection, s)| (selection, s.as_str()))
            .collect::<Vec<(&Selection, &str)>>();
        let delta = self.edit(&edits, true, EditType::InsertChars);
        Arc::make_mut(&mut self.editor).cursor.apply_delta(&delta);
    }

    fn outdent_line(&mut self, selection: Selection) {
        let indent = self.buffer.indent_unit();
        let mut edits = Vec::new();

        let mut lines = HashSet::new();
        for region in selection.regions() {
            let start_line = self.buffer.line_of_offset(region.min());
            let mut end_line = self.buffer.line_of_offset(region.max());
            if end_line > start_line {
                let end_line_start = self.buffer.offset_of_line(end_line);
                if end_line_start == region.max() {
                    end_line -= 1;
                }
            }
            for line in start_line..end_line + 1 {
                if lines.contains(&line) {
                    continue;
                }
                lines.insert(line);
                let line_content = self.buffer.line_content(line);
                if line_content == "\n" || line_content == "\r\n" {
                    continue;
                }
                let nonblank = self.buffer.first_non_blank_character_on_line(line);
                let (_, col) = self
                    .buffer
                    .offset_to_line_col(nonblank, self.config.editor.tab_width);
                if col == 0 {
                    continue;
                }

                if indent.starts_with('\t') {
                    edits.push((
                        Selection::region(nonblank - 1, nonblank),
                        "".to_string(),
                    ));
                } else {
                    let r = col % indent.len();
                    let r = if r == 0 { indent.len() } else { r };
                    edits.push((
                        Selection::region(nonblank - r, nonblank),
                        "".to_string(),
                    ));
                }
            }
        }

        let edits = edits
            .iter()
            .map(|(selection, s)| (selection, s.as_str()))
            .collect::<Vec<(&Selection, &str)>>();
        let delta = self.edit(&edits, true, EditType::InsertChars);
        Arc::make_mut(&mut self.editor).cursor.apply_delta(&delta);
    }

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id
                && self.buffer.content == editor.content
            {
                Arc::make_mut(editor).cursor.apply_delta(delta);
            }
        }
    }

    /// Check if there are completions that are being rendered
    fn has_completions(&self) -> bool {
        self.completion.status != CompletionStatus::Inactive
            && self.completion.len() > 0
    }

    fn has_hover(&self) -> bool {
        self.hover.status != HoverStatus::Inactive && !self.hover.is_empty()
    }

    pub fn apply_completion_item(&mut self, item: &CompletionItem) -> Result<()> {
        let additional_edit: Option<Vec<_>> =
            item.additional_text_edits.as_ref().map(|edits| {
                edits
                    .iter()
                    .map(|edit| {
                        let selection = Selection::region(
                            self.buffer.offset_of_position(
                                &edit.range.start,
                                self.config.editor.tab_width,
                            ),
                            self.buffer.offset_of_position(
                                &edit.range.end,
                                self.config.editor.tab_width,
                            ),
                        );
                        (selection, edit.new_text.clone())
                    })
                    .collect::<Vec<(Selection, String)>>()
            });
        let additioal_edit: Option<Vec<_>> = additional_edit.as_ref().map(|edits| {
            edits
                .iter()
                .map(|(selection, c)| (selection, c.as_str()))
                .collect()
        });

        let text_format = item
            .insert_text_format
            .unwrap_or(lsp_types::InsertTextFormat::PlainText);
        if let Some(edit) = &item.text_edit {
            match edit {
                CompletionTextEdit::Edit(edit) => {
                    let offset = self.editor.cursor.offset();
                    let start_offset = self.buffer.prev_code_boundary(offset);
                    let end_offset = self.buffer.next_code_boundary(offset);
                    let edit_start = self.buffer.offset_of_position(
                        &edit.range.start,
                        self.config.editor.tab_width,
                    );
                    let edit_end = self.buffer.offset_of_position(
                        &edit.range.end,
                        self.config.editor.tab_width,
                    );
                    let selection = Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PlainText => {
                            let delta = self.edit(
                                &[
                                    &[(&selection, edit.new_text.as_str())][..],
                                    &additioal_edit.unwrap_or_default()[..],
                                ]
                                .concat(),
                                true,
                                EditType::InsertChars,
                            );
                            let selection = selection.apply_delta(
                                &delta,
                                true,
                                InsertDrift::Default,
                            );
                            self.set_cursor_after_change(selection);
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::Snippet => {
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let delta = self.edit(
                                &[
                                    &[(&selection, text.as_str())][..],
                                    &additioal_edit.unwrap_or_default()[..],
                                ]
                                .concat(),
                                true,
                                EditType::InsertChars,
                            );
                            let selection = selection.apply_delta(
                                &delta,
                                true,
                                InsertDrift::Default,
                            );

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer
                                .transform(start_offset.min(edit_start), false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.is_empty() {
                                self.set_cursor_after_change(selection);
                                return Ok(());
                            }

                            let mut selection = Selection::new();
                            let (_tab, (start, end)) = &snippet_tabs[0];
                            let region = SelRegion::new(*start, *end, None);
                            selection.add_region(region);
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
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
        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let selection = Selection::region(start_offset, end_offset);

        let delta = self.edit(
            &[
                &[(
                    &selection,
                    item.insert_text.as_deref().unwrap_or(item.label.as_str()),
                )][..],
                &additioal_edit.unwrap_or_default()[..],
            ]
            .concat(),
            true,
            EditType::InsertChars,
        );
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        self.set_cursor_after_change(selection);
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

    fn update_completion(&mut self, ctx: &mut EventCtx) {
        if self.get_mode() != Mode::Insert {
            return;
        }
        if !self.buffer.loaded {
            return;
        }
        if self.buffer.local {
            return;
        }
        let offset = self.editor.cursor.offset();
        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let input = self
            .buffer
            .slice_to_cow(start_offset..end_offset)
            .to_string();
        let char = if start_offset == 0 {
            "".to_string()
        } else {
            self.buffer
                .slice_to_cow(start_offset - 1..start_offset)
                .to_string()
        };
        let completion = Arc::make_mut(&mut self.completion);
        if input.is_empty() && char != "." && char != ":" {
            completion.cancel();
            return;
        }

        if completion.status != CompletionStatus::Inactive
            && completion.offset == start_offset
            && completion.buffer_id == self.buffer.id
        {
            completion.update_input(input.clone());

            if !completion.input_items.contains_key("") {
                let event_sink = ctx.get_external_handle();
                completion.request(
                    self.proxy.clone(),
                    completion.request_id,
                    self.buffer.id,
                    "".to_string(),
                    self.buffer.offset_to_position(
                        start_offset,
                        self.config.editor.tab_width,
                    ),
                    completion.id,
                    event_sink,
                );
            }

            if !completion.input_items.contains_key(&input) {
                let event_sink = ctx.get_external_handle();
                completion.request(
                    self.proxy.clone(),
                    completion.request_id,
                    self.buffer.id,
                    input,
                    self.buffer
                        .offset_to_position(offset, self.config.editor.tab_width),
                    completion.id,
                    event_sink,
                );
            }

            return;
        }

        completion.buffer_id = self.buffer.id;
        completion.offset = start_offset;
        completion.input = input.clone();
        completion.status = CompletionStatus::Started;
        completion.input_items.clear();
        completion.request_id += 1;
        let event_sink = ctx.get_external_handle();
        completion.request(
            self.proxy.clone(),
            completion.request_id,
            self.buffer.id,
            "".to_string(),
            self.buffer
                .offset_to_position(start_offset, self.config.editor.tab_width),
            completion.id,
            event_sink.clone(),
        );
        if !input.is_empty() {
            completion.request(
                self.proxy.clone(),
                completion.request_id,
                self.buffer.id,
                input,
                self.buffer
                    .offset_to_position(offset, self.config.editor.tab_width),
                completion.id,
                event_sink,
            );
        }
    }

    pub fn update_hover(&mut self, ctx: &mut EventCtx, offset: usize) {
        if !self.buffer.loaded {
            return;
        }

        if self.buffer.local {
            return;
        }

        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let input = self.buffer.slice_to_cow(start_offset..end_offset);
        if input.trim().is_empty() {
            return;
        }

        let mut hover = Arc::make_mut(&mut self.hover);

        if hover.status != HoverStatus::Inactive
            && hover.offset == start_offset
            && hover.buffer_id == self.buffer.id
        {
            // We're hovering over the same location, but are trying to update
            return;
        }

        hover.buffer_id = self.buffer.id;
        hover.offset = start_offset;
        hover.status = HoverStatus::Started;
        Arc::make_mut(&mut hover.items).clear();
        hover.request_id += 1;

        let event_sink = ctx.get_external_handle();
        hover.request(
            self.proxy.clone(),
            hover.request_id,
            self.buffer.id,
            self.buffer
                .offset_to_position(start_offset, self.config.editor.tab_width),
            hover.id,
            event_sink,
        );
    }

    pub fn update_global_search(&self, ctx: &mut EventCtx, pattern: String) {
        let tab_id = *self.main_split.tab_id;
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateSearch(pattern.to_string()),
            Target::Widget(tab_id),
        ));
        if pattern.is_empty() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::GlobalSearchResult(
                    pattern,
                    Arc::new(HashMap::new()),
                ),
                Target::Widget(tab_id),
            ));
        } else {
            let event_sink = ctx.get_external_handle();
            self.proxy.global_search(
                pattern.clone(),
                Box::new(move |result| {
                    if let Ok(matches) = result {
                        if let Ok(matches) = serde_json::from_value::<
                            HashMap<PathBuf, Vec<(usize, (usize, usize), String)>>,
                        >(matches)
                        {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::GlobalSearchResult(
                                    pattern,
                                    Arc::new(matches),
                                ),
                                Target::Widget(tab_id),
                            );
                        }
                    }
                }),
            )
        }
    }

    fn insert_tab(&mut self) {
        if let CursorMode::Insert(selection) = &self.editor.cursor.mode {
            let indent = self.buffer.indent_unit();
            let mut edits = Vec::new();
            for region in selection.regions() {
                if region.is_caret() {
                    if indent.starts_with('\t') {
                        edits.push((
                            Selection::caret(region.start),
                            indent.to_string(),
                        ));
                    } else {
                        let (_, col) = self.buffer.offset_to_line_col(
                            region.start,
                            self.config.editor.tab_width,
                        );
                        let indent = " ".repeat(indent.len() - col % indent.len());
                        edits.push((Selection::caret(region.start), indent));
                    }
                } else {
                    let start_line = self.buffer.line_of_offset(region.min());
                    let end_line = self.buffer.line_of_offset(region.max());
                    for line in start_line..end_line + 1 {
                        let offset =
                            self.buffer.first_non_blank_character_on_line(line);
                        if indent.starts_with('\t') {
                            edits.push((
                                Selection::caret(offset),
                                indent.to_string(),
                            ));
                        } else {
                            let (_, col) = self.buffer.offset_to_line_col(
                                offset,
                                self.config.editor.tab_width,
                            );
                            let indent =
                                " ".repeat(indent.len() - col % indent.len());
                            edits.push((Selection::caret(offset), indent));
                        }
                    }
                }
            }

            let edits = edits
                .iter()
                .map(|(selection, s)| (selection, s.as_str()))
                .collect::<Vec<(&Selection, &str)>>();
            let delta = self.edit(&edits, true, EditType::InsertChars);
            Arc::make_mut(&mut self.editor).cursor.apply_delta(&delta);
        }
    }

    fn insert_new_line(&mut self, ctx: &mut EventCtx, selection: Selection) {
        match &self.buffer.content {
            BufferContent::File(_) => {}
            BufferContent::Value(_name) => {
                return;
            }
            BufferContent::Local(local) => match local {
                LocalBufferKind::Keymap => {
                    let tab_id = *self.main_split.tab_id;
                    let pattern = self.buffer.rope.to_string();
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateKeymapsFilter(pattern),
                        Target::Widget(tab_id),
                    ));
                    return;
                }
                LocalBufferKind::Settings => {
                    let tab_id = *self.main_split.tab_id;
                    let pattern = self.buffer.rope.to_string();
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSettingsFilter(pattern),
                        Target::Widget(tab_id),
                    ));
                    return;
                }
                LocalBufferKind::Search => {
                    let pattern = self.buffer.rope.to_string();
                    if let Some(parent_view_id) = self.editor.parent_view_id {
                        ctx.submit_command(Command::new(
                            LAPCE_NEW_COMMAND,
                            LapceCommandNew {
                                cmd: LapceCommand::SearchForward.to_string(),
                                data: None,
                                palette_desc: None,
                                target: CommandTarget::Focus,
                            },
                            Target::Widget(parent_view_id),
                        ));
                    } else {
                        self.update_global_search(ctx, pattern);
                    }
                    return;
                }
                LocalBufferKind::FilePicker => {
                    let pwd = self.buffer.rope.to_string();
                    let pwd = PathBuf::from(pwd);
                    let tab_id = *self.main_split.tab_id;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdatePickerPwd(pwd),
                        Target::Widget(tab_id),
                    ));
                    return;
                }
                LocalBufferKind::SourceControl | LocalBufferKind::Empty => {}
            },
        }

        let mut edits = Vec::new();
        let mut extra_edits = Vec::new();
        let mut shift = 0i32;
        for region in selection.regions() {
            let offset = region.max();
            let line = self.buffer.line_of_offset(offset);
            let line_start = self.buffer.offset_of_line(line);
            let line_end = self.buffer.line_end_offset(line, true);
            let line_indent = self.buffer.indent_on_line(line);
            let first_half =
                self.buffer.slice_to_cow(line_start..offset).to_string();
            let second_half = self.buffer.slice_to_cow(offset..line_end).to_string();

            let indent = if has_unmatched_pair(&first_half) {
                format!("{}    ", line_indent)
            } else {
                let next_line_indent = self.buffer.indent_on_line(line + 1);
                if next_line_indent.len() > line_indent.len() {
                    next_line_indent
                } else {
                    line_indent.clone()
                }
            };

            let selection = Selection::region(region.min(), region.max());
            let content = format!("{}{}", "\n", indent);

            shift -= (region.max() - region.min()) as i32;
            shift += content.len() as i32;

            edits.push((selection, content));

            for c in first_half.chars().rev() {
                if c != ' ' {
                    if let Some(pair_start) = matching_pair_direction(c) {
                        if pair_start {
                            if let Some(c) = matching_char(c) {
                                if second_half.trim().starts_with(&c.to_string()) {
                                    let selection = Selection::caret(
                                        (region.max() as i32 + shift) as usize,
                                    );
                                    let content = format!("{}{}", "\n", line_indent);
                                    extra_edits.push((selection.clone(), content));
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }

        let edits = edits
            .iter()
            .map(|(selection, s)| (selection, s.as_str()))
            .collect::<Vec<(&Selection, &str)>>();
        let delta = self.edit(&edits, true, EditType::InsertNewline);
        let mut selection =
            selection.apply_delta(&delta, true, InsertDrift::Default);

        if !extra_edits.is_empty() {
            let edits = extra_edits
                .iter()
                .map(|(selection, s)| (selection, s.as_str()))
                .collect::<Vec<(&Selection, &str)>>();
            let delta = self.edit(&edits, true, EditType::InsertNewline);
            selection = selection.apply_delta(&delta, false, InsertDrift::Default);
        }

        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor.mode = CursorMode::Insert(selection);
        editor.cursor.horiz = None;
    }

    fn set_cursor_after_change(&mut self, selection: Selection) {
        match self.editor.cursor.mode {
            CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                let offset = selection.min_offset();
                let offset = self.buffer.offset_line_end(offset, false).min(offset);
                self.set_cursor(Cursor::new(CursorMode::Normal(offset), None));
            }
            CursorMode::Insert(_) => {
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    fn paste(&mut self, ctx: &mut EventCtx, data: &RegisterData) {
        match data.mode {
            VisualMode::Normal => {
                Arc::make_mut(&mut self.editor).snippet = None;
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line_end = self.buffer.offset_line_end(offset, true);
                        let offset = (offset + 1).min(line_end);
                        Selection::caret(offset)
                    }
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                };
                let after =
                    self.editor.cursor.is_insert() || !data.content.contains('\n');
                let delta = self.edit(
                    &[(&selection, &data.content)],
                    after,
                    EditType::InsertChars,
                );
                let selection =
                    selection.apply_delta(&delta, after, InsertDrift::Default);
                if !after {
                    self.set_cursor_after_change(selection);
                } else {
                    match self.editor.cursor.mode {
                        CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                            let offset = self.buffer.prev_grapheme_offset(
                                selection.min_offset(),
                                1,
                                0,
                            );
                            self.set_cursor(Cursor::new(
                                CursorMode::Normal(offset),
                                None,
                            ));
                        }
                        CursorMode::Insert { .. } => {
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                        }
                    }
                }
            }
            VisualMode::Linewise | VisualMode::Blockwise => {
                let (selection, content) = match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line = self.buffer.line_of_offset(*offset);
                        let offset = self.buffer.offset_of_line(line + 1);
                        (Selection::caret(offset), data.content.clone())
                    }
                    CursorMode::Insert(selection) => {
                        let mut selection = selection.clone();
                        for region in selection.regions_mut() {
                            if region.is_caret() {
                                let line = self.buffer.line_of_offset(region.start);
                                let start = self.buffer.offset_of_line(line);
                                region.start = start;
                                region.end = start;
                            }
                        }
                        (selection, data.content.clone())
                    }
                    CursorMode::Visual { mode, .. } => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );
                        let data = match mode {
                            VisualMode::Linewise => data.content.clone(),
                            _ => "\n".to_string() + &data.content,
                        };
                        (selection, data)
                    }
                };
                let delta = self.edit(
                    &[(&selection, &content)],
                    self.editor.cursor.is_insert(),
                    EditType::InsertChars,
                );
                let selection = selection.apply_delta(
                    &delta,
                    self.editor.cursor.is_insert(),
                    InsertDrift::Default,
                );
                match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset = if self.editor.cursor.is_visual() {
                            offset + 1
                        } else {
                            offset
                        };
                        let line = self.buffer.line_of_offset(offset);
                        let offset =
                            self.buffer.first_non_blank_character_on_line(line);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                }
            }
        }

        self.update_completion(ctx);
    }

    fn check_selection_history(&mut self) {
        if self.editor.content != self.editor.selection_history.content
            || self.buffer.rev != self.editor.selection_history.rev
        {
            let editor = Arc::make_mut(&mut self.editor);
            editor.selection_history.content = editor.content.clone();
            editor.selection_history.rev = self.buffer.rev;
            editor.selection_history.selections.clear();
        }
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        self.check_selection_history();
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor.clone();
        if let CursorMode::Insert(selection) = cursor.mode {
            editor.selection_history.selections.push_back(selection);
        }
    }

    fn jump_to_nearest_delta(&mut self, delta: &RopeDelta) {
        let mut transformer = Transformer::new(delta);

        let offset = self.editor.cursor.offset();
        let offset = transformer.transform(offset, false);
        let (ins, del) = delta.clone().factor();
        let ins = ins.transform_shrink(&del);
        for el in ins.els.iter() {
            match el {
                xi_rope::DeltaElement::Copy(b, e) => {
                    // if b == e, ins.inserted_subset() will panic
                    if b == e {
                        return;
                    }
                }
                xi_rope::DeltaElement::Insert(_) => {}
            }
        }
        let mut positions = ins
            .inserted_subset()
            .complement_iter()
            .map(|s| s.1)
            .collect::<Vec<usize>>();
        positions.append(
            &mut del
                .complement_iter()
                .map(|s| transformer.transform(s.1, false))
                .collect::<Vec<usize>>(),
        );
        positions.sort_by_key(|p| {
            let p = *p as i32 - offset as i32;
            if p > 0 {
                p as usize
            } else {
                -p as usize
            }
        });
        if let Some(new_offset) = positions.get(0) {
            let selection = Selection::caret(*new_offset);
            self.set_cursor_after_change(selection);
        }
    }

    fn initiate_diagnositcs_offset(&mut self) {
        let buffer = self.buffer.clone();
        let tab_width = self.config.editor.tab_width;
        if let Some(diagnostics) = self.diagnostics_mut() {
            for diagnostic in diagnostics.iter_mut() {
                if diagnostic.range.is_none() {
                    diagnostic.range = Some((
                        buffer.offset_of_position(
                            &diagnostic.diagnositc.range.start,
                            tab_width,
                        ),
                        buffer.offset_of_position(
                            &diagnostic.diagnositc.range.end,
                            tab_width,
                        ),
                    ));
                }
            }
        }
    }

    fn update_diagnositcs_offset(&mut self, delta: &RopeDelta) {
        let buffer = self.buffer.clone();
        let tab_width = self.config.editor.tab_width;
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
                    diagnostic.diagnositc.range.start =
                        buffer.offset_to_position(new_start, tab_width);
                }
                if end != new_end {
                    diagnostic.diagnositc.range.end =
                        buffer.offset_to_position(new_end, tab_width);
                }
            }
        }
    }

    fn edit(
        &mut self,
        edits: &[(&Selection, &str)],
        _after: bool,
        edit_type: EditType,
    ) -> RopeDelta {
        match &self.editor.cursor.mode {
            CursorMode::Normal(_) => {}
            #[allow(unused_variables)]
            CursorMode::Visual { start, end, mode } => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
                let register = Arc::make_mut(&mut self.register);
                register.add_delete(data);
            }
            CursorMode::Insert(_) => {}
        }

        self.initiate_diagnositcs_offset();

        let buffer = Arc::make_mut(&mut self.buffer);
        let delta = buffer.edit_multiple(edits, &self.proxy, edit_type);
        self.inactive_apply_delta(&delta);
        if let Some(snippet) = self.editor.snippet.clone() {
            let mut transformer = Transformer::new(&delta);
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

        self.update_diagnositcs_offset(&delta);

        delta
    }

    fn next_diff(&mut self, ctx: &mut EventCtx, _env: &Env) {
        if let BufferContent::File(buffer_path) = &self.buffer.content {
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
                    if let Some(buffer) = self.main_split.open_files.get(path) {
                        if let Some(changes) = buffer.history_changes.get("head") {
                            for (i, change) in changes.iter().enumerate() {
                                match change {
                                    DiffLines::Left(_) => {
                                        if let Some(next) = changes.get(i + 1) {
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
            let position = self
                .buffer
                .offset_to_position(offset, self.config.editor.tab_width);
            let (path, position) =
                next_in_file_diff_offset(position, buffer_path, &diff_files);
            let location = EditorLocationNew {
                path,
                position: Some(position),
                scroll_offset: None,
                hisotry: Some("head".to_string()),
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
    }

    fn next_error(&mut self, ctx: &mut EventCtx, _env: &Env) {
        if let BufferContent::File(buffer_path) = &self.buffer.content {
            let mut file_diagnostics = self
                .main_split
                .diagnostics
                .iter()
                .filter_map(|(path, diagnositics)| {
                    //let buffer = self.get_buffer_from_path(ctx, ui_state, path);
                    let mut errors: Vec<Position> = diagnositics
                        .iter()
                        .filter_map(|d| {
                            let severity = d
                                .diagnositc
                                .severity
                                .unwrap_or(DiagnosticSeverity::Hint);
                            if severity != DiagnosticSeverity::Error {
                                return None;
                            }
                            Some(d.diagnositc.range.start)
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
            let position = self
                .buffer
                .offset_to_position(offset, self.config.editor.tab_width);
            let (path, position) =
                next_in_file_errors_offset(position, buffer_path, &file_diagnostics);
            let location = EditorLocationNew {
                path,
                position: Some(position),
                scroll_offset: None,
                hisotry: None,
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location),
                Target::Auto,
            ));
        }
    }

    fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        _env: &Env,
    ) -> Option<()> {
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

    fn jump_location_backward(
        &mut self,
        ctx: &mut EventCtx,
        _env: &Env,
    ) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer, self.config.editor.tab_width);
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

    fn page_move(
        &mut self,
        ctx: &mut EventCtx,
        down: bool,
        mods: Modifiers,
        _env: &Env,
    ) {
        let line_height = self.config.editor.line_height as f64;
        let lines =
            (self.editor.size.borrow().height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.do_move(
            if down { &Movement::Down } else { &Movement::Up },
            lines,
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
        _env: &Env,
    ) {
        let line_height = self.config.editor.line_height as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, _col) = self
            .buffer
            .offset_to_line_col(offset, self.config.editor.tab_width);
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
                self.do_move(&Movement::Down, new_line - line, mods)
            }
            Ordering::Less => self.do_move(&Movement::Up, line - new_line, mods),
            _ => (),
        };

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((self.editor.scroll_offset.x, top)),
            Target::Widget(self.editor.view_id),
        ));
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        if !self.config.lapce.modal {
            return;
        }

        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;

        match &cursor.mode {
            CursorMode::Visual { start, end, mode } => {
                if mode != &visual_mode {
                    cursor.mode = CursorMode::Visual {
                        start: *start,
                        end: *end,
                        mode: visual_mode,
                    };
                } else {
                    cursor.mode = CursorMode::Normal(*end);
                };
            }
            _ => {
                let offset = cursor.offset();
                cursor.mode = CursorMode::Visual {
                    start: offset,
                    end: offset,
                    mode: visual_mode,
                };
            }
        }
    }

    pub fn current_code_actions(&self) -> Option<&CodeActionResponse> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        self.buffer.code_actions.get(&prev_offset)
    }

    pub fn diagnostics(&self) -> Option<&Arc<Vec<EditorDiagnostic>>> {
        if let BufferContent::File(path) = &self.buffer.content {
            self.main_split.diagnostics.get(path)
        } else {
            None
        }
    }

    pub fn diagnostics_mut(&mut self) -> Option<&mut Vec<EditorDiagnostic>> {
        if let BufferContent::File(path) = &self.buffer.content {
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
            let (line, font_size) = if let Some(syntax) = self.buffer.syntax.as_ref()
            {
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
            let line = self.buffer.diff_actual_line_from_visual(compare, line);
            (
                line,
                config.char_width(text, config.editor.font_size as f64),
            )
        } else {
            let line = (pos.y / config.editor.line_height as f64).floor() as usize;
            (
                line,
                config.char_width(text, config.editor.font_size as f64),
            )
        };

        let last_line = self.buffer.last_line();
        let (line, col) = if line > last_line {
            (last_line, 0)
        } else {
            let line_end = self.buffer.line_end_col(
                line,
                self.editor.cursor.get_mode() != Mode::Normal,
                config.editor.tab_width,
            );

            let col = (if self.editor.cursor.get_mode() == Mode::Insert {
                (pos.x / char_width).round() as usize
            } else {
                (pos.x / char_width).floor() as usize
            })
            .min(line_end);
            (line, col)
        };
        self.buffer
            .offset_of_line_col(line, col, config.editor.tab_width)
    }

    pub fn single_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &Config,
    ) {
        let new_offset = self.offset_of_mouse(ctx.text(), mouse_event.pos, config);
        self.set_cursor(self.editor.cursor.set_offset(
            new_offset,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        ));

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
                LAPCE_NEW_COMMAND,
                LapceCommandNew {
                    cmd: LapceCommand::GotoDefinition.to_string(),
                    data: None,
                    palette_desc: None,
                    target: CommandTarget::Workbench,
                },
                Target::Widget(self.editor.view_id),
            ));
        } else {
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
        let mouse_offset = self.offset_of_mouse(ctx.text(), mouse_event.pos, config);
        let (start, end) = self.buffer.select_word(mouse_offset);
        self.set_cursor(self.editor.cursor.add_region(
            start,
            end,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        ));
    }

    pub fn triple_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &Config,
    ) {
        ctx.set_active(true);
        let mouse_offset = self.offset_of_mouse(ctx.text(), mouse_event.pos, config);
        let line = self.buffer.line_of_offset(mouse_offset);
        let start = self.buffer.offset_of_line(line);
        let end = self.buffer.offset_of_line(line + 1);
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = editor.cursor.add_region(
            start,
            end,
            mouse_event.mods.shift(),
            mouse_event.mods.alt(),
        );
    }
}

impl KeyPressFocus for LapceEditorBufferData {
    fn get_mode(&self) -> Mode {
        self.editor.cursor.get_mode()
    }

    fn expect_char(&self) -> bool {
        self.editor.inline_find.is_some()
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "search_focus" => {
                self.editor.content == BufferContent::Local(LocalBufferKind::Search)
            }
            "editor_focus" => match self.editor.content {
                BufferContent::File(_) => true,
                BufferContent::Local(_) => false,
                BufferContent::Value(_) => false,
            },
            "diff_focus" => self.editor.compare.is_some(),
            "source_control_focus" => {
                self.editor.content
                    == BufferContent::Local(LocalBufferKind::SourceControl)
            }
            "in_snippet" => self.editor.snippet.is_some(),
            "completion_focus" => self.has_completions(),
            "hover_focus" => self.has_hover(),
            "list_focus" => self.has_completions(),
            "modal_focus" => self.has_completions() || self.has_hover(),
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
        env: &Env,
    ) -> CommandExecuted {
        if let Some(movement) = cmd.move_command(count) {
            self.do_move(&movement, count.unwrap_or(1), mods);
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
            Arc::make_mut(&mut self.editor).motion_mode = None;
            return CommandExecuted::Yes;
        }
        if let Some(mode) = cmd.motion_mode_command() {
            self.set_motion_mode(mode);
            return CommandExecuted::Yes;
        }
        Arc::make_mut(&mut self.editor).motion_mode = None;
        match cmd {
            LapceCommand::SplitLeft => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Left,
                    );
                }
            }
            LapceCommand::SplitRight => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Right,
                    );
                }
            }
            LapceCommand::SplitUp => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Up,
                    );
                }
            }
            LapceCommand::SplitDown => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split.split_move(
                        ctx,
                        SplitContent::EditorTab(*widget_id),
                        SplitMoveDirection::Down,
                    );
                }
            }
            LapceCommand::SplitExchange => {
                if let Some(widget_id) = self.editor.tab_id.as_ref() {
                    self.main_split
                        .split_exchange(ctx, SplitContent::EditorTab(*widget_id));
                }
            }
            LapceCommand::SplitHorizontal => {
                self.main_split.split_editor(
                    ctx,
                    Arc::make_mut(&mut self.editor),
                    SplitDirection::Horizontal,
                    &self.config,
                );
            }
            LapceCommand::SplitVertical => {
                self.main_split.split_editor(
                    ctx,
                    Arc::make_mut(&mut self.editor),
                    SplitDirection::Vertical,
                    &self.config,
                );
            }
            LapceCommand::SplitClose => {
                self.main_split.editor_close(ctx, self.view_id);
            }
            LapceCommand::Undo => {
                self.initiate_diagnositcs_offset();
                let buffer = Arc::make_mut(&mut self.buffer);
                if let Some(delta) = buffer.do_undo(&self.proxy) {
                    self.jump_to_nearest_delta(&delta);
                    self.update_diagnositcs_offset(&delta);
                    self.update_completion(ctx);
                }
            }
            LapceCommand::Redo => {
                self.initiate_diagnositcs_offset();
                let buffer = Arc::make_mut(&mut self.buffer);
                if let Some(delta) = buffer.do_redo(&self.proxy) {
                    self.jump_to_nearest_delta(&delta);
                    self.update_diagnositcs_offset(&delta);
                    self.update_completion(ctx);
                }
            }
            LapceCommand::Append => {
                let offset = self
                    .buffer
                    .move_offset(
                        self.editor.cursor.offset(),
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                        self.editor.code_lens,
                        self.editor.compare.clone(),
                        &self.config,
                    )
                    .0;
                Arc::make_mut(&mut self.buffer).update_edit_type();
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(Selection::caret(offset)),
                    None,
                ));
            }
            LapceCommand::AppendEndOfLine => {
                let (offset, horiz) = self.buffer.move_offset(
                    self.editor.cursor.offset(),
                    None,
                    1,
                    &Movement::EndOfLine,
                    Mode::Insert,
                    self.editor.code_lens,
                    self.editor.compare.clone(),
                    &self.config,
                );
                Arc::make_mut(&mut self.buffer).update_edit_type();
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(Selection::caret(offset)),
                    Some(horiz),
                ));
            }
            LapceCommand::InsertMode => {
                Arc::make_mut(&mut self.editor).cursor.mode = CursorMode::Insert(
                    Selection::caret(self.editor.cursor.offset()),
                );
                Arc::make_mut(&mut self.buffer).update_edit_type();
            }
            LapceCommand::InsertFirstNonBlank => {
                match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let (offset, horiz) = self.buffer.move_offset(
                            *offset,
                            None,
                            1,
                            &Movement::FirstNonBlank,
                            Mode::Normal,
                            self.editor.code_lens,
                            self.editor.compare.clone(),
                            &self.config,
                        );
                        Arc::make_mut(&mut self.buffer).update_edit_type();
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(Selection::caret(offset)),
                            Some(horiz),
                        ));
                    }
                    #[allow(unused_variables)]
                    CursorMode::Visual { start, end, mode } => {
                        let mut selection = Selection::new();
                        for region in self
                            .editor
                            .cursor
                            .edit_selection(
                                &self.buffer,
                                self.config.editor.tab_width,
                            )
                            .regions()
                        {
                            selection.add_region(SelRegion::caret(region.min()));
                        }
                        Arc::make_mut(&mut self.buffer).update_edit_type();
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {}
                };
            }
            LapceCommand::NewLineAbove => {
                let line = self.editor.cursor.current_line(&self.buffer);
                let offset = if line > 0 {
                    self.buffer.line_end_offset(line - 1, true)
                } else {
                    self.buffer.first_non_blank_character_on_line(line)
                };
                self.insert_new_line(ctx, Selection::caret(offset));
            }
            LapceCommand::NewLineBelow => {
                let offset = self.editor.cursor.offset();
                let offset = self.buffer.offset_line_end(offset, true);
                self.insert_new_line(ctx, Selection::caret(offset));
            }
            LapceCommand::DeleteToBeginningOfLine => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                    CursorMode::Insert(_) => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );

                        self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::StartOfLine,
                            Mode::Insert,
                            true,
                            self.editor.code_lens,
                            self.editor.compare.clone(),
                            &self.config,
                        )
                    }
                };
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset =
                            self.buffer.offset_line_end(offset, false).min(offset);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                }
            }
            LapceCommand::Yank => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
                let register = Arc::make_mut(&mut self.register);
                register.add_yank(data);
                match &self.editor.cursor.mode {
                    #[allow(unused_variables)]
                    CursorMode::Visual { start, end, mode } => {
                        let offset = *start.min(end);
                        let offset =
                            self.buffer.offset_line_end(offset, false).min(offset);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Normal(_) => {}
                    CursorMode::Insert(_) => {}
                }
            }
            LapceCommand::ClipboardCut => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
                Application::global().clipboard().put_string(data.content);

                let selection = if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    for region in selection.regions_mut() {
                        if region.is_caret() {
                            let line = self.buffer.line_of_offset(region.start);
                            let start = self.buffer.offset_of_line(line);
                            let end = self.buffer.offset_of_line(line + 1);
                            region.start = start;
                            region.end = end;
                        }
                    }
                    selection
                } else {
                    self.editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width)
                };

                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor_after_change(selection);
                self.cancel_completion();
            }
            LapceCommand::MotionModeYank => {
                if self.editor.motion_mode.is_none() {
                    Arc::make_mut(&mut self.editor).motion_mode =
                        Some(MotionMode::Yank);
                } else if let Some(MotionMode::Yank) = self.editor.motion_mode {
                    let data = self
                        .editor
                        .cursor
                        .yank(&self.buffer, self.config.editor.tab_width);
                    let register = Arc::make_mut(&mut self.register);
                    register.add_yank(data);
                } else {
                    Arc::make_mut(&mut self.editor).motion_mode = None;
                }
            }
            LapceCommand::ClipboardCopy => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
                Application::global().clipboard().put_string(data.content);
                match &self.editor.cursor.mode {
                    CursorMode::Visual {
                        start,
                        end,
                        mode: _,
                    } => {
                        let offset = *start.min(end);
                        let offset =
                            self.buffer.offset_line_end(offset, false).min(offset);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Normal(_) => {}
                    CursorMode::Insert(_) => {}
                }
            }
            LapceCommand::ClipboardPaste => {
                if let Some(s) = Application::global().clipboard().get_string() {
                    let mode = if s.ends_with('\n') {
                        VisualMode::Linewise
                    } else {
                        VisualMode::Normal
                    };
                    let data = RegisterData { content: s, mode };
                    self.paste(ctx, &data);
                }
            }
            LapceCommand::Paste => {
                let data = self.register.unamed.clone();
                self.paste(ctx, &data);
            }
            LapceCommand::DeleteWordForward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                    CursorMode::Insert(_) => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );

                        self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::WordForward,
                            Mode::Insert,
                            true,
                            self.editor.code_lens,
                            self.editor.compare.clone(),
                            &self.config,
                        )
                    }
                };
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteWordBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                    CursorMode::Insert(_) => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );

                        self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::WordBackward,
                            Mode::Insert,
                            true,
                            self.editor.code_lens,
                            self.editor.compare.clone(),
                            &self.config,
                        )
                    }
                };
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                    CursorMode::Insert(_) => {
                        let indent = self.buffer.indent_unit();
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );
                        let mut new_selection = Selection::new();
                        for region in selection.regions() {
                            let new_region = if region.is_caret() {
                                if indent.starts_with('\t') {
                                    self.buffer.update_region(
                                        region,
                                        1,
                                        &Movement::Left,
                                        Mode::Insert,
                                        true,
                                        self.editor.code_lens,
                                        self.editor.compare.clone(),
                                        &self.config,
                                    )
                                } else {
                                    let line =
                                        self.buffer.line_of_offset(region.start);
                                    let nonblank = self
                                        .buffer
                                        .first_non_blank_character_on_line(line);
                                    let (_, col) = self.buffer.offset_to_line_col(
                                        region.start,
                                        self.config.editor.tab_width,
                                    );
                                    let count =
                                        if region.start <= nonblank && col > 0 {
                                            let r = col % indent.len();
                                            if r == 0 {
                                                indent.len()
                                            } else {
                                                r
                                            }
                                        } else {
                                            1
                                        };
                                    self.buffer.update_region(
                                        region,
                                        count,
                                        &Movement::Left,
                                        Mode::Insert,
                                        true,
                                        self.editor.code_lens,
                                        self.editor.compare.clone(),
                                        &self.config,
                                    )
                                }
                            } else {
                                *region
                            };
                            new_selection.add_region(new_region);
                        }

                        let mut selection = new_selection;
                        if selection.regions().len() == 1 {
                            let delete_str = self
                                .buffer
                                .slice_to_cow(
                                    selection.min_offset()..selection.max_offset(),
                                )
                                .to_string();
                            if str_is_pair_left(&delete_str) {
                                if let Some(c) = str_matching_pair(&delete_str) {
                                    let offset = selection.max_offset();
                                    let line = self.buffer.line_of_offset(offset);
                                    let line_end =
                                        self.buffer.line_end_offset(line, true);
                                    let content = self
                                        .buffer
                                        .slice_to_cow(offset..line_end)
                                        .to_string();
                                    if content.trim().starts_with(&c.to_string()) {
                                        let index = content
                                            .match_indices(c)
                                            .next()
                                            .unwrap()
                                            .0;
                                        selection = Selection::region(
                                            selection.min_offset(),
                                            offset + index + 1,
                                        );
                                    }
                                }
                            }
                        }
                        selection
                    }
                };
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                    CursorMode::Insert(_) => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );
                        let mut new_selection = Selection::new();
                        for region in selection.regions() {
                            let new_region = if region.is_caret() {
                                self.buffer.update_region(
                                    region,
                                    1,
                                    &Movement::Right,
                                    Mode::Insert,
                                    true,
                                    self.editor.code_lens,
                                    self.editor.compare.clone(),
                                    &self.config,
                                )
                            } else {
                                *region
                            };
                            new_selection.add_region(new_region);
                        }
                        new_selection
                    }
                };
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForwardAndInsert => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                let delta = self.edit(&[(&selection, "")], true, EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
                self.update_completion(ctx);
            }
            LapceCommand::InsertTab => {
                self.insert_tab();
                self.update_completion(ctx);
            }
            LapceCommand::InsertNewLine => {
                match self.editor.cursor.mode.clone() {
                    CursorMode::Normal(offset) => {
                        self.insert_new_line(ctx, Selection::caret(offset));
                    }
                    CursorMode::Insert(selection) => {
                        self.insert_new_line(ctx, selection);
                    }
                    CursorMode::Visual {
                        start: _,
                        end: _,
                        mode: _,
                    } => {}
                }
                self.update_completion(ctx);
            }
            LapceCommand::ToggleVisualMode => {
                self.toggle_visual(VisualMode::Normal);
            }
            LapceCommand::ToggleLinewiseVisualMode => {
                self.toggle_visual(VisualMode::Linewise);
            }
            LapceCommand::ToggleBlockwiseVisualMode => {
                self.toggle_visual(VisualMode::Blockwise);
            }
            LapceCommand::CenterOfWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorCenter,
                    Target::Widget(self.editor.view_id),
                ));
            }
            LapceCommand::ScrollDown => {
                self.scroll(ctx, true, count.unwrap_or(1), mods, env);
            }
            LapceCommand::ScrollUp => {
                self.scroll(ctx, false, count.unwrap_or(1), mods, env);
            }
            LapceCommand::PageDown => {
                self.page_move(ctx, true, mods, env);
            }
            LapceCommand::PageUp => {
                self.page_move(ctx, false, mods, env);
            }
            LapceCommand::JumpLocationBackward => {
                self.jump_location_backward(ctx, env);
            }
            LapceCommand::JumpLocationForward => {
                self.jump_location_forward(ctx, env);
            }
            LapceCommand::MoveLineUp => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    for region in selection.regions_mut() {
                        let start_line = self.buffer.line_of_offset(region.min());
                        if start_line > 0 {
                            let previous_line_len =
                                self.buffer.line_content(start_line - 1).len();

                            let end_line = self.buffer.line_of_offset(region.max());
                            let start = self.buffer.offset_of_line(start_line);
                            let end = self.buffer.offset_of_line(end_line + 1);
                            let content =
                                self.buffer.slice_to_cow(start..end).to_string();
                            self.edit(
                                &[
                                    (&Selection::region(start, end), ""),
                                    (
                                        &Selection::caret(
                                            self.buffer
                                                .offset_of_line(start_line - 1),
                                        ),
                                        &content,
                                    ),
                                ],
                                true,
                                EditType::InsertChars,
                            );
                            region.start -= previous_line_len;
                            region.end -= previous_line_len;
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::MoveLineDown => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    for region in selection.regions_mut().iter_mut().rev() {
                        let last_line = self.buffer.last_line();
                        let start_line = self.buffer.line_of_offset(region.min());
                        let end_line = self.buffer.line_of_offset(region.max());
                        if end_line < last_line {
                            let next_line_len =
                                self.buffer.line_content(end_line + 1).len();

                            let start = self.buffer.offset_of_line(start_line);
                            let end = self.buffer.offset_of_line(end_line + 1);
                            let content =
                                self.buffer.slice_to_cow(start..end).to_string();
                            self.edit(
                                &[
                                    (
                                        &Selection::caret(
                                            self.buffer.offset_of_line(end_line + 2),
                                        ),
                                        &content,
                                    ),
                                    (&Selection::region(start, end), ""),
                                ],
                                true,
                                EditType::InsertChars,
                            );
                            region.start += next_line_len;
                            region.end += next_line_len;
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::InsertCursorAbove => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    let offset = selection.first().map(|s| s.end()).unwrap_or(0);
                    let (new_offset, _) = self.buffer.move_offset(
                        offset,
                        self.editor.cursor.horiz.as_ref(),
                        1,
                        &Movement::Up,
                        Mode::Insert,
                        self.editor.code_lens,
                        self.editor.compare.clone(),
                        &self.config,
                    );
                    if new_offset != offset {
                        selection.add_region(SelRegion::new(
                            new_offset, new_offset, None,
                        ));
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::InsertCursorBelow => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    let offset = selection.last().map(|s| s.end()).unwrap_or(0);
                    let (new_offset, _) = self.buffer.move_offset(
                        offset,
                        self.editor.cursor.horiz.as_ref(),
                        1,
                        &Movement::Down,
                        Mode::Insert,
                        self.editor.code_lens,
                        self.editor.compare.clone(),
                        &self.config,
                    );
                    if new_offset != offset {
                        selection.add_region(SelRegion::new(
                            new_offset, new_offset, None,
                        ));
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::InsertCursorEndOfLine => {
                if let CursorMode::Insert(selection) =
                    self.editor.cursor.mode.clone()
                {
                    let mut new_selection = Selection::new();
                    for region in selection.regions() {
                        let (start_line, _) = self.buffer.offset_to_line_col(
                            region.min(),
                            self.config.editor.tab_width,
                        );
                        let (end_line, end_col) = self.buffer.offset_to_line_col(
                            region.max(),
                            self.config.editor.tab_width,
                        );
                        for line in start_line..end_line + 1 {
                            let offset = if line == end_line {
                                self.buffer.offset_of_line_col(
                                    line,
                                    end_col,
                                    self.config.editor.tab_width,
                                )
                            } else {
                                self.buffer.line_end_offset(line, true)
                            };
                            new_selection
                                .add_region(SelRegion::new(offset, offset, None));
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(new_selection),
                        None,
                    ));
                }
            }
            LapceCommand::SelectCurrentLine => {
                if let CursorMode::Insert(selection) =
                    self.editor.cursor.mode.clone()
                {
                    let mut new_selection = Selection::new();
                    for region in selection.regions() {
                        let start_line = self.buffer.line_of_offset(region.min());
                        let start = self.buffer.offset_of_line(start_line);
                        let end_line = self.buffer.line_of_offset(region.max());
                        let end = self.buffer.offset_of_line(end_line + 1);
                        new_selection.add_region(SelRegion::new(start, end, None));
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(new_selection),
                        None,
                    ));
                }
            }
            LapceCommand::SelectAllCurrent => {
                if let CursorMode::Insert(selection) =
                    self.editor.cursor.mode.clone()
                {
                    let mut new_selection = Selection::new();
                    if !selection.is_empty() {
                        let first = selection.first().unwrap();
                        let (start, end) = if first.is_caret() {
                            self.buffer.select_word(first.start())
                        } else {
                            (first.min(), first.max())
                        };
                        let search_str = self.buffer.slice_to_cow(start..end);
                        let mut find = Find::new(0);
                        find.set_find(&search_str, false, false, false);
                        let mut offset = 0;
                        while let Some((start, end)) =
                            find.next(&self.buffer.rope, offset, false, false)
                        {
                            offset = end;
                            new_selection
                                .add_region(SelRegion::new(start, end, None));
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(new_selection),
                        None,
                    ));
                }
            }
            LapceCommand::SelectNextCurrent => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    if !selection.is_empty() {
                        let mut had_caret = false;
                        for region in selection.regions_mut() {
                            if region.is_caret() {
                                had_caret = true;
                                let (start, end) =
                                    self.buffer.select_word(region.start());
                                region.start = start;
                                region.end = end;
                            }
                        }
                        if !had_caret {
                            let r = selection.last_inserted().unwrap();
                            let search_str =
                                self.buffer.slice_to_cow(r.min()..r.max());
                            let mut find = Find::new(0);
                            find.set_find(&search_str, false, false, false);
                            let mut offset = r.max();
                            let mut seen = HashSet::new();
                            while let Some((start, end)) =
                                find.next(&self.buffer.rope, offset, false, true)
                            {
                                if !selection
                                    .regions()
                                    .iter()
                                    .any(|r| r.min() == start && r.max() == end)
                                {
                                    selection.add_region(SelRegion::new(
                                        start, end, None,
                                    ));
                                    break;
                                }
                                if seen.contains(&end) {
                                    break;
                                }
                                offset = end;
                                seen.insert(offset);
                            }
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::SelectSkipCurrent => {
                if let CursorMode::Insert(mut selection) =
                    self.editor.cursor.mode.clone()
                {
                    if !selection.is_empty() {
                        let r = selection.last_inserted().unwrap();
                        if r.is_caret() {
                            let (start, end) = self.buffer.select_word(r.start());
                            selection.replace_last_inserted_region(SelRegion::new(
                                start, end, None,
                            ));
                        } else {
                            let search_str =
                                self.buffer.slice_to_cow(r.min()..r.max());
                            let mut find = Find::new(0);
                            find.set_find(&search_str, false, false, false);
                            let mut offset = r.max();
                            let mut seen = HashSet::new();
                            while let Some((start, end)) =
                                find.next(&self.buffer.rope, offset, false, true)
                            {
                                if !selection
                                    .regions()
                                    .iter()
                                    .any(|r| r.min() == start && r.max() == end)
                                {
                                    selection.replace_last_inserted_region(
                                        SelRegion::new(start, end, None),
                                    );
                                    break;
                                }
                                if seen.contains(&end) {
                                    break;
                                }
                                offset = end;
                                seen.insert(offset);
                            }
                        }
                    }
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                }
            }
            LapceCommand::SelectUndo => {
                if let CursorMode::Insert(_) = self.editor.cursor.mode.clone() {
                    self.check_selection_history();
                    let editor = Arc::make_mut(&mut self.editor);
                    editor.selection_history.selections.pop_back();
                    if let Some(selection) =
                        editor.selection_history.selections.last().cloned()
                    {
                        editor.cursor =
                            Cursor::new(CursorMode::Insert(selection), None);
                    }
                }
            }
            LapceCommand::NextError => {
                self.next_error(ctx, env);
            }
            LapceCommand::PreviousError => {}
            LapceCommand::NextDiff => {
                self.next_diff(ctx, env);
            }
            LapceCommand::PreviousDiff => {}
            LapceCommand::ListNext => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.next();
            }
            LapceCommand::ListPrevious => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.previous();
            }
            LapceCommand::ModalClose => {
                if self.has_completions() {
                    self.cancel_completion();
                }

                if self.has_hover() {
                    self.cancel_hover();
                }
            }
            LapceCommand::JumpToNextSnippetPlaceholder => {
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
                        let mut selection = Selection::new();
                        let region = SelRegion::new(*start, *end, None);
                        selection.add_region(region);
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }

                    if last_placeholder {
                        Arc::make_mut(&mut self.editor).snippet = None;
                    }
                    self.cancel_completion();
                }
            }
            LapceCommand::JumpToPrevSnippetPlaceholder => {
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
                            let mut selection = Selection::new();
                            let region = SelRegion::new(*start, *end, None);
                            selection.add_region(region);
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                        }
                        self.cancel_completion();
                    }
                }
            }
            LapceCommand::ListSelect => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);

                let count = self.completion.input.len();
                let _selection = if count > 0 {
                    self.buffer.update_selection(
                        &selection,
                        count,
                        &Movement::Left,
                        Mode::Insert,
                        true,
                        self.editor.code_lens,
                        self.editor.compare.clone(),
                        &self.config,
                    )
                } else {
                    selection
                };

                let item = self.completion.current_item().to_owned();
                self.cancel_completion();
                if item.data.is_some() {
                    let view_id = self.editor.view_id;
                    let buffer_id = self.buffer.id;
                    let rev = self.buffer.rev;
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
            LapceCommand::IndentLine => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                self.indent_line(selection);
            }
            LapceCommand::OutdentLine => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                self.outdent_line(selection);
            }
            LapceCommand::ToggleLineComment => {
                let mut lines = HashSet::new();
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                let comment_token = self
                    .buffer
                    .syntax
                    .as_ref()
                    .map(|s| s.language.comment_token())
                    .unwrap_or("//")
                    .to_string();
                let mut had_comment = true;
                let mut smallest_indent = usize::MAX;
                for region in selection.regions() {
                    let mut line = self.buffer.line_of_offset(region.min());
                    let end_line = self.buffer.line_of_offset(region.max());
                    let end_line_offset = self.buffer.offset_of_line(end_line);
                    let end = if end_line > line && region.max() == end_line_offset {
                        end_line_offset
                    } else {
                        self.buffer.offset_of_line(end_line + 1)
                    };
                    let start = self.buffer.offset_of_line(line);
                    for content in self.buffer.rope.lines(start..end) {
                        let trimed_content = content.trim_start();
                        if trimed_content.is_empty() {
                            line += 1;
                            continue;
                        }
                        let indent = content.len() - trimed_content.len();
                        if indent < smallest_indent {
                            smallest_indent = indent;
                        }
                        if !trimed_content.starts_with(&comment_token) {
                            had_comment = false;
                            lines.insert((line, indent, 0));
                        } else {
                            let had_space_after_comment =
                                trimed_content.chars().nth(comment_token.len())
                                    == Some(' ');
                            lines.insert((
                                line,
                                indent,
                                comment_token.len()
                                    + if had_space_after_comment { 1 } else { 0 },
                            ));
                        }
                        line += 1;
                    }
                }

                let delta = if had_comment {
                    let mut selection = Selection::new();
                    for (line, indent, len) in lines.iter() {
                        let start = self.buffer.offset_of_line(*line) + indent;
                        selection.add_region(SelRegion::new(
                            start,
                            start + len,
                            None,
                        ))
                    }
                    self.edit(&[(&selection, "")], true, EditType::Delete)
                } else {
                    let mut selection = Selection::new();
                    for (line, _, _) in lines.iter() {
                        let start =
                            self.buffer.offset_of_line(*line) + smallest_indent;
                        selection.add_region(SelRegion::new(start, start, None))
                    }
                    self.edit(
                        &[(&selection, &(comment_token + " "))],
                        true,
                        EditType::InsertChars,
                    )
                };
                Arc::make_mut(&mut self.editor).cursor.apply_delta(&delta);
            }
            LapceCommand::NormalMode => {
                if !self.config.lapce.modal {
                    if let CursorMode::Insert(selection) = &self.editor.cursor.mode {
                        match selection.regions().len() {
                            i if i > 1 => {
                                if let Some(region) = selection.last_inserted() {
                                    let new_selection =
                                        Selection::region(region.start, region.end);
                                    self.set_cursor(Cursor::new(
                                        CursorMode::Insert(new_selection),
                                        None,
                                    ));
                                    return CommandExecuted::Yes;
                                }
                            }
                            i if i == 1 => {
                                let region = selection.regions()[0];
                                if !region.is_caret() {
                                    let new_selection = Selection::caret(region.end);
                                    self.set_cursor(Cursor::new(
                                        CursorMode::Insert(new_selection),
                                        None,
                                    ));
                                    return CommandExecuted::Yes;
                                }
                            }
                            _ => (),
                        }
                    }

                    return CommandExecuted::No;
                }

                let offset = match &self.editor.cursor.mode {
                    CursorMode::Insert(selection) => {
                        self.buffer
                            .move_offset(
                                selection.get_cursor_offset(),
                                None,
                                1,
                                &Movement::Left,
                                Mode::Normal,
                                self.editor.code_lens,
                                self.editor.compare.clone(),
                                &self.config,
                            )
                            .0
                    }
                    #[allow(unused_variables)]
                    CursorMode::Visual { start, end, mode } => {
                        self.buffer.offset_line_end(*end, false).min(*end)
                    }
                    CursorMode::Normal(offset) => *offset,
                };
                Arc::make_mut(&mut self.buffer).update_edit_type();

                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Normal(offset);
                editor.cursor.horiz = None;
                editor.snippet = None;
                editor.inline_find = None;
                self.cancel_completion();
            }
            LapceCommand::ToggleCodeLens => {
                let editor = Arc::make_mut(&mut self.editor);
                editor.code_lens = !editor.code_lens;
            }
            LapceCommand::GotoDefinition => {
                let offset = self.editor.cursor.offset();
                let start_offset = self.buffer.prev_code_boundary(offset);
                let start_position = self
                    .buffer
                    .offset_to_position(start_offset, self.config.editor.tab_width);
                let event_sink = ctx.get_external_handle();
                let buffer_id = self.buffer.id;
                let position = self
                    .buffer
                    .offset_to_position(offset, self.config.editor.tab_width);
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
                                                EditorLocationNew {
                                                    path: path_from_url(
                                                        &location.uri,
                                                    ),
                                                    position: Some(
                                                        location.range.start,
                                                    ),
                                                    scroll_offset: None,
                                                    hisotry: None,
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
            LapceCommand::SourceControl => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FocusSourceControl,
                    Target::Auto,
                ));
            }
            LapceCommand::SourceControlCancel => {
                if self.editor.content
                    == BufferContent::Local(LocalBufferKind::SourceControl)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::FocusEditor,
                        Target::Auto,
                    ));
                }
            }
            LapceCommand::ShowCodeActions => {
                if let Some(actions) = self.current_code_actions() {
                    if !actions.is_empty() {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ShowCodeActions,
                            Target::Auto,
                        ));
                    }
                }
            }
            LapceCommand::Search => {
                Arc::make_mut(&mut self.find).visual = true;
                let region = match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => SelRegion::caret(*offset),
                    CursorMode::Visual {
                        start,
                        end,
                        mode: _,
                    } => SelRegion::new(
                        *start.min(end),
                        self.buffer.next_grapheme_offset(
                            *start.max(end),
                            1,
                            self.buffer.len(),
                        ),
                        None,
                    ),
                    CursorMode::Insert(selection) => {
                        *selection.last_inserted().unwrap()
                    }
                };
                let pattern = if region.is_caret() {
                    let (start, end) = self.buffer.select_word(region.start);
                    self.buffer.slice_to_cow(start..end).to_string()
                } else {
                    self.buffer
                        .slice_to_cow(region.min()..region.max())
                        .to_string()
                };
                if !pattern.contains('\n') {
                    Arc::make_mut(&mut self.find)
                        .set_find(&pattern, false, false, false);
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSearch(pattern),
                        Target::Widget(*self.main_split.tab_id),
                    ));
                }
                if let Some(find_view_id) = self.editor.find_view_id {
                    ctx.submit_command(Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceCommand::SelectAll.to_string(),
                            data: None,
                            palette_desc: None,
                            target: CommandTarget::Focus,
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
            LapceCommand::SearchWholeWordForward => {
                Arc::make_mut(&mut self.find).visual = true;
                let offset = self.editor.cursor.offset();
                let (start, end) = self.buffer.select_word(offset);
                let word = self.buffer.slice_to_cow(start..end).to_string();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSearch(word.clone()),
                    Target::Widget(*self.main_split.tab_id),
                ));
                Arc::make_mut(&mut self.find).set_find(&word, false, false, true);
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, _end)) = next {
                    self.do_move(&Movement::Offset(start), 1, mods);
                }
            }
            LapceCommand::SearchInView => {
                let start_line = ((self.editor.scroll_offset.y
                    / self.config.editor.line_height as f64)
                    .ceil() as usize)
                    .max(self.buffer.last_line());
                let end_line = ((self.editor.scroll_offset.y
                    + self.editor.size.borrow().height
                        / self.config.editor.line_height as f64)
                    .ceil() as usize)
                    .max(self.buffer.last_line());
                let end_offset = self.buffer.offset_of_line(end_line + 1);

                let offset = self.editor.cursor.offset();
                let line = self.buffer.line_of_offset(offset);
                let offset = self.buffer.offset_of_line(line);
                let next = self.find.next(&self.buffer.rope, offset, false, false);

                if let Some(start) = next
                    .map(|(start, _)| start)
                    .filter(|start| *start < end_offset)
                {
                    self.do_move(&Movement::Offset(start), 1, mods);
                } else {
                    let start_offset = self.buffer.offset_of_line(start_line);
                    if let Some((start, _)) =
                        self.find.next(&self.buffer.rope, start_offset, false, true)
                    {
                        self.do_move(&Movement::Offset(start), 1, mods);
                    }
                }
            }
            LapceCommand::SearchForward => {
                Arc::make_mut(&mut self.find).visual = true;
                let offset = self.editor.cursor.offset();
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, _end)) = next {
                    self.do_move(&Movement::Offset(start), 1, mods);
                }
            }
            LapceCommand::SearchBackward => {
                if self.editor.content.is_search() {
                    if let Some(parent_view_id) = self.editor.parent_view_id {
                        ctx.submit_command(Command::new(
                            LAPCE_NEW_COMMAND,
                            LapceCommandNew {
                                cmd: LapceCommand::SearchBackward.to_string(),
                                data: None,
                                palette_desc: None,
                                target: CommandTarget::Focus,
                            },
                            Target::Widget(parent_view_id),
                        ));
                    }
                } else {
                    Arc::make_mut(&mut self.find).visual = true;
                    let offset = self.editor.cursor.offset();
                    let next = self.find.next(&self.buffer.rope, offset, true, true);
                    if let Some((start, _end)) = next {
                        self.do_move(&Movement::Offset(start), 1, mods);
                    }
                }
            }
            LapceCommand::ClearSearch => {
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
            LapceCommand::SelectAll => {
                let new_selection = Selection::region(0, self.buffer.len());
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(new_selection),
                    None,
                ));
            }
            LapceCommand::RepeatLastInlineFind => {
                if let Some((direction, c)) = self.editor.last_inline_find.clone() {
                    self.inline_find(direction, &c);
                }
            }
            LapceCommand::InlineFindLeft => {
                Arc::make_mut(&mut self.editor).inline_find =
                    Some(InlineFindDirection::Left);
            }
            LapceCommand::InlineFindRight => {
                Arc::make_mut(&mut self.editor).inline_find =
                    Some(InlineFindDirection::Right);
            }
            LapceCommand::JoinLines => {
                let offset = self.editor.cursor.offset();
                let (line, _col) = self
                    .buffer
                    .offset_to_line_col(offset, self.config.editor.tab_width);
                if line < self.buffer.last_line() {
                    let start = self.buffer.line_end_offset(line, true);
                    let end =
                        self.buffer.first_non_blank_character_on_line(line + 1);
                    self.edit(
                        &[(&Selection::region(start, end), " ")],
                        false,
                        EditType::Other,
                    );
                }
            }
            LapceCommand::FormatDocument => {
                if let BufferContent::File(path) = &self.buffer.content {
                    let path = path.clone();
                    let proxy = self.proxy.clone();
                    let buffer_id = self.buffer.id;
                    let rev = self.buffer.rev;
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
            LapceCommand::Save => {
                if !self.buffer.dirty {
                    return CommandExecuted::Yes;
                }

                if let BufferContent::File(path) = &self.buffer.content {
                    let path = path.clone();
                    let proxy = self.proxy.clone();
                    let buffer_id = self.buffer.id;
                    let rev = self.buffer.rev;
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
                            LapceUICommand::DocumentFormatAndSave(path, rev, result),
                            Target::Auto,
                        );
                    });
                }
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            let mut selection = self
                .editor
                .cursor
                .edit_selection(&self.buffer, self.config.editor.tab_width);
            let cursor_char =
                self.buffer.char_at_offset(selection.get_cursor_offset());

            let mut content = c.to_string();
            if c.chars().count() == 1 {
                let c = c.chars().next().unwrap();
                if !matching_pair_direction(c).unwrap_or(true) {
                    if cursor_char == Some(c) {
                        self.do_move(&Movement::Right, 1, Modifiers::empty());
                        return;
                    } else {
                        let offset = selection.get_cursor_offset();
                        let line = self.buffer.line_of_offset(offset);
                        let line_start = self.buffer.offset_of_line(line);
                        if self.buffer.slice_to_cow(line_start..offset).trim() == ""
                        {
                            if let Some(c) = matching_char(c) {
                                if let Some(previous_offset) =
                                    self.buffer.previous_unmatched(c, offset)
                                {
                                    let previous_line =
                                        self.buffer.line_of_offset(previous_offset);
                                    let line_indent =
                                        self.buffer.indent_on_line(previous_line);
                                    content = line_indent + &content;
                                    selection =
                                        Selection::region(line_start, offset);
                                }
                            }
                        };
                    }
                }
            }

            let delta =
                self.edit(&[(&selection, &content)], true, EditType::InsertChars);
            let selection =
                selection.apply_delta(&delta, true, InsertDrift::Default);
            let editor = Arc::make_mut(&mut self.editor);
            editor.cursor.mode = CursorMode::Insert(selection.clone());
            editor.cursor.horiz = None;
            if c.chars().count() == 1 {
                let c = c.chars().next().unwrap();
                let is_whitespace_or_punct = cursor_char
                    .map(|c| {
                        let prop = get_word_property(c);
                        prop == WordProperty::Lf
                            || prop == WordProperty::Space
                            || prop == WordProperty::Punctuation
                    })
                    .unwrap_or(true);
                if is_whitespace_or_punct
                    && matching_pair_direction(c).unwrap_or(false)
                {
                    if let Some(c) = matching_char(c) {
                        self.edit(
                            &[(&selection, &c.to_string())],
                            false,
                            EditType::InsertChars,
                        );
                    }
                }
            }
            self.update_completion(ctx);
            self.cancel_hover();
        } else if let Some(direction) = self.editor.inline_find.clone() {
            self.inline_find(direction.clone(), c);
            let editor = Arc::make_mut(&mut self.editor);
            editor.last_inline_find = Some((direction, c.to_string()));
            editor.inline_find = None;
        }
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
pub struct RegisterContent {
    #[allow(dead_code)]
    kind: VisualMode,

    #[allow(dead_code)]
    content: Vec<String>,
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

fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
fn str_is_pair_right(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return !matching_pair_direction(c).unwrap_or(true);
    }
    false
}

fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
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
                EditorLocationNew {
                    path: path_from_url(&location.uri),
                    position: Some(location.range.start),
                    scroll_offset: None,
                    hisotry: None,
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
