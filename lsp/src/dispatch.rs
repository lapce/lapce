use std::collections::HashMap;

use crate::{buffer::Buffer, plugin::Plugin};
use lapce_core::{
    buffer::BufferId,
    plugin::{HostNotification, HostRequest, PluginId},
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use xi_rpc::Handler;

pub struct Dispatcher<'a, P: 'a + Plugin> {
    plugin: &'a mut P,
    plugin_id: Option<PluginId>,
    buffers: HashMap<BufferId, Buffer>,
}

impl<'a, P: 'a + Plugin> Dispatcher<'a, P> {
    pub(crate) fn new(plugin: &'a mut P) -> Self {
        Dispatcher {
            plugin,
            plugin_id: None,
            buffers: HashMap::new(),
        }
    }
}

impl<'a, P: Plugin> Handler for Dispatcher<'a, P> {
    type Notification = HostNotification;
    type Request = HostRequest;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
        match rpc {
            HostNotification::Initialize { plugin_id } => {
                self.plugin_id = Some(plugin_id);
            }
            HostNotification::NewBuffer { buffer_info } => {
                let buffer_id = buffer_info.buffer_id.clone();
                let buffer = Buffer::new(
                    ctx.get_peer().clone(),
                    self.plugin_id.as_ref().unwrap().clone(),
                    buffer_info,
                );
                self.buffers.insert(buffer_id.clone(), buffer);

                let buffer = self.buffers.get_mut(&buffer_id).unwrap();
                self.plugin.new_buffer(buffer);
            }
            HostNotification::Update { buffer_id, delta } => {
                let buffer = self.buffers.get_mut(&buffer_id).unwrap();
                self.plugin.update(buffer, &delta);
            }
        }
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}
