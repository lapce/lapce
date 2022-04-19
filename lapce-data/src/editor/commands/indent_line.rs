
use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    movement::{Cursor, Selection},
};
use super::indentation;

pub struct IndentLineCommand<'a> {
    pub(super) selection: Option<Selection>,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> IndentLineCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let Self {
            selection,
            cursor,
            tab_width,
        } = self;

        Some(indentation::execute(
            buffer, selection, cursor, tab_width, indent_one_line
        ))
    }
}

fn indent_one_line<'s, 'b, L: BufferDataListener>(
    buffer: &EditableBufferData<'b, L>,
    offset: usize,
    indent: &'s str,
    tab_width: usize,
) -> Option<(Selection, &'s str)> {
    Some(indentation::create_edit(buffer, offset, indent, tab_width))
}

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn indent_single_line() {
        let mut editor = MockEditor::new("line\n<$0>foo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    <$0>foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_indents_to_next_width_if_leading_space_is_selected() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n<$0>   foo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n<$0>    foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_partially_indented_line_indents_to_next_width() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n   fo<$0>o</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    fo<$0>o</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_line_indents_partially_selected() {
        let mut editor = MockEditor::new("line\nf<$0>o</$0>o\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    f<$0>o</$0>o\nthird line", editor.state());
    }

    #[test]
    fn indent_multiple_lines() {
        let mut editor = MockEditor::new("li<$0>ne\nfoo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("    li<$0>ne\n    foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_multiple_selections() {
        let mut editor = MockEditor::new("li<$0>n</$0>e\nfoo\nth<$1>ir</$1>d line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!(
            "    li<$0>n</$0>e\nfoo\n    th<$1>ir</$1>d line",
            editor.state()
        );
    }
}
