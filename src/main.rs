mod app;
mod config;
mod editor;
mod input;
mod line_cache;
mod rpc;
mod xi_thread;

use app::{App, AppDispatcher};
use config::{AppFont, Config};
use crane_ui::{Column, UiHandler};
use druid::shell::{runloop, WindowBuilder};
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
    let main_widget = Arc::new(Column::new());
    let ui_handler = UiHandler::new(main_widget.clone());
    builder.set_title("Crane");
    builder.set_handler(Box::new(ui_handler));
    let window = builder.build().unwrap();
    let idle_handle = window.get_idle_handle().unwrap();

    let core = Core::new(xi_peer, rx, dispatcher.clone());
    let font = AppFont::new("Consolas", 13.0, 11.0);
    let config = Config::new(font);
    let app = App::new(core, window.clone(), idle_handle, main_widget, config);
    dispatcher.set_app(&app);

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
    app.req_new_view(Some("/Users/Lulu/crane/src/app.rs"));

    window.show();
    run_loop.run();
}
