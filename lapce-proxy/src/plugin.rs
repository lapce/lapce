use anyhow::{anyhow, Result};
use home::home_dir;
use hotwatch::Hotwatch;
use lapce_rpc::counter::Counter;
use lapce_rpc::plugin::{PluginDescription, PluginId, PluginInfo};
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use toml;
use wasmer::ChainableNamedResolver;
use wasmer::ImportObject;
use wasmer::Store;
use wasmer::WasmerEnv;
use wasmer_wasi::Pipe;
use wasmer_wasi::WasiEnv;
use wasmer_wasi::WasiState;

use crate::dispatch::Dispatcher;

pub type PluginName = String;
pub type PluginNameRef<'a> = &'a str;

#[derive(WasmerEnv, Clone)]
pub(crate) struct PluginEnv {
    wasi_env: WasiEnv,
    desc: PluginDescription,
    dispatcher: Dispatcher,
    /// Events that the plugin wants to receive
    pub subscribed_events: Arc<RwLock<Vec<PluginEventKind>>>,
}
impl PluginEnv {
    pub(crate) fn new(
        wasi_env: WasiEnv,
        desc: PluginDescription,
        dispatcher: Dispatcher,
    ) -> PluginEnv {
        PluginEnv {
            wasi_env,
            desc,
            dispatcher,
            subscribed_events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub(crate) fn can_receive_event(&self, event_kind: PluginEventKind) -> bool {
        self.subscribed_events
            .read()
            .iter()
            .any(|sub_event_kind| *sub_event_kind == event_kind)
    }
}

#[derive(Clone)]
pub(crate) struct PluginNew {
    instance: wasmer::Instance,
    env: PluginEnv,
}
impl PluginNew {
    pub fn send_event(&self, event: PluginEvent) {
        let plugin = self.clone();
        thread::spawn(move || {
            if let Ok(update_fn) = plugin.instance.exports.get_function("update") {
                wasi_write_object(&plugin.env.wasi_env, &event);
                update_fn.call(&[]).unwrap();
            }
        });
    }
}

pub struct PluginCatalog {
    id_counter: Counter,
    pub items: HashMap<PluginName, PluginDescription>,
    plugins: HashMap<PluginName, PluginNew>,
    store: wasmer::Store,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            id_counter: Counter::new(),
            items: HashMap::new(),
            plugins: HashMap::new(),
            store: wasmer::Store::default(),
        }
    }

    pub fn stop(&mut self) {
        self.items.clear();
        self.plugins.clear();
    }

    pub fn reload(&mut self) {
        self.items.clear();
        self.plugins.clear();
        self.load();
    }

    pub fn load(&mut self) {
        let all_plugins = find_all_plugins();
        for plugin_path in &all_plugins {
            match load_plugin(plugin_path) {
                Err(_e) => (),
                Ok(plugin) => {
                    self.items.insert(plugin.name.clone(), plugin);
                }
            }
        }
    }

    pub fn install_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin: PluginDescription,
    ) -> Result<()> {
        let home = home_dir().unwrap();
        let path = home.join(".lapce").join("plugins").join(&plugin.name);
        let _ = std::fs::remove_dir_all(&path);

        std::fs::create_dir_all(&path)?;

        {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path.join("plugin.toml"))?;
            file.write_all(&toml::to_vec(&plugin)?)?;
        }

        {
            let url = format!(
                "https://raw.githubusercontent.com/{}/master/{}",
                plugin.repository, plugin.wasm
            );
            let mut resp = ureq::get(&url).call()?.into_reader();
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path.join(&plugin.wasm))?;
            std::io::copy(&mut resp, &mut file)?;
        }

        let mut plugin = plugin;
        plugin.dir = Some(path.clone());
        plugin.wasm = path
            .join(&plugin.wasm)
            .to_str()
            .ok_or_else(|| anyhow!("path can't to string"))?
            .to_string();
        let p = self.start_plugin(dispatcher, plugin.clone())?;
        self.items.insert(plugin.name.clone(), plugin.clone());
        self.plugins.insert(plugin.name, p);
        Ok(())
    }

    pub fn start_all(&mut self, dispatcher: Dispatcher) {
        for (_, plugin) in self.items.clone().iter() {
            if let Ok(p) = self.start_plugin(dispatcher.clone(), plugin.clone()) {
                self.plugins.insert(plugin.name.clone(), p);
            }
        }
    }

    fn start_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin_desc: PluginDescription,
    ) -> Result<PluginNew> {
        let module = wasmer::Module::from_file(&self.store, &plugin_desc.wasm)?;

        let output = Pipe::new();
        let input = Pipe::new();
        let mut wasi_env = WasiState::new("Lapce")
            .map_dir("/", plugin_desc.dir.clone().unwrap())?
            .stdin(Box::new(input))
            .stdout(Box::new(output))
            .finalize()?;
        let wasi = wasi_env.import_object(&module)?;

        let plugin_env = PluginEnv::new(wasi_env, plugin_desc.clone(), dispatcher);
        let lapce = lapce_exports(&self.store, &plugin_env);
        let instance = wasmer::Instance::new(&module, &lapce.chain_back(wasi))?;
        let plugin = PluginNew {
            instance,
            env: plugin_env,
        };

        let local_plugin = plugin.clone();
        thread::spawn(move || {
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
                    configuration: plugin_desc.configuration,
                },
            );
            initialize.call(&[]).unwrap();
        });

        Ok(plugin)
    }

    pub fn next_plugin_id(&mut self) -> PluginId {
        PluginId(self.id_counter.next())
    }

    /// Send an event to a specific plugin, if it is subscribed to the event
    /// If the plugin does not exist, it does nothing
    pub fn send_event(&self, target: PluginNameRef<'_>, event: PluginEvent) {
        let plugin = self.plugins.get(target);
        if let Some(plugin) = plugin {
            let event_kind = event.kind();
            if plugin.env.can_receive_event(event_kind) {
                plugin.send_event(event);
            }
        }
    }

    /// Broadcast an event to all plugins that are subscribed to the event
    pub fn broadcast_event(&self, event: PluginEvent) {
        let event_kind = event.kind();
        for (_, plugin) in self.plugins.iter() {
            if plugin.env.can_receive_event(event_kind) {
                plugin.send_event(event.clone());
            }
        }
    }
}

impl Default for PluginCatalog {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn lapce_exports(store: &Store, plugin_env: &PluginEnv) -> ImportObject {
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {
    Subscribe {
        events: Vec<PluginEventKind>,
    },
    DebugLog {
        text: String,
    },
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
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

// TODO: Can this be shared between lapce and lapce-plugin-rust?
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEventKind {
    FileEditorClosed,
}

// TODO: Can this be shared between lapce and lapce-plugin-rust?
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PluginEvent {
    /// A file-backed editor was closed
    FileEditorClosed { path: PathBuf },
}
impl PluginEvent {
    pub fn kind(&self) -> PluginEventKind {
        match self {
            PluginEvent::FileEditorClosed { .. } => {
                PluginEventKind::FileEditorClosed
            }
        }
    }
}

fn host_handle_notification(plugin_env: &PluginEnv) {
    let notification: Result<PluginNotification> =
        wasi_read_object(&plugin_env.wasi_env);
    if let Ok(notification) = notification {
        match notification {
            PluginNotification::Subscribe { events } => {
                plugin_env
                    .subscribed_events
                    .write()
                    .extend(events.into_iter());
            }
            PluginNotification::DebugLog { text } => {
                println!("[{}]: {}", plugin_env.desc.name, text);
            }
            PluginNotification::StartLspServer {
                exec_path,
                language_id,
                options,
            } => {
                plugin_env.dispatcher.lsp.lock().start_server(
                    plugin_env
                        .desc
                        .dir
                        .clone()
                        .unwrap()
                        .join(&exec_path)
                        .to_str()
                        .unwrap(),
                    &language_id,
                    options,
                );
            }
            PluginNotification::DownloadFile { url, path } => {
                let mut resp = ureq::get(&url)
                    .call()
                    .expect("request failed")
                    .into_reader();
                let mut out = std::fs::File::create(
                    plugin_env.desc.dir.clone().unwrap().join(path),
                )
                .expect("failed to create file");
                std::io::copy(&mut resp, &mut out).expect("failed to copy content");
            }
            PluginNotification::LockFile { path } => {
                let path = plugin_env.desc.dir.clone().unwrap().join(path);
                let mut n = 0;
                loop {
                    if let Ok(_file) = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                    {
                        return;
                    }
                    if n > 10 {
                        return;
                    }
                    n += 1;
                    let mut hotwatch =
                        Hotwatch::new().expect("hotwatch failed to initialize!");
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    let _ = hotwatch.watch(&path, move |_event| {
                        #[allow(deprecated)]
                        let _ = tx.send(0);
                    });
                    let _ = rx.recv_timeout(Duration::from_secs(10));
                }
            }
            PluginNotification::MakeFileExecutable { path } => {
                let _ = Command::new("chmod")
                    .arg("+x")
                    .arg(&plugin_env.desc.dir.clone().unwrap().join(path))
                    .output();
            }
        }
    }
}

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

#[derive(Serialize, Deserialize, Debug)]
pub enum PluginRequest {}

pub struct PluginHandler {}

fn find_all_plugins() -> Vec<PathBuf> {
    let mut plugin_paths = Vec::new();
    let home = home_dir().unwrap();
    let path = home.join(".lapce").join("plugins");
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
    plugin.wasm = path
        .parent()
        .unwrap()
        .join(&plugin.wasm)
        .canonicalize()?
        .to_str()
        .ok_or_else(|| anyhow!("path can't to string"))?
        .to_string();
    Ok(plugin)
}
