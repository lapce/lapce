use crate::plugin::Plugin;
use lapce_core::plugin::{HostNotification, HostRequest};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use xi_rpc::Handler;

pub struct Dispatcher<'a, P: 'a + Plugin> {
    plugin: &'a mut P,
}

impl<'a, P: 'a + Plugin> Dispatcher<'a, P> {
    pub(crate) fn new(plugin: &'a mut P) -> Self {
        Dispatcher { plugin }
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
            HostNotification::NewBuffer { buffer_info } => {
                self.plugin.new_buffer(&buffer_info);
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
