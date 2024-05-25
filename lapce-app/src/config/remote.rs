use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RemoteConfig {
    #[field_names(desc = "List of custom remote options to connect to")]
    pub entries: HashMap<String, RemoteEntryConfig>,
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RemoteEntryConfig {
    #[field_names(desc = "Base executable to use")]
    pub program: String,

    #[field_names(desc = "Copy file command")]
    pub copy_args: Vec<String>,

    #[field_names(desc = "Connect command")]
    pub exec_args: Vec<String>,

    #[field_names(desc = "Start command")]
    pub start_args: Option<Vec<String>>,

    #[field_names(desc = "Stop command")]
    pub stop_args: Option<Vec<String>>,
}
