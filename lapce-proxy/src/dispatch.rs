use crate::buffer::{get_mod_time, load_file, Buffer};
use crate::lsp::LspCatalog;
use crate::plugin::PluginCatalog;
use crate::terminal::Terminal;
use crate::watcher::{FileWatcher, Notify, WatchToken};
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
use lapce_rpc::buffer::{BufferHeadResponse, BufferId, NewBufferResponse};
use lapce_rpc::core::CoreNotification;
use lapce_rpc::file::FileNodeItem;
use lapce_rpc::proxy::{ProxyNotification, ProxyRequest, ReadDirResponse};
use lapce_rpc::source_control::{DiffInfo, FileDiff};
use lapce_rpc::terminal::TermId;
use lapce_rpc::{self, Call, RequestId, RpcObject};
use parking_lot::Mutex;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::{collections::HashSet, io::BufRead};
use xi_rope::Rope;

const OPEN_FILE_EVENT_TOKEN: WatchToken = WatchToken(1);
const WORKSPACE_EVENT_TOKEN: WatchToken = WatchToken(2);

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
    pub file_watcher: Arc<Mutex<Option<FileWatcher>>>,
    last_diff: Arc<Mutex<DiffInfo>>,
}

impl Notify for Dispatcher {
    fn notify(&self) {
        self.handle_fs_events();
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
            file_watcher: Arc::new(Mutex::new(None)),
            last_diff: Arc::new(Mutex::new(DiffInfo::default())),
        };
        *dispatcher.file_watcher.lock() = Some(FileWatcher::new(dispatcher.clone()));
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
                match rpc.into_rpc::<ProxyNotification, ProxyRequest>() {
                    Ok(Call::Request(id, request)) => {
                        self.handle_request(id, request);
                    }
                    Ok(Call::Notification(notification)) => {
                        if let ProxyNotification::Shutdown {} = &notification {
                            for (_, sender) in self.terminals.lock().iter() {
                                #[allow(deprecated)]
                                let _ = sender.send(Msg::Shutdown);
                            }
                            self.open_files.lock().clear();
                            self.buffers.lock().clear();
                            self.plugins.lock().stop();
                            self.lsp.lock().stop();
                            self.file_watcher.lock().take();
                            return Ok(());
                        }
                        self.handle_notification(notification);
                    }
                    Err(e) => {
                        eprintln!("{e:?}")
                    }
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

    pub fn respond_rpc<T: serde::Serialize>(
        &self,
        id: RequestId,
        result: Result<T>,
    ) {
        let mut resp = json!({ "id": id });
        match result {
            Ok(v) => resp["result"] = serde_json::to_value(v).unwrap(),
            Err(e) => {
                resp["error"] = json!({
                    "code": 0,
                    "message": format!("{}",e),
                })
            }
        }
        let _ = self.sender.send(resp);
    }

    pub fn send_rpc_notification<T: serde::Serialize>(&self, notification: T) {
        let _ = self
            .sender
            .send(serde_json::to_value(notification).unwrap());
    }

    pub fn send_notification(&self, method: &str, params: Value) {
        let _ = self.sender.send(json!({
            "method": method,
            "params": params,
        }));
    }

    fn handle_fs_events(&self) {
        let mut events = {
            self.file_watcher
                .lock()
                .as_mut()
                .map(|w| w.take_events())
                .unwrap_or_default()
        };

        for (token, event) in events.drain(..) {
            match token {
                OPEN_FILE_EVENT_TOKEN => self.handle_open_file_fs_event(event),
                WORKSPACE_EVENT_TOKEN => self.handle_workspace_fs_event(event),
                _ => {}
            }
        }
    }

    fn handle_open_file_fs_event(&self, event: notify::Event) {
        use notify::event::*;
        let path = match event.kind {
            EventKind::Modify(_) => &event.paths[0],
            _ => {
                return;
            }
        };

        if let Some(path) = path.to_str() {
            if let Some(buffer_id) = self.open_files.lock().get(path) {
                if let Some(buffer) = self.buffers.lock().get_mut(buffer_id) {
                    if get_mod_time(&buffer.path) == buffer.mod_time {
                        return;
                    }
                    if let Ok(content) = load_file(&buffer.path) {
                        self.send_rpc_notification(
                            CoreNotification::OpenFileChanged {
                                path: buffer.path.clone(),
                                content,
                            },
                        );
                    }
                }
            }
        }
    }

    fn handle_workspace_fs_event(&self, event: notify::Event) {
        if let Some(workspace) = self.workspace.lock().clone() {
            self.send_rpc_notification(CoreNotification::FileChange { event });
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

    fn handle_notification(&self, rpc: ProxyNotification) {
        use ProxyNotification::*;
        match rpc {
            Initialize { workspace } => {
                *self.workspace.lock() = Some(workspace.clone());
                self.file_watcher.lock().as_mut().unwrap().watch(
                    &workspace,
                    true,
                    WORKSPACE_EVENT_TOKEN,
                );
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
            Shutdown {} => {}
            Update {
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
            InstallPlugin { plugin } => {
                let catalog = self.plugins.clone();
                let dispatcher = self.clone();
                std::thread::spawn(move || {
                    if let Err(e) =
                        catalog.lock().install_plugin(dispatcher.clone(), plugin)
                    {
                        eprintln!("install plugin error {e}");
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
            NewTerminal {
                term_id,
                cwd,
                shell,
            } => {
                let mut terminal = Terminal::new(term_id, cwd, shell, 50, 10);
                let tx = terminal.tx.clone();
                self.terminals.lock().insert(term_id, tx);
                let dispatcher = self.clone();
                std::thread::spawn(move || {
                    terminal.run(dispatcher);
                });
            }
            TerminalClose { term_id } => {
                let mut terminals = self.terminals.lock();
                if let Some(tx) = terminals.remove(&term_id) {
                    #[allow(deprecated)]
                    let _ = tx.send(Msg::Shutdown);
                }
            }
            TerminalWrite { term_id, content } => {
                let terminals = self.terminals.lock();
                if let Some(tx) = terminals.get(&term_id) {
                    #[allow(deprecated)]
                    let _ = tx.send(Msg::Input(content.into_bytes().into()));
                }
            }
            TerminalResize {
                term_id,
                width,
                height,
            } => {
                let terminals = self.terminals.lock();
                if let Some(tx) = terminals.get(&term_id) {
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
            }
            GitCommit { message, diffs } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    match git_commit(&workspace, &message, diffs) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitCheckout { branch } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    match git_checkout(&workspace, &branch) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
        }
    }

    fn handle_request(&self, id: RequestId, rpc: ProxyRequest) {
        use ProxyRequest::*;
        match rpc {
            NewBuffer { buffer_id, path } => {
                self.file_watcher.lock().as_mut().unwrap().watch(
                    &path,
                    false,
                    OPEN_FILE_EVENT_TOKEN,
                );
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
            BufferHead { path, .. } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    let result = file_get_head(&workspace, &path);
                    if let Ok((_blob_id, content)) = result {
                        let resp = BufferHeadResponse {
                            version: "head".to_string(),
                            content,
                        };
                        let _ = self.sender.send(json!({
                            "id": id,
                            "result": resp,
                        }));
                    }
                }
            }
            GetCompletion {
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
            CompletionResolve {
                buffer_id,
                completion_item,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp
                    .lock()
                    .completion_resolve(id, buffer, &completion_item);
            }
            GetHover {
                buffer_id,
                position,
                request_id,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_hover(id, request_id, buffer, position);
            }
            GetSignature {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_signature(id, buffer, position);
            }
            GetReferences {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_references(id, buffer, position);
            }
            GetDefinition {
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
            GetCodeActions {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_code_actions(id, buffer, position);
            }
            GetDocumentSymbols { buffer_id } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_document_symbols(id, buffer);
            }
            GetDocumentFormatting { buffer_id } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_document_formatting(id, buffer);
            }
            ReadDir { path } => {
                let local_dispatcher = self.clone();
                thread::spawn(move || {
                    let result = fs::read_dir(path)
                        .map(|entries| {
                            let items = entries
                                .into_iter()
                                .filter_map(|entry| {
                                    entry
                                        .map(|e| {
                                            (
                                                e.path(),
                                                FileNodeItem {
                                                    path_buf: e.path(),
                                                    is_dir: e.path().is_dir(),
                                                    open: false,
                                                    read: false,
                                                    children: HashMap::new(),
                                                    children_open_count: 0,
                                                },
                                            )
                                        })
                                        .ok()
                                })
                                .collect::<HashMap<PathBuf, FileNodeItem>>();

                            ReadDirResponse { items }
                        })
                        .map_err(|e| anyhow!(e));
                    local_dispatcher.respond_rpc(id, result);
                });
            }
            GetFiles { .. } => {
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
            Save { rev, buffer_id } => {
                let mut buffers = self.buffers.lock();
                let buffer = buffers.get_mut(&buffer_id).unwrap();
                let resp = buffer.save(rev).map(|_r| json!({}));
                self.lsp.lock().save_buffer(buffer);
                self.respond(id, resp);
            }
            SaveBufferAs {
                buffer_id,
                path,
                rev,
                content,
            } => {
                let mut buffer =
                    Buffer::new(buffer_id, path.clone(), self.git_sender.clone());
                buffer.rope = Rope::from(content);
                buffer.rev = rev;
                let resp = buffer.save(rev).map(|_r| json!({}));
                if resp.is_ok() {
                    self.buffers.lock().insert(buffer_id, buffer);
                    self.open_files
                        .lock()
                        .insert(path.to_str().unwrap().to_string(), buffer_id);
                    let _ = self.git_sender.send((buffer_id, 0));
                }
                self.respond(id, resp);
            }
            GlobalSearch { pattern } => {
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

fn git_checkout(workspace_path: &Path, branch: &str) -> Result<()> {
    let repo = Repository::open(
        workspace_path
            .to_str()
            .ok_or_else(|| anyhow!("workspace path can't changed to str"))?,
    )?;
    let (object, reference) = repo.revparse_ext(branch)?;
    repo.checkout_tree(&object, None)?;
    repo.set_head(reference.unwrap().name().unwrap())?;
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
