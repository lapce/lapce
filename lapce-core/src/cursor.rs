use lapce_xi_rope::{RopeDelta, Transformer};
use serde::{Deserialize, Serialize};

use crate::{
    buffer::Buffer,
    mode::{Mode, MotionMode, VisualMode},
    register::RegisterData,
    selection::{InsertDrift, SelRegion, Selection},
};

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ColPosition {
    FirstNonBlank,
    Start,
    End,
    Col(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub mode: CursorMode,
    pub horiz: Option<ColPosition>,
    pub motion_mode: Option<MotionMode>,
    pub history_selections: Vec<Selection>,
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

impl CursorMode {
    pub fn offset(&self) -> usize {
        match &self {
            CursorMode::Normal(offset) => *offset,
            CursorMode::Visual { end, .. } => *end,
            CursorMode::Insert(selection) => selection.get_cursor_offset(),
        }
    }
}

impl Cursor {
    pub fn new(
        mode: CursorMode,
        horiz: Option<ColPosition>,
        motion_mode: Option<MotionMode>,
    ) -> Self {
        Self {
            mode,
            horiz,
            motion_mode,
            history_selections: Vec::new(),
        }
    }

    pub fn offset(&self) -> usize {
        self.mode.offset()
    }

    pub fn is_normal(&self) -> bool {
        matches!(&self.mode, CursorMode::Normal(_))
    }

    pub fn is_insert(&self) -> bool {
        matches!(&self.mode, CursorMode::Insert(_))
    }

    pub fn is_visual(&self) -> bool {
        matches!(&self.mode, CursorMode::Visual { .. })
    }

    pub fn get_mode(&self) -> Mode {
        match &self.mode {
            CursorMode::Normal(_) => Mode::Normal,
            CursorMode::Visual { .. } => Mode::Visual,
            CursorMode::Insert(_) => Mode::Insert,
        }
    }

    pub fn set_mode(&mut self, mode: CursorMode) {
        if let CursorMode::Insert(selection) = &self.mode {
            self.history_selections.push(selection.clone());
        }
        self.mode = mode;
    }

    pub fn set_insert(&mut self, selection: Selection) {
        self.set_mode(CursorMode::Insert(selection));
    }

    pub fn update_selection(&mut self, buffer: &Buffer, selection: Selection) {
        match self.mode {
            CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                let offset = selection.min_offset();
                let offset = buffer.offset_line_end(offset, false).min(offset);
                self.mode = CursorMode::Normal(offset);
            }
            CursorMode::Insert(_) => {
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
                        selection.add_region(SelRegion::new(left, right, None));
                    }
                    selection
                }
            },
        }
    }

    pub fn apply_delta(&mut self, delta: &RopeDelta) {
        match &self.mode {
            CursorMode::Normal(offset) => {
                let mut transformer = Transformer::new(delta);
                let new_offset = transformer.transform(*offset, true);
                self.mode = CursorMode::Normal(new_offset);
            }
            CursorMode::Visual { start, end, mode } => {
                let mut transformer = Transformer::new(delta);
                let start = transformer.transform(*start, false);
                let end = transformer.transform(*end, true);
                self.mode = CursorMode::Visual {
                    start,
                    end,
                    mode: *mode,
                };
            }
            CursorMode::Insert(selection) => {
                let selection =
                    selection.apply_delta(delta, true, InsertDrift::Default);
                self.mode = CursorMode::Insert(selection);
            }
        }
        self.horiz = None;
    }

    pub fn yank(&self, buffer: &Buffer) -> RegisterData {
        let (content, mode) = match &self.mode {
            CursorMode::Insert(selection) => {
                let mut mode = VisualMode::Normal;
                let mut content = "".to_string();
                for region in selection.regions() {
                    let region_content = if region.is_caret() {
                        mode = VisualMode::Linewise;
                        let line = buffer.line_of_offset(region.start);
                        buffer.line_content(line)
                    } else {
                        buffer.slice_to_cow(region.min()..region.max())
                    };
                    if content.is_empty() {
                        content = region_content.to_string();
                    } else if content.ends_with('\n') {
                        content += &region_content;
                    } else {
                        content += "\n";
                        content += &region_content;
                    }
                }
                (content, mode)
            }
            CursorMode::Normal(offset) => {
                let new_offset =
                    buffer.next_grapheme_offset(*offset, 1, buffer.len());
                (
                    buffer.slice_to_cow(*offset..new_offset).to_string(),
                    VisualMode::Normal,
                )
            }
            CursorMode::Visual { start, end, mode } => match mode {
                VisualMode::Normal => (
                    buffer
                        .slice_to_cow(
                            *start.min(end)
                                ..buffer.next_grapheme_offset(
                                    *start.max(end),
                                    1,
                                    buffer.len(),
                                ),
                        )
                        .to_string(),
                    VisualMode::Normal,
                ),
                VisualMode::Linewise => {
                    let start_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.min(end)));
                    let end_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.max(end)) + 1);
                    (
                        buffer.slice_to_cow(start_offset..end_offset).to_string(),
                        VisualMode::Linewise,
                    )
                }
                VisualMode::Blockwise => {
                    let mut lines = Vec::new();
                    let (start_line, start_col) =
                        buffer.offset_to_line_col(*start.min(end));
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end));
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true);
                        if left > max_col {
                            lines.push("".to_string());
                        } else {
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
                            lines.push(buffer.slice_to_cow(left..right).to_string());
                        }
                    }
                    (lines.join("\n") + "\n", VisualMode::Blockwise)
                }
            },
        };
        RegisterData { content, mode }
    }

    /// Return the current selection start and end position for a
    /// Single cursor selection
    pub fn get_selection(&self) -> Option<(usize, usize)> {
        match &self.mode {
            CursorMode::Visual {
                start,
                end,
                mode: _,
            } => Some((*start, *end)),
            CursorMode::Insert(selection) => selection
                .regions()
                .first()
                .map(|region| (region.start, region.end)),
            _ => None,
        }
    }

    pub fn get_line_col_char(
        &self,
        buffer: &Buffer,
    ) -> Option<(usize, usize, usize)> {
        match &self.mode {
            CursorMode::Normal(offset) => {
                let ln_col = buffer.offset_to_line_col(*offset);
                Some((ln_col.0, ln_col.1, *offset))
            }
            CursorMode::Visual {
                start,
                end,
                mode: _,
            } => {
                let v = buffer.offset_to_line_col(*start.min(end));
                Some((v.0, v.1, *start))
            }
            CursorMode::Insert(selection) => {
                if selection.regions().len() > 1 {
                    return None;
                }

                let x = selection.regions().get(0).unwrap();
                let v = buffer.offset_to_line_col(x.start);

                Some((v.0, v.1, x.start))
            }
        }
    }

    pub fn get_selection_count(&self) -> usize {
        match &self.mode {
            CursorMode::Insert(selection) => selection.regions().len(),
            _ => 0,
        }
    }

    pub fn set_offset(&mut self, offset: usize, modify: bool, new_cursor: bool) {
        match &self.mode {
            CursorMode::Normal(old_offset) => {
                if modify && *old_offset != offset {
                    self.mode = CursorMode::Visual {
                        start: *old_offset,
                        end: offset,
                        mode: VisualMode::Normal,
                    };
                } else {
                    self.mode = CursorMode::Normal(offset);
                }
            }
            CursorMode::Visual {
                start,
                end: _,
                mode: _,
            } => {
                if modify {
                    self.mode = CursorMode::Visual {
                        start: *start,
                        end: offset,
                        mode: VisualMode::Normal,
                    };
                } else {
                    self.mode = CursorMode::Normal(offset);
                }
            }
            CursorMode::Insert(selection) => {
                if new_cursor {
                    let mut new_selection = selection.clone();
                    if modify {
                        if let Some(region) = new_selection.last_inserted_mut() {
                            region.end = offset;
                        } else {
                            new_selection.add_region(SelRegion::caret(offset));
                        }
                        self.set_insert(new_selection);
                    } else {
                        let mut new_selection = selection.clone();
                        new_selection.add_region(SelRegion::caret(offset));
                        self.set_insert(new_selection);
                    }
                } else if modify {
                    let mut new_selection = Selection::new();
                    if let Some(region) = selection.first() {
                        let new_region = SelRegion::new(region.start, offset, None);
                        new_selection.add_region(new_region);
                    } else {
                        new_selection
                            .add_region(SelRegion::new(offset, offset, None));
                    }
                    self.set_insert(new_selection);
                } else {
                    self.set_insert(Selection::caret(offset));
                }
            }
        }
    }

    pub fn add_region(
        &mut self,
        start: usize,
        end: usize,
        modify: bool,
        new_cursor: bool,
    ) {
        match &self.mode {
            CursorMode::Normal(_offset) => {
                self.mode = CursorMode::Visual {
                    start,
                    end: end - 1,
                    mode: VisualMode::Normal,
                };
            }
            CursorMode::Visual {
                start: old_start,
                end: old_end,
                mode: _,
            } => {
                let forward = old_end >= old_start;
                let new_start = (*old_start).min(*old_end).min(start).min(end - 1);
                let new_end = (*old_start).max(*old_end).max(start).max(end - 1);
                let (new_start, new_end) = if forward {
                    (new_start, new_end)
                } else {
                    (new_end, new_start)
                };
                self.mode = CursorMode::Visual {
                    start: new_start,
                    end: new_end,
                    mode: VisualMode::Normal,
                };
            }
            CursorMode::Insert(selection) => {
                let new_selection = if new_cursor {
                    let mut new_selection = selection.clone();
                    if modify {
                        let new_region =
                            if let Some(last_inserted) = selection.last_inserted() {
                                last_inserted
                                    .merge_with(SelRegion::new(start, end, None))
                            } else {
                                SelRegion::new(start, end, None)
                            };
                        new_selection.replace_last_inserted_region(new_region);
                    } else {
                        new_selection.add_region(SelRegion::new(start, end, None));
                    }
                    new_selection
                } else if modify {
                    let mut new_selection = selection.clone();
                    new_selection.add_region(SelRegion::new(start, end, None));
                    new_selection
                } else {
                    Selection::region(start, end)
                };
                self.mode = CursorMode::Insert(new_selection);
            }
        }
    }
}

pub fn get_first_selection_after(
    cursor: &Cursor,
    buffer: &Buffer,
    delta: &RopeDelta,
) -> Option<Cursor> {
    let mut transformer = Transformer::new(delta);

    let offset = cursor.offset();
    let offset = transformer.transform(offset, false);
    let (ins, del) = delta.clone().factor();
    let ins = ins.transform_shrink(&del);
    for el in ins.els.iter() {
        match el {
            lapce_xi_rope::DeltaElement::Copy(b, e) => {
                // if b == e, ins.inserted_subset() will panic
                if b == e {
                    return None;
                }
            }
            lapce_xi_rope::DeltaElement::Insert(_) => {}
        }
    }

    // TODO it's silly to store the whole thing in memory, we only need the first element.
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

    positions
        .first()
        .cloned()
        .map(Selection::caret)
        .map(|selection| {
            let cursor_mode = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                    let offset = selection.min_offset();
                    let offset = buffer.offset_line_end(offset, false).min(offset);
                    CursorMode::Normal(offset)
                }
                CursorMode::Insert(_) => CursorMode::Insert(selection),
            };

            Cursor::new(cursor_mode, None, None)
        })
}
