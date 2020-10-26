use anyhow::{anyhow, Result};
use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use std::{
    borrow::Borrow,
    borrow::Cow,
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
use xi_rope::{LinesMetric, Rope, RopeDelta};
use xi_rpc::{self, Handler, RpcLoop, RpcPeer};

use crate::{buffer::BufferId, editor::Counter, state::LAPCE_STATE};

pub type PluginName = String;

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PluginId(pub usize);

pub struct PluginCatalog {
    items: HashMap<PluginName, Arc<PluginDescription>>,
    locations: HashMap<PathBuf, Arc<PluginDescription>>,
    id_counter: Counter,
    running: Vec<Plugin>,
}

pub struct PluginHandler {}

#[derive(Deserialize)]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub exec_path: PathBuf,
    dir: Option<PathBuf>,
}

pub struct Plugin {
    peer: RpcPeer,
    id: PluginId,
    name: String,
    process: Child,
}

impl Plugin {
    pub fn new_buffer(&self, info: &PluginBufferInfo) {
        self.peer
            .send_rpc_notification("new_buffer", &json!({ "buffer_info": [info] }))
    }

    pub fn initialize(&self) {
        self.peer.send_rpc_notification(
            "initialize",
            &json!({
                "plugin_id": self.id,
            }),
        )
    }
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            items: HashMap::new(),
            locations: HashMap::new(),
            id_counter: Counter::default(),
            running: Vec::new(),
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

    pub fn start_all(&mut self) {
        for (_, manifest) in self.items.clone().iter() {
            start_plugin_process(manifest.clone(), self.next_plugin_id());
        }
    }

    pub fn send_rpc_notification(&self, notification: HostNotification) {
        let notification = serde_json::to_value(notification).unwrap();
        let method = notification.get("method").unwrap().as_str().unwrap();
        let params = notification.get("params").unwrap();
        self.running.iter().for_each(|plugin| {
            plugin.peer.send_rpc_notification(method, params);
        });
    }

    pub fn new_buffer(&self, info: &PluginBufferInfo) {
        // let notification = HostNotification::NewBuffer {
        //     buffer_info: info.clone(),
        // };
        // self.send_rpc_notification(notification);
    }

    pub fn update(
        &self,
        buffer_id: &BufferId,
        delta: &RopeDelta,
        new_len: usize,
        new_line_count: usize,
        rev: u64,
    ) {
        // let notification = HostNotification::Update {
        //     buffer_id: buffer_id.clone(),
        //     delta: delta.clone(),
        //     new_len,
        //     new_line_count,
        //     rev,
        // };
        // self.send_rpc_notification(notification);
    }

    pub fn get_completion(
        &self,
        buffer_id: &BufferId,
        request_id: usize,
        offset: usize,
    ) {
        // println!("retrive completion");
        // let notification = HostNotification::GetCompletion {
        //     buffer_id: buffer_id.clone(),
        //     request_id,
        //     offset,
        // };
        // self.send_rpc_notification(notification);
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
    println!("mainfiest paths {:?}", manifest_paths);
    manifest_paths
}

fn load_manifest(path: &Path) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut manifest: PluginDescription = toml::from_str(&contents)?;
    // normalize relative paths
    manifest.dir = Some(path.parent().unwrap().canonicalize()?);
    if manifest.exec_path.starts_with("./") {
        manifest.exec_path = path
            .parent()
            .unwrap()
            .join(manifest.exec_path)
            .canonicalize()?;
    }
    Ok(manifest)
}

fn start_plugin_process(plugin_desc: Arc<PluginDescription>, id: PluginId) {
    thread::spawn(move || {
        println!(
            "start plugin {:?} {:?}",
            plugin_desc.exec_path, plugin_desc.dir
        );
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
        if let Err(e) = child.map(|mut child| {
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

            plugin.initialize();
            LAPCE_STATE.plugins.lock().running.push(plugin);

            let mut handler = PluginHandler {};
            if let Err(e) =
                looper.mainloop(|| BufReader::new(child_stdout), &mut handler)
            {
                println!("plugin main loop failed {} {:?}", e, plugin_desc.dir);
            }
            println!("plugin main loop exit {:?}", plugin_desc.dir);
        }) {
            println!(
                "can't start plugin sub process {} {:?}",
                e, plugin_desc.exec_path
            );
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginBufferInfo {
    pub buffer_id: BufferId,
    pub language_id: String,
    pub path: String,
    pub rev: u64,
    pub buf_size: usize,
    pub nb_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
/// RPC Notifications sent from the host
pub enum HostNotification {
    Initialize {
        plugin_id: PluginId,
    },
    Update {
        buffer_id: BufferId,
        delta: RopeDelta,
        new_len: usize,
        new_line_count: usize,
        rev: u64,
    },
    GetCompletion {
        buffer_id: BufferId,
        request_id: usize,
        offset: usize,
    },
    NewBuffer {
        buffer_info: PluginBufferInfo,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
/// RPC Request sent from the host
pub enum HostRequest {}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {
    ShowCompletion {
        request_id: usize,
        result: Value,
    },
    StartLspServer {
        exec_path: String,
        language_id: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginRequest {
    GetData {
        start: usize,
        unit: TextUnit,
        max_size: usize,
        rev: u64,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TextUnit {
    /// The requested offset is in bytes. The returned chunk will be valid
    /// UTF8, and is guaranteed to include the byte specified the offset.
    Utf8,
    /// The requested offset is a line number. The returned chunk will begin
    /// at the offset of the requested line.
    Line,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetDataResponse {
    pub chunk: String,
    pub offset: usize,
    pub first_line: usize,
    pub first_line_offset: usize,
}

/// Range expressed in terms of PluginPosition. Meant to be sent from
/// plugin to core.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

/// Hover Item sent from Plugin to Core
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Hover {
    pub content: String,
    pub range: Option<Range>,
}

pub struct PluginCommand<T> {
    pub buffer_id: BufferId,
    pub plugin_id: PluginId,
    pub cmd: T,
}

impl<T: Serialize> Serialize for PluginCommand<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut v = serde_json::to_value(&self.cmd).map_err(ser::Error::custom)?;
        v["params"]["view_id"] = json!(self.buffer_id);
        v["params"]["plugin_id"] = json!(self.plugin_id);
        v.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PluginCommand<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct InnerIds {
            buffer_id: BufferId,
            plugin_id: PluginId,
        }
        #[derive(Deserialize)]
        struct IdsWrapper {
            params: InnerIds,
        }

        let v = Value::deserialize(deserializer)?;
        let helper = IdsWrapper::deserialize(&v).map_err(de::Error::custom)?;
        let InnerIds {
            buffer_id,
            plugin_id,
        } = helper.params;
        let cmd = T::deserialize(v).map_err(de::Error::custom)?;
        Ok(PluginCommand {
            buffer_id,
            plugin_id,
            cmd,
        })
    }
}

impl Handler for PluginHandler {
    type Notification = PluginCommand<PluginNotification>;
    type Request = PluginCommand<PluginRequest>;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
        let PluginCommand {
            buffer_id,
            plugin_id,
            cmd,
        } = rpc;
        match cmd {
            PluginNotification::ShowCompletion { request_id, result } => {
                LAPCE_STATE
                    .editor_split
                    .lock()
                    .show_completion(request_id, result);
            }
            PluginNotification::StartLspServer {
                exec_path,
                language_id,
            } => {
                LAPCE_STATE
                    .lsp
                    .lock()
                    .start_server(&exec_path, &language_id);
            }
        }
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<serde_json::Value, xi_rpc::RemoteError> {
        let PluginCommand {
            buffer_id,
            plugin_id,
            cmd,
        } = rpc;
        match cmd {
            PluginRequest::GetData {
                start,
                unit,
                max_size,
                rev,
            } => {
                let mut editor_split = LAPCE_STATE.editor_split.lock();
                let buffer = editor_split.get_buffer(&buffer_id).unwrap();
                let text_cow = Cow::Borrowed(&buffer.rope);
                let text = &text_cow;
                let offset = unit.resolve_offset(text.borrow(), start);
                if offset.is_none() {
                    return Err(xi_rpc::RemoteError::InvalidRequest(None));
                }
                let offset = offset.unwrap();
                let mut end_off = offset.saturating_add(max_size);
                if end_off >= text.len() {
                    end_off = text.len();
                } else {
                    // Snap end to codepoint boundary.
                    end_off = text.prev_codepoint_offset(end_off + 1).unwrap();
                }

                let chunk = text.slice_to_cow(offset..end_off).into_owned();
                let first_line = text.line_of_offset(offset);
                let first_line_offset = offset - text.offset_of_line(first_line);

                // Some(GetDataResponse { chunk, offset, first_line, first_line_offset })
                return Ok(json!(GetDataResponse {
                    chunk,
                    offset,
                    first_line,
                    first_line_offset,
                }));
            }
        }
        println!("get request from plugin");
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}

impl TextUnit {
    /// Converts an offset in some unit to a concrete byte offset. Returns
    /// `None` if the input offset is out of bounds in its unit space.
    pub fn resolve_offset<T: Borrow<Rope>>(
        self,
        text: T,
        offset: usize,
    ) -> Option<usize> {
        let text = text.borrow();
        match self {
            TextUnit::Utf8 => {
                if offset > text.len() {
                    None
                } else {
                    text.at_or_prev_codepoint_boundary(offset)
                }
            }
            TextUnit::Line => {
                let max_line_number = text.measure::<LinesMetric>() + 1;
                if offset > max_line_number {
                    None
                } else {
                    text.offset_of_line(offset).into()
                }
            }
        }
    }
}
