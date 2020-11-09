use anyhow::{anyhow, Result};
use druid::{
    piet::{PietText, Text, TextAttribute, TextLayoutBuilder},
    Color, Command, EventCtx, ExtEventSink, Target, UpdateCtx, WidgetId, WindowId,
};
use druid::{Env, PaintCtx};
use language::{new_highlight_config, new_parser, LapceLanguage};
use lsp_types::{
    CodeActionResponse, Position, Range, TextDocumentContentChangeEvent,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::{
    borrow::Cow,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use std::{collections::HashMap, fs::File};
use std::{fs, str::FromStr};
use tree_sitter::{Parser, Tree};
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use xi_core_lib::{
    line_offset::{LineOffset, LogicalLines},
    selection::InsertDrift,
};
use xi_rope::{
    interval::IntervalBounds, rope::Rope, Cursor, Delta, DeltaBuilder, Interval,
    LinesMetric, RopeDelta, RopeInfo, Transformer,
};

use crate::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    editor::EditorOperator,
    editor::HighlightTextLayout,
    language,
    movement::{ColPosition, Movement, SelRegion, Selection},
    plugin::PluginBufferInfo,
    state::LapceTabState,
    state::LapceWorkspaceType,
    state::Mode,
    state::LAPCE_APP_STATE,
    theme::LapceTheme,
};

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct BufferId(pub usize);

#[derive(Clone)]
pub struct BufferUIState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub id: BufferId,
    pub text_layouts: Vec<Arc<Option<HighlightTextLayout>>>,
    pub max_len_line: usize,
    pub max_len: usize,
    pub dirty: bool,
}

#[derive(Clone)]
pub struct Buffer {
    window_id: WindowId,
    tab_id: WidgetId,
    pub id: BufferId,
    pub rope: Rope,
    highlight_config: Arc<HighlightConfiguration>,
    highlight_names: Vec<String>,
    pub highlights: Vec<(usize, usize, Highlight)>,
    pub line_highlights: HashMap<usize, Vec<(usize, usize, String)>>,
    undos: Vec<Vec<(RopeDelta, RopeDelta)>>,
    current_undo: usize,
    pub path: String,
    pub language_id: String,
    pub rev: u64,
    pub dirty: bool,
    pub code_actions: HashMap<usize, CodeActionResponse>,
    sender: Sender<(WindowId, WidgetId, BufferId, u64)>,
}

impl Buffer {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        buffer_id: BufferId,
        path: &str,
        event_sink: ExtEventSink,
        sender: Sender<(WindowId, WidgetId, BufferId, u64)>,
    ) -> Buffer {
        let rope = if let Ok(rope) = load_file(&window_id, &tab_id, path) {
            rope
        } else {
            Rope::from("")
        };
        let mut parser = new_parser(LapceLanguage::Rust);
        let tree = parser.parse(&rope.to_string(), None).unwrap();

        let (highlight_config, highlight_names) =
            new_highlight_config(LapceLanguage::Rust);

        let path_buf = PathBuf::from_str(path).unwrap();
        path_buf.extension().unwrap().to_str().unwrap().to_string();

        let mut buffer = Buffer {
            window_id: window_id.clone(),
            tab_id: tab_id.clone(),
            id: buffer_id.clone(),
            rope,
            highlight_config: Arc::new(highlight_config),
            highlight_names,
            highlights: Vec::new(),
            line_highlights: HashMap::new(),
            undos: Vec::new(),
            current_undo: 0,
            code_actions: HashMap::new(),
            rev: 0,
            dirty: false,
            language_id: language_id_from_path(path).unwrap_or("").to_string(),
            path: path.to_string(),
            sender,
        };

        let language_id = buffer.language_id.clone();

        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        state.plugins.lock().new_buffer(&PluginBufferInfo {
            buffer_id: buffer_id.clone(),
            language_id: buffer.language_id.clone(),
            path: path.to_string(),
            nb_lines: buffer.num_lines(),
            buf_size: buffer.len(),
            rev: buffer.rev,
        });
        state.lsp.lock().new_buffer(
            &buffer_id,
            path,
            &buffer.language_id,
            buffer.rope.to_string(),
        );
        buffer.update_highlights();
        buffer
    }

    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let workspace_type = state.workspace.lock().kind.clone();
        match workspace_type {
            LapceWorkspaceType::RemoteSSH(host) => {
                state.get_ssh_session(&host)?;
                let mut ssh_session = state.ssh_session.lock();
                let ssh_session = ssh_session.as_mut().unwrap();
                let tmp_path = format!("{}.swp", self.path);
                let mut remote_file =
                    ssh_session.send(&tmp_path, 0o644, self.len() as u64)?;
                for chunk in self.rope.iter_chunks(..self.rope.len()) {
                    ssh_session.channel_write(&mut remote_file, chunk.as_bytes())?;
                }
                println!("send remote_file {}", tmp_path);
                ssh_session.exec(&format!("mv {} {}", tmp_path, self.path))?;
            }
            LapceWorkspaceType::Local => {
                let path = PathBuf::from_str(&self.path)?;
                let tmp_extension = path.extension().map_or_else(
                    || OsString::from("swp"),
                    |ext| {
                        let mut ext = ext.to_os_string();
                        ext.push(".swp");
                        ext
                    },
                );
                let tmp_path = &path.with_extension(tmp_extension);

                let mut f = File::create(tmp_path)?;
                for chunk in self.rope.iter_chunks(..self.rope.len()) {
                    f.write_all(chunk.as_bytes())?;
                }
                fs::rename(tmp_path, path)?;
            }
        };
        self.dirty = false;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn highlights_apply_delta(
        &mut self,
        delta: &RopeDelta,
    ) -> Vec<(usize, usize, Highlight)> {
        let mut transformer = Transformer::new(delta);
        self.highlights
            .iter()
            .map(|h| {
                (
                    transformer.transform(h.0, true),
                    transformer.transform(h.1, true),
                    h.2.clone(),
                )
            })
            .collect()
    }

    pub fn update_highlights(&mut self) {
        self.line_highlights = HashMap::new();
        self.sender
            .send((self.window_id, self.tab_id, self.id, self.rev));
    }

    pub fn get_line_highligh(
        &mut self,
        line: usize,
    ) -> &Vec<(usize, usize, String)> {
        if self.line_highlights.get(&line).is_none() {
            let mut line_highlight = Vec::new();
            let start_offset = self.offset_of_line(line);
            let end_offset = self.offset_of_line(line + 1) - 1;
            for (start, end, hl) in &self.highlights {
                if *start > end_offset {
                    break;
                }
                if *start >= start_offset && *start <= end_offset {
                    let end = if *end > end_offset {
                        end_offset - start_offset
                    } else {
                        end - start_offset
                    };
                    line_highlight.push((
                        start - start_offset,
                        end,
                        self.highlight_names[hl.0].to_string(),
                    ));
                }
            }
            self.line_highlights.insert(line, line_highlight);
        }
        self.line_highlights.get(&line).unwrap()
    }

    pub fn correct_offset(&self, selection: &Selection) -> Selection {
        let mut result = Selection::new();
        for region in selection.regions() {
            let (line, col) = self.offset_to_line_col(region.start());
            let max_col = self.line_max_col(line, false);
            let (start, col) = if col > max_col {
                (self.offset_of_line(line) + max_col, max_col)
            } else {
                (region.start(), col)
            };

            let (line, col) = self.offset_to_line_col(region.start());
            let max_col = self.line_max_col(line, false);
            let end = if col > max_col {
                self.offset_of_line(line) + max_col
            } else {
                region.end()
            };

            let new_region =
                SelRegion::new(start, end, region.horiz().map(|h| h.clone()));
            result.add_region(new_region);
        }
        result
    }

    pub fn fill_horiz(&self, selection: &Selection) -> Selection {
        let mut result = Selection::new();
        for region in selection.regions() {
            let new_region = if region.horiz().is_some() {
                region.clone()
            } else {
                let (_, col) = self.offset_to_line_col(region.min());
                SelRegion::new(
                    region.start(),
                    region.end(),
                    Some(ColPosition::Col(col)),
                )
            };
            result.add_region(new_region);
        }
        result
    }

    fn update_size(
        &mut self,
        ui_state: &mut BufferUIState,
        inval_lines: &InvalLines,
    ) {
        if ui_state.max_len_line >= inval_lines.start_line
            && ui_state.max_len_line
                < inval_lines.start_line + inval_lines.inval_count
        {
            let (max_len, max_len_line) = self.get_max_line_len();
            ui_state.max_len = max_len;
            ui_state.max_len_line = max_len_line;
        } else {
            let mut max_len = 0;
            let mut max_len_line = 0;
            for line in inval_lines.start_line
                ..inval_lines.start_line + inval_lines.new_count
            {
                let line_len = self.line_len(line);
                if line_len > max_len {
                    max_len = line_len;
                    max_len_line = line;
                }
            }
            if max_len > ui_state.max_len {
                ui_state.max_len = max_len;
                ui_state.max_len_line = max_len_line;
            } else if ui_state.max_len >= inval_lines.start_line {
                ui_state.max_len_line = ui_state.max_len_line
                    + inval_lines.new_count
                    - inval_lines.inval_count;
            }
        }
    }

    fn inv_delta(&self, delta: &RopeDelta) -> RopeDelta {
        let (ins, del) = delta.clone().factor();
        let del_rope = del.complement().delete_from(&self.rope);
        let ins = ins.inserted_subset();
        let del = del.transform_expand(&ins);
        Delta::synthesize(&del_rope, &del, &ins)
    }

    fn add_undo(&mut self, delta: &RopeDelta, new_undo_group: bool) {
        let inv_delta = self.inv_delta(delta);
        if new_undo_group {
            self.undos.truncate(self.current_undo);
            self.undos.push(vec![(delta.clone(), inv_delta)]);
            self.current_undo += 1;
        } else {
            if self.undos.is_empty() {
                self.undos.push(Vec::new());
                self.current_undo += 1;
            }
            // let mut undos = &self.undos[self.current_undo - 1];
            // let last_undo = &undos[undos.len() - 1];
            // last_undo.0.is_identity();
            self.undos[self.current_undo - 1].push((delta.clone(), inv_delta));
        }
    }

    pub fn redo(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
    ) -> Option<usize> {
        if self.current_undo >= self.undos.len() {
            return None;
        }
        let deltas = self.undos[self.current_undo].clone();
        self.current_undo += 1;
        for (delta, __) in deltas.iter() {
            self.apply_delta(ctx, ui_state, &delta);
        }
        self.update_highlights();
        Some(deltas[0].1.summary().0.start())
    }

    pub fn undo(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
    ) -> Option<usize> {
        if self.current_undo < 1 {
            return None;
        }

        self.current_undo -= 1;
        let deltas = self.undos[self.current_undo].clone();
        for (_, delta) in deltas.iter().rev() {
            self.apply_delta(ctx, ui_state, &delta);
        }
        self.update_highlights();
        Some(deltas[0].1.summary().0.start())
    }

    fn apply_delta(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        delta: &RopeDelta,
    ) {
        self.rev += 1;
        self.dirty = true;
        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;
        let old_logical_end_offset = self.rope.offset_of_line(old_logical_end_line);

        let content_change = get_document_content_changes(delta, self);

        self.rope = delta.apply(&self.rope);
        let content_change = match content_change {
            Some(content_change) => content_change,
            None => TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            },
        };

        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        state.plugins.lock().update(
            &self.id,
            delta,
            self.len(),
            self.num_lines(),
            self.rev,
        );
        state.lsp.lock().update(&self, &content_change, self.rev);

        let logical_start_line = self.rope.line_of_offset(iv.start);
        let new_logical_end_line = self.rope.line_of_offset(iv.start + newlen) + 1;
        let old_hard_count = old_logical_end_line - logical_start_line;
        let new_hard_count = new_logical_end_line - logical_start_line;

        let inval_lines = InvalLines {
            start_line: logical_start_line,
            inval_count: old_hard_count,
            new_count: new_hard_count,
        };
        self.code_actions = HashMap::new();
        self.highlights = self.highlights_apply_delta(delta);
        self.update_size(ui_state, &inval_lines);
        ui_state.update_text_layouts(&inval_lines);
    }

    pub fn yank(&self, selection: &Selection) -> Vec<String> {
        selection
            .regions()
            .iter()
            .map(|region| {
                self.rope
                    .slice_to_cow(region.min()..region.max())
                    .to_string()
            })
            .collect()
    }

    pub fn do_move(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        mode: &Mode,
        movement: &Movement,
        selection: &Selection,
        operator: Option<EditorOperator>,
        count: Option<usize>,
    ) -> Selection {
        if let Some(operator) = operator {
            let selection = movement.update_selection(
                selection,
                &self,
                count.unwrap_or(1),
                true,
                true,
            );
            let mut new_selection = Selection::new();
            for region in selection.regions() {
                let start_line = self.line_of_offset(region.min());
                let end_line = self.line_of_offset(region.max());
                let new_region = if movement.is_vertical() {
                    let region = SelRegion::new(
                        self.offset_of_line(start_line),
                        self.offset_of_line(end_line + 1),
                        Some(ColPosition::Col(0)),
                    );
                    region
                } else {
                    if movement.is_inclusive() {
                        SelRegion::new(
                            region.min(),
                            region.max() + 1,
                            region.horiz().map(|h| h.clone()),
                        )
                    } else {
                        region.clone()
                    }
                };
                new_selection.add_region(new_region);
            }
            match operator {
                EditorOperator::Delete(_) => {
                    let delta = self.edit(ctx, ui_state, "", &new_selection, true);
                    new_selection.apply_delta(&delta, true, InsertDrift::Default)
                }
                EditorOperator::Yank(_) => new_selection,
            }
        } else {
            movement.update_selection(
                &selection,
                &self,
                count.unwrap_or(1),
                mode == &Mode::Insert,
                mode == &Mode::Visual,
            )
        }
    }

    pub fn edit_multiple(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        edits: Vec<(&Selection, &str)>,
        new_undo_group: bool,
    ) -> RopeDelta {
        let mut builder = DeltaBuilder::new(self.len());
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.regions() {
                builder.replace(region.min()..region.max(), rope.clone());
            }
        }
        let delta = builder.build();
        self.add_undo(&delta, new_undo_group);
        self.apply_delta(ctx, ui_state, &delta);
        self.update_highlights();
        delta
    }

    pub fn edit(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut BufferUIState,
        content: &str,
        selection: &Selection,
        new_undo_group: bool,
    ) -> RopeDelta {
        self.edit_multiple(ctx, ui_state, vec![(selection, content)], new_undo_group)
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.rope.offset_of_line(line);
        let word_boundary =
            WordCursor::new(&self.rope, line_start_offset).next_non_blank_char();
        let indent = self.rope.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.line_of_offset(offset)
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.rope.offset_of_line(line)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        LogicalLines.offset_to_line_col(&self.rope, offset)
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = LogicalLines.offset_to_line_col(&self.rope, offset);
        Position {
            line: line as u64,
            character: col as u64,
        }
    }

    pub fn offset_of_position(&self, position: &Position) -> Option<usize> {
        let line = position.line as usize;
        if line > self.num_lines() {
            return None;
        }
        let offset = self.offset_of_line(line) + position.character as usize;
        if offset > self.len() {
            return None;
        }
        Some(offset)
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.rope.len()) + 1
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    pub fn get_max_line_len(&self) -> (usize, usize) {
        let mut pre_offset = 0;
        let mut max_len = 0;
        let mut max_len_line = 0;
        for line in 0..self.num_lines() {
            let offset = self.rope.offset_of_line(line);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_len {
                max_len = line_len;
                max_len_line = line;
            }
        }
        (max_len, max_len_line)
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.rope.len())
    }

    pub fn line_max_col(&self, line: usize, include_newline: bool) -> usize {
        match self.offset_of_line(line + 1) - self.offset_of_line(line) {
            n if n == 0 => 0,
            n if n == 1 => 0,
            n => match include_newline {
                true => n - 1,
                false => n - 2,
            },
        }
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        include_newline: bool,
    ) -> usize {
        let max_col = self.line_max_col(line, include_newline);
        match horiz {
            &ColPosition::Col(n) => match max_col > n {
                true => n,
                false => max_col,
            },
            &ColPosition::End => max_col,
            _ => 0,
        }
    }

    pub fn line_end_offset(&self, offset: usize, include_newline: bool) -> usize {
        let line = self.line_of_offset(offset);
        let line_start_offset = self.offset_of_line(line);
        let line_end_offset = self.offset_of_line(line + 1);
        let line_end_offset = if line_end_offset - line_start_offset <= 1 {
            line_start_offset
        } else {
            if include_newline {
                line_end_offset - 1
            } else {
                line_end_offset - 2
            }
        };
        line_end_offset
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let line_start_offset = self.rope.offset_of_line(line);
        WordCursor::new(&self.rope, line_start_offset).next_non_blank_char()
    }

    pub fn word_forward(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_boundary().unwrap()
    }

    pub fn word_end_forward(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).end_boundary().unwrap()
    }

    pub fn word_backword(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_boundary().unwrap()
    }

    pub fn prev_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_code_boundary()
    }

    pub fn next_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_code_boundary()
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    pub fn update_line_layouts(
        &mut self,
        text: &mut PietText,
        line: usize,
        env: &Env,
    ) -> bool {
        // if line >= self.num_lines() {
        //     return false;
        // }

        // let theme = &LAPCE_STATE.theme;

        // let line_hightlight = self.get_line_highligh(line).clone();
        // if self.text_layouts[line].is_none()
        //     || self.text_layouts[line]
        //         .as_ref()
        //         .as_ref()
        //         .unwrap()
        //         .highlights
        //         != line_hightlight
        // {
        //     let line_content = self
        //         .slice_to_cow(
        //             self.offset_of_line(line)..self.offset_of_line(line + 1),
        //         )
        //         .to_string();
        //     self.text_layouts[line] = Arc::new(Some(self.get_text_layout(
        //         text,
        //         theme,
        //         line,
        //         line_content,
        //         env,
        //     )));
        //     return true;
        // }

        false
    }

    pub fn get_text_layout(
        &mut self,
        text: &mut PietText,
        theme: &HashMap<String, Color>,
        line: usize,
        line_content: String,
        env: &Env,
    ) -> HighlightTextLayout {
        let mut layout_builder = text
            .new_text_layout(line_content.clone())
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
        for (start, end, hl) in self.get_line_highligh(line) {
            if let Some(color) = theme.get(hl) {
                layout_builder = layout_builder.range_attribute(
                    start..end,
                    TextAttribute::TextColor(color.clone()),
                );
            }
        }
        let layout = layout_builder.build().unwrap();
        HighlightTextLayout {
            layout,
            text: line_content,
            highlights: self.get_line_highligh(line).clone(),
        }
    }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
    }
}

fn load_file(window_id: &WindowId, tab_id: &WidgetId, path: &str) -> Result<Rope> {
    let state = LAPCE_APP_STATE.get_tab_state(window_id, tab_id);
    let workspace_type = state.workspace.lock().kind.clone();
    let bytes = match workspace_type {
        LapceWorkspaceType::Local => {
            let mut f = File::open(path)?;
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            bytes
        }
        LapceWorkspaceType::RemoteSSH(host) => {
            state.get_ssh_session(&host)?;
            let mut ssh_session = state.ssh_session.lock();
            let ssh_session = ssh_session.as_mut().unwrap();
            ssh_session.read_file(path)?
        }
    };
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}

pub struct WordCursor<'a> {
    inner: Cursor<'a, RopeInfo>,
}

impl<'a> WordCursor<'a> {
    pub fn new(text: &'a Rope, pos: usize) -> WordCursor<'a> {
        let inner = Cursor::new(text, pos);
        WordCursor { inner }
    }

    /// Get previous boundary, and set the cursor at the boundary found.
    pub fn prev_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.prev_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_word_property(prev);
                if classify_boundary(prop_prev, prop).is_start() {
                    break;
                }
                prop = prop_prev;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn next_non_blank_char(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(next) = self.inner.next_codepoint() {
            let prop = get_word_property(next);
            if prop != WordProperty::Space {
                break;
            }
            candidate = self.inner.pos();
        }
        self.inner.set(candidate);
        candidate
    }

    /// Get next boundary, and set the cursor at the boundary found.
    pub fn next_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_word_property(next);
                if classify_boundary(prop, prop_next).is_start() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn end_boundary(&mut self) -> Option<usize> {
        self.inner.next_codepoint();
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_word_property(next);
                if classify_boundary(prop, prop_next).is_end() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate - 1);
        }
        None
    }

    pub fn prev_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.prev_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        return candidate;
    }

    pub fn next_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.next_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        return candidate;
    }

    /// Return the selection for the word containing the current cursor. The
    /// cursor is moved to the end of that selection.
    pub fn select_word(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let init_prop_after = self.inner.next_codepoint().map(get_word_property);
        self.inner.set(initial);
        let init_prop_before = self.inner.prev_codepoint().map(get_word_property);
        let mut start = initial;
        let init_boundary =
            if let (Some(pb), Some(pa)) = (init_prop_before, init_prop_after) {
                classify_boundary_initial(pb, pa)
            } else {
                WordBoundary::Both
            };
        let mut prop_after = init_prop_after;
        let mut prop_before = init_prop_before;
        if prop_after.is_none() {
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if start == initial {
                if init_boundary.is_start() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_start() {
                break;
            }
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        self.inner.set(initial);
        let mut end = initial;
        prop_after = init_prop_after;
        prop_before = init_prop_before;
        if prop_before.is_none() {
            prop_before = self.inner.next_codepoint().map(get_word_property);
            end = self.inner.pos();
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if end == initial {
                if init_boundary.is_end() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_end() {
                break;
            }
            end = self.inner.pos();
            prop_before = prop_after;
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        self.inner.set(end);
        (start, end)
    }
}

#[derive(PartialEq, Eq)]
enum WordBoundary {
    Interior,
    Start, // a boundary indicating the end of a word
    End,   // a boundary indicating the start of a word
    Both,
}

impl WordBoundary {
    fn is_start(&self) -> bool {
        *self == WordBoundary::Start || *self == WordBoundary::Both
    }

    fn is_end(&self) -> bool {
        *self == WordBoundary::End || *self == WordBoundary::Both
    }

    fn is_boundary(&self) -> bool {
        *self != WordBoundary::Interior
    }
}

fn classify_boundary(prev: WordProperty, next: WordProperty) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        (Lf, Lf) => Start,
        (Lf, Space) => Interior,
        (_, Lf) => End,
        (Lf, _) => Start,
        (Space, Space) => Interior,
        (_, Space) => End,
        (Space, _) => Start,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

fn classify_boundary_initial(
    prev: WordProperty,
    next: WordProperty,
) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        (Lf, Other) => Start,
        (Other, Lf) => End,
        (Lf, Space) => Interior,
        (Lf, Punctuation) => Interior,
        (Space, Lf) => Interior,
        (Punctuation, Lf) => Interior,
        (Space, Punctuation) => Interior,
        (Punctuation, Space) => Interior,
        _ => classify_boundary(prev, next),
    }
}

#[derive(Copy, Clone, PartialEq)]
enum WordProperty {
    Lf,
    Space,
    Punctuation,
    Other, // includes letters and all of non-ascii unicode
}

fn get_word_property(codepoint: char) -> WordProperty {
    if codepoint <= ' ' {
        if codepoint == '\n' {
            return WordProperty::Lf;
        }
        return WordProperty::Space;
    } else if codepoint <= '\u{3f}' {
        if (0xfc00fffe00000000u64 >> (codepoint as u32)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    } else if codepoint <= '\u{7f}' {
        // Hardcoded: @[\]^`{|}~
        if (0x7800000178000001u64 >> ((codepoint as u32) & 0x3f)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    }
    WordProperty::Other
}

impl BufferUIState {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        buffer_id: BufferId,
        lines: usize,
        max_len: usize,
        max_len_line: usize,
    ) -> BufferUIState {
        BufferUIState {
            window_id,
            tab_id,
            id: buffer_id,
            text_layouts: vec![Arc::new(None); lines],
            max_len,
            max_len_line,
            dirty: false,
        }
    }

    fn update_text_layouts(&mut self, inval_lines: &InvalLines) {
        let mut new_layouts = Vec::new();
        if inval_lines.start_line < self.text_layouts.len() {
            new_layouts
                .extend_from_slice(&self.text_layouts[..inval_lines.start_line]);
        }
        for _ in 0..inval_lines.new_count {
            new_layouts.push(Arc::new(None));
        }
        if inval_lines.start_line + inval_lines.inval_count < self.text_layouts.len()
        {
            new_layouts.extend_from_slice(
                &self.text_layouts
                    [inval_lines.start_line + inval_lines.inval_count..],
            );
        }
        self.text_layouts = new_layouts;
    }

    pub fn update_line_layouts(
        &mut self,
        text: &mut PietText,
        buffer: &mut Buffer,
        line: usize,
        env: &Env,
    ) -> bool {
        if line >= self.text_layouts.len() {
            return false;
        }

        //let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let theme = &LAPCE_APP_STATE.theme;

        let line_hightlight = buffer.get_line_highligh(line).clone();
        if self.text_layouts[line].is_none()
            || self.text_layouts[line]
                .as_ref()
                .as_ref()
                .unwrap()
                .highlights
                != line_hightlight
        {
            let line_content = buffer
                .slice_to_cow(
                    buffer.offset_of_line(line)..buffer.offset_of_line(line + 1),
                )
                .to_string();
            self.text_layouts[line] = Arc::new(Some(self.get_text_layout(
                text,
                buffer,
                theme,
                line,
                line_content,
                env,
            )));
            return true;
        }

        false
    }

    pub fn get_text_layout(
        &mut self,
        text: &mut PietText,
        buffer: &mut Buffer,
        theme: &HashMap<String, Color>,
        line: usize,
        line_content: String,
        env: &Env,
    ) -> HighlightTextLayout {
        let mut layout_builder = text
            .new_text_layout(line_content.replace('\t', "    "))
            .font(env.get(LapceTheme::EDITOR_FONT).family, 13.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
        let highlights = buffer.get_line_highligh(line);
        for (start, end, hl) in highlights {
            let start = start + &line_content[..*start].matches('\t').count() * 3;
            let end = end + &line_content[..*end].matches('\t').count() * 3;
            if let Some(color) = theme.get(hl) {
                layout_builder = layout_builder.range_attribute(
                    start..end,
                    TextAttribute::TextColor(color.clone()),
                );
            }
        }
        let layout = layout_builder.build().unwrap();
        HighlightTextLayout {
            layout,
            text: line_content,
            highlights: highlights.clone(),
        }
    }
}

pub fn start_buffer_highlights(
    receiver: Receiver<(WindowId, WidgetId, BufferId, u64)>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let mut highlighter = Highlighter::new();
    let mut highlight_configs = HashMap::new();

    loop {
        let (window_id, tab_id, buffer_id, rev) = receiver.recv()?;
        let (language, rope_str) = {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let editor_split = state.editor_split.lock();
            let buffer = editor_split.buffers.get(&buffer_id).unwrap();
            let language = match buffer.language_id.as_str() {
                "rust" => LapceLanguage::Rust,
                "go" => LapceLanguage::Go,
                _ => continue,
            };
            if buffer.rev != rev {
                continue;
            } else {
                (language, buffer.slice_to_cow(..buffer.len()).to_string())
            }
        };

        if !highlight_configs.contains_key(&language) {
            let (highlight_config, highlight_names) = new_highlight_config(language);
            highlight_configs.insert(language, highlight_config);
        }
        let highlight_config = highlight_configs.get(&language).unwrap();

        let mut highlights: Vec<(usize, usize, Highlight)> = Vec::new();
        let mut current_hl: Option<Highlight> = None;
        for hightlight in highlighter
            .highlight(highlight_config, &rope_str.as_bytes(), None, |_| None)
            .unwrap()
        {
            if let Ok(highlight) = hightlight {
                match highlight {
                    HighlightEvent::Source { start, end } => {
                        if let Some(hl) = current_hl {
                            highlights.push((start, end, hl.clone()));
                        }
                    }
                    HighlightEvent::HighlightStart(hl) => {
                        current_hl = Some(hl);
                    }
                    HighlightEvent::HighlightEnd => current_hl = None,
                }
            }
        }

        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let mut editor_split = state.editor_split.lock();
        let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
        if buffer.rev != rev {
            continue;
        }
        buffer.highlights = highlights.to_owned();
        buffer.line_highlights = HashMap::new();

        for (view_id, editor) in editor_split.editors.iter() {
            if editor.buffer_id.as_ref() == Some(&buffer_id) {
                event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(view_id.clone()),
                );
            }
        }
    }
}

//fn highlights_process(
//    language_id: String,
//    receiver: Receiver<u64>,
//    buffer_id: BufferId,
//    event_sink: ExtEventSink,
//) -> Result<()> {
//    let language = match language_id.as_ref() {
//        "rust" => LapceLanguage::Rust,
//        "go" => LapceLanguage::Go,
//        _ => return Ok(()),
//    };
//    let mut highlighter = Highlighter::new();
//    let (highlight_config, highlight_names) = new_highlight_config(language);
//    loop {
//        let rev = receiver.recv()?;
//        let rope_str = {
//            let state = LAPCE_APP_STATE.get_active_state();
//            let editor_split = state.editor_split.lock();
//            let buffer = editor_split.buffers.get(&buffer_id).unwrap();
//            if buffer.rev != rev {
//                continue;
//            } else {
//                buffer.slice_to_cow(..buffer.len()).to_string()
//            }
//        };
//
//        let mut highlights: Vec<(usize, usize, Highlight)> = Vec::new();
//        let mut current_hl: Option<Highlight> = None;
//        for hightlight in highlighter
//            .highlight(&highlight_config, &rope_str.as_bytes(), None, |_| None)
//            .unwrap()
//        {
//            if let Ok(highlight) = hightlight {
//                match highlight {
//                    HighlightEvent::Source { start, end } => {
//                        if let Some(hl) = current_hl {
//                            highlights.push((start, end, hl.clone()));
//                        }
//                    }
//                    HighlightEvent::HighlightStart(hl) => {
//                        current_hl = Some(hl);
//                    }
//                    HighlightEvent::HighlightEnd => current_hl = None,
//                }
//            }
//        }
//
//        let state = LAPCE_APP_STATE.get_active_state();
//        let mut editor_split = state.editor_split.lock();
//        let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
//        if buffer.rev != rev {
//            continue;
//        }
//        buffer.highlights = highlights.to_owned();
//        buffer.line_highlights = HashMap::new();
//
//        for (view_id, editor) in editor_split.editors.iter() {
//            if editor.buffer_id.as_ref() == Some(&buffer_id) {
//                event_sink.submit_command(
//                    LAPCE_UI_COMMAND,
//                    LapceUICommand::FillTextLayouts,
//                    Target::Widget(view_id.clone()),
//                );
//            }
//        }
//    }
//}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use xi_rope::Delta;
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_reverse_delta() {
        let rope = Rope::from_str("0123456789").unwrap();
        let mut builder = DeltaBuilder::new(rope.len());
        builder.replace(3..4, Rope::from_str("a").unwrap());
        let delta1 = builder.build();
        println!("{:?}", delta1);
        let middle_rope = delta1.apply(&rope);

        let mut builder = DeltaBuilder::new(middle_rope.len());
        builder.replace(1..5, Rope::from_str("b").unwrap());
        let delta2 = builder.build();
        println!("{:?}", delta2);
        let new_rope = delta2.apply(&middle_rope);

        let (ins1, del1) = delta1.factor();
        let in1 = ins1.inserted_subset();
        let (ins2, del2) = delta2.factor();
        let in2 = ins2.inserted_subset();

        ins2.transform_expand(&in1, true)
            .inserted_subset()
            .transform_union(&in1);
        // del1.transform_expand(&in1).transform_expand(&del2);
        // let del1 = del1.transform_expand(&in1).transform_expand(&in2);
        // let del2 = del2.transform_expand(&in2);
        // let del = del1.union(&del2);
        let union = ins2.transform_expand(&in1, true).apply(&ins1.apply(&rope));

        println!("{}", union);

        // if delta1.is_simple_delete()
    }
}

fn language_id_from_path(path: &str) -> Option<&str> {
    let path_buf = PathBuf::from_str(path).ok()?;
    Some(match path_buf.extension()?.to_str()? {
        "rs" => "rust",
        "go" => "go",
        _ => return None,
    })
}

pub fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let text = String::from(node);

        let (start, end) = interval.start_end();
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: buffer.offset_to_position(end),
            }),
            range_length: Some((end - start) as u64),
            text,
        };

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = buffer.offset_to_position(end);

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: end_position,
            }),
            range_length: Some((end - start) as u64),
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
}
