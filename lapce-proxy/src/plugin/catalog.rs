use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use crossbeam_channel::Sender;
use dyn_clone::DynClone;
use lapce_rpc::{plugin::PluginId, proxy::CoreProxyResponse, RpcError};
use lsp_types::{
    notification::DidOpenTextDocument, DidOpenTextDocumentParams, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};
use parking_lot::Mutex;
use psp_types::Notification;
use serde_json::Value;
use xi_rope::{Rope, RopeDelta};

use super::{
    lsp::NewLspClient,
    psp::{ClonableCallback, PluginServerRpc, PluginServerRpcHandler, RpcCallback},
    wasi::load_all_plugins,
    PluginCatalogNotification, PluginCatalogRpcHandler,
};

pub struct NewPluginCatalog {
    plugin_rpc: PluginCatalogRpcHandler,
    new_plugins: HashMap<PluginId, PluginServerRpcHandler>,
}

impl NewPluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        let plugin = Self {
            plugin_rpc: plugin_rpc.clone(),
            new_plugins: HashMap::new(),
        };

        thread::spawn(move || {
            load_all_plugins(workspace, plugin_rpc);
        });

        plugin
    }

    pub fn handle_server_request(
        &mut self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: Value,
        f: Box<dyn ClonableCallback>,
    ) {
        if let Some(plugin_id) = plugin_id {
            if let Some(plugin) = self.new_plugins.get(&plugin_id) {
                plugin.server_request_async(method, params, true, move |result| {
                    f(plugin_id, result);
                });
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
            request_sent.fetch_add(self.new_plugins.len(), Ordering::Relaxed);
        }
        for (plugin_id, plugin) in self.new_plugins.iter() {
            let f = dyn_clone::clone_box(&*f);
            let plugin_id = *plugin_id;
            plugin.server_request_async(
                method,
                params.clone(),
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
    ) {
        for (_, plugin) in self.new_plugins.iter() {
            plugin.server_notification(method, params.clone(), true);
        }
    }

    pub fn handle_did_change_text_document(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
    ) {
        let change = Arc::new(Mutex::new((None, None)));
        for (_, plugin) in self.new_plugins.iter() {
            plugin.handle_rpc(PluginServerRpc::DidChangeTextDocument {
                document: document.clone(),
                delta: delta.clone(),
                text: text.clone(),
                new_text: new_text.clone(),
                change: change.clone(),
            });
        }
    }

    pub fn handle_notification(&mut self, notification: PluginCatalogNotification) {
        use PluginCatalogNotification::*;
        match notification {
            PluginServerLoaded(plugin) => {
                eprintln!("plugin server loaded");
                // TODO: check if the server has did open registered
                if let Ok(CoreProxyResponse::GetOpenFilesContentResponse { items }) =
                    self.plugin_rpc.proxy_rpc.get_open_files_content()
                {
                    for item in items {
                        plugin.server_notification(
                            DidOpenTextDocument::METHOD,
                            DidOpenTextDocumentParams {
                                text_document: item,
                            },
                            true,
                        );
                    }
                }
                self.new_plugins.insert(plugin.plugin_id, plugin);
            } // NewPluginNotification::StartLspServer {
              //     workspace,
              //     plugin_id,
              //     exec_path,
              //     language_id,
              //     options,
              //     system_lsp,
              // } => {
              //     // let exec_path = if system_lsp.unwrap_or(false) {
              //     //     // System LSP should be handled by PATH during
              //     //     // process creation, so we forbid anything that
              //     //     // is not just an executable name
              //     //     match PathBuf::from(&exec_path).file_name() {
              //     //         Some(v) => v.to_str().unwrap().to_string(),
              //     //         None => return,
              //     //     }
              //     // } else {
              //     //     let plugin = self.plugins.get(&plugin_id).unwrap();
              //     //     plugin
              //     //         .env
              //     //         .desc
              //     //         .dir
              //     //         .as_ref()
              //     //         .unwrap()
              //     //         .join(&exec_path)
              //     //         .to_str()
              //     //         .unwrap()
              //     //         .to_string()
              //     // };
              //     let plugin_rpc = self.plugin_rpc.clone();
              //     thread::spawn(move || {
              //         NewLspClient::start(
              //             plugin_rpc,
              //             workspace,
              //             exec_path,
              //             Vec::new(),
              //         );
              //     });
              // }
              // NewPluginNotification::PluginServerNotification { method, params } => {
              //     for plugin in self.new_plugins.iter() {
              //         plugin.server_notification(method, params.clone());
              //     }
              // }
        }
    }
}
