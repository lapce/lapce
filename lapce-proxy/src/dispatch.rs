use crate::buffer::{get_mod_time, load_file, Buffer};
use crate::plugin::lsp::LspCatalog;
use crate::plugin::{catalog::NewPluginCatalog, PluginCatalog};
use crate::plugin::{install_plugin, remove_plugin, PluginCatalogRpcHandler};
use crate::terminal::Terminal;
use crate::watcher::{FileWatcher, Notify, WatchToken};
use crate::{
    buffer::{get_mod_time, load_file, Buffer},
    plugin::plugins_directory,
};
use alacritty_terminal::event_loop::Msg;
use alacritty_terminal::term::SizeInfo;
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use git2::build::CheckoutBuilder;
use git2::{DiffOptions, Repository};
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use lapce_rpc::buffer::{BufferHeadResponse, BufferId, NewBufferResponse};
use lapce_rpc::core::{CoreNotification, CoreRpcHandler};
use lapce_rpc::file::FileNodeItem;
use lapce_rpc::proxy::{
    CoreProxyNotification, CoreProxyRequest, CoreProxyResponse, ProxyHandler,
    ProxyRpcHandler, ReadDirResponse,
};
use lapce_rpc::source_control::{DiffInfo, FileDiff};
use lapce_rpc::style::{LineStyle, SemanticStyles};
use lapce_rpc::terminal::TermId;
use lapce_rpc::{self, Call, RequestId, RpcError, RpcObject};
use lsp_types::{Position, Range, TextDocumentItem, Url};
use parking_lot::Mutex;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::{collections::HashSet, io::BufRead};
use xi_rope::Rope;

const OPEN_FILE_EVENT_TOKEN: WatchToken = WatchToken(1);
const WORKSPACE_EVENT_TOKEN: WatchToken = WatchToken(2);

pub struct NewDispatcher {
    workspace: Option<PathBuf>,
    pub proxy_rpc: ProxyRpcHandler,
    core_rpc: CoreRpcHandler,
    catalog_rpc: PluginCatalogRpcHandler,
    buffers: HashMap<PathBuf, Buffer>,
    #[allow(deprecated)]
    terminals: HashMap<TermId, mio::channel::Sender<Msg>>,
    file_watcher: FileWatcher,
}

impl ProxyHandler for NewDispatcher {
    fn handle_notification(&mut self, rpc: CoreProxyNotification) {
        use CoreProxyNotification::*;
        match rpc {
            Initialize {
                workspace,
                plugin_configurations,
            } => {
                self.workspace = workspace;
                self.file_watcher.notify(FileWatchNotifer::new(
                    self.workspace.clone(),
                    self.core_rpc.clone(),
                    self.proxy_rpc.clone(),
                ));
                if let Some(workspace) = self.workspace.as_ref() {
                    self.file_watcher
                        .watch(workspace, true, WORKSPACE_EVENT_TOKEN);
                }

                let plugin_rpc = self.catalog_rpc.clone();
                let workspace = self.workspace.clone();
                thread::spawn(move || {
                    let mut plugin = NewPluginCatalog::new(
                        workspace,
                        plugin_configurations,
                        plugin_rpc.clone(),
                    );
                    plugin_rpc.mainloop(&mut plugin);
                });
            }
            OpenFileChanged { path } => {
                if let Some(buffer) = self.buffers.get(&path) {
                    if get_mod_time(&buffer.path) == buffer.mod_time {
                        return;
                    }
                    if let Ok(content) = load_file(&buffer.path) {
                        self.core_rpc.open_file_changed(path, content);
                    }
                }
            }
            Completion {
                request_id,
                path,
                input,
                position,
            } => {
                self.catalog_rpc
                    .completion(request_id, &path, input, position);
            }
            Shutdown {} => {
                self.catalog_rpc.shutdown();
                for (_, sender) in self.terminals.iter() {
                    #[allow(deprecated)]
                    let _ = sender.send(Msg::Shutdown);
                }
            }
            Update { path, delta, rev } => {
                let buffer = self.buffers.get_mut(&path).unwrap();
                let old_text = buffer.rope.clone();
                buffer.update(&delta, rev);
                self.catalog_rpc.did_change_text_document(
                    &path,
                    rev,
                    delta,
                    old_text,
                    buffer.rope.clone(),
                );
            }
            NewTerminal {
                term_id,
                cwd,
                shell,
            } => {
                let mut terminal = Terminal::new(term_id, cwd, shell, 50, 10);
                let tx = terminal.tx.clone();
                self.terminals.insert(term_id, tx);
                let rpc = self.core_rpc.clone();
                thread::spawn(move || {
                    terminal.run(rpc);
                });
            }
            TerminalWrite { term_id, content } => {
                if let Some(tx) = self.terminals.get(&term_id) {
                    #[allow(deprecated)]
                    let _ = tx.send(Msg::Input(content.into_bytes().into()));
                }
            }
            TerminalResize {
                term_id,
                width,
                height,
            } => {
                if let Some(tx) = self.terminals.get(&term_id) {
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
            TerminalClose { term_id } => {
                if let Some(tx) = self.terminals.remove(&term_id) {
                    #[allow(deprecated)]
                    let _ = tx.send(Msg::Shutdown);
                }
            }
            InstallPlugin { plugin } => {
                let catalog_rpc = self.catalog_rpc.clone();
                thread::spawn(move || {
                    let _ = install_plugin(catalog_rpc, plugin);
                });
            }
            DisablePlugin { plugin } => todo!(),
            EnablePlugin { plugin } => todo!(),
            RemovePlugin { plugin } => {
                let catalog_rpc = self.catalog_rpc.clone();
                thread::spawn(move || {
                    let _ = remove_plugin(catalog_rpc, plugin);
                });
            }
            GitCommit { message, diffs } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_commit(workspace, &message, diffs) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitCheckout { branch } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_checkout(workspace, &branch) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitDiscardFilesChanges { files } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_discard_files_changes(
                        workspace,
                        files.iter().map(AsRef::as_ref),
                    ) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitDiscardWorkspaceChanges {} => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_discard_workspace_changes(workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitInit {} => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_init(workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
        }
    }

    fn handle_request(&mut self, id: RequestId, rpc: CoreProxyRequest) {
        use CoreProxyRequest::*;
        match rpc {
            NewBuffer { buffer_id, path } => {
                let buffer = Buffer::new(buffer_id, path.clone());
                let content = buffer.rope.to_string();
                self.catalog_rpc.document_did_open(
                    &path,
                    buffer.language_id.to_string(),
                    buffer.rev as i32,
                    content.clone(),
                );
                self.file_watcher.watch(&path, false, OPEN_FILE_EVENT_TOKEN);
                self.buffers.insert(path, buffer);
                self.respond_rpc(
                    id,
                    Ok(CoreProxyResponse::NewBufferResponse { content }),
                );
            }
            BufferHead { path } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    let result = file_get_head(workspace, &path);
                    if let Ok((_blob_id, content)) = result {
                        Ok(CoreProxyResponse::BufferHeadResponse {
                            version: "head".to_string(),
                            content,
                        })
                    } else {
                        Err(RpcError {
                            code: 0,
                            message: "can't get file head".to_string(),
                        })
                    }
                } else {
                    Err(RpcError {
                        code: 0,
                        message: "no workspace set".to_string(),
                    })
                };
                self.respond_rpc(id, result);
            }
            GetCompletion {
                request_id,
                buffer_id,
                position,
            } => {}
            GlobalSearch { pattern } => {
                let workspace = self.workspace.clone();
                let proxy_rpc = self.proxy_rpc.clone();
                thread::spawn(move || {
                    let result = if let Some(workspace) = workspace.as_ref() {
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
                                                    lnum as usize,
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
                        Ok(CoreProxyResponse::GlobalSearchResponse { matches })
                    } else {
                        Err(RpcError {
                            code: 0,
                            message: "no workspace set".to_string(),
                        })
                    };
                    proxy_rpc.handle_response(id, result);
                });
            }
            CompletionResolve {
                plugin_id,
                completion_item,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.completion_resolve(
                    plugin_id,
                    *completion_item,
                    move |result| {
                        let result = result.map(|item| {
                            CoreProxyResponse::CompletionResolveResponse {
                                item: Box::new(item),
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetHover {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.hover(&path, position, move |_, result| {
                    let result = result.map(|hover| {
                        CoreProxyResponse::HoverResponse { request_id, hover }
                    });
                    proxy_rpc.handle_response(id, result);
                });
            }
            GetSignature { .. } => todo!(),
            GetReferences { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_references(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|references| {
                            CoreProxyResponse::GetReferencesResponse { references }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetDefinition {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_definition(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|definition| {
                            CoreProxyResponse::GetDefinitionResponse {
                                request_id,
                                definition,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetTypeDefinition {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_type_definition(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|definition| {
                            CoreProxyResponse::GetTypeDefinition {
                                request_id,
                                definition,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetInlayHints { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                let buffer = self.buffers.get(&path).unwrap();
                let range = Range {
                    start: Position::new(0, 0),
                    end: buffer.offset_to_position(buffer.len()).unwrap(),
                };
                self.catalog_rpc
                    .get_inlay_hints(&path, range, move |_, result| {
                        let result = result
                            .map(|hints| CoreProxyResponse::GetInlayHints { hints });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetSemanticTokens { path } => {
                let buffer = self.buffers.get(&path).unwrap();
                let text = buffer.rope.clone();
                let rev = buffer.rev;
                let len = buffer.len();
                let local_path = path.clone();
                let proxy_rpc = self.proxy_rpc.clone();
                let catalog_rpc = self.catalog_rpc.clone();

                let handle_tokens =
                    move |result: Result<Vec<LineStyle>, RpcError>| match result {
                        Ok(styles) => {
                            proxy_rpc.handle_response(
                                id,
                                Ok(CoreProxyResponse::GetSemanticTokens {
                                    styles: SemanticStyles {
                                        rev,
                                        path: local_path,
                                        styles,
                                        len,
                                    },
                                }),
                            );
                        }
                        Err(e) => {
                            proxy_rpc.handle_response(id, Err(e));
                        }
                    };

                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_semantic_tokens(
                    &path,
                    move |plugin_id, result| match result {
                        Ok(result) => {
                            catalog_rpc.format_semantic_tokens(
                                plugin_id,
                                result,
                                text,
                                Box::new(handle_tokens),
                            );
                        }
                        Err(e) => {
                            proxy_rpc.handle_response(id, Err(e));
                        }
                    },
                );
            }
            GetCodeActions { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_code_actions(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|resp| {
                            CoreProxyResponse::GetCodeActionsResponse { resp }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetDocumentSymbols { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_document_symbols(&path, move |_, result| {
                        let result = result.map(|resp| {
                            CoreProxyResponse::GetDocumentSymbols { resp }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetWorkspaceSymbols { query } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_workspace_symbols(query, move |_, result| {
                        let result = result.map(|symbols| {
                            CoreProxyResponse::GetWorkspaceSymbols { symbols }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetDocumentFormatting { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_document_formatting(&path, move |_, result| {
                        let result = result.map(|edits| {
                            CoreProxyResponse::GetDocumentFormatting { edits }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetFiles { .. } => {
                let workspace = self.workspace.clone();
                let proxy_rpc = self.proxy_rpc.clone();
                thread::spawn(move || {
                    let result = if let Some(workspace) = workspace {
                        let mut items = Vec::new();
                        for path in ignore::Walk::new(workspace).flatten() {
                            if let Some(file_type) = path.file_type() {
                                if file_type.is_file() {
                                    items.push(path.into_path());
                                }
                            }
                        }
                        Ok(CoreProxyResponse::GetFilesResponse { items })
                    } else {
                        Err(RpcError {
                            code: 0,
                            message: "no workspace set".to_string(),
                        })
                    };
                    proxy_rpc.handle_response(id, result);
                });
            }
            GetOpenFilesContent {} => {
                let items = self
                    .buffers
                    .iter()
                    .map(|(path, buffer)| TextDocumentItem {
                        uri: Url::from_file_path(path).unwrap(),
                        language_id: buffer.language_id.to_string(),
                        version: buffer.rev as i32,
                        text: buffer.get_document(),
                    })
                    .collect();
                let resp = CoreProxyResponse::GetOpenFilesContentResponse { items };
                self.proxy_rpc.handle_response(id, Ok(resp));
            }
            ReadDir { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
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

                            CoreProxyResponse::ReadDirResponse { items }
                        })
                        .map_err(|e| RpcError {
                            code: 0,
                            message: e.to_string(),
                        });
                    proxy_rpc.handle_response(id, result);
                });
            }
            Save { rev, path } => {
                let buffer = self.buffers.get_mut(&path).unwrap();
                let result = buffer
                    .save(rev)
                    .map(|_r| {
                        self.catalog_rpc
                            .did_save_text_document(&path, buffer.rope.clone());
                        CoreProxyResponse::SaveResponse {}
                    })
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            SaveBufferAs {
                buffer_id,
                path,
                rev,
                content,
            } => {
                let mut buffer = Buffer::new(buffer_id, path);
                buffer.rope = Rope::from(content);
                buffer.rev = rev;
                let result = buffer
                    .save(rev)
                    .map(|_| CoreProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            CreateFile { path } => {
                let result = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map(|_| CoreProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            CreateDirectory { path } => {
                let result = std::fs::create_dir(path)
                    .map(|_| CoreProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            TrashPath { path } => {
                let result = trash::delete(path)
                    .map(|_| CoreProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            RenamePath { from, to } => {
                // We first check if the destination already exists, because rename can overwrite it
                // and that's not the default behavior we want for when a user renames a document.
                let result = if to.exists() {
                    Err(RpcError {
                        code: 0,
                        message: format!("{:?} already exists", to),
                    })
                } else {
                    std::fs::rename(from, to)
                        .map(|_| CoreProxyResponse::Success {})
                        .map_err(|e| RpcError {
                            code: 0,
                            message: e.to_string(),
                        })
                };
                self.respond_rpc(id, result);
            }
        }
    }
}

impl NewDispatcher {
    pub fn new(core_rpc: CoreRpcHandler, proxy_rpc: ProxyRpcHandler) -> Self {
        let plugin_rpc =
            PluginCatalogRpcHandler::new(core_rpc.clone(), proxy_rpc.clone());

        let file_watcher = FileWatcher::new();

        Self {
            workspace: None,
            proxy_rpc,
            core_rpc,
            catalog_rpc: plugin_rpc,
            buffers: HashMap::new(),
            terminals: HashMap::new(),
            file_watcher,
        }
    }

    fn respond_rpc(
        &self,
        id: RequestId,
        result: Result<CoreProxyResponse, RpcError>,
    ) {
        self.proxy_rpc.handle_response(id, result);
    }
}

struct FileWatchNotifer {
    core_rpc: CoreRpcHandler,
    proxy_rpc: ProxyRpcHandler,
    workspace: Option<PathBuf>,
    workspace_fs_change_handler: Arc<Mutex<Option<Sender<bool>>>>,
    last_diff: Arc<Mutex<DiffInfo>>,
}

impl Notify for FileWatchNotifer {
    fn notify(&self, events: Vec<(WatchToken, notify::Event)>) {
        self.handle_fs_events(events);
    }
}

impl FileWatchNotifer {
    fn new(
        workspace: Option<PathBuf>,
        core_rpc: CoreRpcHandler,
        proxy_rpc: ProxyRpcHandler,
    ) -> Self {
        let notifier = Self {
            workspace,
            core_rpc,
            proxy_rpc,
            workspace_fs_change_handler: Arc::new(Mutex::new(None)),
            last_diff: Arc::new(Mutex::new(DiffInfo::default())),
        };

        if let Some(workspace) = notifier.workspace.clone() {
            let core_rpc = notifier.core_rpc.clone();
            let last_diff = notifier.last_diff.clone();
            thread::spawn(move || {
                if let Some(diff) = git_diff_new(&workspace) {
                    core_rpc.diff_info(diff.clone());
                    *last_diff.lock() = diff;
                }
            });
        }

        notifier
    }

    fn handle_fs_events(&self, events: Vec<(WatchToken, notify::Event)>) {
        for (token, event) in events {
            match token {
                OPEN_FILE_EVENT_TOKEN => self.handle_open_file_fs_event(event),
                WORKSPACE_EVENT_TOKEN => self.handle_workspace_fs_event(event),
                _ => {}
            }
        }
    }

    fn handle_open_file_fs_event(&self, event: notify::Event) {
        if event.kind.is_modify() {
            for path in event.paths {
                self.proxy_rpc
                    .notification(CoreProxyNotification::OpenFileChanged { path });
            }
        }
    }

    fn handle_workspace_fs_event(&self, event: notify::Event) {
        let explorer_change = match &event.kind {
            notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => true,
            notify::EventKind::Modify(_) => false,
            _ => return,
        };

        let mut handler = self.workspace_fs_change_handler.lock();
        if let Some(sender) = handler.as_mut() {
            if explorer_change {
                // only send the value if we need to update file explorer as well
                let _ = sender.send(explorer_change);
            }
            return;
        }
        let (sender, receiver) = crossbeam_channel::unbounded();
        if explorer_change {
            // only send the value if we need to update file explorer as well
            let _ = sender.send(explorer_change);
        }

        let local_handler = self.workspace_fs_change_handler.clone();
        let core_rpc = self.core_rpc.clone();
        let workspace = self.workspace.clone().unwrap();
        let last_diff = self.last_diff.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));

            {
                local_handler.lock().take();
            }

            let mut explorer_change = false;
            for e in receiver {
                if e {
                    explorer_change = true;
                    break;
                }
            }
            if explorer_change {
                core_rpc.workspace_file_change();
            }
            if let Some(diff) = git_diff_new(&workspace) {
                let mut last_diff = last_diff.lock();
                if diff != *last_diff {
                    core_rpc.diff_info(diff.clone());
                    *last_diff = diff;
                }
            }
        });
        *handler = Some(sender);
    }
}

#[derive(Clone)]
pub struct Dispatcher {
    pub sender: Arc<Sender<Value>>,
    pub workspace: Arc<Mutex<Option<PathBuf>>>,
    pub buffers: Arc<Mutex<HashMap<BufferId, Buffer>>>,

    #[allow(deprecated)]
    pub terminals: Arc<Mutex<HashMap<TermId, mio::channel::Sender<Msg>>>>,

    open_files: Arc<Mutex<HashMap<String, BufferId>>>,
    plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
    pub file_watcher: Arc<Mutex<Option<FileWatcher>>>,
    workspace_fs_change_handler: Arc<Mutex<Option<Sender<bool>>>>,
    last_diff: Arc<Mutex<DiffInfo>>,
}

impl Notify for Dispatcher {
    fn notify(&self, events: Vec<(WatchToken, notify::Event)>) {
        self.handle_fs_events();
    }
}

impl Dispatcher {
    pub fn new(sender: Sender<Value>) -> Dispatcher {
        let plugins = PluginCatalog::new();
        let dispatcher = Dispatcher {
            sender: Arc::new(sender),
            workspace: Arc::new(Mutex::new(None)),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            terminals: Arc::new(Mutex::new(HashMap::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            lsp: Arc::new(Mutex::new(LspCatalog::new())),
            file_watcher: Arc::new(Mutex::new(None)),
            last_diff: Arc::new(Mutex::new(DiffInfo::default())),
            workspace_fs_change_handler: Arc::new(Mutex::new(None)),
        };
        *dispatcher.file_watcher.lock() = Some(FileWatcher::new());
        dispatcher.lsp.lock().dispatcher = Some(dispatcher.clone());
        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            local_dispatcher.plugins.lock().reload();
            let plugins = { local_dispatcher.plugins.lock().items.clone() };
            let delay = std::time::Duration::from_millis(50);
            std::thread::sleep(delay);
            local_dispatcher.send_notification(
                "installed_plugins",
                json!({
                    "plugins": plugins,
                }),
            );
            // local_dispatcher
            //     .plugins
            //     .lock()
            //     .start_all(local_dispatcher.clone());
        });
        let local_dispatcher = dispatcher.clone();
        thread::spawn(move || {
            if let Some(path) = plugins_directory() {
                local_dispatcher.send_notification(
                    "home_dir",
                    json!({
                        "path": path,
                    }),
                );
            }
        });

        dispatcher.send_notification("proxy_connected", json!({}));

        dispatcher
    }

    pub fn mainloop(&self, receiver: Receiver<Value>) -> Result<()> {
        for msg in receiver {
            let rpc: RpcObject = msg.into();
            if rpc.is_response() {
            } else {
                match rpc.into_rpc::<CoreProxyNotification, CoreProxyRequest>() {
                    Ok(Call::Request(id, request)) => {
                        self.handle_request(id, request);
                    }
                    Ok(Call::Notification(notification)) => {
                        if let CoreProxyNotification::Shutdown {} = &notification {
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
        log::debug!(target: "lapce_proxy::dispatch::send_notification", "{method} : {params}");
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
            let explorer_change = match &event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Remove(_)
                | notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                    true
                }
                notify::EventKind::Modify(_) => false,
                _ => return,
            };

            let mut handler = self.workspace_fs_change_handler.lock();
            if let Some(sender) = handler.as_mut() {
                if explorer_change {
                    // only send the value if we need to update file explorer as well
                    let _ = sender.send(explorer_change);
                }
                return;
            }
            let (sender, receiver) = crossbeam_channel::unbounded();
            if explorer_change {
                // only send the value if we need to update file explorer as well
                let _ = sender.send(explorer_change);
            }

            let local_handler = self.workspace_fs_change_handler.clone();
            let local_dispatcher = self.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(1));

                {
                    local_handler.lock().take();
                }

                let mut explorer_change = false;
                for e in receiver {
                    if e {
                        explorer_change = true;
                        break;
                    }
                }
                if explorer_change {
                    local_dispatcher.send_rpc_notification(
                        CoreNotification::WorkspaceFileChange {},
                    );
                }
                if let Some(diff) = git_diff_new(&workspace) {
                    let mut last_diff = local_dispatcher.last_diff.lock();
                    if diff != *last_diff {
                        local_dispatcher.send_notification(
                            "diff_info",
                            json!({
                                "diff": diff,
                            }),
                        );
                        *last_diff = diff;
                    }
                }
            });
            *handler = Some(sender);
        }
    }

    fn handle_notification(&self, rpc: CoreProxyNotification) {
        use CoreProxyNotification::*;
        match rpc {
            Completion { .. } => {}
            OpenFileChanged { .. } => {}
            Initialize { workspace, .. } => {
                *self.workspace.lock() = workspace.clone();
                if let Some(workspace) = workspace {
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
            }
            Shutdown {} => {}
            Update { path, delta, rev } => {}
            InstallPlugin { plugin } => {
                let catalog = self.plugins.clone();
                let dispatcher = self.clone();
                std::thread::spawn(move || {
                    // if let Err(e) =
                    //     catalog.lock().install_plugin(dispatcher.clone(), plugin)
                    // {
                    //     eprintln!("install plugin error {e}");
                    // }
                    // let plugins = { dispatcher.plugins.lock().items.clone() };
                    // dispatcher.send_notification(
                    //     "installed_plugins",
                    //     json!({
                    //         "plugins": plugins,
                    //     }),
                    // );
                });
            }
            DisablePlugin { plugin } => {}
            EnablePlugin { plugin } => {}
            RemovePlugin { plugin } => {}
            // DisablePlugin { plugin } => {
            //     let catalog = self.plugins.clone();
            //     let dispatcher = self.clone();
            //     std::thread::spawn(move || {
            //         if let Err(e) = catalog
            //             .lock()
            //             .disable_plugin(dispatcher.clone(), plugin.clone())
            //         {
            //             eprintln!("disable plugin error {e}");
            //         }
            //         let plugins = { dispatcher.plugins.lock().disabled.clone() };
            //         dispatcher.send_notification(
            //             "disabled_plugins",
            //             json!({
            //                 "plugins": plugins,
            //             }),
            //         )
            //     });
            // }
            // EnablePlugin { plugin } => {
            //     let catalog = self.plugins.clone();
            //     let dispatcher = self.clone();
            //     std::thread::spawn(move || {
            //         if let Err(e) = catalog
            //             .lock()
            //             .enable_plugin(dispatcher.clone(), plugin.clone())
            //         {
            //             eprintln!("enable plugin error {e}");
            //         }
            //         let plugins = { dispatcher.plugins.lock().disabled.clone() };
            //         dispatcher.send_notification(
            //             "disabled_plugins",
            //             json!({
            //                 "plugins": plugins,
            //             }),
            //         )
            //     });
            // }
            // RemovePlugin { plugin } => {
            //     let catalog = self.plugins.clone();
            //     let dispatcher = self.clone();
            //     std::thread::spawn(move || {
            //         if let Err(e) = catalog
            //             .lock()
            //             .remove_plugin(dispatcher.clone(), plugin.clone())
            //         {
            //             eprintln!("remove plugin error {e}");
            //         }
            //         let plugins = { dispatcher.plugins.lock().items.clone() };
            //         dispatcher.send_notification(
            //             "installed_plugins",
            //             json!({
            //                 "plugins": plugins,
            //             }),
            //         );
            //         let disabled_plugins =
            //             { dispatcher.plugins.lock().disabled.clone() };
            //         dispatcher.send_notification(
            //             "disabled_plugins",
            //             json!({
            //                 "plugins": disabled_plugins,
            //             }),
            //         );
            //     });
            // }
            NewTerminal {
                term_id,
                cwd,
                shell,
            } => {}
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
            GitInit {} => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    match git_init(&workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
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
            GitDiscardFilesChanges { files } => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    match git_discard_files_changes(
                        &workspace,
                        files.iter().map(AsRef::as_ref),
                    ) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitDiscardWorkspaceChanges {} => {
                if let Some(workspace) = self.workspace.lock().clone() {
                    match git_discard_workspace_changes(&workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
        }
    }

    fn handle_request(&self, id: RequestId, rpc: CoreProxyRequest) {
        use CoreProxyRequest::*;
        match rpc {
            GetOpenFilesContent {} => {}
            NewBuffer { buffer_id, path } => {
                self.file_watcher.lock().as_mut().unwrap().watch(
                    &path,
                    false,
                    OPEN_FILE_EVENT_TOKEN,
                );
                self.open_files
                    .lock()
                    .insert(path.to_str().unwrap().to_string(), buffer_id);
                let buffer = Buffer::new(buffer_id, path);
                let content = buffer.rope.to_string();
                self.buffers.lock().insert(buffer_id, buffer);
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
                plugin_id,
                completion_item,
            } => {
                // let buffers = self.buffers.lock();
                // let buffer = buffers.get(&buffer_id).unwrap();
                // self.lsp
                //     .lock()
                //     .completion_resolve(id, buffer, &completion_item);
            }
            GetHover {
                path,
                position,
                request_id,
            } => {
                // let buffers = self.buffers.lock();
                // let buffer = buffers.get(&buffer_id).unwrap();
                // self.lsp.lock().get_hover(id, request_id, buffer, position);
            }
            GetSignature {
                buffer_id,
                position,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp.lock().get_signature(id, buffer, position);
            }
            GetReferences { .. } => {}
            GetDefinition { .. } => {}
            GetTypeDefinition { .. } => {}
            GetInlayHints { .. } => {}
            GetSemanticTokens { .. } => {}
            GetCodeActions { .. } => {}
            GetDocumentSymbols { .. } => {}
            GetWorkspaceSymbols { .. } => {}
            GetDocumentFormatting { .. } => {}
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
            Save { rev, path } => {
                // if let Some(workspace) = self.workspace.lock().as_ref() {
                //     let mut buffers = self.buffers.lock();
                //     let buffer = buffers.get_mut(&buffer_id).unwrap();
                //     let resp = buffer.save(rev).map(|_r| json!({}));
                //     self.lsp.lock().save_buffer(buffer, workspace);
                //     self.respond(id, resp);
                // }
            }
            SaveBufferAs { .. } => {
                // let mut buffer = Buffer::new(buffer_id, path.clone());
                // buffer.rope = Rope::from(content);
                // buffer.rev = rev;
                // let resp = buffer.save(rev).map(|_r| json!({}));
                // if resp.is_ok() {
                //     self.buffers.lock().insert(buffer_id, buffer);
                //     self.open_files
                //         .lock()
                //         .insert(path.to_str().unwrap().to_string(), buffer_id);
                // }
                // self.respond(id, resp);
            }
            CreateFile { path } => {
                // Create the file, specifically choosing to error if it already exists
                // We also throw away the file object because we only want to create it,
                // and return any errors that occur
                let resp = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map(|_| json!({}))
                    .map_err(anyhow::Error::from);
                self.respond(id, resp);
            }
            CreateDirectory { path } => {
                let resp = std::fs::create_dir(path)
                    .map(|_| json!({}))
                    .map_err(anyhow::Error::from);
                self.respond(id, resp);
            }
            TrashPath { path } => {
                let resp = trash::delete(path)
                    .map(|_| json!({}))
                    .map_err(anyhow::Error::from);
                self.respond(id, resp);
            }
            RenamePath { from, to } => {
                // We first check if the destination already exists, because rename can overwrite it
                // and that's not the default behavior we want for when a user renames a document.
                if to.exists() {
                    self.respond(id, Err(anyhow!("{:?} already exists", to)));
                } else {
                    let resp = std::fs::rename(from, to)
                        .map(|_| json!({}))
                        .map_err(anyhow::Error::from);
                    self.respond(id, resp);
                }
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

fn git_init(workspace_path: &Path) -> Result<()> {
    Repository::init(workspace_path)?;
    Ok(())
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

fn git_discard_files_changes<'a>(
    workspace_path: &Path,
    files: impl Iterator<Item = &'a Path>,
) -> Result<()> {
    let repo = Repository::open(workspace_path)?;

    let mut checkout_b = CheckoutBuilder::new();
    checkout_b.update_only(true).force();

    let mut had_path = false;
    for path in files {
        // Remove the workspace path so it is relative to the folder
        if let Ok(path) = path.strip_prefix(workspace_path) {
            had_path = true;
            checkout_b.path(path);
        }
    }

    if !had_path {
        // If there we no paths then we do nothing
        // because the default behavior of checkout builder is to select all files
        // if it is not given a path
        return Ok(());
    }

    repo.checkout_index(None, Some(&mut checkout_b))?;

    Ok(())
}

fn git_discard_workspace_changes(workspace_path: &Path) -> Result<()> {
    let repo = Repository::open(workspace_path)?;
    let mut checkout_b = CheckoutBuilder::new();
    checkout_b.force();

    repo.checkout_index(None, Some(&mut checkout_b))?;

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
