use std::{
    fmt::Display,
    hash::{Hash, Hasher},
    str::FromStr,
};

use druid::{
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    Modifiers, PaintCtx, Point, Rect,
};

use super::KeyPressData;
use crate::{
    config::{LapceConfig, LapceTheme},
    keypress::paint_key,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub(super) key: Key,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    pub fn keyboard(ev: &druid::KeyEvent) -> Option<KeyPress> {
        use druid::KbKey as K;

        match ev.key {
            K::Shift | K::Meta | K::Super | K::Alt | K::Control => None,
            ref key => Some(Self {
                key: Key::Keyboard(match key {
                    K::Character(c) => K::Character(c.to_lowercase()),
                    key => key.clone(),
                }),
                mods: KeyPressData::get_key_modifiers(ev),
            }),
        }
    }

    pub fn mouse(ev: &druid::MouseEvent) -> KeyPress {
        Self {
            key: Key::Mouse(ev.button),
            mods: ev.mods,
        }
    }

    pub fn hotkey(&self) -> Option<druid::HotKey> {
        self.key
            .as_keyboard()
            .map(|k| druid::HotKey::new(self.mods, k.clone()))
    }

    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            if let Key::Keyboard(druid::KbKey::Character(_c)) = &self.key {
                return true;
            }
        }
        false
    }

    pub fn to_lowercase(&self) -> Self {
        let key = match &self.key {
            Key::Keyboard(druid::KbKey::Character(c)) => {
                Key::Keyboard(druid::KbKey::Character(c.to_lowercase()))
            }
            _ => self.key.clone(),
        };
        Self {
            key,
            mods: self.mods,
        }
    }

    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        origin: Point,
        config: &LapceConfig,
    ) -> (Point, Vec<(Option<Rect>, PietTextLayout, Point)>) {
        let mut origin = origin;
        let mut keys = Vec::new();
        if self.mods.ctrl() {
            keys.push("Ctrl".to_string());
        }
        if self.mods.alt() {
            keys.push("Alt".to_string());
        }
        if self.mods.meta() {
            let keyname = match std::env::consts::OS {
                "macos" => "Cmd",
                "windows" => "Win",
                _ => "Meta",
            };
            keys.push(keyname.to_string());
        }
        if self.mods.shift() {
            keys.push("Shift".to_string());
        }
        keys.push(self.key.to_string());

        let mut items = Vec::new();
        let keys_len = keys.len();
        for (i, key) in keys.iter().enumerate() {
            let (rect, text_layout, text_layout_pos) =
                paint_key(ctx, key, origin, config);
            origin += (rect.width() + 5.0, 0.0);

            items.push((Some(rect), text_layout, text_layout_pos));

            if i < keys_len - 1 {
                let text_layout = ctx
                    .text()
                    .new_text_layout("+")
                    .font(config.ui.font_family(), config.ui.font_size() as f64)
                    .text_color(
                        config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let text_layout_pos = origin + (0.0, -(text_size.height / 2.0));
                items.push((None, text_layout, text_layout_pos));
                origin += (text_size.width + 5.0, 0.0);
            }
        }

        (origin, items)
    }

    pub fn parse(key: &str) -> Vec<Self> {
        key.split(' ')
            .filter_map(|k| {
                let (modifiers, key) = match k.rsplit_once('+') {
                    Some(pair) => pair,
                    None => ("", k),
                };

                let key = match key.parse().ok() {
                    Some(key) => key,
                    None => {
                        // Skip past unrecognized key definitions
                        log::warn!("Unrecognized key: {key}");
                        return None;
                    }
                };

                let mut mods = Modifiers::default();
                for part in modifiers.to_lowercase().split('+') {
                    match part {
                        "ctrl" => mods.set(Modifiers::CONTROL, true),
                        "meta" => mods.set(Modifiers::META, true),
                        "shift" => mods.set(Modifiers::SHIFT, true),
                        "alt" => mods.set(Modifiers::ALT, true),
                        "" => (),
                        other => log::warn!("Invalid key modifier: {}", other),
                    }
                }

                Some(KeyPress { key, mods })
            })
            .collect()
    }
}

impl Display for KeyPress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.ctrl() {
            let _ = f.write_str("Ctrl+");
        }
        if self.mods.alt() {
            let _ = f.write_str("Alt+");
        }
        if self.mods.meta() {
            let _ = f.write_str("Meta+");
        }
        if self.mods.shift() {
            let _ = f.write_str("Shift+");
        }
        f.write_str(&self.key.to_string())
    }
}

#[derive(Clone, Debug, Eq)]
pub(super) enum Key {
    Keyboard(druid::KbKey),
    Mouse(druid::MouseButton),
}

impl Key {
    pub(super) fn as_keyboard(&self) -> Option<&'_ druid::KbKey> {
        match self {
            Self::Keyboard(key) => Some(key),
            Self::Mouse(_) => None,
        }
    }

    fn keyboard_from_str(s: &str) -> Option<druid::KbKey> {
        // Checks if it is a character key
        fn is_key_string(s: &str) -> bool {
            s.chars().all(|c| !c.is_control())
                && s.chars().skip(1).all(|c| !c.is_ascii())
        }

        // Import into scope to reduce noise
        use druid::keyboard_types::Key::*;
        Some(match s {
            s if is_key_string(s) => Character(s.to_string()),
            "unidentified" => Unidentified,
            "alt" => Alt,
            "altgraph" => AltGraph,
            "capslock" => CapsLock,
            "control" => Control,
            "fn" => Fn,
            "fnlock" => FnLock,
            "meta" => Meta,
            "numlock" => NumLock,
            "scrolllock" => ScrollLock,
            "shift" => Shift,
            "symbol" => Symbol,
            "symbollock" => SymbolLock,
            "hyper" => Hyper,
            "super" => Super,
            "enter" => Enter,
            "tab" => Tab,
            "arrowdown" => ArrowDown,
            "arrowleft" => ArrowLeft,
            "arrowright" => ArrowRight,
            "arrowup" => ArrowUp,
            "end" => End,
            "home" => Home,
            "pagedown" => PageDown,
            "pageup" => PageUp,
            "backspace" => Backspace,
            "clear" => Clear,
            "copy" => Copy,
            "crsel" => CrSel,
            "cut" => Cut,
            "delete" => Delete,
            "eraseeof" => EraseEof,
            "exsel" => ExSel,
            "insert" => Insert,
            "paste" => Paste,
            "redo" => Redo,
            "undo" => Undo,
            "accept" => Accept,
            "again" => Again,
            "attn" => Attn,
            "cancel" => Cancel,
            "contextmenu" => ContextMenu,
            "escape" => Escape,
            "execute" => Execute,
            "find" => Find,
            "help" => Help,
            "pause" => Pause,
            "play" => Play,
            "props" => Props,
            "select" => Select,
            "zoomin" => ZoomIn,
            "zoomout" => ZoomOut,
            "brightnessdown" => BrightnessDown,
            "brightnessup" => BrightnessUp,
            "eject" => Eject,
            "logoff" => LogOff,
            "power" => Power,
            "poweroff" => PowerOff,
            "printscreen" => PrintScreen,
            "hibernate" => Hibernate,
            "standby" => Standby,
            "wakeup" => WakeUp,
            "allcandidates" => AllCandidates,
            "alphanumeric" => Alphanumeric,
            "codeinput" => CodeInput,
            "compose" => Compose,
            "convert" => Convert,
            "dead" => Dead,
            "finalmode" => FinalMode,
            "groupfirst" => GroupFirst,
            "grouplast" => GroupLast,
            "groupnext" => GroupNext,
            "groupprevious" => GroupPrevious,
            "modechange" => ModeChange,
            "nextcandidate" => NextCandidate,
            "nonconvert" => NonConvert,
            "previouscandidate" => PreviousCandidate,
            "process" => Process,
            "singlecandidate" => SingleCandidate,
            "hangulmode" => HangulMode,
            "hanjamode" => HanjaMode,
            "junjamode" => JunjaMode,
            "eisu" => Eisu,
            "hankaku" => Hankaku,
            "hiragana" => Hiragana,
            "hiraganakatakana" => HiraganaKatakana,
            "kanamode" => KanaMode,
            "kanjimode" => KanjiMode,
            "katakana" => Katakana,
            "romaji" => Romaji,
            "zenkaku" => Zenkaku,
            "zenkakuhankaku" => ZenkakuHankaku,
            "f1" => F1,
            "f2" => F2,
            "f3" => F3,
            "f4" => F4,
            "f5" => F5,
            "f6" => F6,
            "f7" => F7,
            "f8" => F8,
            "f9" => F9,
            "f10" => F10,
            "f11" => F11,
            "f12" => F12,
            "soft1" => Soft1,
            "soft2" => Soft2,
            "soft3" => Soft3,
            "soft4" => Soft4,
            "channeldown" => ChannelDown,
            "channelup" => ChannelUp,
            "close" => Close,
            "mailforward" => MailForward,
            "mailreply" => MailReply,
            "mailsend" => MailSend,
            "mediaclose" => MediaClose,
            "mediafastforward" => MediaFastForward,
            "mediapause" => MediaPause,
            "mediaplay" => MediaPlay,
            "mediaplaypause" => MediaPlayPause,
            "mediarecord" => MediaRecord,
            "mediarewind" => MediaRewind,
            "mediastop" => MediaStop,
            "mediatracknext" => MediaTrackNext,
            "mediatrackprevious" => MediaTrackPrevious,
            "new" => New,
            "open" => Open,
            "print" => Print,
            "save" => Save,
            "spellcheck" => SpellCheck,
            "key11" => Key11,
            "key12" => Key12,
            "audiobalanceleft" => AudioBalanceLeft,
            "audiobalanceright" => AudioBalanceRight,
            "audiobassboostdown" => AudioBassBoostDown,
            "audiobassboosttoggle" => AudioBassBoostToggle,
            "audiobassboostup" => AudioBassBoostUp,
            "audiofaderfront" => AudioFaderFront,
            "audiofaderrear" => AudioFaderRear,
            "audiosurroundmodenext" => AudioSurroundModeNext,
            "audiotrebledown" => AudioTrebleDown,
            "audiotrebleup" => AudioTrebleUp,
            "audiovolumedown" => AudioVolumeDown,
            "audiovolumeup" => AudioVolumeUp,
            "audiovolumemute" => AudioVolumeMute,
            "microphonetoggle" => MicrophoneToggle,
            "microphonevolumedown" => MicrophoneVolumeDown,
            "microphonevolumeup" => MicrophoneVolumeUp,
            "microphonevolumemute" => MicrophoneVolumeMute,
            "speechcorrectionlist" => SpeechCorrectionList,
            "speechinputtoggle" => SpeechInputToggle,
            "launchapplication1" => LaunchApplication1,
            "launchapplication2" => LaunchApplication2,
            "launchcalendar" => LaunchCalendar,
            "launchcontacts" => LaunchContacts,
            "launchmail" => LaunchMail,
            "launchmediaplayer" => LaunchMediaPlayer,
            "launchmusicplayer" => LaunchMusicPlayer,
            "launchphone" => LaunchPhone,
            "launchscreensaver" => LaunchScreenSaver,
            "launchspreadsheet" => LaunchSpreadsheet,
            "launchwebbrowser" => LaunchWebBrowser,
            "launchwebcam" => LaunchWebCam,
            "launchwordprocessor" => LaunchWordProcessor,
            "browserback" => BrowserBack,
            "browserfavorites" => BrowserFavorites,
            "browserforward" => BrowserForward,
            "browserhome" => BrowserHome,
            "browserrefresh" => BrowserRefresh,
            "browsersearch" => BrowserSearch,
            "browserstop" => BrowserStop,
            "appswitch" => AppSwitch,
            "call" => Call,
            "camera" => Camera,
            "camerafocus" => CameraFocus,
            "endcall" => EndCall,
            "goback" => GoBack,
            "gohome" => GoHome,
            "headsethook" => HeadsetHook,
            "lastnumberredial" => LastNumberRedial,
            "notification" => Notification,
            "mannermode" => MannerMode,
            "voicedial" => VoiceDial,
            "tv" => TV,
            "tv3dmode" => TV3DMode,
            "tvantennacable" => TVAntennaCable,
            "tvaudiodescription" => TVAudioDescription,
            "tvaudiodescriptionmixdown" => TVAudioDescriptionMixDown,
            "tvaudiodescriptionmixup" => TVAudioDescriptionMixUp,
            "tvcontentsmenu" => TVContentsMenu,
            "tvdataservice" => TVDataService,
            "tvinput" => TVInput,
            "tvinputcomponent1" => TVInputComponent1,
            "tvinputcomponent2" => TVInputComponent2,
            "tvinputcomposite1" => TVInputComposite1,
            "tvinputcomposite2" => TVInputComposite2,
            "tvinputhdmi1" => TVInputHDMI1,
            "tvinputhdmi2" => TVInputHDMI2,
            "tvinputhdmi3" => TVInputHDMI3,
            "tvinputhdmi4" => TVInputHDMI4,
            "tvinputvga1" => TVInputVGA1,
            "tvmediacontext" => TVMediaContext,
            "tvnetwork" => TVNetwork,
            "tvnumberentry" => TVNumberEntry,
            "tvpower" => TVPower,
            "tvradioservice" => TVRadioService,
            "tvsatellite" => TVSatellite,
            "tvsatellitebs" => TVSatelliteBS,
            "tvsatellitecs" => TVSatelliteCS,
            "tvsatellitetoggle" => TVSatelliteToggle,
            "tvterrestrialanalog" => TVTerrestrialAnalog,
            "tvterrestrialdigital" => TVTerrestrialDigital,
            "tvtimer" => TVTimer,
            "avrinput" => AVRInput,
            "avrpower" => AVRPower,
            "colorf0red" => ColorF0Red,
            "colorf1green" => ColorF1Green,
            "colorf2yellow" => ColorF2Yellow,
            "colorf3blue" => ColorF3Blue,
            "colorf4grey" => ColorF4Grey,
            "colorf5brown" => ColorF5Brown,
            "closedcaptiontoggle" => ClosedCaptionToggle,
            "dimmer" => Dimmer,
            "displayswap" => DisplaySwap,
            "dvr" => DVR,
            "exit" => Exit,
            "favoriteclear0" => FavoriteClear0,
            "favoriteclear1" => FavoriteClear1,
            "favoriteclear2" => FavoriteClear2,
            "favoriteclear3" => FavoriteClear3,
            "favoriterecall0" => FavoriteRecall0,
            "favoriterecall1" => FavoriteRecall1,
            "favoriterecall2" => FavoriteRecall2,
            "favoriterecall3" => FavoriteRecall3,
            "favoritestore0" => FavoriteStore0,
            "favoritestore1" => FavoriteStore1,
            "favoritestore2" => FavoriteStore2,
            "favoritestore3" => FavoriteStore3,
            "guide" => Guide,
            "guidenextday" => GuideNextDay,
            "guidepreviousday" => GuidePreviousDay,
            "info" => Info,
            "instantreplay" => InstantReplay,
            "link" => Link,
            "listprogram" => ListProgram,
            "livecontent" => LiveContent,
            "lock" => Lock,
            "mediaapps" => MediaApps,
            "mediaaudiotrack" => MediaAudioTrack,
            "medialast" => MediaLast,
            "mediaskipbackward" => MediaSkipBackward,
            "mediaskipforward" => MediaSkipForward,
            "mediastepbackward" => MediaStepBackward,
            "mediastepforward" => MediaStepForward,
            "mediatopmenu" => MediaTopMenu,
            "navigatein" => NavigateIn,
            "navigatenext" => NavigateNext,
            "navigateout" => NavigateOut,
            "navigateprevious" => NavigatePrevious,
            "nextfavoritechannel" => NextFavoriteChannel,
            "nextuserprofile" => NextUserProfile,
            "ondemand" => OnDemand,
            "pairing" => Pairing,
            "pinpdown" => PinPDown,
            "pinpmove" => PinPMove,
            "pinptoggle" => PinPToggle,
            "pinpup" => PinPUp,
            "playspeeddown" => PlaySpeedDown,
            "playspeedreset" => PlaySpeedReset,
            "playspeedup" => PlaySpeedUp,
            "randomtoggle" => RandomToggle,
            "rclowbattery" => RcLowBattery,
            "recordspeednext" => RecordSpeedNext,
            "rfbypass" => RfBypass,
            "scanchannelstoggle" => ScanChannelsToggle,
            "screenmodenext" => ScreenModeNext,
            "settings" => Settings,
            "splitscreentoggle" => SplitScreenToggle,
            "stbinput" => STBInput,
            "stbpower" => STBPower,
            "subtitle" => Subtitle,
            "teletext" => Teletext,
            "videomodenext" => VideoModeNext,
            "wink" => Wink,
            "zoomtoggle" => ZoomToggle,

            // Custom key name mappings
            "esc" => Escape,
            "space" => Character(" ".to_string()),
            "bs" => Backspace,
            "up" => ArrowUp,
            "down" => ArrowDown,
            "right" => ArrowRight,
            "left" => ArrowLeft,
            "del" => Delete,

            _ => return None,
        })
    }

    fn mouse_from_str(s: &str) -> Option<druid::MouseButton> {
        use druid::MouseButton as B;

        Some(match s {
            "mousemiddle" => B::Middle,
            "mouseforward" => B::X2,
            "mousebackward" => B::X1,
            _ => return None,
        })
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use druid::MouseButton as B;

        match self {
            Self::Keyboard(key) => return key.fmt(f),
            Self::Mouse(B::Middle) => "MouseMiddle",
            Self::Mouse(B::X2) => "MouseForward",
            Self::Mouse(B::X1) => "MouseBackward",
            Self::Mouse(_) => "MouseUnimplemented",
        }
        .fmt(f)
    }
}

impl FromStr for Key {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();

        Key::keyboard_from_str(&s)
            .map(Key::Keyboard)
            .or_else(|| Key::mouse_from_str(&s).map(Key::Mouse))
            .ok_or(())
    }
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Keyboard(key) => key.hash(state),
            // TODO: Implement `Hash` for `druid::MouseButton`
            Self::Mouse(btn) => (*btn as u8).hash(state),
        }
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Key::Keyboard(a), Key::Keyboard(b)) => a.eq(b),
            (Key::Mouse(a), Key::Mouse(b)) => a.eq(b),
            _ => false,
        }
    }
}
