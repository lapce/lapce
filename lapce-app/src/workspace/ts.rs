use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Host {
    pub user: Option<String>,
    pub host: String,
}

impl Host {
    pub fn from_string(s: &str) -> Self {
        let mut whole_splits = s.split(':');
        let splits = whole_splits
            .next()
            .unwrap()
            .split('@')
            .collect::<Vec<&str>>();
        let mut splits = splits.iter().rev();
        let host = splits.next().unwrap().to_string();
        let user = splits.next().map(|s| s.to_string());
        Self { user, host }
    }

    pub fn user_host(&self) -> String {
        if let Some(user) = self.user.as_ref() {
            format!("{user}@{}", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(user) = self.user.as_ref() {
            write!(f, "{user}@")?;
        }
        write!(f, "{}", self.host)?;
        Ok(())
    }
}
