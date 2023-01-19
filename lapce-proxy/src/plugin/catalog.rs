use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use lapce_rpc::{
    dap_types::DapId,
    plugin::{PluginId, VoltID, VoltMetadata},
    proxy::ProxyResponse,
    style::LineStyle,
    RpcError,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    notification::DidOpenTextDocument, DidOpenTextDocumentParams, SemanticTokens,
    TextDocumentIdentifier, TextDocumentItem, VersionedTextDocumentIdentifier,
};
use parking_lot::Mutex;
use psp_types::Notification;
use serde_json::Value;

use super::{
    dap::{DapClient, DapRpcHandler},
    psp::{ClonableCallback, PluginServerRpc, PluginServerRpcHandler, RpcCallback},
    wasi::{load_all_volts, start_volt},
    PluginCatalogNotification, PluginCatalogRpcHandler,
};
use crate::plugin::{install_volt, wasi::enable_volt};

pub struct PluginCatalog {
    workspace: Option<PathBuf>,
    plugin_rpc: PluginCatalogRpcHandler,
    plugins: HashMap<PluginId, PluginServerRpcHandler>,
    daps: HashMap<DapId, DapRpcHandler>,
    plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
    unactivated_volts: HashMap<VoltID, VoltMetadata>,
    open_files: HashMap<PathBuf, String>,
}

impl PluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        disabled_volts: Vec<VoltID>,
        plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        {
            let workspace = workspace.clone();
            let core_rpc = plugin_rpc.core_rpc.clone();
            thread::spawn(move || {
                // let mut dap = DapClient::new(
                //     workspace,
                //     Url::parse("file:///opt/homebrew/opt/llvm@14/bin/lldb-vscode")
                //         .unwrap(),
                //     Vec::new(),
                //     core_rpc,
                // )
                // .unwrap();
                // let dap_rpc = dap.dap_rpc.clone();
                // // let _ = dap.initialize();

                // {
                //     let dap_rpc = dap_rpc.clone();
                //     thread::spawn(move || {
                //         dap_rpc.mainloop(&mut dap);
                //     });
                // }

                // std::thread::sleep(std::time::Duration::from_secs(1));
                // let _ = dap_rpc.launch(serde_json::json!({
                //     "program": "/Users/dz/lapce/target/debug/lapce",
                //     "args": vec!["--wait", "--new"],
                //     "runInTerminal": true,
                // }));
            });
        }

        let plugin = Self {
            workspace,
            plugin_rpc: plugin_rpc.clone(),
            plugin_configurations,
            plugins: HashMap::new(),
            daps: HashMap::new(),
            unactivated_volts: HashMap::new(),
            open_files: HashMap::new(),
        };

        thread::spawn(move || {
            load_all_volts(plugin_rpc, disabled_volts);
        });

        plugin
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_server_request(
        &mut self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
        f: Box<dyn ClonableCallback>,
    ) {
        if let Some(plugin_id) = plugin_id {
            if let Some(plugin) = self.plugins.get(&plugin_id) {
                plugin.server_request_async(
                    method,
                    params,
                    language_id,
                    path,
                    true,
                    move |result| {
                        f(plugin_id, result);
                    },
                );
            } else {
                f(
                    plugin_id,
                    Err(RpcError {
                        code: 0,
                        message: "plugin doesn't exist".to_string(),
                    }),
                );
            }
            return;
        }

        if let Some(request_sent) = request_sent {
            // if there are no plugins installed the callback of the client is not called
            // so check if plugins list is empty
            if self.plugins.is_empty() {
                // Add a request
                request_sent.fetch_add(1, Ordering::Relaxed);

                // make a direct callback with an "error"
                f(
                    lapce_rpc::plugin::PluginId(0),
                    Err(RpcError {
                        code: 0,
                        message: "no available plugin could make a callback, because the plugins list is empty".to_string(),
                    }),
                );
                return;
            } else {
                request_sent.fetch_add(self.plugins.len(), Ordering::Relaxed);
            }
        }
        for (plugin_id, plugin) in self.plugins.iter() {
            let f = dyn_clone::clone_box(&*f);
            let plugin_id = *plugin_id;
            plugin.server_request_async(
                method,
                params.clone(),
                language_id.clone(),
                path.clone(),
                true,
                move |result| {
                    f(plugin_id, result);
                },
            );
        }
    }

    pub fn handle_server_notification(
        &mut self,
        method: &'static str,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
    ) {
        for (_, plugin) in self.plugins.iter() {
            plugin.server_notification(
                method,
                params.clone(),
                language_id.clone(),
                path.clone(),
                true,
            );
        }
    }

    fn start_unactivated_volts(&mut self, to_be_activated: Vec<VoltID>) {
        for id in to_be_activated.iter() {
            let workspace = self.workspace.clone();
            if let Some(meta) = self.unactivated_volts.remove(id) {
                let configurations =
                    self.plugin_configurations.get(&meta.name).cloned();
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    let _ = start_volt(workspace, configurations, plugin_rpc, meta);
                });
            }
        }
    }

    fn check_unactivated_volts(&mut self) {
        let to_be_activated: Vec<VoltID> = self
            .unactivated_volts
            .iter()
            .filter_map(|(id, meta)| {
                let contains = meta
                    .activation
                    .as_ref()
                    .and_then(|a| a.language.as_ref())
                    .map(|l| {
                        self.open_files
                            .iter()
                            .any(|(_, language_id)| l.contains(language_id))
                    })
                    .unwrap_or(false);
                if contains {
                    return Some(id.clone());
                }

                if let Some(workspace) = self.workspace.as_ref() {
                    if let Some(globs) = meta
                        .activation
                        .as_ref()
                        .and_then(|a| a.workspace_contains.as_ref())
                    {
                        let mut builder = globset::GlobSetBuilder::new();
                        for glob in globs {
                            if let Ok(glob) = globset::Glob::new(glob) {
                                builder.add(glob);
                            }
                        }
                        if let Ok(matcher) = builder.build() {
                            if !matcher.is_empty() {
                                for entry in walkdir::WalkDir::new(workspace)
                                    .into_iter()
                                    .flatten()
                                {
                                    if matcher.is_match(entry.path()) {
                                        return Some(id.clone());
                                    }
                                }
                            }
                        }
                    }
                }

                None
            })
            .collect();
        self.start_unactivated_volts(to_be_activated);
    }

    pub fn handle_did_open_text_document(&mut self, document: TextDocumentItem) {
        let language_id = document.language_id.clone();
        if let Ok(path) = document.uri.to_file_path() {
            self.open_files.insert(path, language_id.clone());
        }

        let to_be_activated: Vec<VoltID> = self
            .unactivated_volts
            .iter()
            .filter_map(|(id, meta)| {
                let contains = meta
                    .activation
                    .as_ref()
                    .and_then(|a| a.language.as_ref())
                    .map(|l| l.contains(&language_id))?;
                if contains {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        self.start_unactivated_volts(to_be_activated);

        let path = document.uri.to_file_path().ok();
        for (_, plugin) in self.plugins.iter() {
            plugin.server_notification(
                DidOpenTextDocument::METHOD,
                DidOpenTextDocumentParams {
                    text_document: document.clone(),
                },
                Some(language_id.clone()),
                path.clone(),
                true,
            );
        }
    }

    pub fn handle_did_save_text_document(
        &mut self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    ) {
        for (_, plugin) in self.plugins.iter() {
            plugin.handle_rpc(PluginServerRpc::DidSaveTextDocument {
                language_id: language_id.clone(),
                path: path.clone(),
                text_document: text_document.clone(),
                text: text.clone(),
            });
        }
    }

    pub fn handle_did_change_text_document(
        &mut self,
        language_id: String,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
    ) {
        let change = Arc::new(Mutex::new((None, None)));
        for (_, plugin) in self.plugins.iter() {
            plugin.handle_rpc(PluginServerRpc::DidChangeTextDocument {
                language_id: language_id.clone(),
                document: document.clone(),
                delta: delta.clone(),
                text: text.clone(),
                new_text: new_text.clone(),
                change: change.clone(),
            });
        }
    }

    pub fn format_semantic_tokens(
        &self,
        plugin_id: PluginId,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        if let Some(plugin) = self.plugins.get(&plugin_id) {
            plugin.handle_rpc(PluginServerRpc::FormatSemanticTokens {
                tokens,
                text,
                f,
            });
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn handle_notification(&mut self, notification: PluginCatalogNotification) {
        use PluginCatalogNotification::*;
        match notification {
            UnactivatedVolts(volts) => {
                for volt in volts {
                    let id = volt.id();
                    self.unactivated_volts.insert(id, volt);
                }
                self.check_unactivated_volts();
            }
            UpdatePluginConfigs(configs) => {
                self.plugin_configurations = configs;
            }
            PluginServerLoaded(plugin) => {
                // TODO: check if the server has did open registered
                if let Ok(ProxyResponse::GetOpenFilesContentResponse { items }) =
                    self.plugin_rpc.proxy_rpc.get_open_files_content()
                {
                    for item in items {
                        let language_id = Some(item.language_id.clone());
                        let path = item.uri.to_file_path().ok();
                        plugin.server_notification(
                            DidOpenTextDocument::METHOD,
                            DidOpenTextDocumentParams {
                                text_document: item,
                            },
                            language_id,
                            path,
                            true,
                        );
                    }
                }
                self.plugins.insert(plugin.plugin_id, plugin);
            }
            InstallVolt(volt) => {
                let workspace = self.workspace.clone();
                let configurations =
                    self.plugin_configurations.get(&volt.name).cloned();
                let catalog_rpc = self.plugin_rpc.clone();
                let _ = catalog_rpc.stop_volt(volt.clone());
                thread::spawn(move || {
                    let _ =
                        install_volt(catalog_rpc, workspace, configurations, volt);
                });
            }
            ReloadVolt(volt) => {
                let volt_id = volt.id();
                let ids: Vec<PluginId> = self.plugins.keys().cloned().collect();
                for id in ids {
                    if self.plugins.get(&id).unwrap().volt_id == volt_id {
                        let plugin = self.plugins.remove(&id).unwrap();
                        plugin.shutdown();
                    }
                }
                let _ = self.plugin_rpc.unactivated_volts(vec![volt]);
            }
            StopVolt(volt) => {
                let volt_id = volt.id();
                let ids: Vec<PluginId> = self.plugins.keys().cloned().collect();
                for id in ids {
                    if self.plugins.get(&id).unwrap().volt_id == volt_id {
                        let plugin = self.plugins.remove(&id).unwrap();
                        plugin.shutdown();
                    }
                }
            }
            EnableVolt(volt) => {
                let volt_id = volt.id();
                for (_, volt) in self.plugins.iter() {
                    if volt.volt_id == volt_id {
                        return;
                    }
                }
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    let _ = enable_volt(plugin_rpc, volt);
                });
            }
            DapLoaded(dap_rpc) => {
                self.daps.insert(dap_rpc.dap_id, dap_rpc);
            }
            DapStart { config } => {
                let workspace = self.workspace.clone();
                let core_rpc = self.plugin_rpc.core_rpc.clone();
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    if let Ok(dap_rpc) = DapClient::start(
                        "/opt/homebrew/opt/llvm@14/bin/lldb-vscode".to_string(),
                        Vec::new(),
                        workspace,
                        config.clone(),
                        core_rpc,
                    ) {
                        let _ = plugin_rpc.dap_loaded(dap_rpc.clone());

                        let _ = dap_rpc.launch(serde_json::json!({
                            "program": config.program,
                            "args": config.args,
                            "cwd": config.cwd,
                            "runInTerminal": true,
                        }));
                    }
                });
            }
            DapProcessId { dap_id, process_id } => {
                println!("dap process id {process_id}");
                if let Some(dap) = self.daps.get(&dap_id) {
                    let _ = dap.termain_process_tx.send(process_id);
                }
            }
            DapContinue { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    if let Ok(_) = dap.continue_thread(thread_id) {
                        self.plugin_rpc.core_rpc.dap_continued(dap_id);
                    }
                }
            }
            DapStop { dap_id } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    let _ = dap.disconnect();
                }
            }
            Shutdown => {
                for (_, plugin) in self.plugins.iter() {
                    plugin.shutdown();
                }
            }
        }
    }
}
