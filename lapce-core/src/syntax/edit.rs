use tree_sitter::Point;
use xi_rope::{multiset::CountMatcher, Interval, Rope, RopeDelta};

use crate::buffer::InsertsValueIter;

fn point_at_offset(text: &Rope, offset: usize) -> Point {
    let line = text.line_of_offset(offset);
    let col = text.offset_of_line(line + 1) - offset;
    Point::new(line, col)
}

fn traverse(point: Point, text: &str) -> Point {
    let Point {
        mut row,
        mut column,
    } = point;

    for ch in text.chars() {
        if ch == '\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn create_insert_edit(
    old_text: &Rope,
    start: usize,
    inserted: &Rope,
) -> tree_sitter::InputEdit {
    let start_position = point_at_offset(old_text, start);
    tree_sitter::InputEdit {
        start_byte: start,
        old_end_byte: start,
        new_end_byte: start + inserted.len(),
        start_position,
        old_end_position: start_position,
        new_end_position: traverse(
            start_position,
            &inserted.slice_to_cow(0..inserted.len()),
        ),
    }
}

fn create_delete_edit(
    old_text: &Rope,
    start: usize,
    end: usize,
) -> tree_sitter::InputEdit {
    let start_position = point_at_offset(old_text, start);
    let end_position = point_at_offset(old_text, end);
    tree_sitter::InputEdit {
        start_byte: start,
        // The old end byte position was at the end
        old_end_byte: end,
        // but since we're deleting everything up to it, it gets 'moved' to where we start
        new_end_byte: start,

        start_position,
        old_end_position: end_position,
        new_end_position: start_position,
    }
}

/// Add to a vector of treesitter edits from a delta
pub fn generate_edits(
    old_text: &Rope,
    delta: &RopeDelta,
    edits: &mut Vec<tree_sitter::InputEdit>,
) {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();
    if let Some(inserted) = delta.as_simple_insert() {
        edits.push(create_insert_edit(old_text, start, inserted));
    } else if delta.is_simple_delete() {
        edits.push(create_delete_edit(old_text, start, end));
    } else {
        // TODO: This probably generates more insertions/deletions than it needs to.
        // It also creates a bunch of deltas and intermediate ropes which are not truly needed
        // Which is why, for the common case of simple inserts/deletions, we use the above logic

        // Break the delta into two parts, the insertions and the deletions
        // This makes it easier to translate them into the tree_sitter::InputEdit format
        let (insertions, deletions) = delta.clone().factor();

        let mut text = old_text.clone();
        for insert in InsertsValueIter::new(&insertions) {
            // We may not need the inserted text in order to calculate the new end position
            // but I was sufficiently uncertain, and so continued with how we did it previously
            let start = insert.old_offset;
            let inserted = insert.node;
            edits.push(create_insert_edit(&text, start, inserted));

            // Create a delta of this specific part of the insert
            // We have to apply it because future inserts assume it already happened
            let insert_delta = RopeDelta::simple_edit(
                Interval::new(start, start),
                inserted.clone(),
                text.len(),
            );
            text = insert_delta.apply(&text);
        }

        // We have to keep track of a shift because the deletions aren't properly moved forward
        let mut shift = insertions.inserts_len();
        // I believe this is the correct `CountMatcher` to use for this iteration, since it is what they use
        // for deleting a subset from a string.
        for (start, end) in deletions.range_iter(CountMatcher::Zero) {
            edits.push(create_delete_edit(&text, start + shift, end + shift));

            let delete_delta = RopeDelta::simple_edit(
                Interval::new(start + shift, end + shift),
                Rope::default(),
                text.len(),
            );
            text = delete_delta.apply(&text);
            shift -= end - start;
        }
    }
}
