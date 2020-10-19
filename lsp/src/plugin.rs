use lapce_core::plugin::{PluginBufferInfo, PluginId};
use xi_rope::RopeDelta;
use xi_rpc::RpcPeer;

use crate::buffer::Buffer;

#[derive(Clone)]
pub struct CoreProxy {
    plugin_id: PluginId,
    peer: RpcPeer,
}

pub trait Plugin {
    fn initialize(&mut self, core: CoreProxy);

    fn new_buffer(&mut self, buffer: &mut Buffer);

    fn update(&mut self, buffer: &mut Buffer, delta: &RopeDelta);
}
