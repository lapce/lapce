use crate::editor::{commands::EditCommandKind, test::MockEditor};

#[test]
fn insert_tab_inserts_spaces() {
    let mut editor = MockEditor::new("<$0>");

    editor.command(EditCommandKind::InsertTab);

    assert_eq!("    <$0>", editor.state());
}

#[test]
fn insert_tab_inserts_at_multiple_places() {
    let mut editor = MockEditor::new(
        r#"<$0>
<$1>"#,
    );

    editor.command(EditCommandKind::InsertTab);

    assert_eq!(
        r#"    <$0>
    <$1>"#,
        editor.state()
    );
}
