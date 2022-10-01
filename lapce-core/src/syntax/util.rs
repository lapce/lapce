use std::collections::HashMap;

use tree_sitter::TextProvider;
use xi_rope::{rope::ChunkIter, Rope};

use crate::buffer::Buffer;

pub struct RopeChunksIterBytes<'a> {
    chunks: ChunkIter<'a>,
}
impl<'a> Iterator for RopeChunksIterBytes<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(str::as_bytes)
    }
}

/// This allows tree-sitter to iterate over our Rope without us having to convert it into
/// a contiguous byte-list.
pub struct RopeProvider<'a>(pub &'a Rope);
impl<'a> TextProvider<'a> for RopeProvider<'a> {
    type I = RopeChunksIterBytes<'a>;
    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let start = node.start_byte();
        let end = node.end_byte().min(self.0.len());
        let chunks = self.0.iter_chunks(start..end);
        RopeChunksIterBytes { chunks }
    }
}

const PAIRS: &[(char, char)] = &[('(', ')'), ('{', '}'), ('[', ']')];

pub fn matching_pair_direction(c: char) -> Option<bool> {
    Some(match c {
        '{' => true,
        '}' => false,
        '(' => true,
        ')' => false,
        '[' => true,
        ']' => false,
        _ => return None,
    })
}

pub fn matching_char(c: char) -> Option<char> {
    Some(match c {
        '{' => '}',
        '}' => '{',
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        _ => return None,
    })
}

pub fn has_unmatched_pair(line: &str) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line.chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            pair_first.entry(key).or_insert(left);
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

pub fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
}

fn is_bracket(c: char) -> bool {
    PAIRS.iter().any(|(l, r)| *l == c || *r == c)
}

fn is_valid_pair(pair: &(char, char)) -> bool {
    PAIRS.contains(pair)
}

pub fn find_matching_bracket(
    buffer: &Buffer,
    offset: usize,
    start_line: usize,
    end_line: usize,
) -> Option<usize> {
    let char_at_cursor = buffer.char_at_offset(offset)?;

    if is_bracket(char_at_cursor) {
        let start = buffer.offset_of_line(start_line);
        let end = buffer.offset_of_line(end_line + 1);

        let direction = matching_pair_direction(char_at_cursor)?;

        if direction {
            let pos = buffer
                .char_indices_iter(offset..end)
                .position(|c| is_valid_pair(&(char_at_cursor, c.1)));

            if pos.is_some() {
                return offset.checked_add(pos.unwrap());
            }
        }
    }

    None
}
