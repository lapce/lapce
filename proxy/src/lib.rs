pub mod buffer;
pub mod dispatch;
pub mod lsp;
pub mod plugin;
pub mod terminal;

use dispatch::Dispatcher;

pub fn mainloop() {
    let (sender, receiver) = lapce_rpc::stdio();
    let dispatcher = Dispatcher::new(sender);
    dispatcher.mainloop(receiver);
}
