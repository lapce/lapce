use crate::completion::CompletionState;
use crate::{
    buffer::{Buffer, BufferId, BufferUIState, InvalLines},
    command::EnsureVisiblePosition,
    command::LapceCommand,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    completion::ScoredCompletionItem,
    container::LapceContainer,
    movement::ColPosition,
    movement::LinePosition,
    movement::Movement,
    movement::SelRegion,
    movement::Selection,
    scroll::LapceScroll,
    split::SplitMoveDirection,
    state::LapceTabState,
    state::LapceUIState,
    state::Mode,
    state::VisualMode,
    state::LAPCE_APP_STATE,
    theme::LapceTheme,
};
use anyhow::{anyhow, Result};
use bit_vec::BitVec;
use druid::{
    kurbo::Line, piet::PietText, theme, widget::IdentityWrapper, widget::Padding,
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx,
    FontDescriptor, FontFamily, Insets, KeyEvent, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout,
    UpdateCtx, Vec2, Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};
use druid::{
    piet::{PietTextLayout, Text, TextAttribute, TextLayoutBuilder},
    FontWeight,
};
use fzyr::{has_match, locate};
use lsp_types::{
    CompletionItem, CompletionResponse, Diagnostic, DiagnosticSeverity,
    GotoDefinitionResponse, Location, Position, TextEdit,
};
use serde_json::Value;
use std::str::FromStr;
use std::{cmp::Ordering, iter::Iterator, path::PathBuf};
use std::{collections::HashMap, sync::Arc};
use xi_core_lib::selection::InsertDrift;
use xi_rope::{Interval, RopeDelta};

pub struct LapceUI {
    container: LapceContainer,
}

#[derive(Clone, Debug, Default)]
pub struct Counter(usize);

impl Counter {
    pub fn next(&mut self) -> usize {
        let n = self.0;
        self.0 = n + 1;
        n + 1
    }
}

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
    pub saved_selection: Selection,
    pub saved_scroll_offset: Vec2,
}

#[derive(Clone, Debug)]
pub struct EditorLocation {
    pub path: String,
    pub offset: usize,
    pub scroll_offset: Option<Vec2>,
}

impl EditorState {
    pub fn new(
        tab_id: WidgetId,
        split_id: WidgetId,
        buffer_id: Option<BufferId>,
    ) -> EditorState {
        EditorState {
            editor_id: WidgetId::next(),
            view_id: WidgetId::next(),
            split_id,
            tab_id: tab_id,
            buffer_id,
            char_width: 7.6171875,
            width: 0.0,
            height: 0.0,
            selection: Selection::new_simple(),
            scroll_offset: Vec2::ZERO,
            scroll_size: Size::ZERO,
            view_size: Size::ZERO,
            gutter_width: 0.0,
            header_height: 0.0,
            locations: Vec::new(),
            current_location: 0,
            saved_selection: Selection::new_simple(),
            saved_scroll_offset: Vec2::ZERO,
        }
    }

    pub fn update(
        &self,
        ctx: &mut UpdateCtx,
        data: &LapceUIState,
        old_data: &LapceUIState,
        env: &Env,
    ) -> Option<()> {
        let buffer_id = self.buffer_id.as_ref()?;
        let buffer = data.buffers.get(buffer_id)?;
        let old_buffer = old_data.buffers.get(buffer_id)?;
        let editor = data.get_editor(&self.view_id);
        let old_editor = old_data.get_editor(&self.view_id);
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);

        if buffer.max_len != old_buffer.max_len
            || buffer.text_layouts.len() != old_buffer.text_layouts.len()
        {
            ctx.request_layout();
            return None;
        }

        if editor.selection != old_editor.selection
            || editor.visual_mode != old_editor.visual_mode
            || editor.mode != old_editor.mode
        {
            let rect = Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    editor.selection_start_line as f64 * line_height,
                ))
                .with_size(Size::new(
                    ctx.size().width,
                    (editor.selection_end_line + 1 - editor.selection_start_line)
                        as f64
                        * line_height,
                ));
            ctx.request_paint_rect(rect);

            let rect = Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    old_editor.selection_start_line as f64 * line_height,
                ))
                .with_size(Size::new(
                    ctx.size().width,
                    (old_editor.selection_end_line + 1
                        - old_editor.selection_start_line)
                        as f64
                        * line_height,
                ));
            ctx.request_paint_rect(rect);
        }

        let offset = self.scroll_offset;
        let start_line = (offset.y / line_height) as usize;
        let num_lines = (self.view_size.height / line_height) as usize;
        let mut updated_start_line = None;
        let mut updated_end_line = None;
        for line in start_line..start_line + num_lines + 1 {
            if line >= buffer.text_layouts.len() {
                break;
            }
            if !old_buffer.text_layouts[line].same(&buffer.text_layouts[line]) {
                if updated_start_line.is_none() {
                    updated_start_line = Some(line);
                }
                updated_end_line = Some(line);
            }
        }

        if let Some(updated_start_line) = updated_start_line {
            let updated_end_line = updated_end_line.unwrap();
            let rect = Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    updated_start_line as f64 * line_height,
                ))
                .with_size(Size::new(
                    self.view_size.width,
                    (updated_end_line + 1 - updated_start_line) as f64 * line_height,
                ));
            ctx.request_paint_rect(rect);
        }

        None
    }

    pub fn update_ui_state(
        &mut self,
        ui_state: &mut LapceUIState,
        buffer: &Buffer,
    ) -> Option<()> {
        let editor_ui_state = ui_state.get_editor_mut(&self.view_id);
        editor_ui_state.selection_start_line =
            buffer.line_of_offset(self.selection.min_offset());
        editor_ui_state.selection_end_line =
            buffer.line_of_offset(self.selection.max_offset());
        editor_ui_state.selection = self.selection.clone();
        editor_ui_state.cursor =
            buffer.offset_to_line_col(self.selection.get_cursor_offset());
        None
    }

    fn get_count(
        &self,
        count: Option<usize>,
        operator: Option<EditorOperator>,
    ) -> Option<usize> {
        count.or(operator
            .map(|o| match o {
                EditorOperator::Delete(count) => count.0,
                EditorOperator::Yank(count) => count.0,
            })
            .flatten())
    }

    fn center_of_window(
        &mut self,
        ctx: &mut EventCtx,
        buffer: &mut Buffer,
        env: &Env,
    ) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let line = buffer.line_of_offset(self.selection.get_cursor_offset());
        let y = if line as f64 * line_height > self.view_size.height / 2.0 {
            line as f64 * line_height - self.view_size.height / 2.0
        } else {
            0.0
        };
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((0.0, y)),
            Target::Widget(self.view_id),
        ));
    }

    pub fn save_jump_location(&mut self, buffer: &Buffer) {
        // self.locations.truncate(self.current_location + 1);
        self.locations.push(EditorLocation {
            path: buffer.path.clone(),
            offset: self.selection.get_cursor_offset(),
            scroll_offset: Some(self.scroll_offset),
        });
        self.current_location = self.locations.len();
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
            _ => None,
        }
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        mode: Mode,
        count: Option<usize>,
        buffer: &mut Buffer,
        cmd: LapceCommand,
        operator: Option<EditorOperator>,
        env: &Env,
    ) {
        let count = self.get_count(count, operator);
        if let Some(movement) = self.move_command(count, &cmd) {
            if movement.is_jump() {
                self.save_jump_location(buffer);
            }
            self.selection = buffer.do_move(
                ctx,
                ui_state,
                &mode,
                &movement,
                &self.selection,
                operator,
                count,
            );
            if mode != Mode::Insert {
                self.selection = buffer.correct_offset(&self.selection);
            }
            if movement.is_jump() {
                self.ensure_cursor_visible(
                    ctx,
                    buffer,
                    env,
                    Some(EnsureVisiblePosition::CenterOfWindow),
                );
            } else {
                self.ensure_cursor_visible(ctx, buffer, env, None);
            }
            return;
        }

        match cmd {
            LapceCommand::PageDown => {
                let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
                let lines =
                    (self.view_size.height / line_height / 2.0).floor() as usize;
                self.selection = Movement::Down.update_selection(
                    &self.selection,
                    buffer,
                    lines,
                    mode == Mode::Insert,
                    mode == Mode::Visual,
                );
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Scroll((0.0, lines as f64 * line_height)),
                    Target::Widget(self.view_id),
                ));
            }
            LapceCommand::PageUp => {
                let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
                let lines =
                    (self.view_size.height / line_height / 2.0).floor() as usize;
                self.selection = Movement::Up.update_selection(
                    &self.selection,
                    buffer,
                    lines,
                    mode == Mode::Insert,
                    mode == Mode::Visual,
                );
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Scroll((0.0, -(lines as f64 * line_height))),
                    Target::Widget(self.view_id),
                ));
            }
            LapceCommand::CenterOfWindow => {
                self.center_of_window(ctx, buffer, env);
            }
            LapceCommand::ScrollUp => {
                let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Scroll((0.0, -line_height)),
                    Target::Widget(self.view_id),
                ));
            }
            LapceCommand::ScrollDown => {
                let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Scroll((0.0, line_height)),
                    Target::Widget(self.view_id),
                ));
            }
            LapceCommand::SplitHorizontal => {}
            LapceCommand::SplitRight => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitMove(SplitMoveDirection::Right),
                    Target::Widget(self.split_id),
                ));
            }
            LapceCommand::SplitLeft => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitMove(SplitMoveDirection::Left),
                    Target::Widget(self.split_id),
                ));
            }
            LapceCommand::SplitExchange => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitExchange,
                    Target::Widget(self.split_id),
                ));
            }
            LapceCommand::SplitClose => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitClose,
                    Target::Widget(self.split_id),
                ));
            }
            LapceCommand::NewLineAbove => {}
            LapceCommand::NewLineBelow => {}
            _ => (),
        }

        self.ensure_cursor_visible(ctx, buffer, env, None);
    }

    pub fn get_selection(
        &self,
        buffer: &Buffer,
        mode: &Mode,
        visual_mode: &VisualMode,
        start_insert: bool,
    ) -> Selection {
        match mode {
            Mode::Normal => self.selection.clone(),
            Mode::Insert => self.selection.clone(),
            Mode::Visual => match visual_mode {
                VisualMode::Normal => self.selection.expand(),
                VisualMode::Linewise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, _) =
                            buffer.offset_to_line_col(region.min());
                        let start = buffer.offset_of_line(start_line);
                        let (end_line, _) = buffer.offset_to_line_col(region.max());
                        let mut end = buffer.offset_of_line(end_line + 1);
                        if start_insert {
                            end -= 1;
                        }
                        new_selection.add_region(SelRegion::new(
                            start,
                            end,
                            Some(ColPosition::Col(0)),
                        ));
                    }
                    new_selection
                }
                VisualMode::Blockwise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, start_col) =
                            buffer.offset_to_line_col(region.min());
                        let (end_line, end_col) =
                            buffer.offset_to_line_col(region.max() + 1);
                        let left = start_col.min(end_col);
                        let right = start_col.max(end_col);
                        for line in start_line..end_line + 1 {
                            let max_col = buffer.line_max_col(line, true);
                            if left > max_col {
                                continue;
                            }
                            let right = match region.horiz() {
                                Some(&ColPosition::End) => max_col,
                                _ => {
                                    if right > max_col {
                                        max_col
                                    } else {
                                        right
                                    }
                                }
                            };
                            let offset = buffer.offset_of_line(line);
                            new_selection.add_region(SelRegion::new(
                                offset + left,
                                offset + right,
                                Some(ColPosition::Col(left)),
                            ));
                        }
                    }
                    new_selection
                }
            },
        }
    }

    pub fn insert_mode(
        &mut self,
        buffer: &mut Buffer,
        mode: &Mode,
        visual_mode: &VisualMode,
        position: ColPosition,
    ) {
        match mode {
            Mode::Visual => match visual_mode {
                VisualMode::Blockwise => match position {
                    ColPosition::FirstNonBlank => {
                        let mut selection = Selection::new();
                        for region in self.selection.regions() {
                            let (start_line, start_col) =
                                buffer.offset_to_line_col(region.min());
                            let (end_line, end_col) =
                                buffer.offset_to_line_col(region.max());
                            let left = start_col.min(end_col);
                            for line in start_line..end_line + 1 {
                                let max_col = buffer.line_max_col(line, true);
                                if left > max_col {
                                    continue;
                                }
                                let offset = buffer.offset_of_line(line) + left;
                                selection.add_region(SelRegion::new(
                                    offset,
                                    offset,
                                    Some(ColPosition::Col(left)),
                                ));
                            }
                        }
                        self.selection = selection;
                    }
                    _ => (),
                },
                _ => {
                    self.selection = self.selection.min();
                }
            },
            Mode::Normal => {
                self.selection = Movement::StartOfLine.update_selection(
                    &self.selection,
                    buffer,
                    1,
                    mode == &Mode::Insert,
                    mode == &Mode::Visual,
                )
            }
            _ => (),
        }
    }

    pub fn paste(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        mode: &Mode,
        visual_mode: &VisualMode,
        buffer: &mut Buffer,
        content: &RegisterContent,
        env: &Env,
    ) {
        match content.kind {
            VisualMode::Linewise => {
                let old_offset = self.selection.get_cursor_offset();
                let mut selection = if mode == &Mode::Visual {
                    self.get_selection(buffer, mode, visual_mode, false)
                } else {
                    Selection::caret(buffer.line_end_offset(old_offset, true) + 1)
                };
                for s in &content.content {
                    let delta = buffer.edit(
                        ctx,
                        ui_state,
                        &format!("{}", s),
                        &selection,
                        true,
                    );
                    selection =
                        selection.apply_delta(&delta, false, InsertDrift::Default);
                }
                // let (old_line, _) = buffer.offset_to_line_col(old_offset);
                // let new_offset = buffer.offset_of_line(old_line + 1);
                self.selection = selection.to_start_caret();
            }
            VisualMode::Normal => {
                let mut selection = if mode == &Mode::Visual {
                    self.get_selection(buffer, mode, visual_mode, false)
                } else {
                    Selection::caret(self.selection.get_cursor_offset() + 1)
                };
                for s in &content.content {
                    let delta = buffer.edit(ctx, ui_state, s, &selection, true);
                    selection =
                        selection.apply_delta(&delta, true, InsertDrift::Default);
                }
                self.selection = Selection::caret(selection.get_cursor_offset() - 1);
            }
            VisualMode::Blockwise => (),
        };
        self.ensure_cursor_visible(
            ctx,
            buffer,
            env,
            Some(EnsureVisiblePosition::CenterOfWindow),
        );
    }

    // pub fn insert_new_line(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     buffer: &mut Buffer,
    //     offset: usize,
    //     env: &Env,
    // ) {
    //     let (line, col) = buffer.offset_to_line_col(offset);
    //     let indent = buffer.indent_on_line(line);

    //     let indent = if indent.len() >= col {
    //         indent[..col].to_string()
    //     } else {
    //         let next_line_indent = buffer.indent_on_line(line + 1);
    //         if next_line_indent.len() > indent.len() {
    //             next_line_indent
    //         } else {
    //             indent
    //         }
    //     };

    //     let content = format!("{}{}", "\n", indent);
    //     let selection = Selection::caret(offset);
    //     let delta = buffer.insert(&content, &selection);
    //     self.selection =
    //         selection.apply_delta(&delta, true, InsertDrift::Default);
    //     // let new_offset = offset + content.len();
    //     // self.selection = Selection::caret(new_offset);
    //     self.ensure_cursor_visible(ctx, buffer, env);
    // }

    pub fn selection_apply_delta(&mut self, delta: &RopeDelta) {
        self.selection =
            self.selection
                .apply_delta(delta, true, InsertDrift::Default);
    }

    pub fn delete(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        mode: &Mode,
        visual_mode: &VisualMode,
        buffer: &mut Buffer,
        movement: Movement,
        count: Option<usize>,
    ) {
        let mut selection = self.get_selection(buffer, mode, visual_mode, false);
        if mode != &Mode::Visual {
            selection = movement.update_selection(
                &selection,
                buffer,
                count.unwrap_or(1),
                true,
                true,
            );
        }
        if selection.min_offset() == selection.max_offset() {
            return;
        }
        let delta =
            buffer.edit(ctx, ui_state, "", &selection, mode != &Mode::Insert);
        self.selection = selection.apply_delta(&delta, true, InsertDrift::Default);
    }

    pub fn ensure_cursor_visible(
        &self,
        ctx: &mut EventCtx,
        buffer: &Buffer,
        env: &Env,
        ensure_position: Option<EnsureVisiblePosition>,
    ) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let offset = self.selection.get_cursor_offset();
        let (line, col) = buffer.offset_to_line_col(offset);
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureVisible((
                Rect::ZERO
                    .with_origin(Point::new(
                        col as f64 * self.char_width,
                        line as f64 * line_height,
                    ))
                    .with_size(Size::new(self.char_width, line_height)),
                (self.char_width, line_height),
                ensure_position,
            )),
            self.view_id,
        ));
    }

    pub fn request_layout(&self) {
        // LAPCE_STATE
        //     .submit_ui_command(LapceUICommand::RequestLayout, self.view_id);
    }

    pub fn request_paint(&self) {
        // LAPCE_STATE
        //     .submit_ui_command(LapceUICommand::RequestPaint, self.view_id);
    }

    pub fn request_paint_rect(&self, rect: Rect) {
        // LAPCE_STATE.submit_ui_command(
        //     LapceUICommand::RequestPaintRect(rect),
        //     self.editor_id,
        // );
    }
}

impl EditorUIState {
    pub fn new() -> EditorUIState {
        EditorUIState {
            buffer_id: BufferId(0),
            cursor: (0, 0),
            mode: Mode::Normal,
            visual_mode: VisualMode::Normal,
            selection: Selection::new(),
            selection_start_line: 0,
            selection_end_line: 0,
        }
    }
}

#[derive(Clone)]
pub struct RegisterContent {
    kind: VisualMode,
    content: Vec<String>,
}

#[derive(Clone)]
pub struct EditorSplitState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub active: WidgetId,
    pub editors: HashMap<WidgetId, EditorState>,
    pub buffers: HashMap<BufferId, Buffer>,
    open_files: HashMap<String, BufferId>,
    id_counter: Counter,
    mode: Mode,
    visual_mode: VisualMode,
    operator: Option<EditorOperator>,
    register: HashMap<String, RegisterContent>,
    inserting: bool,
    pub completion: CompletionState,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
}

impl EditorSplitState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> EditorSplitState {
        let editor_split_id = WidgetId::next();
        let editor = EditorState::new(tab_id, editor_split_id.clone(), None);
        let active = editor.view_id.clone();
        let mut editors = HashMap::new();
        editors.insert(editor.view_id, editor);
        EditorSplitState {
            window_id,
            tab_id,
            widget_id: editor_split_id,
            active,
            editors,
            id_counter: Counter::default(),
            buffers: HashMap::new(),
            open_files: HashMap::new(),
            mode: Mode::Normal,
            visual_mode: VisualMode::Normal,
            operator: None,
            register: HashMap::new(),
            inserting: false,
            completion: CompletionState::new(),
            diagnostics: HashMap::new(),
        }
    }

    pub fn set_active(&mut self, widget_id: WidgetId) {
        self.active = widget_id;
    }

    pub fn active(&self) -> WidgetId {
        self.active
    }

    pub fn set_editor_scroll_offset(&mut self, editor_id: WidgetId, offset: Vec2) {
        if let Some(editor) = self.editors.get_mut(&editor_id) {
            editor.scroll_offset = offset;
        }
    }

    pub fn set_editor_size(&mut self, editor_id: WidgetId, size: Size) {
        if let Some(editor) = self.editors.get_mut(&editor_id) {
            editor.height = size.height;
            editor.width = size.width;
        }
    }

    pub fn get_buffer_from_path(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        path: &str,
    ) -> &Buffer {
        let buffer_id = if let Some(buffer_id) = self.open_files.get(path) {
            buffer_id.clone()
        } else {
            let buffer_id = self.next_buffer_id();
            let buffer = Buffer::new(
                self.window_id.clone(),
                self.tab_id.clone(),
                buffer_id.clone(),
                path,
                ctx.get_external_handle(),
                ui_state.highlight_sender.clone(),
            );
            let num_lines = buffer.num_lines();
            let (max_len, max_len_line) = buffer.get_max_line_len();
            self.buffers.insert(buffer_id.clone(), buffer);
            Arc::make_mut(&mut ui_state.buffers).insert(
                buffer_id.clone(),
                Arc::new(BufferUIState::new(
                    self.window_id.clone(),
                    self.tab_id.clone(),
                    buffer_id.clone(),
                    num_lines,
                    max_len,
                    max_len_line,
                )),
            );
            self.open_files.insert(path.to_string(), buffer_id.clone());
            buffer_id
        };
        self.buffers.get(&buffer_id).unwrap()
    }

    pub fn clear_buffer_text_layouts(
        &mut self,
        ui_state: &mut LapceUIState,
        buffer_id: BufferId,
    ) {
        for (view_id, editor) in self.editors.iter() {
            if editor.buffer_id.as_ref() == Some(&buffer_id) {
                return;
            }
        }
        let mut old_buffer = Arc::make_mut(&mut ui_state.buffers)
            .get_mut(&buffer_id)
            .unwrap();
        for mut text_layout in Arc::make_mut(&mut old_buffer).text_layouts.iter_mut()
        {
            if text_layout.is_some() {
                *Arc::make_mut(&mut text_layout) = None;
            }
        }
    }

    pub fn open_file(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        path: &str,
    ) {
        let buffer = self.get_buffer_from_path(ctx, ui_state, path);
        let buffer_id = buffer.id.clone();
        let editor = self.editors.get(&self.active).unwrap();
        if editor.buffer_id.as_ref() == Some(&buffer_id) {
            return;
        }
        let old_buffer_id = editor.buffer_id.clone();
        let editor = self.editors.get_mut(&self.active).unwrap();
        editor.buffer_id = Some(buffer_id.clone());
        editor.selection = Selection::new_simple();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((0.0, 0.0)),
            Target::Widget(editor.view_id),
        ));
        if let Some(old_buffer_id) = old_buffer_id {
            self.clear_buffer_text_layouts(ui_state, old_buffer_id);
        }
        self.notify_fill_text_layouts(ctx, &buffer_id);
        ctx.request_layout();
    }

    fn next_buffer_id(&mut self) -> BufferId {
        BufferId(self.id_counter.next())
    }

    pub fn get_buffer(&mut self, id: &BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(id)
    }

    pub fn get_buffer_id(&self, view_id: &WidgetId) -> Option<BufferId> {
        self.editors
            .get(view_id)
            .map(|e| e.buffer_id.clone())
            .unwrap()
    }

    fn get_editor(&mut self, view_id: &WidgetId) -> &mut EditorState {
        self.editors.get_mut(view_id).unwrap()
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        match self.mode {
            Mode::Visual => match self.visual_mode {
                _ if self.visual_mode == visual_mode => {
                    self.mode = Mode::Normal;
                    if let Some(editor) = self.editors.get_mut(&self.active) {
                        editor.selection = editor.selection.to_caret();
                    }
                }
                _ => self.visual_mode = visual_mode,
            },
            _ => {
                self.mode = Mode::Visual;
                self.visual_mode = visual_mode;
            }
        };
    }

    fn get_active_editor(&mut self) -> Option<&mut EditorState> {
        self.editors.get_mut(&self.active)
    }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    pub fn has_operator(&self) -> bool {
        self.operator.is_some()
    }

    pub fn notify_fill_text_layouts(
        &self,
        ctx: &mut EventCtx,
        buffer_id: &BufferId,
    ) {
        for (view_id, editor) in self.editors.iter() {
            if editor.buffer_id.as_ref() == Some(&buffer_id) {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(view_id.clone()),
                ));
            }
        }
    }

    pub fn save_selection(&mut self) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        editor.saved_selection = editor.selection.clone();
        editor.saved_scroll_offset = editor.scroll_offset.clone();
        None
    }

    pub fn restore_selection(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer = self.buffers.get(editor.buffer_id.as_ref()?)?;
        editor.selection = editor.saved_selection.clone();
        editor.update_ui_state(ui_state, buffer);
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((
                editor.saved_scroll_offset.x,
                editor.saved_scroll_offset.y,
            )),
            Target::Widget(editor.view_id),
        ));
        None
    }

    pub fn jump_to_postion(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        position: &Position,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let offset = buffer.offset_of_line(position.line as usize)
            + position.character as usize;
        editor.selection = Selection::caret(offset);
        editor.ensure_cursor_visible(
            ctx,
            buffer,
            env,
            Some(EnsureVisiblePosition::CenterOfWindow),
        );
        editor.update_ui_state(ui_state, buffer);
        self.notify_fill_text_layouts(ctx, &buffer_id);
        None
    }

    pub fn jump_to_line(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        line: usize,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
        editor.selection = buffer.do_move(
            ctx,
            buffer_ui_state,
            &Mode::Normal,
            &Movement::Line(LinePosition::Line(line)),
            &editor.selection,
            None,
            None,
        );
        editor.ensure_cursor_visible(
            ctx,
            buffer,
            env,
            Some(EnsureVisiblePosition::CenterOfWindow),
        );
        editor.update_ui_state(ui_state, buffer);
        self.notify_fill_text_layouts(ctx, &buffer_id);
        None
    }

    pub fn insert(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        content: &str,
        env: &Env,
    ) -> Option<()> {
        if self.mode != Mode::Insert {
            return None;
        }
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
        let delta = buffer.edit(
            ctx,
            buffer_ui_state,
            content,
            &editor.selection,
            !self.inserting,
        );
        editor.selection_apply_delta(&delta);
        editor.ensure_cursor_visible(ctx, buffer, env, None);
        self.update_completion(ctx);
        self.inactive_editor_apply_delta(&delta);
        self.notify_fill_text_layouts(ctx, &buffer_id);
        self.inserting = true;
        None
    }

    pub fn update_completion(&mut self, ctx: &mut EventCtx) -> Option<()> {
        let editor = self.editors.get(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get(&buffer_id)?;
        let offset = editor.selection.get_cursor_offset();
        let prev_offset = buffer.prev_code_boundary(offset);
        let next_offset = buffer.next_code_boundary(offset);
        let prev_char = buffer
            .slice_to_cow(prev_offset - 1..prev_offset)
            .to_string();
        let input = buffer.slice_to_cow(prev_offset..next_offset).to_string();
        if input == "" && prev_char != "." && prev_char != ":" {
            self.completion.cancel(ctx);
            return None;
        }
        if prev_offset != self.completion.offset {
            self.completion.offset = prev_offset;
            LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .plugins
                .lock()
                .get_completion(&buffer_id, prev_offset, prev_offset);
            LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .lsp
                .lock()
                .get_completion(
                    prev_offset,
                    buffer,
                    buffer.offset_to_position(prev_offset),
                );
        } else {
            self.completion.update_input(ctx, input);
        }

        None
    }

    pub fn fill_text_layouts(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceUIState,
        offset: Vec2,
        editor_id: &WidgetId,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get(editor_id)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer_ui =
            Arc::make_mut(Arc::make_mut(&mut data.buffers).get_mut(buffer_id)?);
        let buffer = self.buffers.get_mut(buffer_id)?;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let start_line = (offset.y / line_height) as usize;
        let size = ctx.size();
        let num_lines = (size.height / line_height) as usize;
        let text = ctx.text();
        for line in start_line..start_line + num_lines + 1 {
            buffer_ui.update_line_layouts(text, buffer, line, env);
        }
        None
    }

    pub fn insert_new_line(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        offset: usize,
        new_undo_group: bool,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);

        let (line, col) = buffer.offset_to_line_col(offset);
        let indent = buffer.indent_on_line(line);

        let indent = if indent.len() >= col {
            indent[..col].to_string()
        } else {
            let next_line_indent = buffer.indent_on_line(line + 1);
            if next_line_indent.len() > indent.len() {
                next_line_indent
            } else {
                indent
            }
        };

        let content = format!("{}{}", "\n", indent);
        let selection = Selection::caret(offset);
        let delta =
            buffer.edit(ctx, buffer_ui_state, &content, &selection, new_undo_group);
        editor.selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        editor.ensure_cursor_visible(ctx, buffer, env, None);

        self.inactive_editor_apply_delta(&delta);
        None
    }

    pub fn inactive_editor_apply_delta(&mut self, delta: &RopeDelta) -> Option<()> {
        let buffer_id = self.editors.get(&self.active)?.buffer_id.as_ref()?.clone();
        for (_, other_editor) in self.editors.iter_mut() {
            if self.active != other_editor.view_id
                && other_editor.buffer_id.as_ref() == Some(&buffer_id)
            {
                other_editor.selection = other_editor.selection.apply_delta(
                    &delta,
                    true,
                    InsertDrift::Default,
                );
            }
        }

        let buffer = self.buffers.get(&buffer_id)?;
        for (_, editor) in self.editors.iter_mut() {
            for location in editor.locations.iter_mut() {
                if location.path == buffer.path {
                    location.offset = Selection::caret(location.offset)
                        .apply_delta(delta, true, InsertDrift::Default)
                        .get_cursor_offset();
                }
            }
        }
        None
    }

    pub fn next_error(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) -> Option<()> {
        let diagnostics = self.diagnostics.clone();
        let mut file_diagnostics = diagnostics
            .iter()
            .filter_map(|(path, diagnositics)| {
                let buffer = self.get_buffer_from_path(ctx, ui_state, path);
                let mut errors: Vec<usize> = diagnositics
                    .iter()
                    .filter_map(|d| {
                        let severity = d.severity?;
                        if severity != DiagnosticSeverity::Error {
                            return None;
                        }
                        Some(
                            buffer.offset_of_line(d.range.start.line as usize)
                                + d.range.start.character as usize,
                        )
                    })
                    .collect();
                if errors.len() == 0 {
                    None
                } else {
                    errors.sort();
                    Some((path, errors))
                }
            })
            .collect::<Vec<(&String, Vec<usize>)>>();
        if file_diagnostics.len() == 0 {
            return None;
        }
        file_diagnostics.sort_by(|a, b| a.0.cmp(b.0));

        let editor = self.editors.get(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get(buffer_id)?;
        let (path, offset) = next_in_file_errors_offset(
            editor.selection.get_cursor_offset(),
            &buffer.path,
            &file_diagnostics,
        );
        let location = EditorLocation {
            path,
            offset,
            scroll_offset: None,
        };
        self.save_jump_location();
        self.jump_to_location(ctx, ui_state, &location, env);
        None
    }

    pub fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        if editor.current_location >= editor.locations.len() - 1 {
            return None;
        }
        editor.current_location += 1;
        let location = editor.locations[editor.current_location].clone();
        self.jump_to_location(ctx, ui_state, &location, env);
        None
    }

    pub fn jump_location_backward(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get(buffer_id)?;
        if editor.current_location < 1 {
            return None;
        }
        if editor.current_location >= editor.locations.len() {
            editor.save_jump_location(buffer);
            editor.current_location -= 1;
        }
        editor.current_location -= 1;
        let location = editor.locations[editor.current_location].clone();
        self.jump_to_location(ctx, ui_state, &location, env);
        None
    }

    pub fn save_jump_location(&mut self) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get(buffer_id)?;
        editor.save_jump_location(buffer);
        None
    }

    pub fn jump_to_location(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        location: &EditorLocation,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get(buffer_id)?;

        let mut new_buffer = false;
        if buffer.path != location.path {
            self.open_file(ctx, ui_state, &location.path);
            new_buffer = true;
        }

        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get(&buffer_id)?;
        editor.selection = Selection::caret(location.offset);
        if let Some(scroll_offset) = location.scroll_offset {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ScrollTo((scroll_offset.x, scroll_offset.y)),
                Target::Widget(editor.view_id),
            ));
        } else {
            if new_buffer {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CenterOfWindow,
                    Target::Widget(editor.view_id),
                ));
            } else {
                editor.ensure_cursor_visible(
                    ctx,
                    buffer,
                    env,
                    Some(EnsureVisiblePosition::CenterOfWindow),
                );
            }
        }
        editor.update_ui_state(ui_state, buffer);
        self.notify_fill_text_layouts(ctx, &buffer_id);
        None
    }

    pub fn go_to_definition(
        &mut self,
        request_id: usize,
        value: Value,
    ) -> Option<()> {
        let editor = self.editors.get(&self.active)?;
        let offset = editor.selection.get_cursor_offset();
        if offset != request_id {
            return None;
        }

        let resp: Result<GotoDefinitionResponse, serde_json::Error> =
            serde_json::from_value(value);
        let resp = resp.ok()?;

        if let Some(location) = match resp {
            GotoDefinitionResponse::Scalar(location) => Some(location),
            GotoDefinitionResponse::Array(locations) => {
                if locations.len() > 0 {
                    Some(locations[0].clone())
                } else {
                    None
                }
            }
            GotoDefinitionResponse::Link(location_links) => None,
        } {
            LAPCE_APP_STATE.submit_ui_command(
                LapceUICommand::GotoLocation(location),
                self.widget_id,
            );
        }
        None
    }

    pub fn show_completion(
        &mut self,
        request_id: usize,
        value: Value,
    ) -> Option<()> {
        let resp: Result<CompletionResponse, serde_json::Error> =
            serde_json::from_value(value);
        if let Ok(resp) = resp {
            let items = match resp {
                CompletionResponse::Array(items) => items,
                CompletionResponse::List(list) => list.items,
            };
            let editor = self.editors.get(&self.active)?;
            let buffer_id = editor.buffer_id?;
            let buffer = self.buffers.get(&buffer_id)?;
            let offset = editor.selection.get_cursor_offset();
            let prev_offset = buffer.prev_code_boundary(offset);
            if request_id != prev_offset {
                return None;
            }

            let input = buffer.slice_to_cow(prev_offset..offset).to_string();
            self.completion.update(input, items);
            LAPCE_APP_STATE.submit_ui_command(
                LapceUICommand::RequestLayout,
                self.completion.widget_id,
            );
        }
        None
    }

    pub fn request_layout(&self) {
        LAPCE_APP_STATE.submit_ui_command(
            LapceUICommand::RequestLayout,
            self.widget_id.clone(),
        );
    }

    pub fn apply_edits(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        rev: u64,
        edits: &Vec<TextEdit>,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
        if buffer.rev != rev {
            return None;
        }
        let edits: Vec<(Selection, String)> = edits
            .iter()
            .map(|edit| {
                let selection = Selection::region(
                    buffer.offset_of_position(&edit.range.start).unwrap(),
                    buffer.offset_of_position(&edit.range.end).unwrap(),
                );
                (selection, edit.new_text.clone())
            })
            .collect();
        buffer.edit_multiple(
            ctx,
            buffer_ui_state,
            edits.iter().map(|(s, c)| (s, c.as_ref())).collect(),
            true,
        );
        self.notify_fill_text_layouts(ctx, &buffer_id);
        None
    }

    pub fn check_diagnositics(&self, ctx: &mut EventCtx) -> Option<()> {
        let editor = self.editors.get(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get(&buffer_id)?;
        let diagnositics = self.diagnostics.get(&buffer.path)?;
        let offset = editor.selection.get_cursor_offset();
        for diagnostic in diagnositics {
            if let Some(diagnostic_offset) =
                buffer.offset_of_position(&diagnostic.range.start)
            {
                if offset == diagnostic_offset {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::RequestPaint,
                        Target::Widget(editor.view_id),
                    ));
                    return None;
                }
            }
        }
        None
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        count: Option<usize>,
        cmd: LapceCommand,
        env: &Env,
    ) -> Option<()> {
        let operator = self.operator.take();
        //let buffer_id = self.editors.get(&self.active)?.buffer_id.clone()?;
        //let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
        match cmd {
            LapceCommand::InsertMode => {
                self.mode = Mode::Insert;
            }
            LapceCommand::InsertFirstNonBlank => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                editor.insert_mode(
                    buffer,
                    &self.mode,
                    &self.visual_mode,
                    ColPosition::FirstNonBlank,
                );
                self.mode = Mode::Insert;
            }
            LapceCommand::ListSelect => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.clone()?;
                let buffer = self.buffers.get_mut(&buffer_id)?;
                let offset = editor.selection.get_cursor_offset();
                let prev_offset = buffer.prev_code_boundary(offset);
                let next_offset = buffer.next_code_boundary(offset);
                let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
                let selection = Selection::region(prev_offset, next_offset);
                let delta = buffer.edit(
                    ctx,
                    buffer_ui_state,
                    &self.completion.current_items()[self.completion.index]
                        .item
                        .label,
                    &selection,
                    true,
                );
                editor.selection_apply_delta(&delta);
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                self.inactive_editor_apply_delta(&delta);
                self.completion.cancel(ctx);
            }
            LapceCommand::ListNext => {
                self.completion.index = Movement::Down.update_index(
                    self.completion.index,
                    self.completion.len(),
                    1,
                    true,
                );
                self.completion.request_paint(ctx);
            }
            LapceCommand::ListPrevious => {
                self.completion.index = Movement::Up.update_index(
                    self.completion.index,
                    self.completion.len(),
                    1,
                    true,
                );
                self.completion.request_paint(ctx);
            }
            LapceCommand::NormalMode => {
                self.completion.cancel(ctx);
                self.inserting = false;
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let old_mode = self.mode.clone();
                self.mode = Mode::Normal;
                editor.selection = editor.selection.to_caret();
                if old_mode == Mode::Insert {
                    editor.selection = Movement::Left.update_selection(
                        &editor.selection,
                        buffer,
                        1,
                        false,
                        false,
                    );
                }
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
            LapceCommand::Append => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                self.mode = Mode::Insert;
                editor.selection = Movement::Right.update_selection(
                    &editor.selection,
                    buffer,
                    1,
                    true,
                    false,
                );
            }
            LapceCommand::AppendEndOfLine => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                self.mode = Mode::Insert;
                editor.selection = Movement::EndOfLine.update_selection(
                    &editor.selection,
                    buffer,
                    1,
                    true,
                    false,
                );
            }
            LapceCommand::NewLineAbove => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let line =
                    buffer.line_of_offset(editor.selection.get_cursor_offset());
                let offset = buffer.first_non_blank_character_on_line(line);
                self.insert_new_line(ctx, ui_state, offset, true, env);

                let editor = self.editors.get_mut(&self.active)?;
                editor.selection = Selection::caret(offset);
                self.mode = Mode::Insert;
                self.inserting = true;
            }

            LapceCommand::NewLineBelow => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                self.mode = Mode::Insert;
                let offset = buffer
                    .line_end_offset(editor.selection.get_cursor_offset(), true);
                self.insert_new_line(ctx, ui_state, offset, true, env);
                self.inserting = true;
                // editor.insert_new_line(ctx, buffer, offset, env);
            }
            LapceCommand::InsertNewLine => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                if editor.selection.regions().len() == 1 {
                    let offset = editor.selection.get_cursor_offset();
                    self.insert_new_line(ctx, ui_state, offset, false, env);
                } else {
                    let delta = buffer.edit(
                        ctx,
                        buffer_ui_state,
                        "\n",
                        &editor.selection,
                        false,
                    );
                    editor.selection_apply_delta(&delta);
                    editor.ensure_cursor_visible(ctx, buffer, env, None);
                    self.inactive_editor_apply_delta(&delta);
                }
                if self.mode == Mode::Insert {
                    self.inserting = true;
                }
            }
            LapceCommand::DeleteWordBackward => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                let offset = editor.selection.get_cursor_offset();
                let new_offset = buffer.word_backword(offset);
                buffer.edit(
                    ctx,
                    buffer_ui_state,
                    "",
                    &Selection::region(new_offset, offset),
                    self.mode != Mode::Insert,
                );
                editor.selection = Selection::caret(new_offset);
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                if self.mode == Mode::Insert {
                    self.inserting = true;
                    self.update_completion(ctx);
                }
                // editor.request_paint();
            }
            LapceCommand::DeleteBackward => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                editor.delete(
                    ctx,
                    buffer_ui_state,
                    &self.mode,
                    &self.visual_mode,
                    buffer,
                    Movement::Left,
                    count,
                );
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                }
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                if self.mode == Mode::Insert {
                    self.inserting = true;
                    self.update_completion(ctx);
                }
            }
            LapceCommand::DeleteForeward => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);

                editor.delete(
                    ctx,
                    buffer_ui_state,
                    &self.mode,
                    &self.visual_mode,
                    buffer,
                    Movement::Right,
                    count,
                );
                if self.mode == Mode::Normal || self.mode == Mode::Visual {
                    editor.selection =
                        buffer.correct_offset(&editor.selection.collapse());
                }
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                }
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                if self.mode == Mode::Insert {
                    self.inserting = true;
                }
            }
            LapceCommand::DeleteForewardAndInsert => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                editor.delete(
                    ctx,
                    buffer_ui_state,
                    &self.mode,
                    &self.visual_mode,
                    buffer,
                    Movement::Right,
                    count,
                );
                self.mode = Mode::Insert;
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                self.inserting = true;
            }
            LapceCommand::DeleteOperator => {
                self.operator = Some(EditorOperator::Delete(EditorCount(count)));
            }
            LapceCommand::Paste => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                if let Some(content) = self.register.get("x") {
                    editor.paste(
                        ctx,
                        buffer_ui_state,
                        &self.mode,
                        &self.visual_mode,
                        buffer,
                        content,
                        env,
                    );
                }
                self.mode = Mode::Normal;
            }
            LapceCommand::DeleteVisual => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                let content = buffer.yank(&editor.get_selection(
                    buffer,
                    &self.mode,
                    &self.visual_mode,
                    false,
                ));
                self.register.insert(
                    "x".to_string(),
                    RegisterContent {
                        kind: self.visual_mode.clone(),
                        content,
                    },
                );
                let selection = editor.get_selection(
                    buffer,
                    &self.mode,
                    &self.visual_mode,
                    false,
                );
                let delta = buffer.edit(ctx, buffer_ui_state, "", &selection, true);
                editor.selection = buffer.correct_offset(
                    &selection
                        .apply_delta(&delta, true, InsertDrift::Default)
                        .collapse(),
                );
                self.mode = Mode::Normal;
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                self.mode = Mode::Normal;
            }
            LapceCommand::Yank => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let content = buffer.yank(&editor.get_selection(
                    buffer,
                    &self.mode,
                    &self.visual_mode,
                    false,
                ));
                self.register.insert(
                    "x".to_string(),
                    RegisterContent {
                        kind: self.visual_mode.clone(),
                        content,
                    },
                );
                editor.selection = editor.selection.min();
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                editor.request_paint();
                self.mode = Mode::Normal;
            }
            LapceCommand::SplitVertical => {
                let editor = self.editors.get_mut(&self.active)?;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Split(true),
                    Target::Widget(editor.split_id),
                ));
            }
            LapceCommand::NewTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewTab,
                    Target::Global,
                ));
            }
            LapceCommand::NextTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NextTab,
                    Target::Global,
                ));
            }
            LapceCommand::PreviousTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PreviousTab,
                    Target::Global,
                ));
            }
            LapceCommand::Undo => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                if let Some(offset) = buffer.undo(ctx, buffer_ui_state) {
                    editor.selection = Selection::caret(offset);
                    editor.ensure_cursor_visible(
                        ctx,
                        buffer,
                        env,
                        Some(EnsureVisiblePosition::CenterOfWindow),
                    );
                }
            }
            LapceCommand::Redo => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                if let Some(offset) = buffer.redo(ctx, buffer_ui_state) {
                    editor.selection = Selection::caret(offset);
                    editor.ensure_cursor_visible(
                        ctx,
                        buffer,
                        env,
                        Some(EnsureVisiblePosition::CenterOfWindow),
                    );
                }
            }
            LapceCommand::GetCompletion => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                let offset = editor.selection.get_cursor_offset();
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .plugins
                    .lock()
                    .get_completion(&buffer_id, 0, offset);
            }
            LapceCommand::GotoDefinition => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let offset = editor.selection.get_cursor_offset();
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .lsp
                    .lock()
                    .go_to_definition(
                        offset,
                        buffer,
                        buffer.offset_to_position(offset),
                    );
            }
            LapceCommand::JumpLocationBackward => {
                self.jump_location_backward(ctx, ui_state, env);
            }
            LapceCommand::JumpLocationForward => {
                self.jump_location_forward(ctx, ui_state, env);
            }
            LapceCommand::NextError => {
                self.next_error(ctx, ui_state, env);
            }
            LapceCommand::PreviousError => {}
            LapceCommand::DocumentFormatting => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer = self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .lsp
                    .lock()
                    .request_document_formatting(buffer);
            }
            LapceCommand::Save => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.clone()?;
                let buffer = self.buffers.get_mut(&buffer_id)?;
                let rev = buffer.rev;

                if let Some(edits) = {
                    let state =
                        LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                    let lsp = state.lsp.lock();
                    let edits = lsp.get_document_formatting(buffer);
                    edits
                } {
                    if edits.len() > 0 {
                        self.apply_edits(ctx, ui_state, rev, &edits);
                    }
                }

                let buffer_ui_state = ui_state.get_buffer_mut(&buffer_id);
                let buffer = self.buffers.get_mut(&buffer_id)?;
                if let Err(e) = buffer.save() {
                    println!("buffer save error {}", e);
                }
                buffer_ui_state.dirty = buffer.dirty;
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .lsp
                    .lock()
                    .save_buffer(buffer);
            }
            _ => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer_id = editor.buffer_id.as_ref()?;
                let buffer = self.buffers.get_mut(buffer_id)?;
                let buffer_ui_state = ui_state.get_buffer_mut(buffer_id);
                editor.run_command(
                    ctx,
                    buffer_ui_state,
                    self.mode.clone(),
                    count,
                    buffer,
                    cmd,
                    operator,
                    env,
                );
            }
        }
        let buffer_id = self
            .editors
            .get_mut(&self.active)?
            .buffer_id
            .as_ref()?
            .clone();
        let editor = self.editors.get_mut(&self.active)?;
        let buffer = self.buffers.get(editor.buffer_id.as_ref()?)?;
        editor.update_ui_state(ui_state, buffer);
        let editor_ui_state = ui_state.get_editor_mut(&self.active);
        editor_ui_state.visual_mode = self.visual_mode.clone();
        editor_ui_state.mode = self.mode.clone();
        self.notify_fill_text_layouts(ctx, &buffer_id);
        self.check_diagnositics(ctx);
        None
    }

    pub fn buffer_request_layout(&self, buffer_id: &BufferId) {
        for (_, editor) in &self.editors {
            if let Some(b) = &editor.buffer_id {
                if b == buffer_id {
                    editor.request_layout();
                }
            }
        }
    }

    pub fn get_cursor(&self, view_id: &WidgetId) -> Option<(usize, usize)> {
        if &self.active != view_id {
            return None;
        }

        let editor = self.editors.get(view_id)?;
        let offset = editor.selection.get_cursor_offset();
        let buffer = self.buffers.get(editor.buffer_id.as_ref()?)?;
        Some(buffer.offset_to_line_col(offset))
    }

    pub fn get_mode(&self) -> Mode {
        self.mode.clone()
    }

    pub fn request_paint(&self) {}
}

pub struct EditorHeader {
    window_id: WindowId,
    tab_id: WidgetId,
    view_id: WidgetId,
}

impl EditorHeader {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        view_id: WidgetId,
    ) -> EditorHeader {
        EditorHeader {
            window_id,
            tab_id,
            view_id,
        }
    }
}

impl Widget<LapceUIState> for EditorHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        let editor = data.get_editor(&self.view_id);
        let old_editor = old_data.get_editor(&self.view_id);
        let cursor = editor.cursor;
        let old_cursor = old_editor.cursor;

        if cursor.0 != old_cursor.0 {
            ctx.request_paint();
            return;
        }

        if editor.buffer_id != old_editor.buffer_id {
            ctx.request_paint();
            return;
        }

        if let Some(buffer) = data.buffers.get(&editor.buffer_id) {
            if let Some(old_buffer) = old_data.buffers.get(&editor.buffer_id) {
                if buffer.dirty != old_buffer.dirty {
                    ctx.request_paint();
                    return;
                }
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        Size::new(bc.max().width, line_height + 10.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        let blur_color = Color::grey8(100);
        let shadow_width = 5.0;
        let shift = 2.0;
        ctx.blurred_rect(
            rect - Insets::new(shift, shadow_width, shift, shadow_width),
            shadow_width,
            &blur_color,
        );
        ctx.fill(
            rect - Insets::new(0.0, 0.0, 0.0, shadow_width),
            &env.get(LapceTheme::EDITOR_BACKGROUND),
        );

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let editor = editor_split.editors.get(&self.view_id).unwrap();
        if let Some(buffer_id) = editor.buffer_id.as_ref() {
            let buffer = editor_split.buffers.get(buffer_id).unwrap();
            let path = PathBuf::from_str(&buffer.path).unwrap();
            let file_name = format!(
                "{}{}",
                if buffer.dirty { "*" } else { "" },
                path.file_name().unwrap().to_str().unwrap().to_string()
            );
            let mut x = 10.0;

            let mut text_layout = TextLayout::new(file_name.clone());
            text_layout.set_font(
                FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0),
            );
            text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
            text_layout.rebuild_if_needed(ctx.text(), env);
            text_layout.draw(ctx, Point::new(10.0, 5.0));
            x += text_layout.size().width;

            let cwd = PathBuf::from_str("./").unwrap().canonicalize().unwrap();
            let dir = if let Ok(dir) = path.strip_prefix(cwd) {
                dir
            } else {
                path.as_path()
            };
            let dir = dir.to_str().unwrap().to_string();
            let mut text_layout = TextLayout::new(dir);
            text_layout.set_font(
                FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(13.0),
            );
            text_layout.set_text_color(LapceTheme::EDITOR_COMMENT);
            text_layout.rebuild_if_needed(ctx.text(), env);
            text_layout.draw(ctx, Point::new(5.0 + x, 5.0));
        }
    }
}

pub struct EditorView {
    window_id: WindowId,
    tab_id: WidgetId,
    split_id: WidgetId,
    view_id: WidgetId,
    pub editor_id: WidgetId,
    pub editor:
        WidgetPod<LapceUIState, LapceScroll<LapceUIState, Padding<LapceUIState>>>,
    gutter: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    header: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
}

impl EditorView {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        split_id: WidgetId,
        view_id: WidgetId,
        editor_id: WidgetId,
    ) -> EditorView {
        let editor = IdentityWrapper::wrap(
            Editor::new(window_id, tab_id.clone(), view_id),
            editor_id.clone(),
        );
        let scroll = LapceScroll::new(editor.padding((10.0, 0.0, 10.0, 0.0)));
        EditorView {
            window_id,
            tab_id,
            split_id,
            view_id,
            editor_id,
            editor: WidgetPod::new(scroll),
            gutter: WidgetPod::new(
                EditorGutter::new(window_id, tab_id, view_id)
                    .padding((10.0, 0.0, 10.0, 0.0)),
            )
            .boxed(),
            header: WidgetPod::new(EditorHeader::new(window_id, tab_id, view_id))
                .boxed(),
        }
    }

    pub fn center_of_window(&mut self, ctx: &mut EventCtx, env: &Env) {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let mut editor_split = state.editor_split.lock();
        let editor_state = editor_split.editors.get_mut(&self.view_id).unwrap();
        let buffer_id = editor_state.buffer_id.as_ref().unwrap().clone();
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let offset = editor_state.selection.get_cursor_offset();
        let buffer = editor_split.buffers.get(&buffer_id).unwrap();
        let line = buffer.line_of_offset(offset);
        let y = if line as f64 * line_height > ctx.size().height / 2.0 {
            line as f64 * line_height - ctx.size().height / 2.0
        } else {
            0.0
        };
        let scroll = self.editor.widget_mut();
        scroll.scroll_to(0.0, y);
        let editor_state = editor_split.editors.get_mut(&self.view_id).unwrap();
        editor_state.scroll_offset = scroll.offset();
        ctx.request_paint();
    }
}

impl Widget<LapceUIState> for EditorView {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                self.gutter.event(ctx, event, data, env);
                self.editor.event(ctx, event, data, env);
            }
            Event::Wheel(_) => {
                self.editor.event(ctx, event, data, env);
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .fill_text_layouts(
                        ctx,
                        data,
                        self.editor.widget().offset(),
                        &self.view_id,
                        env,
                    );
                ctx.request_paint();
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        LapceUICommand::FillTextLayouts => {
                            LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id)
                                .editor_split
                                .lock()
                                .fill_text_layouts(
                                    ctx,
                                    data,
                                    self.editor.widget().offset(),
                                    &self.view_id,
                                    env,
                                );
                        }
                        LapceUICommand::CenterOfWindow => {
                            self.center_of_window(ctx, env);
                        }
                        LapceUICommand::EnsureVisible((rect, margin, position)) => {
                            let scroll_size = {
                                let state = LAPCE_APP_STATE
                                    .get_tab_state(&self.window_id, &self.tab_id);
                                let editor_split = state.editor_split.lock();
                                let editor =
                                    editor_split.editors.get(&self.view_id).unwrap();
                                let size = editor.scroll_size.clone();
                                size
                            };
                            let editor = self.editor.widget_mut();
                            if editor.ensure_visible(scroll_size, rect, margin) {
                                match position {
                                    Some(EnsureVisiblePosition::CenterOfWindow) => {
                                        self.center_of_window(ctx, env);
                                    }
                                    None => {
                                        let state = LAPCE_APP_STATE.get_tab_state(
                                            &self.window_id,
                                            &self.tab_id,
                                        );
                                        let mut editor_split =
                                            state.editor_split.lock();
                                        let offset = editor.offset();
                                        editor_split
                                            .editors
                                            .get_mut(&self.view_id)
                                            .unwrap()
                                            .scroll_offset = offset;
                                        self.gutter.set_viewport_offset(Vec2::new(
                                            0.0, offset.y,
                                        ));
                                    }
                                }
                                ctx.request_paint();
                            }
                        }
                        LapceUICommand::ScrollTo((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll_to(*x, *y);
                            LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id)
                                .editor_split
                                .lock()
                                .editors
                                .get_mut(&self.view_id)
                                .unwrap()
                                .scroll_offset = scroll.offset();
                            ctx.request_paint();
                        }
                        LapceUICommand::Scroll((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll(*x, *y);
                            LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id)
                                .editor_split
                                .lock()
                                .editors
                                .get_mut(&self.view_id)
                                .unwrap()
                                .scroll_offset = scroll.offset();
                            ctx.request_paint();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => self.editor.event(ctx, event, data, env),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        match event {
            LifeCycle::Size(size) => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .editors
                    .get_mut(&self.view_id)
                    .unwrap()
                    .view_size = *size;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(self.view_id.clone()),
                ));
            }
            _ => (),
        }
        self.header.lifecycle(ctx, event, data, env);
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        self.editor.update(ctx, data, env);
        self.gutter.update(ctx, data, env);
        // self.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let header_size = self.header.layout(ctx, bc, data, env);
        {
            let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
            let mut editor_split = state.editor_split.lock();
            let editor = editor_split.editors.get_mut(&self.view_id).unwrap();
            editor.header_height = header_size.height;
        }
        self.header.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(header_size),
        );
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        {
            let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
            let mut editor_split = state.editor_split.lock();
            let editor = editor_split.editors.get_mut(&self.view_id).unwrap();
            editor.gutter_width = gutter_size.width;
        }
        self.gutter.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO
                .with_size(gutter_size)
                .with_origin(Point::new(0.0, header_size.height)),
        );
        let editor_size = Size::new(
            self_size.width - gutter_size.width,
            self_size.height - header_size.height,
        );
        {
            let editor_split = LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .editor_split;
            let mut editor_split = editor_split.lock();
            let editor = editor_split.editors.get_mut(&self.view_id).unwrap();
            editor.scroll_size = editor_size.clone();
        }
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO
                .with_origin(Point::new(gutter_size.width, header_size.height))
                .with_size(editor_size),
        );
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let viewport = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            let scroll_offset = self.editor.widget().offset();
            ctx.clip(viewport);
            ctx.transform(Affine::translate(-scroll_offset));

            let mut visible = ctx.region().clone();
            visible += scroll_offset;
            ctx.with_child_ctx(visible, |ctx| {
                self.gutter.paint(ctx, data, env);
            })
        });
        self.editor.paint(ctx, data, env);
        self.header.paint(ctx, data, env);
    }

    fn id(&self) -> Option<WidgetId> {
        Some(self.view_id)
    }
}

pub struct EditorGutter {
    window_id: WindowId,
    tab_id: WidgetId,
    view_id: WidgetId,
    text_layouts: HashMap<String, EditorTextLayout>,
}

impl EditorGutter {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        view_id: WidgetId,
    ) -> EditorGutter {
        EditorGutter {
            window_id,
            tab_id,
            view_id,
            text_layouts: HashMap::new(),
        }
    }
}

impl Widget<LapceUIState> for EditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        let cursor = data.get_editor(&self.view_id).cursor;
        let old_cursor = old_data.get_editor(&self.view_id).cursor;

        if cursor.0 != old_cursor.0 {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        if let Some(buffer_id) = editor_split
            .editors
            .get(&self.view_id)
            .as_ref()
            .unwrap()
            .buffer_id
            .clone()
        {
            let buffer = editor_split.buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            let gutter_width = width * buffer.last_line().to_string().len() as f64;
            let gutter_height = 25.0 * buffer.num_lines() as f64;
            Size::new(gutter_width, gutter_height)
        } else {
            Size::new(50.0, 50.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let buffer_id = editor_split
            .editors
            .get(&self.view_id)
            .as_ref()
            .unwrap()
            .buffer_id
            .as_ref();
        if buffer_id.is_none() {
            return;
        }

        let mut layout = TextLayout::new("W");
        layout.set_font(LapceTheme::EDITOR_FONT);
        layout.rebuild_if_needed(&mut ctx.text(), env);
        let width = layout.point_for_text_position(1).x;

        let buffer = editor_split.buffers.get(buffer_id.unwrap()).unwrap();
        let last_line = buffer.last_line();
        let rects = ctx.region().rects().to_vec();
        let active = editor_split.active;
        let editor = editor_split.editors.get(&self.view_id).unwrap();
        let (current_line, _) =
            buffer.offset_to_line_col(editor.selection.get_cursor_offset());
        for rect in rects {
            let start_line = (rect.y0 / line_height).floor() as usize;
            let num_lines = (rect.height() / line_height).floor() as usize;
            for line in start_line..start_line + num_lines + 1 {
                if line > last_line {
                    break;
                }
                let content = if active != self.view_id {
                    line
                } else {
                    if line == current_line {
                        line
                    } else if line > current_line {
                        line - current_line
                    } else {
                        current_line - line
                    }
                };
                let x = (last_line.to_string().len() - content.to_string().len())
                    as f64
                    * width;
                let content = content.to_string();
                if let Some(text_layout) = self.text_layouts.get_mut(&content) {
                    if text_layout.text != content {
                        text_layout.layout.set_text(content.clone());
                        text_layout.text = content;
                        text_layout.layout.rebuild_if_needed(&mut ctx.text(), env);
                    }
                    text_layout
                        .layout
                        .draw(ctx, Point::new(x, line_height * line as f64));
                } else {
                    let mut layout = TextLayout::new(content.clone());
                    layout.set_font(LapceTheme::EDITOR_FONT);
                    layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
                    layout.rebuild_if_needed(&mut ctx.text(), env);
                    layout.draw(ctx, Point::new(x, line_height * line as f64));
                    let text_layout = EditorTextLayout {
                        layout,
                        text: content.clone(),
                    };
                    self.text_layouts.insert(content, text_layout);
                }
            }
        }
    }
}

struct EditorTextLayout {
    layout: TextLayout,
    text: String,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

pub struct Editor {
    window_id: WindowId,
    tab_id: WidgetId,
    view_id: WidgetId,
    view_size: Size,
}

impl Editor {
    pub fn new(window_id: WindowId, tab_id: WidgetId, view_id: WidgetId) -> Self {
        Editor {
            window_id,
            tab_id,
            view_id,
            view_size: Size::ZERO,
        }
    }

    fn paint_insert_cusor(
        &mut self,
        ctx: &mut PaintCtx,
        selection: &Selection,
        buffer: &Buffer,
        line_height: f64,
        width: f64,
        start_line: usize,
        number_lines: usize,
        env: &Env,
    ) {
        let start = buffer.offset_of_line(start_line);
        let last_line = buffer.last_line();
        let mut end_line = start_line + number_lines;
        if end_line > last_line {
            end_line = last_line;
        }
        let end = buffer.offset_of_line(end_line + 1);
        let regions = selection.regions_in_range(start, end);
        for region in regions {
            let (line, col) = buffer.offset_to_line_col(region.min());
            let line_content = buffer
                .slice_to_cow(
                    buffer.offset_of_line(line)..buffer.offset_of_line(line + 1),
                )
                .to_string();
            let x = (line_content[..col]
                .chars()
                .filter_map(|c| if c == '\t' { Some('\t') } else { None })
                .count()
                * 3
                + col) as f64
                * width;
            let y = line as f64 * line_height;
            ctx.stroke(
                Line::new(Point::new(x, y), Point::new(x, y + line_height)),
                &env.get(LapceTheme::EDITOR_CURSOR_COLOR),
                2.0,
            )
        }
    }

    fn paint_selection(
        &mut self,
        ctx: &mut PaintCtx,
        mode: &Mode,
        visual_mode: &VisualMode,
        selection: &Selection,
        buffer: &Buffer,
        line_height: f64,
        width: f64,
        start_line: usize,
        number_lines: usize,
        env: &Env,
    ) {
        match mode {
            &Mode::Visual => (),
            _ => return,
        }
        let start = buffer.offset_of_line(start_line);
        let last_line = buffer.last_line();
        let mut end_line = start_line + number_lines;
        if end_line > last_line {
            end_line = last_line;
        }
        let end = buffer.offset_of_line(end_line + 1);

        let regions = selection.regions_in_range(start, end);
        for region in regions {
            let (start_line, start_col) = buffer.offset_to_line_col(region.min());
            let (end_line, end_col) = buffer.offset_to_line_col(region.max());

            for line in start_line..end_line + 1 {
                let line_content = buffer
                    .slice_to_cow(
                        buffer.offset_of_line(line)..buffer.offset_of_line(line + 1),
                    )
                    .to_string();

                let left_col = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    },
                    &VisualMode::Linewise => 0,
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false);
                        let left = start_col.min(end_col);
                        if left > max_col {
                            continue;
                        }
                        left
                    }
                };
                let x0 = (left_col
                    + &line_content[..left_col].matches('\t').count() * 3)
                    as f64
                    * width;

                let right_col = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == end_line => end_col + 1,
                        _ => {
                            buffer.offset_of_line(line + 1)
                                - buffer.offset_of_line(line)
                        }
                    },
                    &VisualMode::Linewise => {
                        buffer.offset_of_line(line + 1) - buffer.offset_of_line(line)
                    }
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false) + 1;
                        let right = match region.horiz() {
                            Some(&ColPosition::End) => max_col,
                            _ => (end_col.max(start_col) + 1).min(max_col),
                        };
                        right
                    }
                };
                let x1 = (right_col
                    + &line_content[..right_col].matches('\t').count() * 3)
                    as f64
                    * width;

                let y0 = line as f64 * line_height;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    &env.get(LapceTheme::EDITOR_SELECTION_COLOR),
                );
            }
        }
    }
}

impl Widget<LapceUIState> for Editor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            println!("editor request layout");
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            println!("editor request paint");
                            ctx.request_paint();
                        }
                        LapceUICommand::RequestPaintRect(rect) => {
                            ctx.request_paint_rect(*rect);
                        }
                        _ => println!("editor unprocessed ui command {:?}", command),
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceUIState,
        old_data: &LapceUIState,
        env: &Env,
    ) {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let editor = editor_split.editors.get(&self.view_id).unwrap();
        editor.update(ctx, data, old_data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        self.view_size = bc.min();
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        if let Some(buffer_id) = editor_split.get_buffer_id(&self.view_id) {
            let buffer = data.get_buffer(&buffer_id);
            let width = 7.6171875;
            Size::new(
                (width * buffer.max_len as f64).max(bc.min().width),
                25.0 * buffer.text_layouts.len() as f64 + bc.min().height
                    - line_height,
            )
        } else {
            Size::new(0.0, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let buffer_id = editor_split.get_buffer_id(&self.view_id);
        if buffer_id.is_none() {
            return;
        }
        let buffer_id = buffer_id.unwrap();
        let size = ctx.size();

        let mut layout = TextLayout::new("W");
        layout.set_font(LapceTheme::EDITOR_FONT);
        layout.rebuild_if_needed(&mut ctx.text(), env);
        let width = layout.point_for_text_position(1).x;

        let buffer = editor_split.buffers.get(&buffer_id).unwrap();
        let editor = editor_split.editors.get(&self.view_id).unwrap();
        let editor_offset = editor.selection.get_cursor_offset();
        let cursor = buffer.offset_to_line_col(editor_offset);

        let mode = editor_split.mode.clone();
        let visual_mode = editor_split.visual_mode.clone();

        let rects = ctx.region().rects().to_vec();
        for rect in rects {
            let start_line = (rect.y0 / line_height).floor() as usize;
            let num_lines = (rect.height() / line_height).floor() as usize;
            if mode == Mode::Visual {
                self.paint_selection(
                    ctx,
                    &mode,
                    &visual_mode,
                    &editor.selection,
                    buffer,
                    line_height,
                    width,
                    start_line,
                    num_lines,
                    env,
                );
            }
            let last_line = buffer.last_line();
            for line in start_line..start_line + num_lines + 1 {
                if line > last_line {
                    break;
                }

                if line == cursor.0 {
                    match mode {
                        Mode::Visual => (),
                        _ => {
                            ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(
                                        0.0,
                                        cursor.0 as f64 * line_height,
                                    ))
                                    .with_size(Size::new(size.width, line_height)),
                                &env.get(LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND),
                            );
                        }
                    };

                    let line_content = buffer
                        .slice_to_cow(
                            buffer.offset_of_line(line)
                                ..buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    if editor_split.active == self.view_id {
                        let cursor_x =
                            (line_content[..cursor.1]
                                .chars()
                                .filter_map(|c| {
                                    if c == '\t' {
                                        Some('\t')
                                    } else {
                                        None
                                    }
                                })
                                .count()
                                * 3
                                + cursor.1) as f64
                                * width;
                        match mode {
                            Mode::Insert => self.paint_insert_cusor(
                                ctx,
                                &editor.selection,
                                buffer,
                                line_height,
                                width,
                                start_line,
                                num_lines,
                                env,
                            ),
                            _ => ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(
                                        cursor_x,
                                        cursor.0 as f64 * line_height,
                                    ))
                                    .with_size(Size::new(width, line_height)),
                                &env.get(LapceTheme::EDITOR_CURSOR_COLOR),
                            ),
                        };
                    }
                }
                let buffer_ui = data.buffers.get(&buffer_id).unwrap();
                if buffer_ui.text_layouts.len() > line {
                    if let Some(layout) = buffer_ui.text_layouts[line].as_ref() {
                        ctx.draw_text(
                            &layout.layout,
                            Point::new(0.0, line_height * line as f64 + 5.0),
                        );
                    }
                }
            }
            if let Some(diagnostics) = editor_split.diagnostics.get(&buffer.path) {
                for diagnositic in diagnostics {
                    if let Some(severity) = diagnositic.severity {
                        let color = match severity {
                            DiagnosticSeverity::Error => {
                                env.get(LapceTheme::EDITOR_ERROR)
                            }
                            DiagnosticSeverity::Warning => {
                                env.get(LapceTheme::EDITOR_WARN)
                            }
                            _ => env.get(LapceTheme::EDITOR_WARN),
                        };
                        let start = diagnositic.range.start;
                        let end = diagnositic.range.end;
                        if (start.line as usize) < start_line + num_lines
                            || (end.line as usize) > start_line
                        {
                            for line in start.line as usize..end.line as usize + 1 {
                                if line > last_line {
                                    break;
                                }
                                let x0 = if line == start.line as usize {
                                    start.character as f64 * width
                                } else {
                                    buffer.first_non_blank_character_on_line(line)
                                        as f64
                                        * width
                                };
                                let x1 = if line == end.line as usize {
                                    end.character as f64 * width
                                } else {
                                    buffer.line_len(line) as f64 * width
                                };
                                let y1 = (line + 1) as f64 * line_height;
                                let y0 = (line + 1) as f64 * line_height - 2.0;
                                ctx.fill(Rect::new(x0, y0, x1, y1), &color);
                            }
                            if Some(editor.selection.get_cursor_offset())
                                == buffer.offset_of_position(&start)
                            {
                                let mut text_layout =
                                    TextLayout::new(diagnositic.message.clone());
                                text_layout.set_font(
                                    FontDescriptor::new(FontFamily::SYSTEM_UI)
                                        .with_size(14.0),
                                );
                                text_layout
                                    .set_text_color(LapceTheme::EDITOR_FOREGROUND);
                                text_layout.rebuild_if_needed(ctx.text(), env);
                                let text_size = text_layout.size();
                                let rect = Rect::ZERO
                                    .with_origin(Point::new(
                                        0.0,
                                        (start.line + 1) as f64 * line_height,
                                    ))
                                    .with_size(Size::new(
                                        size.width,
                                        text_size.height + 20.0,
                                    ));
                                ctx.fill(
                                    rect,
                                    &env.get(LapceTheme::EDITOR_SELECTION_COLOR),
                                );
                                ctx.stroke(rect, &color, 1.0);
                                text_layout.draw(
                                    ctx,
                                    Point::new(
                                        10.0,
                                        (start.line + 1) as f64 * line_height + 10.0,
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn next_in_file_errors_offset(
    offset: usize,
    path: &String,
    file_diagnostics: &Vec<(&String, Vec<usize>)>,
) -> (String, usize) {
    for (current_path, offsets) in file_diagnostics {
        if &path == current_path {
            for error_offset in offsets {
                if *error_offset > offset {
                    return ((*current_path).clone(), *error_offset);
                }
            }
        }
        if current_path > &path {
            return ((*current_path).clone(), offsets[0]);
        }
    }
    ((*file_diagnostics[0].0).clone(), file_diagnostics[0].1[0])
}
