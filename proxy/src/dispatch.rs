use crate::buffer::{Buffer, BufferId};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use xi_rpc::{Handler, RpcCtx};

pub struct Dispatcher {
    buffers: HashMap<BufferId, Buffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Notification {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

impl Dispatcher {
    pub fn new() -> Dispatcher {
        Dispatcher {
            buffers: HashMap::new(),
        }
    }
}

impl Handler for Dispatcher {
    type Notification = Notification;
    type Request = Request;

    fn handle_notification(&mut self, ctx: &RpcCtx, rpc: Self::Notification) {}

    fn handle_request(
        &mut self,
        ctx: &RpcCtx,
        rpc: Self::Request,
    ) -> Result<Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}
