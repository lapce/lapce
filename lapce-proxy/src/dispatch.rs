use crate::buffer::{get_mod_time, Buffer, BufferId};
use crate::lsp::LspCatalog;
use crate::plugin::{PluginCatalog, PluginDescription};
use crate::terminal::{TermId, Terminal};
use alacritty_terminal::event_loop::Msg;
use alacritty_terminal::term::SizeInfo;
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use directories::BaseDirs;
use git2::{DiffOptions, Repository};
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use lapce_rpc::{self, Call, RequestId, RpcObject};
use lsp_types::{CompletionItem, Position, TextDocumentContentChangeEvent};
use notify::Watcher;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::{cmp, fs};
use std::{collections::HashSet, io::BufRead};
use xi_rope::RopeDelta;

#[derive(Clone)]
pub struct Dispatcher {
    pub sender: Arc<Sender<Value>>,
    pub git_sender: Sender<(BufferId, u64)>,
    pub workspace: Arc<Mutex<Option<PathBuf>>>,
    pub buffers: Arc<Mutex<HashMap<BufferId, Buffer>>>,

    #[allow(deprecated)]
    pub terminals: Arc<Mutex<HashMap<TermId, mio::channel::Sender<Msg>>>>,

    open_files: Arc<Mutex<HashMap<String, BufferId>>>,
    plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
    pub watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    last_diff: Arc<Mutex<DiffInfo>>,
}

impl notify::EventHandler for Dispatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(event) = event {
            for path in event.paths.iter() {
                if let Some(path) = path.to_str() {
                    if let Some(buffer_id) = self.open_files.lock().get(path) {
                        match event.kind {
                            notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_) => {
                                if let Some(buffer) =
                                    self.buffers.lock().get_mut(buffer_id)
                                {
                                    if get_mod_time(&buffer.path) == buffer.mod_time
                                    {
                                        return;
                                    }
                                    if !buffer.dirty {
                                        buffer.reload();
                                        self.lsp.lock().update(
                                            buffer,
                                            &TextDocumentContentChangeEvent {
                                                range: None,
                                                range_length: None,
                                                text: buffer.get_document(),
                                            },
                                            buffer.rev,
                                        );
                                        let _ = self.sender.send(json!({
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
                            _ => (),
                        }
                    }
                }
            }
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    if let Some(workspace) = self.workspace.lock().clone() {
                        if let Some(diff) = git_diff_new(&workspace) {
                            if diff != *self.last_diff.lock() {
                                self.send_notification(
                                    "diff_info",
                                    json!({
                                        "diff": diff,
                                    }),
                                );
                                *self.last_diff.lock() = diff;
                            }
                        }
                    }
                }
                _ => (),
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    Initialize {
        workspace: PathBuf,
    },
    Shutdown {},
    Update {
        buffer_id: BufferId,
        delta: RopeDelta,
        rev: u64,
    },
    NewTerminal {
        term_id: TermId,
        cwd: Option<PathBuf>,
    },
    InstallPlugin {
        plugin: PluginDescription,
    },
    GitCommit {
        message: String,
        diffs: Vec<FileDiff>,
    },
    TerminalWrite {
        term_id: TermId,
        content: String,
    },
    TerminalResize {
        term_id: TermId,
        width: usize,
        height: usize,
    },
    TerminalClose {
        term_id: TermId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
#[allow(clippy::large_enum_variant)]
pub enum Request {
    NewBuffer {
        buffer_id: BufferId,
        path: PathBuf,
    },
    BufferHead {
        buffer_id: BufferId,
        path: PathBuf,
    },
    GetCompletion {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GlobalSearch {
        pattern: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferHeadResponse {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DiffInfo {
    pub head: String,
    pub branches: Vec<String>,
    pub diffs: Vec<FileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileDiff {
    Modified(PathBuf),
    Added(PathBuf),
    Deleted(PathBuf),
    Renamed(PathBuf, PathBuf),
}

impl FileDiff {
    pub fn path(&self) -> &PathBuf {
        match &self {
            FileDiff::Modified(p)
            | FileDiff::Added(p)
            | FileDiff::Deleted(p)
            | FileDiff::Renamed(_, p) => p,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileNodeItem {
    pub path_buf: PathBuf,
    pub is_dir: bool,
    pub read: bool,
    pub open: bool,
    pub children: HashMap<PathBuf, FileNodeItem>,
    pub children_open_count: usize,
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
        if self_file_name.starts_with('.') && !other_file_name.starts_with('.') {
            return Some(cmp::Ordering::Less);
        }
        if !self_file_name.starts_with('.') && other_file_name.starts_with('.') {
            return Some(cmp::Ordering::Greater);
        }
        self_file_name.partial_cmp(&other_file_name)
    }
}

impl FileNodeItem {
    pub fn sorted_children(&self) -> Vec<&FileNodeItem> {
        let mut children = self
            .children
            .iter()
            .map(|(_, item)| item)
            .collect::<Vec<&FileNodeItem>>();
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, true) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
        });
        children
    }

    pub fn sorted_children_mut(&mut self) -> Vec<&mut FileNodeItem> {
        let mut children = self
            .children
            .iter_mut()
            .map(|(_, item)| item)
            .collect::<Vec<&mut FileNodeItem>>();
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, true) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
        });
        children
    }
}

impl Dispatcher {
    pub fn new(sender: Sender<Value>) -> Dispatcher {
        let plugins = PluginCatalog::new();
        let (git_sender, git_receiver) = unbounded();
        let dispatcher = Dispatcher {
            sender: Arc::new(sender),
            git_sender,
            workspace: Arc::new(Mutex::new(None)),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            terminals: Arc::new(Mutex::new(HashMap::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            lsp: Arc::new(Mutex::new(LspCatalog::new())),
            watcher: Arc::new(Mutex::new(None)),
            last_diff: Arc::new(Mutex::new(DiffInfo::default())),
        };
        *dispatcher.watcher.lock() =
            Some(notify::recommended_watcher(dispatcher.clone()).unwrap());
        dispatcher.lsp.lock().dispatcher = Some(dispatcher.clone());

        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            local_dispatcher.plugins.lock().reload();
            let plugins = { local_dispatcher.plugins.lock().items.clone() };
            local_dispatcher.send_notification(
                "installed_plugins",
                json!({
                    "plugins": plugins,
                }),
            );
            local_dispatcher
                .plugins
                .lock()
                .start_all(local_dispatcher.clone());
        });

        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            if let Some(path) = BaseDirs::new().map(|d| PathBuf::from(d.home_dir()))
            {
                local_dispatcher.send_notification(
                    "home_dir",
                    json!({
                        "path": path,
                    }),
                );
            }
        });

        dispatcher.start_update_process(git_receiver);
        dispatcher.send_notification("proxy_connected", json!({}));

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
                        if let Notification::Shutdown {} = &notification {
                            for (_, sender) in self.terminals.lock().iter() {
                                #[allow(deprecated)]
                                let _ = sender.send(Msg::Shutdown);
                            }
                            self.open_files.lock().clear();
                            self.buffers.lock().clear();
                            self.plugins.lock().stop();
                            self.lsp.lock().stop();
                            self.watcher.lock().take();
                            return Ok(());
                        }
                        self.handle_notification(notification);
                    }
                    Err(_e) => {}
                }
            }
        }
        Ok(())
    }

    pub fn start_update_process(&self, receiver: Receiver<(BufferId, u64)>) {
        let buffers = self.buffers.clone();
        let lsp = self.lsp.clone();
        thread::spawn(move || loop {
            match receiver.recv() {
                Ok((buffer_id, rev)) => {
                    let buffers = buffers.lock();
                    let buffer = buffers.get(&buffer_id).unwrap();
                    let (_path, _content) = if buffer.rev != rev {
                        continue;
                    } else {
                        (
                            buffer.path.clone(),
                            buffer.slice_to_cow(..buffer.len()).to_string(),
                        )
                    };

                    lsp.lock().get_semantic_tokens(buffer);
                }
                Err(_) => {
                    return;
                }
            }
        });
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
        let val = serde_json::from_str::<Value>(s)?;
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
        let _ = self.sender.send(resp);
    }

    pub fn send_notification(&self, method: &str, params: Value) {
        let _ = self.sender.send(json!({
            "method": method,
            "params": params,
        }));
    }

    fn handle_notification(&self, rpc: Notification) {
        match rpc {
            Notification::Initialize { workspace } => {
                *self.workspace.lock() = Some(workspace.clone());
                let _ = self
                    .watcher
                    .lock()
                    .as_mut()
                    .unwrap()
                    .watch(&workspace, notify::RecursiveMode::Recursive);
                if let Some(diff) = git_diff_new(&workspace) {
                    self.send_notification(
                        "diff_info",
                        json!({
                            "diff": diff,
                        }),
                    );
                    *self.last_diff.lock() = diff;
                }
            }
            Notification::Shutdown {} => {}
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
            Notification::InstallPlugin { plugin } => {
                let catalog = self.plugins.clone();
                let dispatcher = self.clone();
                std::thread::spawn(move || {
                    if let Err(_e) =
                        catalog.lock().install_plugin(dispatcher.clone(), plugin)
                    {
                    }
                    let plugins = { dispatcher.plugins.lock().items.clone() };
                    dispatcher.send_notification(
                        "installed_plugins",
                        json!({
                            "plugins": plugins,
                        }),
                    );
                });
            }
            Notification::NewTerminal { term_id, cwd } => {
                let mut terminal = Terminal::new(term_id, cwd, 50, 10);
                let tx = terminal.tx.clone();
                self.terminals.lock().insert(term_id, tx);
                let dispatcher = self.clone();
                std::thread::spawn(move || {
                    terminal.run(dispatcher);
                });
            }
            Notification::TerminalClose { term_id } => {
                let mut terminals = self.terminals.lock();
                if let Some(tx) = terminals.remove(&term_id) {
                    #[allow(deprecated)]
                    let _ = tx.send(Msg::Shutdown);
                }
            }
            Notification::TerminalWrite { term_id, content } => {
                let terminals = self.terminals.lock();
                let tx = terminals.get(&term_id).unwrap();

                #[allow(deprecated)]
                let _ = tx.send(Msg::Input(content.into_bytes().into()));
            }
            Notification::TerminalResize {
                term_id,
                width,
                height,
            } => {
                let terminals = self.terminals.lock();
                let tx = terminals.get(&term_id).unwrap();
                let size = SizeInfo::new(
                    width as f32,
                    height as f32,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                    true,
                );

                #[allow(deprecated)]
                let _ = tx.send(Msg::Resize(size));
            }
            Notification::GitCommit { message, diffs } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    if let Err(_e) = git_commit(&workspace, &message, diffs) {}
                }
            }
        }
    }

    fn handle_request(&self, id: RequestId, rpc: Request) {
        match rpc {
            Request::NewBuffer { buffer_id, path } => {
                let _ = self
                    .watcher
                    .lock()
                    .as_mut()
                    .unwrap()
                    .watch(&path, notify::RecursiveMode::Recursive);
                self.open_files
                    .lock()
                    .insert(path.to_str().unwrap().to_string(), buffer_id);
                let buffer = Buffer::new(buffer_id, path, self.git_sender.clone());
                let content = buffer.rope.to_string();
                self.buffers.lock().insert(buffer_id, buffer);
                let _ = self.git_sender.send((buffer_id, 0));
                let resp = NewBufferResponse { content };
                let _ = self.sender.send(json!({
                    "id": id,
                    "result": resp,
                }));
            }
            #[allow(unused_variables)]
            Request::BufferHead { buffer_id, path } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    let result = file_get_head(&workspace, &path);
                    if let Ok((_blob_id, content)) = result {
                        let resp = BufferHeadResponse {
                            id: "head".to_string(),
                            content,
                        };
                        let _ = self.sender.send(json!({
                            "id": id,
                            "result": resp,
                        }));
                    }
                }
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
                            let items = entries
                                .into_iter()
                                .filter_map(|entry| {
                                    entry
                                        .map(|e| FileNodeItem {
                                            path_buf: e.path(),
                                            is_dir: e.path().is_dir(),
                                            open: false,
                                            read: false,
                                            children: HashMap::new(),
                                            children_open_count: 0,
                                        })
                                        .ok()
                                })
                                .collect::<Vec<FileNodeItem>>();
                            serde_json::to_value(items).unwrap()
                        })
                        .map_err(|e| anyhow!(e));
                    local_dispatcher.respond(id, result);
                });
            }
            #[allow(unused_variables)]
            Request::GetFiles { path } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    let local_dispatcher = self.clone();
                    thread::spawn(move || {
                        let mut items = Vec::new();
                        for path in ignore::Walk::new(workspace).flatten() {
                            if let Some(file_type) = path.file_type() {
                                if file_type.is_file() {
                                    items.push(path.into_path());
                                }
                            }
                        }
                        local_dispatcher
                            .respond(id, Ok(serde_json::to_value(items).unwrap()));
                    });
                }
            }
            Request::Save { rev, buffer_id } => {
                let mut buffers = self.buffers.lock();
                let buffer = buffers.get_mut(&buffer_id).unwrap();
                let resp = buffer.save(rev).map(|_r| json!({}));
                self.lsp.lock().save_buffer(buffer);
                self.respond(id, resp);
            }
            Request::GlobalSearch { pattern } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    let local_dispatcher = self.clone();
                    thread::spawn(move || {
                        let mut matches = HashMap::new();
                        let pattern = regex::escape(&pattern);
                        if let Ok(matcher) = RegexMatcherBuilder::new()
                            .case_insensitive(true)
                            .build_literals(&[&pattern])
                        {
                            let mut searcher = SearcherBuilder::new().build();
                            for path in ignore::Walk::new(workspace).flatten() {
                                if let Some(file_type) = path.file_type() {
                                    if file_type.is_file() {
                                        let path = path.into_path();
                                        let mut line_matches = Vec::new();
                                        let _ = searcher.search_path(
                                            &matcher,
                                            path.clone(),
                                            UTF8(|lnum, line| {
                                                let mymatch = matcher
                                                    .find(line.as_bytes())?
                                                    .unwrap();
                                                line_matches.push((
                                                    lnum,
                                                    (mymatch.start(), mymatch.end()),
                                                    line.to_string(),
                                                ));
                                                Ok(true)
                                            }),
                                        );
                                        if !line_matches.is_empty() {
                                            matches
                                                .insert(path.clone(), line_matches);
                                        }
                                    }
                                }
                            }
                        }
                        local_dispatcher
                            .respond(id, Ok(serde_json::to_value(matches).unwrap()));
                    });
                }
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

fn git_commit(
    workspace_path: &Path,
    message: &str,
    diffs: Vec<FileDiff>,
) -> Result<()> {
    let repo = Repository::open(
        workspace_path
            .to_str()
            .ok_or_else(|| anyhow!("workspace path can't changed to str"))?,
    )?;
    let mut index = repo.index()?;
    for diff in diffs {
        match diff {
            FileDiff::Modified(p) | FileDiff::Added(p) => {
                index.add_path(p.strip_prefix(workspace_path)?)?;
            }
            FileDiff::Renamed(a, d) => {
                index.add_path(a.strip_prefix(workspace_path)?)?;
                index.remove_path(d.strip_prefix(workspace_path)?)?;
            }
            FileDiff::Deleted(p) => {
                index.remove_path(p.strip_prefix(workspace_path)?)?;
            }
        }
    }
    index.write()?;
    let tree = index.write_tree()?;
    let tree = repo.find_tree(tree)?;
    let signature = repo.signature()?;
    let parent = repo.head()?.peel_to_commit()?;
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&parent],
    )?;
    Ok(())
}

fn git_delta_format(
    workspace_path: &Path,
    delta: &git2::DiffDelta,
) -> Option<(git2::Delta, git2::Oid, PathBuf)> {
    match delta.status() {
        git2::Delta::Added | git2::Delta::Untracked => Some((
            git2::Delta::Added,
            delta.new_file().id(),
            delta.new_file().path().map(|p| workspace_path.join(p))?,
        )),
        git2::Delta::Deleted => Some((
            git2::Delta::Deleted,
            delta.old_file().id(),
            delta.old_file().path().map(|p| workspace_path.join(p))?,
        )),
        git2::Delta::Modified => Some((
            git2::Delta::Modified,
            delta.new_file().id(),
            delta.new_file().path().map(|p| workspace_path.join(p))?,
        )),
        _ => None,
    }
}

fn git_diff_new(workspace_path: &Path) -> Option<DiffInfo> {
    let repo = Repository::open(workspace_path.to_str()?).ok()?;
    let head = repo.head().ok()?;
    let name = head.shorthand()?.to_string();

    let mut branches = Vec::new();
    for branch in repo.branches(None).ok()? {
        branches.push(branch.ok()?.0.name().ok()??.to_string());
    }

    let mut deltas = Vec::new();
    let mut diff_options = DiffOptions::new();
    let diff = repo
        .diff_index_to_workdir(None, Some(diff_options.include_untracked(true)))
        .ok()?;
    for delta in diff.deltas() {
        if let Some(delta) = git_delta_format(workspace_path, &delta) {
            deltas.push(delta);
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
        if let Some(delta) = git_delta_format(workspace_path, &delta) {
            deltas.push(delta);
        }
    }
    let mut renames = Vec::new();
    let mut renamed_deltas = HashSet::new();

    for (i, delta) in deltas.iter().enumerate() {
        if delta.0 == git2::Delta::Added {
            for (j, d) in deltas.iter().enumerate() {
                if d.0 == git2::Delta::Deleted && d.1 == delta.1 {
                    renames.push((i, j));
                    renamed_deltas.insert(i);
                    renamed_deltas.insert(j);
                    break;
                }
            }
        }
    }

    let mut file_diffs = Vec::new();
    for (i, j) in renames.iter() {
        file_diffs.push(FileDiff::Renamed(
            deltas[*i].2.clone(),
            deltas[*j].2.clone(),
        ));
    }
    for (i, delta) in deltas.iter().enumerate() {
        if renamed_deltas.contains(&i) {
            continue;
        }
        let diff = match delta.0 {
            git2::Delta::Added => FileDiff::Added(delta.2.clone()),
            git2::Delta::Deleted => FileDiff::Deleted(delta.2.clone()),
            git2::Delta::Modified => FileDiff::Modified(delta.2.clone()),
            _ => continue,
        };
        file_diffs.push(diff);
    }
    file_diffs.sort_by_key(|d| match d {
        FileDiff::Modified(p)
        | FileDiff::Added(p)
        | FileDiff::Renamed(p, _)
        | FileDiff::Deleted(p) => p.clone(),
    });
    Some(DiffInfo {
        head: name,
        branches,
        diffs: file_diffs,
    })
}

fn file_get_head(workspace_path: &Path, path: &Path) -> Result<(String, String)> {
    let repo = Repository::open(
        workspace_path
            .to_str()
            .ok_or_else(|| anyhow!("can't to str"))?,
    )?;
    let head = repo.head()?;
    let tree = head.peel_to_tree()?;
    let tree_entry = tree.get_path(path.strip_prefix(workspace_path)?)?;
    let blob = repo.find_blob(tree_entry.id())?;
    let id = blob.id().to_string();
    let content = std::str::from_utf8(blob.content())
        .with_context(|| "content bytes to string")?
        .to_string();
    Ok((id, content))
}

#[allow(dead_code)]
fn file_git_diff(
    workspace_path: &Path,
    path: &Path,
    content: &str,
) -> Option<(Vec<DiffHunk>, HashMap<usize, char>)> {
    let repo = Repository::open(workspace_path.to_str()?).ok()?;
    let head = repo.head().ok()?;
    let tree = head.peel_to_tree().ok()?;
    let tree_entry = tree
        .get_path(path.strip_prefix(workspace_path).ok()?)
        .ok()?;
    let blob = repo.find_blob(tree_entry.id()).ok()?;
    let patch = git2::Patch::from_blob_and_buffer(
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
