use crate::buffer::get_word_property;
use crate::buffer::matching_char;
use crate::buffer::{
    has_unmatched_pair, BufferContent, DiffLines, EditType, LocalBufferKind,
};
use crate::buffer::{matching_pair_direction, Buffer};
use crate::command::CommandExecuted;
use crate::completion::{CompletionData, CompletionStatus, Snippet};
use crate::config::{Config, LapceTheme};
use crate::data::EditorTabChild;
use crate::data::{
    EditorDiagnostic, InlineFindDirection, LapceEditorData, LapceMainSplitData,
    LapceTabData, PanelData, PanelKind, RegisterData, SplitContent,
};
use crate::state::LapceWorkspace;
use crate::svg::get_svg;
use crate::{
    buffer::BufferId,
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    movement::{ColPosition, Movement, SelRegion, Selection},
    split::SplitMoveDirection,
    state::Mode,
    state::VisualMode,
};
use crate::{buffer::WordProperty, movement::CursorMode};
use crate::{find::Find, split::SplitDirection};
use crate::{keypress::KeyPressFocus, movement::Cursor};
use crate::{movement::InsertDrift, panel::PanelPosition};
use crate::{proxy::LapceProxy, source_control::SourceControlData};
use anyhow::{anyhow, Result};
use crossbeam_channel::{self, bounded};
use druid::kurbo::BezPath;
use druid::piet::Svg;
use druid::piet::{
    PietTextLayout, Text, TextLayout as TextLayoutTrait, TextLayoutBuilder,
};
use druid::Modifiers;
use druid::{
    kurbo::Line, piet::PietText, Color, Command, Env, EventCtx, FontFamily,
    PaintCtx, Point, Rect, RenderContext, Size, Target, Vec2, WidgetId,
};
use druid::{Application, ExtEventSink, MouseEvent};
use lapce_core::syntax::Syntax;
use lsp_types::CompletionTextEdit;
use lsp_types::{
    CodeActionResponse, CompletionItem, DiagnosticSeverity, DocumentChanges,
    GotoDefinitionResponse, Location, Position, TextEdit, Url, WorkspaceEdit,
};
use serde_json::Value;
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
    pub workspace: Arc<LapceWorkspace>,
    pub main_split: LapceMainSplitData,
    pub source_control: Arc<SourceControlData>,
    pub find: Arc<Find>,
    pub proxy: Arc<LapceProxy>,
    pub config: Arc<Config>,
}

impl LapceEditorBufferData {
    fn buffer_mut(&mut self) -> &mut Buffer {
        Arc::make_mut(&mut self.buffer)
    }

    pub fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
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
            self.do_move(
                &Movement::Offset(new_index + line_start_offset),
                1,
                Modifiers::empty(),
            );
        }
    }

    pub fn get_size(
        &self,
        text: &mut PietText,
        editor_size: Size,
        panels: im::HashMap<PanelPosition, Arc<PanelData>>,
    ) -> Size {
        let line_height = self.config.editor.line_height as f64;
        let width = self.config.editor_text_width(text, "W");
        match &self.editor.content {
            BufferContent::File(_) => {
                if self.editor.code_lens {
                    if let Some(syntax) = self.buffer.syntax.as_ref() {
                        let height =
                            syntax.lens.height_of_line(syntax.lens.len() + 1);
                        Size::new(
                            (width * self.buffer.max_len as f64)
                                .max(editor_size.width),
                            (height as f64 - line_height).max(0.0)
                                + editor_size.height,
                        )
                    } else {
                        let height = self.buffer.num_lines
                            * self.config.editor.code_lens_font_size;
                        Size::new(
                            (width * self.buffer.max_len as f64)
                                .max(editor_size.width),
                            (height as f64 - line_height).max(0.0)
                                + editor_size.height,
                        )
                    }
                } else if let Some(compare) = self.editor.compare.as_ref() {
                    let mut lines = 0;
                    if let Some(changes) = self.buffer.history_changes.get(compare) {
                        for change in changes.iter() {
                            match change {
                                DiffLines::Left(l) => lines += l.len(),
                                DiffLines::Both(_l, r) => lines += r.len(),
                                DiffLines::Skip(_l, _r) => lines += 1,
                                DiffLines::Right(r) => lines += r.len(),
                            }
                        }
                    }
                    Size::new(
                        (width * self.buffer.max_len as f64).max(editor_size.width),
                        (line_height * lines as f64 - line_height).max(0.0)
                            + editor_size.height,
                    )
                } else {
                    Size::new(
                        (width * self.buffer.max_len as f64).max(editor_size.width),
                        (line_height * self.buffer.num_lines as f64 - line_height)
                            .max(0.0)
                            + editor_size.height,
                    )
                }
            }
            BufferContent::Local(kind) => match kind {
                LocalBufferKind::FilePicker
                | LocalBufferKind::Search
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => {
                    Size::new(editor_size.width, line_height)
                }
                LocalBufferKind::SourceControl => {
                    for (pos, panels) in panels.iter() {
                        for panel_kind in panels.widgets.iter() {
                            if panel_kind == &PanelKind::SourceControl {
                                return match pos {
                                    PanelPosition::BottomLeft
                                    | PanelPosition::BottomRight => {
                                        let width = 200.0;
                                        Size::new(width, editor_size.height)
                                    }
                                    _ => {
                                        let height = 100.0f64;
                                        let height = height.max(
                                            line_height
                                                * self.buffer.num_lines() as f64,
                                        );
                                        Size::new(
                                            (width * self.buffer.max_len as f64)
                                                .max(editor_size.width),
                                            height,
                                        )
                                    }
                                };
                            }
                        }
                    }
                    Size::ZERO
                }
                LocalBufferKind::Empty => editor_size,
            },
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

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id
                && self.buffer.content == editor.content
            {
                Arc::make_mut(editor).cursor.apply_delta(delta);
            }
        }
    }

    pub fn apply_completion_item(
        &mut self,
        ctx: &mut EventCtx,
        item: &CompletionItem,
    ) -> Result<()> {
        let additioal_edit = item.additional_text_edits.as_ref().map(|edits| {
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
        let additioal_edit = additioal_edit.as_ref().map(|edits| {
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

    pub fn cursor_region(&self, text: &mut PietText, config: &Config) -> Rect {
        let offset = self.editor.cursor.offset();
        let (line, col) = self
            .buffer
            .offset_to_line_col(offset, self.config.editor.tab_width);
        let width = config.editor_text_width(text, "W");
        let cursor_x = col as f64 * width;
        let line_height = config.editor.line_height as f64;

        let y = if self.editor.code_lens {
            let empty_vec = Vec::new();
            let normal_lines = self
                .buffer
                .syntax
                .as_ref()
                .map(|s| &s.normal_lines)
                .unwrap_or(&empty_vec);

            let mut y = 0.0;
            let mut current_line = 0;
            let mut normal_lines = normal_lines.iter();
            loop {
                match normal_lines.next() {
                    Some(next_normal_line) => {
                        let next_normal_line = *next_normal_line;
                        if next_normal_line < line {
                            let chunk_height = config.editor.code_lens_font_size
                                as f64
                                * (next_normal_line - current_line) as f64
                                + line_height;
                            y += chunk_height;
                            current_line = next_normal_line + 1;
                            continue;
                        };
                        y += (line - current_line) as f64
                            * config.editor.code_lens_font_size as f64;
                        break;
                    }
                    None => {
                        y += (line - current_line) as f64
                            * config.editor.code_lens_font_size as f64;
                        break;
                    }
                }
            }
            y
        } else {
            let line = if let Some(compare) = self.editor.compare.as_ref() {
                self.buffer.diff_visual_line(compare, line)
            } else {
                line
            };
            line as f64 * line_height
        };

        Rect::ZERO
            .with_size(Size::new(width, line_height))
            .with_origin(Point::new(cursor_x, y))
            .inflate(width, line_height)
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

    fn insert_new_line(&mut self, ctx: &mut EventCtx, offset: usize) {
        match &self.buffer.content {
            BufferContent::File(_) => {}
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
                    self.update_global_search(ctx, pattern);
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
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                };
                let after =
                    self.editor.cursor.is_insert() || !data.content.contains('\n');
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
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &content,
                    None,
                    self.editor.cursor.is_insert(),
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
                    let data = self
                        .editor
                        .cursor
                        .yank(&self.buffer, self.config.editor.tab_width);
                    let register = Arc::make_mut(&mut self.main_split.register);
                    register.add_delete(data);
                }
            }
            #[allow(unused_variables)]
            CursorMode::Visual { start, end, mode } => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
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
            buffer.edit(ctx, selection, c, proxy, edit_type)
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

        if new_line > line {
            self.do_move(&Movement::Down, new_line - line, mods);
        } else if new_line < line {
            self.do_move(&Movement::Up, line - new_line, mods);
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

    pub fn current_code_actions(&self) -> Option<&CodeActionResponse> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        self.buffer.code_actions.get(&prev_offset)
    }

    fn diagnostics(&self) -> Option<&Arc<Vec<EditorDiagnostic>>> {
        if let BufferContent::File(path) = &self.buffer.content {
            self.main_split.diagnostics.get(path)
        } else {
            None
        }
    }

    fn diagnostics_mut(&mut self) -> Option<&mut Vec<EditorDiagnostic>> {
        if let BufferContent::File(path) = &self.buffer.content {
            self.main_split.diagnostics.get_mut(path).map(Arc::make_mut)
        } else {
            None
        }
    }

    fn paint_gutter_inline_diff(
        &self,
        ctx: &mut PaintCtx,
        compare: &str,
        gutter_width: f64,
    ) {
        if self.buffer.history_changes.get(compare).is_none() {
            return;
        }
        let self_size = ctx.size();
        let rect = self_size.to_rect();
        let changes = self.buffer.history_changes.get(compare).unwrap();
        let line_height = self.config.editor.line_height as f64;
        let scroll_offset = self.editor.scroll_offset;
        let start_line = (scroll_offset.y / line_height).floor() as usize;
        let end_line =
            (scroll_offset.y + rect.height() / line_height).ceil() as usize;
        let current_line = self.editor.cursor.current_line(&self.buffer);
        let last_line = self.buffer.last_line();
        let width = self.config.editor_text_width(ctx.text(), "W");

        let mut line = 0;
        for change in changes.iter() {
            match change {
                DiffLines::Left(r) => {
                    let len = r.len();
                    line += len;

                    if line < start_line {
                        continue;
                    }
                    ctx.fill(
                        Size::new(self_size.width, line_height * len as f64)
                            .to_rect()
                            .with_origin(Point::new(
                                0.0,
                                line_height * (line - len) as f64 - scroll_offset.y,
                            )),
                        self.config
                            .get_color_unchecked(LapceTheme::SOURCE_CONTROL_REMOVED),
                    );
                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let actual_line = l - (line - len) + r.start;

                        let content = actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(
                                content.to_string()
                                    + &vec![
                                        " ";
                                        (last_line + 1).to_string().len() + 2
                                    ]
                                    .join("")
                                    + " -",
                            )
                            .font(
                                self.config.editor.font_family(),
                                self.config.editor.font_size as f64,
                            )
                            .text_color(
                                self.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
                DiffLines::Both(left, r) => {
                    let len = r.len();
                    line += len;
                    if line < start_line {
                        continue;
                    }

                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let left_actual_line = l - (line - len) + left.start;
                        let right_actual_line = l - (line - len) + r.start;

                        let left_content = left_actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - left_content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(left_content.to_string())
                            .font(
                                self.config.editor.font_family(),
                                self.config.editor.font_size as f64,
                            )
                            .text_color(
                                self.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        let right_content = right_actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - right_content.to_string().len())
                            as f64
                            * width
                            + gutter_width
                            + 2.0 * width;
                        let pos = Point::new(x, y);
                        let text_layout = ctx
                            .text()
                            .new_text_layout(right_content.to_string())
                            .font(
                                self.config.editor.font_family(),
                                self.config.editor.font_size as f64,
                            )
                            .text_color(if right_actual_line == current_line {
                                self.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone()
                            } else {
                                self.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone()
                            })
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
                DiffLines::Skip(_l, _r) => {
                    let rect = Size::new(self_size.width, line_height)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            line_height * line as f64 - scroll_offset.y,
                        ));
                    ctx.fill(
                        rect,
                        self.config
                            .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                    );
                    ctx.stroke(
                        rect,
                        self.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        1.0,
                    );
                    line += 1;
                }
                DiffLines::Right(r) => {
                    let len = r.len();
                    line += len;
                    if line < start_line {
                        continue;
                    }

                    ctx.fill(
                        Size::new(self_size.width, line_height * len as f64)
                            .to_rect()
                            .with_origin(Point::new(
                                0.0,
                                line_height * (line - len) as f64 - scroll_offset.y,
                            )),
                        self.config
                            .get_color_unchecked(LapceTheme::SOURCE_CONTROL_ADDED),
                    );

                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let actual_line = l - (line - len) + r.start;

                        let content = actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width
                            + gutter_width
                            + 2.0 * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(content.to_string() + " +")
                            .font(
                                self.config.editor.font_family(),
                                self.config.editor.font_size as f64,
                            )
                            .text_color(if actual_line == current_line {
                                self.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone()
                            } else {
                                self.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone()
                            })
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
            }
        }
    }

    fn paint_gutter_code_lens(&self, ctx: &mut PaintCtx, _gutter_width: f64) {
        let rect = ctx.size().to_rect();
        let scroll_offset = self.editor.scroll_offset;
        let empty_lens = Syntax::lens_from_normal_lines(
            self.buffer.len(),
            self.config.editor.line_height,
            self.config.editor.code_lens_font_size,
            &[],
        );
        let (rope, lens) = if let Some(syntax) = self.buffer.syntax.as_ref() {
            (&syntax.text, &syntax.lens)
        } else {
            (&self.buffer.rope, &empty_lens)
        };

        let cursor_line =
            rope.line_of_offset(self.editor.cursor.offset().min(rope.len()));
        let last_line = rope.line_of_offset(rope.len());
        let start_line = lens
            .line_of_height(scroll_offset.y.floor() as usize)
            .min(last_line);
        let end_line = lens
            .line_of_height(
                (scroll_offset.y + rect.height()).ceil() as usize
                    + self.config.editor.line_height,
            )
            .min(last_line);
        let char_width = self
            .config
            .char_width(ctx.text(), self.config.editor.font_size as f64);
        let max_line_width = (last_line + 1).to_string().len() as f64 * char_width;

        let mut y = lens.height_of_line(start_line) as f64;
        for (line, line_height) in lens.iter_chunks(start_line..end_line) {
            let content = if *self.main_split.active != Some(self.view_id)
                || self.editor.cursor.is_insert()
                || line == cursor_line
            {
                line + 1
            } else if line > cursor_line {
                line - cursor_line
            } else {
                cursor_line - line
            };
            let content = content.to_string();
            let is_small = line_height < self.config.editor.line_height;
            let text_layout = ctx
                .text()
                .new_text_layout(content.clone())
                .font(
                    self.config.editor.font_family(),
                    if is_small {
                        self.config.editor.code_lens_font_size as f64
                    } else {
                        self.config.editor.font_size as f64
                    },
                )
                .text_color(if line == cursor_line {
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
            let x = max_line_width - text_layout.size().width;
            let pos = Point::new(
                x,
                y - scroll_offset.y
                    + if is_small {
                        0.0
                    } else {
                        (line_height as f64 - text_layout.size().height) / 2.0
                    },
            );
            ctx.draw_text(&text_layout, pos);

            y += line_height as f64;
        }
    }

    pub fn paint_gutter(&self, ctx: &mut PaintCtx, gutter_width: f64) {
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            let clip_rect = rect;
            ctx.clip(clip_rect);
            if let Some(compare) = self.editor.compare.as_ref() {
                self.paint_gutter_inline_diff(ctx, compare, gutter_width);
                return;
            }
            if self.editor.code_lens {
                self.paint_gutter_code_lens(ctx, gutter_width);
                return;
            }
            let line_height = self.config.editor.line_height as f64;
            let scroll_offset = self.editor.scroll_offset;
            let start_line = (scroll_offset.y / line_height).floor() as usize;
            let end_line =
                (scroll_offset.y + rect.height() / line_height).ceil() as usize;
            let num_lines = (ctx.size().height / line_height).floor() as usize;
            let last_line = self.buffer.last_line();
            let current_line = self.editor.cursor.current_line(&self.buffer);
            let width = self.config.editor_text_width(ctx.text(), "W");
            for line in start_line..start_line + num_lines + 1 {
                if line > last_line {
                    break;
                }
                let content = if *self.main_split.active != Some(self.view_id)
                    || self.editor.cursor.is_insert()
                    || line == current_line
                {
                    line + 1
                } else if line > current_line {
                    line - current_line
                } else {
                    current_line - line
                };
                let content = content.to_string();

                let text_layout = ctx
                    .text()
                    .new_text_layout(content.clone())
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
                let x = ((last_line + 1).to_string().len() - content.len()) as f64
                    * width;
                let y = line_height * line as f64 - scroll_offset.y
                    + (line_height - text_layout.size().height) / 2.0;
                let pos = Point::new(x, y);
                ctx.draw_text(&text_layout, pos);
            }

            if let Some(changes) = self.buffer.history_changes.get("head") {
                let mut line = 0;
                let mut last_change = None;
                for change in changes.iter() {
                    let len = match change {
                        DiffLines::Left(_range) => 0,
                        DiffLines::Skip(_left, right) => right.len(),
                        DiffLines::Both(_left, right) => right.len(),
                        DiffLines::Right(range) => range.len(),
                    };
                    line += len;
                    if line < start_line {
                        last_change = Some(change.clone());
                        continue;
                    }

                    let mut modified = false;
                    let color = match change {
                        DiffLines::Left(_range) => {
                            Some(self.config.get_color_unchecked(
                                LapceTheme::SOURCE_CONTROL_REMOVED,
                            ))
                        }
                        DiffLines::Right(_range) => {
                            if let Some(last_change) = last_change.as_ref() {
                                if let DiffLines::Left(_l) = last_change {
                                    modified = true;
                                }
                            }
                            if modified {
                                Some(self.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_MODIFIED,
                                ))
                            } else {
                                Some(self.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_ADDED,
                                ))
                            }
                        }
                        _ => None,
                    };

                    if let Some(color) = color {
                        let removed_height = 10.0;
                        let size = Size::new(
                            3.0,
                            if len == 0 {
                                removed_height
                            } else {
                                line_height * len as f64
                            },
                        );
                        let x = gutter_width + width;
                        let mut y =
                            (line - len) as f64 * line_height - scroll_offset.y;
                        if len == 0 {
                            y -= removed_height / 2.0;
                        }
                        if modified {
                            let rect = Size::new(3.0, removed_height)
                                .to_rect()
                                .with_origin(Point::new(
                                    x,
                                    y - removed_height / 2.0,
                                ));
                            ctx.fill(
                                rect,
                                self.config.get_color_unchecked(
                                    LapceTheme::EDITOR_BACKGROUND,
                                ),
                            );
                        }
                        let rect = size.to_rect().with_origin(Point::new(x, y));
                        ctx.fill(rect, &color.clone().with_alpha(0.8));
                    }

                    if line > end_line {
                        break;
                    }
                    last_change = Some(change.clone());
                }
            }

            if *self.main_split.active == Some(self.view_id) {
                self.paint_code_actions_hint(ctx, gutter_width);
            }
        });
    }

    fn paint_code_actions_hint(&self, ctx: &mut PaintCtx, gutter_width: f64) {
        if let Some(actions) = self.current_code_actions() {
            if !actions.is_empty() {
                let line_height = self.config.editor.line_height as f64;
                let offset = self.editor.cursor.offset();
                let (line, _) = self
                    .buffer
                    .offset_to_line_col(offset, self.config.editor.tab_width);
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

    // TODO: Use or remove
    fn _paint_code_lens_line(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        is_focused: bool,
        cursor_line: usize,
        y: f64,
        y_shift: f64,
        bounds: [f64; 2],
        code_lens: bool,
        char_width: f64,
        code_lens_char_width: f64,
        config: &Config,
    ) {
        if line > self.buffer.last_line() {
            return;
        }

        let line_content = if let Some(syntax) = self.buffer.syntax.as_ref() {
            let rope = &syntax.text;
            let last_line = rope.line_of_offset(rope.len());
            if line > last_line {
                return;
            }
            let start = rope.offset_of_line(line);
            let end = rope.offset_of_line(line + 1);
            rope.slice_to_cow(start..end)
        } else {
            self.buffer.line_content(line)
        };

        let mut x = 0.0;
        let mut x_shift = 0.0;
        let mut start_char = 0;
        if code_lens {
            for ch in line_content.chars() {
                if ch == ' ' {
                    x += char_width;
                    start_char += 1;
                } else if ch == '\t' {
                    x += char_width * config.editor.tab_width as f64;
                    start_char += 1;
                } else {
                    break;
                }
            }

            x_shift = x - start_char as f64 * code_lens_char_width;
        }

        let line_height = if code_lens {
            config.editor.code_lens_font_size as f64
        } else {
            config.editor.line_height as f64
        };

        self.paint_cursor_on_line(
            ctx,
            is_focused,
            cursor_line,
            line,
            x_shift,
            y,
            if code_lens {
                code_lens_char_width
            } else {
                char_width
            },
            line_height,
            config,
        );
        let text_layout = self.buffer.new_text_layout(
            ctx,
            line,
            &line_content[start_char..],
            None,
            12,
            bounds,
            config,
        );
        ctx.draw_text(
            &text_layout,
            Point::new(x, if code_lens { y } else { y + y_shift }),
        );
    }

    pub fn paint_code_lens_content(
        &self,
        ctx: &mut PaintCtx,
        is_focused: bool,
        config: &Config,
    ) {
        let rect = ctx.region().bounding_box();

        let ref_text_layout = ctx
            .text()
            .new_text_layout("W")
            .font(
                self.config.editor.font_family(),
                self.config.editor.font_size as f64,
            )
            .build()
            .unwrap();
        let char_width = ref_text_layout.size().width;
        let y_shift =
            (config.editor.line_height as f64 - ref_text_layout.size().height) / 2.0;
        let small_char_width =
            config.char_width(ctx.text(), config.editor.code_lens_font_size as f64);

        let empty_lens = Syntax::lens_from_normal_lines(
            self.buffer.len(),
            config.editor.line_height,
            config.editor.code_lens_font_size,
            &[],
        );
        let (rope, lens) = if let Some(syntax) = self.buffer.syntax.as_ref() {
            (&syntax.text, &syntax.lens)
        } else {
            (&self.buffer.rope, &empty_lens)
        };

        let cursor_line =
            rope.line_of_offset(self.editor.cursor.offset().min(rope.len()));
        let last_line = rope.line_of_offset(rope.len());
        let start_line =
            lens.line_of_height(rect.y0.floor() as usize).min(last_line);
        let end_line = lens
            .line_of_height(rect.y1.ceil() as usize + config.editor.line_height)
            .min(last_line);
        let start_offset = rope.offset_of_line(start_line);
        let end_offset = rope.offset_of_line(end_line + 1);
        let mut lines_iter = rope.lines(start_offset..end_offset);

        let mut y = lens.height_of_line(start_line) as f64;
        for (line, line_height) in lens.iter_chunks(start_line..end_line) {
            let is_small = line_height < config.editor.line_height;
            let line_content = lines_iter.next().unwrap();

            let mut x = 0.0;
            if is_small {
                for ch in line_content.chars() {
                    if ch == ' ' {
                        x += char_width - small_char_width;
                    } else if ch == '\t' {
                        x += (char_width - small_char_width)
                            * config.editor.tab_width as f64;
                    } else {
                        break;
                    }
                }
            }

            self.paint_cursor_on_line(
                ctx,
                is_focused,
                cursor_line,
                line,
                x,
                y,
                if is_small {
                    small_char_width
                } else {
                    char_width
                },
                line_height as f64,
                config,
            );
            let text_layout = self.buffer.new_text_layout(
                ctx,
                line,
                &line_content,
                None,
                if is_small {
                    config.editor.code_lens_font_size
                } else {
                    config.editor.font_size
                },
                [rect.x0, rect.x1],
                config,
            );
            ctx.draw_text(
                &text_layout,
                Point::new(x, if is_small { y } else { y + y_shift }),
            );
            y += line_height as f64;
        }
    }

    pub fn paint_content(
        &self,
        ctx: &mut PaintCtx,
        is_focused: bool,
        placeholder: Option<&String>,
        config: &Config,
    ) {
        let line_height = self.config.editor.line_height as f64;
        if self.editor.compare.is_none() && !self.editor.code_lens {
            self.paint_cursor(ctx, is_focused, placeholder, config);
            self.paint_find(ctx);
        }
        let self_size = ctx.size();
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
        let char_width = text_layout.size().width;
        let y_shift = (line_height - text_layout.size().height) / 2.0;

        if self.editor.code_lens {
            self.paint_code_lens_content(ctx, is_focused, config);
        } else if let Some(compare) = self.editor.compare.as_ref() {
            if let Some(changes) = self.buffer.history_changes.get(compare) {
                let cursor_line =
                    self.buffer.line_of_offset(self.editor.cursor.offset());
                let mut line = 0;
                for change in changes.iter() {
                    match change {
                        DiffLines::Left(range) => {
                            let len = range.len();
                            line += len;

                            if line < start_line {
                                continue;
                            }
                            ctx.fill(
                                Size::new(self_size.width, line_height * len as f64)
                                    .to_rect()
                                    .with_origin(Point::new(
                                        0.0,
                                        line_height * (line - len) as f64,
                                    )),
                                config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_REMOVED,
                                ),
                            );
                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let actual_line = l - (line - len) + range.start;
                                if let Some(text_layout) =
                                    self.buffer.history_text_layout(
                                        ctx,
                                        compare,
                                        actual_line,
                                        None,
                                        [rect.x0, rect.x1],
                                        config,
                                    )
                                {
                                    ctx.draw_text(
                                        &text_layout,
                                        Point::new(
                                            0.0,
                                            line_height * l as f64 + y_shift,
                                        ),
                                    );
                                }
                                if l > end_line {
                                    break;
                                }
                            }
                        }
                        DiffLines::Skip(left, right) => {
                            let rect = Size::new(self_size.width, line_height)
                                .to_rect()
                                .with_origin(Point::new(
                                    0.0,
                                    line_height * line as f64,
                                ));
                            ctx.fill(
                                rect,
                                self.config.get_color_unchecked(
                                    LapceTheme::PANEL_BACKGROUND,
                                ),
                            );
                            ctx.stroke(
                                rect,
                                config.get_color_unchecked(
                                    LapceTheme::EDITOR_FOREGROUND,
                                ),
                                1.0,
                            );
                            let text_layout = ctx
                                .text()
                                .new_text_layout(format!(
                                    " -{}, +{}",
                                    left.end + 1,
                                    right.end + 1
                                ))
                                .font(
                                    config.editor.font_family(),
                                    config.editor.font_size as f64,
                                )
                                .text_color(
                                    config
                                        .get_color_unchecked(
                                            LapceTheme::EDITOR_FOREGROUND,
                                        )
                                        .clone(),
                                )
                                .build_with_info(
                                    true,
                                    config.editor.tab_width,
                                    Some([rect.x0, rect.x1]),
                                );
                            ctx.draw_text(
                                &text_layout,
                                Point::new(0.0, line_height * line as f64 + y_shift),
                            );
                            line += 1;
                        }
                        DiffLines::Both(_left, right) => {
                            let len = right.len();
                            line += len;
                            if line < start_line {
                                continue;
                            }
                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let rope_line = l - (line - len) + right.start;
                                self.paint_cursor_on_line(
                                    ctx,
                                    is_focused,
                                    cursor_line,
                                    rope_line,
                                    0.0,
                                    l as f64 * line_height,
                                    char_width,
                                    line_height,
                                    config,
                                );
                                let text_layout = self.buffer.new_text_layout(
                                    ctx,
                                    rope_line,
                                    &self.buffer.line_content(rope_line),
                                    None,
                                    config.editor.font_size,
                                    [rect.x0, rect.x1],
                                    &self.config,
                                );
                                ctx.draw_text(
                                    &text_layout,
                                    Point::new(
                                        0.0,
                                        line_height * l as f64 + y_shift,
                                    ),
                                );
                                if l > end_line {
                                    break;
                                }
                            }
                        }
                        DiffLines::Right(range) => {
                            let len = range.len();
                            line += len;

                            if line < start_line {
                                continue;
                            }

                            ctx.fill(
                                Size::new(
                                    self_size.width,
                                    line_height * range.len() as f64,
                                )
                                .to_rect()
                                .with_origin(
                                    Point::new(
                                        0.0,
                                        line_height * (line - range.len()) as f64,
                                    ),
                                ),
                                config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_ADDED,
                                ),
                            );

                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let rope_line = l - (line - len) + range.start;
                                self.paint_cursor_on_line(
                                    ctx,
                                    is_focused,
                                    cursor_line,
                                    rope_line,
                                    0.0,
                                    l as f64 * line_height,
                                    char_width,
                                    line_height,
                                    config,
                                );
                                let text_layout = self.buffer.new_text_layout(
                                    ctx,
                                    rope_line,
                                    &self.buffer.line_content(rope_line),
                                    None,
                                    config.editor.font_size,
                                    [rect.x0, rect.x1],
                                    &self.config,
                                );
                                ctx.draw_text(
                                    &text_layout,
                                    Point::new(
                                        0.0,
                                        line_height * l as f64 + y_shift,
                                    ),
                                );
                                if l > end_line {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            return;
        } else {
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
                        let cursor_line_start = self
                            .buffer
                            .offset_of_line(cursor_line)
                            .min(self.buffer.len());
                        let index = self
                            .buffer
                            .slice_to_cow(cursor_line_start..cursor_offset)
                            .len();
                        Some(index)
                    } else {
                        None
                    };
                let text_layout = self.buffer.new_text_layout(
                    ctx,
                    line,
                    line_content,
                    cursor_index,
                    config.editor.font_size,
                    [rect.x0, rect.x1],
                    &self.config,
                );
                ctx.draw_text(
                    &text_layout,
                    Point::new(0.0, line_height * line as f64 + y_shift),
                );
            }
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

    fn paint_cursor_on_line(
        &self,
        ctx: &mut PaintCtx,
        is_focused: bool,
        cursor_line: usize,
        actual_line: usize,
        x_shift: f64,
        y: f64,
        char_width: f64,
        line_height: f64,
        config: &Config,
    ) {
        match &self.editor.cursor.mode {
            CursorMode::Normal(_) => {}
            CursorMode::Visual { start, end, mode } => {
                let (start_line, start_col) = self.buffer.offset_to_line_col(
                    *start.min(end),
                    self.config.editor.tab_width,
                );
                let (end_line, end_col) = self.buffer.offset_to_line_col(
                    *start.max(end),
                    self.config.editor.tab_width,
                );
                if actual_line < start_line || actual_line > end_line {
                    return;
                }

                let left_col = match mode {
                    VisualMode::Normal => {
                        if start_line == actual_line {
                            start_col
                        } else {
                            0
                        }
                    }
                    VisualMode::Linewise => 0,
                    VisualMode::Blockwise => {
                        let max_col = self.buffer.line_end_col(
                            actual_line,
                            false,
                            self.config.editor.tab_width,
                        );
                        let left = start_col.min(end_col);
                        if left > max_col {
                            return;
                        }
                        left
                    }
                };

                let right_col = match mode {
                    VisualMode::Normal => {
                        if actual_line == end_line {
                            let max_col = self.buffer.line_end_col(
                                actual_line,
                                true,
                                self.config.editor.tab_width,
                            );
                            (end_col + 1).min(max_col)
                        } else {
                            self.buffer.line_end_col(
                                actual_line,
                                true,
                                self.config.editor.tab_width,
                            ) + 1
                        }
                    }
                    VisualMode::Linewise => {
                        self.buffer.line_end_col(
                            actual_line,
                            true,
                            self.config.editor.tab_width,
                        ) + 1
                    }
                    VisualMode::Blockwise => {
                        let max_col = self.buffer.line_end_col(
                            actual_line,
                            true,
                            self.config.editor.tab_width,
                        );
                        let right = match self.editor.cursor.horiz.as_ref() {
                            Some(&ColPosition::End) => max_col,
                            _ => (end_col.max(start_col) + 1).min(max_col),
                        };
                        right
                    }
                };

                let x0 = left_col as f64 * char_width + x_shift;
                let x1 = right_col as f64 * char_width + x_shift;
                let y0 = y;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    self.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );
            }
            CursorMode::Insert(selection) => {
                let start_offset = self.buffer.offset_of_line(actual_line);
                let end_offset = self.buffer.offset_of_line(actual_line + 1);
                let regions = selection.regions_in_range(start_offset, end_offset);
                for region in regions {
                    if region.is_caret() {
                        let caret_actual_line =
                            self.buffer.line_of_offset(region.end());
                        if caret_actual_line == actual_line {
                            let size = ctx.size();
                            ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(0.0, y))
                                    .with_size(Size::new(size.width, line_height)),
                                self.config.get_color_unchecked(
                                    LapceTheme::EDITOR_CURRENT_LINE,
                                ),
                            );
                        }
                    } else {
                        let start = region.start();
                        let end = region.end();
                        let (start_line, start_col) =
                            self.buffer.offset_to_line_col(
                                start.min(end),
                                self.config.editor.tab_width,
                            );
                        let (end_line, end_col) = self.buffer.offset_to_line_col(
                            start.max(end),
                            self.config.editor.tab_width,
                        );
                        let left_col = match actual_line {
                            _ if actual_line == start_line => start_col,
                            _ => 0,
                        };
                        let right_col = match actual_line {
                            _ if actual_line == end_line => {
                                let max_col = self.buffer.line_end_col(
                                    actual_line,
                                    true,
                                    self.config.editor.tab_width,
                                );
                                end_col.min(max_col)
                            }
                            _ => self.buffer.line_end_col(
                                actual_line,
                                true,
                                self.config.editor.tab_width,
                            ),
                        };
                        let x0 = left_col as f64 * char_width + x_shift;
                        let x1 = right_col as f64 * char_width + x_shift;
                        let y0 = y;
                        let y1 = y0 + line_height;
                        ctx.fill(
                            Rect::new(x0, y0, x1, y1),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                        );
                    }
                }
                for region in regions {
                    if is_focused {
                        let (caret_actual_line, col) =
                            self.buffer.offset_to_line_col(
                                region.end(),
                                self.config.editor.tab_width,
                            );
                        if caret_actual_line == actual_line {
                            let x = col as f64 * char_width + x_shift;
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
        if cursor_line == actual_line {
            if let CursorMode::Normal(_) = &self.editor.cursor.mode {
                let size = ctx.size();
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, y))
                        .with_size(Size::new(size.width, line_height)),
                    self.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            match &self.editor.cursor.mode {
                CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                    if is_focused {
                        let (x0, x1) = self.editor.cursor.current_char(
                            &self.buffer,
                            char_width,
                            config,
                        );
                        let cursor_width =
                            if x1 > x0 { x1 - x0 } else { char_width };
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(x0 + x_shift, y))
                                .with_size(Size::new(cursor_width, line_height)),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                        );
                    }
                }
                CursorMode::Insert(_) => {}
            }
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
    }

    pub fn double_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        config: &Config,
    ) {
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
                let line = self.buffer.line_of_offset(*offset);
                self.paint_cursor_line(ctx, line, is_focused, placeholder);

                if is_focused {
                    let (x0, x1) =
                        self.editor.cursor.current_char(&self.buffer, width, config);
                    let char_width = if x1 > x0 { x1 - x0 } else { width };
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(x0, line as f64 * line_height))
                            .with_size(Size::new(char_width, line_height)),
                        self.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                    );
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) = self.buffer.offset_to_line_col(
                    *start.min(end),
                    self.config.editor.tab_width,
                );
                let (end_line, end_col) = self.buffer.offset_to_line_col(
                    *start.max(end),
                    self.config.editor.tab_width,
                );
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = self.buffer.line_content(line);
                    let left_col = match mode {
                        VisualMode::Normal => match line {
                            _ if line == start_line => start_col,
                            _ => 0,
                        },
                        VisualMode::Linewise => 0,
                        VisualMode::Blockwise => {
                            let max_col = self.buffer.line_end_col(
                                line,
                                false,
                                self.config.editor.tab_width,
                            );
                            let left = start_col.min(end_col);
                            if left > max_col {
                                continue;
                            }
                            left
                        }
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match mode {
                        VisualMode::Normal => match line {
                            _ if line == end_line => {
                                let max_col = self.buffer.line_end_col(
                                    line,
                                    true,
                                    self.config.editor.tab_width,
                                );
                                (end_col + 1).min(max_col)
                            }
                            _ => {
                                self.buffer.line_end_col(
                                    line,
                                    true,
                                    self.config.editor.tab_width,
                                ) + 1
                            }
                        },
                        VisualMode::Linewise => {
                            self.buffer.line_end_col(
                                line,
                                true,
                                self.config.editor.tab_width,
                            ) + 1
                        }
                        VisualMode::Blockwise => {
                            let max_col = self.buffer.line_end_col(
                                line,
                                true,
                                self.config.editor.tab_width,
                            );
                            let right = match self.editor.cursor.horiz.as_ref() {
                                Some(&ColPosition::End) => max_col,
                                _ => (end_col.max(start_col) + 1).min(max_col),
                            };
                            right
                        }
                    };
                    if !line_content.is_empty() {
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
                        let line = self.buffer.line_of_offset(*end);

                        let (x0, x1) = self.editor.cursor.current_char(
                            &self.buffer,
                            width,
                            config,
                        );
                        let char_width = if x1 > x0 { x1 - x0 } else { width };
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(
                                    x0,
                                    line as f64 * line_height,
                                ))
                                .with_size(Size::new(char_width, line_height)),
                            self.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                        );
                    }
                }
            }
            CursorMode::Insert(selection) => {
                let offset = selection.get_cursor_offset();
                let _line = self.buffer.line_of_offset(offset);
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
                            self.buffer.offset_to_line_col(
                                start.min(end),
                                self.config.editor.tab_width,
                            );
                        let (end_line, end_col) = self.buffer.offset_to_line_col(
                            start.max(end),
                            self.config.editor.tab_width,
                        );
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
                                    let max_col = self.buffer.line_end_col(
                                        line,
                                        true,
                                        self.config.editor.tab_width,
                                    );
                                    end_col.min(max_col)
                                }
                                _ => self.buffer.line_end_col(
                                    line,
                                    true,
                                    self.config.editor.tab_width,
                                ),
                            };

                            if !line_content.is_empty() {
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
                }

                for region in regions {
                    if is_focused {
                        let (line, col) = self.buffer.offset_to_line_col(
                            region.end(),
                            self.config.editor.tab_width,
                        );
                        let x = col as f64 * width;
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
        if self.editor.content.is_input() {
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
        if self.editor.content.is_search() {
            return;
        }
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
                let (start_line, start_col) = self
                    .buffer
                    .offset_to_line_col(start, self.config.editor.tab_width);
                let (end_line, end_col) = self
                    .buffer
                    .offset_to_line_col(end, self.config.editor.tab_width);
                for line in start_line..end_line + 1 {
                    let left_col = if line == start_line { start_col } else { 0 };
                    let right_col = if line == end_line {
                        end_col
                    } else {
                        self.buffer.line_end_col(
                            line,
                            true,
                            self.config.editor.tab_width,
                        ) + 1
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
                let (start_line, start_col) = self.buffer.offset_to_line_col(
                    *start.min(end),
                    self.config.editor.tab_width,
                );
                let (end_line, end_col) = self.buffer.offset_to_line_col(
                    *start.max(end),
                    self.config.editor.tab_width,
                );
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
                            let max_col = self.buffer.line_end_col(
                                line,
                                true,
                                self.config.editor.tab_width,
                            );
                            end_col.min(max_col)
                        }
                        _ => self.buffer.line_end_col(
                            line,
                            true,
                            self.config.editor.tab_width,
                        ),
                    };
                    if !line_content.is_empty() {
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
                        self.buffer
                            .offset_of_position(&start, self.config.editor.tab_width)
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
                                self.config.editor.tab_width,
                            );
                            col as f64 * width
                        };
                        let x1 = if line == end.line as usize {
                            end.character as f64 * width
                        } else {
                            (self.buffer.line_end_col(
                                line,
                                false,
                                self.config.editor.tab_width,
                            ) + 1) as f64
                                * width
                        };
                        let _y1 = (line + 1) as f64 * line_height;
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
                        paint_wave_line(ctx, Point::new(x0, y0), x1 - x0, color);
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
                    .unwrap_or_else(Vec::new);

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

impl KeyPressFocus for LapceEditorBufferData {
    fn get_mode(&self) -> Mode {
        self.editor.cursor.get_mode()
    }

    fn expect_char(&self) -> bool {
        self.editor.inline_find.is_some()
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "editor_focus" => match self.editor.content {
                BufferContent::File(_) => true,
                BufferContent::Local(_) => false,
            },
            "diff_focus" => self.editor.compare.is_some(),
            "source_control_focus" => {
                self.editor.content
                    == BufferContent::Local(LocalBufferKind::SourceControl)
            }
            "search_focus" => {
                self.editor.content == BufferContent::Local(LocalBufferKind::Search)
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
            return CommandExecuted::Yes;
        }
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
                );
            }
            LapceCommand::SplitVertical => {
                self.main_split.split_editor(
                    ctx,
                    Arc::make_mut(&mut self.editor),
                    SplitDirection::Vertical,
                );
            }
            LapceCommand::SplitClose => {
                self.main_split.editor_close(ctx, self.view_id);
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
                        self.editor.code_lens,
                        self.editor.compare.clone(),
                        &self.config,
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
                    self.editor.code_lens,
                    self.editor.compare.clone(),
                    &self.config,
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
                            self.editor.code_lens,
                            self.editor.compare.clone(),
                            &self.config,
                        );
                        self.buffer_mut().update_edit_type();
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
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
                let register = Arc::make_mut(&mut self.main_split.register);
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

                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
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
                let data = self.main_split.register.unamed.clone();
                self.paste(ctx, &data);
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
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
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
                                    &Movement::Left,
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
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
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
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForwardAndInsert => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
                self.update_completion(ctx);
            }
            LapceCommand::InsertTab => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    "\t",
                    None,
                    true,
                    EditType::InsertChars,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
                self.update_completion(ctx);
            }
            LapceCommand::InsertNewLine => {
                let selection = self
                    .editor
                    .cursor
                    .edit_selection(&self.buffer, self.config.editor.tab_width);
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
                    return CommandExecuted::Yes;
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
                                ctx,
                                &Selection::region(start, end),
                                "",
                                None,
                                true,
                                EditType::InsertChars,
                            );
                            self.edit(
                                ctx,
                                &Selection::caret(
                                    self.buffer.offset_of_line(start_line - 1),
                                ),
                                &content,
                                None,
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
                                ctx,
                                &Selection::caret(
                                    self.buffer.offset_of_line(end_line + 2),
                                ),
                                &content,
                                None,
                                true,
                                EditType::InsertChars,
                            );
                            self.edit(
                                ctx,
                                &Selection::region(start, end),
                                "",
                                None,
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
                                    buffer_id, rev, offset, item,
                                ),
                                Target::Widget(view_id),
                            );
                        }),
                    );
                } else {
                    let _ = self.apply_completion_item(ctx, &item);
                }
            }
            LapceCommand::NormalMode => {
                if !self.config.lapce.modal {
                    return CommandExecuted::Yes;
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
                self.buffer_mut().update_edit_type();

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
                                                    path: PathBuf::from(
                                                        location.uri.path(),
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
            LapceCommand::SearchWholeWordForward => {
                let offset = self.editor.cursor.offset();
                let (start, end) = self.buffer.select_word(offset);
                let word = self.buffer.slice_to_cow(start..end).to_string();
                Arc::make_mut(&mut self.find).set_find(&word, false, false, true);
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, _end)) = next {
                    self.do_move(&Movement::Offset(start), 1, mods);
                }
            }
            LapceCommand::SearchForward => {
                let offset = self.editor.cursor.offset();
                let next = self.find.next(&self.buffer.rope, offset, false, true);
                if let Some((start, _end)) = next {
                    self.do_move(&Movement::Offset(start), 1, mods);
                }
            }
            LapceCommand::SearchBackward => {
                let offset = self.editor.cursor.offset();
                let next = self.find.next(&self.buffer.rope, offset, true, true);
                if let Some((start, _end)) = next {
                    self.do_move(&Movement::Offset(start), 1, mods);
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
                let (line, _col) = self
                    .buffer
                    .offset_to_line_col(offset, self.config.editor.tab_width);
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
                if matching_pair_direction(c).unwrap_or(false)
                    && cursor_char
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
            self.update_completion(ctx);
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

impl TabRect {
    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        widget_id: WidgetId,
        i: usize,
        size: Size,
        mouse_pos: Point,
    ) {
        let width = 13.0;
        let height = 13.0;
        let editor_tab = data.main_split.editor_tabs.get(&widget_id).unwrap();

        let rect = Size::new(width, height).to_rect().with_origin(Point::new(
            self.rect.x0 + (size.height - width) / 2.0,
            (size.height - height) / 2.0,
        ));
        if i == editor_tab.active {
            ctx.fill(
                self.rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        ctx.draw_svg(&self.svg, rect, None);
        let text_size = self.text_layout.size();
        ctx.draw_text(
            &self.text_layout,
            Point::new(
                self.rect.x0 + size.height,
                (size.height - text_size.height) / 2.0,
            ),
        );
        let x = self.rect.x1;
        ctx.stroke(
            Line::new(Point::new(x - 0.5, 0.0), Point::new(x - 0.5, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        if ctx.is_hot() {
            if self.close_rect.contains(mouse_pos) {
                ctx.fill(
                    &self.close_rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            if self.rect.contains(mouse_pos) {
                let svg = get_svg("close.svg").unwrap();
                ctx.draw_svg(
                    &svg,
                    self.close_rect.inflate(-4.0, -4.0),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                );
            }
        }

        // Only display dirty icon if focus is not on tab bar, so that the close svg can be shown
        if !(ctx.is_hot() && self.rect.contains(mouse_pos)) {
            // See if any of the children are dirty
            let is_dirty = match &editor_tab.children[i] {
                EditorTabChild::Editor(editor_id) => {
                    let buffer = data.main_split.editor_buffer(*editor_id);
                    buffer.dirty
                }
            };

            if is_dirty {
                let svg = get_svg("unsaved.svg").unwrap();
                ctx.draw_svg(
                    &svg,
                    self.close_rect.inflate(-4.0, -4.0),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                )
            }
        }
    }
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

// TODO: Use or remove
fn _get_workspace_edit_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.changes.as_ref()?;
    changes.get(url).map(|c| c.iter().collect())
}

// TODO: Use or remove
fn _get_workspace_edit_document_changes_edits<'a>(
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
                    path: PathBuf::from(location.uri.path()),
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
