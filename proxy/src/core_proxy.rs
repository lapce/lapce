use xi_rpc::RpcPeer;

#[derive(Clone)]
pub struct CoreProxy {
    pub peer: RpcPeer,
}

impl CoreProxy {
    pub fn new(peer: RpcPeer) -> Self {
        Self { peer }
    }
}
