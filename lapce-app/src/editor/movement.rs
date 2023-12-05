//! Movement logic for the editor.

use std::collections::HashSet;

use lapce_core::{
    buffer::rope_text::RopeText,
    command::MultiSelectionCommand,
    cursor::{ColPosition, Cursor, CursorAffinity, CursorMode},
    editor::Editor,
    mode::{Mode, MotionMode, VisualMode},
    movement::{LinePosition, Movement},
    register::Register,
    selection::{SelRegion, Selection},
    soft_tab::{snap_to_soft_tab, SnapDirection},
};

use crate::doc::Document;

use super::{
    view_data::EditorViewData,
    visual_line::{RVLine, VLineInfo},
};

/// Move a selection region by a given movement.  
/// Much of the time, this will just be a matter of moving the cursor, but
/// some movements may depend on the current selection.
fn move_region(
    view: &EditorViewData,
    region: &SelRegion,
    affinity: &mut CursorAffinity,
    count: usize,
    modify: bool,
    movement: &Movement,
    mode: Mode,
) -> SelRegion {
    let (count, region) = if count >= 1 && !modify && !region.is_caret() {
        // If we're not a caret, and we are moving left/up or right/down, we want to move
        // the cursor to the left or right side of the selection.
        // Ex: `|abc|` -> left/up arrow key -> `|abc`
        // Ex: `|abc|` -> right/down arrow key -> `abc|`
        // and it doesn't matter which direction the selection is going, so we use min/max
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

    let (end, horiz) = move_offset(
        view,
        region.end,
        region.horiz.as_ref(),
        affinity,
        count,
        movement,
        mode,
    );
    let start = match modify {
        true => region.start,
        false => end,
    };
    SelRegion::new(start, end, horiz)
}

pub fn move_selection(
    view: &EditorViewData,
    selection: &Selection,
    affinity: &mut CursorAffinity,
    count: usize,
    modify: bool,
    movement: &Movement,
    mode: Mode,
) -> Selection {
    let mut new_selection = Selection::new();
    for region in selection.regions() {
        new_selection.add_region(move_region(
            view, region, affinity, count, modify, movement, mode,
        ));
    }
    new_selection
}

// TODO: It would probably fit the overall logic better if affinity was immutable and it just returned the new affinity!
pub fn move_offset(
    view: &EditorViewData,
    offset: usize,
    horiz: Option<&ColPosition>,
    affinity: &mut CursorAffinity,
    count: usize,
    movement: &Movement,
    mode: Mode,
) -> (usize, Option<ColPosition>) {
    let config = view.config.get_untracked();

    match movement {
        Movement::Left => {
            let new_offset = move_left(
                view.rope_text(),
                offset,
                affinity,
                mode,
                count,
                config.editor.atomic_soft_tab_width(),
            );

            (new_offset, None)
        }
        Movement::Right => {
            let new_offset = move_right(
                view,
                offset,
                affinity,
                mode,
                count,
                config.editor.atomic_soft_tab_width(),
            );

            (new_offset, None)
        }
        Movement::Up => {
            let (new_offset, horiz) =
                move_up(view, offset, affinity, horiz.cloned(), mode, count);

            (new_offset, Some(horiz))
        }
        Movement::Down => {
            let (new_offset, horiz) =
                move_down(view, offset, affinity, horiz.cloned(), mode, count);

            (new_offset, Some(horiz))
        }
        Movement::DocumentStart => {
            // Put it before any inlay hints at the very start
            *affinity = CursorAffinity::Backward;
            (0, Some(ColPosition::Start))
        }
        Movement::DocumentEnd => {
            let (new_offset, horiz) = document_end(view.rope_text(), affinity, mode);

            (new_offset, Some(horiz))
        }
        Movement::FirstNonBlank => {
            let (new_offset, horiz) = first_non_blank(view, affinity, offset);

            (new_offset, Some(horiz))
        }
        Movement::StartOfLine => {
            let (new_offset, horiz) = start_of_line(view, affinity, offset);

            (new_offset, Some(horiz))
        }
        Movement::EndOfLine => {
            let (new_offset, horiz) = end_of_line(view, affinity, offset, mode);

            (new_offset, Some(horiz))
        }
        Movement::Line(position) => {
            let (new_offset, horiz) =
                to_line(view, offset, horiz.cloned(), mode, position);

            (new_offset, Some(horiz))
        }
        Movement::Offset(offset) => {
            let new_offset = view.text().prev_grapheme_offset(*offset + 1).unwrap();
            (new_offset, None)
        }
        Movement::WordEndForward => {
            let new_offset = view.rope_text().move_n_wordends_forward(
                offset,
                count,
                mode == Mode::Insert,
            );
            (new_offset, None)
        }
        Movement::WordForward => {
            let new_offset = view.rope_text().move_n_words_forward(offset, count);
            (new_offset, None)
        }
        Movement::WordBackward => {
            let new_offset =
                view.rope_text().move_n_words_backward(offset, count, mode);
            (new_offset, None)
        }
        Movement::NextUnmatched(char) => {
            let new_offset = view.find_unmatched(offset, false, *char);

            (new_offset, None)
        }
        Movement::PreviousUnmatched(char) => {
            let new_offset = view.find_unmatched(offset, true, *char);

            (new_offset, None)
        }
        Movement::MatchPairs => {
            let new_offset = view.find_matching_pair(offset);

            (new_offset, None)
        }
        Movement::ParagraphForward => {
            let new_offset =
                view.rope_text().move_n_paragraphs_forward(offset, count);

            (new_offset, None)
        }
        Movement::ParagraphBackward => {
            let new_offset =
                view.rope_text().move_n_paragraphs_backward(offset, count);

            (new_offset, None)
        }
    }
}

/// Move the offset to the left by `count` amount.  
/// If `soft_tab_width` is `Some` (and greater than 1) then the offset will snap to the soft tab.  
fn move_left(
    rope_text: impl RopeText,
    offset: usize,
    affinity: &mut CursorAffinity,
    mode: Mode,
    count: usize,
    soft_tab_width: Option<usize>,
) -> usize {
    let mut new_offset = rope_text.move_left(offset, mode, count);

    if let Some(soft_tab_width) = soft_tab_width {
        if soft_tab_width > 1 {
            new_offset = snap_to_soft_tab(
                rope_text.text(),
                new_offset,
                SnapDirection::Left,
                soft_tab_width,
            );
        }
    }

    *affinity = CursorAffinity::Forward;

    new_offset
}

/// Move the offset to the right by `count` amount.
/// If `soft_tab_width` is `Some` (and greater than 1) then the offset will snap to the soft tab.
fn move_right(
    view: &EditorViewData,
    offset: usize,
    affinity: &mut CursorAffinity,
    mode: Mode,
    count: usize,
    soft_tab_width: Option<usize>,
) -> usize {
    let rope_text = view.rope_text();
    let mut new_offset = rope_text.move_right(offset, mode, count);

    if let Some(soft_tab_width) = soft_tab_width {
        if soft_tab_width > 1 {
            new_offset = snap_to_soft_tab(
                rope_text.text(),
                new_offset,
                SnapDirection::Right,
                soft_tab_width,
            );
        }
    }

    let (rvline, col) = view.rvline_col_of_offset(offset, *affinity);
    let info = view.rvline_info(rvline);

    *affinity = if col == info.last_col(&view.text_prov(), false) {
        CursorAffinity::Backward
    } else {
        CursorAffinity::Forward
    };

    new_offset
}

fn find_prev_rvline(
    view: &EditorViewData,
    start: RVLine,
    count: usize,
) -> Option<RVLine> {
    if count == 0 {
        return Some(start);
    }

    // We can't just directly subtract count because of multi-line phantom text.
    // As just subtracting count wouldn't properly skip over the phantom lines.
    // So we have to search backwards for the previous line that has real content.
    let mut info = None;
    let mut found_count = 0;
    for prev_info in view.iter_rvlines(true, start).skip(1) {
        if prev_info.is_empty() {
            // We skip any phantom text lines in our consideration
            continue;
        }

        // Otherwise we found a real line.
        found_count += 1;

        if found_count == count {
            // If we've completed all the count instances then we're done
            info = Some(prev_info);
            break;
        }
        // Otherwise we continue on to find the previous line with content before that.
    }

    info.map(|info| info.rvline)
}

/// Move the offset up by `count` amount.  
/// `count` may be zero, because moving up in a selection just jumps to the start of the selection.
fn move_up(
    view: &EditorViewData,
    offset: usize,
    affinity: &mut CursorAffinity,
    horiz: Option<ColPosition>,
    mode: Mode,
    count: usize,
) -> (usize, ColPosition) {
    let rvline = view.rvline_of_offset(offset, *affinity);
    if rvline.line == 0 && rvline.line_index == 0 {
        // Zeroth line
        let horiz = horiz.unwrap_or_else(|| {
            ColPosition::Col(view.line_point_of_offset(offset, *affinity).x)
        });

        *affinity = CursorAffinity::Backward;

        return (0, horiz);
    }

    let Some(rvline) = find_prev_rvline(view, rvline, count) else {
        // Zeroth line
        let horiz = horiz.unwrap_or_else(|| {
            ColPosition::Col(view.line_point_of_offset(offset, *affinity).x)
        });

        *affinity = CursorAffinity::Backward;

        return (0, horiz);
    };

    let horiz = horiz.unwrap_or_else(|| {
        ColPosition::Col(view.line_point_of_offset(offset, *affinity).x)
    });
    let col = view.rvline_horiz_col(rvline, &horiz, mode != Mode::Normal);

    // TODO: this should maybe be doing `new_offset == info.interval.start`?
    *affinity = if col == 0 {
        CursorAffinity::Forward
    } else {
        CursorAffinity::Backward
    };

    let new_offset = view.offset_of_line_col(rvline.line, col);

    (new_offset, horiz)
}

/// Move down for when the cursor is on the last visual line.
fn move_down_last_rvline(
    view: &EditorViewData,
    offset: usize,
    affinity: &mut CursorAffinity,
    horiz: Option<ColPosition>,
    mode: Mode,
) -> (usize, ColPosition) {
    let rope_text = view.rope_text();

    let last_line = rope_text.last_line();
    let new_offset = rope_text.line_end_offset(last_line, mode != Mode::Normal);

    // We should appear after any phantom text at the very end of the line.
    *affinity = CursorAffinity::Forward;

    let horiz = horiz.unwrap_or_else(|| {
        ColPosition::Col(view.line_point_of_offset(offset, *affinity).x)
    });

    (new_offset, horiz)
}

fn find_next_rvline_info(
    view: &EditorViewData,
    offset: usize,
    start: RVLine,
    count: usize,
) -> Option<VLineInfo<()>> {
    // We can't just directly add count because of multi-line phantom text.
    // These lines are 'not there' and also don't have any position that can be moved into
    // (unlike phantom text that is mixed with real text)
    // So we have to search forward for the next line that has real content.
    // The typical iteration count for this is 1, and even after that it is usually only a handful.
    let mut found_count = 0;
    for next_info in view.iter_rvlines(false, start) {
        if count == 0 {
            return Some(next_info);
        }

        if next_info.is_empty() {
            // We skip any phantom text lines in our consideration
            // TODO: Would this skip over an empty line?
            continue;
        }

        if next_info.interval.start <= offset {
            // If we're on or before our current visual line then we skip it
            continue;
        }

        // Otherwise we found a real line.
        found_count += 1;

        if found_count == count {
            // If we've completed all the count instances then we're done
            return Some(next_info);
        }
        // Otherwise we continue on to find the next line with content after that.
    }

    None
}

/// Move the offset down by `count` amount.  
/// `count` may be zero, because moving down in a selection just jumps to the end of the selection.
fn move_down(
    view: &EditorViewData,
    offset: usize,
    affinity: &mut CursorAffinity,
    horiz: Option<ColPosition>,
    mode: Mode,
    count: usize,
) -> (usize, ColPosition) {
    let rvline = view.rvline_of_offset(offset, *affinity);

    let Some(info) = find_next_rvline_info(view, offset, rvline, count) else {
        // There was no next entry, this typically means that we would go past the end if we went
        // further
        return move_down_last_rvline(view, offset, affinity, horiz, mode);
    };

    // TODO(minor): is this the right affinity?
    let horiz = horiz.unwrap_or_else(|| {
        ColPosition::Col(view.line_point_of_offset(offset, *affinity).x)
    });

    let col = view.rvline_horiz_col(info.rvline, &horiz, mode != Mode::Normal);

    let new_offset = view.offset_of_line_col(info.rvline.line, col);

    *affinity = if new_offset == info.interval.start {
        // The column was zero so we shift it to be at the line itself.
        // This lets us move down to an empty - for example - next line and appear at the
        // start of that line without coinciding with the offset at the end of the previous line.
        CursorAffinity::Forward
    } else {
        CursorAffinity::Backward
    };

    (new_offset, horiz)
}

fn document_end(
    rope_text: impl RopeText,
    affinity: &mut CursorAffinity,
    mode: Mode,
) -> (usize, ColPosition) {
    let last_offset =
        rope_text.offset_line_end(rope_text.len(), mode != Mode::Normal);

    // Put it past any inlay hints directly at the end
    *affinity = CursorAffinity::Forward;

    (last_offset, ColPosition::End)
}

fn first_non_blank(
    view: &EditorViewData,
    affinity: &mut CursorAffinity,
    offset: usize,
) -> (usize, ColPosition) {
    let info = view.rvline_info_of_offset(offset, *affinity);
    let non_blank_offset = info.first_non_blank_character(&view.text_prov());
    let start_line_offset = info.interval.start;
    // TODO: is this always the correct affinity? It might be desirable for the very first character on a wrapped line?
    *affinity = CursorAffinity::Forward;

    if offset > non_blank_offset {
        // Jump to the first non-whitespace character if we're strictly after it
        (non_blank_offset, ColPosition::FirstNonBlank)
    } else {
        // If we're at the start of the line, also jump to the first not blank
        if start_line_offset == offset {
            (non_blank_offset, ColPosition::FirstNonBlank)
        } else {
            // Otherwise, jump to the start of the line
            (start_line_offset, ColPosition::Start)
        }
    }
}

fn start_of_line(
    view: &EditorViewData,
    affinity: &mut CursorAffinity,
    offset: usize,
) -> (usize, ColPosition) {
    let rvline = view.rvline_of_offset(offset, *affinity);
    let new_offset = view.offset_of_rvline(rvline);
    // TODO(minor): if the line has zero characters, it should probably be forward affinity but
    // other cases might be better as backwards?
    *affinity = CursorAffinity::Forward;

    (new_offset, ColPosition::Start)
}

fn end_of_line(
    view: &EditorViewData,
    affinity: &mut CursorAffinity,
    offset: usize,
    mode: Mode,
) -> (usize, ColPosition) {
    let info = view.rvline_info_of_offset(offset, *affinity);
    let new_col = info.last_col(&view.text_prov(), mode != Mode::Normal);
    *affinity = if new_col == 0 {
        CursorAffinity::Forward
    } else {
        CursorAffinity::Backward
    };

    let new_offset = view.offset_of_line_col(info.rvline.line, new_col);

    (new_offset, ColPosition::End)
}

fn to_line(
    view: &EditorViewData,
    offset: usize,
    horiz: Option<ColPosition>,
    mode: Mode,
    position: &LinePosition,
) -> (usize, ColPosition) {
    let rope_text = view.rope_text();

    // TODO(minor): Should this use rvline?
    let line = match position {
        LinePosition::Line(line) => (line - 1).min(rope_text.last_line()),
        LinePosition::First => 0,
        LinePosition::Last => rope_text.last_line(),
    };
    // TODO(minor): is this the best affinity?
    let horiz = horiz.unwrap_or_else(|| {
        ColPosition::Col(
            view.line_point_of_offset(offset, CursorAffinity::Backward)
                .x,
        )
    });
    let col = view.line_horiz_col(line, &horiz, mode != Mode::Normal);
    let new_offset = rope_text.offset_of_line_col(line, col);

    (new_offset, horiz)
}

/// Move the current cursor.  
/// This will signal-update the document for some motion modes.
pub fn move_cursor(
    view: &EditorViewData,
    cursor: &mut Cursor,
    movement: &Movement,
    count: usize,
    modify: bool,
    register: &mut Register,
) {
    match cursor.mode {
        CursorMode::Normal(offset) => {
            let count = if let Some(motion_mode) = cursor.motion_mode.as_ref() {
                count.max(motion_mode.count())
            } else {
                count
            };
            let (new_offset, horiz) = move_offset(
                view,
                offset,
                cursor.horiz.as_ref(),
                &mut cursor.affinity,
                count,
                movement,
                Mode::Normal,
            );
            if let Some(motion_mode) = cursor.motion_mode.clone() {
                let (moved_new_offset, _) = move_offset(
                    view,
                    new_offset,
                    None,
                    &mut cursor.affinity,
                    1,
                    &Movement::Right,
                    Mode::Insert,
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
                let doc = view.doc.get_untracked();
                let deltas = doc
                    .buffer
                    .try_update(|buffer| {
                        Editor::execute_motion_mode(
                            cursor,
                            buffer,
                            motion_mode,
                            start,
                            end,
                            movement.is_vertical(),
                            register,
                        )
                    })
                    .unwrap();
                doc.apply_deltas(&deltas);
                cursor.motion_mode = None;
            } else {
                cursor.mode = CursorMode::Normal(new_offset);
                cursor.horiz = horiz;
            }
        }
        CursorMode::Visual { start, end, mode } => {
            let (new_offset, horiz) = move_offset(
                view,
                end,
                cursor.horiz.as_ref(),
                &mut cursor.affinity,
                count,
                movement,
                Mode::Visual(VisualMode::Normal),
            );
            cursor.mode = CursorMode::Visual {
                start,
                end: new_offset,
                mode,
            };
            cursor.horiz = horiz;
        }
        CursorMode::Insert(ref selection) => {
            let selection = move_selection(
                view,
                selection,
                &mut cursor.affinity,
                count,
                modify,
                movement,
                Mode::Insert,
            );
            cursor.set_insert(selection);
        }
    }
}

pub fn do_multi_selection(
    view: &EditorViewData,
    cursor: &mut Cursor,
    cmd: &MultiSelectionCommand,
) {
    use MultiSelectionCommand::*;
    let rope_text = view.rope_text();

    match cmd {
        SelectUndo => {
            if let CursorMode::Insert(_) = cursor.mode.clone() {
                if let Some(selection) = cursor.history_selections.last().cloned() {
                    cursor.mode = CursorMode::Insert(selection);
                }
                cursor.history_selections.pop();
            }
        }
        InsertCursorAbove => {
            if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                let offset = selection.first().map(|s| s.end).unwrap_or(0);
                let (new_offset, _) = move_offset(
                    view,
                    offset,
                    cursor.horiz.as_ref(),
                    &mut cursor.affinity,
                    1,
                    &Movement::Up,
                    Mode::Insert,
                );
                if new_offset != offset {
                    selection
                        .add_region(SelRegion::new(new_offset, new_offset, None));
                }
                cursor.set_insert(selection);
            }
        }
        InsertCursorBelow => {
            if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                let offset = selection.last().map(|s| s.end).unwrap_or(0);
                let (new_offset, _) = move_offset(
                    view,
                    offset,
                    cursor.horiz.as_ref(),
                    &mut cursor.affinity,
                    1,
                    &Movement::Down,
                    Mode::Insert,
                );
                if new_offset != offset {
                    selection
                        .add_region(SelRegion::new(new_offset, new_offset, None));
                }
                cursor.set_insert(selection);
            }
        }
        InsertCursorEndOfLine => {
            if let CursorMode::Insert(selection) = cursor.mode.clone() {
                let mut new_selection = Selection::new();
                for region in selection.regions() {
                    let (start_line, _) = rope_text.offset_to_line_col(region.min());
                    let (end_line, end_col) =
                        rope_text.offset_to_line_col(region.max());
                    for line in start_line..end_line + 1 {
                        let offset = if line == end_line {
                            rope_text.offset_of_line_col(line, end_col)
                        } else {
                            rope_text.line_end_offset(line, true)
                        };
                        new_selection
                            .add_region(SelRegion::new(offset, offset, None));
                    }
                }
                cursor.set_insert(new_selection);
            }
        }
        SelectCurrentLine => {
            if let CursorMode::Insert(selection) = cursor.mode.clone() {
                let mut new_selection = Selection::new();
                for region in selection.regions() {
                    let start_line = rope_text.line_of_offset(region.min());
                    let start = rope_text.offset_of_line(start_line);
                    let end_line = rope_text.line_of_offset(region.max());
                    let end = rope_text.offset_of_line(end_line + 1);
                    new_selection.add_region(SelRegion::new(start, end, None));
                }
                cursor.set_insert(new_selection);
            }
        }
        SelectAllCurrent => {
            if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                if !selection.is_empty() {
                    let find = view.find();
                    let config = view.config.get_untracked();

                    let first = selection.first().unwrap();
                    let (start, end) = if first.is_caret() {
                        rope_text.select_word(first.start)
                    } else {
                        (first.min(), first.max())
                    };
                    let search_str = rope_text.slice_to_cow(start..end);
                    let case_sensitive = find.case_sensitive(false);
                    let multicursor_case_sensitive =
                        config.editor.multicursor_case_sensitive;
                    let case_sensitive =
                        multicursor_case_sensitive || case_sensitive;
                    // let search_whole_word = config.editor.multicursor_whole_words;
                    find.set_case_sensitive(case_sensitive);
                    find.set_find(&search_str);
                    let mut offset = 0;
                    while let Some((start, end)) =
                        find.next(rope_text.text(), offset, false, false)
                    {
                        offset = end;
                        selection.add_region(SelRegion::new(start, end, None));
                    }
                }
                cursor.set_insert(selection);
            }
        }
        SelectNextCurrent => {
            if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                if !selection.is_empty() {
                    let mut had_caret = false;
                    for region in selection.regions_mut() {
                        if region.is_caret() {
                            had_caret = true;
                            let (start, end) = rope_text.select_word(region.start);
                            region.start = start;
                            region.end = end;
                        }
                    }
                    if !had_caret {
                        let find = view.find();
                        let config = view.config.get_untracked();

                        let r = selection.last_inserted().unwrap();
                        let search_str = rope_text.slice_to_cow(r.min()..r.max());
                        let case_sensitive = find.case_sensitive(false);
                        let case_sensitive =
                            config.editor.multicursor_case_sensitive
                                || case_sensitive;
                        // let search_whole_word =
                        // config.editor.multicursor_whole_words;
                        find.set_case_sensitive(case_sensitive);
                        find.set_find(&search_str);
                        let mut offset = r.max();
                        let mut seen = HashSet::new();
                        while let Some((start, end)) =
                            find.next(rope_text.text(), offset, false, true)
                        {
                            if !selection
                                .regions()
                                .iter()
                                .any(|r| r.min() == start && r.max() == end)
                            {
                                selection
                                    .add_region(SelRegion::new(start, end, None));
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
                cursor.set_insert(selection);
            }
        }
        SelectSkipCurrent => {
            if let CursorMode::Insert(mut selection) = cursor.mode.clone() {
                if !selection.is_empty() {
                    let r = selection.last_inserted().unwrap();
                    if r.is_caret() {
                        let (start, end) = rope_text.select_word(r.start);
                        selection.replace_last_inserted_region(SelRegion::new(
                            start, end, None,
                        ));
                    } else {
                        let find = view.find();

                        let search_str = rope_text.slice_to_cow(r.min()..r.max());
                        find.set_find(&search_str);
                        let mut offset = r.max();
                        let mut seen = HashSet::new();
                        while let Some((start, end)) =
                            find.next(rope_text.text(), offset, false, true)
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
                cursor.set_insert(selection);
            }
        }
        SelectAll => {
            let new_selection = Selection::region(0, rope_text.len());
            cursor.set_insert(new_selection);
        }
    }
}

pub fn do_motion_mode(
    doc: &Document,
    cursor: &mut Cursor,
    motion_mode: MotionMode,
    register: &mut Register,
) {
    if let Some(cached_motion_mode) = cursor.motion_mode.take() {
        // If it's the same MotionMode discriminant, continue, count is cached in the old motion_mode.
        if core::mem::discriminant(&cached_motion_mode)
            == core::mem::discriminant(&motion_mode)
        {
            let offset = cursor.offset();
            let deltas = doc
                .buffer
                .try_update(|buffer| {
                    Editor::execute_motion_mode(
                        cursor,
                        buffer,
                        cached_motion_mode,
                        offset,
                        offset,
                        true,
                        register,
                    )
                })
                .unwrap();
            doc.apply_deltas(&deltas);
        }
    } else {
        cursor.motion_mode = Some(motion_mode);
    }
}

// TODO: Write tests for the various functions. We'll need a more easily swappable API than
// `EditorViewData` for that.
