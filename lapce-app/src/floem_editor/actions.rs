use itertools::Itertools;
use lapce_core::{
    buffer::{rope_text::RopeText, Buffer, InvalLines},
    cursor::{Cursor, CursorMode},
    editor::EditType,
    mode::{Mode, VisualMode},
    selection::{InsertDrift, SelRegion, Selection},
    syntax::{
        edit::SyntaxEdit,
        util::{matching_char, matching_pair_direction},
    },
    word::{get_char_property, CharClassification},
};
use lapce_xi_rope::RopeDelta;

/// Insert into a buffer with a cursor, getting the deltas
pub fn insert(
    cursor: &mut Cursor,
    buffer: &mut Buffer,
    s: &str,
    prev_unmatched: &dyn Fn(&Buffer, char, usize) -> Option<usize>,
    auto_closing_matching_pairs: bool,
    auto_surround: bool,
) -> Vec<(RopeDelta, InvalLines, SyntaxEdit)> {
    let CursorMode::Insert(selection) = &cursor.mode else {
        return Vec::new();
    };

    if s.chars().count() != 1 {
        let (delta, inval_lines, edits) =
            buffer.edit([(selection, s)], EditType::InsertChars);
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        cursor.mode = CursorMode::Insert(selection);
        return vec![(delta, inval_lines, edits)];
    }
    let mut deltas = Vec::new();
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
        let prev_offset = buffer.move_left(offset, Mode::Normal, 1);
        let prev_cursor_char = if prev_offset < offset {
            buffer.char_at_offset(prev_offset)
        } else {
            None
        };

        // when text is selected, and [,{,(,'," is inserted
        // wrap the text with that char and its corresponding closing pair
        if region.start != region.end
            && auto_surround
            && (matching_pair_type == Some(true) || c == '"' || c == '\'')
        {
            edits.push((
                Selection::region(region.min(), region.min()),
                c.to_string(),
            ));
            edits_after.push((
                idx,
                match c {
                    '"' => '"',
                    '\'' => '\'',
                    _ => matching_char(c).unwrap(),
                },
            ));
            continue;
        }

        if auto_closing_matching_pairs {
            if (c == '"' || c == '\'') && cursor_char == Some(c) {
                // Skip the closing character
                let new_offset =
                    buffer.next_grapheme_offset(offset, 1, buffer.len());

                *region = SelRegion::caret(new_offset);
                continue;
            }

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
                    if let Some(previous_offset) =
                        prev_unmatched(buffer, opening_character, offset)
                    {
                        // Auto-indent closing character to the same level as the opening.
                        let previous_line = buffer.line_of_offset(previous_offset);
                        let line_indent = buffer.indent_on_line(previous_line);

                        let current_selection =
                            Selection::region(line_start, offset);

                        edits.push((current_selection, format!("{line_indent}{c}")));
                        continue;
                    }
                }
            }

            if matching_pair_type == Some(true) || c == '"' || c == '\'' {
                // Create a late edit to insert the closing pair, if allowed.
                let is_whitespace_or_punct = cursor_char
                    .map(|c| {
                        let prop = get_char_property(c);
                        prop == CharClassification::Lf
                            || prop == CharClassification::Space
                            || prop == CharClassification::Punctuation
                    })
                    .unwrap_or(true);

                let should_insert_pair = match c {
                    '"' | '\'' => {
                        is_whitespace_or_punct
                            && prev_cursor_char
                                .map(|c| {
                                    let prop = get_char_property(c);
                                    prop == CharClassification::Lf
                                        || prop == CharClassification::Space
                                        || prop == CharClassification::Punctuation
                                })
                                .unwrap_or(true)
                    }
                    _ => is_whitespace_or_punct,
                };

                if should_insert_pair {
                    let insert_after = match c {
                        '"' => '"',
                        '\'' => '\'',
                        _ => matching_char(c).unwrap(),
                    };
                    edits_after.push((idx, insert_after));
                }
            };
        }

        let current_selection = Selection::region(region.start, region.end);

        edits.push((current_selection, c.to_string()));
    }

    // Apply edits to current selection
    let edits = edits
        .iter()
        .map(|(selection, content)| (selection, content.as_str()))
        .collect::<Vec<_>>();

    let (delta, inval_lines, edits) = buffer.edit(&edits, EditType::InsertChars);

    buffer.set_cursor_before(CursorMode::Insert(selection.clone()));

    // Update selection
    let mut selection = selection.apply_delta(&delta, true, InsertDrift::Default);

    buffer.set_cursor_after(CursorMode::Insert(selection.clone()));

    deltas.push((delta, inval_lines, edits));
    // Apply late edits
    let edits_after = edits_after
        .iter()
        .map(|(idx, content)| {
            let region = &selection.regions()[*idx];
            (
                Selection::region(region.max(), region.max()),
                content.to_string(),
            )
        })
        .collect::<Vec<_>>();

    let edits_after = edits_after
        .iter()
        .map(|(selection, content)| (selection, content.as_str()))
        .collect::<Vec<_>>();

    if !edits_after.is_empty() {
        let (delta, inval_lines, edits) =
            buffer.edit(&edits_after, EditType::InsertChars);
        deltas.push((delta, inval_lines, edits));
    }

    // Adjust selection according to previous late edits
    let mut adjustment = 0;
    for region in selection
        .regions_mut()
        .iter_mut()
        .sorted_by(|region_a, region_b| region_a.start.cmp(&region_b.start))
    {
        let new_region =
            SelRegion::new(region.start + adjustment, region.end + adjustment, None);

        if let Some(inserted) = edits_after.iter().find_map(|(selection, str)| {
            if selection.last_inserted().map(|r| r.start) == Some(region.start) {
                Some(str)
            } else {
                None
            }
        }) {
            adjustment += inserted.len();
        }

        *region = new_region;
    }

    cursor.mode = CursorMode::Insert(selection);

    deltas
}

pub fn toggle_visual(cursor: &mut Cursor, visual_mode: VisualMode, modal: bool) {
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
