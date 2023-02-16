use std::path::PathBuf;

use floem::{
    app::AppContext,
    peniko::{kurbo::Point, Brush, Color},
    text::ParleyBrush,
};
use lapce_core::{
    buffer::{Buffer, InvalLines},
    char_buffer::CharBuffer,
    command::EditCommand,
    cursor::{ColPosition, Cursor, CursorMode},
    editor::Editor,
    mode::Mode,
    movement::{LinePosition, Movement},
    register::{Clipboard, Register},
    selection::{SelRegion, Selection},
    soft_tab::{snap_to_soft_tab, SnapDirection},
    syntax::{edit::SyntaxEdit, Syntax},
    word::WordCursor,
};
use lapce_xi_rope::RopeDelta;
use smallvec::SmallVec;

use crate::config::LapceConfig;

pub struct SystemClipboard {}

impl SystemClipboard {
    fn clipboard() -> floem::glazier::Clipboard {
        floem::glazier::Application::global().clipboard()
    }
}

impl Clipboard for SystemClipboard {
    fn get_string(&self) -> Option<String> {
        Self::clipboard().get_string()
    }

    fn put_string(&mut self, s: impl AsRef<str>) {
        Self::clipboard().put_string(s)
    }
}

pub enum DocContent {
    /// A file at some location. This can be a remote path.
    File(PathBuf),
    /// A local document, which doens't need to be sync to the disk.
    Local,
}

impl DocContent {
    pub fn is_local(&self) -> bool {
        if let DocContent::Local = self {
            true
        } else {
            false
        }
    }
}

pub struct Document {
    pub content: DocContent,
    buffer: Buffer,
    syntax: Option<Syntax>,
}

impl Document {
    pub fn new_local(cx: AppContext) -> Self {
        Self {
            buffer: Buffer::new(""),
            content: DocContent::Local,
            syntax: None,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn do_insert(
        &mut self,
        cursor: &mut Cursor,
        s: &str,
        config: &LapceConfig,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        let old_cursor = cursor.mode.clone();
        let deltas = Editor::insert(
            cursor,
            &mut self.buffer,
            s,
            self.syntax.as_ref(),
            config.editor.auto_closing_matching_pairs,
        );
        // Keep track of the change in the cursor mode for undo/redo
        self.buffer.set_cursor_before(old_cursor);
        self.buffer.set_cursor_after(cursor.mode.clone());
        // self.apply_deltas(&deltas);
        deltas
    }

    pub fn do_edit(
        &mut self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
    ) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
        let mut clipboard = SystemClipboard {};
        let old_cursor = cursor.mode.clone();
        let deltas = Editor::do_edit(
            cursor,
            &mut self.buffer,
            cmd,
            self.syntax.as_ref(),
            &mut clipboard,
            modal,
            register,
        );

        if !deltas.is_empty() {
            self.buffer.set_cursor_before(old_cursor);
            self.buffer.set_cursor_after(cursor.mode.clone());
        }

        self.apply_deltas(&deltas);
        deltas
    }

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines, SyntaxEdit)]) {
        let rev = self.rev() - deltas.len() as u64;
        for (i, (delta, _, _)) in deltas.iter().enumerate() {
            // self.update_styles(delta);
            // self.update_inlay_hints(delta);
            // self.update_diagnostics(delta);
            // self.update_completion(delta);
            // if let BufferContent::File(path) = &self.content {
            //     self.proxy.proxy_rpc.update(
            //         path.clone(),
            //         delta.clone(),
            //         rev + i as u64 + 1,
            //     );
            // }
        }

        // TODO(minor): We could avoid this potential allocation since most apply_delta callers are actually using a Vec
        // which we could reuse.
        // We use a smallvec because there is unlikely to be more than a couple of deltas
        let edits = deltas.iter().map(|(_, _, edits)| edits.clone()).collect();
        self.on_update(Some(edits));
    }

    /// Get the buffer's current revision. This is used to track whether the buffer has changed.
    fn rev(&self) -> u64 {
        self.buffer.rev()
    }

    fn on_update(&mut self, edits: Option<SmallVec<[SyntaxEdit; 3]>>) {
        // self.clear_code_actions();
        // self.find.borrow_mut().unset();
        // *self.find_progress.borrow_mut() = FindProgress::Started;
        // self.get_inlay_hints();
        // self.clear_style_cache();
        // self.trigger_syntax_change(edits);
        // self.get_semantic_styles();
        // self.clear_sticky_headers_cache();
        // self.trigger_head_change();
        // self.notify_special();
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        font_size: usize,
        horiz: &ColPosition,
        caret: bool,
        config: &LapceConfig,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let line_content = self.buffer.line_content(line);
                let mut text_layout_builder =
                    floem::parley::LayoutContext::builder(&line_content[..], 1.0);
                text_layout_builder.push_default(
                    &floem::parley::style::StyleProperty::Brush(ParleyBrush(
                        Brush::Solid(Color::rgb8(0xf0, 0xf0, 0xea)),
                    )),
                );
                let mut text_layout = text_layout_builder.build();
                text_layout
                    .break_all_lines(None, floem::parley::layout::Alignment::Start);
                let cursor = floem::parley::layout::Cursor::from_point(
                    &text_layout,
                    x as f32,
                    0.0,
                );
                let range = cursor.text_range();
                let n = if cursor.is_trailing() {
                    range.end
                } else {
                    range.start
                };

                n.min(self.buffer.line_end_col(line, caret))
            }
            ColPosition::End => self.buffer.line_end_col(line, caret),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.buffer.first_non_blank_character_on_line(line)
            }
        }
    }

    /// Move a selection region by a given movement.  
    /// Much of the time, this will just be a matter of moving the cursor, but
    /// some movements may depend on the current selection.
    fn move_region(
        &self,
        region: &SelRegion,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        config: &LapceConfig,
    ) -> SelRegion {
        let (count, region) = if count >= 1 && !modify && !region.is_caret() {
            // If we're not a caret, and we are moving left/up or right/down, we want to move
            // the cursor to the left or right side of the selection.
            // Ex: `|abc|` -> left/up arrow key -> `|abc`
            // Ex: `|abc|` -> right/down arrow key -> `abc|`
            // and it doesn't matter which direction the selection os going, so we use min/max
            match movement {
                Movement::Left | Movement::Up => {
                    let leftmost = region.min();
                    (count - 1, SelRegion::new(leftmost, leftmost, region.horiz))
                }
                Movement::Right | Movement::Down => {
                    let rightmost = region.max();
                    (
                        count - 1,
                        SelRegion::new(rightmost, rightmost, region.horiz),
                    )
                }
                _ => (count, *region),
            }
        } else {
            (count, *region)
        };

        let (end, horiz) = self.move_offset(
            region.end,
            region.horiz.as_ref(),
            count,
            movement,
            mode,
            config,
        );
        let start = match modify {
            true => region.start,
            false => end,
        };
        SelRegion::new(start, end, horiz)
    }

    pub fn move_selection(
        &self,
        selection: &Selection,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        config: &LapceConfig,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            new_selection.add_region(
                self.move_region(region, count, modify, movement, mode, config),
            );
        }
        new_selection
    }

    pub fn move_offset(
        &self,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
        config: &LapceConfig,
    ) -> (usize, Option<ColPosition>) {
        match movement {
            Movement::Left => {
                let mut new_offset = self.buffer.move_left(offset, mode, count);

                if config.editor.atomic_soft_tabs && config.editor.tab_width > 1 {
                    new_offset = snap_to_soft_tab(
                        &self.buffer,
                        new_offset,
                        SnapDirection::Left,
                        config.editor.tab_width,
                    );
                }

                (new_offset, None)
            }
            Movement::Right => {
                let mut new_offset = self.buffer.move_right(offset, mode, count);

                if config.editor.atomic_soft_tabs && config.editor.tab_width > 1 {
                    new_offset = snap_to_soft_tab(
                        &self.buffer,
                        new_offset,
                        SnapDirection::Right,
                        config.editor.tab_width,
                    );
                }

                (new_offset, None)
            }
            Movement::Up => {
                let line = self.buffer.line_of_offset(offset);
                if line == 0 {
                    let line = self.buffer.line_of_offset(offset);
                    let new_offset = self.buffer.offset_of_line(line);
                    return (new_offset, Some(ColPosition::Start));
                }

                let line = line.saturating_sub(count);
                let font_size = config.editor.font_size;

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.line_point_of_offset(offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Down => {
                let last_line = self.buffer.last_line();
                let line = self.buffer.line_of_offset(offset);
                if line == last_line {
                    let new_offset =
                        self.buffer.offset_line_end(offset, mode != Mode::Normal);
                    return (new_offset, Some(ColPosition::End));
                }

                let line = line + count;
                let font_size = config.editor.font_size;

                let line = line.min(last_line);

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.line_point_of_offset(offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::DocumentStart => (0, Some(ColPosition::Start)),
            Movement::DocumentEnd => {
                let last_offset = self
                    .buffer
                    .offset_line_end(self.buffer.len(), mode != Mode::Normal);
                (last_offset, Some(ColPosition::End))
            }
            Movement::FirstNonBlank => {
                let line = self.buffer.line_of_offset(offset);
                let non_blank_offset =
                    self.buffer.first_non_blank_character_on_line(line);
                let start_line_offset = self.buffer.offset_of_line(line);
                if offset > non_blank_offset {
                    // Jump to the first non-whitespace character if we're strictly after it
                    (non_blank_offset, Some(ColPosition::FirstNonBlank))
                } else {
                    // If we're at the start of the line, also jump to the first not blank
                    if start_line_offset == offset {
                        (non_blank_offset, Some(ColPosition::FirstNonBlank))
                    } else {
                        // Otherwise, jump to the start of the line
                        (start_line_offset, Some(ColPosition::Start))
                    }
                }
            }
            Movement::StartOfLine => {
                let line = self.buffer.line_of_offset(offset);
                let new_offset = self.buffer.offset_of_line(line);
                (new_offset, Some(ColPosition::Start))
            }
            Movement::EndOfLine => {
                let new_offset =
                    self.buffer.offset_line_end(offset, mode != Mode::Normal);
                (new_offset, Some(ColPosition::End))
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => {
                        (line - 1).min(self.buffer.last_line())
                    }
                    LinePosition::First => 0,
                    LinePosition::Last => self.buffer.last_line(),
                };
                let font_size = config.editor.font_size;
                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.line_point_of_offset(offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let new_offset = self
                    .buffer
                    .text()
                    .prev_grapheme_offset(new_offset + 1)
                    .unwrap();
                (new_offset, None)
            }
            Movement::WordEndForward => {
                let new_offset = self.buffer.move_n_wordends_forward(
                    offset,
                    count,
                    mode == Mode::Insert,
                );
                (new_offset, None)
            }
            Movement::WordForward => {
                let new_offset = self.buffer.move_n_words_forward(offset, count);
                (new_offset, None)
            }
            Movement::WordBackward => {
                let new_offset =
                    self.buffer.move_n_words_backward(offset, count, mode);
                (new_offset, None)
            }
            Movement::NextUnmatched(char) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, false, &CharBuffer::from(char))
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .next_unmatched(*char)
                        .map_or(offset, |new| new - 1);
                    (new_offset, None)
                }
            }
            Movement::PreviousUnmatched(char) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, true, &CharBuffer::from(char))
                        .unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .previous_unmatched(*char)
                        .unwrap_or(offset);
                    (new_offset, None)
                }
            }
            Movement::MatchPairs => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset =
                        syntax.find_matching_pair(offset).unwrap_or(offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .match_pairs()
                        .unwrap_or(offset);
                    (new_offset, None)
                }
            }
            Movement::ParagraphForward => {
                let new_offset =
                    self.buffer.move_n_paragraphs_forward(offset, count);
                (new_offset, None)
            }
            Movement::ParagraphBackward => {
                let new_offset =
                    self.buffer.move_n_paragraphs_backward(offset, count);
                (new_offset, None)
            }
        }
    }

    pub fn move_cursor(
        &mut self,
        cursor: &mut Cursor,
        movement: &Movement,
        count: usize,
        modify: bool,
        register: &mut Register,
        config: &LapceConfig,
    ) {
        match cursor.mode {
            CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.move_offset(
                    offset,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                    config,
                );
                if let Some(motion_mode) = cursor.motion_mode.clone() {
                    let (moved_new_offset, _) = self.move_offset(
                        new_offset,
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                        config,
                    );
                    let (start, end) = match movement {
                        Movement::EndOfLine | Movement::WordEndForward => {
                            (offset, moved_new_offset)
                        }
                        Movement::MatchPairs => {
                            if new_offset > offset {
                                (offset, moved_new_offset)
                            } else {
                                (moved_new_offset, new_offset)
                            }
                        }
                        _ => (offset, new_offset),
                    };
                    let deltas = Editor::execute_motion_mode(
                        cursor,
                        &mut self.buffer,
                        motion_mode,
                        start,
                        end,
                        movement.is_vertical(),
                        register,
                    );
                    self.apply_deltas(&deltas);
                    cursor.motion_mode = None;
                } else {
                    cursor.mode = CursorMode::Normal(new_offset);
                    cursor.horiz = horiz;
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.move_offset(
                    end,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                    config,
                );
                cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                cursor.horiz = horiz;
            }
            CursorMode::Insert(ref selection) => {
                let selection = self.move_selection(
                    selection,
                    count,
                    modify,
                    movement,
                    Mode::Insert,
                    config,
                );
                cursor.set_insert(selection);
            }
        }
    }

    /// Returns the point into the text layout of the line at the given offset.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_offset(
        &self,
        offset: usize,
        font_size: usize,
        config: &LapceConfig,
    ) -> Point {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        self.line_point_of_line_col(line, col, font_size, config)
    }

    /// Returns the point into the text layout of the line at the given line and column.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_line_col(
        &self,
        line: usize,
        col: usize,
        font_size: usize,
        config: &LapceConfig,
    ) -> Point {
        let line_content = self.buffer.line_content(line);
        let mut text_layout_builder =
            floem::parley::LayoutContext::builder(&line_content[..col], 1.0);
        text_layout_builder.push_default(
            &floem::parley::style::StyleProperty::Brush(ParleyBrush(Brush::Solid(
                Color::rgb8(0xf0, 0xf0, 0xea),
            ))),
        );
        let mut text_layout = text_layout_builder.build();
        text_layout.break_all_lines(None, floem::parley::layout::Alignment::Start);
        let cursor =
            floem::parley::layout::Cursor::from_position(&text_layout, col, true);
        Point::new(cursor.offset() as f64, cursor.baseline() as f64)
    }
}
