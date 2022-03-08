use crate::{
    command::{
        CommandTarget, LapceCommandNew, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::{LapceTabData, PanelKind, SplitContent},
    keypress::{Alignment, DefaultKeyPressHandler, KeyMap, KeyPress},
    svg::logo_svg,
};

use druid::{
    kurbo::{Line, Rect},
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    Command, FontFamily, Target, WidgetId,
};
use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use lapce_proxy::terminal::TermId;
use serde::{Deserialize, Serialize};
use strum::EnumMessage;

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

pub struct ChildWidgetNew {
    pub widget: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
}

fn empty_editor_commands(modal: bool, has_workspace: bool) -> Vec<LapceCommandNew> {
    if !has_workspace {
        vec![
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteCommand.to_string(),
                data: None,
                palette_desc: Some("Show All Commands".to_string()),
                target: CommandTarget::Workbench,
            },
            if modal {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::DisableModal.to_string(),
                    data: None,
                    palette_desc: LapceWorkbenchCommand::DisableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            } else {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::EnableModal.to_string(),
                    data: None,
                    palette_desc: LapceWorkbenchCommand::EnableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::OpenFolder.to_string(),
                data: None,
                palette_desc: Some("Open Folder".to_string()),
                target: CommandTarget::Workbench,
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteWorkspace.to_string(),
                data: None,
                palette_desc: Some("Open Recent".to_string()),
                target: CommandTarget::Workbench,
            },
        ]
    } else {
        vec![
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::PaletteCommand.to_string(),
                data: None,
                palette_desc: Some("Show All Commands".to_string()),
                target: CommandTarget::Workbench,
            },
            if modal {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::DisableModal.to_string(),
                    data: None,
                    palette_desc: LapceWorkbenchCommand::DisableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            } else {
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::EnableModal.to_string(),
                    data: None,
                    palette_desc: LapceWorkbenchCommand::EnableModal
                        .get_message()
                        .map(|m| m.to_string()),
                    target: CommandTarget::Workbench,
                }
            },
            LapceCommandNew {
                cmd: LapceWorkbenchCommand::Palette.to_string(),
                data: None,
                palette_desc: Some("Go To File".to_string()),
                target: CommandTarget::Workbench,
            },
        ]
    }
}

pub fn keybinding_to_string(keypress: &KeyPress) -> String {
    let mut keymap_str = "".to_string();
    if keypress.mods.ctrl() {
        keymap_str += "Ctrl+";
    }
    if keypress.mods.alt() {
        keymap_str += "Alt+";
    }
    if keypress.mods.meta() {
        let keyname = match std::env::consts::OS {
            "macos" => "Cmd",
            "windows" => "Win",
            _ => "Meta",
        };
        keymap_str += keyname;
        keymap_str += "+";
    }
    if keypress.mods.shift() {
        keymap_str += "Shift+";
    }
    keymap_str += &keypress.key.to_string();
    keymap_str
}
