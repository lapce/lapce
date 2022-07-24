pub mod buffer;
pub mod dispatch;
pub mod lsp;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use dispatch::{Dispatcher, NewDispatcher};

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

pub fn new_mainloop() {
    let (core_sender, core_receiver) = lapce_rpc::stdio();
    let mut dispatcher = NewDispatcher::new(core_sender);
    dispatcher.mainloop(core_receiver);
}
