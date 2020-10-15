use lapce::plugin::{HostNotfication, HostRequest};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use xi_rpc::Handler;

pub struct Dispatcher {}

impl Dispatcher {
    pub fn new() -> Dispatcher {
        Dispatcher {}
    }
}

impl Handler for Dispatcher {
    type Notification = HostNotification;
    type Request = HostRequest;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}
