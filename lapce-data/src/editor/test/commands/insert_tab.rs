use crate::editor::{commands::EditCommandKind, test::MockEditor};

#[test]
fn insert_tab_inserts_spaces() {
    let mut editor = MockEditor::new("<$0>");

    editor.command(EditCommandKind::InsertTab);

    assert_eq!("    <$0>", editor.state());
}
