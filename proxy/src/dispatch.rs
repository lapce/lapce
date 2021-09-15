use crate::buffer::{get_mod_time, Buffer, BufferId};
use crate::core_proxy::CoreProxy;
use crate::lsp::LspCatalog;
use crate::plugin::PluginCatalog;
use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use git2::{DiffOptions, Oid, Repository};
use jsonrpc_lite::{self, JsonRpc};
use lapce_rpc::{self, Call, RequestId, RpcObject};
use lsp_types::{CompletionItem, Position, TextDocumentContentChangeEvent};
use notify::DebouncedEvent;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use std::{cmp, fs};
use std::{collections::HashMap, io};
use std::{collections::HashSet, io::BufRead};
use std::{path::PathBuf, sync::atomic::AtomicBool};
use std::{sync::atomic, thread};
use std::{sync::Arc, time::Duration};
use xi_core_lib::watcher::{EventQueue, FileWatcher, Notify, WatchToken};
use xi_rope::{RopeDelta, RopeInfo};

pub const OPEN_FILE_EVENT_TOKEN: WatchToken = WatchToken(1);
pub const GIT_EVENT_TOKEN: WatchToken = WatchToken(2);

#[derive(Clone)]
pub struct Dispatcher {
    pub sender: Arc<Sender<Value>>,
    pub git_sender: Sender<(BufferId, u64)>,
    pub workspace: Arc<Mutex<PathBuf>>,
    pub buffers: Arc<Mutex<HashMap<BufferId, Buffer>>>,
    open_files: Arc<Mutex<HashMap<String, BufferId>>>,
    plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
    pub watcher: Arc<Mutex<Option<FileWatcher>>>,
    pub workspace_updated: Arc<AtomicBool>,
}

impl Notify for Dispatcher {
    fn notify(&self) {
        let dispatcher = self.clone();
        thread::spawn(move || {
            for (token, event) in
                { dispatcher.watcher.lock().as_mut().unwrap().take_events() }
                    .drain(..)
            {
                match token {
                    OPEN_FILE_EVENT_TOKEN => match event {
                        DebouncedEvent::Write(path)
                        | DebouncedEvent::Create(path) => {
                            if let Some(buffer_id) = {
                                dispatcher
                                    .open_files
                                    .lock()
                                    .get(&path.to_str().unwrap().to_string())
                            } {
                                if let Some(buffer) =
                                    dispatcher.buffers.lock().get_mut(buffer_id)
                                {
                                    if get_mod_time(&buffer.path) == buffer.mod_time
                                    {
                                        return;
                                    }
                                    if !buffer.dirty {
                                        buffer.reload();
                                        dispatcher.lsp.lock().update(
                                            buffer,
                                            &TextDocumentContentChangeEvent {
                                                range: None,
                                                range_length: None,
                                                text: buffer.get_document(),
                                            },
                                            buffer.rev,
                                        );
                                        dispatcher.sender.send(json!({
                                            "method": "reload_buffer",
                                            "params": {
                                                "buffer_id": buffer_id,
                                                "rev": buffer.rev,
                                                "new_content": buffer.get_document(),
                                            },
                                        }));
                                    }
                                }
                            }
                        }
                        _ => (),
                    },
                    GIT_EVENT_TOKEN => {
                        dispatcher
                            .workspace_updated
                            .store(true, atomic::Ordering::Relaxed);
                    }
                    WatchToken(_) => {}
                }
            }
        });
    }
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
    CompletionResolve {
        buffer_id: BufferId,
        completion_item: CompletionItem,
    },
    GetSignature {
        buffer_id: BufferId,
        position: Position,
    },
    GetReferences {
        buffer_id: BufferId,
        position: Position,
    },
    GetDefinition {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GetCodeActions {
        buffer_id: BufferId,
        position: Position,
    },
    GetDocumentSymbols {
        buffer_id: BufferId,
    },
    GetDocumentFormatting {
        buffer_id: BufferId,
    },
    GetFiles {
        path: String,
    },
    ReadDir {
        path: PathBuf,
    },
    Save {
        rev: u64,
        buffer_id: BufferId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBufferResponse {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Ord, Eq)]
pub struct FileNodeItem {
    pub path_buf: PathBuf,
    pub is_dir: bool,
    pub read: bool,
    pub open: bool,
    pub children: Vec<FileNodeItem>,
}

impl std::cmp::PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        let self_dir = self.is_dir;
        let other_dir = other.is_dir;
        if self_dir && !other_dir {
            return Some(cmp::Ordering::Less);
        }
        if !self_dir && other_dir {
            return Some(cmp::Ordering::Greater);
        }

        let self_file_name = self.path_buf.file_name()?.to_str()?.to_lowercase();
        let other_file_name = other.path_buf.file_name()?.to_str()?.to_lowercase();
        if self_file_name.starts_with(".") && !other_file_name.starts_with(".") {
            return Some(cmp::Ordering::Less);
        }
        if !self_file_name.starts_with(".") && other_file_name.starts_with(".") {
            return Some(cmp::Ordering::Greater);
        }
        self_file_name.partial_cmp(&other_file_name)
    }
}

impl Dispatcher {
    pub fn new(sender: Sender<Value>) -> Dispatcher {
        let plugins = PluginCatalog::new();
        let (git_sender, git_receiver) = unbounded();
        let dispatcher = Dispatcher {
            sender: Arc::new(sender),
            git_sender,
            workspace: Arc::new(Mutex::new(PathBuf::new())),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            lsp: Arc::new(Mutex::new(LspCatalog::new())),
            watcher: Arc::new(Mutex::new(None)),
            workspace_updated: Arc::new(AtomicBool::new(false)),
        };
        *dispatcher.watcher.lock() = Some(FileWatcher::new(dispatcher.clone()));
        dispatcher.lsp.lock().dispatcher = Some(dispatcher.clone());
        dispatcher.plugins.lock().reload();
        dispatcher.plugins.lock().start_all(dispatcher.clone());
        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            local_dispatcher.start_update_process(git_receiver);
        });

        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            local_dispatcher.monitor_workspace_update();
        });
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

    pub fn monitor_workspace_update(&self) -> Result<()> {
        loop {
            thread::sleep(Duration::from_secs(1));
            if self.workspace_updated.load(atomic::Ordering::Relaxed) {
                self.workspace_updated
                    .store(false, atomic::Ordering::Relaxed);
                if let Some(diff_files) = git_diff(&self.workspace.lock()) {
                    self.send_notification(
                        "diff_files",
                        json!({
                            "files": diff_files,
                        }),
                    );
                }
            }
        }
    }

    pub fn start_update_process(
        &self,
        receiver: Receiver<(BufferId, u64)>,
    ) -> Result<()> {
        loop {
            let workspace = self.workspace.lock().clone();
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

            self.lsp.lock().get_semantic_tokens(buffer);

            if let Some((diff, line_changes)) =
                file_git_diff(&workspace, &PathBuf::from(path), &content)
            {
                eprintln!("diff {:?}", diff);
                self.sender.send(json!({
                    "method": "update_git",
                    "params": {
                        "buffer_id": buffer_id,
                        "line_changes": line_changes,
                        "rev": rev,
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

    pub fn respond(&self, id: RequestId, result: Result<Value>) {
        let mut resp = json!({ "id": id });
        match result {
            Ok(v) => resp["result"] = v,
            Err(e) => {
                resp["error"] = json!({
                    "code": 0,
                    "message": format!("{}",e),
                })
            }
        }
        self.sender.send(resp);
    }

    pub fn send_notification(&self, method: &str, params: Value) {
        self.sender.send(json!({
            "method": method,
            "params": params,
        }));
    }

    fn handle_notification(&self, rpc: Notification) {
        match rpc {
            Notification::Initialize { workspace } => {
                *self.workspace.lock() = workspace.clone();
                let mut items = Vec::new();
                if let Ok(entries) = fs::read_dir(&workspace) {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let item = FileNodeItem {
                                path_buf: entry.path(),
                                is_dir: entry.path().is_dir(),
                                open: false,
                                read: false,
                                children: Vec::new(),
                            };
                            items.push(item);
                        }
                    }
                }
                self.send_notification(
                    "list_dir",
                    json!({
                        "items": items,
                    }),
                );
                self.watcher.lock().as_mut().unwrap().watch(
                    &workspace,
                    true,
                    GIT_EVENT_TOKEN,
                );
                if let Some(diff_files) = git_diff(&workspace) {
                    self.send_notification(
                        "diff_files",
                        json!({
                            "files": diff_files,
                        }),
                    );
                }
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
                self.watcher.lock().as_mut().unwrap().watch(
                    &path,
                    true,
                    OPEN_FILE_EVENT_TOKEN,
                );
                self.open_files
                    .lock()
                    .insert(path.to_str().unwrap().to_string(), buffer_id);
                let buffer = Buffer::new(buffer_id, path, self.git_sender.clone());
                let content = buffer.rope.to_string();
                self.buffers.lock().insert(buffer_id, buffer);
                self.git_sender.send((buffer_id, 0));
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
            Request::CompletionResolve {
                buffer_id,
                completion_item,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp
                    .lock()
                    .completion_resolve(id, buffer, &completion_item);
            }
            Request::GetSignature {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_signature(id, buffer, position);
            }
            Request::GetReferences {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_references(id, buffer, position);
            }
            Request::GetDefinition {
                buffer_id,
                position,
                request_id,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp
                    .lock()
                    .get_definition(id, request_id, buffer, position);
            }
            Request::GetCodeActions {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_code_actions(id, buffer, position);
            }
            Request::GetDocumentSymbols { buffer_id } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_document_symbols(id, buffer);
            }
            Request::GetDocumentFormatting { buffer_id } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_document_formatting(id, buffer);
            }
            Request::ReadDir { path } => {
                let local_dispatcher = self.clone();
                thread::spawn(move || {
                    let result = fs::read_dir(path)
                        .map(|entries| {
                            let mut items = entries
                                .into_iter()
                                .filter_map(|entry| {
                                    entry
                                        .map(|e| FileNodeItem {
                                            path_buf: e.path(),
                                            is_dir: e.path().is_dir(),
                                            open: false,
                                            read: false,
                                            children: Vec::new(),
                                        })
                                        .ok()
                                })
                                .collect::<Vec<FileNodeItem>>();
                            items.sort();
                            serde_json::to_value(items).unwrap()
                        })
                        .map_err(|e| anyhow!(e));
                    local_dispatcher.respond(id, result);
                });
            }
            Request::GetFiles { path } => {
                eprintln!("get files");
                let workspace = self.workspace.lock().clone();
                let local_dispatcher = self.clone();
                thread::spawn(move || {
                    let mut items = Vec::new();
                    let mut dirs = Vec::new();
                    dirs.push(workspace.clone());
                    while let Some(dir) = dirs.pop() {
                        for entry in fs::read_dir(dir).unwrap() {
                            let entry = entry.unwrap();
                            let path = entry.path();
                            if entry.file_name().to_str().unwrap().starts_with(".") {
                                continue;
                            }
                            if path.is_dir() {
                                if !path
                                    .as_path()
                                    .to_str()
                                    .unwrap()
                                    .to_string()
                                    .ends_with("target")
                                {
                                    dirs.push(path);
                                }
                            } else {
                                items.push(path.to_str().unwrap().to_string());
                            }
                        }
                    }
                    local_dispatcher
                        .respond(id, Ok(serde_json::to_value(items).unwrap()));
                });
            }
            Request::Save { rev, buffer_id } => {
                let mut buffers = self.buffers.lock();
                let buffer = buffers.get_mut(&buffer_id).unwrap();
                let resp = buffer.save(rev).map(|r| json!({}));
                self.lsp.lock().save_buffer(buffer);
                self.respond(id, resp);
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

fn git_diff(workspace_path: &PathBuf) -> Option<Vec<String>> {
    let repo = Repository::open(workspace_path.to_str()?).ok()?;
    let mut diff_files = HashSet::new();
    let diff = repo.diff_index_to_workdir(None, None).ok()?;
    for delta in diff.deltas() {
        if let Some(path) = delta.new_file().path() {
            if let Some(s) = path.to_str() {
                diff_files.insert(workspace_path.join(s).to_str()?.to_string());
            }
        }
    }
    let cached_diff = repo
        .diff_tree_to_index(
            repo.find_tree(repo.revparse_single("HEAD^{tree}").ok()?.id())
                .ok()
                .as_ref(),
            None,
            None,
        )
        .ok()?;
    for delta in cached_diff.deltas() {
        if let Some(path) = delta.new_file().path() {
            if let Some(s) = path.to_str() {
                diff_files.insert(workspace_path.join(s).to_str()?.to_string());
            }
        }
    }
    let mut diff_files: Vec<String> = diff_files.into_iter().collect();
    diff_files.sort();
    Some(diff_files)
}

fn file_git_diff(
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
