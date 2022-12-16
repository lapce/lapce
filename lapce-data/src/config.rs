use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use druid::{
    piet::{PietText, Svg, Text, TextLayout, TextLayoutBuilder},
    Color, ExtEventSink, FontFamily, Size, Target,
};
use indexmap::IndexMap;
use lapce_core::directory::Directory;
use lapce_proxy::plugin::wasi::find_all_volts;
use lsp_types::{CompletionItemKind, SymbolKind};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use structdesc::FieldNames;
use thiserror::Error;
use toml_edit::easy as toml;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceWorkspace, LapceWorkspaceType},
    svg::SvgStore,
};

pub const LOGO: &str = include_str!("../../extra/images/logo.svg");
const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
const DEFAULT_ICON_THEME: &str = include_str!("../../defaults/icon-theme.toml");

static DEFAULT_CONFIG: Lazy<config::Config> = Lazy::new(LapceConfig::default_config);
static DEFAULT_LAPCE_CONFIG: Lazy<LapceConfig> =
    Lazy::new(LapceConfig::default_lapce_config);

pub struct LapceTheme {}

impl LapceTheme {
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
    pub const STATUS_MODAL_NORMAL: &str = "status.modal.normal";
    pub const STATUS_MODAL_INSERT: &str = "status.modal.insert";
    pub const STATUS_MODAL_VISUAL: &str = "status.modal.visual";
    pub const STATUS_MODAL_TERMINAL: &str = "status.modal.terminal";

    pub const PALETTE_INPUT_LINE_HEIGHT: druid::Key<f64> =
        druid::Key::new("lapce.palette_input_line_height");
    pub const PALETTE_INPUT_LINE_PADDING: druid::Key<f64> =
        druid::Key::new("lapce.palette_input_line_padding");
    pub const INPUT_LINE_HEIGHT: druid::Key<f64> =
        druid::Key::new("lapce.input_line_height");
    pub const INPUT_LINE_PADDING: druid::Key<f64> =
        druid::Key::new("lapce.input_line_padding");
    pub const INPUT_FONT_SIZE: druid::Key<u64> =
        druid::Key::new("lapce.input_font_size");

    pub const MARKDOWN_BLOCKQUOTE: &'static str = "markdown.blockquote";
}

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

pub struct LapceIcons {}

impl LapceIcons {
    pub const WINDOW_CLOSE: &str = "window.close";
    pub const WINDOW_RESTORE: &str = "window.restore";
    pub const WINDOW_MAXIMIZE: &str = "window.maximize";
    pub const WINDOW_MINIMIZE: &str = "window.minimize";

    pub const LINK: &str = "link";
    pub const ERROR: &str = "error";
    pub const ADD: &str = "add";
    pub const CLOSE: &str = "close";
    pub const REMOTE: &str = "remote";
    pub const PROBLEM: &str = "error";
    pub const UNSAVED: &str = "unsaved";
    pub const WARNING: &str = "warning";
    pub const TERMINAL: &str = "terminal";
    pub const SETTINGS: &str = "settings";
    pub const LIGHTBULB: &str = "lightbulb";
    pub const EXTENSIONS: &str = "extensions";
    pub const BREADCRUMB_SEPARATOR: &str = "breadcrumb_separator";

    pub const FILE: &str = "file";
    pub const FILE_EXPLORER: &str = "file_explorer";
    pub const FILE_PICKER_UP: &str = "file_picker_up";

    pub const SCM: &str = "scm.icon";
    pub const SCM_DIFF_MODIFIED: &str = "scm.diff.modified";
    pub const SCM_DIFF_ADDED: &str = "scm.diff.added";
    pub const SCM_DIFF_REMOVED: &str = "scm.diff.removed";
    pub const SCM_DIFF_RENAMED: &str = "scm.diff.renamed";
    pub const SCM_CHANGE_ADD: &str = "scm.change.add";
    pub const SCM_CHANGE_REMOVE: &str = "scm.change.remove";

    pub const PALETTE_MENU: &str = "palette.menu";

    pub const LOCATION_BACKWARD: &str = "location.backward";
    pub const LOCATION_FORWARD: &str = "location.forward";

    pub const ITEM_OPENED: &str = "item.opened";
    pub const ITEM_CLOSED: &str = "item.closed";

    pub const DIRECTORY_CLOSED: &str = "directory.closed";
    pub const DIRECTORY_OPENED: &str = "directory.opened";

    pub const PANEL_RESTORE: &str = "panel.restore";
    pub const PANEL_MAXIMISE: &str = "panel.maximise";

    pub const SPLIT_HORIZONTAL: &str = "split.horizontal";

    pub const TAB_PREVIOUS: &str = "tab.previous";
    pub const TAB_NEXT: &str = "tab.next";

    pub const SIDEBAR_LEFT: &str = "sidebar.left.on";
    pub const SIDEBAR_LEFT_OFF: &str = "sidebar.left.off";
    pub const SIDEBAR_RIGHT: &str = "sidebar.right.on";
    pub const SIDEBAR_RIGHT_OFF: &str = "sidebar.right.off";

    pub const LAYOUT_PANEL: &str = "layout.panel.on";
    pub const LAYOUT_PANEL_OFF: &str = "layout.panel.off";

    pub const SEARCH: &'static str = "search.icon";
    pub const SEARCH_CLEAR: &'static str = "search.clear";
    pub const SEARCH_FORWARD: &'static str = "search.forward";
    pub const SEARCH_BACKWARD: &'static str = "search.backward";
    pub const SEARCH_CASE_SENSITIVE: &'static str = "search.case_sensitive";

    pub const FILE_TYPE_CODE: &str = "file-code";
    pub const FILE_TYPE_MEDIA: &str = "file-media";
    pub const FILE_TYPE_BINARY: &str = "file-binary";
    pub const FILE_TYPE_ARCHIVE: &str = "file-zip";
    pub const FILE_TYPE_SUBMODULE: &str = "file-submodule";
    pub const FILE_TYPE_SYMLINK_FILE: &str = "file-symlink-file";
    pub const FILE_TYPE_SYMLINK_DIRECTORY: &str = "file-symlink-directory";

    pub const SYMBOL_KIND_ARRAY: &str = "symbol_kind.array";
    pub const SYMBOL_KIND_BOOLEAN: &str = "symbol_kind.boolean";
    pub const SYMBOL_KIND_CLASS: &str = "symbol_kind.class";
    pub const SYMBOL_KIND_CONSTANT: &str = "symbol_kind.constant";
    pub const SYMBOL_KIND_ENUM_MEMBER: &str = "symbol_kind.enum_member";
    pub const SYMBOL_KIND_ENUM: &str = "symbol_kind.enum";
    pub const SYMBOL_KIND_EVENT: &str = "symbol_kind.event";
    pub const SYMBOL_KIND_FIELD: &str = "symbol_kind.field";
    pub const SYMBOL_KIND_FILE: &str = "symbol_kind.file";
    pub const SYMBOL_KIND_FUNCTION: &str = "symbol_kind.function";
    pub const SYMBOL_KIND_INTERFACE: &str = "symbol_kind.interface";
    pub const SYMBOL_KIND_KEY: &str = "symbol_kind.key";
    pub const SYMBOL_KIND_METHOD: &str = "symbol_kind.method";
    pub const SYMBOL_KIND_NAMESPACE: &str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_NUMBER: &str = "symbol_kind.number";
    pub const SYMBOL_KIND_OBJECT: &str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_OPERATOR: &str = "symbol_kind.operator";
    pub const SYMBOL_KIND_PROPERTY: &str = "symbol_kind.property";
    pub const SYMBOL_KIND_STRING: &str = "symbol_kind.string";
    pub const SYMBOL_KIND_STRUCT: &str = "symbol_kind.struct";
    pub const SYMBOL_KIND_TYPE_PARAMETER: &str = "symbol_kind.type_parameter";
    pub const SYMBOL_KIND_VARIABLE: &str = "symbol_kind.variable";

    pub const COMPLETION_ITEM_KIND_CLASS: &str = "completion_item_kind.class";
    pub const COMPLETION_ITEM_KIND_CONSTANT: &str = "completion_item_kind.constant";
    pub const COMPLETION_ITEM_KIND_ENUM_MEMBER: &str =
        "completion_item_kind.enum_member";
    pub const COMPLETION_ITEM_KIND_ENUM: &str = "completion_item_kind.enum";
    pub const COMPLETION_ITEM_KIND_FIELD: &str = "completion_item_kind.field";
    pub const COMPLETION_ITEM_KIND_FUNCTION: &str = "completion_item_kind.function";
    pub const COMPLETION_ITEM_KIND_INTERFACE: &str =
        "completion_item_kind.interface";
    pub const COMPLETION_ITEM_KIND_KEYWORD: &str = "completion_item_kind.keyword";
    pub const COMPLETION_ITEM_KIND_METHOD: &str = "completion_item_kind.method";
    pub const COMPLETION_ITEM_KIND_MODULE: &str = "completion_item_kind.module";
    pub const COMPLETION_ITEM_KIND_PROPERTY: &str = "completion_item_kind.property";
    pub const COMPLETION_ITEM_KIND_SNIPPET: &str = "completion_item_kind.snippet";
    pub const COMPLETION_ITEM_KIND_STRING: &str = "completion_item_kind.string";
    pub const COMPLETION_ITEM_KIND_STRUCT: &str = "completion_item_kind.struct";
    pub const COMPLETION_ITEM_KIND_VARIABLE: &str = "completion_item_kind.variable";
}

pub trait GetConfig {
    fn get_config(&self) -> &LapceConfig;
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CoreConfig {
    #[field_names(desc = "Enable modal editing (Vim like)")]
    pub modal: bool,
    #[field_names(desc = "Set the color theme of Lapce")]
    pub color_theme: String,
    #[field_names(desc = "Set the icon theme of Lapce")]
    pub icon_theme: String,
    #[field_names(
        desc = "Enable customised titlebar and disable OS native one (Linux, BSD, Windows)"
    )]
    pub custom_titlebar: bool,
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct EditorConfig {
    #[field_names(desc = "Set the editor font family")]
    pub font_family: String,
    #[field_names(desc = "Set the editor font size")]
    pub font_size: usize,
    #[field_names(desc = "Set the font size in the code lens")]
    pub code_lens_font_size: usize,
    #[field_names(
        desc = "Set the editor line height. If less than 5.0, line height will be a multiple of the font size."
    )]
    line_height: f64,
    #[field_names(desc = "Set the tab width")]
    pub tab_width: usize,
    #[field_names(desc = "If opened editors are shown in a tab")]
    pub show_tab: bool,
    #[field_names(desc = "If navigation breadcrumbs are shown for the file")]
    pub show_bread_crumbs: bool,
    #[field_names(desc = "If the editor can scroll beyond the last line")]
    pub scroll_beyond_last_line: bool,
    #[field_names(
        desc = "Set the minimum number of visible lines above and below the cursor"
    )]
    pub cursor_surrounding_lines: usize,
    #[field_names(
        desc = "Show code context like functions and classes at the top of editor when scroll"
    )]
    pub sticky_header: bool,
    #[field_names(
        desc = "If the editor should show the documentation of the current completion item"
    )]
    pub completion_show_documentation: bool,
    #[field_names(
        desc = "If the editor should show the signature of the function as the parameters are being typed"
    )]
    pub show_signature: bool,
    #[field_names(
        desc = "If the signature view should put the codeblock into a label. This might not work nicely for LSPs which provide invalid code for their labels."
    )]
    pub signature_label_code_block: bool,
    #[field_names(
        desc = "Whether the editor should disable automatic closing of matching pairs"
    )]
    pub auto_closing_matching_pairs: bool,
    #[field_names(
        desc = "How long (in ms) it should take before the hover information appears"
    )]
    pub hover_delay: u64,
    #[field_names(
        desc = "If modal mode should have relative line numbers (though, not in insert mode)"
    )]
    pub modal_mode_relative_line_numbers: bool,
    #[field_names(
        desc = "Whether it should format the document on save (if there is an available formatter)"
    )]
    pub format_on_save: bool,

    #[field_names(desc = "If matching brackets are highlighted")]
    pub highlight_matching_brackets: bool,

    #[field_names(desc = "If scope lines are highlighted")]
    pub highlight_scope_lines: bool,

    #[field_names(desc = "If inlay hints should be displayed")]
    pub enable_inlay_hints: bool,

    #[field_names(
        desc = "Set the inlay hint font family. If empty, it uses the editor font family."
    )]
    pub inlay_hint_font_family: String,
    #[field_names(
        desc = "Set the inlay hint font size. If less than 5 or greater than editor font size, it uses the editor font size."
    )]
    pub inlay_hint_font_size: usize,
    #[field_names(desc = "If diagnostics should be displayed inline")]
    pub enable_error_lens: bool,
    #[field_names(
        desc = "Whether error lens should go to the end of view line, or only to the end of the diagnostic"
    )]
    pub error_lens_end_of_line: bool,
    #[field_names(
        desc = "Set error lens font family. If empty, it uses the inlay hint font family."
    )]
    pub error_lens_font_family: String,
    #[field_names(
        desc = "Set the error lens font size. If 0 it uses the inlay hint font size."
    )]
    pub error_lens_font_size: usize,
    #[field_names(
        desc = "Set the cursor blink interval (in milliseconds). Set to 0 to completely disable."
    )]
    pub blink_interval: u64, // TODO: change to u128 when upgrading config-rs to >0.11
    #[field_names(
        desc = "Whether the multiple cursor selection is case sensitive."
    )]
    pub multicursor_case_sensitive: bool,
    #[field_names(
        desc = "Whether the multiple cursor selection only selects whole words."
    )]
    pub multicursor_whole_words: bool,
    #[field_names(
        desc = "How the editor should render whitespace characters.\nOptions: none, all, boundary, trailing."
    )]
    pub render_whitespace: String,
    #[field_names(desc = "Whether the editor show indent guide.")]
    pub show_indent_guide: bool,
    #[field_names(
        desc = "Set the auto save delay (in milliseconds), Set to 0 to completely disable"
    )]
    pub autosave_interval: u64,
    #[field_names(
        desc = "Whether the document should be formatted when an autosave is triggered (required Format on Save)"
    )]
    pub format_on_autosave: bool,
    #[field_names(
        desc = "If enabled the cursor treats leading soft tabs as if they are hard tabs."
    )]
    pub atomic_soft_tabs: bool,
}

impl EditorConfig {
    pub fn line_height(&self) -> usize {
        const SCALE_OR_SIZE_LIMIT: f64 = 5.0;

        let line_height = if self.line_height < SCALE_OR_SIZE_LIMIT {
            self.line_height * self.font_size as f64
        } else {
            self.line_height
        };

        // Prevent overlapping lines
        (line_height.round() as usize).max(self.font_size)
    }

    pub fn font_family(&self) -> FontFamily {
        if self.font_family.is_empty() {
            FontFamily::SYSTEM_UI
        } else {
            FontFamily::new_unchecked(self.font_family.clone())
        }
    }

    pub fn inlay_hint_font_family(&self) -> FontFamily {
        if self.inlay_hint_font_family.is_empty() {
            self.font_family()
        } else {
            FontFamily::new_unchecked(self.inlay_hint_font_family.clone())
        }
    }

    pub fn inlay_hint_font_size(&self) -> usize {
        if self.inlay_hint_font_size < 5
            || self.inlay_hint_font_size > self.font_size
        {
            self.font_size
        } else {
            self.inlay_hint_font_size
        }
    }

    pub fn error_lens_font_family(&self) -> FontFamily {
        if self.error_lens_font_family.is_empty() {
            self.inlay_hint_font_family()
        } else {
            FontFamily::new_unchecked(self.error_lens_font_family.clone())
        }
    }

    pub fn error_lens_font_size(&self) -> usize {
        if self.error_lens_font_size == 0 {
            self.inlay_hint_font_size()
        } else {
            self.error_lens_font_size
        }
    }
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct UIConfig {
    #[field_names(
        desc = "Set the UI font family. If empty, it uses system default."
    )]
    font_family: String,

    #[field_names(desc = "Set the UI base font size")]
    font_size: usize,

    #[field_names(desc = "Set the icon size in the UI")]
    icon_size: usize,

    #[field_names(
        desc = "Set the header height for panel header and editor tab header"
    )]
    header_height: usize,

    #[field_names(desc = "Set the height for status line")]
    status_height: usize,

    #[field_names(desc = "Set the minimum width for editor tab")]
    tab_min_width: usize,

    #[field_names(desc = "Set the width for scroll bar")]
    scroll_width: usize,

    #[field_names(desc = "Controls the width of drop shadow in the UI")]
    drop_shadow_width: usize,

    #[field_names(desc = "Controls the width of the preview editor")]
    preview_editor_width: usize,

    #[field_names(
        desc = "Set the hover font family. If empty, it uses the UI font family"
    )]
    hover_font_family: String,
    #[field_names(desc = "Set the hover font size. If 0, uses the UI font size")]
    hover_font_size: usize,

    #[field_names(desc = "Trim whitespace from search results")]
    trim_search_results_whitespace: bool,
}

impl UIConfig {
    pub fn font_family(&self) -> FontFamily {
        if self.font_family.is_empty() {
            FontFamily::SYSTEM_UI
        } else {
            FontFamily::new_unchecked(self.font_family.as_str())
        }
    }

    pub fn font_size(&self) -> usize {
        self.font_size.clamp(6, 32)
    }

    pub fn icon_size(&self) -> usize {
        if self.icon_size == 0 {
            self.font_size() + 2
        } else {
            self.icon_size.clamp(6, 32)
        }
    }

    pub fn header_height(&self) -> usize {
        let font_size = self.font_size();
        self.header_height.max(font_size)
    }

    pub fn status_height(&self) -> usize {
        let font_size = self.font_size();
        self.status_height.max(font_size)
    }

    pub fn tab_min_width(&self) -> usize {
        self.tab_min_width
    }

    pub fn scroll_width(&self) -> usize {
        self.scroll_width
    }

    pub fn drop_shadow_width(&self) -> usize {
        self.drop_shadow_width
    }

    pub fn preview_editor_width(&self) -> usize {
        self.preview_editor_width
    }

    pub fn hover_font_family(&self) -> FontFamily {
        if self.hover_font_family.is_empty() {
            self.font_family()
        } else {
            FontFamily::new_unchecked(self.hover_font_family.as_str())
        }
    }

    pub fn hover_font_size(&self) -> usize {
        if self.hover_font_size == 0 {
            self.font_size()
        } else {
            self.hover_font_size
        }
    }

    pub fn trim_search_results_whitespace(&self) -> bool {
        self.trim_search_results_whitespace
    }
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct TerminalConfig {
    #[field_names(
        desc = "Set the terminal font family. If empty, it uses editor font family."
    )]
    pub font_family: String,
    #[field_names(
        desc = "Set the terminal font size, If 0, it uses editor font size."
    )]
    pub font_size: usize,
    #[field_names(
        desc = "Set the terminal line height, If 0, it uses editor line height"
    )]
    pub line_height: usize,
    #[field_names(desc = "Set the terminal Shell")]
    pub shell: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ColorThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub high_contrast: Option<bool>,
    pub base: ThemeBaseConfig,
    pub syntax: IndexMap<String, String>,
    pub ui: IndexMap<String, String>,
}

impl ColorThemeConfig {
    fn resolve_color(
        colors: &IndexMap<String, String>,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        colors
            .iter()
            .map(|(name, hex)| {
                let color = if let Some(stripped) = hex.strip_prefix('$') {
                    base.get(stripped).cloned()
                } else {
                    Color::from_hex_str(hex).ok()
                };

                let color = color
                    .or_else(|| {
                        default.and_then(|default| default.get(name).cloned())
                    })
                    .unwrap_or(Color::rgb8(0, 0, 0));

                (name.to_string(), color)
            })
            .collect()
    }

    fn resolve_ui_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.ui, base, default)
    }

    fn resolve_syntax_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.syntax, base, default)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ThemeBaseConfig {
    pub white: String,
    pub black: String,
    pub grey: String,
    pub blue: String,
    pub red: String,
    pub yellow: String,
    pub orange: String,
    pub green: String,
    pub purple: String,
    pub cyan: String,
    pub magenta: String,
}

impl ThemeBaseConfig {
    pub fn resolve(&self, default: Option<&ThemeBaseColor>) -> ThemeBaseColor {
        let default = default.cloned().unwrap_or_default();
        ThemeBaseColor {
            white: Color::from_hex_str(&self.white).unwrap_or(default.white),
            black: Color::from_hex_str(&self.black).unwrap_or(default.black),
            grey: Color::from_hex_str(&self.grey).unwrap_or(default.grey),
            blue: Color::from_hex_str(&self.blue).unwrap_or(default.blue),
            red: Color::from_hex_str(&self.red).unwrap_or(default.red),
            yellow: Color::from_hex_str(&self.yellow).unwrap_or(default.yellow),
            orange: Color::from_hex_str(&self.orange).unwrap_or(default.orange),
            green: Color::from_hex_str(&self.green).unwrap_or(default.green),
            purple: Color::from_hex_str(&self.purple).unwrap_or(default.purple),
            cyan: Color::from_hex_str(&self.cyan).unwrap_or(default.cyan),
            magenta: Color::from_hex_str(&self.magenta).unwrap_or(default.magenta),
        }
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        Some(match name {
            "white" => &self.white,
            "black" => &self.black,
            "grey" => &self.grey,
            "blue" => &self.blue,
            "red" => &self.red,
            "yellow" => &self.yellow,
            "orange" => &self.orange,
            "green" => &self.green,
            "purple" => &self.purple,
            "cyan" => &self.cyan,
            "magenta" => &self.magenta,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct IconThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub use_editor_color: Option<bool>,
    pub ui: IndexMap<String, String>,
    pub foldername: IndexMap<String, String>,
    pub filename: IndexMap<String, String>,
    pub extension: IndexMap<String, String>,
}

impl IconThemeConfig {
    pub fn resolve_path_to_icon(&self, path: &Path) -> Option<PathBuf> {
        if let Some((_, icon)) = self.filename.get_key_value(
            path.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        ) {
            Some(self.path.join(icon))
        } else if let Some((_, icon)) = self.extension.get_key_value(
            path.extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        ) {
            Some(self.path.join(icon))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum ThemeColorPreference {
    #[default]
    Light,
    Dark,
    HighContrastDark,
    HighContrastLight,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeColor {
    pub color_preference: ThemeColorPreference,
    pub base: ThemeBaseColor,
    pub syntax: HashMap<String, Color>,
    pub ui: HashMap<String, Color>,
}

#[derive(Debug, Clone)]
pub struct ThemeBaseColor {
    pub white: Color,
    pub black: Color,
    pub grey: Color,
    pub blue: Color,
    pub red: Color,
    pub yellow: Color,
    pub orange: Color,
    pub green: Color,
    pub purple: Color,
    pub cyan: Color,
    pub magenta: Color,
}

impl Default for ThemeBaseColor {
    fn default() -> Self {
        Self {
            white: Color::rgb8(0, 0, 0),
            black: Color::rgb8(0, 0, 0),
            grey: Color::rgb8(0, 0, 0),
            blue: Color::rgb8(0, 0, 0),
            red: Color::rgb8(0, 0, 0),
            yellow: Color::rgb8(0, 0, 0),
            orange: Color::rgb8(0, 0, 0),
            green: Color::rgb8(0, 0, 0),
            purple: Color::rgb8(0, 0, 0),
            cyan: Color::rgb8(0, 0, 0),
            magenta: Color::rgb8(0, 0, 0),
        }
    }
}

impl ThemeBaseColor {
    pub fn get(&self, name: &str) -> Option<&Color> {
        Some(match name {
            "white" => &self.white,
            "black" => &self.black,
            "grey" => &self.grey,
            "blue" => &self.blue,
            "red" => &self.red,
            "yellow" => &self.yellow,
            "orange" => &self.orange,
            "green" => &self.green,
            "purple" => &self.purple,
            "cyan" => &self.cyan,
            "magenta" => &self.magenta,
            _ => return None,
        })
    }

    pub fn keys(&self) -> [&'static str; 11] {
        [
            "white", "black", "grey", "blue", "red", "yellow", "orange", "green",
            "purple", "cyan", "magenta",
        ]
    }
}

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct LapceConfig {
    #[serde(skip)]
    pub id: u64,
    pub core: CoreConfig,
    pub ui: UIConfig,
    pub editor: EditorConfig,
    pub terminal: TerminalConfig,
    pub color_theme: ColorThemeConfig,
    pub icon_theme: IconThemeConfig,
    #[serde(flatten)]
    pub plugins: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(skip)]
    pub default_color_theme: ColorThemeConfig,
    #[serde(skip)]
    pub default_icon_theme: IconThemeConfig,
    #[serde(skip)]
    pub color: ThemeColor,
    #[serde(skip)]
    pub available_color_themes: HashMap<String, (String, config::Config)>,
    #[serde(skip)]
    pub available_icon_themes:
        HashMap<String, (String, config::Config, Option<PathBuf>)>,
    #[serde(skip)]
    tab_layout_info: Arc<RwLock<HashMap<(FontFamily, usize), f64>>>,
    #[serde(skip)]
    svg_store: Arc<RwLock<SvgStore>>,
}

pub struct ConfigWatcher {
    event_sink: ExtEventSink,
    delay_handler: Arc<Mutex<Option<()>>>,
}

impl ConfigWatcher {
    pub fn new(event_sink: ExtEventSink) -> Self {
        Self {
            event_sink,
            delay_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(event) = event {
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    *self.delay_handler.lock() = Some(());
                    let delay_handler = self.delay_handler.clone();
                    let event_sink = self.event_sink.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if delay_handler.lock().take().is_some() {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ReloadConfig,
                                Target::Auto,
                            );
                        }
                    });
                }
                _ => (),
            }
        }
    }
}

impl LapceConfig {
    pub fn load(workspace: &LapceWorkspace, disabled_volts: &[String]) -> Self {
        let config = Self::merge_config(workspace, None, None);
        let mut lapce_config: LapceConfig = config
            .try_deserialize()
            .unwrap_or_else(|_| DEFAULT_LAPCE_CONFIG.clone());

        lapce_config.available_color_themes =
            Self::load_color_themes(disabled_volts);
        lapce_config.available_icon_themes = Self::load_icon_themes(disabled_volts);
        lapce_config.resolve_theme(workspace);
        lapce_config
    }

    fn resolve_theme(&mut self, workspace: &LapceWorkspace) {
        let mut default_lapce_config = DEFAULT_LAPCE_CONFIG.clone();
        if let Some((_, color_theme_config)) = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
        {
            if let Ok(mut theme_lapce_config) = config::Config::builder()
                .add_source(DEFAULT_CONFIG.clone())
                .add_source(color_theme_config.clone())
                .build()
                .and_then(|theme| theme.try_deserialize::<LapceConfig>())
            {
                theme_lapce_config.resolve_colors(Some(&default_lapce_config));
                default_lapce_config = theme_lapce_config;
            }
        }

        let color_theme_config = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
            .map(|(_, config)| config);

        let icon_theme_config = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, config, _)| config);

        let icon_theme_path = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, _, path)| path);

        if color_theme_config.is_some() || icon_theme_config.is_some() {
            if let Ok(new) = Self::merge_config(
                workspace,
                color_theme_config.cloned(),
                icon_theme_config.cloned(),
            )
            .try_deserialize::<LapceConfig>()
            {
                self.core = new.core;
                self.ui = new.ui;
                self.editor = new.editor;
                self.terminal = new.terminal;
                self.color_theme = new.color_theme;
                self.icon_theme = new.icon_theme;
                if let Some(icon_theme_path) = icon_theme_path {
                    self.icon_theme.path =
                        icon_theme_path.clone().unwrap_or_default();
                }
                self.plugins = new.plugins;
            }
        }
        self.resolve_colors(Some(&default_lapce_config));
        self.default_color_theme = default_lapce_config.color_theme.clone();
        self.default_icon_theme = default_lapce_config.icon_theme.clone();
        self.update_id();
    }

    fn merge_config(
        workspace: &LapceWorkspace,
        color_theme_config: Option<config::Config>,
        icon_theme_config: Option<config::Config>,
    ) -> config::Config {
        let mut config = DEFAULT_CONFIG.clone();
        if let Some(theme) = color_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(theme) = icon_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(path) = Self::settings_file() {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(config::File::from(path.as_path()).required(false))
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        match workspace.kind {
            LapceWorkspaceType::Local => {
                if let Some(path) = workspace.path.as_ref() {
                    let path = path.join("./.lapce/settings.toml");
                    config = config::Config::builder()
                        .add_source(config.clone())
                        .add_source(
                            config::File::from(path.as_path()).required(false),
                        )
                        .build()
                        .unwrap_or_else(|_| config.clone());
                }
            }
            LapceWorkspaceType::RemoteSSH(_) => {}
            LapceWorkspaceType::RemoteWSL => {}
        }

        config
    }

    fn resolve_colors(&mut self, default_config: Option<&LapceConfig>) {
        self.color.base = self
            .color_theme
            .base
            .resolve(default_config.map(|c| &c.color.base));
        self.color.ui = self
            .color_theme
            .resolve_ui_color(&self.color.base, default_config.map(|c| &c.color.ui));
        self.color.syntax = self.color_theme.resolve_syntax_color(
            &self.color.base,
            default_config.map(|c| &c.color.syntax),
        );

        let fg = self
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .as_rgba();
        let bg = self
            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
            .as_rgba();
        let is_light = fg.0 + fg.1 + fg.2 > bg.0 + bg.1 + bg.2;
        let high_contrast = self.color_theme.high_contrast.unwrap_or(false);
        self.color.color_preference = match (is_light, high_contrast) {
            (true, true) => ThemeColorPreference::HighContrastLight,
            (false, true) => ThemeColorPreference::HighContrastDark,
            (true, false) => ThemeColorPreference::Light,
            (false, false) => ThemeColorPreference::Dark,
        };
    }

    fn load_color_themes(
        disabled_volts: &[String],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes = Self::load_local_themes().unwrap_or_default();

        for (key, theme) in Self::load_plugin_color_themes(disabled_volts) {
            themes.insert(key, theme);
        }

        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_LIGHT_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_DARK_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));

        themes
    }

    fn load_icon_themes(
        disabled_volts: &[String],
    ) -> HashMap<String, (String, config::Config, Option<PathBuf>)> {
        let mut themes = HashMap::new();

        for (key, (name, theme, path)) in
            Self::load_plugin_icon_themes(disabled_volts)
        {
            themes.insert(key, (name, theme, Some(path)));
        }

        let (name, theme) =
            Self::load_icon_theme_from_str(DEFAULT_ICON_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme, None));

        themes
    }

    fn load_local_themes() -> Option<HashMap<String, (String, config::Config)>> {
        let themes_folder = Directory::themes_directory()?;
        let themes: HashMap<String, (String, config::Config)> =
            std::fs::read_dir(themes_folder)
                .ok()?
                .filter_map(|entry| {
                    entry
                        .ok()
                        .and_then(|entry| Self::load_color_theme(&entry.path()))
                })
                .collect();
        Some(themes)
    }

    fn load_plugin_color_themes(
        disabled_volts: &[String],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_all_volts() {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.color_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
                        Self::load_color_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_plugin_icon_themes(
        disabled_volts: &[String],
    ) -> HashMap<String, (String, config::Config, PathBuf)> {
        let mut themes: HashMap<String, (String, config::Config, PathBuf)> =
            HashMap::new();
        for meta in find_all_volts() {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.icon_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
                        Self::load_icon_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_color_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_icon_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_color_theme(path: &Path) -> Option<(String, (String, config::Config))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name.to_lowercase(), (name, config)))
    }

    fn load_icon_theme(
        path: &Path,
    ) -> Option<(String, (String, config::Config, PathBuf))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((
            name.to_lowercase(),
            (name, config, path.parent().unwrap().to_path_buf()),
        ))
    }

    fn default_config() -> config::Config {
        config::Config::builder()
            .add_source(config::File::from_str(
                DEFAULT_SETTINGS,
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
    }

    fn default_lapce_config() -> LapceConfig {
        let mut default_lapce_config: LapceConfig =
            DEFAULT_CONFIG.clone().try_deserialize().unwrap();
        default_lapce_config.resolve_colors(None);
        default_lapce_config
    }

    pub fn export_theme(&self) -> String {
        let mut table = toml::value::Table::new();
        let mut theme = self.color_theme.clone();
        theme.name = "".to_string();
        theme.syntax.sort_keys();
        theme.ui.sort_keys();
        table.insert(
            "color-theme".to_string(),
            toml::Value::try_from(&theme).unwrap(),
        );
        table.insert("ui".to_string(), toml::Value::try_from(&self.ui).unwrap());
        let value = toml::Value::Table(table);
        toml::to_string_pretty(&value).unwrap()
    }

    pub fn keymaps_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("keymaps.toml");

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    pub fn log_file() -> Option<PathBuf> {
        let time = chrono::Local::now().format("%Y%m%d-%H%M%S");

        let file_name = format!("{time}.log");

        let path = Directory::logs_directory()?.join(file_name);

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    pub fn settings_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("settings.toml");

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    fn get_file_table() -> Option<toml_edit::Document> {
        let path = Self::settings_file()?;
        let content = std::fs::read_to_string(path).ok()?;
        let document: toml_edit::Document = content.parse().ok()?;
        Some(document)
    }

    pub fn reset_setting(parent: &str, key: &str) -> Option<()> {
        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        table.remove(key);

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }

    pub fn update_file(
        parent: &str,
        key: &str,
        value: toml_edit::Value,
    ) -> Option<()> {
        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        // Update key
        table.insert(key, toml_edit::Item::Value(value));

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }

    fn update_id(&mut self) {
        self.id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    pub fn set_color_theme(
        &mut self,
        workspace: &LapceWorkspace,
        theme: &str,
        preview: bool,
    ) {
        self.core.color_theme = theme.to_string();
        self.resolve_theme(workspace);
        if !preview {
            LapceConfig::update_file(
                "core",
                "color-theme",
                toml_edit::Value::from(theme),
            );
        }
    }

    pub fn set_icon_theme(
        &mut self,
        workspace: &LapceWorkspace,
        theme: &str,
        preview: bool,
    ) {
        self.core.icon_theme = theme.to_string();
        self.resolve_theme(workspace);
        if !preview {
            LapceConfig::update_file(
                "core",
                "icon-theme",
                toml_edit::Value::from(theme),
            );
        }
    }

    /// Get the color by the name from the current theme if it exists
    /// Otherwise, get the color from the base them
    /// # Panics
    /// If the color was not able to be found in either theme, which may be indicative that
    /// it is misspelled or needs to be added to the base-theme.
    pub fn get_color_unchecked(&self, name: &str) -> &Color {
        self.color
            .ui
            .get(name)
            .unwrap_or_else(|| panic!("Key not found: {name}"))
    }

    pub fn get_hover_color(&self, color: &Color) -> Color {
        let (r, g, b, a) = color.as_rgba();
        let shift = 0.05;
        match self.color.color_preference {
            ThemeColorPreference::Dark => {
                Color::rgba(r + -shift, g + -shift, b + -shift, a)
            }
            ThemeColorPreference::Light => {
                Color::rgba(r + shift, g + shift, b + shift, a)
            }
            ThemeColorPreference::HighContrastDark => {
                let shift = shift * 2.0;
                Color::rgba(r + -shift, g + -shift, b + -shift, a)
            }
            ThemeColorPreference::HighContrastLight => {
                let shift = shift * 2.0;
                Color::rgba(r + shift, g + shift, b + shift, a)
            }
        }
    }

    /// Retrieve a color value whose key starts with "style."
    pub fn get_style_color(&self, name: &str) -> Option<&Color> {
        self.color.syntax.get(name)
    }

    /// Calculate the width of the character "W" (being the widest character)
    /// in the editor's current font family at the specified font size.
    pub fn char_width(
        &self,
        text: &mut PietText,
        font_size: f64,
        font_family: FontFamily,
    ) -> f64 {
        Self::text_size_internal(font_family, font_size, text, "W").width
    }

    pub fn terminal_font_family(&self) -> FontFamily {
        if self.terminal.font_family.is_empty() {
            self.editor.font_family()
        } else {
            FontFamily::new_unchecked(self.terminal.font_family.clone())
        }
    }

    pub fn terminal_font_size(&self) -> usize {
        if self.terminal.font_size > 0 {
            self.terminal.font_size
        } else {
            self.editor.font_size
        }
    }

    pub fn terminal_line_height(&self) -> usize {
        if self.terminal.line_height > 0 {
            self.terminal.line_height
        } else {
            self.editor.line_height()
        }
    }

    /// Calculate the width of the character "W" (being the widest character)
    /// in the editor's current font family and current font size.
    pub fn editor_char_width(&self, text: &mut PietText) -> f64 {
        self.char_width(
            text,
            self.editor.font_size as f64,
            self.editor.font_family(),
        )
    }

    /// Calculate the width of `text_to_measure` in the editor's current font family and font size.
    pub fn editor_text_width(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> f64 {
        Self::text_size_internal(
            self.editor.font_family(),
            self.editor.font_size as f64,
            text,
            text_to_measure,
        )
        .width
    }

    /// Calculate the size of `text_to_measure` in the editor's current font family and font size.
    pub fn editor_text_size(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> Size {
        Self::text_size_internal(
            self.editor.font_family(),
            self.editor.font_size as f64,
            text,
            text_to_measure,
        )
    }

    /// Calculate the width of the character "W" (being the widest character)
    /// in the editor's current font family and current font size.
    pub fn terminal_char_width(&self, text: &mut PietText) -> f64 {
        self.char_width(
            text,
            self.terminal_font_size() as f64,
            self.terminal_font_family(),
        )
    }

    /// Calculate the width of `text_to_measure` in the editor's current font family and font size.
    pub fn terminal_text_width(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> f64 {
        Self::text_size_internal(
            self.terminal_font_family(),
            self.terminal_font_size() as f64,
            text,
            text_to_measure,
        )
        .width
    }

    /// Calculate the size of `text_to_measure` in the terminal's current font family and font size.
    pub fn terminal_text_size(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> Size {
        Self::text_size_internal(
            self.terminal_font_family(),
            self.terminal_font_size() as f64,
            text,
            text_to_measure,
        )
    }

    /// Efficiently calculate the size of a piece of text, without allocating.
    /// This function should not be made public, use one of the public wrapper functions instead.
    fn text_size_internal(
        font_family: FontFamily,
        font_size: f64,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> Size {
        // Lie about the lifetime of `text_to_measure`.
        //
        // The method `new_text_layout` will take ownership of its parameter
        // and hold it inside the `text_layout` object. It will normally only do this efficiently
        // for `&'static str`, and not other `&'a str`. It can safely do this for static strings
        // because they are known to live for the lifetime of the program.
        //
        // `new_text_layout` can also work by taking ownership of an owned type such
        // as String, Rc, or Arc. But they all require allocation. We want to measure
        // `strs` with arbitrary lifetimes. If we 'cheat' by extending the lifetime of the
        // `text_to_measure` (in this function only) then we can safely call `new_text_layout`
        // because the `text_layout` value that is produced is local to this function and hence
        // always dropped inside this function, and hence its lifetime is always strictly less
        // than the lifetime of `text_to_measure`, irrespective of whether `text_to_measure`
        // is actually static or not.
        //
        // Note that this technique also assumes that `new_text_layout` does not stash
        // its parameter away somewhere, such as a global cache. If it did, this would
        // break and we would have to go back to calling `to_string` on the parameter.
        let static_str: &'static str =
            unsafe { std::mem::transmute(text_to_measure) };

        let text_layout = text
            .new_text_layout(static_str)
            .font(font_family, font_size)
            .build()
            .unwrap();

        text_layout.size()
    }

    pub fn tab_width(
        &self,
        text: &mut PietText,
        font_family: FontFamily,
        font_size: usize,
    ) -> f64 {
        {
            let info = self.tab_layout_info.read();
            if let Some(width) = info.get(&(font_family.clone(), font_size)) {
                return self.editor.tab_width as f64 * *width;
            };
        }

        let width = text
            .new_text_layout(" a")
            .font(font_family.clone(), font_size as f64)
            .build()
            .unwrap()
            .hit_test_text_position(1)
            .point
            .x;

        self.tab_layout_info
            .write()
            .insert((font_family, font_size), width);
        self.editor.tab_width as f64 * width
    }

    pub fn logo_svg(&self) -> Svg {
        self.svg_store.read().logo_svg()
    }

    pub fn ui_svg(&self, icon: &'static str) -> Svg {
        let svg = self.icon_theme.ui.get(icon).and_then(|path| {
            let path = self.icon_theme.path.join(path);
            self.svg_store.write().get_svg_on_disk(&path)
        });

        svg.unwrap_or_else(|| {
            let name = self.default_icon_theme.ui.get(icon).unwrap();
            self.svg_store.write().get_default_svg(name)
        })
    }

    pub fn folder_svg(&self, path: &Path) -> Option<(Svg, Option<&Color>)> {
        self.icon_theme
            .foldername
            .get_key_value(
                path.file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default(),
            )
            .and_then(|(_, path)| {
                let path = self.icon_theme.path.join(path);
                self.svg_store.write().get_svg_on_disk(&path)
            })
            .map(|svg| {
                let color = if self.icon_theme.use_editor_color.unwrap_or(false) {
                    Some(self.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE))
                } else {
                    None
                };
                (svg, color)
            })
    }

    pub fn file_svg(&self, path: &Path) -> (Svg, Option<&Color>) {
        let svg = self
            .icon_theme
            .resolve_path_to_icon(path)
            .and_then(|p| self.svg_store.write().get_svg_on_disk(&p));
        if let Some(svg) = svg {
            let color = if self.icon_theme.use_editor_color.unwrap_or(false) {
                Some(self.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE))
            } else {
                None
            };
            (svg, color)
        } else {
            (
                self.ui_svg(LapceIcons::FILE),
                Some(self.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
            )
        }
    }

    pub fn symbol_svg(&self, kind: &SymbolKind) -> Option<Svg> {
        let kind_str = match *kind {
            SymbolKind::ARRAY => LapceIcons::SYMBOL_KIND_ARRAY,
            SymbolKind::BOOLEAN => LapceIcons::SYMBOL_KIND_BOOLEAN,
            SymbolKind::CLASS => LapceIcons::SYMBOL_KIND_CLASS,
            SymbolKind::CONSTANT => LapceIcons::SYMBOL_KIND_CONSTANT,
            SymbolKind::ENUM_MEMBER => LapceIcons::SYMBOL_KIND_ENUM_MEMBER,
            SymbolKind::ENUM => LapceIcons::SYMBOL_KIND_ENUM,
            SymbolKind::EVENT => LapceIcons::SYMBOL_KIND_EVENT,
            SymbolKind::FIELD => LapceIcons::SYMBOL_KIND_FIELD,
            SymbolKind::FILE => LapceIcons::SYMBOL_KIND_FILE,
            SymbolKind::INTERFACE => LapceIcons::SYMBOL_KIND_INTERFACE,
            SymbolKind::KEY => LapceIcons::SYMBOL_KIND_KEY,
            SymbolKind::FUNCTION => LapceIcons::SYMBOL_KIND_FUNCTION,
            SymbolKind::METHOD => LapceIcons::SYMBOL_KIND_METHOD,
            SymbolKind::OBJECT => LapceIcons::SYMBOL_KIND_OBJECT,
            SymbolKind::NAMESPACE => LapceIcons::SYMBOL_KIND_NAMESPACE,
            SymbolKind::NUMBER => LapceIcons::SYMBOL_KIND_NUMBER,
            SymbolKind::OPERATOR => LapceIcons::SYMBOL_KIND_OPERATOR,
            SymbolKind::TYPE_PARAMETER => LapceIcons::SYMBOL_KIND_TYPE_PARAMETER,
            SymbolKind::PROPERTY => LapceIcons::SYMBOL_KIND_PROPERTY,
            SymbolKind::STRING => LapceIcons::SYMBOL_KIND_STRING,
            SymbolKind::STRUCT => LapceIcons::SYMBOL_KIND_STRUCT,
            SymbolKind::VARIABLE => LapceIcons::SYMBOL_KIND_VARIABLE,
            _ => return None,
        };

        Some(self.ui_svg(kind_str))
    }

    pub fn completion_svg(
        &self,
        kind: Option<CompletionItemKind>,
    ) -> Option<(Svg, Option<Color>)> {
        let kind = kind?;
        let kind_str = match kind {
            CompletionItemKind::METHOD => LapceIcons::COMPLETION_ITEM_KIND_METHOD,
            CompletionItemKind::FUNCTION => {
                LapceIcons::COMPLETION_ITEM_KIND_FUNCTION
            }
            CompletionItemKind::ENUM => LapceIcons::COMPLETION_ITEM_KIND_ENUM,
            CompletionItemKind::ENUM_MEMBER => {
                LapceIcons::COMPLETION_ITEM_KIND_ENUM_MEMBER
            }
            CompletionItemKind::CLASS => LapceIcons::COMPLETION_ITEM_KIND_CLASS,
            CompletionItemKind::VARIABLE => LapceIcons::SYMBOL_KIND_VARIABLE,
            CompletionItemKind::STRUCT => LapceIcons::SYMBOL_KIND_STRUCT,
            CompletionItemKind::KEYWORD => LapceIcons::COMPLETION_ITEM_KIND_KEYWORD,
            CompletionItemKind::CONSTANT => {
                LapceIcons::COMPLETION_ITEM_KIND_CONSTANT
            }
            CompletionItemKind::PROPERTY => {
                LapceIcons::COMPLETION_ITEM_KIND_PROPERTY
            }
            CompletionItemKind::FIELD => LapceIcons::COMPLETION_ITEM_KIND_FIELD,
            CompletionItemKind::INTERFACE => {
                LapceIcons::COMPLETION_ITEM_KIND_INTERFACE
            }
            CompletionItemKind::SNIPPET => LapceIcons::COMPLETION_ITEM_KIND_SNIPPET,
            CompletionItemKind::MODULE => LapceIcons::COMPLETION_ITEM_KIND_MODULE,
            _ => LapceIcons::COMPLETION_ITEM_KIND_STRING,
        };
        let theme_str = match kind {
            CompletionItemKind::METHOD => "method",
            CompletionItemKind::FUNCTION => "method",
            CompletionItemKind::ENUM => "enum",
            CompletionItemKind::ENUM_MEMBER => "enum-member",
            CompletionItemKind::CLASS => "class",
            CompletionItemKind::VARIABLE => "field",
            CompletionItemKind::STRUCT => "structure",
            CompletionItemKind::KEYWORD => "keyword",
            CompletionItemKind::CONSTANT => "constant",
            CompletionItemKind::PROPERTY => "property",
            CompletionItemKind::FIELD => "field",
            CompletionItemKind::INTERFACE => "interface",
            CompletionItemKind::SNIPPET => "snippet",
            CompletionItemKind::MODULE => "builtinType",
            _ => "string",
        };

        Some((
            self.ui_svg(kind_str),
            self.get_style_color(theme_str).cloned(),
        ))
    }
}
