use serde::{Deserialize, Serialize};

use crate::counter::Counter;

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct BufferId(pub u64);

impl BufferId {
    pub fn next() -> Self {
        static BUFFER_ID_COUNTER: Counter = Counter::new();
        Self(BUFFER_ID_COUNTER.next())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBufferResponse {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferHeadResponse {
    pub version: String,
    pub content: String,
}
