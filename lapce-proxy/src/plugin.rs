use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use hotwatch::Hotwatch;
use lapce_rpc::counter::Counter;
use lapce_rpc::plugin::{PluginDescription, PluginId, PluginInfo, PluginResponse};
use lapce_rpc::proxy::{
    CoreProxyNotification, CoreProxyRequest, CoreProxyResponse,
    PluginProxyNotification, PluginProxyRequest, PluginProxyResponse,
    ProxyRpcHandler, ProxyRpcMessage,
};
use lapce_rpc::{NewRpcHandler, RequestId, RpcError, RpcMessage};
use lsp_types::notification::{DidOpenTextDocument, Notification};
use lsp_types::request::{Completion, Request};
use lsp_types::{
    CompletionParams, CompletionResponse, DidOpenTextDocumentParams,
    PartialResultParams, Position, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, WorkDoneProgressParams,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use toml_edit::easy as toml;
use wasi_common::pipe::ReadPipe;
use wasi_common::WasiCtx;
use wasmer::ChainableNamedResolver;
use wasmer::ImportObject;
use wasmer::Store;
use wasmer::WasmerEnv;
use wasmer_wasi::Pipe;
use wasmer_wasi::WasiEnv;
use wasmer_wasi::WasiState;
use wasmtime_wasi::WasiCtxBuilder;

use crate::dispatch::Dispatcher;
use crate::lsp::{LspRpcHandler, NewLspClient};
use crate::{dispatch::Dispatcher, APPLICATION_NAME};

pub type PluginName = String;

pub type PluginRpcMessage =
    RpcMessage<PluginRequest, NewPluginNotification, PluginProxyResponse>;

pub enum PluginRequest {
    PluginServerRequest { method: &'static str, params: Value },
}

pub enum NewPluginNotification {
    PluginLoaded(NewPlugin),
    LspLoaded(LspRpcHandler),
    DocumentDidOpen(TextDocumentItem),
    PluginServerNotification {
        method: &'static str,
        params: Value,
    },
    StartLspServer {
        workspace: Option<PathBuf>,
        plugin_id: PluginId,
        exec_path: String,
        language_id: String,
        options: Option<Value>,
        system_lsp: Option<bool>,
    },
}

#[derive(WasmerEnv, Clone)]
pub struct NewPluginEnv {
    id: PluginId,
    plugin_rpc: PluginRpcHandler,
    wasi_env: WasiEnv,
    desc: PluginDescription,
}

#[derive(Clone)]
pub struct NewPlugin {
    id: PluginId,
    instance: wasmer::Instance,
    env: NewPluginEnv,
}

#[derive(WasmerEnv, Clone)]
pub(crate) struct PluginEnv {
    wasi_env: WasiEnv,
    desc: PluginDescription,
    dispatcher: Dispatcher,
}

#[derive(Clone)]
pub(crate) struct Plugin {
    instance: wasmer::Instance,
    env: PluginEnv,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct PluginConfig {
    disabled: Vec<String>,
}

pub struct NewPluginCatalog {
    plugin_sender: Sender<PluginRpcMessage>,
}

#[derive(Clone)]
pub struct PluginRpcHandler {
    plugin_tx: Sender<PluginRpcMessage>,
    plugin_rx: Receiver<PluginRpcMessage>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, Sender<Result<Value, RpcError>>>>>,
}

impl PluginRpcHandler {
    pub fn new() -> Self {
        let (plugin_tx, plugin_rx) = crossbeam_channel::unbounded();
        Self {
            plugin_tx,
            plugin_rx,
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn handle_response(&self, id: RequestId, result: Result<Value, RpcError>) {
        if let Some(chan) = { self.pending.lock().remove(&id) } {
            chan.send(result);
        }
    }

    pub fn mainloop(&self, plugin: &mut NewPluginCatalog) {
        for msg in &self.plugin_rx {
            match msg {
                RpcMessage::Request(id, request) => {
                    plugin.handle_request(request);
                }
                RpcMessage::Response(id, response) => {
                    todo!()
                }
                RpcMessage::Notification(notification) => {
                    plugin.handle_notification(notification);
                }
                RpcMessage::Error(id, err) => {
                    self.handle_response(id, Err(err));
                }
            }
        }
    }

    fn send_notification(&self, notification: NewPluginNotification) {
        let _ = self.plugin_tx.send(RpcMessage::Notification(notification));
    }

    fn send_plugin_server_notification<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
    ) {
        let params = serde_json::to_value(params).unwrap();
        let notification =
            NewPluginNotification::PluginServerNotification { method, params };
        let _ = self.plugin_tx.send(RpcMessage::Notification(notification));
    }

    fn send_request<P: Serialize, D: DeserializeOwned>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<D, RpcError> {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = crossbeam_channel::bounded(1);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, tx);
        }

        let params = serde_json::to_value(params).unwrap();
        let request = PluginRequest::PluginServerRequest { method, params };
        let _ = self.plugin_tx.send(RpcMessage::Request(id, request));

        rx.recv()
            .unwrap_or_else(|_| {
                Err(RpcError {
                    code: 0,
                    message: "io error".to_string(),
                })
            })
            .and_then(|v| {
                serde_json::from_value(v).map_err(|_| RpcError {
                    code: 0,
                    message: "deserilize error".to_string(),
                })
            })
    }

    pub fn completion(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<CompletionResponse, RpcError> {
        let uri = Url::from_file_path(path).unwrap();
        let method = Completion::METHOD;
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        self.send_request(method, params)
    }

    pub fn document_did_open(
        &self,
        path: &Path,
        language_id: String,
        version: i32,
        text: String,
    ) {
        let method = DidOpenTextDocument::METHOD;
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(
                Url::from_file_path(path).unwrap(),
                language_id,
                version,
                text,
            ),
        };
        self.send_plugin_server_notification(method, params);
    }

    fn plugin_loaded(&self, plugin: NewPlugin) {
        self.send_notification(NewPluginNotification::PluginLoaded(plugin));
    }
}

pub struct NewPluginCatalog {
    plugin_rpc: PluginRpcHandler,
    plugins: HashMap<PluginId, NewPlugin>,
    lsps: Vec<LspRpcHandler>,
}

impl NewPluginCatalog {
    pub fn new(plugin_rpc: PluginRpcHandler) -> Self {
        let plugin = Self {
            plugin_rpc: plugin_rpc.clone(),
            plugins: HashMap::new(),
            lsps: Vec::new(),
        };

        thread::spawn(move || {
            Self::load(plugin_rpc);
        });

        plugin
    }

    pub fn load(plugin_rpc: PluginRpcHandler) {
        eprintln!("start to load plugins");
        let all_plugins = find_all_plugins();
        for plugin_path in &all_plugins {
            match load_plugin(plugin_path) {
                Err(_e) => (),
                Ok(plugin_desc) => {
                    if let Err(e) =
                        Self::start_plugin(plugin_rpc.clone(), plugin_desc)
                    {
                        eprintln!("start plugin error {}", e);
                    }
                }
            }
        }
    }

    fn start_plugin(
        plugin_rpc: PluginRpcHandler,
        plugin_desc: PluginDescription,
    ) -> Result<()> {
        eprintln!("start a certain plugin");
        let store = Store::default();
        let module = wasmer::Module::from_file(
            &store,
            plugin_desc
                .wasm
                .as_ref()
                .ok_or_else(|| anyhow!("no wasm in plugin"))?,
        )?;

        let output = Pipe::new();
        let input = Pipe::new();
        let env = plugin_desc.get_plugin_env()?;
        let mut wasi_env = WasiState::new("Lapce")
            .map_dir("/", plugin_desc.dir.clone().unwrap())?
            .stdin(Box::new(input))
            .stdout(Box::new(output))
            .envs(env)
            .finalize()?;
        let wasi = wasi_env.import_object(&module)?;

        let id = PluginId::next();
        let plugin_env = NewPluginEnv {
            id,
            wasi_env,
            plugin_rpc,
            desc: plugin_desc.clone(),
        };
        let lapce = lapce_exports(&store, &plugin_env);
        let instance = wasmer::Instance::new(&module, &lapce.chain_back(wasi))?;
        let plugin = NewPlugin {
            id,
            instance,
            env: plugin_env.clone(),
        };

        plugin_rpc.plugin_loaded(plugin.clone());

        thread::spawn(move || {
            let initialize =
                plugin.instance.exports.get_function("initialize").unwrap();
            wasi_write_object(
                &plugin.env.wasi_env,
                &PluginInfo {
                    os: std::env::consts::OS.to_string(),
                    arch: std::env::consts::ARCH.to_string(),
                    configuration: plugin_desc.configuration,
                },
            );
            initialize.call(&[]).unwrap();
        });

        // let (plugin_sender, plugin_receiver) =
        //     crossbeam_channel::unbounded::<PluginRpcMessage>();
        // let (proxy_sender, proxy_receiver) = crossbeam_channel::unbounded();
        // let mut rpc_handler = ProxyRpcHandler::new(plugin_sender);
        // rpc_handler.mainloop(proxy_receiver, &mut plugin);

        // for msg in plugin_receiver {
        //     wasi_write_object(&plugin_env.wasi_env, &msg.to_value().unwrap());
        // }

        Ok(())
    }

    fn handle_notification(&mut self, rpc: NewPluginNotification) {
        match rpc {
            NewPluginNotification::PluginLoaded(plugin) => {
                eprintln!("plugin loaded");
                self.plugins.insert(plugin.id, plugin);
            }
            NewPluginNotification::LspLoaded(lsp) => {
                self.lsps.push(lsp);
            }
            NewPluginNotification::StartLspServer {
                workspace,
                plugin_id,
                exec_path,
                language_id,
                options,
                system_lsp,
            } => {
                let exec_path = if system_lsp.unwrap_or(false) {
                    // System LSP should be handled by PATH during
                    // process creation, so we forbid anything that
                    // is not just an executable name
                    match PathBuf::from(&exec_path).file_name() {
                        Some(v) => v.to_str().unwrap().to_string(),
                        None => return,
                    }
                } else {
                    let plugin = self.plugins.get(&plugin_id).unwrap();
                    plugin
                        .env
                        .desc
                        .dir
                        .as_ref()
                        .unwrap()
                        .join(&exec_path)
                        .to_str()
                        .unwrap()
                        .to_string()
                };
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    NewLspClient::start(
                        plugin_rpc,
                        workspace,
                        exec_path,
                        Vec::new(),
                    );
                });
            }
            NewPluginNotification::DocumentDidOpen(document) => {
                for lsp in self.lsps.iter() {
                    lsp.send_notification(
                        DidOpenTextDocument::METHOD,
                        DidOpenTextDocumentParams {
                            text_document: document.clone(),
                        },
                    );
                }
            }
            NewPluginNotification::PluginServerNotification { method, params } => {
                for lsp in self.lsps.iter() {
                    lsp.send_notification(method, params.clone());
                }
            }
        }
    }

    fn handle_request(&mut self, rpc: PluginRequest) {
        todo!()
    }
}

impl PluginServerRpc {
    pub fn new(plugin_sender: Sender<PluginRpcMessage>) -> Self {
        Self { plugin_sender }
    }

    pub fn get_completion(&self, path: &Path, position: Position) {
        let uri = Url::from_file_path(path).unwrap();
        let method = Completion::METHOD;
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
    }

    pub fn document_did_open(
        &self,
        path: &Path,
        language_id: String,
        version: i32,
        text: String,
    ) {
        let method = DidOpenTextDocument::METHOD;
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(
                Url::from_file_path(path).unwrap(),
                language_id,
                version,
                text,
            ),
        };
        self.send_notification(method, params);
    }

    pub fn send_notification<N: Serialize>(&self, method: &'static str, params: N) {
        let params = serde_json::to_value(params).unwrap();
        let notification =
            NewPluginNotification::PluginServerNotification { method, params };
        self.plugin_sender
            .send(RpcMessage::Notification(notification));
    }
}

pub struct PluginCatalog {
    id_counter: Counter,
    pub items: HashMap<PluginName, PluginDescription>,
    plugins: HashMap<PluginName, Plugin>,
    pub disabled: HashMap<PluginName, PluginDescription>,
    store: Store,
    senders: HashMap<PluginName, Sender<PluginTransmissionMessage>>,
}

enum PluginTransmissionMessage {
    Initialize,
    Stop,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            id_counter: Counter::new(),
            items: HashMap::new(),
            plugins: HashMap::new(),
            disabled: HashMap::new(),
            store: Store::default(),
            senders: HashMap::new(),
        }
    }

    pub fn stop(&mut self) {
        self.items.clear();
        self.plugins.clear();
    }

    pub fn reload(&mut self) {
        self.items.clear();
        self.plugins.clear();
        self.disabled.clear();
        let _ = self.load();
    }

    pub fn load(&mut self) -> Result<()> {
        let all_plugins = find_all_plugins();
        for plugin_path in &all_plugins {
            match load_plugin(plugin_path) {
                Err(_e) => (),
                Ok(plugin) => {
                    self.items.insert(plugin.name.clone(), plugin.clone());
                }
            }
        }
        let path = config_directory()
            .expect("couldn't obtain config dir")
            .join("plugins.toml");
        let mut file = fs::File::open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let plugin_config: PluginConfig = toml::from_str(&content)?;
        let mut disabled = HashMap::new();
        for plugin_name in plugin_config.disabled.iter() {
            if let Some(plugin) = self.items.get(plugin_name) {
                disabled.insert(plugin_name.clone(), plugin.clone());
            }
        }
        self.disabled = disabled;
        Ok(())
    }

    pub fn install_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin: PluginDescription,
    ) -> Result<()> {
        let path = plugins_directory()
            .expect("Couldn't obtain plugins dir")
            .join(&plugin.name);
        let _ = fs::remove_dir_all(&path);

        fs::create_dir_all(&path)?;

        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path.join("plugin.toml"))?;
            file.write_all(&toml::to_vec(&plugin)?)?;
        }

        let mut plugin = plugin;
        if let Some(wasm) = plugin.wasm.clone() {
            {
                let url = format!(
                    "https://raw.githubusercontent.com/{}/master/{}",
                    plugin.repository, wasm
                );
                let mut resp = reqwest::blocking::get(url)?;
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(path.join(&wasm))?;
                std::io::copy(&mut resp, &mut file)?;
            }

            plugin.dir = Some(path.clone());
            plugin.wasm = Some(
                path.join(&wasm)
                    .to_str()
                    .ok_or_else(|| anyhow!("path can't to string"))?
                    .to_string(),
            );

            if let Ok((p, tx)) = self.start_plugin(dispatcher, plugin.clone()) {
                self.plugins.insert(plugin.name.clone(), p);
                self.senders.insert(plugin.name.clone(), tx);
            }
        }
        if let Some(themes) = plugin.themes.as_ref() {
            for theme in themes {
                {
                    let url = format!(
                        "https://raw.githubusercontent.com/{}/HEAD/{}",
                        plugin.repository, theme
                    );
                    let mut resp = reqwest::blocking::get(url)?;
                    let mut file = fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(path.join(theme))?;
                    std::io::copy(&mut resp, &mut file)?;
                }
            }
        }
        self.items.insert(plugin.name.clone(), plugin);
        Ok(())
    }

    pub fn remove_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin: PluginDescription,
    ) -> Result<()> {
        self.disable_plugin(dispatcher, plugin.clone())?;
        let path = plugins_directory()
            .expect("Couldn't obtain plugins dir")
            .join(&plugin.name);
        fs::remove_dir_all(&path)?;

        let _ = self.items.remove(&plugin.name);
        let _ = self.plugins.remove(&plugin.name);
        let _ = self.disabled.remove(&plugin.name);
        Ok(())
    }

    pub fn start_all(&mut self, dispatcher: Dispatcher) {
        for (_, plugin) in self.items.clone().iter() {
            if !self.disabled.contains_key(&plugin.name) {
                if let Ok((p, _tx)) =
                    self.start_plugin(dispatcher.clone(), plugin.clone())
                {
                    self.plugins.insert(plugin.name.clone(), p);
                }
            }
        }
    }

    fn start_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin_desc: PluginDescription,
    ) -> Result<(Plugin, Sender<PluginTransmissionMessage>)> {
        let module = wasmer::Module::from_file(
            &self.store,
            plugin_desc
                .wasm
                .as_ref()
                .ok_or_else(|| anyhow!("no wasm in plugin"))?,
        )?;
        let output = Pipe::new();
        let input = Pipe::new();
        let env = plugin_desc.get_plugin_env()?;
        let mut wasi_env = WasiState::new("Lapce")
            .map_dir("/", plugin_desc.dir.clone().unwrap())?
            .stdin(Box::new(input))
            .stdout(Box::new(output))
            .envs(env)
            .finalize()?;
        let wasi = wasi_env.import_object(&module)?;

        // let plugin_env = PluginEnv {
        //     wasi_env,
        //     desc: plugin_desc.clone(),
        //     dispatcher,
        // };
        // let lapce = lapce_exports(&self.store, &plugin_env);
        // let instance = wasmer::Instance::new(&module, &lapce.chain_back(wasi))?;
        // let plugin = Plugin {
        //     instance,
        //     env: plugin_env,
        // };

        let local_plugin = plugin.clone();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || loop {
            match rx.recv() {
                Ok(PluginTransmissionMessage::Initialize) => {
                    let initialize = local_plugin
                        .instance
                        .exports
                        .get_function("initialize")
                        .unwrap();
                    wasi_write_object(
                        &local_plugin.env.wasi_env,
                        &PluginInfo {
                            os: std::env::consts::OS.to_string(),
                            arch: std::env::consts::ARCH.to_string(),
                            configuration: plugin_desc.clone().configuration,
                        },
                    );
                    initialize.call(&[]).unwrap();
                }
                Ok(PluginTransmissionMessage::Stop) => {
                    let stop = local_plugin.instance.exports.get_function("stop");
                    if let Ok(stop_func) = stop {
                        stop_func.call(&[]).unwrap();
                    } else if let Some(Value::Object(conf)) =
                        &plugin_desc.configuration
                    {
                        if let Some(Value::String(lang)) = conf.get("language_id") {
                            local_plugin
                                .env
                                .dispatcher
                                .lsp
                                .lock()
                                .stop_language_lsp(lang);
                        }
                    }
                    break;
                }
                // There was an error when receiving, which means that the other end was closed.
                // So we simply shutdown this thread by breaking out of the loop
                Err(_) => break,
            }
        });
        tx.send(PluginTransmissionMessage::Initialize)?;
        Ok((plugin, tx))
    }

    pub fn disable_plugin(
        &mut self,
        _dispatcher: Dispatcher,
        plugin_desc: PluginDescription,
    ) -> Result<()> {
        let plugin_tx = self.senders.get(&plugin_desc.name);
        if let Some(tx) = plugin_tx {
            let local_tx = tx.clone();
            thread::spawn(move || {
                let _ = local_tx.send(PluginTransmissionMessage::Stop);
            });
        }
        self.senders.remove(&plugin_desc.name);
        let plugin = plugin_desc.clone();
        self.disabled.insert(plugin_desc.name.clone(), plugin);
        let disabled_plugin_list =
            self.disabled.clone().into_keys().collect::<Vec<String>>();
        let plugin_config = PluginConfig {
            disabled: disabled_plugin_list,
        };
        let path = config_directory().expect("couldn't obtain config dir");
        fs::create_dir_all(&path)?;
        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path.join("plugins.toml"))?;
            file.write_all(&toml::to_vec(&plugin_config)?)?;
        }

        Ok(())
    }

    pub fn enable_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin_desc: PluginDescription,
    ) -> Result<()> {
        let mut plugin = plugin_desc.clone();
        let path = plugins_directory()
            .expect("Couldn't obtain plugins dir")
            .join(&plugin.name);
        plugin.dir = Some(path.clone());
        if let Some(wasm) = plugin.wasm {
            plugin.wasm = Some(
                path.join(&wasm)
                    .to_str()
                    .ok_or_else(|| anyhow!("path can't to string"))?
                    .to_string(),
            );
            self.start_plugin(dispatcher, plugin.clone())?;
            self.disabled.remove(&plugin_desc.name);
            let config_path =
                config_directory().expect("couldn't obtain config dir");
            let disabled_plugin_list =
                self.disabled.clone().into_keys().collect::<Vec<String>>();
            let plugin_config = PluginConfig {
                disabled: disabled_plugin_list,
            };
            {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(config_path.join("plugins.toml"))?;
                file.write_all(&toml::to_vec(&plugin_config)?)?;
            }
            Ok(())
        } else {
            Err(anyhow!("no wasm in plugin"))
        }
    }

    pub fn next_plugin_id(&mut self) -> PluginId {
        PluginId(self.id_counter.next())
    }
}

impl Default for PluginCatalog {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn lapce_exports(
    store: &Store,
    plugin_env: &NewPluginEnv,
) -> ImportObject {
    macro_rules! lapce_export {
        ($($host_function:ident),+ $(,)?) => {
            wasmer::imports! {
                "lapce" => {
                    $(stringify!($host_function) =>
                        wasmer::Function::new_native_with_env(store, plugin_env.clone(), $host_function),)+
                }
            }
        }
    }

    lapce_export! {
        host_handle_notification,
    }
}

// pub(crate) fn lapce_exports(store: &Store, plugin_env: &PluginEnv) -> ImportObject {
//     macro_rules! lapce_export {
//         ($($host_function:ident),+ $(,)?) => {
//             wasmer::imports! {
//                 "lapce" => {
//                     $(stringify!($host_function) =>
//                         wasmer::Function::new_native_with_env(store, plugin_env.clone(), $host_function),)+
//                 }
//             }
//         }
//     }

//     lapce_export! {
//         host_handle_notification,
//     }
// }

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
        system_lsp: Option<bool>,
    },
    DownloadFile {
        url: String,
        path: PathBuf,
    },
    LockFile {
        path: PathBuf,
    },
    MakeFileExecutable {
        path: PathBuf,
    },
}

fn host_handle_notification(plugin_env: &NewPluginEnv) {
    let notification: Result<PluginProxyNotification> =
        wasi_read_object(&plugin_env.wasi_env);
    if let Ok(notification) = notification {
        // let _ = plugin_env
        //     .plugin_rpc
        //     .send_notification(ProxyRpcMessage::Plugin(
        //         plugin_env.id,
        //         RpcMessage::Notification(notification),
        //     ));
    }
}

// fn host_handle_notification(plugin_env: &PluginEnv) {
//     let notification: Result<PluginNotification> =
//         wasi_read_object(&plugin_env.wasi_env);
//     if let Ok(notification) = notification {
//         match notification {
//             PluginNotification::StartLspServer {
//                 exec_path,
//                 language_id,
//                 options,
//                 system_lsp,
//             } => {
//                 let exec_path = if system_lsp.unwrap_or(false) {
//                     // System LSP should be handled by PATH during
//                     // process creation, so we forbid anything that
//                     // is not just an executable name
//                     match PathBuf::from(&exec_path).file_name() {
//                         Some(v) => v.to_str().unwrap().to_string(),
//                         None => return,
//                     }
//                 } else {
//                     plugin_env
//                         .desc
//                         .dir
//                         .clone()
//                         .unwrap()
//                         .join(&exec_path)
//                         .to_str()
//                         .unwrap()
//                         .to_string()
//                 };
//                 plugin_env.dispatcher.lsp.lock().start_server(
//                     &exec_path,
//                     &language_id,
//                     options,
//                 );
//             }
//             PluginNotification::DownloadFile { url, path } => {
//                 let mut resp = reqwest::blocking::get(url).expect("request failed");
//                 let mut out = fs::File::create(
//                     plugin_env.desc.dir.clone().unwrap().join(path),
//                 )
//                 .expect("failed to create file");
//                 std::io::copy(&mut resp, &mut out).expect("failed to copy content");
//             }
//             PluginNotification::LockFile { path } => {
//                 let path = plugin_env.desc.dir.clone().unwrap().join(path);
//                 let mut n = 0;
//                 loop {
//                     if let Ok(_file) = fs::OpenOptions::new()
//                         .write(true)
//                         .create_new(true)
//                         .open(&path)
//                     {
//                         return;
//                     }
//                     if n > 10 {
//                         return;
//                     }
//                     n += 1;
//                     let mut hotwatch =
//                         Hotwatch::new().expect("hotwatch failed to initialize!");
//                     let (tx, rx) = crossbeam_channel::bounded(1);
//                     let _ = hotwatch.watch(&path, move |_event| {
//                         #[allow(deprecated)]
//                         let _ = tx.send(0);
//                     });
//                     let _ = rx.recv_timeout(Duration::from_secs(10));
//                 }
//             }
//             PluginNotification::MakeFileExecutable { path } => {
//                 let _ = Command::new("chmod")
//                     .arg("+x")
//                     .arg(&plugin_env.desc.dir.clone().unwrap().join(path))
//                     .output();
//             }
//         }
//     }
// }

pub fn wasi_read_string(wasi_env: &WasiEnv) -> Result<String> {
    let mut state = wasi_env.state();
    let wasi_file = state
        .fs
        .stdout_mut()?
        .as_mut()
        .ok_or_else(|| anyhow!("can't get stdout"))?;
    let mut buf = String::new();
    wasi_file.read_to_string(&mut buf)?;
    Ok(buf)
}

pub fn wasi_read_object<T: DeserializeOwned>(wasi_env: &WasiEnv) -> Result<T> {
    let json = wasi_read_string(wasi_env)?;
    Ok(serde_json::from_str(&json)?)
}

pub fn wasi_write_string(wasi_env: &WasiEnv, buf: &str) {
    let mut state = wasi_env.state();
    let wasi_file = state.fs.stdin_mut().unwrap().as_mut().unwrap();
    writeln!(wasi_file, "{}\r", buf).unwrap();
}

pub fn wasi_write_object(wasi_env: &WasiEnv, object: &(impl Serialize + ?Sized)) {
    wasi_write_string(wasi_env, &serde_json::to_string(&object).unwrap());
}

pub struct PluginHandler {}

fn find_all_plugins() -> Vec<PathBuf> {
    let mut plugin_paths = Vec::new();
    let path = plugins_directory().expect("Couldn't obtain plugin dirs");
    let _ = path.read_dir().map(|dir| {
        dir.flat_map(|item| item.map(|p| p.path()).ok())
            .map(|dir| dir.join("plugin.toml"))
            .filter(|f| f.exists())
            .for_each(|f| plugin_paths.push(f))
    });
    plugin_paths
}

fn load_plugin(path: &Path) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut plugin: PluginDescription = toml::from_str(&contents)?;
    plugin.dir = Some(path.parent().unwrap().canonicalize()?);
    plugin.wasm = plugin.wasm.as_ref().and_then(|wasm| {
        Some(
            path.parent()?
                .join(wasm)
                .canonicalize()
                .ok()?
                .to_str()?
                .to_string(),
        )
    });
    plugin.themes = plugin.themes.as_ref().map(|themes| {
        themes
            .iter()
            .filter_map(|theme| {
                Some(
                    path.parent()?
                        .join(theme)
                        .canonicalize()
                        .ok()?
                        .to_str()?
                        .to_string(),
                )
            })
            .collect()
    });
    Ok(plugin)
}

pub fn plugins_directory() -> Option<PathBuf> {
    match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
        Some(dir) => {
            if !dir.data_local_dir().exists() {
                match std::fs::create_dir_all(dir.data_local_dir()) {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!(target: "lapce_proxy::plugin::plugins_directory", "{e}")
                    }
                };
            }
            Some(dir.data_local_dir().join("plugins"))
        }
        None => None,
    }
}

pub fn config_directory() -> Option<PathBuf> {
    match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
        Some(dir) => {
            if !dir.config_dir().exists() {
                match std::fs::create_dir_all(dir.config_dir()) {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!(target: "lapce_proxy::plugin::config_directory", "{e}")
                    }
                };
            }
            Some(dir.config_dir().to_path_buf())
        }
        None => None,
    }
}

pub fn send_plugin_notification(
    plugin_sender: &Sender<PluginRpcMessage>,
    notification: NewPluginNotification,
) {
    let _ = plugin_sender.send(RpcMessage::Notification(notification));
}
