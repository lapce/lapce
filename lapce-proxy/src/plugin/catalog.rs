use std::{path::PathBuf, sync::Arc, thread};

use crossbeam_channel::Sender;
use dyn_clone::DynClone;
use lapce_rpc::RpcError;
use lsp_types::VersionedTextDocumentIdentifier;
use parking_lot::Mutex;
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
    new_plugins: Vec<PluginServerRpcHandler>,
}

impl NewPluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        let plugin = Self {
            plugin_rpc: plugin_rpc.clone(),
            new_plugins: Vec::new(),
        };

        thread::spawn(move || {
            load_all_plugins(workspace, plugin_rpc);
        });

        plugin
    }

    pub fn handle_server_request(
        &mut self,
        method: &'static str,
        params: Value,
        f: Box<dyn ClonableCallback>,
    ) {
        for plugin in self.new_plugins.iter() {
            let f = dyn_clone::clone_box(&*f);
            plugin.server_request_async(
                method,
                params.clone(),
                true,
                move |result| {
                    f(result);
                },
            );
        }
    }

    pub fn handle_server_notification(
        &mut self,
        method: &'static str,
        params: Value,
    ) {
        for plugin in self.new_plugins.iter() {
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
        for plugin in self.new_plugins.iter() {
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
                self.new_plugins.push(plugin);
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
