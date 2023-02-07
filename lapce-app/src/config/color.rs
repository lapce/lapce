use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoadThemeError {
    #[error("themes folder not found, possibly it could not be created")]
    ThemesFolderNotFound,
    #[error("theme file ({theme_name}.toml) was not found in {themes_folder:?}")]
    FileNotFound {
        themes_folder: PathBuf,
        theme_name: String,
    },
    #[error("There was an error reading the theme file")]
    Read(std::io::Error),
}

pub struct LapceColor {}

impl LapceColor {
    pub const LAPCE_WARN: &str = "lapce.warn";
    pub const LAPCE_ERROR: &str = "lapce.error";
    pub const LAPCE_DROPDOWN_SHADOW: &str = "lapce.dropdown_shadow";
    pub const LAPCE_BORDER: &str = "lapce.border";
    pub const LAPCE_SCROLL_BAR: &str = "lapce.scroll_bar";

    pub const LAPCE_BUTTON_PRIMARY_BACKGROUND: &str =
        "lapce.button.primary.background";
    pub const LAPCE_BUTTON_PRIMARY_FOREGROUND: &str =
        "lapce.button.primary.foreground";

    pub const LAPCE_TAB_ACTIVE_BACKGROUND: &str = "lapce.tab.active.background";
    pub const LAPCE_TAB_ACTIVE_FOREGROUND: &str = "lapce.tab.active.foreground";
    pub const LAPCE_TAB_ACTIVE_UNDERLINE: &str = "lapce.tab.active.underline";

    pub const LAPCE_TAB_INACTIVE_BACKGROUND: &str = "lapce.tab.inactive.background";
    pub const LAPCE_TAB_INACTIVE_FOREGROUND: &str = "lapce.tab.inactive.foreground";
    pub const LAPCE_TAB_INACTIVE_UNDERLINE: &str = "lapce.tab.inactive.underline";

    pub const LAPCE_TAB_SEPARATOR: &str = "lapce.tab.separator";

    pub const LAPCE_ICON_ACTIVE: &str = "lapce.icon.active";
    pub const LAPCE_ICON_INACTIVE: &str = "lapce.icon.inactive";

    pub const LAPCE_REMOTE_ICON: &str = "lapce.remote.icon";
    pub const LAPCE_REMOTE_LOCAL: &str = "lapce.remote.local";
    pub const LAPCE_REMOTE_CONNECTED: &str = "lapce.remote.connected";
    pub const LAPCE_REMOTE_CONNECTING: &str = "lapce.remote.connecting";
    pub const LAPCE_REMOTE_DISCONNECTED: &str = "lapce.remote.disconnected";

    pub const LAPCE_PLUGIN_NAME: &str = "lapce.plugin.name";
    pub const LAPCE_PLUGIN_DESCRIPTION: &str = "lapce.plugin.description";
    pub const LAPCE_PLUGIN_AUTHOR: &str = "lapce.plugin.author";

    pub const EDITOR_BACKGROUND: &str = "editor.background";
    pub const EDITOR_FOREGROUND: &str = "editor.foreground";
    pub const EDITOR_DIM: &str = "editor.dim";
    pub const EDITOR_FOCUS: &str = "editor.focus";
    pub const EDITOR_CARET: &str = "editor.caret";
    pub const EDITOR_SELECTION: &str = "editor.selection";
    pub const EDITOR_CURRENT_LINE: &str = "editor.current_line";
    pub const EDITOR_LINK: &str = "editor.link";
    pub const EDITOR_VISIBLE_WHITESPACE: &str = "editor.visible_whitespace";
    pub const EDITOR_INDENT_GUIDE: &str = "editor.indent_guide";
    pub const EDITOR_DRAG_DROP_BACKGROUND: &str = "editor.drag_drop_background";
    pub const EDITOR_STICKY_HEADER_BACKGROUND: &str =
        "editor.sticky_header_background";
    pub const EDITOR_DRAG_DROP_TAB_BACKGROUND: &str =
        "editor.drag_drop_tab_background";

    pub const INLAY_HINT_FOREGROUND: &str = "inlay_hint.foreground";
    pub const INLAY_HINT_BACKGROUND: &str = "inlay_hint.background";

    pub const ERROR_LENS_ERROR_FOREGROUND: &str = "error_lens.error.foreground";
    pub const ERROR_LENS_ERROR_BACKGROUND: &str = "error_lens.error.background";
    pub const ERROR_LENS_WARNING_FOREGROUND: &str = "error_lens.warning.foreground";
    pub const ERROR_LENS_WARNING_BACKGROUND: &str = "error_lens.warning.background";
    pub const ERROR_LENS_OTHER_FOREGROUND: &str = "error_lens.other.foreground";
    pub const ERROR_LENS_OTHER_BACKGROUND: &str = "error_lens.other.background";

    pub const COMPLETION_LENS_FOREGROUND: &str = "completion_lens.foreground";

    pub const SOURCE_CONTROL_ADDED: &str = "source_control.added";
    pub const SOURCE_CONTROL_REMOVED: &str = "source_control.removed";
    pub const SOURCE_CONTROL_MODIFIED: &str = "source_control.modified";

    pub const TERMINAL_CURSOR: &str = "terminal.cursor";
    pub const TERMINAL_BACKGROUND: &str = "terminal.background";
    pub const TERMINAL_FOREGROUND: &str = "terminal.foreground";
    pub const TERMINAL_RED: &str = "terminal.red";
    pub const TERMINAL_BLUE: &str = "terminal.blue";
    pub const TERMINAL_GREEN: &str = "terminal.green";
    pub const TERMINAL_YELLOW: &str = "terminal.yellow";
    pub const TERMINAL_BLACK: &str = "terminal.black";
    pub const TERMINAL_WHITE: &str = "terminal.white";
    pub const TERMINAL_CYAN: &str = "terminal.cyan";
    pub const TERMINAL_MAGENTA: &str = "terminal.magenta";

    pub const TERMINAL_BRIGHT_RED: &str = "terminal.bright_red";
    pub const TERMINAL_BRIGHT_BLUE: &str = "terminal.bright_blue";
    pub const TERMINAL_BRIGHT_GREEN: &str = "terminal.bright_green";
    pub const TERMINAL_BRIGHT_YELLOW: &str = "terminal.bright_yellow";
    pub const TERMINAL_BRIGHT_BLACK: &str = "terminal.bright_black";
    pub const TERMINAL_BRIGHT_WHITE: &str = "terminal.bright_white";
    pub const TERMINAL_BRIGHT_CYAN: &str = "terminal.bright_cyan";
    pub const TERMINAL_BRIGHT_MAGENTA: &str = "terminal.bright_magenta";

    pub const PALETTE_BACKGROUND: &str = "palette.background";
    pub const PALETTE_FOREGROUND: &str = "palette.foreground";
    pub const PALETTE_CURRENT_BACKGROUND: &str = "palette.current.background";
    pub const PALETTE_CURRENT_FOREGROUND: &str = "palette.current.foreground";

    pub const COMPLETION_BACKGROUND: &str = "completion.background";
    pub const COMPLETION_CURRENT: &str = "completion.current";

    pub const HOVER_BACKGROUND: &str = "hover.background";

    pub const ACTIVITY_BACKGROUND: &str = "activity.background";
    pub const ACTIVITY_CURRENT: &str = "activity.current";

    pub const DEBUG_BREAKPOINT: &str = "debug.breakpoint";
    pub const DEBUG_BREAKPOINT_HOVER: &str = "debug.breakpoint.hover";

    pub const PANEL_BACKGROUND: &str = "panel.background";
    pub const PANEL_FOREGROUND: &str = "panel.foreground";
    pub const PANEL_FOREGROUND_DIM: &str = "panel.foreground.dim";
    pub const PANEL_CURRENT_BACKGROUND: &str = "panel.current.background";
    pub const PANEL_CURRENT_FOREGROUND: &str = "panel.current.foreground";
    pub const PANEL_CURRENT_FOREGROUND_DIM: &str = "panel.current.foreground.dim";
    pub const PANEL_HOVERED_BACKGROUND: &str = "panel.hovered.background";
    pub const PANEL_HOVERED_FOREGROUND: &str = "panel.hovered.foreground";
    pub const PANEL_HOVERED_FOREGROUND_DIM: &str = "panel.hovered.foreground.dim";

    pub const STATUS_BACKGROUND: &str = "status.background";
    pub const STATUS_FOREGROUND: &str = "status.foreground";
    pub const STATUS_MODAL_NORMAL_BACKGROUND: &str =
        "status.modal.normal.background";
    pub const STATUS_MODAL_NORMAL_FOREGROUND: &str =
        "status.modal.normal.foreground";
    pub const STATUS_MODAL_INSERT_BACKGROUND: &str =
        "status.modal.insert.background";
    pub const STATUS_MODAL_INSERT_FOREGROUND: &str =
        "status.modal.insert.foreground";
    pub const STATUS_MODAL_VISUAL_BACKGROUND: &str =
        "status.modal.visual.background";
    pub const STATUS_MODAL_VISUAL_FOREGROUND: &str =
        "status.modal.visual.foreground";
    pub const STATUS_MODAL_TERMINAL_BACKGROUND: &str =
        "status.modal.terminal.background";
    pub const STATUS_MODAL_TERMINAL_FOREGROUND: &str =
        "status.modal.terminal.foreground";

    pub const MARKDOWN_BLOCKQUOTE: &'static str = "markdown.blockquote";
}
