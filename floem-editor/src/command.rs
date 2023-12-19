use lapce_core::command::{
    EditCommand, FocusCommand, MotionModeCommand, MoveCommand, MultiSelectionCommand,
};
use strum::EnumMessage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Edit(EditCommand),
    Move(MoveCommand),
    Focus(FocusCommand),
    MotionMode(MotionModeCommand),
    MultiSelection(MultiSelectionCommand),
}

impl Command {
    pub fn desc(&self) -> Option<&'static str> {
        match &self {
            Command::Edit(cmd) => cmd.get_message(),
            Command::Move(cmd) => cmd.get_message(),
            Command::Focus(cmd) => cmd.get_message(),
            Command::MotionMode(cmd) => cmd.get_message(),
            Command::MultiSelection(cmd) => cmd.get_message(),
        }
    }

    pub fn str(&self) -> &'static str {
        match &self {
            Command::Edit(cmd) => cmd.into(),
            Command::Move(cmd) => cmd.into(),
            Command::Focus(cmd) => cmd.into(),
            Command::MotionMode(cmd) => cmd.into(),
            Command::MultiSelection(cmd) => cmd.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExecuted {
    Yes,
    No,
}
