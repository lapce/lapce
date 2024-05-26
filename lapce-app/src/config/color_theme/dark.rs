use std::{collections::HashMap, path::PathBuf};

use floem::peniko::Color;
use once_cell::sync::Lazy;

use super::{ColorThemeConfig, ThemeBaseConfig};

pub static THEME: Lazy<ColorThemeConfig> = Lazy::new(theme);

fn theme() -> ColorThemeConfig {
    ColorThemeConfig {
        path: PathBuf::new(),
        name: String::from("Lapce Dark"),
        high_contrast: None,
        base: ThemeBaseConfig(
            BASE.iter()
                .map(|i| (i.0.to_owned(), i.1.to_owned()))
                .collect(),
        ),
        syntax: SYNTAX
            .iter()
            .map(|i| (i.0.to_owned(), i.1.to_owned()))
            .collect(),
        ui: UI
            .iter()
            .map(|i| (i.0.to_owned(), i.1.to_owned()))
            .collect(),
    }
}

pub static BASE_THEME: Lazy<HashMap<String, Color>> = Lazy::new(|| {
    BASE_RESOLVED
        .iter()
        .map(|i| (i.0.to_owned(), i.1))
        .collect()
});

pub struct Base {}

impl Base {
    /// Hex: #ABB2BF
    pub const WHITE: Color = Color::rgba8(171, 178, 191, 1);
    /// Hex: #282C34
    pub const BLACK: Color = Color::rgba8(40, 44, 52, 1);
    /// Hex: 61AFEF
    pub const BLUE: Color = Color::rgba8(97, 175, 239, 1);
    /// Hex: 56B6C2
    pub const CYAN: Color = Color::rgba8(86, 182, 194, 1);
    /// Hex: 98C379
    pub const GREEN: Color = Color::rgba8(152, 195, 121, 1);
    /// Hex: 3E4451
    pub const GREY: Color = Color::rgba8(62, 68, 81, 1);
    /// Hex: C678DD
    pub const MAGENTA: Color = Color::rgba8(198, 120, 221, 1);
    /// Hex: D19A66
    pub const ORANGE: Color = Color::rgba8(209, 154, 102, 1);
    /// Hex: C678DD
    pub const PURPLE: Color = Color::rgba8(198, 120, 221, 1);
    /// Hex: E06C75
    pub const RED: Color = Color::rgba8(224, 108, 117, 1);
    /// Hex: #E5C07B
    pub const YELLOW: Color = Color::rgba8(229, 192, 123, 1);
}

const BASE_RESOLVED: [(&str, Color); 16] = [
    ("white", Base::WHITE),
    ("black", Base::BLACK),
    ("blue", Base::BLUE),
    ("cyan", Base::CYAN),
    ("green", Base::GREEN),
    ("grey", Base::GREY),
    ("magenta", Base::MAGENTA),
    ("orange", Base::ORANGE),
    ("purple", Base::PURPLE),
    ("red", Base::RED),
    ("yellow", Base::YELLOW),
    ("primary-background", Base::BLACK),
    ("secondary-background", Color::rgba8(33, 37, 43, 1)),
    ("current-background", Color::rgba8(44, 49, 58, 1)),
    ("text", Base::WHITE),
    ("dim-text", Color::rgba8(92, 99, 112, 1)),
];

const BASE: [(&str, &str); 16] = [
    ("black", "#282C34"),
    ("blue", "#61AFEF"),
    ("cyan", "#56B6C2"),
    ("green", "#98C379"),
    ("grey", "#3E4451"),
    ("magenta", "#C678DD"),
    ("orange", "#D19A66"),
    ("purple", "#C678DD"),
    ("red", "#E06C75"),
    ("white", "#ABB2BF"),
    ("yellow", "#E5C07B"),
    ("primary-background", "$black"),
    ("secondary-background", "#21252B"),
    ("current-background", "#2C313A"),
    ("text", "$white"),
    ("dim-text", "#5C6370"),
];

const SYNTAX: [(&str, &str); 37] = [
    ("comment", "$dim-text"),
    ("constant", "$yellow"),
    ("type", "$yellow"),
    ("typeAlias", "$yellow"),
    ("number", "$yellow"),
    ("enum", "$yellow"),
    ("struct", "$yellow"),
    ("structure", "$yellow"),
    ("interface", "$yellow"),
    ("attribute", "$yellow"),
    ("constructor", "$yellow"),
    ("function", "$blue"),
    ("method", "$blue"),
    ("function.method", "$blue"),
    ("keyword", "$purple"),
    ("selfKeyword", "$purple"),
    ("field", "$red"),
    ("property", "$red"),
    ("enumMember", "$red"),
    ("enum-member", "$red"),
    ("string", "$green"),
    ("type.builtin", "$cyan"),
    ("builtinType", "$cyan"),
    ("escape", "$cyan"),
    ("string.escape", "$cyan"),
    ("embedded", "$cyan"),
    ("punctuation.delimiter", "$yellow"),
    ("text.title", "$orange"),
    ("text.uri", "$cyan"),
    ("text.reference", "$yellow"),
    ("variable", "$red"),
    ("variable.other.member", "$red"),
    ("tag", "$blue"),
    ("bracket.color.1", "$blue"),
    ("bracket.color.2", "$yellow"),
    ("bracket.color.3", "$purple"),
    ("bracket.unpaired", "$red"),
];

const UI: [(&str, &str); 103] = [
    ("lapce.error", "$red"),
    ("lapce.warn", "$yellow"),
    ("lapce.dropdown_shadow", "#000000"),
    ("lapce.border", "#000000"),
    ("lapce.scroll_bar", "#3E4451BB"),
    ("lapce.button.primary.background", "#50a14f"),
    ("lapce.button.primary.foreground", "$black"),
    ("lapce.tab.active.background", "$primary-background"),
    ("lapce.tab.active.foreground", "$text"),
    ("lapce.tab.active.underline", "#528BFF"),
    ("lapce.tab.inactive.background", "$secondary-background"),
    ("lapce.tab.inactive.foreground", "$text"),
    ("lapce.tab.inactive.underline", "#528BFF77"),
    ("lapce.tab.separator", ""),
    ("lapce.icon.active", "$text"),
    ("lapce.icon.inactive", "$dim-text"),
    ("lapce.remote.icon", "$black"),
    ("lapce.remote.local", "#4078F2"),
    ("lapce.remote.connected", "#50A14F"),
    ("lapce.remote.connecting", "#C18401"),
    ("lapce.remote.disconnected", "#E45649"),
    ("lapce.plugin.name", "#DDDDDD"),
    ("lapce.plugin.description", "$text"),
    ("lapce.plugin.author", "#B0B0B0"),
    ("editor.background", "$primary-background"),
    ("editor.foreground", "$text"),
    ("editor.dim", "$dim-text"),
    ("editor.focus", "#CCCCCC"),
    ("editor.caret", "#528BFF"),
    ("editor.selection", "$grey"),
    ("editor.current_line", "#2C313C"),
    ("editor.debug_break_line", "#528abF37"),
    ("editor.link", "$blue"),
    ("editor.visible_whitespace", "$grey"),
    ("editor.indent_guide", "$grey"),
    ("editor.drag_drop_background", "#79c1fc55"),
    ("editor.drag_drop_tab_background", "#0b0e1455"),
    ("editor.sticky_header_background", "$primary-background"),
    ("inlay_hint.foreground", "$text"),
    ("inlay_hint.background", "#528abF37"),
    ("error_lens.error.foreground", "$red"),
    ("error_lens.error.background", "#E06C7520"),
    ("error_lens.warning.foreground", "$yellow"),
    ("error_lens.warning.background", "#E5C07B20"),
    ("error_lens.other.foreground", "$dim-text"),
    ("error_lens.other.background", "#5C637020"),
    ("completion_lens.foreground", "$dim-text"),
    ("source_control.added", "#50A14FCC"),
    ("source_control.removed", "#FF5266CC"),
    ("source_control.modified", "#0184BCCC"),
    ("tooltip.background", "$primary-background"),
    ("tooltip.foreground", "$text"),
    ("palette.background", "$secondary-background"),
    ("palette.foreground", "$text"),
    ("palette.current.background", "$current-background"),
    ("palette.current.foreground", "$text"),
    ("completion.background", "$secondary-background"),
    ("completion.current", "$current-background"),
    ("hover.background", "$secondary-background"),
    ("activity.background", "$secondary-background"),
    ("activity.current", "$primary-background"),
    ("debug.breakpoint", "$red"),
    ("debug.breakpoint.hover", "#E06C7566"),
    ("panel.background", "$secondary-background"),
    ("panel.foreground", "$text"),
    ("panel.foreground.dim", "$dim-text"),
    ("panel.current.background", "$current-background"),
    ("panel.current.foreground", "$text"),
    ("panel.current.foreground.dim", "$dim-text"),
    ("panel.hovered.background", "#343A45"),
    ("panel.hovered.active.background", "$dim-text"),
    ("panel.hovered.foreground", "$text"),
    ("panel.hovered.foreground.dim", "$dim-text"),
    ("status.background", "$secondary-background"),
    ("status.foreground", "$text"),
    ("status.modal.normal.background", "$blue"),
    ("status.modal.normal.foreground", "$black"),
    ("status.modal.insert.background", "$red"),
    ("status.modal.insert.foreground", "$black"),
    ("status.modal.visual.background", "$yellow"),
    ("status.modal.visual.foreground", "$black"),
    ("status.modal.terminal.background", "$purple"),
    ("status.modal.terminal.foreground", "$black"),
    ("markdown.blockquote", "#898989"),
    ("terminal.cursor", "$text"),
    ("terminal.foreground", "$text"),
    ("terminal.background", "$primary-background"),
    ("terminal.white", "$white"),
    ("terminal.black", "$black"),
    ("terminal.red", "$red"),
    ("terminal.blue", "$blue"),
    ("terminal.green", "$green"),
    ("terminal.yellow", "$yellow"),
    ("terminal.cyan", "$cyan"),
    ("terminal.magenta", "$magenta"),
    ("terminal.bright_white", "#C8CCD4"),
    ("terminal.bright_red", "$red"),
    ("terminal.bright_blue", "$blue"),
    ("terminal.bright_green", "$green"),
    ("terminal.bright_yellow", "$yellow"),
    ("terminal.bright_cyan", "$cyan"),
    ("terminal.bright_magenta", "$magenta"),
    ("terminal.bright_black", "#545862"),
];
