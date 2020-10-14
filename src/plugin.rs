use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::BufReader,
    io::Read,
    path::Path,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
};
use toml;
use xi_rpc::{self, RpcLoop, RpcPeer};

use crate::{editor::Counter, state::LapceState};

pub type PluginName = String;

#[derive(Eq, PartialEq, Hash, Clone, Debug)]
pub struct PluginId(pub usize);

pub struct PluginCatalog {
    items: HashMap<PluginName, Arc<PluginDescription>>,
    locations: HashMap<PathBuf, Arc<PluginDescription>>,
    id_counter: Counter,
}

#[derive(Deserialize)]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub exec_path: PathBuf,
}

pub struct Plugin {
    peer: RpcPeer,
    id: PluginId,
    name: String,
    process: Child,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            items: HashMap::new(),
            locations: HashMap::new(),
            id_counter: Counter::default(),
        }
    }

    pub fn next_plugin_id(&mut self) -> PluginId {
        PluginId(self.id_counter.next())
    }

    pub fn reload_from_paths(&mut self, paths: &[PathBuf]) {
        self.items.clear();
        self.locations.clear();
        self.load_from_paths(paths);
    }

    pub fn load_from_paths(&mut self, paths: &[PathBuf]) {
        let all_manifests = find_all_manifests(paths);
        for manifest_path in &all_manifests {
            match load_manifest(manifest_path) {
                Err(e) => (),
                Ok(manifest) => {
                    let manifest = Arc::new(manifest);
                    self.items.insert(manifest.name.clone(), manifest.clone());
                    self.locations.insert(manifest_path.clone(), manifest);
                }
            }
        }
    }
}

fn find_all_manifests(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut manifest_paths = Vec::new();
    for path in paths.iter() {
        let manif_path = path.join("manifest.toml");
        if manif_path.exists() {
            manifest_paths.push(manif_path);
            continue;
        }

        let result = path.read_dir().map(|dir| {
            dir.flat_map(|item| item.map(|p| p.path()).ok())
                .map(|dir| dir.join("manifest.toml"))
                .filter(|f| f.exists())
                .for_each(|f| manifest_paths.push(f))
        });
    }
    manifest_paths
}

fn load_manifest(path: &Path) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut manifest: PluginDescription = toml::from_str(&contents)?;
    // normalize relative paths
    if manifest.exec_path.starts_with("./") {
        manifest.exec_path = path
            .parent()
            .unwrap()
            .join(manifest.exec_path)
            .canonicalize()?;
    }
    Ok(manifest)
}

fn start_plugin_process(
    plugin_desc: Arc<PluginDescription>,
    id: PluginId,
    state: LapceState,
) {
    thread::spawn(move || {
        let child = Command::new(&plugin_desc.exec_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
        child.map(|mut child| {
            let child_stdin = child.stdin.take().unwrap();
            let child_stdout = child.stdout.take().unwrap();
            let mut looper = RpcLoop::new(child_stdin);
            let peer: RpcPeer = Box::new(looper.get_raw_peer());
            let name = plugin_desc.name.clone();
            let plugin = Plugin {
                peer,
                process: child,
                name,
                id,
            };

            looper.mainloop(|| BufReader::new(child_stdout), handler);
        });
    });
}
