use crate::buffer::{Buffer, BufferId};
use crate::core_proxy::CoreProxy;
use crate::lsp::LspCatalog;
use crate::plugin::PluginCatalog;
use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use git2::Repository;
use jsonrpc_lite::{self, JsonRpc};
use lapce_rpc::{self, Call, RequestId, RpcObject};
use lsp_types::Position;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::{collections::HashMap, io};
use xi_rope::{RopeDelta, RopeInfo};
use xi_rpc::RpcPeer;
use xi_rpc::{Handler, RpcCtx};

#[derive(Clone)]
pub struct Dispatcher {
    pub sender: Arc<Sender<Value>>,
    pub workspace: Arc<Mutex<PathBuf>>,
    buffers: Arc<Mutex<HashMap<BufferId, Buffer>>>,
    plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    Initialize {
        workspace: PathBuf,
    },
    Update {
        buffer_id: BufferId,
        delta: RopeDelta,
        rev: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Request {
    NewBuffer {
        buffer_id: BufferId,
        path: PathBuf,
    },
    GetCompletion {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBufferResponse {
    pub content: String,
}

impl Dispatcher {
    pub fn new(sender: Sender<Value>) -> Dispatcher {
        let plugins = PluginCatalog::new();
        let dispatcher = Dispatcher {
            sender: Arc::new(sender),
            workspace: Arc::new(Mutex::new(PathBuf::new())),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            lsp: Arc::new(Mutex::new(LspCatalog::new())),
        };
        dispatcher.lsp.lock().dispatcher = Some(dispatcher.clone());
        dispatcher.plugins.lock().reload();
        dispatcher.plugins.lock().start_all(dispatcher.clone());
        dispatcher
    }

    pub fn mainloop(&self, receiver: Receiver<Value>) -> Result<()> {
        for msg in receiver {
            let rpc: RpcObject = msg.into();
            if rpc.is_response() {
            } else {
                match rpc.into_rpc::<Notification, Request>() {
                    Ok(Call::Request(id, request)) => {
                        self.handle_request(id, request);
                    }
                    Ok(Call::Notification(notification)) => {
                        self.handle_notification(notification)
                    }
                    Err(e) => {}
                }
            }
        }
        Ok(())
    }

    pub fn start_git_process(
        &self,
        receiver: Receiver<(BufferId, u64)>,
    ) -> Result<()> {
        let workspace = self.workspace.lock().clone();
        loop {
            let (buffer_id, rev) = receiver.recv()?;
            let buffers = self.buffers.lock();
            let buffer = buffers.get(&buffer_id).unwrap();
            let (path, content) = if buffer.rev != rev {
                continue;
            } else {
                (
                    buffer.path.clone(),
                    buffer.slice_to_cow(..buffer.len()).to_string(),
                )
            };

            if let Some((diff, line_changes)) =
                get_git_diff(&workspace, &PathBuf::from(path), &content)
            {
                self.sender.send(json!({
                    "method": "update_git",
                    "params": {
                        "buffer_id": buffer_id,
                        "line_changes": line_changes,
                    },
                }));
            }
        }
    }

    pub fn next<R: BufRead>(
        &self,
        reader: &mut R,
        s: &mut String,
    ) -> Result<RpcObject> {
        s.clear();
        let _ = reader.read_line(s)?;
        if s.is_empty() {
            Err(anyhow!("empty line"))
        } else {
            self.parse(s)
        }
    }

    fn parse(&self, s: &str) -> Result<RpcObject> {
        let val = serde_json::from_str::<Value>(&s)?;
        if !val.is_object() {
            Err(anyhow!("not json object"))
        } else {
            Ok(val.into())
        }
    }

    fn handle_notification(&self, rpc: Notification) {
        match rpc {
            Notification::Initialize { workspace } => {
                *self.workspace.lock() = workspace;
            }
            Notification::Update {
                buffer_id,
                delta,
                rev,
            } => {
                let mut buffers = self.buffers.lock();
                let buffer = buffers.get_mut(&buffer_id).unwrap();
                if let Some(content_change) = buffer.update(&delta, rev) {
                    self.lsp.lock().update(buffer, &content_change, buffer.rev);
                }
            }
        }
    }

    fn handle_request(&self, id: RequestId, rpc: Request) {
        match rpc {
            Request::NewBuffer { buffer_id, path } => {
                let buffer = Buffer::new(buffer_id, path);
                let content = buffer.rope.to_string();
                self.buffers.lock().insert(buffer_id, buffer);
                let resp = NewBufferResponse { content };
                self.sender.send(json!({
                    "id": id,
                    "result": resp,
                }));
            }
            Request::GetCompletion {
                buffer_id,
                position,
                request_id,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp
                    .lock()
                    .get_completion(id, request_id, buffer, position);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
}

fn get_git_diff(
    workspace_path: &PathBuf,
    path: &PathBuf,
    content: &str,
) -> Option<(Vec<DiffHunk>, HashMap<usize, char>)> {
    let repo = Repository::open(workspace_path.to_str()?).ok()?;
    let head = repo.head().ok()?;
    let tree = head.peel_to_tree().ok()?;
    let tree_entry = tree
        .get_path(path.strip_prefix(workspace_path).ok()?)
        .ok()?;
    let blob = repo.find_blob(tree_entry.id()).ok()?;
    let mut patch = git2::Patch::from_blob_and_buffer(
        &blob,
        None,
        content.as_bytes(),
        None,
        None,
    )
    .ok()?;
    let mut line_changes = HashMap::new();
    Some((
        (0..patch.num_hunks())
            .into_iter()
            .filter_map(|i| {
                let hunk = patch.hunk(i).ok()?;
                let hunk = DiffHunk {
                    old_start: hunk.0.old_start(),
                    old_lines: hunk.0.old_lines(),
                    new_start: hunk.0.new_start(),
                    new_lines: hunk.0.new_lines(),
                    header: String::from_utf8(hunk.0.header().to_vec()).ok()?,
                };
                let mut line_diff = 0;
                for line in 0..hunk.old_lines + hunk.new_lines {
                    if let Ok(diff_line) = patch.line_in_hunk(i, line as usize) {
                        match diff_line.origin() {
                            ' ' => {
                                let new_line = diff_line.new_lineno().unwrap();
                                let old_line = diff_line.old_lineno().unwrap();
                                line_diff = new_line as i32 - old_line as i32;
                            }
                            '-' => {
                                let old_line = diff_line.old_lineno().unwrap() - 1;
                                let new_line =
                                    (old_line as i32 + line_diff) as usize;
                                line_changes.insert(new_line, '-');
                                line_diff -= 1;
                            }
                            '+' => {
                                let new_line =
                                    diff_line.new_lineno().unwrap() as usize - 1;
                                if let Some(c) = line_changes.get(&new_line) {
                                    if c == &'-' {
                                        line_changes.insert(new_line, 'm');
                                    }
                                } else {
                                    line_changes.insert(new_line, '+');
                                }
                                line_diff += 1;
                            }
                            _ => continue,
                        }
                        diff_line.origin();
                    }
                }
                Some(hunk)
            })
            .collect(),
        line_changes,
    ))
}
