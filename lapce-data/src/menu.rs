use crate::command::LapceCommand;

#[derive(Debug)]
pub enum MenuKind {
    Item(MenuItem),
    Separator,
}

#[derive(Debug)]
pub struct MenuItem {
    pub desc: Option<String>,
    pub command: LapceCommand,
    pub enabled: bool,
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
