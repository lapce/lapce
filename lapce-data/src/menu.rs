use crate::command::LapceCommand;

pub enum MenuKind {
    Item(MenuItem),
    Separator,
}

#[derive(Debug)]
pub struct MenuItem {
    pub desc: Option<String>,
    pub command: LapceCommand,
}

impl MenuItem {
    pub fn desc(&self) -> &str {
        self.desc.as_deref().unwrap_or_else(|| {
            self.command
                .kind
                .desc()
                .unwrap_or_else(|| self.command.kind.str())
        })
    }
}
