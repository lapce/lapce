use crate::{
    buffer::start_buffer_highlights,
    buffer::Buffer,
    buffer::BufferId,
    buffer::BufferUIState,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    command::{LapceCommand, LAPCE_COMMAND},
    editor::EditorSplitState,
    editor::EditorUIState,
    editor::HighlightTextLayout,
    explorer::{FileExplorerState, ICONS_DIR},
    keypress::KeyPressState,
    language::TreeSitter,
    lsp::LspCatalog,
    palette::PaletteState,
    palette::PaletteType,
    plugin::PluginCatalog,
    ssh::SshSession,
};
use anyhow::{anyhow, Result};
use druid::{
    widget::SvgData, Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers,
    Target, WidgetId, WindowId,
};
use git2::Oid;
use git2::Repository;
use lapce_proxy::dispatch::NewBufferResponse;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use std::io::BufReader;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::{
    collections::HashMap, fs::File, io::Read, path::PathBuf, str::FromStr,
    sync::Arc, thread,
};
use toml;
use xi_rpc::Handler;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;
use xi_trace::enable_tracing;

lazy_static! {
    //pub static ref LAPCE_STATE: LapceState = LapceState::new();
    pub static ref LAPCE_APP_STATE: LapceAppState = LapceAppState::new();
}

#[derive(PartialEq)]
enum KeymapMatch {
    Full,
    Prefix,
}

#[derive(Clone, PartialEq)]
pub enum LapceFocus {
    Palette,
    Editor,
    FileExplorer,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum VisualMode {
    Normal,
    Linewise,
    Blockwise,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Mode {
    Insert,
    Visual,
    Normal,
}

#[derive(PartialEq, Eq, Hash, Default, Clone)]
pub struct KeyPress {
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

#[derive(Clone)]
pub struct LapceUIState {
    pub focus: LapceFocus,
    pub mode: Mode,
    pub buffers: Arc<HashMap<BufferId, Arc<BufferUIState>>>,
    pub editors: Arc<HashMap<WidgetId, EditorUIState>>,
    pub highlight_sender: Sender<(WindowId, WidgetId, BufferId, u64)>,
}

impl Data for LapceUIState {
    fn same(&self, other: &Self) -> bool {
        self.focus == other.focus
            && self.buffers.same(&other.buffers)
            && self.editors.same(&other.editors)
    }
}

impl LapceUIState {
    pub fn new(event_sink: ExtEventSink) -> LapceUIState {
        let mut editors = HashMap::new();
        for (_, state) in LAPCE_APP_STATE.states.lock().iter() {
            for (_, state) in state.states.lock().iter() {
                for (_, editor) in state.editor_split.lock().editors.iter() {
                    let editor_ui_state = EditorUIState::new();
                    editors.insert(editor.view_id, editor_ui_state);
                }
            }
        }

        let (sender, receiver) = channel();
        let state = LapceUIState {
            mode: Mode::Normal,
            buffers: Arc::new(HashMap::new()),
            focus: LapceFocus::Editor,
            editors: Arc::new(editors),
            highlight_sender: sender,
        };
        thread::spawn(move || {
            start_buffer_highlights(receiver, event_sink);
        });
        state
    }

    pub fn get_buffer_mut(&mut self, buffer_id: &BufferId) -> &mut BufferUIState {
        Arc::make_mut(Arc::make_mut(&mut self.buffers).get_mut(buffer_id).unwrap())
    }

    pub fn get_buffer(&self, buffer_id: &BufferId) -> &BufferUIState {
        self.buffers.get(buffer_id).unwrap()
    }

    pub fn new_editor(&mut self, editor_id: &WidgetId) {
        let editor_ui_state = EditorUIState::new();
        Arc::make_mut(&mut self.editors).insert(editor_id.clone(), editor_ui_state);
    }

    pub fn get_editor_mut(&mut self, view_id: &WidgetId) -> &mut EditorUIState {
        Arc::make_mut(&mut self.editors).get_mut(view_id).unwrap()
    }

    pub fn get_editor(&self, view_id: &WidgetId) -> &EditorUIState {
        self.editors.get(view_id).unwrap()
    }
}

#[derive(Clone, Debug)]
pub enum LapceWorkspaceType {
    Local,
    RemoteSSH(String, String),
}

#[derive(Clone, Debug)]
pub struct LapceWorkspace {
    pub kind: LapceWorkspaceType,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct Counter(usize);

impl Counter {
    pub fn next(&mut self) -> usize {
        let n = self.0;
        self.0 = n + 1;
        n + 1
    }
}

#[derive(Clone)]
pub struct LapceAppState {
    pub states: Arc<Mutex<HashMap<WindowId, LapceWindowState>>>,
    pub theme: HashMap<String, Color>,
    pub ui_sink: Arc<Mutex<Option<ExtEventSink>>>,
    id_counter: Arc<Mutex<Counter>>,
}

impl LapceAppState {
    pub fn new() -> LapceAppState {
        LapceAppState {
            states: Arc::new(Mutex::new(HashMap::new())),
            theme: Self::get_theme().unwrap_or(HashMap::new()),
            ui_sink: Arc::new(Mutex::new(None)),
            id_counter: Arc::new(Mutex::new(Counter::default())),
        }
    }

    fn get_theme() -> Result<HashMap<String, Color>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/theme.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_theme: HashMap<String, String> = toml::from_slice(&content)?;

        let mut theme = HashMap::new();
        for (name, hex) in toml_theme.iter() {
            if let Ok(color) = hex_to_color(hex) {
                theme.insert(name.to_string(), color);
            }
        }
        Ok(theme)
    }

    pub fn next_id(&self) -> usize {
        self.id_counter.lock().next()
    }

    pub fn set_ui_sink(&self, ui_event_sink: ExtEventSink) {
        *self.ui_sink.lock() = Some(ui_event_sink);
    }

    pub fn get_window_state(&self, window_id: &WindowId) -> LapceWindowState {
        self.states.lock().get(window_id).unwrap().clone()
    }

    pub fn get_active_tab_state(&self, window_id: &WindowId) -> LapceTabState {
        let window_state = self.get_window_state(window_id);
        let active = window_state.active.lock();
        let tab_states = window_state.states.lock();
        tab_states.get(&active).unwrap().clone()
    }

    pub fn get_tab_state(
        &self,
        window_id: &WindowId,
        tab_id: &WidgetId,
    ) -> LapceTabState {
        self.states
            .lock()
            .get(window_id)
            .unwrap()
            .states
            .lock()
            .get(tab_id)
            .unwrap()
            .clone()
    }

    pub fn submit_ui_command(&self, comand: LapceUICommand, widget_id: WidgetId) {
        self.ui_sink.lock().as_ref().unwrap().submit_command(
            LAPCE_UI_COMMAND,
            comand,
            Target::Widget(widget_id),
        );
    }
}

#[derive(Clone)]
pub struct LapceWindowState {
    pub window_id: WindowId,
    pub states: Arc<Mutex<HashMap<WidgetId, LapceTabState>>>,
    pub active: Arc<Mutex<WidgetId>>,
}

impl LapceWindowState {
    pub fn new() -> LapceWindowState {
        let window_id = WindowId::next();
        let state = LapceTabState::new(window_id.clone());
        let active = state.tab_id;
        let mut states = HashMap::new();
        states.insert(active.clone(), state);
        LapceWindowState {
            window_id,
            states: Arc::new(Mutex::new(states)),
            active: Arc::new(Mutex::new(active)),
        }
    }

    pub fn get_active_state(&self) -> LapceTabState {
        self.states.lock().get(&self.active.lock()).unwrap().clone()
    }

    pub fn get_state(&self, tab_id: &WidgetId) -> LapceTabState {
        self.states.lock().get(tab_id).unwrap().clone()
    }
}

#[derive(Clone)]
pub struct LapceTabState {
    pub tab_id: WidgetId,
    pub status_id: WidgetId,
    pub workspace: Arc<Mutex<LapceWorkspace>>,
    pub palette: Arc<Mutex<PaletteState>>,
    pub keypress: Arc<Mutex<KeyPressState>>,
    pub focus: Arc<Mutex<LapceFocus>>,
    pub editor_split: Arc<Mutex<EditorSplitState>>,
    pub container: Option<WidgetId>,
    pub file_explorer: Arc<Mutex<FileExplorerState>>,
    pub plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
    pub ssh_session: Arc<Mutex<Option<SshSession>>>,
    pub proxy: Arc<Mutex<Option<LapceProxy>>>,
}

impl LapceTabState {
    pub fn new(window_id: WindowId) -> LapceTabState {
        let workspace = LapceWorkspace {
            kind: LapceWorkspaceType::Local,
            path: PathBuf::from("/Users/Lulu/lapce"),
        };
        //let workspace = LapceWorkspace {
        //    kind: LapceWorkspaceType::RemoteSSH("10.132.0.2:22".to_string()),
        //    path: PathBuf::from("/home/dz/cosmos"),
        //};
        let tab_id = WidgetId::next();
        let status_id = WidgetId::next();
        let state = LapceTabState {
            tab_id: tab_id.clone(),
            status_id,
            workspace: Arc::new(Mutex::new(workspace)),
            focus: Arc::new(Mutex::new(LapceFocus::Editor)),
            palette: Arc::new(Mutex::new(PaletteState::new(
                window_id.clone(),
                tab_id.clone(),
            ))),
            editor_split: Arc::new(Mutex::new(EditorSplitState::new(
                window_id.clone(),
                tab_id.clone(),
            ))),
            file_explorer: Arc::new(Mutex::new(FileExplorerState::new(
                window_id.clone(),
                tab_id.clone(),
            ))),
            container: None,
            keypress: Arc::new(Mutex::new(KeyPressState::new(
                window_id.clone(),
                tab_id.clone(),
            ))),
            plugins: Arc::new(Mutex::new(PluginCatalog::new(window_id, tab_id))),
            lsp: Arc::new(Mutex::new(LspCatalog::new(
                window_id.clone(),
                tab_id.clone(),
            ))),
            ssh_session: Arc::new(Mutex::new(None)),
            proxy: Arc::new(Mutex::new(None)),
        };
        start_proxy_process(window_id, tab_id);
        let local_state = state.clone();
        thread::spawn(move || {
            local_state.start_plugin();
        });
        state
    }

    pub fn stop(&self) {
        self.plugins.lock().stop();
        self.lsp.lock().stop();
    }

    pub fn start_plugin(&self) {
        // let mut plugins = self.plugins.lock();
        // plugins.reload_from_paths(&[PathBuf::from_str("./lsp").unwrap()]);
        // plugins.start_all();
    }

    pub fn open(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        path: &Path,
    ) {
        if path.is_dir() {
            *self.workspace.lock() = LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: path.to_path_buf(),
            };
            *self.ssh_session.lock() = None;
            self.stop();
            self.start_plugin();
            ctx.request_paint();
        } else {
            self.editor_split.lock().open_file(
                ctx,
                ui_state,
                path.to_str().unwrap(),
            );
        }
    }

    pub fn get_icon(&self, name: &str) -> Option<&SvgData> {
        None
    }

    pub fn get_mode(&self) -> Mode {
        match *self.focus.lock() {
            LapceFocus::Palette => Mode::Insert,
            LapceFocus::Editor => self.editor_split.lock().get_mode(),
            LapceFocus::FileExplorer => Mode::Normal,
        }
    }

    pub fn get_ssh_session(&self, user: &str, host: &str) -> Result<()> {
        let mut ssh_session = self.ssh_session.lock();
        if ssh_session.is_none() {
            let session = SshSession::new(user, host)?;
            *ssh_session = Some(session);
        }
        Ok(())
    }

    pub fn insert(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        content: &str,
        env: &Env,
    ) {
        match *self.focus.lock() {
            LapceFocus::Palette => {
                self.palette.lock().insert(ctx, ui_state, content, env);
            }
            LapceFocus::Editor => {
                self.editor_split.lock().insert(ctx, ui_state, content, env);
            }
            _ => (),
        }
        // ctx.request_layout();
    }

    pub fn run_command(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        count: Option<usize>,
        command: &str,
        env: &Env,
    ) -> Result<()> {
        let cmd = LapceCommand::from_str(command)?;
        match cmd {
            LapceCommand::Palette => {
                *self.focus.lock() = LapceFocus::Palette;
                self.palette.lock().run(None);
                ctx.request_layout();
            }
            LapceCommand::PaletteLine => {
                *self.focus.lock() = LapceFocus::Palette;
                self.palette.lock().run(Some(PaletteType::Line));
                ctx.request_layout();
            }
            LapceCommand::PaletteSymbol => {
                *self.focus.lock() = LapceFocus::Palette;
                self.palette.lock().run(Some(PaletteType::DocumentSymbol));
                ctx.request_layout();
            }
            LapceCommand::PaletteCancel => {
                *self.focus.lock() = LapceFocus::Editor;
                self.palette.lock().cancel(ctx, ui_state);
                ctx.request_layout();
            }
            LapceCommand::FileExplorer => {
                *self.focus.lock() = LapceFocus::FileExplorer;
            }
            LapceCommand::FileExplorerCancel => {
                *self.focus.lock() = LapceFocus::Editor;
            }
            _ => {
                let mut focus = self.focus.lock();
                match *focus {
                    LapceFocus::FileExplorer => {
                        *focus = self
                            .file_explorer
                            .lock()
                            .run_command(ctx, ui_state, count, cmd);
                    }
                    LapceFocus::Editor => {
                        self.editor_split
                            .lock()
                            .run_command(ctx, ui_state, count, cmd, env);
                    }
                    LapceFocus::Palette => {
                        let mut palette = self.palette.lock();
                        match cmd {
                            LapceCommand::ListSelect => {
                                palette.select(ctx, ui_state, env);
                                *focus = LapceFocus::Editor;
                                ctx.request_layout();
                            }
                            LapceCommand::ListNext => {
                                palette.change_index(ctx, ui_state, 1, env);
                            }
                            LapceCommand::ListPrevious => {
                                palette.change_index(ctx, ui_state, -1, env);
                            }
                            LapceCommand::Left => {
                                palette.move_cursor(ctx, -1);
                            }
                            LapceCommand::Right => {
                                palette.move_cursor(ctx, 1);
                            }
                            LapceCommand::DeleteBackward => {
                                palette.delete_backward(ctx, ui_state, env);
                            }
                            LapceCommand::DeleteToBeginningOfLine => {
                                palette
                                    .delete_to_beginning_of_line(ctx, ui_state, env);
                            }
                            _ => (),
                        };
                    }
                };
            }
        };
        ui_state.focus = self.focus.lock().clone();
        // ctx.request_layout();
        Ok(())
    }

    pub fn request_paint(&self) {
        LAPCE_APP_STATE.submit_ui_command(LapceUICommand::RequestPaint, self.tab_id);
    }

    pub fn check_condition(&self, condition: &str) -> bool {
        let or_indics: Vec<_> = condition.match_indices("||").collect();
        let and_indics: Vec<_> = condition.match_indices("&&").collect();
        if and_indics.is_empty() {
            if or_indics.is_empty() {
                return self.check_one_condition(condition);
            } else {
                return self.check_one_condition(&condition[..or_indics[0].0])
                    || self.check_condition(&condition[or_indics[0].0 + 2..]);
            }
        } else {
            if or_indics.is_empty() {
                return self.check_one_condition(&condition[..and_indics[0].0])
                    && self.check_condition(&condition[and_indics[0].0 + 2..]);
            } else {
                if or_indics[0].0 < and_indics[0].0 {
                    return self.check_one_condition(&condition[..or_indics[0].0])
                        || self.check_condition(&condition[or_indics[0].0 + 2..]);
                } else {
                    return self.check_one_condition(&condition[..and_indics[0].0])
                        && self.check_condition(&condition[and_indics[0].0 + 2..]);
                }
            }
        }
    }

    fn check_one_condition(&self, condition: &str) -> bool {
        let focus = self.focus.lock();
        let editor_split = self.editor_split.lock();
        match condition.trim() {
            "file_explorer_focus" => *focus == LapceFocus::FileExplorer,
            "palette_focus" => *focus == LapceFocus::Palette,
            "list_focus" => {
                *focus == LapceFocus::Palette
                    || *focus == LapceFocus::FileExplorer
                    || (*focus == LapceFocus::Editor
                        && (editor_split.completion.len() > 0
                            || editor_split.code_actions_show))
            }
            "editor_operator" => {
                *focus == LapceFocus::Editor && editor_split.has_operator()
            }
            _ => false,
        }
    }

    pub fn container_id(&self) -> WidgetId {
        self.container.unwrap().clone()
    }

    pub fn set_container(&mut self, container: WidgetId) {
        self.container = Some(container);
    }
}

pub struct LapceProxy {
    peer: RpcPeer,
    process: Child,
}

impl LapceProxy {
    pub fn new_buffer(&self, buffer_id: BufferId, path: PathBuf) -> Result<String> {
        enable_tracing();
        let result = self
            .peer
            .send_rpc_request(
                "new_buffer",
                &json!({ "buffer_id": buffer_id, "path": path }),
            )
            .map_err(|e| anyhow!("{:?}", e))?;

        let resp: NewBufferResponse = serde_json::from_value(result)?;
        return Ok(resp.content);
    }
}

fn start_proxy_process(window_id: WindowId, tab_id: WidgetId) {
    thread::spawn(move || {
        let child = Command::new("/Users/Lulu/lapce/target/debug/lapce-proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
        if child.is_err() {
            println!("can't start proxy {:?}", child);
            return;
        }
        let mut child = child.unwrap();
        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let mut looper = RpcLoop::new(child_stdin);
        let peer: RpcPeer = Box::new(looper.get_raw_peer());
        let proxy = LapceProxy {
            peer,
            process: child,
        };
        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            *state.proxy.lock() = Some(proxy);
        }

        let mut handler = ProxyHandler { window_id, tab_id };
        if let Err(e) =
            looper.mainloop(|| BufReader::new(child_stdout), &mut handler)
        {
            println!("proxy main loop failed {:?}", e);
        }
        println!("proxy main loop exit");
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

pub struct ProxyHandler {
    window_id: WindowId,
    tab_id: WidgetId,
}

impl Handler for ProxyHandler {
    type Notification = Notification;
    type Request = Request;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
        match rpc {
            Notification::StartLspServer {
                exec_path,
                language_id,
                options,
            } => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .lsp
                    .lock()
                    .start_server(&exec_path, &language_id, options);
            }
        }
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<serde_json::Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches("#");
    let (r, g, b, a) = match hex.len() {
        3 => (
            format!("{}{}", &hex[0..0], &hex[0..0]),
            format!("{}{}", &hex[1..1], &hex[1..1]),
            format!("{}{}", &hex[2..2], &hex[2..2]),
            "ff".to_string(),
        ),
        6 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            "ff".to_string(),
        ),
        8 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            hex[6..8].to_string(),
        ),
        _ => return Err(anyhow!("invalid hex color")),
    };
    Ok(Color::rgba8(
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?,
        u8::from_str_radix(&a, 16)?,
    ))
}

#[cfg(test)]
mod tests {
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_check_condition() {
        // let rope = Rope::from_str("abc\nabc\n").unwrap();
        // assert_eq!(rope.len(), 9);
        // assert_eq!(rope.offset_of_line(1), 1);
        // assert_eq!(rope.line_of_offset(rope.len()), 9);
    }
}
