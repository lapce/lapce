mod dispatch;

use dispatch::Dispatcher;
use std::io;
use xi_rpc::RpcLoop;

pub fn mainloop() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut rpc_looper = RpcLoop::new(stdout);
    let mut dispatcher = Dispatcher::new();

    rpc_looper.mainloop(|| stdin.lock(), &mut dispatcher);
}
