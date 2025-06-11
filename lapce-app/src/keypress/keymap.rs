use std::{fmt::Display, str::FromStr};

use floem::{
    keyboard::{Key, KeyCode, Modifiers, NamedKey, PhysicalKey},
    pointer::{MouseButton, PointerButton},
};
use lapce_core::mode::Modes;

#[derive(PartialEq, Debug, Clone)]
pub enum KeymapMatch {
    Full(String),
    Multiple(Vec<String>),
    Prefix,
    None,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyMapPress>,
    pub modes: Modes,
    pub when: Option<String>,
    pub command: String,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum KeyMapKey {
    Pointer(PointerButton),
    Logical(Key),
    Physical(PhysicalKey),
}

impl std::hash::Hash for KeyMapKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Pointer(btn) => (btn.mouse_button() as u8).hash(state),
            Self::Logical(key) => key.hash(state),
            Self::Physical(physical) => physical.hash(state),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMapPress {
    pub key: KeyMapKey,
    pub mods: Modifiers,
}

impl KeyMapPress {
    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            if let KeyMapKey::Logical(Key::Character(_)) = &self.key {
                return true;
            }
        }
        false
    }

    pub fn is_modifiers(&self) -> bool {
        if let KeyMapKey::Physical(physical) = &self.key {
            matches!(
                physical,
                PhysicalKey::Code(KeyCode::Meta)
                    | PhysicalKey::Code(KeyCode::SuperLeft)
                    | PhysicalKey::Code(KeyCode::SuperRight)
                    | PhysicalKey::Code(KeyCode::ShiftLeft)
                    | PhysicalKey::Code(KeyCode::ShiftRight)
                    | PhysicalKey::Code(KeyCode::ControlLeft)
                    | PhysicalKey::Code(KeyCode::ControlRight)
                    | PhysicalKey::Code(KeyCode::AltLeft)
                    | PhysicalKey::Code(KeyCode::AltRight)
            )
        } else if let KeyMapKey::Logical(Key::Named(key)) = &self.key {
            matches!(
                key,
                NamedKey::Meta
                    | NamedKey::Super
                    | NamedKey::Shift
                    | NamedKey::Control
                    | NamedKey::Alt
                    | NamedKey::AltGraph
            )
        } else {
            false
        }
    }

    pub fn label(&self) -> String {
        let mut keys = String::from("");
        if self.mods.control() {
            keys.push_str("Ctrl+");
        }
        if self.mods.alt() {
            keys.push_str("Alt+");
        }
        if self.mods.altgr() {
            keys.push_str("AltGr+");
        }
        if self.mods.meta() {
            let keyname = match std::env::consts::OS {
                "macos" => "Cmd+",
                "windows" => "Win+",
                _ => "Meta+",
            };
            keys.push_str(keyname);
        }
        if self.mods.shift() {
            keys.push_str("Shift+");
        }
        keys.push_str(&self.key.to_string());
        keys
    }

    pub fn parse(key: &str) -> Vec<Self> {
        key.split(' ')
            .filter_map(|k| {
                let (modifiers, key) = if k == "+" {
                    ("", "+")
                } else if let Some(remaining) = k.strip_suffix("++") {
                    (remaining, "+")
                } else {
                    match k.rsplit_once('+') {
                        Some(pair) => pair,
                        None => ("", k),
                    }
                };

                let key = match key.parse().ok() {
                    Some(key) => key,
                    None => {
                        // Skip past unrecognized key definitions
                        tracing::warn!("Unrecognized key: {key}");
                        return None;
                    }
                };

                let mut mods = Modifiers::empty();
                for part in modifiers.to_lowercase().split('+') {
                    match part {
                        "ctrl" => mods.set(Modifiers::CONTROL, true),
                        "meta" => mods.set(Modifiers::META, true),
                        "shift" => mods.set(Modifiers::SHIFT, true),
                        "alt" => mods.set(Modifiers::ALT, true),
                        "altgr" => mods.set(Modifiers::ALTGR, true),
                        "" => (),
                        other => tracing::warn!("Invalid key modifier: {}", other),
                    }
                }

                Some(KeyMapPress { key, mods })
            })
            .collect()
    }
}

impl FromStr for KeyMapKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = if s.starts_with('[') && s.ends_with(']') {
            let code = match s[1..s.len() - 2].to_lowercase().as_str() {
                "esc" => KeyCode::Escape,
                "space" => KeyCode::Space,
                "bs" => KeyCode::Backspace,
                "up" => KeyCode::ArrowUp,
                "down" => KeyCode::ArrowDown,
                "left" => KeyCode::ArrowLeft,
                "right" => KeyCode::ArrowRight,
                "del" => KeyCode::Delete,
                "alt" => KeyCode::AltLeft,
                "altgraph" => KeyCode::AltRight,
                "capslock" => KeyCode::CapsLock,
                "control" => KeyCode::ControlLeft,
                "fn" => KeyCode::Fn,
                "fnlock" => KeyCode::FnLock,
                "meta" => KeyCode::Meta,
                "numlock" => KeyCode::NumLock,
                "scrolllock" => KeyCode::ScrollLock,
                "shift" => KeyCode::ShiftLeft,
                "hyper" => KeyCode::Hyper,
                "super" => KeyCode::Meta,
                "enter" => KeyCode::Enter,
                "tab" => KeyCode::Tab,
                "arrowdown" => KeyCode::ArrowDown,
                "arrowleft" => KeyCode::ArrowLeft,
                "arrowright" => KeyCode::ArrowRight,
                "arrowup" => KeyCode::ArrowUp,
                "end" => KeyCode::End,
                "home" => KeyCode::Home,
                "pagedown" => KeyCode::PageDown,
                "pageup" => KeyCode::PageUp,
                "backspace" => KeyCode::Backspace,
                "copy" => KeyCode::Copy,
                "cut" => KeyCode::Cut,
                "delete" => KeyCode::Delete,
                "insert" => KeyCode::Insert,
                "paste" => KeyCode::Paste,
                "undo" => KeyCode::Undo,
                "again" => KeyCode::Again,
                "contextmenu" => KeyCode::ContextMenu,
                "escape" => KeyCode::Escape,
                "find" => KeyCode::Find,
                "help" => KeyCode::Help,
                "pause" => KeyCode::Pause,
                "play" => KeyCode::MediaPlayPause,
                "props" => KeyCode::Props,
                "select" => KeyCode::Select,
                "eject" => KeyCode::Eject,
                "power" => KeyCode::Power,
                "printscreen" => KeyCode::PrintScreen,
                "wakeup" => KeyCode::WakeUp,
                "convert" => KeyCode::Convert,
                "nonconvert" => KeyCode::NonConvert,
                "hiragana" => KeyCode::Hiragana,
                "katakana" => KeyCode::Katakana,
                "f1" => KeyCode::F1,
                "f2" => KeyCode::F2,
                "f3" => KeyCode::F3,
                "f4" => KeyCode::F4,
                "f5" => KeyCode::F5,
                "f6" => KeyCode::F6,
                "f7" => KeyCode::F7,
                "f8" => KeyCode::F8,
                "f9" => KeyCode::F9,
                "f10" => KeyCode::F10,
                "f11" => KeyCode::F11,
                "f12" => KeyCode::F12,
                "mediastop" => KeyCode::MediaStop,
                "open" => KeyCode::Open,
                _ => {
                    return Err(anyhow::anyhow!(
                        "unrecognized physical key code {}",
                        &s[1..s.len() - 2]
                    ));
                }
            };
            KeyMapKey::Physical(PhysicalKey::Code(code))
        } else {
            let key = match s.to_lowercase().as_str() {
                "esc" => Key::Named(NamedKey::Escape),
                "space" => Key::Named(NamedKey::Space),
                "bs" => Key::Named(NamedKey::Backspace),
                "up" => Key::Named(NamedKey::ArrowUp),
                "down" => Key::Named(NamedKey::ArrowDown),
                "left" => Key::Named(NamedKey::ArrowLeft),
                "right" => Key::Named(NamedKey::ArrowRight),
                "del" => Key::Named(NamedKey::Delete),
                "alt" => Key::Named(NamedKey::Alt),
                "altgraph" => Key::Named(NamedKey::AltGraph),
                "capslock" => Key::Named(NamedKey::CapsLock),
                "control" => Key::Named(NamedKey::Control),
                "fn" => Key::Named(NamedKey::Fn),
                "fnlock" => Key::Named(NamedKey::FnLock),
                "meta" => Key::Named(NamedKey::Meta),
                "numlock" => Key::Named(NamedKey::NumLock),
                "scrolllock" => Key::Named(NamedKey::ScrollLock),
                "shift" => Key::Named(NamedKey::Shift),
                "hyper" => Key::Named(NamedKey::Hyper),
                "super" => Key::Named(NamedKey::Meta),
                "enter" => Key::Named(NamedKey::Enter),
                "tab" => Key::Named(NamedKey::Tab),
                "arrowdown" => Key::Named(NamedKey::ArrowDown),
                "arrowleft" => Key::Named(NamedKey::ArrowLeft),
                "arrowright" => Key::Named(NamedKey::ArrowRight),
                "arrowup" => Key::Named(NamedKey::ArrowUp),
                "end" => Key::Named(NamedKey::End),
                "home" => Key::Named(NamedKey::Home),
                "pagedown" => Key::Named(NamedKey::PageDown),
                "pageup" => Key::Named(NamedKey::PageUp),
                "backspace" => Key::Named(NamedKey::Backspace),
                "copy" => Key::Named(NamedKey::Copy),
                "cut" => Key::Named(NamedKey::Cut),
                "delete" => Key::Named(NamedKey::Delete),
                "insert" => Key::Named(NamedKey::Insert),
                "paste" => Key::Named(NamedKey::Paste),
                "undo" => Key::Named(NamedKey::Undo),
                "again" => Key::Named(NamedKey::Again),
                "contextmenu" => Key::Named(NamedKey::ContextMenu),
                "escape" => Key::Named(NamedKey::Escape),
                "find" => Key::Named(NamedKey::Find),
                "help" => Key::Named(NamedKey::Help),
                "pause" => Key::Named(NamedKey::Pause),
                "play" => Key::Named(NamedKey::MediaPlayPause),
                "props" => Key::Named(NamedKey::Props),
                "select" => Key::Named(NamedKey::Select),
                "eject" => Key::Named(NamedKey::Eject),
                "power" => Key::Named(NamedKey::Power),
                "printscreen" => Key::Named(NamedKey::PrintScreen),
                "wakeup" => Key::Named(NamedKey::WakeUp),
                "convert" => Key::Named(NamedKey::Convert),
                "nonconvert" => Key::Named(NamedKey::NonConvert),
                "hiragana" => Key::Named(NamedKey::Hiragana),
                "katakana" => Key::Named(NamedKey::Katakana),
                "f1" => Key::Named(NamedKey::F1),
                "f2" => Key::Named(NamedKey::F2),
                "f3" => Key::Named(NamedKey::F3),
                "f4" => Key::Named(NamedKey::F4),
                "f5" => Key::Named(NamedKey::F5),
                "f6" => Key::Named(NamedKey::F6),
                "f7" => Key::Named(NamedKey::F7),
                "f8" => Key::Named(NamedKey::F8),
                "f9" => Key::Named(NamedKey::F9),
                "f10" => Key::Named(NamedKey::F10),
                "f11" => Key::Named(NamedKey::F11),
                "f12" => Key::Named(NamedKey::F12),
                "mediastop" => Key::Named(NamedKey::MediaStop),
                "open" => Key::Named(NamedKey::Open),
                _ => Key::Character(s.to_lowercase().into()),
            };
            KeyMapKey::Logical(key)
        };
        Ok(key)
    }
}

impl Display for KeyMapPress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.contains(Modifiers::CONTROL) {
            if let Err(err) = f.write_str("Ctrl+") {
                tracing::error!("{:?}", err);
            }
        }
        if self.mods.contains(Modifiers::ALT) {
            if let Err(err) = f.write_str("Alt+") {
                tracing::error!("{:?}", err);
            }
        }
        if self.mods.contains(Modifiers::ALTGR) {
            if let Err(err) = f.write_str("AltGr+") {
                tracing::error!("{:?}", err);
            }
        }
        if self.mods.contains(Modifiers::META) {
            if let Err(err) = f.write_str("Meta+") {
                tracing::error!("{:?}", err);
            }
        }
        if self.mods.contains(Modifiers::SHIFT) {
            if let Err(err) = f.write_str("Shift+") {
                tracing::error!("{:?}", err);
            }
        }
        f.write_str(&self.key.to_string())
    }
}

impl Display for KeyMapKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use floem::pointer::PointerButton as B;

        match self {
            Self::Physical(physical) => {
                f.write_str("[")?;
                match physical {
                    PhysicalKey::Unidentified(_) => f.write_str("Unidentified"),
                    PhysicalKey::Code(KeyCode::Backquote) => {
                        f.write_str("Backquote")
                    }
                    PhysicalKey::Code(KeyCode::Backslash) => {
                        f.write_str("Backslash")
                    }
                    PhysicalKey::Code(KeyCode::BracketLeft) => {
                        f.write_str("BracketLeft")
                    }
                    PhysicalKey::Code(KeyCode::BracketRight) => {
                        f.write_str("BracketRight")
                    }
                    PhysicalKey::Code(KeyCode::Comma) => f.write_str("Comma"),
                    PhysicalKey::Code(KeyCode::Digit0) => f.write_str("0"),
                    PhysicalKey::Code(KeyCode::Digit1) => f.write_str("1"),
                    PhysicalKey::Code(KeyCode::Digit2) => f.write_str("2"),
                    PhysicalKey::Code(KeyCode::Digit3) => f.write_str("3"),
                    PhysicalKey::Code(KeyCode::Digit4) => f.write_str("4"),
                    PhysicalKey::Code(KeyCode::Digit5) => f.write_str("5"),
                    PhysicalKey::Code(KeyCode::Digit6) => f.write_str("6"),
                    PhysicalKey::Code(KeyCode::Digit7) => f.write_str("7"),
                    PhysicalKey::Code(KeyCode::Digit8) => f.write_str("8"),
                    PhysicalKey::Code(KeyCode::Digit9) => f.write_str("9"),
                    PhysicalKey::Code(KeyCode::Equal) => f.write_str("Equal"),
                    PhysicalKey::Code(KeyCode::IntlBackslash) => {
                        f.write_str("IntlBackslash")
                    }
                    PhysicalKey::Code(KeyCode::IntlRo) => f.write_str("IntlRo"),
                    PhysicalKey::Code(KeyCode::IntlYen) => f.write_str("IntlYen"),
                    PhysicalKey::Code(KeyCode::KeyA) => f.write_str("A"),
                    PhysicalKey::Code(KeyCode::KeyB) => f.write_str("B"),
                    PhysicalKey::Code(KeyCode::KeyC) => f.write_str("C"),
                    PhysicalKey::Code(KeyCode::KeyD) => f.write_str("D"),
                    PhysicalKey::Code(KeyCode::KeyE) => f.write_str("E"),
                    PhysicalKey::Code(KeyCode::KeyF) => f.write_str("F"),
                    PhysicalKey::Code(KeyCode::KeyG) => f.write_str("G"),
                    PhysicalKey::Code(KeyCode::KeyH) => f.write_str("H"),
                    PhysicalKey::Code(KeyCode::KeyI) => f.write_str("I"),
                    PhysicalKey::Code(KeyCode::KeyJ) => f.write_str("J"),
                    PhysicalKey::Code(KeyCode::KeyK) => f.write_str("K"),
                    PhysicalKey::Code(KeyCode::KeyL) => f.write_str("L"),
                    PhysicalKey::Code(KeyCode::KeyM) => f.write_str("M"),
                    PhysicalKey::Code(KeyCode::KeyN) => f.write_str("N"),
                    PhysicalKey::Code(KeyCode::KeyO) => f.write_str("O"),
                    PhysicalKey::Code(KeyCode::KeyP) => f.write_str("P"),
                    PhysicalKey::Code(KeyCode::KeyQ) => f.write_str("Q"),
                    PhysicalKey::Code(KeyCode::KeyR) => f.write_str("R"),
                    PhysicalKey::Code(KeyCode::KeyS) => f.write_str("S"),
                    PhysicalKey::Code(KeyCode::KeyT) => f.write_str("T"),
                    PhysicalKey::Code(KeyCode::KeyU) => f.write_str("U"),
                    PhysicalKey::Code(KeyCode::KeyV) => f.write_str("V"),
                    PhysicalKey::Code(KeyCode::KeyW) => f.write_str("W"),
                    PhysicalKey::Code(KeyCode::KeyX) => f.write_str("X"),
                    PhysicalKey::Code(KeyCode::KeyY) => f.write_str("Y"),
                    PhysicalKey::Code(KeyCode::KeyZ) => f.write_str("Z"),
                    PhysicalKey::Code(KeyCode::Minus) => f.write_str("Minus"),
                    PhysicalKey::Code(KeyCode::Period) => f.write_str("Period"),
                    PhysicalKey::Code(KeyCode::Quote) => f.write_str("Quote"),
                    PhysicalKey::Code(KeyCode::Semicolon) => {
                        f.write_str("Semicolon")
                    }
                    PhysicalKey::Code(KeyCode::Slash) => f.write_str("Slash"),
                    PhysicalKey::Code(KeyCode::AltLeft) => f.write_str("Alt"),
                    PhysicalKey::Code(KeyCode::AltRight) => f.write_str("Alt"),
                    PhysicalKey::Code(KeyCode::Backspace) => {
                        f.write_str("Backspace")
                    }
                    PhysicalKey::Code(KeyCode::CapsLock) => f.write_str("CapsLock"),
                    PhysicalKey::Code(KeyCode::ContextMenu) => {
                        f.write_str("ContextMenu")
                    }
                    PhysicalKey::Code(KeyCode::ControlLeft) => f.write_str("Ctrl"),
                    PhysicalKey::Code(KeyCode::ControlRight) => f.write_str("Ctrl"),
                    PhysicalKey::Code(KeyCode::Enter) => f.write_str("Enter"),
                    PhysicalKey::Code(KeyCode::SuperLeft) => f.write_str("Meta"),
                    PhysicalKey::Code(KeyCode::SuperRight) => f.write_str("Meta"),
                    PhysicalKey::Code(KeyCode::ShiftLeft) => f.write_str("Shift"),
                    PhysicalKey::Code(KeyCode::ShiftRight) => f.write_str("Shift"),
                    PhysicalKey::Code(KeyCode::Space) => f.write_str("Space"),
                    PhysicalKey::Code(KeyCode::Tab) => f.write_str("Tab"),
                    PhysicalKey::Code(KeyCode::Convert) => f.write_str("Convert"),
                    PhysicalKey::Code(KeyCode::KanaMode) => f.write_str("KanaMode"),
                    PhysicalKey::Code(KeyCode::Lang1) => f.write_str("Lang1"),
                    PhysicalKey::Code(KeyCode::Lang2) => f.write_str("Lang2"),
                    PhysicalKey::Code(KeyCode::Lang3) => f.write_str("Lang3"),
                    PhysicalKey::Code(KeyCode::Lang4) => f.write_str("Lang4"),
                    PhysicalKey::Code(KeyCode::Lang5) => f.write_str("Lang5"),
                    PhysicalKey::Code(KeyCode::NonConvert) => {
                        f.write_str("NonConvert")
                    }
                    PhysicalKey::Code(KeyCode::Delete) => f.write_str("Delete"),
                    PhysicalKey::Code(KeyCode::End) => f.write_str("End"),
                    PhysicalKey::Code(KeyCode::Help) => f.write_str("Help"),
                    PhysicalKey::Code(KeyCode::Home) => f.write_str("Home"),
                    PhysicalKey::Code(KeyCode::Insert) => f.write_str("Insert"),
                    PhysicalKey::Code(KeyCode::PageDown) => f.write_str("PageDown"),
                    PhysicalKey::Code(KeyCode::PageUp) => f.write_str("PageUp"),
                    PhysicalKey::Code(KeyCode::ArrowDown) => f.write_str("Down"),
                    PhysicalKey::Code(KeyCode::ArrowLeft) => f.write_str("Left"),
                    PhysicalKey::Code(KeyCode::ArrowRight) => f.write_str("Right"),
                    PhysicalKey::Code(KeyCode::ArrowUp) => f.write_str("Up"),
                    PhysicalKey::Code(KeyCode::NumLock) => f.write_str("NumLock"),
                    PhysicalKey::Code(KeyCode::Numpad0) => f.write_str("Numpad0"),
                    PhysicalKey::Code(KeyCode::Numpad1) => f.write_str("Numpad1"),
                    PhysicalKey::Code(KeyCode::Numpad2) => f.write_str("Numpad2"),
                    PhysicalKey::Code(KeyCode::Numpad3) => f.write_str("Numpad3"),
                    PhysicalKey::Code(KeyCode::Numpad4) => f.write_str("Numpad4"),
                    PhysicalKey::Code(KeyCode::Numpad5) => f.write_str("Numpad5"),
                    PhysicalKey::Code(KeyCode::Numpad6) => f.write_str("Numpad6"),
                    PhysicalKey::Code(KeyCode::Numpad7) => f.write_str("Numpad7"),
                    PhysicalKey::Code(KeyCode::Numpad8) => f.write_str("Numpad8"),
                    PhysicalKey::Code(KeyCode::Numpad9) => f.write_str("Numpad9"),
                    PhysicalKey::Code(KeyCode::NumpadAdd) => {
                        f.write_str("NumpadAdd")
                    }
                    PhysicalKey::Code(KeyCode::NumpadBackspace) => {
                        f.write_str("NumpadBackspace")
                    }
                    PhysicalKey::Code(KeyCode::NumpadClear) => {
                        f.write_str("NumpadClear")
                    }
                    PhysicalKey::Code(KeyCode::NumpadClearEntry) => {
                        f.write_str("NumpadClearEntry")
                    }
                    PhysicalKey::Code(KeyCode::NumpadComma) => {
                        f.write_str("NumpadComma")
                    }
                    PhysicalKey::Code(KeyCode::NumpadDecimal) => {
                        f.write_str("NumpadDecimal")
                    }
                    PhysicalKey::Code(KeyCode::NumpadDivide) => {
                        f.write_str("NumpadDivide")
                    }
                    PhysicalKey::Code(KeyCode::NumpadEnter) => {
                        f.write_str("NumpadEnter")
                    }
                    PhysicalKey::Code(KeyCode::NumpadEqual) => {
                        f.write_str("NumpadEqual")
                    }
                    PhysicalKey::Code(KeyCode::NumpadHash) => {
                        f.write_str("NumpadHash")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMemoryAdd) => {
                        f.write_str("NumpadMemoryAdd")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMemoryClear) => {
                        f.write_str("NumpadMemoryClear")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMemoryRecall) => {
                        f.write_str("NumpadMemoryRecall")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMemoryStore) => {
                        f.write_str("NumpadMemoryStore")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMemorySubtract) => {
                        f.write_str("NumpadMemorySubtract")
                    }
                    PhysicalKey::Code(KeyCode::NumpadMultiply) => {
                        f.write_str("NumpadMultiply")
                    }
                    PhysicalKey::Code(KeyCode::NumpadParenLeft) => {
                        f.write_str("NumpadParenLeft")
                    }
                    PhysicalKey::Code(KeyCode::NumpadParenRight) => {
                        f.write_str("NumpadParenRight")
                    }
                    PhysicalKey::Code(KeyCode::NumpadStar) => {
                        f.write_str("NumpadStar")
                    }
                    PhysicalKey::Code(KeyCode::NumpadSubtract) => {
                        f.write_str("NumpadSubtract")
                    }
                    PhysicalKey::Code(KeyCode::Escape) => f.write_str("Escape"),
                    PhysicalKey::Code(KeyCode::Fn) => f.write_str("Fn"),
                    PhysicalKey::Code(KeyCode::FnLock) => f.write_str("FnLock"),
                    PhysicalKey::Code(KeyCode::PrintScreen) => {
                        f.write_str("PrintScreen")
                    }
                    PhysicalKey::Code(KeyCode::ScrollLock) => {
                        f.write_str("ScrollLock")
                    }
                    PhysicalKey::Code(KeyCode::Pause) => f.write_str("Pause"),
                    PhysicalKey::Code(KeyCode::BrowserBack) => {
                        f.write_str("BrowserBack")
                    }
                    PhysicalKey::Code(KeyCode::BrowserFavorites) => {
                        f.write_str("BrowserFavorites")
                    }
                    PhysicalKey::Code(KeyCode::BrowserForward) => {
                        f.write_str("BrowserForward")
                    }
                    PhysicalKey::Code(KeyCode::BrowserHome) => {
                        f.write_str("BrowserHome")
                    }
                    PhysicalKey::Code(KeyCode::BrowserRefresh) => {
                        f.write_str("BrowserRefresh")
                    }
                    PhysicalKey::Code(KeyCode::BrowserSearch) => {
                        f.write_str("BrowserSearch")
                    }
                    PhysicalKey::Code(KeyCode::BrowserStop) => {
                        f.write_str("BrowserStop")
                    }
                    PhysicalKey::Code(KeyCode::Eject) => f.write_str("Eject"),
                    PhysicalKey::Code(KeyCode::LaunchApp1) => {
                        f.write_str("LaunchApp1")
                    }
                    PhysicalKey::Code(KeyCode::LaunchApp2) => {
                        f.write_str("LaunchApp2")
                    }
                    PhysicalKey::Code(KeyCode::LaunchMail) => {
                        f.write_str("LaunchMail")
                    }
                    PhysicalKey::Code(KeyCode::MediaPlayPause) => {
                        f.write_str("MediaPlayPause")
                    }
                    PhysicalKey::Code(KeyCode::MediaSelect) => {
                        f.write_str("MediaSelect")
                    }
                    PhysicalKey::Code(KeyCode::MediaStop) => {
                        f.write_str("MediaStop")
                    }
                    PhysicalKey::Code(KeyCode::MediaTrackNext) => {
                        f.write_str("MediaTrackNext")
                    }
                    PhysicalKey::Code(KeyCode::MediaTrackPrevious) => {
                        f.write_str("MediaTrackPrevious")
                    }
                    PhysicalKey::Code(KeyCode::Power) => f.write_str("Power"),
                    PhysicalKey::Code(KeyCode::Sleep) => f.write_str("Sleep"),
                    PhysicalKey::Code(KeyCode::AudioVolumeDown) => {
                        f.write_str("AudioVolumeDown")
                    }
                    PhysicalKey::Code(KeyCode::AudioVolumeMute) => {
                        f.write_str("AudioVolumeMute")
                    }
                    PhysicalKey::Code(KeyCode::AudioVolumeUp) => {
                        f.write_str("AudioVolumeUp")
                    }
                    PhysicalKey::Code(KeyCode::WakeUp) => f.write_str("WakeUp"),
                    PhysicalKey::Code(KeyCode::Meta) => match std::env::consts::OS {
                        "macos" => f.write_str("Cmd"),
                        "windows" => f.write_str("Win"),
                        _ => f.write_str("Meta"),
                    },
                    PhysicalKey::Code(KeyCode::Hyper) => f.write_str("Hyper"),
                    PhysicalKey::Code(KeyCode::Turbo) => f.write_str("Turbo"),
                    PhysicalKey::Code(KeyCode::Abort) => f.write_str("Abort"),
                    PhysicalKey::Code(KeyCode::Resume) => f.write_str("Resume"),
                    PhysicalKey::Code(KeyCode::Suspend) => f.write_str("Suspend"),
                    PhysicalKey::Code(KeyCode::Again) => f.write_str("Again"),
                    PhysicalKey::Code(KeyCode::Copy) => f.write_str("Copy"),
                    PhysicalKey::Code(KeyCode::Cut) => f.write_str("Cut"),
                    PhysicalKey::Code(KeyCode::Find) => f.write_str("Find"),
                    PhysicalKey::Code(KeyCode::Open) => f.write_str("Open"),
                    PhysicalKey::Code(KeyCode::Paste) => f.write_str("Paste"),
                    PhysicalKey::Code(KeyCode::Props) => f.write_str("Props"),
                    PhysicalKey::Code(KeyCode::Select) => f.write_str("Select"),
                    PhysicalKey::Code(KeyCode::Undo) => f.write_str("Undo"),
                    PhysicalKey::Code(KeyCode::Hiragana) => f.write_str("Hiragana"),
                    PhysicalKey::Code(KeyCode::Katakana) => f.write_str("Katakana"),
                    PhysicalKey::Code(KeyCode::F1) => f.write_str("F1"),
                    PhysicalKey::Code(KeyCode::F2) => f.write_str("F2"),
                    PhysicalKey::Code(KeyCode::F3) => f.write_str("F3"),
                    PhysicalKey::Code(KeyCode::F4) => f.write_str("F4"),
                    PhysicalKey::Code(KeyCode::F5) => f.write_str("F5"),
                    PhysicalKey::Code(KeyCode::F6) => f.write_str("F6"),
                    PhysicalKey::Code(KeyCode::F7) => f.write_str("F7"),
                    PhysicalKey::Code(KeyCode::F8) => f.write_str("F8"),
                    PhysicalKey::Code(KeyCode::F9) => f.write_str("F9"),
                    PhysicalKey::Code(KeyCode::F10) => f.write_str("F10"),
                    PhysicalKey::Code(KeyCode::F11) => f.write_str("F11"),
                    PhysicalKey::Code(KeyCode::F12) => f.write_str("F12"),
                    PhysicalKey::Code(KeyCode::F13) => f.write_str("F13"),
                    PhysicalKey::Code(KeyCode::F14) => f.write_str("F14"),
                    PhysicalKey::Code(KeyCode::F15) => f.write_str("F15"),
                    PhysicalKey::Code(KeyCode::F16) => f.write_str("F16"),
                    PhysicalKey::Code(KeyCode::F17) => f.write_str("F17"),
                    PhysicalKey::Code(KeyCode::F18) => f.write_str("F18"),
                    PhysicalKey::Code(KeyCode::F19) => f.write_str("F19"),
                    PhysicalKey::Code(KeyCode::F20) => f.write_str("F20"),
                    PhysicalKey::Code(KeyCode::F21) => f.write_str("F21"),
                    PhysicalKey::Code(KeyCode::F22) => f.write_str("F22"),
                    PhysicalKey::Code(KeyCode::F23) => f.write_str("F23"),
                    PhysicalKey::Code(KeyCode::F24) => f.write_str("F24"),
                    PhysicalKey::Code(KeyCode::F25) => f.write_str("F25"),
                    PhysicalKey::Code(KeyCode::F26) => f.write_str("F26"),
                    PhysicalKey::Code(KeyCode::F27) => f.write_str("F27"),
                    PhysicalKey::Code(KeyCode::F28) => f.write_str("F28"),
                    PhysicalKey::Code(KeyCode::F29) => f.write_str("F29"),
                    PhysicalKey::Code(KeyCode::F30) => f.write_str("F30"),
                    PhysicalKey::Code(KeyCode::F31) => f.write_str("F31"),
                    PhysicalKey::Code(KeyCode::F32) => f.write_str("F32"),
                    PhysicalKey::Code(KeyCode::F33) => f.write_str("F33"),
                    PhysicalKey::Code(KeyCode::F34) => f.write_str("F34"),
                    PhysicalKey::Code(KeyCode::F35) => f.write_str("F35"),
                    _ => f.write_str("Unidentified"),
                }?;
                f.write_str("]")
            }
            Self::Logical(key) => match key {
                Key::Named(key) => match key {
                    NamedKey::Backspace => f.write_str("Backspace"),
                    NamedKey::CapsLock => f.write_str("CapsLock"),
                    NamedKey::Enter => f.write_str("Enter"),
                    NamedKey::Delete => f.write_str("Delete"),
                    NamedKey::End => f.write_str("End"),
                    NamedKey::Home => f.write_str("Home"),
                    NamedKey::PageDown => f.write_str("PageDown"),
                    NamedKey::PageUp => f.write_str("PageUp"),
                    NamedKey::ArrowDown => f.write_str("ArrowDown"),
                    NamedKey::ArrowUp => f.write_str("ArrowUp"),
                    NamedKey::ArrowLeft => f.write_str("ArrowLeft"),
                    NamedKey::ArrowRight => f.write_str("ArrowRight"),
                    NamedKey::Escape => f.write_str("Escape"),
                    NamedKey::Fn => f.write_str("Fn"),
                    NamedKey::Space => f.write_str("Space"),
                    NamedKey::Shift => f.write_str("Shift"),
                    NamedKey::Meta => f.write_str("Meta"),
                    NamedKey::Super => f.write_str("Meta"),
                    NamedKey::Control => f.write_str("Ctrl"),
                    NamedKey::Alt => f.write_str("Alt"),
                    NamedKey::AltGraph => f.write_str("AltGraph"),
                    NamedKey::Tab => f.write_str("Tab"),
                    NamedKey::F1 => f.write_str("F1"),
                    NamedKey::F2 => f.write_str("F2"),
                    NamedKey::F3 => f.write_str("F3"),
                    NamedKey::F4 => f.write_str("F4"),
                    NamedKey::F5 => f.write_str("F5"),
                    NamedKey::F6 => f.write_str("F6"),
                    NamedKey::F7 => f.write_str("F7"),
                    NamedKey::F8 => f.write_str("F8"),
                    NamedKey::F9 => f.write_str("F9"),
                    NamedKey::F10 => f.write_str("F10"),
                    NamedKey::F11 => f.write_str("F11"),
                    NamedKey::F12 => f.write_str("F12"),
                    _ => f.write_str("Unidentified"),
                },
                Key::Character(s) => f.write_str(s),
                Key::Unidentified(_) => f.write_str("Unidentified"),
                Key::Dead(_) => f.write_str("dead"),
            },
            Self::Pointer(B::Mouse(MouseButton::Auxiliary)) => {
                f.write_str("MouseMiddle")
            }
            Self::Pointer(B::Mouse(MouseButton::X2)) => f.write_str("MouseForward"),
            Self::Pointer(B::Mouse(MouseButton::X1)) => f.write_str("MouseBackward"),
            Self::Pointer(_) => f.write_str("MouseUnimplemented"),
        }
    }
}
