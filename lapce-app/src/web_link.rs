use floem::{
    peniko::Color,
    style::CursorStyle,
    view::View,
    views::{label, Decorators},
};

use crate::{command::InternalCommand, listener::Listener};

pub fn web_link(
    text: impl Fn() -> String + 'static,
    uri: impl Fn() -> String + 'static,
    color: impl Fn() -> Color + 'static,
    internal_command: Listener<InternalCommand>,
) -> impl View {
    label(text)
        .on_click(move |_| {
            internal_command.send(InternalCommand::OpenWebUri { uri: uri() });
            true
        })
        .base_style(move |s| s.color(color()))
        .hover_style(move |s| s.cursor(CursorStyle::Pointer))
}
