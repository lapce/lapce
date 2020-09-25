mod buffer;
mod command;
mod container;
mod editor;
mod font;
mod palette;
mod scroll;
mod split;
mod state;
mod theme;

use std::{sync::Arc, thread};

use crate::container::LapceContainer;
use crate::editor::Editor;
use crate::palette::Palette;
use crate::split::LapceSplit;
use crate::state::LAPCE_STATE;

use druid::{piet::Color, FontDescriptor, FontFamily, Size};
use druid::{
    widget::{Align, Container, Flex, Label, Padding, Scroll, Split},
    Point,
};
use druid::{AppLauncher, LocalizedString, Widget, WidgetExt, WindowDesc};
use palette::PaletteWrapper;

fn build_app() -> impl Widget<u32> {
    let container = LapceContainer::new();
    container
        .env_scope(|env: &mut druid::Env, data: &u32| {
            env.set(theme::LapceTheme::EDITOR_LINE_HEIGHT, 25.0);
            env.set(
                theme::LapceTheme::PALETTE_BACKGROUND,
                Color::rgb8(125, 125, 125),
            );
            env.set(
                theme::LapceTheme::PALETTE_INPUT_FOREROUND,
                Color::rgb8(0, 0, 0),
            );
            env.set(
                theme::LapceTheme::PALETTE_INPUT_BACKGROUND,
                Color::rgb8(255, 255, 255),
            );
            env.set(
                theme::LapceTheme::PALETTE_INPUT_BORDER,
                Color::rgb8(0, 0, 0),
            );
            env.set(
                theme::LapceTheme::EDITOR_FONT,
                FontDescriptor::new(FontFamily::new_unchecked("Cascadia Code"))
                    .with_size(13.0),
            );
            env.set(
                theme::LapceTheme::EDITOR_CURSOR_COLOR,
                Color::rgba8(255, 255, 255, 200),
            );
            env.set(
                theme::LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                Color::rgba8(255, 255, 255, 100),
            )
        })
        .debug_invalidation()
}

pub fn main() {
    let window = WindowDesc::new(build_app)
        .title(
            LocalizedString::new("split-demo-window-title")
                .with_placeholder("Split Demo"),
        )
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));

    let launcher = AppLauncher::with_window(window);
    let ui_event_sink = launcher.get_external_handle();
    LAPCE_STATE.set_ui_sink(ui_event_sink);
    thread::spawn(move || {
        LAPCE_STATE.open_file("/Users/Lulu/lapce/src/editor.rs")
    });
    launcher
        .use_simple_logger()
        .launch(0u32)
        .expect("launch failed");
}
