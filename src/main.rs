use std::{sync::Arc, thread, time::Duration};

use lapce_core::container::LapceContainer;
use lapce_core::editor::Editor;
use lapce_core::palette::Palette;
use lapce_core::split::LapceSplit;
use lapce_core::theme::LapceTheme;

use druid::{
    piet::Color, FontDescriptor, FontFamily, FontWeight, Key, Size, Target, WidgetId,
};
use druid::{
    widget::IdentityWrapper,
    widget::{Align, Container, Flex, Label, Padding, Scroll, Split},
    Point,
};
use druid::{AppLauncher, LocalizedString, Widget, WidgetExt, WindowDesc};
use lapce_core::command::{LapceUICommand, LAPCE_UI_COMMAND};
use lapce_core::explorer::FileExplorer;
use lapce_core::state::{LapceState, LapceUIState, LAPCE_STATE};
use tree_sitter::{Language, Parser};

extern "C" {
    fn tree_sitter_rust() -> Language;
}

fn build_app() -> impl Widget<LapceUIState> {
    let container_id = WidgetId::next();
    let container =
        IdentityWrapper::wrap(LapceContainer::new(), container_id.clone());
    // LAPCE_STATE.set_container(container_id);
    let main_split = LapceSplit::new(true)
        .with_child(FileExplorer::new(), 300.0)
        .with_flex_child(container, 1.0);
    main_split
        .env_scope(|env: &mut druid::Env, data: &LapceUIState| {
            let theme = &LAPCE_STATE.theme;
            if let Some(line_highlight) = theme.get("line_highlight") {
                env.set(
                    LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                    line_highlight.clone(),
                );
            };
            if let Some(caret) = theme.get("caret") {
                env.set(LapceTheme::EDITOR_CURSOR_COLOR, caret.clone());
            };
            if let Some(foreground) = theme.get("foreground") {
                env.set(LapceTheme::EDITOR_FOREGROUND, foreground.clone());
            };
            if let Some(selection) = theme.get("selection") {
                env.set(LapceTheme::EDITOR_SELECTION_COLOR, selection.clone());
            };
            env.set(LapceTheme::EDITOR_LINE_HEIGHT, 25.0);
            env.set(LapceTheme::PALETTE_BACKGROUND, Color::rgb8(125, 125, 125));
            env.set(LapceTheme::PALETTE_INPUT_FOREROUND, Color::rgb8(0, 0, 0));
            env.set(
                LapceTheme::PALETTE_INPUT_BACKGROUND,
                Color::rgb8(255, 255, 255),
            );
            env.set(LapceTheme::PALETTE_INPUT_BORDER, Color::rgb8(0, 0, 0));
            env.set(
                LapceTheme::EDITOR_FONT,
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
    {
        // only for #[cfg]
        use parking_lot::deadlock;
        use std::thread;
        use std::time::Duration;

        // Create a background thread which checks for deadlocks every 10s
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(10));
            let deadlocks = deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }

            println!("{} deadlocks detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                println!("Deadlock #{}", i);
                for t in threads {
                    println!("Thread Id {:#?}", t.thread_id());
                    println!("{:#?}", t.backtrace());
                }
            }
        });
    }
    // WindowDesc::new(|| LapceContainer::new());
    let window = WindowDesc::new(build_app)
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
                "/Users/Lulu/go/src/uni/main.go".to_string(),
                // "/Users/Lulu/lapce/core/src/editor.rs".to_string(),
            ),
            Target::Global,
        );
    });
    LAPCE_STATE.set_ui_sink(launcher.get_external_handle());
    // thread::spawn(move || {
    //     LAPCE_STATE.open_file("/Users/Lulu/lapce/src/editor.rs")
    // });
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_rust() };
    parser.set_language(language);
    parser.parse("pub fn main() {}", None).unwrap();
    let ui_state = LapceUIState::new();
    launcher
        .use_simple_logger()
        .launch(ui_state)
        .expect("launch failed");
}
