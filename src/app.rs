use crate::editor::EditViewCommands;
use crate::editor::Editor;
use crate::input::{Cmd, Command, Input, InputState, KeyInput};
use crate::line_cache::Style;
use crate::palette::Palette;
use crate::popup::Popup;
use crate::rpc::{Core, Handler};
use crane_ui::{Column, Flex, WidgetTrait};
use druid::shell::platform::IdleHandle;
use druid::shell::platform::WindowHandle;
use lsp_types::{CompletionResponse, Position};
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

pub trait CommandRunner {
    fn run(&self, cmd: Cmd, key_input: KeyInput);
}

#[derive(Clone)]
pub struct AppState {
    pub active_editor: String,
    pending_keys: Vec<KeyInput>,
    pub palette: Option<Palette>,
    pub popup: Option<Popup>,
}

impl AppState {
    fn new() -> AppState {
        AppState {
            active_editor: "".to_string(),
            pending_keys: Vec::new(),
            palette: None,
            popup: None,
        }
    }
}

#[derive(Clone)]
pub struct App {
    pub core: Core,
    pub state: Arc<Mutex<AppState>>,
    pub idle_handle: IdleHandle,
    pub window_handle: WindowHandle,
    pub main_flex: Flex,
    pub views: Arc<Mutex<HashMap<String, View>>>,
    pub path_views: Arc<Mutex<HashMap<String, View>>>,
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
        main_flex: Flex,
        config: Config,
    ) -> App {
        App {
            state: Arc::new(Mutex::new(AppState::new())),
            core,
            window_handle,
            idle_handle,
            main_flex,
            views: Arc::new(Mutex::new(HashMap::new())),
            path_views: Arc::new(Mutex::new(HashMap::new())),
            editors: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.state.lock().unwrap().palette = Some(palette)
    }

    pub fn set_popup(&mut self, popup: Popup) {
        self.state.lock().unwrap().popup = Some(popup)
    }

    pub fn new_editor(&self) -> Editor {
        let editor = Editor::new(self.clone());
        self.editors
            .lock()
            .unwrap()
            .insert(editor.id().clone(), editor.clone());
        self.main_flex.add_child(Box::new(editor.clone()));
        editor
    }

    pub fn set_active_editor(&self, editor: &Editor) {
        let id = editor.id().clone();
        self.state.lock().unwrap().active_editor = id;
    }

    pub fn get_active_editor(&self) -> Editor {
        let id = self.state.lock().unwrap().active_editor.clone();
        self.editors.lock().unwrap().get(&id).unwrap().clone()
    }

    pub fn send_notification(&self, method: &str, params: &Value) {
        self.core.send_notification(method, params);
    }

    pub fn handle_key_down(&self, key_input: KeyInput) {
        if key_input.text == "" {
            return;
        }
        let mut pending_keys = self.state.lock().unwrap().pending_keys.clone();
        pending_keys.push(key_input.clone());

        let (input_state, runner) = if !self
            .state
            .lock()
            .unwrap()
            .palette
            .clone()
            .unwrap()
            .is_hidden()
        {
            let palette = self.state.lock().unwrap().palette.clone().unwrap();
            (InputState::Palette, Box::new(palette) as Box<CommandRunner>)
        } else {
            let active_editor = self.get_active_editor();

            (
                active_editor.get_state(),
                Box::new(active_editor) as Box<CommandRunner>,
            )
        };

        let cmd = {
            let keymaps = self.config.keymaps.lock().unwrap();
            keymaps.get(input_state, pending_keys.clone())
        };
        if cmd.more_input {
            self.state.lock().unwrap().pending_keys = pending_keys;
            return;
        }

        if cmd.clone().cmd.unwrap() == Command::Unknown {
            for key in pending_keys {
                runner.run(
                    Cmd {
                        cmd: Some(Command::Unknown),
                        more_input: false,
                    },
                    key,
                );
            }
            self.state.lock().unwrap().pending_keys = Vec::new();
            return;
        }
        runner.run(cmd, key_input);
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
                let col = params["col"].as_u64().unwrap() as usize;
                let line = params["line"].as_u64().unwrap() as usize;
                view.scroll_to(col, line);
            }
            "show_completion" => {
                println!("show completion");
                let completion: CompletionResponse =
                    serde_json::from_value(params["result"].clone()).unwrap();
                let popup = self.state.lock().unwrap().popup.clone().unwrap().clone();
                let editor = self.get_active_editor();
                let (col, line, filter) = editor.get_completion_pos();
                popup.set_location(col, line);
                editor.move_popup();
                popup.set_completion(completion);
                popup.filter_items(filter);
                popup.show();
                popup.invalidate();
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
