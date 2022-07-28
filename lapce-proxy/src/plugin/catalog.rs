use std::thread;

use super::{
    lsp::NewLspClient, psp::PluginServerRpcHandler, wasi::load_all_plugins,
    NewPluginNotification, PluginRequest, PluginRpcHandler,
};

pub struct NewPluginCatalog {
    plugin_rpc: PluginRpcHandler,
    new_plugins: Vec<PluginServerRpcHandler>,
}

impl NewPluginCatalog {
    pub fn new(plugin_rpc: PluginRpcHandler) -> Self {
        let plugin = Self {
            plugin_rpc: plugin_rpc.clone(),
            new_plugins: Vec::new(),
        };

        thread::spawn(move || {
            load_all_plugins(plugin_rpc);
        });

        plugin
    }

    pub fn handle_notification(&mut self, rpc: NewPluginNotification) {
        match rpc {
            NewPluginNotification::PluginServerLoaded(plugin) => {
                self.new_plugins.push(plugin);
            }
            NewPluginNotification::StartLspServer {
                workspace,
                plugin_id,
                exec_path,
                language_id,
                options,
                system_lsp,
            } => {
                // let exec_path = if system_lsp.unwrap_or(false) {
                //     // System LSP should be handled by PATH during
                //     // process creation, so we forbid anything that
                //     // is not just an executable name
                //     match PathBuf::from(&exec_path).file_name() {
                //         Some(v) => v.to_str().unwrap().to_string(),
                //         None => return,
                //     }
                // } else {
                //     let plugin = self.plugins.get(&plugin_id).unwrap();
                //     plugin
                //         .env
                //         .desc
                //         .dir
                //         .as_ref()
                //         .unwrap()
                //         .join(&exec_path)
                //         .to_str()
                //         .unwrap()
                //         .to_string()
                // };
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    NewLspClient::start(
                        plugin_rpc,
                        workspace,
                        exec_path,
                        Vec::new(),
                    );
                });
            }
            NewPluginNotification::PluginServerNotification { method, params } => {
                for plugin in self.new_plugins.iter() {
                    plugin.server_notification(method, params.clone());
                }
            }
        }
    }

    pub fn handle_request(&mut self, rpc: PluginRequest) {
        todo!()
    }
}
