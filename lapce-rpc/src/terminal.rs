use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::counter::Counter;

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct TermId(pub u64);

impl TermId {
    pub fn next() -> Self {
        static TERMINAL_ID_COUNTER: Counter = Counter::new();
        Self(TERMINAL_ID_COUNTER.next())
    }
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize, Default)]
pub struct TerminalProfile {
    pub name: String,
    pub command: Option<String>,
    pub arguments: Option<Vec<String>>,
    pub workdir: Option<url::Url>,
    pub environment: Option<HashMap<String, String>>,
}

impl TerminalProfile {}
