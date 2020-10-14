mod buffer;
mod command;
mod container;
mod editor;
mod explorer;
mod font;
mod language;
mod movement;
mod palette;
mod plugin;
mod scroll;
mod split;
mod state;
mod theme;

use std::{sync::Arc, thread, time::Duration};

use crate::container::LapceContainer;
use crate::editor::Editor;
use crate::palette::Palette;
use crate::split::LapceSplit;

use command::{LapceUICommand, LAPCE_UI_COMMAND};
use druid::{
    piet::Color, FontDescriptor, FontFamily, FontWeight, Key, Size, Target,
    WidgetId,
};
use druid::{
    widget::IdentityWrapper,
    widget::{Align, Container, Flex, Label, Padding, Scroll, Split},
    Point,
};
use druid::{AppLauncher, LocalizedString, Widget, WidgetExt, WindowDesc};
use explorer::FileExplorer;
use state::LapceState;
use tree_sitter::{Language, Parser};

extern "C" {
    fn tree_sitter_rust() -> Language;
}

fn build_app(state: LapceState) -> impl Widget<LapceState> {
    let container_id = WidgetId::next();
    let container =
        IdentityWrapper::wrap(LapceContainer::new(state), container_id.clone());
    // LAPCE_STATE.set_container(container_id);
    let main_split = LapceSplit::new(true)
        .with_child(FileExplorer::new(), 300.0)
        .with_flex_child(container, 1.0);
    main_split
        .env_scope(|env: &mut druid::Env, data: &LapceState| {
            if let Some(line_highlight) = data.theme.get("line_highlight") {
                env.set(
                    theme::LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                    line_highlight.clone(),
                );
            };
            if let Some(caret) = data.theme.get("caret") {
                env.set(theme::LapceTheme::EDITOR_CURSOR_COLOR, caret.clone());
            };
            if let Some(foreground) = data.theme.get("foreground") {
                env.set(
                    theme::LapceTheme::EDITOR_FOREGROUND,
                    foreground.clone(),
                );
            };
            if let Some(selection) = data.theme.get("selection") {
                env.set(
                    theme::LapceTheme::EDITOR_SELECTION_COLOR,
                    selection.clone(),
                );
            };
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
        })
        .debug_invalidation()
    // Label::new("test label")
    //     .with_text_color(Color::rgb8(64, 120, 242))
    //     .background(Color::rgb8(64, 120, 242))
}

pub fn main() {
    // WindowDesc::new(|| LapceContainer::new());
    let state = LapceState::new();
    let init_state = state.clone();
    let window = WindowDesc::new(move || build_app(init_state))
        .title(
            LocalizedString::new("split-demo-window-title")
                .with_placeholder("Split Demo"),
        )
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));

    let launcher = AppLauncher::with_window(window);
    let ui_event_sink = launcher.get_external_handle();
    thread::spawn(move || {
        ui_event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::OpenFile(
                "/Users/Lulu/lapce/src/editor.rs".to_string(),
            ),
            Target::Global,
        );
    });
    // LAPCE_STATE.set_ui_sink(ui_event_sink);
    // thread::spawn(move || {
    //     LAPCE_STATE.open_file("/Users/Lulu/lapce/src/editor.rs")
    // });
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_rust() };
    parser.set_language(language);
    parser.parse("pub fn main() {}", None).unwrap();
    launcher
        .use_simple_logger()
        .launch(state)
        .expect("launch failed");
}
