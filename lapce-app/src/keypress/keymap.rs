use std::{fmt::Display, str::FromStr};

use floem::ui_events::{
    keyboard::{Code as KeyCode, Key, Modifiers, NamedKey},
    pointer::PointerButton,
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
    Physical(KeyCode),
}

impl std::hash::Hash for KeyMapKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Pointer(btn) => btn.hash(state),
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
                KeyCode::MetaLeft
                    | KeyCode::MetaRight
                    | KeyCode::ShiftLeft
                    | KeyCode::ShiftRight
                    | KeyCode::ControlLeft
                    | KeyCode::ControlRight
                    | KeyCode::AltLeft
                    | KeyCode::AltRight
            )
        } else if let KeyMapKey::Logical(Key::Named(key)) = &self.key {
            matches!(
                key,
                NamedKey::Meta
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
        if self.mods.ctrl() {
            keys.push_str("Ctrl+");
        }
        if self.mods.alt() {
            keys.push_str("Alt+");
        }
        if self.mods.contains(Modifiers::ALT_GRAPH) {
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
                        "altgr" => mods.set(Modifiers::ALT_GRAPH, true),
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
                "meta" => KeyCode::MetaLeft,
                "numlock" => KeyCode::NumLock,
                "scrolllock" => KeyCode::ScrollLock,
                "shift" => KeyCode::ShiftLeft,
                "super" => KeyCode::MetaLeft,
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
            KeyMapKey::Physical(code)
        } else {
            let key = match s.to_lowercase().as_str() {
                "esc" => Key::Named(NamedKey::Escape),
                "space" => Key::Character(" ".into()),
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
                "hyper" => Key::Named(NamedKey::Meta),
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
                _ => Key::Character(s.to_lowercase()),
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
        if self.mods.contains(Modifiers::ALT_GRAPH) {
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
        match self {
            Self::Physical(physical) => {
                f.write_str("[")?;
                match physical {
                    KeyCode::Backquote => f.write_str("Backquote"),
                    KeyCode::Backslash => f.write_str("Backslash"),
                    KeyCode::BracketLeft => f.write_str("BracketLeft"),
                    KeyCode::BracketRight => f.write_str("BracketRight"),
                    KeyCode::Comma => f.write_str("Comma"),
                    KeyCode::Digit0 => f.write_str("0"),
                    KeyCode::Digit1 => f.write_str("1"),
                    KeyCode::Digit2 => f.write_str("2"),
                    KeyCode::Digit3 => f.write_str("3"),
                    KeyCode::Digit4 => f.write_str("4"),
                    KeyCode::Digit5 => f.write_str("5"),
                    KeyCode::Digit6 => f.write_str("6"),
                    KeyCode::Digit7 => f.write_str("7"),
                    KeyCode::Digit8 => f.write_str("8"),
                    KeyCode::Digit9 => f.write_str("9"),
                    KeyCode::Equal => f.write_str("Equal"),
                    KeyCode::IntlBackslash => f.write_str("IntlBackslash"),
                    KeyCode::IntlRo => f.write_str("IntlRo"),
                    KeyCode::IntlYen => f.write_str("IntlYen"),
                    KeyCode::KeyA => f.write_str("A"),
                    KeyCode::KeyB => f.write_str("B"),
                    KeyCode::KeyC => f.write_str("C"),
                    KeyCode::KeyD => f.write_str("D"),
                    KeyCode::KeyE => f.write_str("E"),
                    KeyCode::KeyF => f.write_str("F"),
                    KeyCode::KeyG => f.write_str("G"),
                    KeyCode::KeyH => f.write_str("H"),
                    KeyCode::KeyI => f.write_str("I"),
                    KeyCode::KeyJ => f.write_str("J"),
                    KeyCode::KeyK => f.write_str("K"),
                    KeyCode::KeyL => f.write_str("L"),
                    KeyCode::KeyM => f.write_str("M"),
                    KeyCode::KeyN => f.write_str("N"),
                    KeyCode::KeyO => f.write_str("O"),
                    KeyCode::KeyP => f.write_str("P"),
                    KeyCode::KeyQ => f.write_str("Q"),
                    KeyCode::KeyR => f.write_str("R"),
                    KeyCode::KeyS => f.write_str("S"),
                    KeyCode::KeyT => f.write_str("T"),
                    KeyCode::KeyU => f.write_str("U"),
                    KeyCode::KeyV => f.write_str("V"),
                    KeyCode::KeyW => f.write_str("W"),
                    KeyCode::KeyX => f.write_str("X"),
                    KeyCode::KeyY => f.write_str("Y"),
                    KeyCode::KeyZ => f.write_str("Z"),
                    KeyCode::Minus => f.write_str("Minus"),
                    KeyCode::Period => f.write_str("Period"),
                    KeyCode::Quote => f.write_str("Quote"),
                    KeyCode::Semicolon => f.write_str("Semicolon"),
                    KeyCode::Slash => f.write_str("Slash"),
                    KeyCode::AltLeft => f.write_str("Alt"),
                    KeyCode::AltRight => f.write_str("Alt"),
                    KeyCode::Backspace => f.write_str("Backspace"),
                    KeyCode::CapsLock => f.write_str("CapsLock"),
                    KeyCode::ContextMenu => f.write_str("ContextMenu"),
                    KeyCode::ControlLeft => f.write_str("Ctrl"),
                    KeyCode::ControlRight => f.write_str("Ctrl"),
                    KeyCode::Enter => f.write_str("Enter"),
                    KeyCode::ShiftLeft => f.write_str("Shift"),
                    KeyCode::ShiftRight => f.write_str("Shift"),
                    KeyCode::Space => f.write_str("Space"),
                    KeyCode::Tab => f.write_str("Tab"),
                    KeyCode::Convert => f.write_str("Convert"),
                    KeyCode::KanaMode => f.write_str("KanaMode"),
                    KeyCode::Lang1 => f.write_str("Lang1"),
                    KeyCode::Lang2 => f.write_str("Lang2"),
                    KeyCode::Lang3 => f.write_str("Lang3"),
                    KeyCode::Lang4 => f.write_str("Lang4"),
                    KeyCode::Lang5 => f.write_str("Lang5"),
                    KeyCode::NonConvert => f.write_str("NonConvert"),
                    KeyCode::Delete => f.write_str("Delete"),
                    KeyCode::End => f.write_str("End"),
                    KeyCode::Help => f.write_str("Help"),
                    KeyCode::Home => f.write_str("Home"),
                    KeyCode::Insert => f.write_str("Insert"),
                    KeyCode::PageDown => f.write_str("PageDown"),
                    KeyCode::PageUp => f.write_str("PageUp"),
                    KeyCode::ArrowDown => f.write_str("Down"),
                    KeyCode::ArrowLeft => f.write_str("Left"),
                    KeyCode::ArrowRight => f.write_str("Right"),
                    KeyCode::ArrowUp => f.write_str("Up"),
                    KeyCode::NumLock => f.write_str("NumLock"),
                    KeyCode::Numpad0 => f.write_str("Numpad0"),
                    KeyCode::Numpad1 => f.write_str("Numpad1"),
                    KeyCode::Numpad2 => f.write_str("Numpad2"),
                    KeyCode::Numpad3 => f.write_str("Numpad3"),
                    KeyCode::Numpad4 => f.write_str("Numpad4"),
                    KeyCode::Numpad5 => f.write_str("Numpad5"),
                    KeyCode::Numpad6 => f.write_str("Numpad6"),
                    KeyCode::Numpad7 => f.write_str("Numpad7"),
                    KeyCode::Numpad8 => f.write_str("Numpad8"),
                    KeyCode::Numpad9 => f.write_str("Numpad9"),
                    KeyCode::NumpadAdd => f.write_str("NumpadAdd"),
                    KeyCode::NumpadBackspace => f.write_str("NumpadBackspace"),
                    KeyCode::NumpadClear => f.write_str("NumpadClear"),
                    KeyCode::NumpadClearEntry => f.write_str("NumpadClearEntry"),
                    KeyCode::NumpadComma => f.write_str("NumpadComma"),
                    KeyCode::NumpadDecimal => f.write_str("NumpadDecimal"),
                    KeyCode::NumpadDivide => f.write_str("NumpadDivide"),
                    KeyCode::NumpadEnter => f.write_str("NumpadEnter"),
                    KeyCode::NumpadEqual => f.write_str("NumpadEqual"),
                    KeyCode::NumpadHash => f.write_str("NumpadHash"),
                    KeyCode::NumpadMemoryAdd => f.write_str("NumpadMemoryAdd"),
                    KeyCode::NumpadMemoryClear => f.write_str("NumpadMemoryClear"),
                    KeyCode::NumpadMemoryRecall => f.write_str("NumpadMemoryRecall"),
                    KeyCode::NumpadMemoryStore => f.write_str("NumpadMemoryStore"),
                    KeyCode::NumpadMemorySubtract => {
                        f.write_str("NumpadMemorySubtract")
                    }
                    KeyCode::NumpadMultiply => f.write_str("NumpadMultiply"),
                    KeyCode::NumpadParenLeft => f.write_str("NumpadParenLeft"),
                    KeyCode::NumpadParenRight => f.write_str("NumpadParenRight"),
                    KeyCode::NumpadStar => f.write_str("NumpadStar"),
                    KeyCode::NumpadSubtract => f.write_str("NumpadSubtract"),
                    KeyCode::Escape => f.write_str("Escape"),
                    KeyCode::Fn => f.write_str("Fn"),
                    KeyCode::FnLock => f.write_str("FnLock"),
                    KeyCode::PrintScreen => f.write_str("PrintScreen"),
                    KeyCode::ScrollLock => f.write_str("ScrollLock"),
                    KeyCode::Pause => f.write_str("Pause"),
                    KeyCode::BrowserBack => f.write_str("BrowserBack"),
                    KeyCode::BrowserFavorites => f.write_str("BrowserFavorites"),
                    KeyCode::BrowserForward => f.write_str("BrowserForward"),
                    KeyCode::BrowserHome => f.write_str("BrowserHome"),
                    KeyCode::BrowserRefresh => f.write_str("BrowserRefresh"),
                    KeyCode::BrowserSearch => f.write_str("BrowserSearch"),
                    KeyCode::BrowserStop => f.write_str("BrowserStop"),
                    KeyCode::Eject => f.write_str("Eject"),
                    KeyCode::LaunchApp1 => f.write_str("LaunchApp1"),
                    KeyCode::LaunchApp2 => f.write_str("LaunchApp2"),
                    KeyCode::LaunchMail => f.write_str("LaunchMail"),
                    KeyCode::MediaPlayPause => f.write_str("MediaPlayPause"),
                    KeyCode::MediaSelect => f.write_str("MediaSelect"),
                    KeyCode::MediaStop => f.write_str("MediaStop"),
                    KeyCode::MediaTrackNext => f.write_str("MediaTrackNext"),
                    KeyCode::MediaTrackPrevious => f.write_str("MediaTrackPrevious"),
                    KeyCode::Power => f.write_str("Power"),
                    KeyCode::Sleep => f.write_str("Sleep"),
                    KeyCode::AudioVolumeDown => f.write_str("AudioVolumeDown"),
                    KeyCode::AudioVolumeMute => f.write_str("AudioVolumeMute"),
                    KeyCode::AudioVolumeUp => f.write_str("AudioVolumeUp"),
                    KeyCode::WakeUp => f.write_str("WakeUp"),
                    KeyCode::MetaLeft | KeyCode::MetaRight => {
                        match std::env::consts::OS {
                            "macos" => f.write_str("Cmd"),
                            "windows" => f.write_str("Win"),
                            _ => f.write_str("Meta"),
                        }
                    }
                    KeyCode::Abort => f.write_str("Abort"),
                    KeyCode::Resume => f.write_str("Resume"),
                    KeyCode::Suspend => f.write_str("Suspend"),
                    KeyCode::Again => f.write_str("Again"),
                    KeyCode::Copy => f.write_str("Copy"),
                    KeyCode::Cut => f.write_str("Cut"),
                    KeyCode::Find => f.write_str("Find"),
                    KeyCode::Open => f.write_str("Open"),
                    KeyCode::Paste => f.write_str("Paste"),
                    KeyCode::Props => f.write_str("Props"),
                    KeyCode::Select => f.write_str("Select"),
                    KeyCode::Undo => f.write_str("Undo"),
                    KeyCode::Hiragana => f.write_str("Hiragana"),
                    KeyCode::Katakana => f.write_str("Katakana"),
                    KeyCode::F1 => f.write_str("F1"),
                    KeyCode::F2 => f.write_str("F2"),
                    KeyCode::F3 => f.write_str("F3"),
                    KeyCode::F4 => f.write_str("F4"),
                    KeyCode::F5 => f.write_str("F5"),
                    KeyCode::F6 => f.write_str("F6"),
                    KeyCode::F7 => f.write_str("F7"),
                    KeyCode::F8 => f.write_str("F8"),
                    KeyCode::F9 => f.write_str("F9"),
                    KeyCode::F10 => f.write_str("F10"),
                    KeyCode::F11 => f.write_str("F11"),
                    KeyCode::F12 => f.write_str("F12"),
                    KeyCode::F13 => f.write_str("F13"),
                    KeyCode::F14 => f.write_str("F14"),
                    KeyCode::F15 => f.write_str("F15"),
                    KeyCode::F16 => f.write_str("F16"),
                    KeyCode::F17 => f.write_str("F17"),
                    KeyCode::F18 => f.write_str("F18"),
                    KeyCode::F19 => f.write_str("F19"),
                    KeyCode::F20 => f.write_str("F20"),
                    KeyCode::F21 => f.write_str("F21"),
                    KeyCode::F22 => f.write_str("F22"),
                    KeyCode::F23 => f.write_str("F23"),
                    KeyCode::F24 => f.write_str("F24"),
                    KeyCode::F25 => f.write_str("F25"),
                    KeyCode::F26 => f.write_str("F26"),
                    KeyCode::F27 => f.write_str("F27"),
                    KeyCode::F28 => f.write_str("F28"),
                    KeyCode::F29 => f.write_str("F29"),
                    KeyCode::F30 => f.write_str("F30"),
                    KeyCode::F31 => f.write_str("F31"),
                    KeyCode::F32 => f.write_str("F32"),
                    KeyCode::F33 => f.write_str("F33"),
                    KeyCode::F34 => f.write_str("F34"),
                    KeyCode::F35 => f.write_str("F35"),
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
                    NamedKey::Shift => f.write_str("Shift"),
                    NamedKey::Meta => f.write_str("Meta"),
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
                    NamedKey::Dead => f.write_str("Dead"),
                    _ => f.write_str("Unidentified"),
                },
                Key::Character(s) => {
                    if s == " " {
                        f.write_str("Space")
                    } else {
                        f.write_str(s)
                    }
                }
            },
            Self::Pointer(PointerButton::Auxiliary) => f.write_str("MouseMiddle"),
            Self::Pointer(PointerButton::X2) => f.write_str("MouseForward"),
            Self::Pointer(PointerButton::X1) => f.write_str("MouseBackward"),
            Self::Pointer(_) => f.write_str("MouseUnimplemented"),
        }
    }
}
