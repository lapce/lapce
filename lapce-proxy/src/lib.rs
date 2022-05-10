pub mod buffer;
pub mod dispatch;
pub mod lsp;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use dispatch::Dispatcher;

pub fn mainloop() {
    let (sender, receiver) = lapce_rpc::stdio();
    let dispatcher = Dispatcher::new(sender);
    let _ = dispatcher.mainloop(receiver);
}
