mod dispatch;
mod lsp;
mod lsp_client;
mod plugin;

use dispatch::Dispatcher;
use lsp::LspPlugin;
use std::io;
use xi_rpc::RpcLoop;

pub fn mainloop() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut lsp_plugin = LspPlugin::new();
    let mut rpc_looper = RpcLoop::new(stdout);
    let mut dispatcher = Dispatcher::new(&mut lsp_plugin);

    let result = rpc_looper.mainloop(|| stdin.lock(), &mut dispatcher);
    eprintln!("rpc looper stopped {:?}", result);
}
