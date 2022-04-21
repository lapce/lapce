use serde::{Deserialize, Serialize};

use crate::buffer::Buffer;
use crate::mode::Mode;
use crate::movement::Movement;
use crate::selection::{ColPosition, SelRegion, Selection};
use crate::syntax::Syntax;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub mode: CursorMode,
    pub horiz: Option<ColPosition>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy, Deserialize, Serialize)]
pub enum VisualMode {
    Normal,
    Linewise,
    Blockwise,
}

impl Default for VisualMode {
    fn default() -> Self {
        VisualMode::Normal
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CursorMode {
    Normal(usize),
    Visual {
        start: usize,
        end: usize,
        mode: VisualMode,
    },
    Insert(Selection),
}

impl Cursor {
    pub fn new(mode: CursorMode, horiz: Option<ColPosition>) -> Self {
        Self { mode, horiz }
    }

    pub fn offset(&self) -> usize {
        match &self.mode {
            CursorMode::Normal(offset) => *offset,
            CursorMode::Visual { end, .. } => *end,
            CursorMode::Insert(selection) => selection.get_cursor_offset(),
        }
    }

    pub fn get_mode(&self) -> Mode {
        match &self.mode {
            CursorMode::Normal(_) => Mode::Normal,
            CursorMode::Visual { .. } => Mode::Visual,
            CursorMode::Insert(_) => Mode::Insert,
        }
    }

    pub fn do_movement(
        &mut self,
        movement: &Movement,
        count: usize,
        modify: bool,
        buffer: &Buffer,
        syntax: Option<&Syntax>,
    ) {
        match self.mode {
            CursorMode::Normal(offset) => {
                let (new_offset, horiz) = buffer.move_offset(
                    offset,
                    self.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                    syntax,
                );
                self.mode = CursorMode::Normal(new_offset);
                self.horiz = Some(horiz);
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = buffer.move_offset(
                    end,
                    self.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                    syntax,
                );
                self.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                self.horiz = Some(horiz);
            }
            CursorMode::Insert(ref selection) => {
                let selection = selection.do_movement(
                    movement,
                    Mode::Insert,
                    count,
                    modify,
                    buffer,
                    syntax,
                    self.horiz.as_ref(),
                );
                self.mode = CursorMode::Insert(selection);
            }
        }
    }

    pub fn edit_selection(&self, buffer: &Buffer) -> Selection {
        match &self.mode {
            CursorMode::Insert(selection) => selection.clone(),
            CursorMode::Normal(offset) => Selection::region(
                *offset,
                buffer.next_grapheme_offset(*offset, 1, buffer.len()),
            ),
            CursorMode::Visual { start, end, mode } => match mode {
                VisualMode::Normal => Selection::region(
                    *start.min(end),
                    buffer.next_grapheme_offset(*start.max(end), 1, buffer.len()),
                ),
                VisualMode::Linewise => {
                    let start_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.min(end)));
                    let end_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.max(end)) + 1);
                    Selection::region(start_offset, end_offset)
                }
                VisualMode::Blockwise => {
                    let mut selection = Selection::new();
                    let (start_line, start_col) =
                        buffer.offset_to_line_col(*start.min(end));
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end));
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true);
                        if left > max_col {
                            continue;
                        }
                        let right = match &self.horiz {
                            Some(ColPosition::End) => max_col,
                            _ => {
                                if right > max_col {
                                    max_col
                                } else {
                                    right
                                }
                            }
                        };
                        let left = buffer.offset_of_line_col(line, left);
                        let right = buffer.offset_of_line_col(line, right);
                        selection.add_region(SelRegion::new(left, right));
                    }
                    selection
                }
            },
        }
    }
}
