use std::collections::HashSet;

use itertools::Itertools;
use xi_rope::RopeDelta;

use crate::{
    buffer::{Buffer, InvalLines},
    command::EditCommand,
    cursor::{get_first_selection_after, Cursor, CursorMode},
    mode::{Mode, MotionMode, VisualMode},
    register::{Clipboard, Register, RegisterData, RegisterKind},
    selection::{InsertDrift, SelRegion, Selection},
    syntax::{
        has_unmatched_pair, matching_char, matching_pair_direction,
        str_is_pair_left, str_matching_pair, Syntax,
    },
    word::{get_word_property, WordProperty},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditType {
    Other,
    InsertChars,
    InsertNewline,
    Delete,
    Undo,
    Redo,
}

impl EditType {
    /// Checks whether a new undo group should be created between two edits.
    pub fn breaks_undo_group(self, previous: EditType) -> bool {
        self == EditType::Other || self != previous
    }
}

pub struct Editor {}

impl Editor {
    pub fn insert(
        cursor: &mut Cursor,
        buffer: &mut Buffer,
        s: &str,
        syntax: Option<&Syntax>,
    ) -> Vec<(RopeDelta, InvalLines)> {
        let mut deltas = Vec::new();
        if let CursorMode::Insert(selection) = &cursor.mode {
            if s.chars().count() != 1 {
                let (delta, inval_lines) =
                    buffer.edit(&[(selection, s)], EditType::InsertChars);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                deltas.push((delta, inval_lines));
                cursor.mode = CursorMode::Insert(selection);
            } else {
                let c = s.chars().next().unwrap();
                let matching_pair_type = matching_pair_direction(c);

                // The main edit operations
                let mut edits = vec![];

                // "Late edits" - characters to be inserted after particular regions
                let mut edits_after = vec![];

                let mut selection = selection.clone();
                for (idx, region) in selection.regions_mut().iter_mut().enumerate() {
                    let offset = region.end;
                    let cursor_char = buffer.char_at_offset(offset);

                    if matching_pair_type == Some(false) {
                        if cursor_char == Some(c) {
                            // Skip the closing character
                            let new_offset =
                                buffer.next_grapheme_offset(offset, 1, buffer.len());

                            *region = SelRegion::caret(new_offset);
                            continue;
                        }

                        let line = buffer.line_of_offset(offset);
                        let line_start = buffer.offset_of_line(line);
                        if buffer.slice_to_cow(line_start..offset).trim() == "" {
                            let opening_character = matching_char(c).unwrap();
                            if let Some(previous_offset) = buffer.previous_unmatched(
                                syntax,
                                opening_character,
                                offset,
                            ) {
                                // Auto-indent closing character to the same level as the opening.
                                let previous_line =
                                    buffer.line_of_offset(previous_offset);
                                let line_indent =
                                    buffer.indent_on_line(previous_line);

                                let current_selection =
                                    Selection::region(line_start, offset);

                                edits.push((
                                    current_selection,
                                    format!("{line_indent}{c}"),
                                ));
                                continue;
                            }
                        }
                    }

                    if matching_pair_type == Some(true) {
                        // Create a late edit to insert the closing pair, if allowed.
                        let is_whitespace_or_punct = cursor_char
                            .map(|c| {
                                let prop = get_word_property(c);
                                prop == WordProperty::Lf
                                    || prop == WordProperty::Space
                                    || prop == WordProperty::Punctuation
                            })
                            .unwrap_or(true);

                        if is_whitespace_or_punct {
                            let insert_after = matching_char(c).unwrap();
                            edits_after.push((idx, insert_after));
                        }
                    };

                    let current_selection =
                        Selection::region(region.start, region.end);

                    edits.push((current_selection, c.to_string()));
                }

                // Apply edits to current selection
                let edits = edits
                    .iter()
                    .map(|(selection, content)| (selection, content.as_str()))
                    .collect::<Vec<_>>();

                let (delta, inval_lines) =
                    buffer.edit(&edits, EditType::InsertChars);

                buffer.set_cursor_before(CursorMode::Insert(selection.clone()));

                // Update selection
                let mut selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);

                buffer.set_cursor_after(CursorMode::Insert(selection.clone()));

                deltas.push((delta, inval_lines));
                // Apply late edits
                let edits_after = edits_after
                    .iter()
                    .map(|(idx, content)| {
                        let region = &selection.regions()[*idx];
                        (
                            Selection::region(region.start, region.end),
                            content.to_string(),
                        )
                    })
                    .collect::<Vec<_>>();

                let edits_after = edits_after
                    .iter()
                    .map(|(selection, content)| (selection, content.as_str()))
                    .collect::<Vec<_>>();

                if !edits_after.is_empty() {
                    let (delta, inval_lines) =
                        buffer.edit(&edits_after, EditType::InsertChars);
                    deltas.push((delta, inval_lines));
                }

                // Adjust selection according to previous late edits
                let mut adjustment = 0;
                for region in selection.regions_mut().iter_mut().sorted_by(
                    |region_a, region_b| region_a.start().cmp(&region_b.start()),
                ) {
                    *region = SelRegion::new(
                        region.start + adjustment,
                        region.end + adjustment,
                        None,
                    );

                    if let Some(inserted) =
                        edits_after.iter().find_map(|(selection, str)| {
                            if selection.last_inserted().map(|r| r.start())
                                == Some(region.start())
                            {
                                Some(str)
                            } else {
                                None
                            }
                        })
                    {
                        adjustment += inserted.len();
                    }
                }

                cursor.mode = CursorMode::Insert(selection);
            }
        }
        deltas
    }

    fn toggle_visual(cursor: &mut Cursor, visual_mode: VisualMode, modal: bool) {
        if !modal {
            return;
        }

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

    fn insert_new_line(
        buffer: &mut Buffer,
        cursor: &mut Cursor,
        selection: Selection,
    ) -> Vec<(RopeDelta, InvalLines)> {
        let mut deltas = Vec::new();
        let mut edits = Vec::new();
        let mut extra_edits = Vec::new();
        let mut shift = 0i32;
        for region in selection.regions() {
            let offset = region.max();
            let line = buffer.line_of_offset(offset);
            let line_start = buffer.offset_of_line(line);
            let line_end = buffer.line_end_offset(line, true);
            let line_indent = buffer.indent_on_line(line);
            let first_half = buffer.slice_to_cow(line_start..offset).to_string();
            let second_half = buffer.slice_to_cow(offset..line_end).to_string();

            let indent = if has_unmatched_pair(&first_half) {
                format!("{}{}", line_indent, buffer.indent_unit())
            } else if second_half.trim().is_empty() {
                let next_line_indent = buffer.indent_on_line(line + 1);
                if next_line_indent.len() > line_indent.len() {
                    next_line_indent
                } else {
                    line_indent.clone()
                }
            } else {
                line_indent.clone()
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
        let (delta, inval_lines) = buffer.edit(&edits, EditType::InsertNewline);
        let mut selection =
            selection.apply_delta(&delta, true, InsertDrift::Default);
        deltas.push((delta, inval_lines));

        if !extra_edits.is_empty() {
            let edits = extra_edits
                .iter()
                .map(|(selection, s)| (selection, s.as_str()))
                .collect::<Vec<(&Selection, &str)>>();
            let (delta, inval_lines) = buffer.edit(&edits, EditType::InsertNewline);
            selection = selection.apply_delta(&delta, false, InsertDrift::Default);
            deltas.push((delta, inval_lines));
        }

        cursor.mode = CursorMode::Insert(selection);

        deltas
    }

    pub fn execute_motion_mode(
        cursor: &mut Cursor,
        buffer: &mut Buffer,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    ) -> Vec<(RopeDelta, InvalLines)> {
        fn format_start_end(
            buffer: &Buffer,
            start: usize,
            end: usize,
            is_vertical: bool,
        ) -> (usize, usize) {
            if is_vertical {
                let start_line = buffer.line_of_offset(start.min(end));
                let end_line = buffer.line_of_offset(end.max(start));
                let start = buffer.offset_of_line(start_line);
                let end = buffer.offset_of_line(end_line + 1);
                (start, end)
            } else {
                let s = start.min(end);
                let e = start.max(end);
                (s, e)
            }
        }

        let mut deltas = Vec::new();
        match motion_mode {
            MotionMode::Delete => {
                let (start, end) = format_start_end(buffer, start, end, is_vertical);
                register.add(
                    RegisterKind::Delete,
                    RegisterData {
                        content: buffer.slice_to_cow(start..end).to_string(),
                        mode: if is_vertical {
                            VisualMode::Linewise
                        } else {
                            VisualMode::Normal
                        },
                    },
                );
                let selection = Selection::region(start, end);
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                cursor.apply_delta(&delta);
                deltas.push((delta, inval_lines));
            }
            MotionMode::Yank => {
                let (start, end) = format_start_end(buffer, start, end, is_vertical);
                register.add(
                    RegisterKind::Yank,
                    RegisterData {
                        content: buffer.slice_to_cow(start..end).to_string(),
                        mode: if is_vertical {
                            VisualMode::Linewise
                        } else {
                            VisualMode::Normal
                        },
                    },
                );
            }
            MotionMode::Indent => {
                let selection = Selection::region(start, end);
                let (delta, inval_lines) = Self::do_indent(buffer, selection);
                deltas.push((delta, inval_lines));
            }
            MotionMode::Outdent => {
                let selection = Selection::region(start, end);
                let (delta, inval_lines) = Self::do_outdent(buffer, selection);
                deltas.push((delta, inval_lines));
            }
        }
        deltas
    }

    pub fn do_paste(
        cursor: &mut Cursor,
        buffer: &mut Buffer,
        data: &RegisterData,
    ) -> Vec<(RopeDelta, InvalLines)> {
        let mut deltas = Vec::new();
        match data.mode {
            VisualMode::Normal => {
                let selection = match cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line_end = buffer.offset_line_end(offset, true);
                        let offset = (offset + 1).min(line_end);
                        Selection::caret(offset)
                    }
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                };
                let after = cursor.is_insert() || !data.content.contains('\n');
                let (delta, inval_lines) = buffer
                    .edit(&[(&selection, &data.content)], EditType::InsertChars);
                let selection =
                    selection.apply_delta(&delta, after, InsertDrift::Default);
                deltas.push((delta, inval_lines));
                if !after {
                    cursor.update_selection(buffer, selection);
                } else {
                    match cursor.mode {
                        CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                            let offset = buffer.prev_grapheme_offset(
                                selection.min_offset(),
                                1,
                                0,
                            );
                            cursor.mode = CursorMode::Normal(offset);
                        }
                        CursorMode::Insert { .. } => {
                            cursor.mode = CursorMode::Insert(selection);
                        }
                    }
                }
            }
            VisualMode::Linewise | VisualMode::Blockwise => {
                let (selection, content) = match &cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line = buffer.line_of_offset(*offset);
                        let offset = buffer.offset_of_line(line + 1);
                        (Selection::caret(offset), data.content.clone())
                    }
                    CursorMode::Insert(selection) => {
                        let mut selection = selection.clone();
                        for region in selection.regions_mut() {
                            if region.is_caret() {
                                let line = buffer.line_of_offset(region.start);
                                let start = buffer.offset_of_line(line);
                                region.start = start;
                                region.end = start;
                            }
                        }
                        (selection, data.content.clone())
                    }
                    CursorMode::Visual { mode, .. } => {
                        let selection = cursor.edit_selection(buffer);
                        let data = match mode {
                            VisualMode::Linewise => data.content.clone(),
                            _ => "\n".to_string() + &data.content,
                        };
                        (selection, data)
                    }
                };
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, &content)], EditType::InsertChars);
                let selection = selection.apply_delta(
                    &delta,
                    cursor.is_insert(),
                    InsertDrift::Default,
                );
                deltas.push((delta, inval_lines));
                match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset = if cursor.is_visual() {
                            offset + 1
                        } else {
                            offset
                        };
                        let line = buffer.line_of_offset(offset);
                        let offset = buffer.first_non_blank_character_on_line(line);
                        cursor.mode = CursorMode::Normal(offset);
                    }
                    CursorMode::Insert(_) => {
                        cursor.mode = CursorMode::Insert(selection);
                    }
                }
            }
        }
        deltas
    }

    fn do_indent(
        buffer: &mut Buffer,
        selection: Selection,
    ) -> (RopeDelta, InvalLines) {
        let indent = buffer.indent_unit();
        let mut edits = Vec::new();

        let mut lines = HashSet::new();
        for region in selection.regions() {
            let start_line = buffer.line_of_offset(region.min());
            let mut end_line = buffer.line_of_offset(region.max());
            if end_line > start_line {
                let end_line_start = buffer.offset_of_line(end_line);
                if end_line_start == region.max() {
                    end_line -= 1;
                }
            }
            for line in start_line..=end_line {
                if lines.contains(&line) {
                    continue;
                }
                lines.insert(line);
                let line_content = buffer.line_content(line);
                if line_content == "\n" || line_content == "\r\n" {
                    continue;
                }
                let nonblank = buffer.first_non_blank_character_on_line(line);
                let edit = crate::indent::create_edit(buffer, nonblank, indent);
                edits.push(edit);
            }
        }

        buffer.edit(&edits, EditType::InsertChars)
    }

    fn do_outdent(
        buffer: &mut Buffer,
        selection: Selection,
    ) -> (RopeDelta, InvalLines) {
        let indent = buffer.indent_unit();
        let mut edits = Vec::new();

        let mut lines = HashSet::new();
        for region in selection.regions() {
            let start_line = buffer.line_of_offset(region.min());
            let mut end_line = buffer.line_of_offset(region.max());
            if end_line > start_line {
                let end_line_start = buffer.offset_of_line(end_line);
                if end_line_start == region.max() {
                    end_line -= 1;
                }
            }
            for line in start_line..=end_line {
                if lines.contains(&line) {
                    continue;
                }
                lines.insert(line);
                let line_content = buffer.line_content(line);
                if line_content == "\n" || line_content == "\r\n" {
                    continue;
                }
                let nonblank = buffer.first_non_blank_character_on_line(line);
                if let Some(edit) =
                    crate::indent::create_outdent(buffer, nonblank, indent)
                {
                    edits.push(edit);
                }
            }
        }

        buffer.edit(&edits, EditType::Delete)
    }

    pub fn do_edit<T: Clipboard>(
        cursor: &mut Cursor,
        buffer: &mut Buffer,
        cmd: &EditCommand,
        syntax: Option<&Syntax>,
        clipboard: &mut T,
        modal: bool,
        register: &mut Register,
    ) -> Vec<(RopeDelta, InvalLines)> {
        use crate::command::EditCommand::*;
        match cmd {
            MoveLineUp => {
                let mut deltas = Vec::new();
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    for region in selection.regions_mut() {
                        let start_line = buffer.line_of_offset(region.min());
                        if start_line > 0 {
                            let previous_line_len =
                                buffer.line_content(start_line - 1).len();

                            let end_line = buffer.line_of_offset(region.max());
                            let start = buffer.offset_of_line(start_line);
                            let end = buffer.offset_of_line(end_line + 1);
                            let content =
                                buffer.slice_to_cow(start..end).to_string();
                            let (delta, inval_lines) = buffer.edit(
                                &[
                                    (&Selection::region(start, end), ""),
                                    (
                                        &Selection::caret(
                                            buffer.offset_of_line(start_line - 1),
                                        ),
                                        &content,
                                    ),
                                ],
                                EditType::InsertChars,
                            );
                            deltas.push((delta, inval_lines));
                            region.start -= previous_line_len;
                            region.end -= previous_line_len;
                        }
                    }
                    cursor.mode = CursorMode::Insert(selection);
                }
                deltas
            }
            MoveLineDown => {
                let mut deltas = Vec::new();
                if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                    for region in selection.regions_mut().iter_mut().rev() {
                        let last_line = buffer.last_line();
                        let start_line = buffer.line_of_offset(region.min());
                        let end_line = buffer.line_of_offset(region.max());
                        if end_line < last_line {
                            let next_line_len =
                                buffer.line_content(end_line + 1).len();

                            let start = buffer.offset_of_line(start_line);
                            let end = buffer.offset_of_line(end_line + 1);
                            let content =
                                buffer.slice_to_cow(start..end).to_string();
                            let (delta, inval_lines) = buffer.edit(
                                &[
                                    (
                                        &Selection::caret(
                                            buffer.offset_of_line(end_line + 2),
                                        ),
                                        &content,
                                    ),
                                    (&Selection::region(start, end), ""),
                                ],
                                EditType::InsertChars,
                            );
                            deltas.push((delta, inval_lines));
                            region.start += next_line_len;
                            region.end += next_line_len;
                        }
                    }
                    cursor.mode = CursorMode::Insert(selection);
                }
                deltas
            }
            InsertNewLine => match cursor.mode.clone() {
                CursorMode::Normal(offset) => {
                    Self::insert_new_line(buffer, cursor, Selection::caret(offset))
                }
                CursorMode::Insert(selection) => {
                    Self::insert_new_line(buffer, cursor, selection)
                }
                CursorMode::Visual {
                    start: _,
                    end: _,
                    mode: _,
                } => {
                    vec![]
                }
            },
            InsertTab => {
                let mut deltas = Vec::new();
                if let CursorMode::Insert(selection) = &cursor.mode {
                    let indent = buffer.indent_unit();
                    let mut edits = Vec::new();

                    for region in selection.regions() {
                        if region.is_caret() {
                            edits.push(crate::indent::create_edit(
                                buffer,
                                region.start,
                                indent,
                            ))
                        } else {
                            let start_line = buffer.line_of_offset(region.min());
                            let end_line = buffer.line_of_offset(region.max());
                            for line in start_line..=end_line {
                                let offset =
                                    buffer.first_non_blank_character_on_line(line);
                                edits.push(crate::indent::create_edit(
                                    buffer, offset, indent,
                                ))
                            }
                        }
                    }

                    let (delta, inval_lines) =
                        buffer.edit(&edits, EditType::InsertChars);
                    let selection =
                        selection.apply_delta(&delta, true, InsertDrift::Default);
                    deltas.push((delta, inval_lines));
                    cursor.mode = CursorMode::Insert(selection);
                }
                deltas
            }
            IndentLine => {
                let selection = cursor.edit_selection(buffer);
                let (delta, inval_lines) = Self::do_indent(buffer, selection);
                cursor.apply_delta(&delta);
                vec![(delta, inval_lines)]
            }
            JoinLines => {
                let offset = cursor.offset();
                let (line, _col) = buffer.offset_to_line_col(offset);
                if line < buffer.last_line() {
                    let start = buffer.line_end_offset(line, true);
                    let end = buffer.first_non_blank_character_on_line(line + 1);
                    vec![buffer.edit(
                        &[(&Selection::region(start, end), " ")],
                        EditType::Other,
                    )]
                } else {
                    vec![]
                }
            }
            OutdentLine => {
                let selection = cursor.edit_selection(buffer);
                let (delta, inval_lines) = Self::do_outdent(buffer, selection);
                cursor.apply_delta(&delta);
                vec![(delta, inval_lines)]
            }
            ToggleLineComment => {
                let mut lines = HashSet::new();
                let selection = cursor.edit_selection(buffer);
                let comment_token =
                    syntax.map(|s| s.language.comment_token()).unwrap_or("//");
                let mut had_comment = true;
                let mut smallest_indent = usize::MAX;
                for region in selection.regions() {
                    let mut line = buffer.line_of_offset(region.min());
                    let end_line = buffer.line_of_offset(region.max());
                    let end_line_offset = buffer.offset_of_line(end_line);
                    let end = if end_line > line && region.max() == end_line_offset {
                        end_line_offset
                    } else {
                        buffer.offset_of_line(end_line + 1)
                    };
                    let start = buffer.offset_of_line(line);
                    for content in buffer.text().lines(start..end) {
                        let trimmed_content = content.trim_start();
                        if trimmed_content.is_empty() {
                            line += 1;
                            continue;
                        }
                        let indent = content.len() - trimmed_content.len();
                        if indent < smallest_indent {
                            smallest_indent = indent;
                        }
                        if !trimmed_content.starts_with(&comment_token) {
                            had_comment = false;
                            lines.insert((line, indent, 0));
                        } else {
                            let had_space_after_comment =
                                trimmed_content.chars().nth(comment_token.len())
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

                let (delta, inval_lines) = if had_comment {
                    let mut selection = Selection::new();
                    for (line, indent, len) in lines.iter() {
                        let start = buffer.offset_of_line(*line) + indent;
                        selection.add_region(SelRegion::new(
                            start,
                            start + len,
                            None,
                        ))
                    }
                    buffer.edit(&[(&selection, "")], EditType::Delete)
                } else {
                    let mut selection = Selection::new();
                    for (line, _, _) in lines.iter() {
                        let start = buffer.offset_of_line(*line) + smallest_indent;
                        selection.add_region(SelRegion::new(start, start, None))
                    }
                    buffer.edit(
                        &[(&selection, &format!("{comment_token} "))],
                        EditType::InsertChars,
                    )
                };
                cursor.apply_delta(&delta);
                vec![(delta, inval_lines)]
            }
            Undo => {
                if let Some((delta, inval_lines, cursor_mode)) = buffer.do_undo() {
                    if let Some(cursor_mode) = cursor_mode {
                        if modal {
                            cursor.mode = CursorMode::Normal(cursor_mode.offset());
                        } else {
                            cursor.mode = cursor_mode;
                        }
                    } else if let Some(new_cursor) =
                        get_first_selection_after(cursor, buffer, &delta)
                    {
                        *cursor = new_cursor
                    } else {
                        cursor.apply_delta(&delta);
                    }
                    vec![(delta, inval_lines)]
                } else {
                    vec![]
                }
            }
            Redo => {
                if let Some((delta, inval_lines, cursor_mode)) = buffer.do_redo() {
                    if let Some(cursor_mode) = cursor_mode {
                        if modal {
                            cursor.mode = CursorMode::Normal(cursor_mode.offset());
                        } else {
                            cursor.mode = cursor_mode;
                        }
                    } else if let Some(new_cursor) =
                        get_first_selection_after(cursor, buffer, &delta)
                    {
                        *cursor = new_cursor
                    } else {
                        cursor.apply_delta(&delta);
                    }
                    vec![(delta, inval_lines)]
                } else {
                    vec![]
                }
            }
            ClipboardCopy => {
                let data = cursor.yank(buffer);
                clipboard.put_string(data.content);

                match &cursor.mode {
                    CursorMode::Visual {
                        start,
                        end,
                        mode: _,
                    } => {
                        let offset = *start.min(end);
                        let offset =
                            buffer.offset_line_end(offset, false).min(offset);
                        cursor.mode = CursorMode::Normal(offset);
                    }
                    CursorMode::Normal(_) | CursorMode::Insert(_) => {}
                }
                vec![]
            }
            ClipboardCut => {
                let data = cursor.yank(buffer);
                clipboard.put_string(data.content);

                let selection =
                    if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                        for region in selection.regions_mut() {
                            if region.is_caret() {
                                let line = buffer.line_of_offset(region.start);
                                let start = buffer.offset_of_line(line);
                                let end = buffer.offset_of_line(line + 1);
                                region.start = start;
                                region.end = end;
                            }
                        }
                        selection
                    } else {
                        cursor.edit_selection(buffer)
                    };

                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            ClipboardPaste => {
                if let Some(s) = clipboard.get_string() {
                    let mode = if s.ends_with('\n') {
                        VisualMode::Linewise
                    } else {
                        VisualMode::Normal
                    };
                    let data = RegisterData { content: s, mode };
                    Self::do_paste(cursor, buffer, &data)
                } else {
                    vec![]
                }
            }
            Yank => {
                match &cursor.mode {
                    CursorMode::Visual { start, end, .. } => {
                        let data = cursor.yank(buffer);
                        register.add_yank(data);

                        let offset = *start.min(end);
                        let offset =
                            buffer.offset_line_end(offset, false).min(offset);
                        cursor.mode = CursorMode::Normal(offset);
                    }
                    CursorMode::Normal(_) => {}
                    CursorMode::Insert(_) => {}
                }
                vec![]
            }
            Paste => {
                let data = register.unnamed.clone();
                Self::do_paste(cursor, buffer, &data)
            }
            NewLineAbove => {
                let offset = cursor.offset();
                let line = buffer.line_of_offset(offset);
                let offset = if line > 0 {
                    buffer.line_end_offset(line - 1, true)
                } else {
                    buffer.first_non_blank_character_on_line(line)
                };
                let delta =
                    Self::insert_new_line(buffer, cursor, Selection::caret(offset));
                if line == 0 {
                    cursor.mode = CursorMode::Insert(Selection::caret(offset));
                }
                delta
            }
            NewLineBelow => {
                let offset = cursor.offset();
                let offset = buffer.offset_line_end(offset, true);
                Self::insert_new_line(buffer, cursor, Selection::caret(offset))
            }
            DeleteBackward => {
                let selection = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                    CursorMode::Insert(_) => {
                        let indent = buffer.indent_unit();
                        let selection = cursor.edit_selection(buffer);
                        let mut new_selection = Selection::new();
                        for region in selection.regions() {
                            let new_region = if region.is_caret() {
                                if indent.starts_with('\t') {
                                    let new_end = buffer.move_left(
                                        region.end,
                                        Mode::Insert,
                                        1,
                                    );
                                    SelRegion::new(region.start, new_end, None)
                                } else {
                                    let line = buffer.line_of_offset(region.start);
                                    let nonblank = buffer
                                        .first_non_blank_character_on_line(line);
                                    let (_, col) =
                                        buffer.offset_to_line_col(region.start);
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
                                    let new_end = buffer.move_left(
                                        region.end,
                                        Mode::Insert,
                                        count,
                                    );
                                    SelRegion::new(region.start, new_end, None)
                                }
                            } else {
                                *region
                            };
                            new_selection.add_region(new_region);
                        }

                        let mut selection = new_selection;
                        if selection.regions().len() == 1 {
                            let delete_str = buffer
                                .slice_to_cow(
                                    selection.min_offset()..selection.max_offset(),
                                )
                                .to_string();
                            if str_is_pair_left(&delete_str) {
                                if let Some(c) = str_matching_pair(&delete_str) {
                                    let offset = selection.max_offset();
                                    let line = buffer.line_of_offset(offset);
                                    let line_end =
                                        buffer.line_end_offset(line, true);
                                    let content = buffer
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
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            DeleteForward => {
                let selection = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection = cursor.edit_selection(buffer);
                        let mut new_selection = Selection::new();
                        for region in selection.regions() {
                            let new_region = if region.is_caret() {
                                let new_end =
                                    buffer.move_right(region.end, Mode::Insert, 1);
                                SelRegion::new(region.start, new_end, None)
                            } else {
                                *region
                            };
                            new_selection.add_region(new_region);
                        }
                        new_selection
                    }
                };
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            DeleteWordForward => {
                let selection = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                    CursorMode::Insert(_) => {
                        let mut new_selection = Selection::new();
                        let selection = cursor.edit_selection(buffer);

                        for region in selection.regions() {
                            let end = buffer.move_word_forward(region.end);
                            let new_region = SelRegion::new(region.start, end, None);
                            new_selection.add_region(new_region);
                        }

                        new_selection
                    }
                };
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            DeleteWordBackward => {
                let selection = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                    CursorMode::Insert(_) => {
                        let mut new_selection = Selection::new();
                        let selection = cursor.edit_selection(buffer);

                        for region in selection.regions() {
                            let end = buffer.move_word_backward(region.end);
                            let new_region = SelRegion::new(region.start, end, None);
                            new_selection.add_region(new_region);
                        }

                        new_selection
                    }
                };
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            DeleteToBeginningOfLine => {
                let selection = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        cursor.edit_selection(buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection = cursor.edit_selection(buffer);

                        let mut new_selection = Selection::new();
                        for region in selection.regions() {
                            let line = buffer.line_of_offset(region.end);
                            let end = buffer.offset_of_line(line);
                            let new_region = SelRegion::new(region.start, end, None);
                            new_selection.add_region(new_region);
                        }

                        new_selection
                    }
                };
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.update_selection(buffer, selection);
                vec![(delta, inval_lines)]
            }
            DeleteForwardAndInsert => {
                let selection = cursor.edit_selection(buffer);
                let (delta, inval_lines) =
                    buffer.edit(&[(&selection, "")], EditType::Delete);
                let selection =
                    selection.apply_delta(&delta, true, InsertDrift::Default);
                cursor.mode = CursorMode::Insert(selection);
                vec![(delta, inval_lines)]
            }
            NormalMode => {
                if !modal {
                    if let CursorMode::Insert(selection) = &cursor.mode {
                        match selection.regions().len() {
                            i if i > 1 => {
                                if let Some(region) = selection.last_inserted() {
                                    let new_selection =
                                        Selection::region(region.start, region.end);
                                    cursor.mode = CursorMode::Insert(new_selection);
                                    return vec![];
                                }
                            }
                            i if i == 1 => {
                                let region = selection.regions()[0];
                                if !region.is_caret() {
                                    let new_selection = Selection::caret(region.end);
                                    cursor.mode = CursorMode::Insert(new_selection);
                                    return vec![];
                                }
                            }
                            _ => (),
                        }
                    }

                    return vec![];
                }

                let offset = match &cursor.mode {
                    CursorMode::Insert(selection) => {
                        let offset = selection.min_offset();
                        buffer.prev_grapheme_offset(
                            offset,
                            1,
                            buffer.offset_of_line(buffer.line_of_offset(offset)),
                        )
                    }
                    CursorMode::Visual { end, .. } => {
                        buffer.offset_line_end(*end, false).min(*end)
                    }
                    CursorMode::Normal(offset) => *offset,
                };

                buffer.reset_edit_type();
                cursor.mode = CursorMode::Normal(offset);
                cursor.horiz = None;
                vec![]
            }
            InsertMode => {
                cursor.mode = CursorMode::Insert(Selection::caret(cursor.offset()));
                vec![]
            }
            InsertFirstNonBlank => {
                match &cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line = buffer.line_of_offset(*offset);
                        let offset = buffer.first_non_blank_character_on_line(line);
                        cursor.mode = CursorMode::Insert(Selection::caret(offset));
                    }
                    CursorMode::Visual { .. } => {
                        let mut selection = Selection::new();
                        for region in cursor.edit_selection(buffer).regions() {
                            selection.add_region(SelRegion::caret(region.min()));
                        }
                        cursor.mode = CursorMode::Insert(selection);
                    }
                    CursorMode::Insert(_) => {}
                };
                vec![]
            }
            Append => {
                let offset = buffer.move_right(cursor.offset(), Mode::Insert, 1);
                cursor.mode = CursorMode::Insert(Selection::caret(offset));
                vec![]
            }
            AppendEndOfLine => {
                let offset = cursor.offset();
                let line = buffer.line_of_offset(offset);
                let offset = buffer.line_end_offset(line, true);
                cursor.mode = CursorMode::Insert(Selection::caret(offset));
                vec![]
            }
            ToggleVisualMode => {
                Self::toggle_visual(cursor, VisualMode::Normal, modal);
                vec![]
            }
            ToggleLinewiseVisualMode => {
                Self::toggle_visual(cursor, VisualMode::Linewise, modal);
                vec![]
            }
            ToggleBlockwiseVisualMode => {
                Self::toggle_visual(cursor, VisualMode::Blockwise, modal);
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::buffer::Buffer;
    use crate::cursor::{Cursor, CursorMode};
    use crate::editor::Editor;
    use crate::selection::{SelRegion, Selection};

    #[test]
    fn test_insert_simple() {
        let mut buffer = Buffer::new("abc");
        let mut cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(1)), None, None);

        Editor::insert(&mut cursor, &mut buffer, "e", None);
        assert_eq!("aebc", buffer.slice_to_cow(0..buffer.len()));
    }

    #[test]
    fn test_insert_multiple_cursor() {
        let mut buffer = Buffer::new("abc\nefg\n");
        let mut selection = Selection::new();
        selection.add_region(SelRegion::caret(1));
        selection.add_region(SelRegion::caret(5));
        let mut cursor = Cursor::new(CursorMode::Insert(selection), None, None);

        Editor::insert(&mut cursor, &mut buffer, "i", None);
        assert_eq!("aibc\neifg\n", buffer.slice_to_cow(0..buffer.len()));
    }

    #[test]
    fn test_insert_complex() {
        let mut buffer = Buffer::new("abc\nefg\n");
        let mut selection = Selection::new();
        selection.add_region(SelRegion::caret(1));
        selection.add_region(SelRegion::caret(5));
        let mut cursor = Cursor::new(CursorMode::Insert(selection), None, None);

        Editor::insert(&mut cursor, &mut buffer, "i", None);
        assert_eq!("aibc\neifg\n", buffer.slice_to_cow(0..buffer.len()));
        Editor::insert(&mut cursor, &mut buffer, "j", None);
        assert_eq!("aijbc\neijfg\n", buffer.slice_to_cow(0..buffer.len()));
        Editor::insert(&mut cursor, &mut buffer, "{", None);
        assert_eq!("aij{bc\neij{fg\n", buffer.slice_to_cow(0..buffer.len()));
        Editor::insert(&mut cursor, &mut buffer, " ", None);
        assert_eq!("aij{ bc\neij{ fg\n", buffer.slice_to_cow(0..buffer.len()));
    }

    #[test]
    fn test_insert_pair() {
        let mut buffer = Buffer::new("a bc\ne fg\n");
        let mut selection = Selection::new();
        selection.add_region(SelRegion::caret(1));
        selection.add_region(SelRegion::caret(6));
        let mut cursor = Cursor::new(CursorMode::Insert(selection), None, None);

        Editor::insert(&mut cursor, &mut buffer, "{", None);
        assert_eq!("a{} bc\ne{} fg\n", buffer.slice_to_cow(0..buffer.len()));
        Editor::insert(&mut cursor, &mut buffer, "}", None);
        assert_eq!("a{} bc\ne{} fg\n", buffer.slice_to_cow(0..buffer.len()));
    }
}
