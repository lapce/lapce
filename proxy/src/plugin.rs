use anyhow::Result;
use home::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::thread;
use toml;
use xi_rpc::Handler;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;

use crate::buffer::BufferId;
use crate::core_proxy::CoreProxy;
use crate::dispatch::Dispatcher;

pub type PluginName = String;

#[derive(Clone, Debug, Default)]
pub struct Counter(usize);

impl Counter {
    pub fn next(&mut self) -> usize {
        let n = self.0;
        self.0 = n + 1;
        n + 1
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PluginId(pub usize);

#[derive(Deserialize, Clone)]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub exec_path: PathBuf,
    dir: Option<PathBuf>,
    configuration: Option<Value>,
}

pub struct Plugin {
    id: PluginId,
    dispatcher: Dispatcher,
    configuration: Option<Value>,
    peer: RpcPeer,
    name: String,
    process: Child,
}

pub struct PluginCatalog {
    id_counter: Counter,
    items: HashMap<PluginName, PluginDescription>,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            id_counter: Counter::default(),
            items: HashMap::new(),
        }
    }

    pub fn reload(&mut self) {
        eprintln!("plugin reload from paths");
        self.items.clear();
        self.load();
    }

    pub fn load(&mut self) {
        let all_manifests = find_all_manifests();
        for manifest_path in &all_manifests {
            match load_manifest(manifest_path) {
                Err(e) => eprintln!("load manifest err {}", e),
                Ok(manifest) => {
                    self.items.insert(manifest.name.clone(), manifest);
                }
            }
        }
    }

    pub fn start_all(&mut self, dispatcher: Dispatcher) {
        for (_, manifest) in self.items.clone().iter() {
            start_plugin_process(
                self.next_plugin_id(),
                dispatcher.clone(),
                manifest.clone(),
            );
        }
    }

    pub fn next_plugin_id(&mut self) -> PluginId {
        PluginId(self.id_counter.next())
    }
}

fn start_plugin_process(
    id: PluginId,
    dispatcher: Dispatcher,
    plugin_desc: PluginDescription,
) {
    thread::spawn(move || {
        let parts: Vec<&str> = plugin_desc
            .exec_path
            .to_str()
            .unwrap()
            .split(" ")
            .into_iter()
            .collect();
        let mut child = Command::new(parts[0]);
        for part in &parts[1..] {
            child.arg(part);
        }
        child.current_dir(plugin_desc.dir.as_ref().unwrap());
        let child = child.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn();
        if child.is_err() {
            eprintln!("can't start proxy {:?}", child);
            return;
        }
        let mut child = child.unwrap();
        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let mut looper = RpcLoop::new(child_stdin);
        let peer: RpcPeer = Box::new(looper.get_raw_peer());
        let name = plugin_desc.name.clone();
        let plugin = Plugin {
            id,
            dispatcher: dispatcher.clone(),
            configuration: plugin_desc.configuration.clone(),
            peer,
            process: child,
            name,
        };
        eprintln!("plugin main loop starting {:?}", &plugin_desc.exec_path);
        plugin.initialize();
        let mut handler = PluginHandler { dispatcher };
        if let Err(e) =
            looper.mainloop(|| BufReader::new(child_stdout), &mut handler)
        {
            eprintln!("plugin main loop failed {} {:?}", e, &plugin_desc.dir);
        }
        eprintln!("plugin main loop exit {:?}", plugin_desc.dir);
    });
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum PluginRequest {}

pub struct PluginHandler {
    dispatcher: Dispatcher,
}

impl Handler for PluginHandler {
    type Notification = PluginNotification;
    type Request = PluginRequest;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
        match &rpc {
            PluginNotification::StartLspServer {
                exec_path,
                language_id,
                options,
            } => {
                self.dispatcher.lsp.lock().start_server(
                    exec_path,
                    language_id,
                    options.clone(),
                );
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

impl Plugin {
    pub fn initialize(&self) {
        self.peer.send_rpc_notification(
            "initialize",
            &json!({
                "plugin_id": self.id,
                "configuration": self.configuration,
            }),
        )
    }
}

fn find_all_manifests() -> Vec<PathBuf> {
    let mut manifest_paths = Vec::new();
    let home = home_dir().unwrap();
    let path = home.join(".lapce").join("plugins");
    path.read_dir().map(|dir| {
        dir.flat_map(|item| item.map(|p| p.path()).ok())
            .map(|dir| dir.join("manifest.toml"))
            .filter(|f| f.exists())
            .for_each(|f| manifest_paths.push(f))
    });
    eprintln!("proxy mainfiest paths {:?}", manifest_paths);
    manifest_paths
}

fn load_manifest(path: &PathBuf) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut manifest: PluginDescription = toml::from_str(&contents)?;
    // normalize relative paths
    //manifest.dir = Some(path.parent().unwrap().canonicalize()?);
    //    if manifest.exec_path.starts_with("./") {
    manifest.dir = Some(path.parent().unwrap().canonicalize()?);
    manifest.exec_path = path
        .parent()
        .unwrap()
        .join(manifest.exec_path)
        .canonicalize()?;
    //   }
    Ok(manifest)
}
