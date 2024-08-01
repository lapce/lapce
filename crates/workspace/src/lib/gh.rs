use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Host {
    pub codespace: String,
}

impl Host {
    pub fn from_string(s: &str) -> Self {
        Self {
            codespace: s.to_owned(),
        }
    }

    pub fn codespace(&self) -> String {
        self.codespace.clone()
    }
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.codespace)?;
        Ok(())
    }
}
