pub mod buffer;
pub mod dispatch;
pub mod lsp;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use dispatch::Dispatcher;

#[cfg(debug_assertions)]
pub const APPLICATION_NAME: &str = "Lapce-debug";

#[cfg(debug_assertions)]
pub const VERSION: &str = "nightly";

#[cfg(not(debug_assertions))]
pub const APPLICATION_NAME: &str = "Lapce";

#[cfg(not(debug_assertions))]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn mainloop() {
    let (sender, receiver) = lapce_rpc::stdio();
    let dispatcher = Dispatcher::new(sender);
    let _ = dispatcher.mainloop(receiver);
}
