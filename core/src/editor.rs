use crate::buffer::{has_unmatched_pair, EditType};
use crate::command::{
    CommandTarget, LapceCommandNew, LapceWorkbenchCommand, LAPCE_NEW_COMMAND,
};
use crate::completion::{CompletionData, CompletionStatus, Snippet};
use crate::config::{Config, LapceTheme, LOGO};
use crate::data::{
    EditorContent, EditorDiagnostic, EditorKind, EditorType, FocusArea,
    InlineFindDirection, LapceEditorData, LapceMainSplitData, LapceTabData,
    RegisterData,
};
use crate::find::Find;
use crate::keypress::{KeyMap, KeyPress, KeyPressFocus};
use crate::proxy::LapceProxy;
use crate::scroll::LapceIdentityWrapper;
use crate::signature::SignatureState;
use crate::split::LapceSplitNew;
use crate::state::LapceWorkspace;
use crate::svg::{file_svg_new, get_svg, logo_svg};
use crate::theme::OldLapceTheme;
use crate::{buffer::get_word_property, state::LapceFocus};
use crate::{buffer::matching_char, data::LapceEditorViewData};
use crate::{buffer::previous_has_unmatched_pair, movement::Cursor};
use crate::{buffer::WordProperty, movement::CursorMode};
use crate::{
    buffer::{matching_pair_direction, BufferNew},
    scroll::LapceScrollNew,
};
use crate::{
    buffer::{next_has_unmatched_pair, BufferState},
    scroll::LapcePadding,
};
use crate::{
    buffer::{BufferId, BufferUIState, InvalLines},
    command::{
        EnsureVisiblePosition, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND,
    },
    completion::ScoredCompletionItem,
    movement::{ColPosition, LinePosition, Movement, SelRegion, Selection},
    scroll::LapceScroll,
    split::SplitMoveDirection,
    state::Mode,
    state::VisualMode,
};
use anyhow::{anyhow, Result};
use bit_vec::BitVec;
use crossbeam_channel::{self, bounded};
use druid::kurbo::BezPath;
use druid::piet::Svg;
use druid::widget::{LensWrap, WidgetWrapper};
use druid::{
    kurbo::Line, piet::PietText, theme, widget::Flex, widget::IdentityWrapper,
    widget::Padding, widget::Scroll, widget::SvgData, Affine, BoxConstraints, Color,
    Command, Data, Env, Event, EventCtx, FontDescriptor, FontFamily, Insets,
    KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2, Widget, WidgetExt,
    WidgetId, WidgetPod, WindowId,
};
use druid::{
    menu, Application, ExtEventSink, FileDialogOptions, Menu, Modifiers, MouseEvent,
};
use druid::{
    piet::{
        PietTextLayout, Text, TextAttribute, TextLayout as TextLayoutTrait,
        TextLayoutBuilder,
    },
    FontWeight,
};
use fzyr::has_match;
use itertools::Itertools;
use lsp_types::CompletionTextEdit;
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DocumentChanges, GotoDefinitionResponse,
    Location, Position, SignatureHelp, TextEdit, Url, WorkspaceEdit,
};
use serde_json::Value;
use std::thread;
use std::{cmp::Ordering, iter::Iterator, path::PathBuf};
use std::{collections::HashMap, sync::Arc};
use std::{str::FromStr, time::Duration};
use strum::EnumMessage;
use unicode_width::UnicodeWidthStr;
use xi_core_lib::selection::InsertDrift;
use xi_rope::{Interval, RopeDelta, Transformer};

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
    last_movement: Movement,
}

#[derive(Clone, Debug)]
pub struct EditorLocationNew {
    pub path: PathBuf,
    pub position: Option<Position>,
    pub scroll_offset: Option<Vec2>,
}

#[derive(Clone, Debug)]
pub struct EditorLocation {
    pub path: String,
    pub offset: usize,
    pub scroll_offset: Option<Vec2>,
}

pub enum LapceEditorContainerKind {
    Container(WidgetPod<LapceEditorViewData, LapceEditorContainer>),
    DiffSplit(LapceSplitNew),
}

pub struct EditorDiffSplit {
    left: WidgetPod<LapceTabData, LapceEditorContainer>,
    right: WidgetPod<LapceTabData, LapceEditorContainer>,
}

impl Widget<LapceTabData> for EditorDiffSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.left.event(ctx, event, data, env);
        self.right.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.lifecycle(ctx, event, data, env);
        self.right.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.update(ctx, data, env);
        self.right.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.left.layout(ctx, bc, data, env);
        self.right.layout(ctx, bc, data, env);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.left.paint(ctx, data, env);
        self.right.paint(ctx, data, env);
    }
}

pub struct LapceEditorBufferData {
    pub view_id: WidgetId,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<BufferNew>,
    pub completion: Arc<CompletionData>,
    pub workspace: Option<Arc<LapceWorkspace>>,
    pub main_split: LapceMainSplitData,
    pub find: Arc<Find>,
    pub proxy: Arc<LapceProxy>,
    pub config: Arc<Config>,
}

impl LapceEditorBufferData {
    fn buffer_mut(&mut self) -> &mut BufferNew {
        Arc::make_mut(&mut self.buffer)
    }

    fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
        let cursor_offset = self.editor.cursor.offset();
        if self.buffer.cursor_offset != cursor_offset
            || self.buffer.scroll_offset != scroll_offset
        {
            let buffer = self.buffer_mut();
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
            self.do_move(&Movement::Offset(new_index + line_start_offset), 1);
        }
    }

    fn get_code_actions(&self, ctx: &mut EventCtx) {
        if !self.buffer.loaded {
            return;
        }
        if self.buffer.local {
            return;
        }
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        if self.buffer.code_actions.get(&prev_offset).is_none() {
            let buffer_id = self.buffer.id;
            let position = self.buffer.offset_to_position(prev_offset);
            let path = self.buffer.path.clone();
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
                            event_sink.submit_command(
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

    fn do_move(&mut self, movement: &Movement, count: usize) {
        if movement.is_jump() && movement != &self.editor.last_movement {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.last_movement = movement.clone();
        match &self.editor.cursor.mode {
            &CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    offset,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                );
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Normal(new_offset);
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    *end,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                );
                let start = *start;
                let mode = mode.clone();
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
                    false,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id {
                match &editor.content {
                    EditorContent::Buffer(path) => {
                        if &self.buffer.path == path {
                            Arc::make_mut(editor).cursor.apply_delta(delta);
                        }
                    }
                    _ => (),
                }
            }
        }
    }

    fn apply_completion_item(
        &mut self,
        ctx: &mut EventCtx,
        item: &CompletionItem,
    ) -> Result<()> {
        let additioal_edit = item.additional_text_edits.as_ref().map(|edits| {
            edits
                .iter()
                .map(|edit| {
                    let selection = Selection::region(
                        self.buffer.offset_of_position(&edit.range.start),
                        self.buffer.offset_of_position(&edit.range.end),
                    );
                    (selection, edit.new_text.clone())
                })
                .collect::<Vec<(Selection, String)>>()
        });
        let additioal_edit = additioal_edit.as_ref().map(|edits| {
            edits
                .into_iter()
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
                    let edit_start =
                        self.buffer.offset_of_position(&edit.range.start);
                    let edit_end = self.buffer.offset_of_position(&edit.range.end);
                    let selection = Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PlainText => {
                            let (selection, _) = self.edit(
                                ctx,
                                &selection,
                                &edit.new_text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );
                            self.set_cursor_after_change(selection);
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::Snippet => {
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let (selection, delta) = self.edit(
                                ctx,
                                &selection,
                                &text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer
                                .transform(start_offset.min(edit_start), false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.len() == 0 {
                                self.set_cursor_after_change(selection);
                                return Ok(());
                            }

                            let mut selection = Selection::new();
                            let (tab, (start, end)) = &snippet_tabs[0];
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

        let (selection, _) = self.edit(
            ctx,
            &selection,
            item.insert_text.as_ref().unwrap_or(&item.label),
            additioal_edit,
            true,
            EditType::InsertChars,
        );
        self.set_cursor_after_change(selection);
        Ok(())
    }

    fn cancel_completion(&mut self) {
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
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
        if input == "" && char != "." && char != ":" {
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
                    self.buffer.offset_to_position(start_offset),
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
                    self.buffer.offset_to_position(offset),
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
            self.buffer.offset_to_position(start_offset),
            completion.id,
            event_sink.clone(),
        );
        if input != "" {
            completion.request(
                self.proxy.clone(),
                completion.request_id,
                self.buffer.id,
                input,
                self.buffer.offset_to_position(offset),
                completion.id,
                event_sink,
            );
        }
    }

    fn cursor_region(&self, text: &mut PietText, config: &Config) -> Rect {
        let offset = self.editor.cursor.offset();
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let width = config.editor_text_width(text, "W");
        let cursor_x = col as f64 * width - width;
        let line_height = config.editor.line_height as f64;
        let cursor_x = if cursor_x < 0.0 { 0.0 } else { cursor_x };
        let line = if line > 1 { line - 1 } else { 0 };
        Rect::ZERO
            .with_origin(Point::new(cursor_x.floor(), line as f64 * line_height))
            .with_size(Size::new((width * 3.0).ceil(), line_height * 3.0))
    }

    fn insert_new_line(&mut self, ctx: &mut EventCtx, offset: usize) {
        let line = self.buffer.line_of_offset(offset);
        let line_start = self.buffer.offset_of_line(line);
        let line_end = self.buffer.line_end_offset(line, true);
        let line_indent = self.buffer.indent_on_line(line);
        let first_half = self.buffer.slice_to_cow(line_start..offset).to_string();
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

        let selection = Selection::caret(offset);
        let content = format!("{}{}", "\n", indent);

        let (selection, _) = self.edit(
            ctx,
            &selection,
            &content,
            None,
            true,
            EditType::InsertNewline,
        );
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor.mode = CursorMode::Insert(selection.clone());
        editor.cursor.horiz = None;

        for c in first_half.chars().rev() {
            if c != ' ' {
                if let Some(pair_start) = matching_pair_direction(c) {
                    if pair_start {
                        if let Some(c) = matching_char(c) {
                            if second_half.trim().starts_with(&c.to_string()) {
                                let content = format!("{}{}", "\n", line_indent);
                                self.edit(
                                    ctx,
                                    &selection,
                                    &content,
                                    None,
                                    true,
                                    EditType::InsertNewline,
                                );
                            }
                        }
                    }
                }
                break;
            }
        }
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
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                };
                let after = !data.content.contains("\n");
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &data.content,
                    None,
                    after,
                    EditType::InsertChars,
                );
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
                    CursorMode::Insert { .. } => (
                        self.editor.cursor.edit_selection(&self.buffer),
                        "\n".to_string() + &data.content,
                    ),
                    CursorMode::Visual { mode, .. } => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let data = match mode {
                            VisualMode::Linewise => data.content.clone(),
                            _ => "\n".to_string() + &data.content,
                        };
                        (selection, data)
                    }
                };
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &content,
                    None,
                    false,
                    EditType::InsertChars,
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
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor;
    }

    fn jump_to_nearest_delta(&mut self, delta: &RopeDelta) {
        let mut transformer = Transformer::new(delta);

        let offset = self.editor.cursor.offset();
        let offset = transformer.transform(offset, false);
        let (ins, del) = delta.clone().factor();
        let ins = ins.transform_shrink(&del);
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
        if let Some(new_offset) = positions.iter().next() {
            let selection = Selection::caret(*new_offset);
            self.set_cursor_after_change(selection);
        }
    }

    fn initiate_diagnositcs_offset(&mut self) {
        let buffer = self.buffer.clone();
        if let Some(diagnostics) = self.diagnostics_mut() {
            for diagnostic in diagnostics.iter_mut() {
                if diagnostic.range.is_none() {
                    diagnostic.range = Some((
                        buffer
                            .offset_of_position(&diagnostic.diagnositc.range.start),
                        buffer.offset_of_position(&diagnostic.diagnositc.range.end),
                    ));
                }
            }
        }
    }

    fn update_diagnositcs_offset(&mut self, delta: &RopeDelta) {
        let buffer = self.buffer.clone();
        if let Some(diagnostics) = self.diagnostics_mut() {
            for diagnostic in diagnostics.iter_mut() {
                let mut transformer = Transformer::new(delta);
                let (start, end) = diagnostic.range.clone().unwrap();
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );
                diagnostic.range = Some((new_start, new_end));
                if start != new_start {
                    diagnostic.diagnositc.range.start =
                        buffer.offset_to_position(new_start);
                }
                if end != new_end {
                    diagnostic.diagnositc.range.end =
                        buffer.offset_to_position(new_end);
                }
            }
        }
    }

    fn edit(
        &mut self,
        ctx: &mut EventCtx,
        selection: &Selection,
        c: &str,
        additional_edit: Option<Vec<(&Selection, &str)>>,
        after: bool,
        edit_type: EditType,
    ) -> (Selection, RopeDelta) {
        match &self.editor.cursor.mode {
            CursorMode::Normal(_) => {
                if !selection.is_caret() {
                    let data = self.editor.cursor.yank(&self.buffer);
                    let register = Arc::make_mut(&mut self.main_split.register);
                    register.add_delete(data);
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let data = self.editor.cursor.yank(&self.buffer);
                let register = Arc::make_mut(&mut self.main_split.register);
                register.add_delete(data);
            }
            CursorMode::Insert(_) => {}
        }

        self.initiate_diagnositcs_offset();

        let proxy = self.proxy.clone();
        let buffer = self.buffer_mut();
        let delta = if let Some(additional_edit) = additional_edit {
            let mut edits = vec![(selection, c)];
            edits.extend_from_slice(&additional_edit);
            buffer.edit_multiple(ctx, edits, proxy, edit_type)
        } else {
            buffer.edit(ctx, &selection, c, proxy, edit_type)
        };
        self.inactive_apply_delta(&delta);
        let selection = selection.apply_delta(&delta, after, InsertDrift::Default);
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

        (selection, delta)
    }

    fn next_error(&mut self, ctx: &mut EventCtx, env: &Env) {
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
                if errors.len() == 0 {
                    None
                } else {
                    errors.sort();
                    Some((path, errors))
                }
            })
            .collect::<Vec<(&PathBuf, Vec<Position>)>>();
        if file_diagnostics.len() == 0 {
            return;
        }
        file_diagnostics.sort_by(|a, b| a.0.cmp(b.0));

        let offset = self.editor.cursor.offset();
        let position = self.buffer.offset_to_position(offset);
        let (path, position) = next_in_file_errors_offset(
            position,
            &self.buffer.path,
            &file_diagnostics,
        );
        let location = EditorLocationNew {
            path,
            position: Some(position),
            scroll_offset: None,
        };
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::JumpToLocation(EditorKind::SplitActive, location),
            Target::Auto,
        ));
    }

    fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.locations.len() == 0 {
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
        env: &Env,
    ) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
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

    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, env: &Env) {
        let line_height = self.config.editor.line_height as f64;
        let lines =
            (self.editor.size.borrow().height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.do_move(if down { &Movement::Down } else { &Movement::Up }, lines);
        let rect = Rect::ZERO
            .with_origin(
                self.editor.scroll_offset.to_point()
                    + Vec2::new(0.0, if down { distance } else { -distance }),
            )
            .with_size(self.editor.size.borrow().clone());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureRectVisible(rect),
            Target::Widget(self.editor.view_id),
        ));
    }

    fn scroll(&mut self, ctx: &mut EventCtx, down: bool, count: usize, env: &Env) {
        let line_height = self.config.editor.line_height as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, col) = self.buffer.offset_to_line_col(offset);
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

        if new_line > line {
            self.do_move(&Movement::Down, new_line - line);
        } else if new_line < line {
            self.do_move(&Movement::Up, line - new_line);
        }
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

    fn move_command(
        &self,
        count: Option<usize>,
        cmd: &LapceCommand,
    ) -> Option<Movement> {
        match cmd {
            LapceCommand::Left => Some(Movement::Left),
            LapceCommand::Right => Some(Movement::Right),
            LapceCommand::Up => Some(Movement::Up),
            LapceCommand::Down => Some(Movement::Down),
            LapceCommand::LineStart => Some(Movement::StartOfLine),
            LapceCommand::LineEnd => Some(Movement::EndOfLine),
            LapceCommand::GotoLineDefaultFirst => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            }),
            LapceCommand::GotoLineDefaultLast => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            }),
            LapceCommand::WordBackward => Some(Movement::WordBackward),
            LapceCommand::WordFoward => Some(Movement::WordForward),
            LapceCommand::WordEndForward => Some(Movement::WordEndForward),
            LapceCommand::MatchPairs => Some(Movement::MatchPairs),
            LapceCommand::NextUnmatchedRightBracket => {
                Some(Movement::NextUnmatched(')'))
            }
            LapceCommand::PreviousUnmatchedLeftBracket => {
                Some(Movement::PreviousUnmatched('('))
            }
            LapceCommand::NextUnmatchedRightCurlyBracket => {
                Some(Movement::NextUnmatched('}'))
            }
            LapceCommand::PreviousUnmatchedLeftCurlyBracket => {
                Some(Movement::PreviousUnmatched('{'))
            }
            _ => None,
        }
    }

    fn current_code_actions(&self) -> Option<&CodeActionResponse> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        self.buffer.code_actions.get(&prev_offset)
    }

    fn diagnostics(&self) -> Option<&Arc<Vec<EditorDiagnostic>>> {
        self.main_split.diagnostics.get(&self.buffer.path)
    }

    fn diagnostics_mut(&mut self) -> Option<&mut Vec<EditorDiagnostic>> {
        self.main_split
            .diagnostics
            .get_mut(&self.buffer.path)
            .map(|d| Arc::make_mut(d))
    }

    fn paint_gutter(&self, ctx: &mut PaintCtx, gutter_width: f64) {
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            self.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );
        let line_height = self.config.editor.line_height as f64;
        let scroll_offset = self.editor.scroll_offset;
        let start_line = (scroll_offset.y / line_height).floor() as usize;
        let num_lines = (ctx.size().height / line_height).floor() as usize;
        let last_line = self.buffer.last_line();
        let current_line = self.editor.cursor.current_line(&self.buffer);
        let width = self.config.editor_text_width(ctx.text(), "W");
        for line in start_line..start_line + num_lines + 1 {
            if line > last_line {
                break;
            }
            let content = if *self.main_split.active != self.view_id {
                line + 1
            } else if self.editor.cursor.is_insert() {
                line + 1
            } else {
                if line == current_line {
                    line + 1
                } else if line > current_line {
                    line - current_line
                } else {
                    current_line - line
                }
            };
            let x = ((last_line + 1).to_string().len() - content.to_string().len())
                as f64
                * width;
            let y = line_height * line as f64 + 5.0 - scroll_offset.y;
            let pos = Point::new(x, y);
            let content = content.to_string();

            let text_layout = ctx
                .text()
                .new_text_layout(content)
                .font(
                    self.config.editor.font_family(),
                    self.config.editor.font_size as f64,
                )
                .text_color(if line == current_line {
                    self.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone()
                } else {
                    self.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone()
                })
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, pos);

            if let Some(line_change) = self.buffer.line_changes.get(&line) {
                let x = gutter_width + width;
                let y = line as f64 * line_height - scroll_offset.y;
                let origin = Point::new(x, y);
                let size = Size::new(3.0, line_height);
                let rect = Rect::ZERO.with_origin(origin).with_size(size);
                match line_change {
                    'm' => {
                        ctx.fill(rect, &Color::rgba8(1, 132, 188, 180));
                    }
                    '+' => {
                        ctx.fill(rect, &Color::rgba8(80, 161, 79, 180));
                    }
                    '-' => {
                        let size = Size::new(3.0, 10.0);
                        let x = gutter_width + width;
                        let y = line as f64 * line_height
                            - size.height / 2.0
                            - scroll_offset.y;
                        let origin = Point::new(x, y);
                        let rect = Rect::ZERO.with_origin(origin).with_size(size);
                        ctx.fill(rect, &Color::rgba8(228, 86, 73, 180));
                    }
                    _ => {}
                }
            }
        }

        if *self.main_split.active == self.view_id {
            self.paint_code_actions_hint(ctx, gutter_width);
        }
    }

    fn paint_code_actions_hint(&self, ctx: &mut PaintCtx, gutter_width: f64) {
        if let Some(actions) = self.current_code_actions() {
            if actions.len() > 0 {
                let line_height = self.config.editor.line_height as f64;
                let offset = self.editor.cursor.offset();
                let (line, _) = self.buffer.offset_to_line_col(offset);
                let svg = get_svg("lightbulb.svg").unwrap();
                let width = 16.0;
                let height = 16.0;
                let char_width = self.config.editor_text_width(ctx.text(), "W");
                let rect =
                    Size::new(width, height).to_rect().with_origin(Point::new(
                        gutter_width + char_width + 3.0,
                        (line_height - height) / 2.0 + line_height * line as f64
                            - self.editor.scroll_offset.y,
                    ));
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(self.config.get_color_unchecked(LapceTheme::LAPCE_WARN)),
                );
            }
        }
    }

    fn paint_content(
        &self,
        ctx: &mut PaintCtx,
        is_focused: bool,
        placeholder: Option<&String>,
        config: &Config,
    ) {
        let line_height = self.config.editor.line_height as f64;
        self.paint_cursor(ctx, is_focused, placeholder, config);
        self.paint_find(ctx);
        let rect = ctx.region().bounding_box();
        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;

        let text_layout = ctx
            .text()
            .new_text_layout("W")
            .font(
                self.config.editor.font_family(),
                self.config.editor.font_size as f64,
            )
            .build()
            .unwrap();
        let y_shift = (line_height - text_layout.size().height) / 2.0;

        let cursor_offset = self.editor.cursor.offset();
        let cursor_line = self.buffer.line_of_offset(cursor_offset);
        let start_offset = self.buffer.offset_of_line(start_line);
        let end_offset = self.buffer.offset_of_line(end_line + 1);
        let mode = self.editor.cursor.get_mode();
        for (i, line_content) in self
            .buffer
            .slice_to_cow(start_offset..end_offset)
            .split('\n')
            .enumerate()
        {
            let line = i + start_line;
            let cursor_index =
                if is_focused && mode != Mode::Insert && line == cursor_line {
                    let cursor_line_start = self.buffer.offset_of_line(cursor_line);
                    let index = self
                        .buffer
                        .slice_to_cow(cursor_line_start..cursor_offset)
                        .chars()
                        .count();
                    Some(index)
                } else {
                    None
                };
            let text_layout = self.buffer.new_text_layout(
                ctx,
                line,
                line_content,
                cursor_index,
                [rect.x0, rect.x1],
                &self.config,
            );
            ctx.draw_text(
                &text_layout,
                Point::new(0.0, line_height * line as f64 + y_shift),
            );
        }

        self.paint_snippet(ctx);
        self.paint_diagnostics(ctx);
        if self.buffer.len() == 0 {
            if let Some(placeholder) = placeholder {
                let text_layout = ctx
                    .text()
                    .new_text_layout(placeholder.to_string())
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        self.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(0.0, y_shift));
            }
        }
    }

    fn paint_cursor(
        &self,
        ctx: &mut PaintCtx,
        is_focused: bool,
        placeholder: Option<&String>,
        config: &Config,
    ) {
        let line_height = self.config.editor.line_height as f64;
        let start_line =
            (self.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((self.editor.size.borrow().height
            + self.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = self.config.editor_text_width(ctx.text(), "W");
        match &self.editor.cursor.mode {
            CursorMode::Normal(offset) => {
                let (line, col) = self.buffer.offset_to_line_col(*offset);
                self.paint_cursor_line(ctx, line, is_focused, placeholder);

                if is_focused {
                    let cursor_x = col as f64 * width;
                    let next = self.buffer.next_grapheme_offset(
                        *offset,
                        1,
                        self.buffer.len(),
                    );
                    let char = self.buffer.slice_to_cow(*offset..next).to_string();
                    let char_width = if char == "\t" {
                        4
                    } else {
                        UnicodeWidthStr::width(char.as_str()).max(1)
                    };
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(
                                cursor_x,
                                line as f64 * line_height,
                            ))
                            .with_size(Size::new(
                                width * char_width as f64,
                                line_height,
                            )),
                        self.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                    );
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    self.buffer.offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    self.buffer.offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = self
                        .buffer
                        .slice_to_cow(
                            self.buffer.offset_of_line(line)
                                ..self.buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    let left_col = match mode {
                        &VisualMode::Normal => match line {
                            _ if line == start_line => start_col,
                            _ => 0,
                        },
                        &VisualMode::Linewise => 0,
                        &VisualMode::Blockwise => {
                            let max_col = self.buffer.line_end_col(line, false);
                            let left = start_col.min(end_col);
                            if left > max_col {
                                continue;
                            }
                            left
                        }
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match mode {
                        &VisualMode::Normal => match line {
                            _ if line == end_line => {
                                let max_col = self.buffer.line_end_col(line, true);
                                (end_col + 1).min(max_col)
                            }
                            _ => self.buffer.line_end_col(line, true) + 1,
                        },
                        &VisualMode::Linewise => {
                            self.buffer.line_end_col(line, true) + 1
                        }
                        &VisualMode::Blockwise => {
                            let max_col = self.buffer.line_end_col(line, true);
                            let right = match self.editor.cursor.horiz.as_ref() {
                                Some(&ColPosition::End) => max_col,
                                _ => (end_col.max(start_col) + 1).min(max_col),
                            };
                            right
                        }
                    };
                    if line_content.len() > 0 {
                        let x1 = right_col as f64 * width;

                        let y0 = line as f64 * line_height;
                        let y1 = y0 + line_height;
                        ctx.fill(
                            Rect::new(x0, y0, x1, y1),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                        );
                    }

                    if is_focused {
                        let (line, col) = self.buffer.offset_to_line_col(*end);
                        let cursor_x = col as f64 * width;
                        let next = self.buffer.next_grapheme_offset(
                            *end,
                            1,
                            self.buffer.len(),
                        );
                        let char = self.buffer.slice_to_cow(*end..next).to_string();
                        let char_width = if char == "\t" {
                            4
                        } else {
                            UnicodeWidthStr::width(char.as_str()).max(1)
                        };
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(
                                    cursor_x,
                                    line as f64 * line_height,
                                ))
                                .with_size(Size::new(
                                    width * char_width as f64,
                                    line_height,
                                )),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                        );
                    }
                }
            }
            CursorMode::Insert(selection) => {
                let offset = selection.get_cursor_offset();
                let line = self.buffer.line_of_offset(offset);
                let last_line = self.buffer.last_line();
                let end_line = if end_line > last_line {
                    last_line
                } else {
                    end_line
                };
                let start = self.buffer.offset_of_line(start_line);
                let end = self.buffer.offset_of_line(end_line + 1);
                let regions = selection.regions_in_range(start, end);
                for region in regions {
                    if region.start() == region.end() {
                        let line = self.buffer.line_of_offset(region.start());
                        self.paint_cursor_line(ctx, line, is_focused, placeholder);
                    } else {
                        let start = region.start();
                        let end = region.end();
                        let paint_start_line = start_line;
                        let paint_end_line = end_line;
                        let (start_line, start_col) =
                            self.buffer.offset_to_line_col(start.min(end));
                        let (end_line, end_col) =
                            self.buffer.offset_to_line_col(start.max(end));
                        for line in paint_start_line..paint_end_line {
                            if line < start_line || line > end_line {
                                continue;
                            }

                            let line_content = self.buffer.line_content(line);
                            let left_col = match line {
                                _ if line == start_line => start_col,
                                _ => 0,
                            };
                            let x0 = left_col as f64 * width;

                            let right_col = match line {
                                _ if line == end_line => {
                                    let max_col =
                                        self.buffer.line_end_col(line, true);
                                    end_col.min(max_col)
                                }
                                _ => self.buffer.line_end_col(line, true),
                            };

                            if line_content.len() > 0 {
                                let x1 = right_col as f64 * width;
                                let y0 = line as f64 * line_height;
                                let y1 = y0 + line_height;
                                ctx.fill(
                                    Rect::new(x0, y0, x1, y1),
                                    self.config.get_color_unchecked(
                                        LapceTheme::EDITOR_SELECTION,
                                    ),
                                );
                            }
                        }
                    }

                    if is_focused {
                        let (line, col) =
                            self.buffer.offset_to_line_col(region.end());
                        let x = (col as f64 * width).round();
                        let y = line as f64 * line_height;
                        ctx.stroke(
                            Line::new(
                                Point::new(x, y),
                                Point::new(x, y + line_height),
                            ),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        )
                    }
                }
            }
        }
    }

    fn paint_cursor_line(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        is_focused: bool,
        placeholder: Option<&String>,
    ) {
        if !is_focused && self.buffer.len() == 0 && placeholder.is_some() {
            return;
        }
        let line_height = self.config.editor.line_height as f64;
        let size = ctx.size();
        ctx.fill(
            Rect::ZERO
                .with_origin(Point::new(0.0, line as f64 * line_height))
                .with_size(Size::new(size.width, line_height)),
            self.config
                .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
        );
    }

    fn paint_find(&self, ctx: &mut PaintCtx) {
        let line_height = self.config.editor.line_height as f64;
        let start_line =
            (self.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((self.editor.size.borrow().height
            + self.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = self.config.editor_text_width(ctx.text(), "W");
        let start_offset = self.buffer.offset_of_line(start_line);
        let end_offset = self.buffer.offset_of_line(end_line + 1);

        self.buffer.update_find(&self.find, start_line, end_line);
        if self.find.search_string.is_some() {
            for region in self
                .buffer
                .find
                .borrow()
                .occurrences()
                .regions_in_range(start_offset, end_offset)
            {
                let start = region.min();
                let end = region.max();
                let (start_line, start_col) = self.buffer.offset_to_line_col(start);
                let (end_line, end_col) = self.buffer.offset_to_line_col(end);
                for line in start_line..end_line + 1 {
                    let left_col = if line == start_line { start_col } else { 0 };
                    let right_col = if line == end_line {
                        end_col
                    } else {
                        self.buffer.line_end_col(line, true) + 1
                    };
                    let x0 = left_col as f64 * width;
                    let x1 = right_col as f64 * width;
                    let y0 = line as f64 * line_height;
                    let y1 = y0 + line_height;
                    ctx.stroke(
                        Rect::new(x0, y0, x1, y1),
                        self.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        1.0,
                    );
                }
            }
        }
    }

    fn paint_snippet(&self, ctx: &mut PaintCtx) {
        let line_height = self.config.editor.line_height as f64;
        let start_line =
            (self.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((self.editor.size.borrow().height
            + self.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = self.config.editor_text_width(ctx.text(), "W");
        if let Some(snippet) = self.editor.snippet.as_ref() {
            for (_, (start, end)) in snippet {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    self.buffer.offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    self.buffer.offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = self.buffer.line_content(line);
                    let left_col = match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match line {
                        _ if line == end_line => {
                            let max_col = self.buffer.line_end_col(line, true);
                            end_col.min(max_col)
                        }
                        _ => self.buffer.line_end_col(line, true),
                    };
                    if line_content.len() > 0 {
                        let x1 = right_col as f64 * width;
                        let y0 = line as f64 * line_height;
                        let y1 = y0 + line_height;
                        ctx.stroke(
                            Rect::new(x0, y0, x1, y1).inflate(1.0, -0.5),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                            1.0,
                        );
                    }
                }
            }
        }
    }

    fn paint_diagnostics(&self, ctx: &mut PaintCtx) {
        let line_height = self.config.editor.line_height as f64;
        let start_line =
            (self.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((self.editor.size.borrow().height
            + self.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;

        let width = self.config.editor_text_width(ctx.text(), "W");
        let mut current = None;
        let cursor_offset = self.editor.cursor.offset();
        if let Some(diagnostics) = self.diagnostics() {
            for diagnostic in diagnostics.iter() {
                let start = diagnostic.diagnositc.range.start;
                let end = diagnostic.diagnositc.range.end;
                if (start.line as usize) <= end_line
                    && (end.line as usize) >= start_line
                {
                    let start_offset = if let Some(range) = diagnostic.range {
                        range.0
                    } else {
                        self.buffer.offset_of_position(&start)
                    };
                    if start_offset == cursor_offset {
                        current = Some(diagnostic.clone());
                    }
                    for line in start.line as usize..end.line as usize + 1 {
                        if line < start_line {
                            continue;
                        }
                        if line > end_line {
                            break;
                        }

                        let x0 = if line == start.line as usize {
                            start.character as f64 * width
                        } else {
                            let (_, col) = self.buffer.offset_to_line_col(
                                self.buffer.first_non_blank_character_on_line(line),
                            );
                            col as f64 * width
                        };
                        let x1 = if line == end.line as usize {
                            end.character as f64 * width
                        } else {
                            (self.buffer.line_end_col(line, false) + 1) as f64
                                * width
                        };
                        let y1 = (line + 1) as f64 * line_height;
                        let y0 = (line + 1) as f64 * line_height - 4.0;

                        let severity = diagnostic
                            .diagnositc
                            .severity
                            .as_ref()
                            .unwrap_or(&DiagnosticSeverity::Information);
                        let color = match severity {
                            DiagnosticSeverity::Error => self
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_ERROR),
                            DiagnosticSeverity::Warning => self
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_WARN),
                            _ => self
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_WARN),
                        };
                        paint_wave_line(ctx, Point::new(x0, y0), x1 - x0, &color);
                    }
                }
            }
        }

        if let Some(diagnostic) = current {
            if self.editor.cursor.is_normal() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(diagnostic.diagnositc.message.clone())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(
                        self.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .max_width(self.editor.size.borrow().width - 20.0)
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let mut text_height = text_size.height;

                let related = diagnostic
                    .diagnositc
                    .related_information
                    .map(|related| {
                        related
                            .iter()
                            .map(|i| {
                                let text_layout = ctx
                                    .text()
                                    .new_text_layout(i.message.clone())
                                    .font(FontFamily::SYSTEM_UI, 14.0)
                                    .text_color(
                                        self.config
                                            .get_color_unchecked(
                                                LapceTheme::EDITOR_FOREGROUND,
                                            )
                                            .clone(),
                                    )
                                    .max_width(
                                        self.editor.size.borrow().width - 20.0,
                                    )
                                    .build()
                                    .unwrap();
                                text_height += 10.0 + text_layout.size().height;
                                text_layout
                            })
                            .collect::<Vec<PietTextLayout>>()
                    })
                    .unwrap_or(Vec::new());

                let start = diagnostic.diagnositc.range.start;
                let rect = Rect::ZERO
                    .with_origin(Point::new(
                        0.0,
                        (start.line + 1) as f64 * line_height,
                    ))
                    .with_size(Size::new(
                        self.editor.size.borrow().width,
                        text_height + 20.0,
                    ));
                ctx.fill(
                    rect,
                    self.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );

                let severity = diagnostic
                    .diagnositc
                    .severity
                    .as_ref()
                    .unwrap_or(&DiagnosticSeverity::Information);
                let color = match severity {
                    DiagnosticSeverity::Error => {
                        self.config.get_color_unchecked(LapceTheme::LAPCE_ERROR)
                    }
                    DiagnosticSeverity::Warning => {
                        self.config.get_color_unchecked(LapceTheme::LAPCE_WARN)
                    }
                    _ => self.config.get_color_unchecked(LapceTheme::LAPCE_WARN),
                };
                ctx.stroke(rect, color, 1.0);
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        10.0 + self.editor.scroll_offset.x,
                        (start.line + 1) as f64 * line_height + 10.0,
                    ),
                );
                let mut text_height = text_size.height;

                for text in related {
                    text_height += 10.0;
                    ctx.draw_text(
                        &text,
                        Point::new(
                            10.0 + self.editor.scroll_offset.x,
                            (start.line + 1) as f64 * line_height
                                + 10.0
                                + text_height,
                        ),
                    );
                    text_height += text.size().height;
                }
            }
        }
    }
}

pub struct LapceEditorEmptyContent {}

pub enum LapceEditorViewContent {
    Buffer(LapceEditorBufferData),
    None,
}

impl KeyPressFocus for LapceEditorEmptyContent {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: &str) -> bool {
        false
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {}
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
            "editor_focus" => true,
            "source_control_focus" => {
                self.editor.editor_type == EditorType::SourceControl
            }
            "in_snippet" => self.editor.snippet.is_some(),
            "list_focus" => {
                self.completion.status != CompletionStatus::Inactive
                    && self.completion.len() > 0
            }
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
        if let Some(movement) = cmd.move_command(count) {
            self.do_move(&movement, count.unwrap_or(1));
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
            return;
        }
        match cmd {
            LapceCommand::SplitLeft => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Left,
                            self.editor.view_id,
                        ),
                        Target::Widget(split_id),
                    ));
                }
            }
            LapceCommand::SplitRight => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Right,
                            self.editor.view_id,
                        ),
                        Target::Widget(split_id),
                    ));
                }
            }
            LapceCommand::SplitUp => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Up,
                            self.editor.view_id,
                        ),
                        Target::Widget(split_id),
                    ));
                }
            }
            LapceCommand::SplitDown => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Down,
                            self.editor.view_id,
                        ),
                        Target::Widget(split_id),
                    ));
                }
            }
            LapceCommand::SplitExchange => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    if self.editor.editor_type == EditorType::Normal {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::SplitEditorExchange(self.editor.view_id),
                            Target::Widget(split_id),
                        ));
                    }
                }
            }
            LapceCommand::SplitVertical => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    if self.editor.editor_type == EditorType::Normal {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::SplitEditor(true, self.editor.view_id),
                            Target::Widget(split_id),
                        ));
                    }
                }
            }
            LapceCommand::SplitClose => {
                if let Some(split_id) = self.editor.split_id.clone() {
                    if self.editor.editor_type == EditorType::Normal {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::SplitEditorClose(self.editor.view_id),
                            Target::Widget(split_id),
                        ));
                    }
                }
            }
            LapceCommand::Undo => {
                self.initiate_diagnositcs_offset();
                let proxy = self.proxy.clone();
                let buffer = self.buffer_mut();
                if let Some(delta) = buffer.do_undo(proxy) {
                    self.jump_to_nearest_delta(&delta);
                    self.update_diagnositcs_offset(&delta);
                }
            }
            LapceCommand::Redo => {
                self.initiate_diagnositcs_offset();
                let proxy = self.proxy.clone();
                let buffer = self.buffer_mut();
                if let Some(delta) = buffer.do_redo(proxy) {
                    self.jump_to_nearest_delta(&delta);
                    self.update_diagnositcs_offset(&delta);
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
                    )
                    .0;
                self.buffer_mut().update_edit_type();
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
                );
                self.buffer_mut().update_edit_type();
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(Selection::caret(offset)),
                    Some(horiz),
                ));
            }
            LapceCommand::InsertMode => {
                Arc::make_mut(&mut self.editor).cursor.mode = CursorMode::Insert(
                    Selection::caret(self.editor.cursor.offset()),
                );
                self.buffer_mut().update_edit_type();
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
                        );
                        self.buffer_mut().update_edit_type();
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(Selection::caret(offset)),
                            Some(horiz),
                        ));
                    }
                    CursorMode::Visual { start, end, mode } => {
                        let mut selection = Selection::new();
                        for region in
                            self.editor.cursor.edit_selection(&self.buffer).regions()
                        {
                            selection.add_region(SelRegion::caret(region.min()));
                        }
                        self.buffer_mut().update_edit_type();
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
                self.insert_new_line(ctx, offset);
            }
            LapceCommand::NewLineBelow => {
                let offset = self.editor.cursor.offset();
                let offset = self.buffer.offset_line_end(offset, true);
                self.insert_new_line(ctx, offset);
            }
            LapceCommand::DeleteToBeginningOfLine => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::StartOfLine,
                            Mode::Insert,
                            true,
                        );
                        selection
                    }
                };
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
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
                let data = self.editor.cursor.yank(&self.buffer);
                let register = Arc::make_mut(&mut self.main_split.register);
                register.add_yank(data);
                match &self.editor.cursor.mode {
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
            LapceCommand::ClipboardCopy => {
                let data = self.editor.cursor.yank(&self.buffer);
                Application::global().clipboard().put_string(data.content);
                match &self.editor.cursor.mode {
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
            LapceCommand::ClipboardPaste => {
                if let Some(s) = Application::global().clipboard().get_string() {
                    let data = RegisterData {
                        content: s.to_string(),
                        mode: VisualMode::Normal,
                    };
                    self.paste(ctx, &data);
                }
            }
            LapceCommand::Paste => {
                let data = self.main_split.register.unamed.clone();
                self.paste(ctx, &data);
            }
            LapceCommand::DeleteWordBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::WordBackward,
                            Mode::Insert,
                            true,
                        );
                        selection
                    }
                };
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let mut selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::Left,
                            Mode::Insert,
                            true,
                        );
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
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForeward => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForewardAndInsert => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
                self.update_completion(ctx);
            }
            LapceCommand::InsertNewLine => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                if selection.regions().len() > 1 {
                    let (selection, _) = self.edit(
                        ctx,
                        &selection,
                        "\n",
                        None,
                        true,
                        EditType::InsertNewline,
                    );
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                    return;
                };
                self.insert_new_line(ctx, self.editor.cursor.offset());
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
                self.scroll(ctx, true, count.unwrap_or(1), env);
            }
            LapceCommand::ScrollUp => {
                self.scroll(ctx, false, count.unwrap_or(1), env);
            }
            LapceCommand::PageDown => {
                self.page_move(ctx, true, env);
            }
            LapceCommand::PageUp => {
                self.page_move(ctx, false, env);
            }
            LapceCommand::JumpLocationBackward => {
                self.jump_location_backward(ctx, env);
            }
            LapceCommand::JumpLocationForward => {
                self.jump_location_forward(ctx, env);
            }
            LapceCommand::NextError => {
                self.next_error(ctx, env);
            }
            LapceCommand::PreviousError => {}
            LapceCommand::ListNext => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.next();
            }
            LapceCommand::ListPrevious => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.previous();
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
                let selection = self.editor.cursor.edit_selection(&self.buffer);

                let count = self.completion.input.len();
                let selection = if count > 0 {
                    self.buffer.update_selection(
                        &selection,
                        count,
                        &Movement::Left,
                        Mode::Insert,
                        true,
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
                            println!("completion resolve result {:?}", result);
                            let mut item = item.clone();
                            if let Ok(res) = result {
                                if let Ok(i) =
                                    serde_json::from_value::<CompletionItem>(res)
                                {
                                    item = i;
                                }
                            };
                            event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ResolveCompletion(
                                    buffer_id, rev, offset, item,
                                ),
                                Target::Widget(view_id),
                            );
                        }),
                    );
                } else {
                    self.apply_completion_item(ctx, &item);
                }
            }
            LapceCommand::NormalMode => {
                if !self.config.lapce.modal {
                    return;
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
                            )
                            .0
                    }
                    CursorMode::Visual { start, end, mode } => {
                        self.buffer.offset_line_end(*end, false).min(*end)
                    }
                    CursorMode::Normal(offset) => *offset,
                };
                self.buffer_mut().update_edit_type();

                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Normal(offset);
                editor.cursor.horiz = None;
                editor.snippet = None;
                editor.inline_find = None;
                self.cancel_completion();
            }
            LapceCommand::GotoDefinition => {
                let offset = self.editor.cursor.offset();
                let start_offset = self.buffer.prev_code_boundary(offset);
                let start_position = self.buffer.offset_to_position(start_offset);
                let event_sink = ctx.get_external_handle();
                let buffer_id = self.buffer.id;
                let position = self.buffer.offset_to_position(offset);
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
                                        if locations.len() > 0 {
                                            Some(locations[0].clone())
                                        } else {
                                            None
                                        }
                                    }
                                    GotoDefinitionResponse::Link(location_links) => {
                                        None
                                    }
                                } {
                                    if location.range.start == start_position {
                                        proxy.get_references(
                                            buffer_id,
                                            position,
                                            Box::new(move |result| {
                                                process_get_references(
                                                    editor_view_id,
                                                    offset,
                                                    result,
                                                    event_sink,
                                                );
                                            }),
                                        );
                                    } else {
                                        event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition(
                                                editor_view_id,
                                                offset,
                                                EditorLocationNew {
                                                    path: PathBuf::from(
                                                        location.uri.path(),
                                                    ),
                                                    position: Some(
                                                        location.range.start,
                                                    ),
                                                    scroll_offset: None,
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
                if self.editor.editor_type == EditorType::SourceControl {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::FocusEditor,
                        Target::Auto,
                    ));
                    println!("source control cancel");
                }
            }
            LapceCommand::ShowCodeActions => {
                if let Some(actions) = self.current_code_actions() {
                    if actions.len() > 0 {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ShowCodeActions,
                            Target::Auto,
                        ));
                    }
                }
            }
            LapceCommand::SearchWholeWordForward => {
                let offset = self.editor.cursor.offset();
                let (start, end) = self.buffer.select_word(offset);
                let word = self.buffer.slice_to_cow(start..end).to_string();
                Arc::make_mut(&mut self.find).set_find(&word, false, false, true);
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, end)) = next {
                    self.do_move(&Movement::Offset(start), 1);
                }
            }
            LapceCommand::SearchForward => {
                let offset = self.editor.cursor.offset();
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, end)) = next {
                    self.do_move(&Movement::Offset(start), 1);
                }
            }
            LapceCommand::SearchBackward => {
                let offset = self.editor.cursor.offset();
                let next = self.find.next(&self.buffer.rope, offset, true, true);
                if let Some((start, end)) = next {
                    self.do_move(&Movement::Offset(start), 1);
                }
            }
            LapceCommand::ClearSearch => {
                Arc::make_mut(&mut self.find).unset();
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
                let (line, col) = self.buffer.offset_to_line_col(offset);
                if line < self.buffer.last_line() {
                    let start = self.buffer.line_end_offset(line, true);
                    let end =
                        self.buffer.first_non_blank_character_on_line(line + 1);
                    self.edit(
                        ctx,
                        &Selection::region(start, end),
                        " ",
                        None,
                        false,
                        EditType::Other,
                    );
                }
            }
            LapceCommand::Save => {
                if !self.buffer.dirty {
                    return;
                }

                let proxy = self.proxy.clone();
                let buffer_id = self.buffer.id;
                let rev = self.buffer.rev;
                let path = self.buffer.path.clone();
                let event_sink = ctx.get_external_handle();
                let (sender, receiver) = bounded(1);
                thread::spawn(move || {
                    proxy.get_document_formatting(
                        buffer_id,
                        Box::new(move |result| {
                            sender.send(result);
                        }),
                    );

                    let result =
                        receiver.recv_timeout(Duration::from_secs(1)).map_or_else(
                            |e| Err(anyhow!("{}", e)),
                            |v| v.map_err(|e| anyhow!("{:?}", e)),
                        );
                    event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::DocumentFormatAndSave(path, rev, result),
                        Target::Auto,
                    );
                });
            }
            _ => (),
        }
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            let mut selection = self.editor.cursor.edit_selection(&self.buffer);
            let cursor_char =
                self.buffer.char_at_offset(selection.get_cursor_offset());

            let mut content = c.to_string();
            if c.chars().count() == 1 {
                let c = c.chars().next().unwrap();
                if !matching_pair_direction(c).unwrap_or(true) {
                    if cursor_char == Some(c) {
                        self.do_move(&Movement::Right, 1);
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

            let (selection, _) = self.edit(
                ctx,
                &selection,
                &content,
                None,
                true,
                EditType::InsertChars,
            );
            let editor = Arc::make_mut(&mut self.editor);
            editor.cursor.mode = CursorMode::Insert(selection.clone());
            editor.cursor.horiz = None;
            if c.chars().count() == 1 {
                let c = c.chars().next().unwrap();
                if matching_pair_direction(c).unwrap_or(false) {
                    if cursor_char
                        .map(|c| {
                            let prop = get_word_property(c);
                            prop == WordProperty::Lf
                                || prop == WordProperty::Space
                                || prop == WordProperty::Punctuation
                        })
                        .unwrap_or(true)
                    {
                        if let Some(c) = matching_char(c) {
                            self.edit(
                                ctx,
                                &selection,
                                &c.to_string(),
                                None,
                                false,
                                EditType::InsertChars,
                            );
                        }
                    }
                }
            }
            self.update_completion(ctx);
        } else {
            if let Some(direction) = self.editor.inline_find.clone() {
                self.inline_find(direction.clone(), c);
                let editor = Arc::make_mut(&mut self.editor);
                editor.last_inline_find = Some((direction.clone(), c.to_string()));
                editor.inline_find = None;
            }
        }
    }
}

pub struct LapceEditorView {
    pub view_id: WidgetId,
    pub header: WidgetPod<LapceTabData, LapceEditorHeader>,
    pub editor: WidgetPod<LapceTabData, LapceEditorContainer>,
}

impl LapceEditorView {
    pub fn new(data: &LapceEditorData) -> LapceEditorView {
        let header = LapceEditorHeader::new(data.view_id);
        let editor = LapceEditorContainer::new(data.view_id);
        Self {
            view_id: data.view_id,
            header: WidgetPod::new(header),
            editor: WidgetPod::new(editor),
        }
    }

    pub fn hide_header(mut self) -> Self {
        self.header.widget_mut().display = false;
        self
    }

    pub fn hide_gutter(mut self) -> Self {
        self.editor.widget_mut().display_gutter = false;
        self
    }

    pub fn set_placeholder(mut self, placehoder: String) -> Self {
        self.editor
            .widget_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .child_mut()
            .placeholder = Some(placehoder);
        self
    }

    pub fn handle_lapce_ui_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceUICommand,
        data: &mut LapceEditorBufferData,
        env: &Env,
    ) {
        match cmd {
            LapceUICommand::EnsureCursorVisible(position) => {
                self.ensure_cursor_visible(ctx, data, position.as_ref(), env);
            }
            LapceUICommand::EnsureCursorCenter => {
                self.ensure_cursor_center(ctx, data, env);
            }
            LapceUICommand::EnsureRectVisible(rect) => {
                self.ensure_rect_visible(ctx, data, *rect, env);
            }
            LapceUICommand::ResolveCompletion(buffer_id, rev, offset, item) => {
                if data.buffer.id != *buffer_id {
                    return;
                }
                if data.buffer.rev != *rev {
                    return;
                }
                if data.editor.cursor.offset() != *offset {
                    return;
                }
                let offset = data.editor.cursor.offset();
                let line = data.buffer.line_of_offset(offset);
                data.apply_completion_item(ctx, item);
                let new_offset = data.editor.cursor.offset();
                let new_line = data.buffer.line_of_offset(new_offset);
                if line != new_line {
                    self.editor
                        .widget_mut()
                        .editor
                        .widget_mut()
                        .inner_mut()
                        .scroll_by(Vec2::new(
                            0.0,
                            (new_line as f64 - line as f64)
                                * data.config.editor.line_height as f64,
                        ));
                }
            }
            LapceUICommand::Scroll((x, y)) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_by(Vec2::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            LapceUICommand::ForceScrollTo(x, y) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .force_scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            LapceUICommand::ScrollTo((x, y)) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            _ => (),
        }
    }

    fn ensure_rect_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        rect: Rect,
        env: &Env,
    ) {
        if self
            .editor
            .widget_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.editor.widget().scroll_id),
            ));
        }
    }

    pub fn ensure_cursor_center(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let offset = data.editor.cursor.offset();
        let (line, col) = data.buffer.offset_to_line_col(offset);
        let width = data.config.editor_text_width(ctx.text(), "W");
        let cursor_x = col as f64 * width - width;
        let cursor_x = if cursor_x < 0.0 { 0.0 } else { cursor_x };
        let rect = Rect::ZERO
            .with_origin(Point::new(
                cursor_x.floor(),
                line as f64 * line_height + line_height / 2.0,
            ))
            .with_size(Size::new((width * 3.0).ceil(), 0.0))
            .inflate(0.0, (data.editor.size.borrow().height / 2.0).ceil());

        let size = Size::new(
            (width * data.buffer.max_len as f64)
                .max(data.editor.size.borrow().width),
            line_height * data.buffer.num_lines as f64
                + data.editor.size.borrow().height
                - line_height,
        );
        let scroll = self.editor.widget_mut().editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.editor.widget().scroll_id),
            ));
        }
    }

    fn ensure_cursor_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        position: Option<&EnsureVisiblePosition>,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let width = data.config.editor_text_width(ctx.text(), "W");
        let size = Size::new(
            (width * data.buffer.max_len as f64)
                .max(data.editor.size.borrow().width),
            line_height * data.buffer.num_lines as f64
                + data.editor.size.borrow().height
                - line_height,
        );

        let rect = data.cursor_region(ctx.text(), &data.config);
        let scroll_id = self.editor.widget().scroll_id;
        let scroll = self.editor.widget_mut().editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        let old_scroll_offset = scroll.offset();
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(scroll_id),
            ));
            if let Some(position) = position {
                match position {
                    EnsureVisiblePosition::CenterOfWindow => {
                        self.ensure_cursor_center(ctx, data, env);
                    }
                }
            } else {
                let scroll_offset = scroll.offset();
                if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
                {
                    self.ensure_cursor_center(ctx, data, env);
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorView {
    fn id(&self) -> Option<WidgetId> {
        Some(self.view_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::WindowConnected => {
                if *data.main_split.active == self.view_id {
                    ctx.request_focus();
                    data.focus = self.view_id;
                    data.focus_area = FocusArea::Editor;
                }
            }
            Event::MouseDown(mouse_event) => match mouse_event.button {
                druid::MouseButton::Left => {
                    ctx.request_focus();
                    data.focus = self.view_id;
                    data.focus_area = FocusArea::Editor;
                    data.main_split.active = Arc::new(self.view_id);
                }
                druid::MouseButton::Right => {}
                _ => (),
            },
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        ctx.request_focus();
                        data.focus = self.view_id;
                        data.focus_area = FocusArea::Editor;
                        data.main_split.active = Arc::new(self.view_id);
                    }
                    _ => (),
                }
            }
            _ => (),
        }

        let editor = data.main_split.editors.get(&self.view_id).unwrap().clone();
        match &editor.content {
            EditorContent::Buffer(path) => {
                let buffer = data.main_split.open_files.get(path).unwrap().clone();
                let mut editor_data = LapceEditorBufferData {
                    view_id: self.view_id,
                    main_split: data.main_split.clone(),
                    completion: data.completion.clone(),
                    proxy: data.proxy.clone(),
                    find: data.find.clone(),
                    buffer: buffer.clone(),
                    editor: editor.clone(),
                    config: data.config.clone(),
                    workspace: data.workspace.clone(),
                };

                match event {
                    Event::KeyDown(key_event) => {
                        ctx.set_handled();
                        let mut keypress = data.keypress.clone();
                        if Arc::make_mut(&mut keypress).key_down(
                            ctx,
                            key_event,
                            &mut editor_data,
                            env,
                        ) {
                            self.ensure_cursor_visible(ctx, &editor_data, None, env);
                        }
                        editor_data.sync_buffer_position(
                            self.editor.widget().editor.widget().inner().offset(),
                        );
                        editor_data.get_code_actions(ctx);

                        data.keypress = keypress.clone();
                    }
                    Event::Command(cmd) if cmd.is(LAPCE_NEW_COMMAND) => {
                        let command = cmd.get_unchecked(LAPCE_NEW_COMMAND);
                        if let Ok(command) = LapceCommand::from_str(&command.cmd) {
                            editor_data.run_command(ctx, &command, None, env);
                        }
                    }
                    Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                        let cmd = cmd.get_unchecked(LAPCE_UI_COMMAND);
                        self.handle_lapce_ui_command(
                            ctx,
                            cmd,
                            &mut editor_data,
                            env,
                        );
                    }
                    _ => (),
                }
                data.update_from_editor_buffer_data(editor_data, &editor, &buffer);
            }
            EditorContent::None => match event {
                Event::KeyDown(key_event) => {
                    let mut keypress = data.keypress.clone();
                    Arc::make_mut(&mut keypress).key_down(
                        ctx,
                        key_event,
                        &mut LapceEditorEmptyContent {},
                        env,
                    );
                    data.keypress = keypress.clone();
                }
                _ => (),
            },
        };

        self.header.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);

        let offset = self.editor.widget().editor.widget().inner().offset();
        if editor.scroll_offset != offset {
            Arc::make_mut(data.main_split.editors.get_mut(&self.view_id).unwrap())
                .scroll_offset = offset;
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        match event {
            LifeCycle::HotChanged(is_hot) => {
                self.header.widget_mut().view_is_hot = *is_hot;
                ctx.request_paint();
            }
            _ => (),
        }
        self.header.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if old_data.config.lapce.modal != data.config.lapce.modal {
            if !data.config.lapce.modal {
                ctx.submit_command(Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::InsertMode.to_string(),
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(self.view_id),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::NormalMode.to_string(),
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(self.view_id),
                ));
            }
        }

        match (
            old_data.editor_view_content(self.view_id),
            data.editor_view_content(self.view_id),
        ) {
            (
                LapceEditorViewContent::Buffer(old_data),
                LapceEditorViewContent::Buffer(data),
            ) => {
                if data.buffer.path != old_data.buffer.path {
                    ctx.request_layout();
                }
                if data.buffer.dirty != old_data.buffer.dirty {
                    ctx.request_paint();
                }
                if data.editor.cursor != old_data.editor.cursor {
                    ctx.request_paint();
                }

                let buffer = &data.buffer;
                let old_buffer = &old_data.buffer;
                if buffer.max_len != old_buffer.max_len
                    || buffer.num_lines != old_buffer.num_lines
                {
                    ctx.request_layout();
                }

                if !buffer.styles.same(&old_buffer.styles) {
                    ctx.request_paint();
                }

                if buffer.rev != old_buffer.rev {
                    ctx.request_paint();
                }

                if old_data.current_code_actions().is_some()
                    != data.current_code_actions().is_some()
                {
                    ctx.request_paint();
                }
            }
            (LapceEditorViewContent::Buffer(_), LapceEditorViewContent::None) => {
                ctx.request_layout();
            }
            (LapceEditorViewContent::None, LapceEditorViewContent::Buffer(_)) => {
                ctx.request_layout();
            }
            (LapceEditorViewContent::None, LapceEditorViewContent::None) => {}
        }
        // self.header.update(ctx, data, env);
        // self.editor.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);
        if self_size.height > header_size.height {
            let editor_size =
                Size::new(self_size.width, self_size.height - header_size.height);
            let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
            self.editor.layout(ctx, &editor_bc, data, env);
            self.editor.set_origin(
                ctx,
                data,
                env,
                Point::new(0.0, header_size.height),
            );
        }
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in &rects {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        let start = std::time::SystemTime::now();
        self.editor.paint(ctx, data, env);
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("editor paint took {}", duration);
        self.header.paint(ctx, data, env);
    }
}

pub struct LapceEditorContainer {
    pub view_id: WidgetId,
    pub scroll_id: WidgetId,
    pub display_gutter: bool,
    pub gutter:
        WidgetPod<LapceTabData, LapcePadding<LapceTabData, LapceEditorGutter>>,
    pub editor: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScrollNew<LapceTabData, LapceEditor>>,
    >,
}

impl LapceEditorContainer {
    pub fn new(view_id: WidgetId) -> Self {
        let scroll_id = WidgetId::next();
        let gutter = LapceEditorGutter::new(view_id);
        let gutter = LapcePadding::new((10.0, 0.0, 0.0, 0.0), gutter);
        let editor = LapceEditor::new(view_id);
        let editor = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(editor).vertical().horizontal(),
            scroll_id,
        );
        Self {
            view_id,
            scroll_id,
            display_gutter: true,
            gutter: WidgetPod::new(gutter),
            editor: WidgetPod::new(editor),
        }
    }

    fn set_focus(&self, ctx: &mut EventCtx, data: &mut LapceEditorViewData) {
        if data.editor.editor_type != EditorType::SourceControl {
            data.main_split.active = Arc::new(self.view_id);
        }
        ctx.request_focus();
    }

    // pub fn handle_lapce_ui_command(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     cmd: &LapceUICommand,
    //     data: &mut LapceEditorViewData,
    //     env: &Env,
    // ) {
    //     match cmd {
    //         LapceUICommand::Focus => {
    //             self.set_focus(ctx, data);
    //             ctx.set_handled();
    //         }
    //         LapceUICommand::EnsureCursorVisible(position) => {
    //             self.ensure_cursor_visible(ctx, data, position.as_ref(), env);
    //         }
    //         LapceUICommand::EnsureCursorCenter => {
    //             self.ensure_cursor_center(ctx, data, env);
    //         }
    //         LapceUICommand::EnsureRectVisible(rect) => {
    //             self.ensure_rect_visible(ctx, data, *rect, env);
    //         }
    //         LapceUICommand::ResolveCompletion(buffer_id, rev, offset, item) => {
    //             if data.buffer.id != *buffer_id {
    //                 return;
    //             }
    //             if data.buffer.rev != *rev {
    //                 return;
    //             }
    //             if data.editor.cursor.offset() != *offset {
    //                 return;
    //             }
    //             data.apply_completion_item(ctx, item);
    //         }
    //         LapceUICommand::Scroll((x, y)) => {
    //             self.editor
    //                 .widget_mut()
    //                 .inner_mut()
    //                 .scroll_by(Vec2::new(*x, *y));
    //             ctx.submit_command(Command::new(
    //                 LAPCE_UI_COMMAND,
    //                 LapceUICommand::ResetFade,
    //                 Target::Widget(self.scroll_id),
    //             ));
    //         }
    //         LapceUICommand::ForceScrollTo(x, y) => {
    //             self.editor
    //                 .widget_mut()
    //                 .inner_mut()
    //                 .force_scroll_to(Point::new(*x, *y));
    //             ctx.submit_command(Command::new(
    //                 LAPCE_UI_COMMAND,
    //                 LapceUICommand::ResetFade,
    //                 Target::Widget(self.scroll_id),
    //             ));
    //         }
    //         LapceUICommand::ScrollTo((x, y)) => {
    //             self.editor
    //                 .widget_mut()
    //                 .inner_mut()
    //                 .scroll_to(Point::new(*x, *y));
    //             ctx.submit_command(Command::new(
    //                 LAPCE_UI_COMMAND,
    //                 LapceUICommand::ResetFade,
    //                 Target::Widget(self.scroll_id),
    //             ));
    //         }
    //         LapceUICommand::FocusTab => {
    //             if *data.main_split.active == self.view_id {
    //                 ctx.request_focus();
    //             }
    //         }
    //         _ => (),
    //     }
    // }

    pub fn ensure_cursor_center(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let offset = data.editor.cursor.offset();
        let (line, col) = data.buffer.offset_to_line_col(offset);
        let width = data.config.editor_text_width(ctx.text(), "W");
        let cursor_x = col as f64 * width - width;
        let cursor_x = if cursor_x < 0.0 { 0.0 } else { cursor_x };
        let rect = Rect::ZERO
            .with_origin(Point::new(
                cursor_x.floor(),
                line as f64 * line_height + line_height / 2.0,
            ))
            .with_size(Size::new((width * 3.0).ceil(), 0.0))
            .inflate(0.0, (data.editor.size.borrow().height / 2.0).ceil());

        let size = Size::new(
            (width * data.buffer.max_len as f64)
                .max(data.editor.size.borrow().width),
            line_height * data.buffer.num_lines as f64
                + data.editor.size.borrow().height
                - line_height,
        );
        let scroll = self.editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    pub fn ensure_rect_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorViewData,
        rect: Rect,
        env: &Env,
    ) {
        if self
            .editor
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    // pub fn ensure_cursor_visible(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     data: &LapceEditorViewData,
    //     position: Option<&EnsureVisiblePosition>,
    //     env: &Env,
    // ) {
    //     let line_height = data.config.editor.line_height as f64;
    //     let width = data.config.editor_text_width(ctx.text(), "W");
    //     let size = Size::new(
    //         (width * data.buffer.max_len as f64)
    //             .max(data.editor.size.borrow().width),
    //         line_height * data.buffer.text_layouts.borrow().len() as f64
    //             + data.editor.size.borrow().height
    //             - line_height,
    //     );

    //     let rect = data.cusor_region(&data.config);
    //     let scroll = self.editor.widget_mut().inner_mut();
    //     scroll.set_child_size(size);
    //     let old_scroll_offset = scroll.offset();
    //     if scroll.scroll_to_visible(rect, env) {
    //         ctx.submit_command(Command::new(
    //             LAPCE_UI_COMMAND,
    //             LapceUICommand::ResetFade,
    //             Target::Widget(self.scroll_id),
    //         ));
    //         if let Some(position) = position {
    //             match position {
    //                 EnsureVisiblePosition::CenterOfWindow => {
    //                     self.ensure_cursor_center(ctx, data, env);
    //                 }
    //             }
    //         } else {
    //             let scroll_offset = scroll.offset();
    //             if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
    //             {
    //                 self.ensure_cursor_center(ctx, data, env);
    //             }
    //         }
    //     }
    // }
}

impl Widget<LapceTabData> for LapceEditorContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.gutter.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);
        match event {
            Event::MouseDown(_) | Event::MouseUp(_) => {
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                match &editor.content {
                    EditorContent::Buffer(path) => {
                        let buffer =
                            data.main_split.open_files.get(path).unwrap().clone();
                        let mut editor_data = LapceEditorBufferData {
                            view_id: self.view_id,
                            main_split: data.main_split.clone(),
                            completion: data.completion.clone(),
                            proxy: data.proxy.clone(),
                            find: data.find.clone(),
                            buffer: buffer.clone(),
                            editor: editor.clone(),
                            config: data.config.clone(),
                            workspace: data.workspace.clone(),
                        };
                        editor_data.sync_buffer_position(
                            self.editor.widget().inner().offset(),
                        );
                        data.update_from_editor_buffer_data(
                            editor_data,
                            &editor,
                            &buffer,
                        );
                    }
                    EditorContent::None => {}
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        // if old_data.editor.scroll_offset != data.editor.scroll_offset {
        //     ctx.request_paint();
        // }

        self.gutter.update(ctx, data, env);
        let start = std::time::SystemTime::now();
        self.editor.update(ctx, data, env);
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("editor update took {}", duration);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        self.gutter.set_origin(ctx, data, env, Point::ZERO);
        let editor_size = Size::new(
            self_size.width
                - if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
            self_size.height,
        );
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_origin(
            ctx,
            data,
            env,
            Point::new(
                if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
                0.0,
            ),
        );
        *data
            .main_split
            .editors
            .get(&self.view_id)
            .unwrap()
            .size
            .borrow_mut() = editor_size.clone();
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in &rects {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        self.editor.paint(ctx, data, env);
        if self.display_gutter {
            self.gutter.paint(ctx, data, env);
        }
    }
}

pub struct LapceEditorHeaderIcon {
    rect: Rect,
    command: Command,
    icon: String,
}

pub struct LapceEditorHeader {
    view_id: WidgetId,
    pub display: bool,
    cross_rect: Rect,
    mouse_pos: Point,
    view_is_hot: bool,
    height: f64,
    icon_size: f64,
    icons: Vec<LapceEditorHeaderIcon>,
    svg_padding: f64,
}

impl LapceEditorHeader {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            display: true,
            view_id,
            cross_rect: Rect::ZERO,
            mouse_pos: Point::ZERO,
            view_is_hot: false,
            height: 30.0,
            icon_size: 25.0,
            svg_padding: 5.0,
            icons: Vec::new(),
        }
    }

    pub fn get_icons(
        &self,
        self_size: Size,
        data: &LapceTabData,
    ) -> Vec<LapceEditorHeaderIcon> {
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                let gap = (self.height - self.icon_size) / 2.0;

                let mut icons = Vec::new();
                let x = self_size.width
                    - ((icons.len() + 1) as f64) * (gap + self.icon_size);
                let icon = LapceEditorHeaderIcon {
                    icon: "close.svg".to_string(),
                    rect: Size::new(self.icon_size, self.icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceCommand::SplitClose.to_string(),
                            palette_desc: None,
                            target: CommandTarget::Focus,
                        },
                        Target::Widget(self.view_id),
                    ),
                };
                icons.push(icon);

                let x = self_size.width
                    - ((icons.len() + 1) as f64) * (gap + self.icon_size);
                let icon = LapceEditorHeaderIcon {
                    icon: "split-horizontal.svg".to_string(),
                    rect: Size::new(self.icon_size, self.icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceCommand::SplitVertical.to_string(),
                            palette_desc: None,
                            target: CommandTarget::Focus,
                        },
                        Target::Widget(self.view_id),
                    ),
                };
                icons.push(icon);

                icons
            }
            LapceEditorViewContent::None => vec![],
        }
    }

    pub fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    pub fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    pub fn paint_buffer(&self, ctx: &mut PaintCtx, data: &LapceEditorBufferData) {
        let shadow_width = 5.0;
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        let mut clip_rect = ctx.size().to_rect();
        if self.view_is_hot {
            if let Some(icon) = self.icons.iter().rev().next().as_ref() {
                clip_rect.x1 = icon.rect.x0;
            }
        }
        ctx.with_save(|ctx| {
            ctx.clip(clip_rect);
            let mut path = data.buffer.path.clone();
            let svg = file_svg_new(&path);

            let width = 13.0;
            let height = 13.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (30.0 - width) / 2.0,
                (30.0 - height) / 2.0,
            ));
            ctx.draw_svg(&svg, rect, None);

            let mut file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if data.buffer.dirty {
                file_name = "*".to_string() + &file_name;
            }
            let text_layout = ctx
                .text()
                .new_text_layout(file_name)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, Point::new(30.0, 7.0));

            if let Some(workspace) = data.workspace.as_ref() {
                path = path
                    .strip_prefix(&workspace.path)
                    .unwrap_or(&path)
                    .to_path_buf();
            }
            let folder = path
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if folder != "" {
                let x = text_layout.size().width;

                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(30.0 + x + 5.0, 7.0));
            }
        });

        if self.view_is_hot {
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        &icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                if let Some(svg) = get_svg(&icon.icon) {
                    ctx.draw_svg(
                        &svg,
                        icon.rect.inflate(-self.svg_padding, -self.svg_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        // ctx.set_paint_insets((0.0, 0.0, 0.0, 10.0));
        if self.display {
            let size = Size::new(bc.max().width, self.height);
            self.icons = self.get_icons(size, data);
            let cross_size = 20.0;
            let padding = (size.height - cross_size) / 2.0;
            let origin = Point::new(size.width - padding - cross_size, padding);
            self.cross_rect = Size::new(cross_size, cross_size)
                .to_rect()
                .with_origin(origin);
            size
        } else {
            Size::new(bc.max().width, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !self.display {
            return;
        }
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                self.paint_buffer(ctx, &data);
            }
            LapceEditorViewContent::None => {}
        }
    }
}

pub struct LapceEditorGutter {
    view_id: WidgetId,
    width: f64,
}

impl LapceEditorGutter {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            width: 0.0,
        }
    }
}

impl Widget<LapceTabData> for LapceEditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        // let old_last_line = old_data.buffer.last_line() + 1;
        // let last_line = data.buffer.last_line() + 1;
        // if old_last_line.to_string().len() != last_line.to_string().len() {
        //     ctx.request_layout();
        //     return;
        // }

        // if (*old_data.main_split.active == self.view_id
        //     && *data.main_split.active != self.view_id)
        //     || (*old_data.main_split.active != self.view_id
        //         && *data.main_split.active == self.view_id)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.editor.cursor.current_line(&old_data.buffer)
        //     != data.editor.cursor.current_line(&data.buffer)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.current_code_actions().is_some()
        //     != data.current_code_actions().is_some()
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                let last_line = data.buffer.last_line() + 1;
                let width = data.config.editor_text_width(ctx.text(), "W");
                self.width = (width * last_line.to_string().len() as f64).ceil();
                let width = self.width + 16.0 + width * 2.0;
                Size::new(width, bc.max().height)
            }
            LapceEditorViewContent::None => Size::new(0.0, bc.max().height),
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                data.paint_gutter(ctx, self.width);
            }
            LapceEditorViewContent::None => {}
        }
    }
}

pub struct LapceEditor {
    view_id: WidgetId,
    placeholder: Option<String>,
    commands: Vec<(LapceCommandNew, PietTextLayout, Rect, PietTextLayout)>,
}

impl LapceEditor {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            placeholder: None,
            commands: vec![],
        }
    }
}

impl Widget<LapceTabData> for LapceEditor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        let editor = data.main_split.editors.get_mut(&self.view_id).unwrap();
        match event {
            Event::MouseMove(mouse_event) => match &editor.content {
                EditorContent::None => {
                    let mut on_command = false;
                    for (_, _, rect, _) in &self.commands {
                        if rect.contains(mouse_event.pos) {
                            on_command = true;
                            break;
                        }
                    }
                    if on_command {
                        ctx.set_cursor(&druid::Cursor::Pointer);
                    } else {
                        ctx.set_cursor(&druid::Cursor::Arrow);
                    }
                }
                EditorContent::Buffer(path) => {
                    ctx.set_cursor(&druid::Cursor::IBeam);
                    if ctx.is_active() {
                        let buffer =
                            data.main_split.open_files.get(path).unwrap().clone();
                        let new_offset = buffer.offset_of_mouse(
                            ctx.text(),
                            mouse_event.pos,
                            editor.cursor.get_mode(),
                            &data.config,
                        );
                        let editor = Arc::make_mut(editor);
                        match editor.cursor.mode.clone() {
                            CursorMode::Normal(offset) => {
                                if new_offset != offset {
                                    editor.cursor = Cursor::new(
                                        CursorMode::Visual {
                                            start: offset,
                                            end: new_offset,
                                            mode: VisualMode::Normal,
                                        },
                                        None,
                                    );
                                }
                            }
                            CursorMode::Visual { start, end, mode } => {
                                let mode = mode.clone();
                                editor.cursor.mode = CursorMode::Visual {
                                    start,
                                    end: new_offset,
                                    mode,
                                };
                                editor.cursor.horiz = None;
                            }
                            CursorMode::Insert(selection) => {
                                let mut new_selection = Selection::new();
                                if let Some(region) = selection.first() {
                                    let new_regoin = SelRegion::new(
                                        region.start(),
                                        new_offset,
                                        None,
                                    );
                                    new_selection.add_region(new_regoin);
                                } else {
                                    new_selection.add_region(SelRegion::new(
                                        new_offset, new_offset, None,
                                    ));
                                }
                                editor.cursor = Cursor::new(
                                    CursorMode::Insert(new_selection),
                                    None,
                                );
                            }
                        }
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::EnsureCursorVisible(None),
                            Target::Widget(self.view_id),
                        ));
                    }
                }
            },
            Event::MouseUp(mouse_event) => {
                ctx.set_active(false);
            }
            Event::MouseDown(mouse_event) => match &editor.content {
                EditorContent::None => {
                    ctx.set_handled();
                    for (cmd, _, rect, _) in &self.commands {
                        if rect.contains(mouse_event.pos) {
                            ctx.submit_command(Command::new(
                                LAPCE_NEW_COMMAND,
                                cmd.clone(),
                                Target::Auto,
                            ));
                            return;
                        }
                    }
                }
                EditorContent::Buffer(path) => {
                    ctx.set_handled();
                    ctx.set_active(true);
                    let buffer =
                        data.main_split.open_files.get(path).unwrap().clone();
                    let line_height = data.config.editor.line_height as f64;
                    let line = (mouse_event.pos.y / line_height).floor() as usize;
                    let last_line = buffer.last_line();
                    let (line, col) = if line > last_line {
                        (last_line, 0)
                    } else {
                        let line_end =
                            buffer.line_end_col(line, !editor.cursor.is_normal());
                        let width = data.config.editor_text_width(ctx.text(), "W");

                        let col = (if editor.cursor.is_insert() {
                            (mouse_event.pos.x / width).round() as usize
                        } else {
                            (mouse_event.pos.x / width).floor() as usize
                        })
                        .min(line_end);
                        (line, col)
                    };
                    let new_offset = buffer.offset_of_line_col(line, col);
                    let editor = Arc::make_mut(editor);
                    match editor.cursor.mode.clone() {
                        CursorMode::Normal(offset) => {
                            if mouse_event.mods.shift() {
                                editor.cursor = Cursor::new(
                                    CursorMode::Visual {
                                        start: offset,
                                        end: new_offset,
                                        mode: VisualMode::Normal,
                                    },
                                    None,
                                );
                            } else {
                                editor.cursor.mode = CursorMode::Normal(new_offset);
                                editor.cursor.horiz = None;
                            }
                        }
                        CursorMode::Visual { start, end, mode } => {
                            if mouse_event.mods.shift() {
                                editor.cursor = Cursor::new(
                                    CursorMode::Visual {
                                        start,
                                        end: new_offset,
                                        mode: VisualMode::Normal,
                                    },
                                    None,
                                );
                            } else {
                                editor.cursor = Cursor::new(
                                    CursorMode::Normal(new_offset),
                                    None,
                                );
                            }
                        }
                        CursorMode::Insert(selection) => {
                            if mouse_event.mods.shift() {
                                let mut new_selection = Selection::new();
                                if let Some(region) = selection.first() {
                                    let new_regoin = SelRegion::new(
                                        region.start(),
                                        new_offset,
                                        None,
                                    );
                                    new_selection.add_region(new_regoin);
                                } else {
                                    new_selection.add_region(SelRegion::new(
                                        new_offset, new_offset, None,
                                    ));
                                }
                                editor.cursor = Cursor::new(
                                    CursorMode::Insert(new_selection),
                                    None,
                                );
                            } else {
                                editor.cursor = Cursor::new(
                                    CursorMode::Insert(Selection::caret(new_offset)),
                                    None,
                                );
                            }
                        }
                    }
                }
            },
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdateWindowOrigin => {
                        let window_origin = ctx.window_origin();
                        let editor =
                            data.main_split.editors.get_mut(&self.view_id).unwrap();
                        if editor.window_origin != window_origin {
                            Arc::make_mut(editor).window_origin = window_origin;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        // let buffer = &data.buffer;
        // let old_buffer = &old_data.buffer;

        // let line_height = data.config.editor.line_height as f64;

        // if data.editor.size != old_data.editor.size {
        //     ctx.request_paint();
        //     return;
        // }

        // if !old_buffer.same(buffer) {
        //     if buffer.max_len != old_buffer.max_len
        //         || buffer.num_lines != old_buffer.num_lines
        //     {
        //         ctx.request_layout();
        //         ctx.request_paint();
        //         return;
        //     }

        //     if !buffer.styles.same(&old_buffer.styles) {
        //         ctx.request_paint();
        //     }

        //     if buffer.rev != old_buffer.rev {
        //         ctx.request_paint();
        //     }
        // }

        // if old_data.editor.cursor != data.editor.cursor {
        //     ctx.request_paint();
        // }

        // if old_data.current_code_actions().is_some()
        //     != data.current_code_actions().is_some()
        // {
        //     ctx.request_paint();
        // }

        // if old_data.on_diagnostic() != data.on_diagnostic() {
        //     ctx.request_paint();
        // }

        // if old_data.diagnostics.len() != data.diagnostics.len() {
        //     ctx.request_paint();
        // }

        // if (*old_data.main_split.active == self.view_id
        //     && *data.main_split.active != self.view_id)
        //     || (*old_data.main_split.active != self.view_id
        //         && *data.main_split.active == self.view_id)
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                let line_height = data.config.editor.line_height as f64;
                let width = data.config.editor_text_width(ctx.text(), "W");
                Size::new(
                    (width * data.buffer.max_len as f64).max(bc.max().width),
                    line_height * data.buffer.num_lines as f64 + bc.max().height
                        - line_height,
                )
            }
            LapceEditorViewContent::None => {
                let size = bc.max();
                let origin = Point::new(size.width / 2.0, size.height / 2.0 + 40.0);
                let line_height = 30.0;

                self.commands = empty_editor_commands(
                    data.config.lapce.modal,
                    data.workspace.is_some(),
                )
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    let text_layout = ctx
                        .text()
                        .new_text_layout(
                            cmd.palette_desc.as_ref().unwrap().to_string(),
                        )
                        .font(FontFamily::SYSTEM_UI, 14.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    let point =
                        origin - (text_layout.size().width, -line_height * i as f64);
                    let rect = text_layout.size().to_rect().with_origin(point);
                    let mut key = None;
                    for (_, keymaps) in data.keypress.keymaps.iter() {
                        for keymap in keymaps {
                            if keymap.command == cmd.cmd {
                                let mut keymap_str = "".to_string();
                                for keypress in &keymap.key {
                                    if keymap_str != "" {
                                        keymap_str += " "
                                    }
                                    keymap_str += &keybinding_to_string(keypress);
                                }
                                key = Some(keymap_str);
                                break;
                            }
                        }
                        if key.is_some() {
                            break;
                        }
                    }
                    let key_text_layout = ctx
                        .text()
                        .new_text_layout(key.unwrap_or("Unbound".to_string()))
                        .font(FontFamily::SYSTEM_UI, 14.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    (cmd.clone(), text_layout, rect, key_text_layout)
                })
                .collect();

                size
            }
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let is_focused = data.focus == self.view_id;
        match data.editor_view_content(self.view_id) {
            LapceEditorViewContent::Buffer(data) => {
                data.paint_content(
                    ctx,
                    is_focused,
                    self.placeholder.as_ref(),
                    &data.config,
                );
            }
            LapceEditorViewContent::None => {
                let svg = logo_svg();
                let size = ctx.size();
                let svg_size = 100.0;
                let rect = Size::ZERO
                    .to_rect()
                    .with_origin(
                        Point::new(size.width / 2.0, size.height / 2.0)
                            + (0.0, -svg_size),
                    )
                    .inflate(svg_size, svg_size);
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(
                        &data
                            .config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone()
                            .with_alpha(0.5),
                    ),
                );
                for (cmd, text, rect, keymap) in &self.commands {
                    ctx.draw_text(text, rect.origin());
                    ctx.draw_text(
                        keymap,
                        rect.origin() + (20.0 + rect.width(), 0.0),
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RegisterContent {
    kind: VisualMode,
    content: Vec<String>,
}

struct EditorTextLayout {
    layout: TextLayout<String>,
    text: String,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

fn get_workspace_edit_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    if let Some(edits) = get_workspace_edit_changes_edits(&url, workspace_edit) {
        Some(edits)
    } else {
        get_workspace_edit_document_changes_edits(&url, workspace_edit)
    }
}

fn get_workspace_edit_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.changes.as_ref()?;
    changes.get(url).map(|c| c.iter().map(|t| t).collect())
}

fn get_workspace_edit_document_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.document_changes.as_ref()?;
    match changes {
        DocumentChanges::Edits(edits) => {
            for edit in edits {
                if &edit.text_document.uri == url {
                    let e = edit
                        .edits
                        .iter()
                        .filter_map(|e| match e {
                            lsp_types::OneOf::Left(edit) => Some(edit),
                            lsp_types::OneOf::Right(_) => None,
                        })
                        .collect();
                    return Some(e);
                }
            }
            None
        }
        DocumentChanges::Operations(_) => None,
    }
}

fn next_in_file_errors_offset(
    position: Position,
    path: &PathBuf,
    file_diagnostics: &Vec<(&PathBuf, Vec<Position>)>,
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
    result: Result<Value, xi_rpc::Error>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let res = result.map_err(|e| anyhow!("{:?}", e))?;
    let locations: Vec<Location> = serde_json::from_value(res)?;
    if locations.len() == 0 {
        return Ok(());
    }
    if locations.len() == 1 {
        let location = &locations[0];
        event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::GotoReference(
                editor_view_id,
                offset,
                EditorLocationNew {
                    path: PathBuf::from(location.uri.path()),
                    position: Some(location.range.start.clone()),
                    scroll_offset: None,
                },
            ),
            Target::Auto,
        );
    }
    event_sink.submit_command(
        LAPCE_UI_COMMAND,
        LapceUICommand::PaletteReferences(offset, locations),
        Target::Auto,
    );
    Ok(())
}

fn paint_wave_line(
    ctx: &mut PaintCtx,
    origin: Point,
    max_width: f64,
    color: &Color,
) {
    let mut path = BezPath::new();
    let mut x = 0.0;
    let width = 3.5;
    let height = 4.0;
    path.move_to(origin + (0.0, height / 2.0));
    let mut direction = 1.0;
    while x < max_width {
        let point = origin + (x, height / 2.0);
        let p1 = point + (width / 2.0, -height / 2.0 * direction);
        let p2 = point + (width, 0.0);
        path.quad_to(p1, p2);
        x += width;
        direction *= -1.0;
    }
    ctx.stroke(path, color, 1.4);
}

fn empty_editor_commands(modal: bool, has_workspace: bool) -> Vec<LapceCommandNew> {
    if !has_workspace {
        vec![
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteCommand.to_string(),
                palette_desc: Some("Show All Commands".to_string()),
                target: CommandTarget::Workbench,
            },
            if modal {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::DisableModal.to_string(),
                    palette_desc: LapceWorkbenchCommand::DisableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            } else {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::EnableModal.to_string(),
                    palette_desc: LapceWorkbenchCommand::EnableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::OpenFolder.to_string(),
                palette_desc: Some("Open Folder".to_string()),
                target: CommandTarget::Workbench,
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteWorkspace.to_string(),
                palette_desc: Some("Open Recent".to_string()),
                target: CommandTarget::Workbench,
            },
        ]
    } else {
        vec![
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteCommand.to_string(),
                palette_desc: Some("Show All Commands".to_string()),
                target: CommandTarget::Workbench,
            },
            if modal {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::DisableModal.to_string(),
                    palette_desc: LapceWorkbenchCommand::DisableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            } else {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::EnableModal.to_string(),
                    palette_desc: LapceWorkbenchCommand::EnableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::Palette.to_string(),
                palette_desc: Some("Go To File".to_string()),
                target: CommandTarget::Workbench,
            },
        ]
    }
}

fn keybinding_to_string(keypress: &KeyPress) -> String {
    let mut keymap_str = "".to_string();
    if keypress.mods.ctrl() {
        keymap_str += "Ctrl+";
    }
    if keypress.mods.alt() {
        keymap_str += "Alt+";
    }
    if keypress.mods.meta() {
        let keyname = match std::env::consts::OS {
            "macos" => "Cmd",
            "windows" => "Win",
            _ => "Meta",
        };
        keymap_str += &keyname;
        keymap_str += "+";
    }
    if keypress.mods.shift() {
        keymap_str += "Shift+";
    }
    keymap_str += &keypress.key.to_string();
    keymap_str
}
