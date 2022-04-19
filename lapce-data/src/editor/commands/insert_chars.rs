use itertools::Itertools;
use lapce_core::syntax::Syntax;
use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        get_word_property, matching_char, matching_pair_direction, EditType,
        WordProperty,
    },
    movement::{ColPosition, Cursor, CursorMode, InsertDrift, SelRegion, Selection},
};

pub struct InsertCharsCommand<'a> {
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
    pub(super) chars: &'a str,
    pub(super) syntax: Option<Syntax>,
}

impl<'a> InsertCharsCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        fn region_to_selection(region: SelRegion) -> Selection {
            let mut current_selection = Selection::new();
            current_selection.add_region(region);

            current_selection
        }

        let Self {
            cursor,
            tab_width,
            chars,
            syntax,
        } = self;

        let mut selection = cursor.edit_selection(buffer.buffer, tab_width);

        if chars.chars().count() != 1 {
            let delta =
                buffer.edit_multiple(&[(&selection, chars)], EditType::InsertChars);
            let selection =
                selection.apply_delta(&delta, true, InsertDrift::Default);

            *cursor = Cursor::new(CursorMode::Insert(selection.clone()), None);
            return None;
        }

        let c = chars.chars().next().unwrap();
        let matching_pair_type = matching_pair_direction(c);

        // The main edit operations
        let mut edits = vec![];

        // "Late edits" - characters to be inserted after particular regions
        let mut edits_after = vec![];

        // Create edits
        for (idx, region) in selection.regions_mut().iter_mut().enumerate() {
            let offset = region.end;
            let cursor_char = buffer.buffer.char_at_offset(offset);

            if matching_pair_type == Some(false) {
                if cursor_char == Some(c) {
                    // Skip the closing character
                    let new_offset = buffer.buffer.next_grapheme_offset(
                        offset,
                        1,
                        buffer.buffer.len(),
                    );

                    *region = SelRegion::caret(new_offset);
                    continue;
                }

                let line = buffer.buffer.line_of_offset(offset);
                let line_start = buffer.buffer.offset_of_line(line);
                if buffer.buffer.slice_to_cow(line_start..offset).trim() == "" {
                    let opening_character = matching_char(c).unwrap();
                    if let Some(previous_offset) = buffer.buffer.previous_unmatched(
                        syntax.as_ref(),
                        opening_character,
                        offset,
                    ) {
                        // Auto-indent closing character to the same level as the opening.
                        let previous_line =
                            buffer.buffer.line_of_offset(previous_offset);
                        let line_indent =
                            buffer.buffer.indent_on_line(previous_line);

                        let current_selection = region_to_selection(SelRegion::new(
                            line_start, offset, None,
                        ));

                        edits.push((current_selection, format!("{line_indent}{c}")));
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

            let current_selection = region_to_selection(*region);

            edits.push((current_selection, c.to_string()));
        }

        // Apply edits to current selection
        let edits = edits
            .iter()
            .map(|(selection, content)| (selection, content.as_str()))
            .collect::<Vec<_>>();

        let delta = buffer.edit_multiple(&edits, EditType::InsertChars);

        // Update selection
        let mut selection =
            selection.apply_delta(&delta, true, InsertDrift::Default);

        // Apply late edits
        let edits_after = edits_after
            .iter()
            .map(|(idx, content)| {
                (
                    region_to_selection(selection.regions()[*idx]),
                    content.to_string(),
                )
            })
            .collect::<Vec<_>>();

        let edits_after = edits_after
            .iter()
            .map(|(selection, content)| (selection, content.as_str()))
            .collect::<Vec<_>>();

        buffer.edit_multiple(&edits_after, EditType::InsertChars);

        // Adjust selection according to previous late edits
        let mut adjustment = 0;
        for region in selection
            .regions_mut()
            .iter_mut()
            .sorted_by(|region_a, region_b| region_a.start().cmp(&region_b.start()))
        {
            *region = SelRegion::new(
                region.start + adjustment,
                region.end + adjustment,
                region.horiz().map(|pos| {
                    if let ColPosition::Col(col) = pos {
                        ColPosition::Col(*col + adjustment)
                    } else {
                        *pos
                    }
                }),
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

        *cursor = Cursor::new(CursorMode::Insert(selection), None);

        None
    }
}

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn characters_are_inserted_where_the_cursor_is() {
        let mut editor = MockEditor::new("foo<$0>baz");

        editor.command(EditCommandKind::InsertChars { chars: "b" });
        editor.command(EditCommandKind::InsertChars { chars: "a" });
        editor.command(EditCommandKind::InsertChars { chars: "r" });

        assert_eq!("foobar<$0>baz", editor.state());
    }

    #[test]
    fn characters_are_inserted_where_the_cursors_are() {
        let mut editor = MockEditor::new("foo<$0>baz<$1>");

        editor.command(EditCommandKind::InsertChars { chars: "b" });
        editor.command(EditCommandKind::InsertChars { chars: "a" });
        editor.command(EditCommandKind::InsertChars { chars: "r" });

        assert_eq!("foobar<$0>bazbar<$1>", editor.state());
    }

    #[test]
    fn can_insert_matching_pair() {
        let mut editor = MockEditor::new("foo<$0>");

        editor.command(EditCommandKind::InsertChars { chars: "(" });
        editor.command(EditCommandKind::InsertChars { chars: "[" });
        editor.command(EditCommandKind::InsertChars { chars: "{" });

        assert_eq!("foo([{<$0>}])", editor.state());
    }

    #[test]
    fn can_insert_matching_pair_multi() {
        let mut editor = MockEditor::new("foo<$0> bar<$1>");

        editor.command(EditCommandKind::InsertChars { chars: "(" });
        editor.command(EditCommandKind::InsertChars { chars: "[" });
        editor.command(EditCommandKind::InsertChars { chars: "{" });

        assert_eq!("foo([{<$0>}]) bar([{<$1>}])", editor.state());
    }

    #[test]
    fn inserting_matching_pair_just_skips_over() {
        let mut editor = MockEditor::new("foo<$0>");

        editor.command(EditCommandKind::InsertChars { chars: "(" });
        editor.command(EditCommandKind::InsertChars { chars: "[" });
        editor.command(EditCommandKind::InsertChars { chars: "{" });
        editor.command(EditCommandKind::InsertChars { chars: "}" });
        editor.command(EditCommandKind::InsertChars { chars: "]" });
        editor.command(EditCommandKind::InsertChars { chars: ")" });

        assert_eq!("foo([{}])<$0>", editor.state());
    }

    #[test]
    fn inserting_matching_pair_just_skips_over_multi() {
        let mut editor = MockEditor::new("foo<$0> bar<$1>");

        editor.command(EditCommandKind::InsertChars { chars: "(" });
        editor.command(EditCommandKind::InsertChars { chars: "[" });
        editor.command(EditCommandKind::InsertChars { chars: "{" });
        editor.command(EditCommandKind::InsertChars { chars: "}" });
        editor.command(EditCommandKind::InsertChars { chars: "]" });
        editor.command(EditCommandKind::InsertChars { chars: ")" });

        assert_eq!("foo([{}])<$0> bar([{}])<$1>", editor.state());
    }

    #[test]
    fn does_not_insert_matching_pair_inside_word() {
        let mut editor = MockEditor::new("foo<$0>bar");

        editor.command(EditCommandKind::InsertChars { chars: "(" });
        editor.command(EditCommandKind::InsertChars { chars: "[" });
        editor.command(EditCommandKind::InsertChars { chars: "{" });

        assert_eq!("foo([{<$0>bar", editor.state());
    }

    #[test]
    fn typing_character_overwrites_selection() {
        let mut editor = MockEditor::new("<$0>foo</$0>");

        editor.command(EditCommandKind::InsertChars { chars: "b" });
        editor.command(EditCommandKind::InsertChars { chars: "a" });
        editor.command(EditCommandKind::InsertChars { chars: "r" });

        assert_eq!("bar<$0>", editor.state());
    }

    #[test]
    fn typing_character_overwrites_selection_multi() {
        let mut editor = MockEditor::new("<$0>foo</$0> <$1>baz</$1>");

        editor.command(EditCommandKind::InsertChars { chars: "b" });
        editor.command(EditCommandKind::InsertChars { chars: "a" });
        editor.command(EditCommandKind::InsertChars { chars: "r" });

        assert_eq!("bar<$0> bar<$1>", editor.state());
    }

    #[test]
    fn inserting_matching_pair_correctly_cursor_positions() {
        let mut editor = MockEditor::new("a<$0>b<$1> \n<$2>");

        editor.command(EditCommandKind::InsertChars { chars: "(" });

        assert_eq!("a(<$0>b(<$1>) \n(<$2>)", editor.state());
    }
}
