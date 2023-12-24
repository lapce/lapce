use std::rc::Rc;

use floem::{
    reactive::Scope,
    style::CursorStyle,
    view::View,
    views::{scroll, Decorators},
};
use floem_editor::{
    color::EditorColor,
    editor::Editor,
    id::EditorId,
    text::{default_dark_color, SimpleStyling, TextDocument},
    view::editor_view,
};

fn main() {
    let cx = Scope::new();
    let doc = TextDocument::new(cx, "Hello, world!");
    let doc = Rc::new(doc);

    let style = SimpleStyling::new(default_dark_color);
    let style = Rc::new(style);

    let id = EditorId::next();
    let editor = Editor::new(cx, id, doc, style, None);

    floem::launch(move || app_view(editor.clone()));
}

fn app_view(editor: Rc<Editor>) -> impl View {
    let background = editor.color(EditorColor::Background);
    // TODO: this should use editor_content
    scroll(editor_view(editor, |_| true).style(move |s| {
        s.absolute()
            .padding_bottom(0.0)
            .cursor(CursorStyle::Text)
            .min_size_pct(100.0, 100.0)
    }))
    .style(move |s| s.absolute().size_pct(100.0, 100.0).background(background))
}
