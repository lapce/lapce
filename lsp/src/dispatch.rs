use std::collections::HashMap;

use crate::plugin::PluginId;
use crate::plugin::{CoreProxy, Plugin};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use xi_rpc::{Handler, RpcCtx};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
/// RPC Notifications sent from the host
pub enum HostNotification {
    Initialize { plugin_id: PluginId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
/// RPC Request sent from the host
pub enum HostRequest {}

pub struct Dispatcher<'a, P: 'a + Plugin> {
    plugin: &'a mut P,
    plugin_id: Option<PluginId>,
    //  buffers: HashMap<BufferId, Buffer>,
}

impl<'a, P: 'a + Plugin> Dispatcher<'a, P> {
    pub(crate) fn new(plugin: &'a mut P) -> Self {
        Dispatcher {
            plugin,
            plugin_id: None,
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
                self.plugin_id = Some(plugin_id.clone());
                let core_proxy = CoreProxy::new(plugin_id, ctx);
                self.plugin.initialize(core_proxy);
            } //        HostNotification::NewBuffer { buffer_info } => {
              //            let buffer_id = buffer_info.buffer_id.clone();
              //            let buffer = Buffer::new(
              //                ctx.get_peer().clone(),
              //                self.plugin_id.as_ref().unwrap().clone(),
              //                buffer_info,
              //            );
              //            self.buffers.insert(buffer_id.clone(), buffer);

              //            let buffer = self.buffers.get_mut(&buffer_id).unwrap();
              //            self.plugin.new_buffer(buffer);
              //        }
              //        HostNotification::Update {
              //            buffer_id,
              //            delta,
              //            new_len,
              //            new_line_count,
              //            rev,
              //        } => {
              //            let buffer = self.buffers.get_mut(&buffer_id).unwrap();
              //            buffer.update(&delta, new_len, new_line_count, rev);
              //            self.plugin.update(buffer, &delta, rev);
              //        }
              //         HostNotification::GetCompletion {
              //             buffer_id,
              //             request_id,
              //             offset,
              //         } => {
              //             let buffer = self.buffers.get_mut(&buffer_id).unwrap();
              //             self.plugin.get_completion(buffer, request_id, offset);
              //         }
        }
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }

    fn idle(&mut self, ctx: &RpcCtx, token: usize) {
        //        let buffer_id: BufferId = BufferId(token);
        //        let buffer = self.buffers.get_mut(&buffer_id).unwrap();
        //        self.plugin.idle(buffer);
    }
}
