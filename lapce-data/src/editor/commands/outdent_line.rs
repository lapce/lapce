
use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    movement::{Cursor, Selection},
};

use super::indent;

pub struct OutdentLineCommand<'a> {
    pub(super) selection: Option<Selection>,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> OutdentLineCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let Self {
            selection,
            cursor,
            tab_width,
        } = self;

        Some(indent::create_multi_edits(
            buffer, selection, cursor, tab_width, edit_one_line
        ))
    }
}

fn edit_one_line<'s, 'b, L: BufferDataListener>(
    buffer: &EditableBufferData<'b, L>,
    offset: usize,
    indent: &'s str,
    tab_width: usize,
) -> Option<(Selection, &'s str)> {
    let (_, col) = buffer.offset_to_line_col(offset, tab_width);
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

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn outdent_does_nothing_if_line_is_not_indented() {
        let mut editor = MockEditor::new("line\n<$0>foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_single_indented_line() {
        let mut editor = MockEditor::new("line\n<$0>    foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_partially_selected_line() {
        let mut editor = MockEditor::new("line\n    f<$0>o</$0>o");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\nf<$0>o</$0>o", editor.state());
    }

    #[test]
    fn outdent_removes_incomplete_indentation() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n<$0>   foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_multiple_lines() {
        let mut editor = MockEditor::new("line\n<$0>    foo\n    bar</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo\nbar</$0>", editor.state());
    }

    #[test]
    fn outdent_multiple_selections() {
        let mut editor = MockEditor::new("line\n<$0>    foo</$0>\n    b<$1>a</$1>r");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>\nb<$1>a</$1>r", editor.state());
    }
}
