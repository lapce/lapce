use lapce_xi_rope::Rope;

use crate::{
    buffer::Buffer,
    chars::{char_is_line_ending, char_is_whitespace},
    selection::Selection,
};

/// Enum representing indentation style.
///
/// Only values 1-8 are valid for the `Spaces` variant.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum IndentStyle {
    Tabs,
    Spaces(u8),
}

impl IndentStyle {
    pub const LONGEST_INDENT: &'static str = "        "; // 8 spaces
    pub const DEFAULT_INDENT: IndentStyle = IndentStyle::Spaces(4);

    /// Creates an `IndentStyle` from an indentation string.
    ///
    /// For example, passing `"    "` (four spaces) will create `IndentStyle::Spaces(4)`.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn from_str(indent: &str) -> Self {
        debug_assert!(
            !indent.is_empty() && indent.len() <= Self::LONGEST_INDENT.len()
        );
        if indent.starts_with(' ') {
            IndentStyle::Spaces(indent.len() as u8)
        } else {
            IndentStyle::Tabs
        }
    }

    #[inline]
    pub fn as_str(&self) -> &'static str {
        match *self {
            IndentStyle::Tabs => "\t",
            IndentStyle::Spaces(x) if x <= Self::LONGEST_INDENT.len() as u8 => {
                Self::LONGEST_INDENT.split_at(x.into()).0
            }
            // Unsupported indentation style.  This should never happen,
            // but just in case fall back to the default of 4 spaces
            IndentStyle::Spaces(n) => {
                debug_assert!(n > 0 && n <= Self::LONGEST_INDENT.len() as u8);
                "    "
            }
        }
    }
}

pub fn create_edit<'s>(
    buffer: &Buffer,
    offset: usize,
    indent: &'s str,
) -> (Selection, &'s str) {
    let indent = if indent.starts_with('\t') {
        indent
    } else {
        let (_, col) = buffer.offset_to_line_col(offset);
        indent.split_at(indent.len() - col % indent.len()).0
    };
    (Selection::caret(offset), indent)
}

pub fn create_outdent<'s>(
    buffer: &Buffer,
    offset: usize,
    indent: &'s str,
) -> Option<(Selection, &'s str)> {
    let (_, col) = buffer.offset_to_line_col(offset);
    if col == 0 {
        return None;
    }

    let start = if indent.starts_with('\t') {
        offset - 1
    } else {
        let r = col % indent.len();
        let r = if r == 0 { indent.len() } else { r };
        offset - r
    };

    Some((Selection::region(start, offset), ""))
}

/// Attempts to detect the indentation style used in a document.
///
/// Returns the indentation style if the auto-detect confidence is
/// reasonably high, otherwise returns `None`.
pub fn auto_detect_indent_style(document_text: &Rope) -> Option<IndentStyle> {
    // Build a histogram of the indentation *increases* between
    // subsequent lines, ignoring lines that are all whitespace.
    //
    // Index 0 is for tabs, the rest are 1-8 spaces.
    let histogram: [usize; 9] = {
        let mut histogram = [0; 9];
        let mut prev_line_is_tabs = false;
        let mut prev_line_leading_count = 0usize;

        // Loop through the lines, checking for and recording indentation
        // increases as we go.
        let offset = document_text.offset_of_line(
            document_text.line_of_offset(document_text.len()).min(1000),
        );
        'outer: for line in document_text.lines(..offset) {
            let mut c_iter = line.chars();

            // Is first character a tab or space?
            let is_tabs = match c_iter.next() {
                Some('\t') => true,
                Some(' ') => false,

                // Ignore blank lines.
                Some(c) if char_is_line_ending(c) => continue,

                _ => {
                    prev_line_is_tabs = false;
                    prev_line_leading_count = 0;
                    continue;
                }
            };

            // Count the line's total leading tab/space characters.
            let mut leading_count = 1;
            let mut count_is_done = false;
            for c in c_iter {
                match c {
                    '\t' if is_tabs && !count_is_done => leading_count += 1,
                    ' ' if !is_tabs && !count_is_done => leading_count += 1,

                    // We stop counting if we hit whitespace that doesn't
                    // qualify as indent or doesn't match the leading
                    // whitespace, but we don't exit the loop yet because
                    // we still want to determine if the line is blank.
                    c if char_is_whitespace(c) => count_is_done = true,

                    // Ignore blank lines.
                    c if char_is_line_ending(c) => continue 'outer,

                    _ => break,
                }

                // Bound the worst-case execution time for weird text files.
                if leading_count > 256 {
                    continue 'outer;
                }
            }

            // If there was an increase in indentation over the previous
            // line, update the histogram with that increase.
            if (prev_line_is_tabs == is_tabs || prev_line_leading_count == 0)
                && prev_line_leading_count < leading_count
            {
                if is_tabs {
                    histogram[0] += 1;
                } else {
                    let amount = leading_count - prev_line_leading_count;
                    if amount <= 8 {
                        histogram[amount] += 1;
                    }
                }
            }

            // Store this line's leading whitespace info for use with
            // the next line.
            prev_line_is_tabs = is_tabs;
            prev_line_leading_count = leading_count;
        }

        // Give more weight to tabs, because their presence is a very
        // strong indicator.
        histogram[0] *= 2;

        histogram
    };

    // Find the most frequent indent, its frequency, and the frequency of
    // the next-most frequent indent.
    let indent = histogram
        .iter()
        .enumerate()
        .max_by_key(|kv| kv.1)
        .unwrap()
        .0;
    let indent_freq = histogram[indent];
    let indent_freq_2 = *histogram
        .iter()
        .enumerate()
        .filter(|kv| kv.0 != indent)
        .map(|kv| kv.1)
        .max()
        .unwrap();

    // Return the the auto-detected result if we're confident enough in its
    // accuracy, based on some heuristics.
    if indent_freq >= 1 && (indent_freq_2 as f64 / indent_freq as f64) < 0.66 {
        Some(match indent {
            0 => IndentStyle::Tabs,
            _ => IndentStyle::Spaces(indent as u8),
        })
    } else {
        None
    }
}
