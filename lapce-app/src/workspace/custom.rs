use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Host {
    pub name: String,
    pub program: String,
    pub copy_args: Vec<String>,
    pub exec_args: Vec<String>,
    pub start_args: Option<Vec<String>>,
    pub stop_args: Option<Vec<String>>,
}

impl Host {
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn display_name(&self) -> String {
        format!("[Custom: {self}]")
    }
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        Ok(())
    }
}
