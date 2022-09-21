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
    plugin::PluginId, proxy::ProxyResponse, style::LineStyle, RpcError,
};
use lsp_types::{
    notification::DidOpenTextDocument, DidOpenTextDocumentParams, SemanticTokens,
    TextDocumentIdentifier, VersionedTextDocumentIdentifier,
};
use parking_lot::Mutex;
use psp_types::Notification;
use serde_json::Value;
use xi_rope::{Rope, RopeDelta};

use crate::plugin::{install_volt, wasi::start_volt_from_info};

use super::{
    psp::{ClonableCallback, PluginServerRpc, PluginServerRpcHandler, RpcCallback},
    wasi::load_all_volts,
    PluginCatalogNotification, PluginCatalogRpcHandler,
};

pub struct PluginCatalog {
    workspace: Option<PathBuf>,
    plugin_rpc: PluginCatalogRpcHandler,
    new_plugins: HashMap<PluginId, PluginServerRpcHandler>,
    plugin_configurations: HashMap<String, serde_json::Value>,
}

impl PluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        disabled_volts: Vec<String>,
        plugin_configurations: HashMap<String, serde_json::Value>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        let plugin = Self {
            workspace: workspace.clone(),
            plugin_rpc: plugin_rpc.clone(),
            plugin_configurations: plugin_configurations.clone(),
            new_plugins: HashMap::new(),
        };

        thread::spawn(move || {
            load_all_volts(
                workspace,
                plugin_rpc,
                disabled_volts,
                plugin_configurations,
            );
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
            if let Some(plugin) = self.new_plugins.get(&plugin_id) {
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
            if self.new_plugins.is_empty() {
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
                request_sent.fetch_add(self.new_plugins.len(), Ordering::Relaxed);
            }
        }
        for (plugin_id, plugin) in self.new_plugins.iter() {
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
        for (_, plugin) in self.new_plugins.iter() {
            plugin.server_notification(
                method,
                params.clone(),
                language_id.clone(),
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
        for (_, plugin) in self.new_plugins.iter() {
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
        for (_, plugin) in self.new_plugins.iter() {
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
        if let Some(plugin) = self.new_plugins.get(&plugin_id) {
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
                self.new_plugins.insert(plugin.plugin_id, plugin);
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
            StopVolt(volt) => {
                let volt_id = volt.id();
                let ids: Vec<PluginId> = self.new_plugins.keys().cloned().collect();
                for id in ids {
                    if self.new_plugins.get(&id).unwrap().volt_id == volt_id {
                        let plugin = self.new_plugins.remove(&id).unwrap();
                        plugin.shutdown();
                    }
                }
            }
            StartVolt(volt) => {
                let volt_id = volt.id();
                for (_, volt) in self.new_plugins.iter() {
                    if volt.volt_id == volt_id {
                        return;
                    }
                }
                let workspace = self.workspace.clone();
                let catalog_rpc = self.plugin_rpc.clone();
                let configurations =
                    self.plugin_configurations.get(&volt.name).cloned();
                thread::spawn(move || {
                    let _ = start_volt_from_info(
                        workspace,
                        configurations,
                        catalog_rpc,
                        volt,
                    );
                });
            }
            Shutdown => {
                for (_, plugin) in self.new_plugins.iter() {
                    plugin.shutdown();
                }
            }
        }
    }
}
