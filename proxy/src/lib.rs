pub mod buffer;
pub mod core_proxy;
pub mod dispatch;
pub mod plugin;

use dispatch::Dispatcher;
use std::io;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;

pub fn mainloop() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut rpc_looper = RpcLoop::new(stdout);
    let peer: RpcPeer = Box::new(rpc_looper.get_raw_peer());
    let mut dispatcher = Dispatcher::new(peer);
    let result = rpc_looper.mainloop(|| stdin.lock(), &mut dispatcher);
    eprintln!("rpc looper stopped {:?}", result);
}
