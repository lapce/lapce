use crate::editor::EditViewCommands;
use crate::editor::EditorView;
use crate::line_cache::Style;
use crate::rpc::{Core, Handler};
use crate::ui::flex::Flex;
use crate::ui::handler::UiHandler;
use crate::ui::widget::Widget;
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
pub struct App {
    core: Arc<Mutex<Core>>,
    idle_handle: IdleHandle,
    window_handle: WindowHandle,
    main_flex: Arc<Mutex<Flex>>,
    views: Arc<Mutex<HashMap<String, Arc<Mutex<View>>>>>,
    config: Config,
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
        main_flex: Arc<Mutex<Flex>>,
        config: Config,
    ) -> App {
        App {
            core: Arc::new(Mutex::new(core)),
            window_handle,
            idle_handle,
            main_flex,
            views: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
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
        let edit_core = self.core.clone();
        let idle_handle = self.idle_handle.clone();
        let main_flex = self.main_flex.clone();
        let views = self.views.clone();
        let config = self.config.clone();
        self.core
            .lock()
            .unwrap()
            .send_request("new_view", &params, move |value| {
                println!("{:?}", value);
                let view_id = value.as_str().unwrap().to_string();
                let view = Arc::new(Mutex::new(View::new(view_id.clone())));
                views.lock().unwrap().insert(view_id.clone(), view.clone());

                let editor_view = Arc::new(Mutex::new(Box::new(EditorView::new(
                    edit_core, view, config,
                ))
                    as Box<Widget + Send + Sync>));
                main_flex.lock().unwrap().add_child(editor_view);
                thread::spawn(move || {
                    core.lock().unwrap().send_notification(
                        "edit",
                        &json!({
                            "view_id": view_id,
                            "method": "scroll",
                            "params": [0, 18],
                        }),
                    );
                    println!("core send notification");
                });
                // idle_handle.add_idle(move |_| {
                //     println!("run idle");
                // });
            });
    }

    pub fn send_notification(&self, method: &str, params: &Value) {
        let core = self.core.lock().unwrap();
        core.send_notification(method, params);
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
                view.lock().unwrap().apply_update(&params["update"]);
                let window_handle = self.window_handle.clone();
                self.idle_handle.add_idle(move |_| {
                    window_handle.invalidate();
                });
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
            // "scroll_to" => self.send_view_cmd(EditViewCommands::ScrollTo(
            //     params["line"].as_u64().unwrap() as usize,
            // )),
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
