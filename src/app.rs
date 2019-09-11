use crate::editor::EditViewCommands;
use crate::editor::{Editor, EditorView};
use crate::line_cache::Style;
use crate::rpc::{Core, Handler};
use crane_ui::Widget;
use druid::shell::platform::IdleHandle;
use druid::shell::platform::WindowHandle;
use serde_json::{self, json, Value};
use std::cell::RefCell;
// use std::marker::{Send, Sync};
use crate::config::AppFont;
use crate::config::Config;
use crate::editor::View;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use syntect::highlighting::ThemeSettings;

#[derive(Clone)]
pub struct AppState {
    pub active_editor: String,
}

impl AppState {
    fn new() -> AppState {
        AppState {
            active_editor: "".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct App {
    pub core: Core,
    pub state: Arc<Mutex<AppState>>,
    pub idle_handle: IdleHandle,
    pub window_handle: WindowHandle,
    pub main_flex: Arc<Widget>,
    pub views: Arc<Mutex<HashMap<String, View>>>,
    pub editors: Arc<Mutex<HashMap<String, Editor>>>,
    pub config: Config,
}

fn rgba_from_argb(argb: u32) -> u32 {
    let a = (argb >> 24) & 0xff;
    let r = (argb >> 16) & 0xff;
    let g = (argb >> 8) & 0xff;
    let b = argb & 0xff;
    (r << 24) | (g << 16) | (b << 8) | 0xff
}

impl App {
    pub fn new(
        core: Core,
        window_handle: WindowHandle,
        idle_handle: IdleHandle,
        main_flex: Arc<Widget>,
        config: Config,
    ) -> App {
        App {
            state: Arc::new(Mutex::new(AppState::new())),
            core,
            window_handle,
            idle_handle,
            main_flex,
            views: Arc::new(Mutex::new(HashMap::new())),
            editors: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub fn set_active_editor(&self, editor: &Editor) {
        let id = editor.id().clone();
        self.state.lock().unwrap().active_editor = id;
    }

    pub fn get_active_editor(&self) -> Editor {
        let id = self.state.lock().unwrap().active_editor.clone();
        self.editors.lock().unwrap().get(&id).unwrap().clone()
    }

    pub fn req_new_view(&self, filename: Option<&str>) {
        let mut params = json!({});

        let filename = if filename.is_some() {
            params["file_path"] = json!(filename.unwrap());
            Some(filename.unwrap().to_string())
        } else {
            None
        };

        let edit_view = 0;
        let core = self.core.clone();
        let idle_handle = self.idle_handle.clone();
        let main_flex = self.main_flex.clone();
        let views = self.views.clone();
        let editors = self.editors.clone();
        let config = self.config.clone();
        let config_for_view = self.config.clone();
        let window_handle = self.window_handle.clone();

        let app = self.clone();

        self.core.send_request("new_view", &params, move |value| {
            let view_id = value.as_str().unwrap().to_string();
            let view = View::new(view_id.clone(), app.clone());
            views.lock().unwrap().insert(view_id.clone(), view.clone());
            let editor = Editor::new(app.clone());
            editors
                .lock()
                .unwrap()
                .insert(editor.id().clone(), editor.clone());
            app.set_active_editor(&editor);
            editor.load_view(view);
            editor.set_active();
            main_flex.add_child(Box::new(editor));
        });
        // self.core.send_request("new_view", &params, move |value| {
        //     println!("{:?}", value);
        //     let view_id = value.as_str().unwrap().to_string();
        //     let view = Arc::new(Mutex::new(View::new(
        //         view_id.clone(),
        //         editor_views.clone(),
        //         config_for_view,
        //     )));
        //     views.lock().unwrap().insert(view_id.clone(), view.clone());

        //     // let editor_view = Arc::new(EditorView::new(
        //     //     idle_handle.clone(),
        //     //     window_handle,
        //     //     core.clone(),
        //     //     view.clone(),
        //     //     config,
        //     // ));
        //     // editor_views
        //     //     .lock()
        //     //     .unwrap()
        //     //     .insert(editor_view.lock().unwrap().id(), editor_view.clone());
        //     // view.clone()
        //     //     .lock()
        //     //     .unwrap()
        //     //     .set_editor_view(editor_view.clone());
        //     // main_flex.lock().unwrap().add_child(editor_view.clone());
        //     // editor_view.lock().unwrap().set_parent(main_flex);
        //     // idle_handle.add_idle(move |_| {
        //     //     println!("run idle");
        //     // });
        // });
    }

    pub fn send_notification(&self, method: &str, params: &Value) {
        self.core.send_notification(method, params);
    }

    fn send_view_cmd(&self, cmd: EditViewCommands) {}

    fn handle_cmd(&self, method: &str, params: &Value) {
        match method {
            "update" => {
                let view = self
                    .views
                    .lock()
                    .unwrap()
                    .get(params["view_id"].as_str().unwrap())
                    .unwrap()
                    .clone();
                view.apply_update(&params["update"]);
                // println!("{}", params);
                // let window_handle = self.window_handle.clone();
                // self.idle_handle.add_idle(move |_| {
                //     // window_handle.invalidate();
                // });
            }
            "def_style" => {
                let mut style: Style = serde_json::from_value(params.clone()).unwrap();
                style.fg_color = style.fg_color.map(|c| rgba_from_argb(c));
                self.config
                    .styles
                    .lock()
                    .unwrap()
                    .insert(style.id.clone(), style);
                // println!("{:?}", params);
            }
            "scroll_to" => {
                let view = self
                    .views
                    .lock()
                    .unwrap()
                    .get(params["view_id"].as_str().unwrap())
                    .unwrap()
                    .clone();
                let col = params["col"].as_u64().unwrap();
                let line = params["line"].as_u64().unwrap();
                view.scroll_to(col, line);
            }
            // "available_themes" => (),    // TODO
            // "available_plugins" => (),   // TODO
            // "available_languages" => (), // TODO
            // "config_changed" => (),      // TODO
            "theme_changed" => {
                let theme_setting: ThemeSettings =
                    serde_json::from_value(params["theme"].clone()).unwrap();
                let mut config_theme = self.config.theme.lock().unwrap();
                config_theme.background = theme_setting.background;
                config_theme.foreground = theme_setting.foreground;
            }
            _ => println!("unhandled core->fe method {}, {:?}", method, params),
            // _ => (),
        }
    }
}

#[derive(Clone)]
pub struct AppDispatcher {
    app: Arc<Mutex<Option<App>>>,
}

impl AppDispatcher {
    pub fn new() -> AppDispatcher {
        AppDispatcher {
            app: Default::default(),
        }
    }

    pub fn set_app(&self, app: &App) {
        *self.app.lock().unwrap() = Some(app.clone());
    }
}

impl Handler for AppDispatcher {
    fn notification(&self, method: &str, params: &Value) {
        // NOTE: For debugging, could be replaced by trace logging
        // println!("core->fe: {} {}", method, params);
        if let Some(ref app) = *self.app.lock().unwrap() {
            app.handle_cmd(method, params);
        }
    }
}
