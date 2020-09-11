mod split;

use crate::split::CraneSplit;

use druid::piet::Color;
use druid::widget::{Align, Container, Label, Padding};
use druid::{AppLauncher, LocalizedString, Widget, WindowDesc};

fn build_app() -> impl Widget<u32> {
    CraneSplit::new(true)
        .with_child(Label::new("Hello"))
        .with_child(Label::new("World"))
        .with_child(Label::new("World"))
        .with_child(Label::new("World"))
}

pub fn main() {
    let window = WindowDesc::new(build_app).title(
        LocalizedString::new("split-demo-window-title")
            .with_placeholder("Split Demo"),
    );
    AppLauncher::with_window(window)
        .use_simple_logger()
        .launch(0u32)
        .expect("launch failed");
}
