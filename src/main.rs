mod app;
mod config;
mod editor;
mod input;
mod line_cache;
mod palette;
mod rpc;
mod xi_thread;

use app::{App, AppDispatcher};
use config::{AppFont, Config};
use crane_ui::{Column, UiHandler, Widget, WidgetTrait};
use druid::piet;
use druid::shell::{runloop, WindowBuilder};
use palette::Palette;
use piet::Color;
use rpc::Core;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use xi_thread::start_xi_thread;

fn main() {
    druid::shell::init();

    let (xi_peer, rx) = start_xi_thread();
    let dispatcher = AppDispatcher::new();

    let mut run_loop = runloop::RunLoop::new();
    let mut builder = WindowBuilder::new();
    // let mut col = Column::new();
    // let state = UiState::new(col, "sdlkfjdslkfjdsklfj".to_string());
    let main_flex = Column::new();
    let main_widget = Widget::new();
    main_widget.add_child(Box::new(main_flex.clone()));
    let ui_handler = UiHandler::new(Arc::new(main_widget.clone()));
    builder.set_title("Crane");
    builder.set_handler(Box::new(ui_handler));
    let window = builder.build().unwrap();
    let idle_handle = window.get_idle_handle().unwrap();

    let core = Core::new(xi_peer, rx, dispatcher.clone());
    let font = AppFont::new("Consolas", 13.0, 11.0);
    let config = Config::new(font);
    let mut app = App::new(core, window.clone(), idle_handle, main_flex.clone(), config);
    dispatcher.set_app(&app);

    let palette = Palette::new(app.clone());
    palette.set_size(500.0, 500.0);
    palette.set_background(Color::rgb8(33, 37, 43));
    palette.set_shadow(0.0, 0.0, 5.0, 0.0, Color::rgba8(0, 0, 0, 200));
    main_widget.add_child(Box::new(palette.clone()));
    app.set_palette(palette.clone());

    app.send_notification(
        "client_started",
        &json!({
            "client_extras_dir": "/Users/Lulu/.crane/",
            "config_dir": "/Users/Lulu/.crane"
        }),
    );
    app.send_notification(
        "set_theme",
        &json!({
            "theme_name": "one_dark"
        }),
    );

    let editor = app.new_editor();
    editor.set_active();
    app.set_active_editor(&editor);
    editor.load_file("/Users/Lulu/crane/src/app.rs".to_string());

    window.show();
    palette.hide();
    run_loop.run();
}
