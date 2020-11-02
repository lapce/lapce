pub mod buffer;
pub mod dispatch;
pub mod plugin;

use dispatch::Dispatcher;
use xi_rpc::RpcLoop;
use std::io;

pub fn mainloop() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut rpc_looper = RpcLoop::new(stdout);
    let mut dispatcher = Dispatcher::new();
    let result = rpc_looper.mainloop(|| stdin.lock(), &mut dispatcher);
    eprintln!("rpc looper stopped {:?}", result);
}
