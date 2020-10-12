use crate::{
    buffer::{Buffer, BufferId, BufferUIState, InvalLines},
    command::EnsureVisiblePosition,
    command::LapceCommand,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    container::LapceContainer,
    movement::ColPosition,
    movement::LinePosition,
    movement::Movement,
    movement::SelRegion,
    movement::Selection,
    scroll::LapceScroll,
    split::SplitMoveDirection,
    state::LapceState,
    state::Mode,
    state::VisualMode,
    theme::LapceTheme,
};
use anyhow::{anyhow, Result};
use druid::{
    kurbo::Line, piet::PietText, widget::IdentityWrapper, widget::Padding,
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx,
    KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use druid::{
    piet::{PietTextLayout, Text, TextAttribute, TextLayoutBuilder},
    FontWeight,
};
use std::iter::Iterator;
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
pub struct EditorState {
    pub editor_id: WidgetId,
    pub view_id: WidgetId,
    pub split_id: WidgetId,
    pub buffer_id: Option<BufferId>,
    pub selection: Selection,
    pub char_width: f64,
    pub width: f64,
    pub height: f64,
    pub scroll_offset: Vec2,
    pub view_size: Size,
}

impl EditorState {
    pub fn new(split_id: WidgetId, buffer_id: Option<BufferId>) -> EditorState {
        EditorState {
            editor_id: WidgetId::next(),
            view_id: WidgetId::next(),
            split_id,
            buffer_id,
            selection: Selection::new_simple(),
            char_width: 0.0,
            width: 0.0,
            height: 0.0,
            scroll_offset: Vec2::ZERO,
            view_size: Size::ZERO,
        }
    }

    pub fn update(
        &self,
        ctx: &mut UpdateCtx,
        data: &LapceState,
        old_data: &LapceState,
        env: &Env,
    ) -> Option<()> {
        if self.buffer_id
            != old_data
                .editor_split
                .editors
                .get(&self.view_id)
                .unwrap()
                .buffer_id
        {
            ctx.request_paint();
            return None;
        }

        let buffer_id = self.buffer_id.as_ref()?;
        let buffer = data.editor_split.buffers.get(buffer_id)?;
        let old_buffer = old_data.editor_split.buffers.get(buffer_id)?;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);

        if buffer.max_len != old_buffer.max_len
            || buffer.num_lines() != old_buffer.num_lines()
        {
            ctx.request_layout();
        }

        if self.view_id == data.editor_split.active {
            for region in self.selection.regions() {
                let start = buffer.line_of_offset(region.min());
                let end = buffer.line_of_offset(region.max());
                let rect = Rect::ZERO
                    .with_origin(Point::new(0.0, start as f64 * line_height))
                    .with_size(Size::new(
                        ctx.size().width,
                        (end + 1 - start) as f64 * line_height,
                    ));
                ctx.request_paint_rect(rect);
            }

            for region in old_data
                .editor_split
                .editors
                .get(&self.view_id)
                .unwrap()
                .selection
                .regions()
            {
                let start = old_buffer.line_of_offset(region.min());
                let end = old_buffer.line_of_offset(region.max());
                let rect = Rect::ZERO
                    .with_origin(Point::new(0.0, start as f64 * line_height))
                    .with_size(Size::new(
                        ctx.size().width,
                        (end + 1 - start) as f64 * line_height,
                    ));
                ctx.request_paint_rect(rect);
            }
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
            if old_buffer.text_layouts[line].is_none()
                || buffer.text_layouts[line].as_ref().unwrap().text
                    != old_buffer.text_layouts[line].as_ref().unwrap().text
                || buffer.text_layouts[line].as_ref().unwrap().highlights
                    != old_buffer.text_layouts[line]
                        .as_ref()
                        .unwrap()
                        .highlights
            {
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
                    (updated_end_line + 1 - updated_start_line) as f64
                        * line_height,
                ));
            ctx.request_paint_rect(rect);
            // ctx.request_paint();
        }
        // let inval_lines = buffer.inval_lines.as_ref()?;
        // let start = inval_lines.start_line;
        // let rect = Rect::ZERO
        //     .with_origin(Point::new(0.0, start as f64 * line_height))
        //     .with_size(Size::new(
        //         ctx.size().width,
        //         inval_lines.new_count as f64 * line_height,
        //     ));
        // ctx.request_paint_rect(rect);

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
        mode: Mode,
        count: Option<usize>,
        buffer: &mut Buffer,
        cmd: LapceCommand,
        operator: Option<EditorOperator>,
        env: &Env,
    ) {
        let count = self.get_count(count, operator);
        if let Some(movement) = self.move_command(count, &cmd) {
            self.selection = buffer.do_move(
                &mode,
                movement,
                &self.selection,
                operator,
                count,
            );
            if mode != Mode::Insert {
                self.selection = buffer.correct_offset(&self.selection);
            }
            if cmd == LapceCommand::GotoLineDefaultFirst
                || cmd == LapceCommand::GotoLineDefaultLast
            {
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
                let lines = (self.view_size.height / line_height / 2.0).floor()
                    as usize;
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
                let lines = (self.view_size.height / line_height / 2.0).floor()
                    as usize;
                self.selection = Movement::Up.update_selection(
                    &self.selection,
                    buffer,
                    lines,
                    mode == Mode::Insert,
                    mode == Mode::Visual,
                );
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Scroll((
                        0.0,
                        -(lines as f64 * line_height),
                    )),
                    Target::Widget(self.view_id),
                ));
            }
            LapceCommand::CenterOfWindow => {
                self.center_of_window(ctx, buffer, env);
            }
            LapceCommand::ScrollUp => {
                // data.submit_ui_command(
                //     LapceUICommand::Scroll((0.0, -self.line_height)),
                //     self.editor_id,
                // );
            }
            LapceCommand::ScrollDown => {
                // data.submit_ui_command(
                //     LapceUICommand::Scroll((0.0, self.line_height)),
                //     self.editor_id,
                // );
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
            Mode::Normal => self.selection.expand(),
            Mode::Insert => self.selection.clone(),
            Mode::Visual => match visual_mode {
                VisualMode::Normal => self.selection.expand(),
                VisualMode::Linewise => {
                    let mut new_selection = Selection::new();
                    for region in self.selection.regions() {
                        let (start_line, _) =
                            buffer.offset_to_line_col(region.min());
                        let start = buffer.offset_of_line(start_line);
                        let (end_line, _) =
                            buffer.offset_to_line_col(region.max());
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
                    Selection::caret(
                        buffer.line_end_offset(old_offset, true) + 1,
                    )
                };
                for s in &content.content {
                    let delta =
                        buffer.edit(&format!("{}", s), &selection, true);
                    selection = selection.apply_delta(
                        &delta,
                        false,
                        InsertDrift::Default,
                    );
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
                    let delta = buffer.edit(s, &selection, true);
                    selection = selection.apply_delta(
                        &delta,
                        false,
                        InsertDrift::Default,
                    );
                }
                self.selection =
                    Selection::caret(selection.get_cursor_offset() - 1);
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
        mode: &Mode,
        visual_mode: &VisualMode,
        buffer: &mut Buffer,
        movement: Movement,
        count: Option<usize>,
    ) {
        let mut selection =
            self.get_selection(buffer, mode, visual_mode, false);
        if mode != &Mode::Visual {
            selection = movement.update_selection(
                &selection,
                buffer,
                count.unwrap_or(1),
                mode == &Mode::Insert,
                true,
            );
        }
        let delta = buffer.edit("", &selection, mode != &Mode::Insert);
        self.selection =
            selection.apply_delta(&delta, true, InsertDrift::Default);
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
        ctx.set_handled();
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

#[derive(Clone)]
pub struct RegisterContent {
    kind: VisualMode,
    content: Vec<String>,
}

#[derive(Clone)]
pub struct EditorSplitState {
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
}

impl EditorSplitState {
    pub fn new() -> EditorSplitState {
        let editor_split_id = WidgetId::next();
        let editor = EditorState::new(editor_split_id.clone(), None);
        let active = editor.view_id.clone();
        let mut editors = HashMap::new();
        editors.insert(editor.view_id, editor);
        EditorSplitState {
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
        }
    }

    pub fn set_active(&mut self, widget_id: WidgetId) {
        self.active = widget_id;
    }

    pub fn active(&self) -> WidgetId {
        self.active
    }

    pub fn new_editor(
        &mut self,
        split_id: WidgetId,
        buffer_id: Option<BufferId>,
        selection: Selection,
    ) -> &EditorState {
        let mut editor = EditorState::new(split_id, buffer_id);
        editor.selection = selection;
        let view_id = editor.view_id.clone();
        self.editors.insert(editor.view_id, editor);
        self.editors.get(&view_id).unwrap()
    }

    pub fn set_editor_scroll_offset(
        &mut self,
        editor_id: WidgetId,
        offset: Vec2,
    ) {
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

    pub fn open_file(&mut self, ctx: &mut EventCtx, path: &str) {
        let buffer_id = if let Some(buffer_id) = self.open_files.get(path) {
            buffer_id.clone()
        } else {
            let buffer_id = self.next_buffer_id();
            let buffer =
                Buffer::new(buffer_id.clone(), path, ctx.get_external_handle());
            self.buffers.insert(buffer_id.clone(), buffer);
            self.open_files.insert(path.to_string(), buffer_id.clone());
            buffer_id
        };

        let editor = self.editors.get_mut(&self.active).unwrap();
        if editor.buffer_id.as_ref() == Some(&buffer_id) {
            return;
        }
        editor.buffer_id = Some(buffer_id.clone());
        editor.selection = Selection::new_simple();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((0.0, 0.0)),
            Target::Widget(editor.view_id),
        ));
        ctx.request_layout();
        self.notify_fill_text_layouts(ctx, &buffer_id);
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

    pub fn insert(
        &mut self,
        ctx: &mut EventCtx,
        content: &str,
        env: &Env,
    ) -> Option<()> {
        if self.mode != Mode::Insert {
            return None;
        }
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = self.buffers.get_mut(&buffer_id)?;
        let delta = buffer.edit(content, &editor.selection, !self.inserting);
        editor.selection_apply_delta(&delta);
        editor.ensure_cursor_visible(ctx, buffer, env, None);
        self.inactive_editor_apply_delta(&delta);
        self.notify_fill_text_layouts(ctx, &buffer_id);
        self.inserting = true;
        None
    }

    pub fn fill_text_layouts(
        &mut self,
        ctx: &mut EventCtx,
        offset: Vec2,
        editor_id: &WidgetId,
        theme: &HashMap<String, Color>,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get(editor_id)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get_mut(buffer_id)?;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let start_line = (offset.y / line_height) as usize;
        let size = ctx.size();
        let num_lines = (size.height / line_height) as usize;
        let text = ctx.text();
        for line in start_line..start_line + num_lines + 1 {
            buffer.update_line_layouts(text, theme, line, env);
        }
        None
    }

    pub fn insert_new_line(
        &mut self,
        ctx: &mut EventCtx,
        offset: usize,
        new_undo_group: bool,
        env: &Env,
    ) -> Option<()> {
        let editor = self.editors.get_mut(&self.active)?;
        let buffer_id = editor.buffer_id.as_ref()?;
        let buffer = self.buffers.get_mut(&buffer_id)?;

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
        let delta = buffer.edit(&content, &selection, new_undo_group);
        editor.selection =
            selection.apply_delta(&delta, true, InsertDrift::Default);
        editor.ensure_cursor_visible(ctx, buffer, env, None);

        self.inactive_editor_apply_delta(&delta);
        None
    }

    pub fn inactive_editor_apply_delta(
        &mut self,
        delta: &RopeDelta,
    ) -> Option<()> {
        let buffer_id =
            self.editors.get(&self.active)?.buffer_id.as_ref()?.clone();
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
        None
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        count: Option<usize>,
        cmd: LapceCommand,
        env: &Env,
    ) -> Option<()> {
        let operator = self.operator.take();
        // let buffer_id = self.editors.get(&self.active)?.buffer_id.clone()?;
        // let editor = self.editors.get_mut(&self.active)?;
        // let buffer = self.buffers.get_mut(&buffer_id)?;
        match cmd {
            LapceCommand::InsertMode => {
                self.mode = Mode::Insert;
            }
            LapceCommand::InsertFirstNonBlank => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                editor.insert_mode(
                    buffer,
                    &self.mode,
                    &self.visual_mode,
                    ColPosition::FirstNonBlank,
                );
                self.mode = Mode::Insert;
            }
            LapceCommand::NormalMode => {
                self.inserting = false;
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let line =
                    buffer.line_of_offset(editor.selection.get_cursor_offset());
                let offset = buffer.first_non_blank_character_on_line(line);
                self.insert_new_line(ctx, offset, true, env);

                let editor = self.editors.get_mut(&self.active)?;
                editor.selection = Selection::caret(offset);
                self.mode = Mode::Insert;
                self.inserting = true;
            }

            LapceCommand::NewLineBelow => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                self.mode = Mode::Insert;
                let offset = buffer.line_end_offset(
                    editor.selection.get_cursor_offset(),
                    true,
                );
                self.insert_new_line(ctx, offset, true, env);
                self.inserting = true;
                // editor.insert_new_line(ctx, buffer, offset, env);
            }
            LapceCommand::InsertNewLine => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                if editor.selection.regions().len() == 1 {
                    let offset = editor.selection.get_cursor_offset();
                    self.insert_new_line(ctx, offset, false, env);
                } else {
                    let delta = buffer.edit("\n", &editor.selection, false);
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                let offset = editor.selection.get_cursor_offset();
                let new_offset = buffer.word_backword(offset);
                buffer.edit(
                    "",
                    &Selection::region(new_offset, offset),
                    self.mode != Mode::Insert,
                );
                editor.selection = Selection::caret(new_offset);
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                if self.mode == Mode::Insert {
                    self.inserting = true;
                }
                // editor.request_paint();
            }
            LapceCommand::DeleteBackward => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                editor.delete(
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
                }
            }
            LapceCommand::DeleteForeward => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;

                editor.delete(
                    &self.mode,
                    &self.visual_mode,
                    buffer,
                    Movement::Right,
                    count,
                );
                if self.mode == Mode::Visual {
                    editor.selection =
                        buffer.correct_offset(&editor.selection.collapse());
                    self.mode = Mode::Normal;
                }
                editor.ensure_cursor_visible(ctx, buffer, env, None);
                if self.mode == Mode::Insert {
                    self.inserting = true;
                }
            }
            LapceCommand::DeleteForewardAndInsert => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                editor.delete(
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
                self.operator =
                    Some(EditorOperator::Delete(EditorCount(count)));
            }
            LapceCommand::Paste => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                if let Some(content) = self.register.get("x") {
                    editor.paste(
                        ctx,
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
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
                let delta = buffer.edit("", &selection, true);
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
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
            LapceCommand::Undo => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                if let Some(offset) = buffer.undo() {
                    let line = buffer.line_of_offset(offset);
                    let offset = buffer.offset_of_line(line);
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
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                if let Some(offset) = buffer.redo() {
                    let line = buffer.line_of_offset(offset);
                    let offset = buffer.offset_of_line(line);
                    editor.selection = Selection::caret(offset);
                    editor.ensure_cursor_visible(
                        ctx,
                        buffer,
                        env,
                        Some(EnsureVisiblePosition::CenterOfWindow),
                    );
                }
            }
            _ => {
                let editor = self.editors.get_mut(&self.active)?;
                let buffer =
                    self.buffers.get_mut(editor.buffer_id.as_ref()?)?;
                editor.run_command(
                    ctx,
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
        self.notify_fill_text_layouts(ctx, &buffer_id);
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

    pub fn request_paint(&self) {
        // LAPCE_STATE.submit_ui_command(
        //     LapceUICommand::RequestPaint,
        //     self.widget_id.unwrap(),
        // );
    }
}

pub struct EditorView {
    split_id: WidgetId,
    view_id: WidgetId,
    pub editor_id: WidgetId,
    editor: WidgetPod<LapceState, LapceScroll<LapceState, Padding<LapceState>>>,
    gutter: WidgetPod<LapceState, Box<dyn Widget<LapceState>>>,
}

impl EditorView {
    pub fn new(
        split_id: WidgetId,
        view_id: WidgetId,
        editor_id: WidgetId,
    ) -> IdentityWrapper<EditorView> {
        let editor =
            IdentityWrapper::wrap(Editor::new(view_id), editor_id.clone());
        let scroll = LapceScroll::new(editor.padding((10.0, 0.0, 10.0, 0.0)));
        EditorView {
            split_id,
            view_id,
            editor_id,
            editor: WidgetPod::new(scroll),
            gutter: WidgetPod::new(
                EditorGutter::new(view_id).padding((10.0, 0.0, 10.0, 0.0)),
            )
            .boxed(),
        }
        .with_id(view_id)
    }
}

impl Widget<LapceState> for EditorView {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                self.gutter.event(ctx, event, data, env);
                self.editor.event(ctx, event, data, env);
            }
            Event::Wheel(_) => {
                self.editor.event(ctx, event, data, env);
                let editor_split = Arc::make_mut(&mut data.editor_split);
                editor_split.fill_text_layouts(
                    ctx,
                    self.editor.widget().offset(),
                    &self.view_id,
                    &data.theme,
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
                        LapceUICommand::EditorViewSize(size) => {
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split
                                .editors
                                .get_mut(&self.view_id)
                                .unwrap()
                                .view_size = *size;
                        }
                        LapceUICommand::FillTextLayouts => {
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split.fill_text_layouts(
                                ctx,
                                self.editor.widget().offset(),
                                &self.view_id,
                                &data.theme,
                                env,
                            );
                        }
                        LapceUICommand::EnsureVisible((
                            rect,
                            margin,
                            position,
                        )) => {
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            let editor = self.editor.widget_mut();
                            if editor.ensure_visible(ctx.size(), rect, margin) {
                                match position {
                                    Some(
                                        EnsureVisiblePosition::CenterOfWindow,
                                    ) => {
                                        let editor_state = editor_split
                                            .editors
                                            .get_mut(&self.view_id)
                                            .unwrap();
                                        let buffer = editor_split
                                            .buffers
                                            .get(
                                                editor_state
                                                    .buffer_id
                                                    .as_ref()
                                                    .unwrap(),
                                            )
                                            .unwrap();
                                        let line_height = env.get(
                                            LapceTheme::EDITOR_LINE_HEIGHT,
                                        );
                                        let line = buffer.line_of_offset(
                                            editor_state
                                                .selection
                                                .get_cursor_offset(),
                                        );
                                        let y = if line as f64 * line_height
                                            > ctx.size().height / 2.0
                                        {
                                            line as f64 * line_height
                                                - ctx.size().height / 2.0
                                        } else {
                                            0.0
                                        };
                                        let scroll = self.editor.widget_mut();
                                        scroll.scroll_to(0.0, y);
                                        editor_state.scroll_offset =
                                            scroll.offset();
                                    }
                                    None => {
                                        let offset = editor.offset();
                                        editor_split
                                            .editors
                                            .get_mut(&self.view_id)
                                            .unwrap()
                                            .scroll_offset = offset;
                                        self.gutter.set_viewport_offset(
                                            Vec2::new(0.0, offset.y),
                                        );
                                    }
                                }
                                ctx.request_paint();
                            }
                        }
                        LapceUICommand::ScrollTo((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll_to(*x, *y);
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split
                                .editors
                                .get_mut(&self.view_id)
                                .unwrap()
                                .scroll_offset = scroll.offset();
                            ctx.request_paint();
                        }
                        LapceUICommand::Scroll((x, y)) => {
                            let scroll = self.editor.widget_mut();
                            scroll.scroll(*x, *y);
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split
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
        data: &LapceState,
        env: &Env,
    ) {
        match event {
            LifeCycle::Size(size) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EditorViewSize(*size),
                    Target::Widget(self.view_id.clone()),
                ));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(self.view_id.clone()),
                ));
            }
            _ => (),
        }
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
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
        data: &LapceState,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        self.gutter.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(gutter_size),
        );
        let editor_size =
            Size::new(self_size.width - gutter_size.width, self_size.height);
        // LAPCE_STATE
        //     .editor_split
        //     .lock()
        //     .unwrap()
        //     .set_editor_size(self.view_id, editor_size);
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO
                .with_origin(Point::new(gutter_size.width, 0.0))
                .with_size(editor_size),
        );
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
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
    }
}

pub struct EditorGutter {
    view_id: WidgetId,
    text_layouts: HashMap<String, EditorTextLayout>,
}

impl EditorGutter {
    pub fn new(view_id: WidgetId) -> EditorGutter {
        EditorGutter {
            view_id,
            text_layouts: HashMap::new(),
        }
    }
}

impl Widget<LapceState> for EditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        let cursor = data.editor_split.get_cursor(&self.view_id);
        let old_cursor = old_data.editor_split.get_cursor(&self.view_id);

        if (cursor.is_none() && old_cursor.is_none())
            || (cursor.is_some()
                && old_cursor.is_some()
                && cursor.unwrap().0 == old_cursor.unwrap().0)
        {
        } else {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        // let buffer_id = {
        //     LAPCE_STATE
        //         .editor_split
        //         .lock()
        //         .unwrap()
        //         .get_buffer_id(&self.view_id)
        // };
        if let Some(buffer_id) = data
            .editor_split
            .editors
            .get(&self.view_id)
            .as_ref()
            .unwrap()
            .buffer_id
            .as_ref()
        {
            let buffer = data.editor_split.buffers.get(buffer_id).unwrap();
            let width = 7.6171875;
            Size::new(
                width * buffer.last_line().to_string().len() as f64,
                25.0 * buffer.num_lines() as f64,
            )
        } else {
            Size::new(50.0, 50.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = data
            .editor_split
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

        let buffer = data.editor_split.buffers.get(buffer_id.unwrap()).unwrap();
        let last_line = buffer.last_line();
        let rects = ctx.region().rects().to_vec();
        let active = data.editor_split.active;
        let editor = data.editor_split.editors.get(&self.view_id).unwrap();
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
                let x = (last_line.to_string().len()
                    - content.to_string().len()) as f64
                    * width;
                let content = content.to_string();
                if let Some(text_layout) = self.text_layouts.get_mut(&content) {
                    if text_layout.text != content {
                        text_layout.layout.set_text(content.clone());
                        text_layout.text = content;
                        text_layout
                            .layout
                            .rebuild_if_needed(&mut ctx.text(), env);
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
        // let buffer_id = {
        //     LAPCE_STATE
        //         .editor_split
        //         .lock()
        //         .unwrap()
        //         .get_buffer_id(&self.view_id)
        //         .clone()
        // };
        // if let Some(buffer_id) = buffer_id {
        //     let mut editor_split = LAPCE_STATE.editor_split.lock().unwrap();
        //     let mut layout = TextLayout::new("W");
        //     layout.set_font(LapceTheme::EDITOR_FONT);
        //     layout.rebuild_if_needed(&mut ctx.text(), env);
        //     let width = layout.point_for_text_position(1).x;
        //     let buffers = &editor_split.buffers;
        //     let buffer = buffers.get(&buffer_id).unwrap();
        //     let (current_line, _) = {
        //         let editor = editor_split.editors.get(&self.view_id).unwrap();
        //         buffer.offset_to_line_col(editor.selection.get_cursor_offset())
        //     };
        //     let active = editor_split.active;
        //     let rects = ctx.region().rects().to_vec();
        //     for rect in rects {
        //         let start_line = (rect.y0 / line_height).floor() as usize;
        //         let num_lines = (rect.height() / line_height).floor() as usize;
        //         let last_line = buffer.last_line();
        //         for line in start_line..start_line + num_lines {
        //             if line > last_line {
        //                 break;
        //             }
        //             let content = if active != self.view_id {
        //                 line
        //             } else {
        //                 if line == current_line {
        //                     line
        //                 } else if line > current_line {
        //                     line - current_line
        //                 } else {
        //                     current_line - line
        //                 }
        //             };
        //             let x = (last_line.to_string().len()
        //                 - content.to_string().len())
        //                 as f64
        //                 * width;
        //             let content = content.to_string();
        //             if let Some(text_layout) =
        //                 self.text_layouts.get_mut(&content)
        //             {
        //                 if text_layout.text != content {
        //                     text_layout.layout.set_text(content.clone());
        //                     text_layout.text = content;
        //                     text_layout
        //                         .layout
        //                         .rebuild_if_needed(&mut ctx.text(), env);
        //                 }
        //                 text_layout.layout.draw(
        //                     ctx,
        //                     Point::new(x, line_height * line as f64),
        //                 );
        //             } else {
        //                 let mut layout = TextLayout::new(content.clone());
        //                 layout.set_font(LapceTheme::EDITOR_FONT);
        //                 layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        //                 layout.rebuild_if_needed(&mut ctx.text(), env);
        //                 layout.draw(
        //                     ctx,
        //                     Point::new(x, line_height * line as f64),
        //                 );
        //                 let text_layout = EditorTextLayout {
        //                     layout,
        //                     text: content.clone(),
        //                 };
        //                 self.text_layouts.insert(content, text_layout);
        //             }
        //         }
        //     }
        // }
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
    text_layout: TextLayout,
    view_id: WidgetId,
    text_layouts: HashMap<usize, HighlightTextLayout>,
    view_size: Size,
}

impl Editor {
    pub fn new(view_id: WidgetId) -> Self {
        let text_layout = TextLayout::new("");
        Editor {
            text_layout,
            view_id,
            text_layouts: HashMap::new(),
            view_size: Size::ZERO,
        }
    }

    fn paint_line_new(
        &mut self,
        ctx: &mut PaintCtx,
        buffer: &mut Buffer,
        line_height: f64,
        line: usize,
        line_content: &str,
        // text_layouts: &mut Vec<Option<HighlightTextLayout>>,
        env: &Env,
    ) {
        // let start_offset = buffer.offset_of_line(line);
        // let end_offset = buffer.offset_of_line(line + 1);
        // let mut offset = start_offset;
        // let mut x = 0.0;
        // let mut layout_builder = ctx
        //     .text()
        //     .new_text_layout(line_content.to_string())
        //     .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
        //     .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));

        // for (start, end, hl) in buffer.get_line_highligh(line) {
        //     if let Some(color) = LAPCE_STATE.theme.lock().unwrap().get(hl) {
        //         layout_builder = layout_builder.range_attribute(
        //             start - start_offset..end - start_offset,
        //             TextAttribute::TextColor(color.clone()),
        //         );
        //     }
        // }
        // let layout = layout_builder.build().unwrap();
        // ctx.draw_text(&layout, Point::new(0.0, line_height * line as f64));
        // let text_layout = HighlightTextLayout {
        //     layout,
        //     text: line_content.to_string(),
        //     highlights: buffer.get_line_highligh(line).clone(),
        // };
    }

    fn paint_line(
        &mut self,
        ctx: &mut PaintCtx,
        buffer: &mut Buffer,
        line_height: f64,
        line: usize,
        line_content: &str,
        env: &Env,
    ) {
        // let start_offset = buffer.offset_of_line(line);
        // let end_offset = buffer.offset_of_line(line + 1);
        // let mut offset = start_offset;
        // let mut x = 0.0;
        // let mut layout_builder = ctx
        //     .text()
        //     .new_text_layout(line_content.to_string())
        //     .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
        //     .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));

        // for (start, end, hl) in buffer.get_line_highligh(line) {
        //     if let Some(color) = LAPCE_STATE.theme.lock().unwrap().get(hl) {
        //         layout_builder = layout_builder.range_attribute(
        //             start - start_offset..end - start_offset,
        //             TextAttribute::TextColor(color.clone()),
        //         );
        //     }
        // }
        // let layout = layout_builder.build().unwrap();
        // ctx.draw_text(&layout, Point::new(0.0, line_height * line as f64));
        // let text_layout = HighlightTextLayout {
        //     layout,
        //     text: line_content.to_string(),
        //     highlights: buffer.get_line_highligh(line).clone(),
        // };
        // self.text_layouts.insert(line, text_layout);
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
            let x = col as f64 * width;
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
            let (start_line, start_col) =
                buffer.offset_to_line_col(region.min());
            let (end_line, end_col) = buffer.offset_to_line_col(region.max());

            for line in start_line..end_line + 1 {
                let x0 = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == start_line => start_col as f64 * width,
                        _ => 0.0,
                    },
                    &VisualMode::Linewise => 0.0,
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false);
                        let left = start_col.min(end_col);
                        if left > max_col {
                            continue;
                        }
                        left as f64 * width
                    }
                };

                let x1 = match visual_mode {
                    &VisualMode::Normal => match line {
                        _ if line == end_line => (end_col + 1) as f64 * width,
                        _ => {
                            (buffer.offset_of_line(line + 1)
                                - buffer.offset_of_line(line))
                                as f64
                                * width
                        }
                    },
                    &VisualMode::Linewise => {
                        (buffer.offset_of_line(line + 1)
                            - buffer.offset_of_line(line))
                            as f64
                            * width
                    }
                    &VisualMode::Blockwise => {
                        let max_col = buffer.line_max_col(line, false) + 1;
                        let right = match region.horiz() {
                            Some(&ColPosition::End) => max_col,
                            _ => (end_col.max(start_col) + 1).min(max_col),
                        };
                        right as f64 * width
                    }
                };
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

impl Widget<LapceState> for Editor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
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
                        _ => println!(
                            "editor unprocessed ui command {:?}",
                            command
                        ),
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
        data: &LapceState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        let editor = data.editor_split.editors.get(&self.view_id).unwrap();
        editor.update(ctx, data, old_data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        // let buffer_id = {
        //     LAPCE_STATE
        //         .editor_split
        //         .lock()
        //         .unwrap()
        //         .get_buffer_id(&self.view_id)
        // };
        self.view_size = bc.min();
        if let Some(buffer_id) = data.editor_split.get_buffer_id(&self.view_id)
        {
            let buffer = data.editor_split.buffers.get(&buffer_id).unwrap();
            let width = 7.6171875;
            Size::new(
                (width * buffer.max_len as f64).max(bc.min().width),
                25.0 * buffer.num_lines() as f64,
            )
        } else {
            Size::new(0.0, 0.0)
        }
        // if let Some(buffer_id) = buffer_id {
        //     let buffers = &LAPCE_STATE.editor_split.lock().unwrap().buffers;
        //     let buffer = buffers.get(&buffer_id).unwrap();
        //     let width = 7.6171875;
        //     Size::new(
        //         width * buffer.max_len as f64,
        //         25.0 * buffer.num_lines() as f64,
        //     )
        // } else {
        //     Size::new(0.0, 0.0)
        // }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let buffer_id = data.editor_split.get_buffer_id(&self.view_id);
        if buffer_id.is_none() {
            return;
        }
        let buffer_id = buffer_id.unwrap();
        let size = ctx.size();

        let mut layout = TextLayout::new("W");
        layout.set_font(LapceTheme::EDITOR_FONT);
        layout.rebuild_if_needed(&mut ctx.text(), env);
        let width = layout.point_for_text_position(1).x;

        let buffer = data.editor_split.buffers.get(&buffer_id).unwrap();
        let editor = data.editor_split.editors.get(&self.view_id).unwrap();
        let editor_offset = editor.selection.get_cursor_offset();
        let cursor = buffer.offset_to_line_col(editor_offset);

        let mode = data.editor_split.mode.clone();
        let visual_mode = data.editor_split.visual_mode.clone();

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
                                    .with_size(Size::new(
                                        size.width,
                                        line_height,
                                    )),
                                &env.get(
                                    LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                                ),
                            );
                        }
                    };

                    let line_content = buffer
                        .slice_to_cow(
                            buffer.offset_of_line(line)
                                ..buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    if data.editor_split.active == self.view_id {
                        let cursor_x = (line_content[..cursor.1]
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
                            + cursor.1)
                            as f64
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
                        // if mode == Mode::Insert {
                        //     self.paint_insert_cusor(
                        //         ctx,
                        //         &editor.selection,
                        //         buffer,
                        //         line_height,
                        //         width,
                        //         start_line,
                        //         num_lines,
                        //         env,
                        //     );
                        // }
                    }
                }
                if buffer.text_layouts.len() > line {
                    if let Some(layout) = buffer.text_layouts[line].as_ref() {
                        ctx.draw_text(
                            &layout.layout,
                            Point::new(0.0, line_height * line as f64),
                        );
                    }
                }
            }
        }
        // if let Some(buffer_id) = buffer_id {
        //     let mut editor_split = LAPCE_STATE.editor_split.lock().unwrap();

        //     let mut layout = TextLayout::new("W");
        //     layout.set_font(LapceTheme::EDITOR_FONT);
        //     layout.rebuild_if_needed(&mut ctx.text(), env);
        //     let width = layout.point_for_text_position(1).x;
        //     let mode = editor_split.get_mode().clone();
        //     let visual_mode = editor_split.visual_mode.clone();
        //     let active_view_id = editor_split.active.clone();
        //     let (editor_width, editor_offset, selection) = {
        //         let editor = editor_split.get_editor(&self.view_id);
        //         editor.set_line_height(line_height);
        //         editor.char_width = width;
        //         (
        //             editor.width,
        //             editor.selection.get_cursor_offset(),
        //             editor.selection.clone(),
        //         )
        //     };

        //     let mut buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
        //     let cursor = buffer.offset_to_line_col(editor_offset);
        //     let rects = ctx.region().rects().to_vec();
        //     for rect in rects {
        //         let start_line = (rect.y0 / line_height).floor() as usize;
        //         let num_lines = (rect.height() / line_height).floor() as usize;
        //         if mode == Mode::Visual {
        //             self.paint_selection(
        //                 ctx,
        //                 &mode,
        //                 &visual_mode,
        //                 &selection,
        //                 buffer,
        //                 line_height,
        //                 width,
        //                 start_line,
        //                 num_lines,
        //                 env,
        //             );
        //         }
        //         let last_line = buffer.last_line();
        //         for line in start_line..start_line + num_lines + 1 {
        //             if line > last_line {
        //                 break;
        //             }
        //             let line_content = buffer
        //                 .slice_to_cow(
        //                     buffer.offset_of_line(line)
        //                         ..buffer.offset_of_line(line + 1),
        //                 )
        //                 .to_string();
        //             if line == cursor.0 {
        //                 match mode {
        //                     Mode::Visual => (),
        //                     _ => {
        //                         ctx.fill(
        //                         Rect::ZERO
        //                             .with_origin(Point::new(
        //                                 0.0,
        //                                 cursor.0 as f64 * line_height,
        //                             ))
        //                             .with_size(Size::new(
        //                                 editor_width,
        //                                 line_height,
        //                             )),
        //                         &env.get(
        //                             LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
        //                         ),
        //                     );
        //                     }
        //                 };

        //                 if active_view_id == self.view_id {
        //                     let cursor_x = (line_content[..cursor.1]
        //                         .chars()
        //                         .filter_map(|c| {
        //                             if c == '\t' {
        //                                 Some('\t')
        //                             } else {
        //                                 None
        //                             }
        //                         })
        //                         .count()
        //                         * 3
        //                         + cursor.1)
        //                         as f64
        //                         * width;
        //                     match mode {
        //                         Mode::Insert => (),
        //                         _ => ctx.fill(
        //                             Rect::ZERO
        //                                 .with_origin(Point::new(
        //                                     cursor_x,
        //                                     cursor.0 as f64 * line_height,
        //                                 ))
        //                                 .with_size(Size::new(
        //                                     width,
        //                                     line_height,
        //                                 )),
        //                             &env.get(LapceTheme::EDITOR_CURSOR_COLOR),
        //                         ),
        //                     };
        //                 }
        //             }

        //             let mut cache_draw = false;
        //             if let Some(buffer_ui) = data.buffers.get(&buffer_id) {
        //                 if buffer_ui.text_layouts.len() > line {
        //                     if let Some(layout) =
        //                         buffer_ui.text_layouts[line].as_ref()
        //                     {
        //                         if layout.text == line_content.to_string()
        //                             && &layout.highlights
        //                                 == buffer.get_line_highligh(line)
        //                         {
        //                             ctx.draw_text(
        //                                 &layout.layout,
        //                                 Point::new(
        //                                     0.0,
        //                                     line_height * line as f64,
        //                                 ),
        //                             );
        //                             cache_draw = true;
        //                         }
        //                     }
        //                 }
        //             }
        //             if mode == Mode::Insert {
        //                 self.paint_insert_cusor(
        //                     ctx,
        //                     &selection,
        //                     buffer,
        //                     line_height,
        //                     width,
        //                     start_line,
        //                     num_lines,
        //                     env,
        //                 );
        //             }
        //         }
        //     }
        // }
    }
}
