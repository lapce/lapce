pub mod buffer;
pub mod core_proxy;
pub mod dispatch;
pub mod lsp;
pub mod plugin;

use dispatch::Dispatcher;
use std::io;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;

pub fn mainloop() {
    let (sender, receiver, io_threads) = lapce_rpc::stdio();
    let dispatcher = Dispatcher::new(sender);
    dispatcher.mainloop(receiver);
}
