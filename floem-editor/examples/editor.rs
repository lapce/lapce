use std::rc::Rc;

use floem::{
    reactive::{create_rw_signal, RwSignal, Scope},
    view::View,
};
use floem_editor::{
    editor::Editor,
    id::EditorId,
    text::{default_dark_color, SimpleStyling, TextDocument},
    view::editor_container_view,
};

fn main() {
    let cx = Scope::new();
    let doc = TextDocument::new(cx, "Hello, world!");
    let doc = Rc::new(doc);

    let style = SimpleStyling::new(default_dark_color);
    let style = Rc::new(style);

    let id = EditorId::next();
    let editor = Editor::new(cx, id, doc, style, None);
    let editor = create_rw_signal(editor);

    floem::launch(move || app_view(editor));
}

fn app_view(editor: RwSignal<Rc<Editor>>) -> impl View {
    editor_container_view(editor, |_| true)
}
