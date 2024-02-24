use std::{
    fmt::Display,
    hash::{Hash, Hasher},
    str::FromStr,
};

use floem::keyboard::{Key, KeyCode, NativeKey, PhysicalKey};

#[derive(Clone, Debug, Eq)]
pub(crate) enum KeyInput {
    Keyboard(floem::keyboard::Key, floem::keyboard::PhysicalKey),
    Pointer(floem::pointer::PointerButton),
}

impl KeyInput {
    fn keyboard_from_str(s: &str) -> Option<(Key, PhysicalKey)> {
        // Checks if it is a character key
        fn is_key_string(s: &str) -> bool {
            s.chars().all(|c| !c.is_control())
                && s.chars().skip(1).all(|c| !c.is_ascii())
        }

        fn char_to_keycode(char: &str) -> PhysicalKey {
            match char {
                "a" => PhysicalKey::Code(KeyCode::KeyA),
                "b" => PhysicalKey::Code(KeyCode::KeyB),
                "c" => PhysicalKey::Code(KeyCode::KeyC),
                "d" => PhysicalKey::Code(KeyCode::KeyD),
                "e" => PhysicalKey::Code(KeyCode::KeyE),
                "f" => PhysicalKey::Code(KeyCode::KeyF),
                "g" => PhysicalKey::Code(KeyCode::KeyG),
                "h" => PhysicalKey::Code(KeyCode::KeyH),
                "i" => PhysicalKey::Code(KeyCode::KeyI),
                "j" => PhysicalKey::Code(KeyCode::KeyJ),
                "k" => PhysicalKey::Code(KeyCode::KeyK),
                "l" => PhysicalKey::Code(KeyCode::KeyL),
                "m" => PhysicalKey::Code(KeyCode::KeyM),
                "n" => PhysicalKey::Code(KeyCode::KeyN),
                "o" => PhysicalKey::Code(KeyCode::KeyO),
                "p" => PhysicalKey::Code(KeyCode::KeyP),
                "q" => PhysicalKey::Code(KeyCode::KeyQ),
                "r" => PhysicalKey::Code(KeyCode::KeyR),
                "s" => PhysicalKey::Code(KeyCode::KeyS),
                "t" => PhysicalKey::Code(KeyCode::KeyT),
                "u" => PhysicalKey::Code(KeyCode::KeyU),
                "v" => PhysicalKey::Code(KeyCode::KeyV),
                "w" => PhysicalKey::Code(KeyCode::KeyW),
                "x" => PhysicalKey::Code(KeyCode::KeyX),
                "y" => PhysicalKey::Code(KeyCode::KeyY),
                "z" => PhysicalKey::Code(KeyCode::KeyZ),
                "=" => PhysicalKey::Code(KeyCode::Equal),
                "-" => PhysicalKey::Code(KeyCode::Minus),
                "0" => PhysicalKey::Code(KeyCode::Digit0),
                "1" => PhysicalKey::Code(KeyCode::Digit1),
                "2" => PhysicalKey::Code(KeyCode::Digit2),
                "3" => PhysicalKey::Code(KeyCode::Digit3),
                "4" => PhysicalKey::Code(KeyCode::Digit4),
                "5" => PhysicalKey::Code(KeyCode::Digit5),
                "6" => PhysicalKey::Code(KeyCode::Digit6),
                "7" => PhysicalKey::Code(KeyCode::Digit7),
                "8" => PhysicalKey::Code(KeyCode::Digit8),
                "9" => PhysicalKey::Code(KeyCode::Digit9),
                "`" => PhysicalKey::Code(KeyCode::Backquote),
                "/" => PhysicalKey::Code(KeyCode::Slash),
                "\\" => PhysicalKey::Code(KeyCode::Backslash),
                "," => PhysicalKey::Code(KeyCode::Comma),
                "." => PhysicalKey::Code(KeyCode::Period),
                "*" => PhysicalKey::Code(KeyCode::NumpadMultiply),
                "+" => PhysicalKey::Code(KeyCode::NumpadAdd),
                ";" => PhysicalKey::Code(KeyCode::Semicolon),
                "'" => PhysicalKey::Code(KeyCode::Quote),
                "[" => PhysicalKey::Code(KeyCode::BracketLeft),
                "]" => PhysicalKey::Code(KeyCode::BracketRight),
                "<" => PhysicalKey::Code(KeyCode::IntlBackslash),
                _ => PhysicalKey::Code(KeyCode::Fn),
            }
        }

        // Import into scope to reduce noise
        use floem::keyboard::NamedKey::*;
        Some(match s {
            s if is_key_string(s) => {
                let char = Key::Character(s.into());
                (char.clone(), char_to_keycode(char.to_text().unwrap()))
            }
            "unidentified" => (
                Key::Unidentified(NativeKey::Unidentified),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "alt" => (Key::Named(Alt), PhysicalKey::Code(KeyCode::AltLeft)),
            "altgraph" => {
                (Key::Named(AltGraph), PhysicalKey::Code(KeyCode::AltRight))
            }
            "capslock" => {
                (Key::Named(CapsLock), PhysicalKey::Code(KeyCode::CapsLock))
            }
            "control" => {
                (Key::Named(Control), PhysicalKey::Code(KeyCode::ControlLeft))
            }
            "fn" => (Key::Named(Fn), PhysicalKey::Code(KeyCode::Fn)),
            "fnlock" => (Key::Named(FnLock), PhysicalKey::Code(KeyCode::FnLock)),
            "meta" => (Key::Named(Meta), PhysicalKey::Code(KeyCode::Meta)),
            "numlock" => (Key::Named(NumLock), PhysicalKey::Code(KeyCode::NumLock)),
            "scrolllock" => (
                Key::Named(ScrollLock),
                PhysicalKey::Code(KeyCode::ScrollLock),
            ),
            "shift" => (Key::Named(Shift), PhysicalKey::Code(KeyCode::ShiftLeft)),
            "symbol" => (Key::Named(Symbol), PhysicalKey::Code(KeyCode::Fn)),
            "symbollock" => (Key::Named(SymbolLock), PhysicalKey::Code(KeyCode::Fn)),
            "hyper" => (Key::Named(Hyper), PhysicalKey::Code(KeyCode::Hyper)),
            "super" => (Key::Named(Super), PhysicalKey::Code(KeyCode::Meta)),
            "enter" => (Key::Named(Enter), PhysicalKey::Code(KeyCode::Enter)),
            "tab" => (Key::Named(Tab), PhysicalKey::Code(KeyCode::Tab)),
            "arrowdown" => {
                (Key::Named(ArrowDown), PhysicalKey::Code(KeyCode::ArrowDown))
            }
            "arrowleft" => {
                (Key::Named(ArrowLeft), PhysicalKey::Code(KeyCode::ArrowLeft))
            }
            "arrowright" => (
                Key::Named(ArrowRight),
                PhysicalKey::Code(KeyCode::ArrowRight),
            ),
            "arrowup" => (Key::Named(ArrowUp), PhysicalKey::Code(KeyCode::ArrowUp)),
            "end" => (Key::Named(End), PhysicalKey::Code(KeyCode::End)),
            "home" => (Key::Named(Home), PhysicalKey::Code(KeyCode::Home)),
            "pagedown" => {
                (Key::Named(PageDown), PhysicalKey::Code(KeyCode::PageDown))
            }
            "pageup" => (Key::Named(PageUp), PhysicalKey::Code(KeyCode::PageUp)),
            "backspace" => {
                (Key::Named(Backspace), PhysicalKey::Code(KeyCode::Backspace))
            }
            "clear" => (Key::Named(Clear), PhysicalKey::Code(KeyCode::Fn)),
            "copy" => (Key::Named(Copy), PhysicalKey::Code(KeyCode::Copy)),
            "crsel" => (Key::Named(CrSel), PhysicalKey::Code(KeyCode::Fn)),
            "cut" => (Key::Named(Cut), PhysicalKey::Code(KeyCode::Cut)),
            "delete" => (Key::Named(Delete), PhysicalKey::Code(KeyCode::Delete)),
            "eraseeof" => (Key::Named(EraseEof), PhysicalKey::Code(KeyCode::Fn)),
            "exsel" => (Key::Named(ExSel), PhysicalKey::Code(KeyCode::Fn)),
            "insert" => (Key::Named(Insert), PhysicalKey::Code(KeyCode::Insert)),
            "paste" => (Key::Named(Paste), PhysicalKey::Code(KeyCode::Paste)),
            "redo" => (Key::Named(Redo), PhysicalKey::Code(KeyCode::Fn)),
            "undo" => (Key::Named(Undo), PhysicalKey::Code(KeyCode::Undo)),
            "accept" => (Key::Named(Accept), PhysicalKey::Code(KeyCode::Fn)),
            "again" => (Key::Named(Again), PhysicalKey::Code(KeyCode::Again)),
            "attn" => (Key::Named(Attn), PhysicalKey::Code(KeyCode::Fn)),
            "cancel" => (Key::Named(Cancel), PhysicalKey::Code(KeyCode::Fn)),
            "contextmenu" => (
                Key::Named(ContextMenu),
                PhysicalKey::Code(KeyCode::ContextMenu),
            ),
            "escape" => (Key::Named(Escape), PhysicalKey::Code(KeyCode::Escape)),
            "execute" => (Key::Named(Execute), PhysicalKey::Code(KeyCode::Fn)),
            "find" => (Key::Named(Find), PhysicalKey::Code(KeyCode::Find)),
            "help" => (Key::Named(Help), PhysicalKey::Code(KeyCode::Help)),
            "pause" => (Key::Named(Pause), PhysicalKey::Code(KeyCode::Pause)),
            "play" => (Key::Named(Play), PhysicalKey::Code(KeyCode::MediaPlayPause)),
            "props" => (Key::Named(Props), PhysicalKey::Code(KeyCode::Props)),
            "select" => (Key::Named(Select), PhysicalKey::Code(KeyCode::Select)),
            "zoomin" => (Key::Named(ZoomIn), PhysicalKey::Code(KeyCode::Fn)),
            "zoomout" => (Key::Named(ZoomOut), PhysicalKey::Code(KeyCode::Fn)),
            "brightnessdown" => {
                (Key::Named(BrightnessDown), PhysicalKey::Code(KeyCode::Fn))
            }
            "brightnessup" => {
                (Key::Named(BrightnessUp), PhysicalKey::Code(KeyCode::Fn))
            }
            "eject" => (Key::Named(Eject), PhysicalKey::Code(KeyCode::Eject)),
            "logoff" => (Key::Named(LogOff), PhysicalKey::Code(KeyCode::Fn)),
            "power" => (Key::Named(Power), PhysicalKey::Code(KeyCode::Power)),
            "poweroff" => (Key::Named(PowerOff), PhysicalKey::Code(KeyCode::Fn)),
            "printscreen" => (
                Key::Named(PrintScreen),
                PhysicalKey::Code(KeyCode::PrintScreen),
            ),
            "hibernate" => (Key::Named(Hibernate), PhysicalKey::Code(KeyCode::Fn)),
            "standby" => (Key::Named(Standby), PhysicalKey::Code(KeyCode::Fn)),
            "wakeup" => (Key::Named(WakeUp), PhysicalKey::Code(KeyCode::WakeUp)),
            "allcandidates" => {
                (Key::Named(AllCandidates), PhysicalKey::Code(KeyCode::Fn))
            }
            "alphanumeric" => {
                (Key::Named(Alphanumeric), PhysicalKey::Code(KeyCode::Fn))
            }
            "codeinput" => (Key::Named(CodeInput), PhysicalKey::Code(KeyCode::Fn)),
            "compose" => (Key::Named(Compose), PhysicalKey::Code(KeyCode::Fn)),
            "convert" => (Key::Named(Convert), PhysicalKey::Code(KeyCode::Convert)),
            "dead" => (Key::Dead(None), PhysicalKey::Code(KeyCode::Fn)),
            "finalmode" => (Key::Named(FinalMode), PhysicalKey::Code(KeyCode::Fn)),
            "groupfirst" => (Key::Named(GroupFirst), PhysicalKey::Code(KeyCode::Fn)),
            "grouplast" => (Key::Named(GroupLast), PhysicalKey::Code(KeyCode::Fn)),
            "groupnext" => (Key::Named(GroupNext), PhysicalKey::Code(KeyCode::Fn)),
            "groupprevious" => {
                (Key::Named(GroupPrevious), PhysicalKey::Code(KeyCode::Fn))
            }
            "modechange" => (Key::Named(ModeChange), PhysicalKey::Code(KeyCode::Fn)),
            "nextcandidate" => {
                (Key::Named(NextCandidate), PhysicalKey::Code(KeyCode::Fn))
            }
            "nonconvert" => (
                Key::Named(NonConvert),
                PhysicalKey::Code(KeyCode::NonConvert),
            ),
            "previouscandidate" => (
                Key::Named(PreviousCandidate),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "process" => (Key::Named(Process), PhysicalKey::Code(KeyCode::Fn)),
            "singlecandidate" => {
                (Key::Named(SingleCandidate), PhysicalKey::Code(KeyCode::Fn))
            }
            "hangulmode" => (Key::Named(HangulMode), PhysicalKey::Code(KeyCode::Fn)),
            "hanjamode" => (Key::Named(HanjaMode), PhysicalKey::Code(KeyCode::Fn)),
            "junjamode" => (Key::Named(JunjaMode), PhysicalKey::Code(KeyCode::Fn)),
            "eisu" => (Key::Named(Eisu), PhysicalKey::Code(KeyCode::Fn)),
            "hankaku" => (Key::Named(Hankaku), PhysicalKey::Code(KeyCode::Fn)),
            "hiragana" => {
                (Key::Named(Hiragana), PhysicalKey::Code(KeyCode::Hiragana))
            }
            "hiraganakatakana" => {
                (Key::Named(HiraganaKatakana), PhysicalKey::Code(KeyCode::Fn))
            }
            "kanamode" => {
                (Key::Named(KanaMode), PhysicalKey::Code(KeyCode::KanaMode))
            }
            "kanjimode" => (Key::Named(KanjiMode), PhysicalKey::Code(KeyCode::Fn)),
            "katakana" => {
                (Key::Named(Katakana), PhysicalKey::Code(KeyCode::Katakana))
            }
            "romaji" => (Key::Named(Romaji), PhysicalKey::Code(KeyCode::Fn)),
            "zenkaku" => (Key::Named(Zenkaku), PhysicalKey::Code(KeyCode::Fn)),
            "zenkakuhankaku" => {
                (Key::Named(ZenkakuHankaku), PhysicalKey::Code(KeyCode::Fn))
            }
            "f1" => (Key::Named(F1), PhysicalKey::Code(KeyCode::F1)),
            "f2" => (Key::Named(F2), PhysicalKey::Code(KeyCode::F2)),
            "f3" => (Key::Named(F3), PhysicalKey::Code(KeyCode::F3)),
            "f4" => (Key::Named(F4), PhysicalKey::Code(KeyCode::F4)),
            "f5" => (Key::Named(F5), PhysicalKey::Code(KeyCode::F5)),
            "f6" => (Key::Named(F6), PhysicalKey::Code(KeyCode::F6)),
            "f7" => (Key::Named(F7), PhysicalKey::Code(KeyCode::F7)),
            "f8" => (Key::Named(F8), PhysicalKey::Code(KeyCode::F8)),
            "f9" => (Key::Named(F9), PhysicalKey::Code(KeyCode::F9)),
            "f10" => (Key::Named(F10), PhysicalKey::Code(KeyCode::F10)),
            "f11" => (Key::Named(F11), PhysicalKey::Code(KeyCode::F11)),
            "f12" => (Key::Named(F12), PhysicalKey::Code(KeyCode::F12)),
            "soft1" => (Key::Named(Soft1), PhysicalKey::Code(KeyCode::Fn)),
            "soft2" => (Key::Named(Soft2), PhysicalKey::Code(KeyCode::Fn)),
            "soft3" => (Key::Named(Soft3), PhysicalKey::Code(KeyCode::Fn)),
            "soft4" => (Key::Named(Soft4), PhysicalKey::Code(KeyCode::Fn)),
            "channeldown" => {
                (Key::Named(ChannelDown), PhysicalKey::Code(KeyCode::Fn))
            }
            "channelup" => (Key::Named(ChannelUp), PhysicalKey::Code(KeyCode::Fn)),
            "close" => (Key::Named(Close), PhysicalKey::Code(KeyCode::Fn)),
            "mailforward" => {
                (Key::Named(MailForward), PhysicalKey::Code(KeyCode::Fn))
            }
            "mailreply" => (Key::Named(MailReply), PhysicalKey::Code(KeyCode::Fn)),
            "mailsend" => (Key::Named(MailSend), PhysicalKey::Code(KeyCode::Fn)),
            "mediaclose" => (Key::Named(MediaClose), PhysicalKey::Code(KeyCode::Fn)),
            "mediafastforward" => {
                (Key::Named(MediaFastForward), PhysicalKey::Code(KeyCode::Fn))
            }
            "mediapause" => (Key::Named(MediaPause), PhysicalKey::Code(KeyCode::Fn)),
            "mediaplay" => (Key::Named(MediaPlay), PhysicalKey::Code(KeyCode::Fn)),
            "mediaplaypause" => (
                Key::Named(MediaPlayPause),
                PhysicalKey::Code(KeyCode::MediaPlayPause),
            ),
            "mediarecord" => {
                (Key::Named(MediaRecord), PhysicalKey::Code(KeyCode::Fn))
            }
            "mediarewind" => {
                (Key::Named(MediaRewind), PhysicalKey::Code(KeyCode::Fn))
            }
            "mediastop" => {
                (Key::Named(MediaStop), PhysicalKey::Code(KeyCode::MediaStop))
            }
            "mediatracknext" => (
                Key::Named(MediaTrackNext),
                PhysicalKey::Code(KeyCode::MediaTrackNext),
            ),
            "mediatrackprevious" => (
                Key::Named(MediaTrackPrevious),
                PhysicalKey::Code(KeyCode::MediaTrackPrevious),
            ),
            "new" => (Key::Named(New), PhysicalKey::Code(KeyCode::Fn)),
            "open" => (Key::Named(Open), PhysicalKey::Code(KeyCode::Open)),
            "print" => (Key::Named(Print), PhysicalKey::Code(KeyCode::Fn)),
            "save" => (Key::Named(Save), PhysicalKey::Code(KeyCode::Fn)),
            "spellcheck" => (Key::Named(SpellCheck), PhysicalKey::Code(KeyCode::Fn)),
            "key11" => (Key::Named(Key11), PhysicalKey::Code(KeyCode::Fn)),
            "key12" => (Key::Named(Key12), PhysicalKey::Code(KeyCode::Fn)),
            "audiobalanceleft" => {
                (Key::Named(AudioBalanceLeft), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiobalanceright" => (
                Key::Named(AudioBalanceRight),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "audiobassboostdown" => (
                Key::Named(AudioBassBoostDown),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "audiobassboosttoggle" => (
                Key::Named(AudioBassBoostToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "audiobassboostup" => {
                (Key::Named(AudioBassBoostUp), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiofaderfront" => {
                (Key::Named(AudioFaderFront), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiofaderrear" => {
                (Key::Named(AudioFaderRear), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiosurroundmodenext" => (
                Key::Named(AudioSurroundModeNext),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "audiotrebledown" => {
                (Key::Named(AudioTrebleDown), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiotrebleup" => {
                (Key::Named(AudioTrebleUp), PhysicalKey::Code(KeyCode::Fn))
            }
            "audiovolumedown" => (
                Key::Named(AudioVolumeDown),
                PhysicalKey::Code(KeyCode::AudioVolumeDown),
            ),
            "audiovolumeup" => (
                Key::Named(AudioVolumeUp),
                PhysicalKey::Code(KeyCode::AudioVolumeUp),
            ),
            "audiovolumemute" => (
                Key::Named(AudioVolumeMute),
                PhysicalKey::Code(KeyCode::AudioVolumeMute),
            ),
            "microphonetoggle" => {
                (Key::Named(MicrophoneToggle), PhysicalKey::Code(KeyCode::Fn))
            }
            "microphonevolumedown" => (
                Key::Named(MicrophoneVolumeDown),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "microphonevolumeup" => (
                Key::Named(MicrophoneVolumeUp),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "microphonevolumemute" => (
                Key::Named(MicrophoneVolumeMute),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "speechcorrectionlist" => (
                Key::Named(SpeechCorrectionList),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "speechinputtoggle" => (
                Key::Named(SpeechInputToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchapplication1" => (
                Key::Named(LaunchApplication1),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchapplication2" => (
                Key::Named(LaunchApplication2),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchcalendar" => {
                (Key::Named(LaunchCalendar), PhysicalKey::Code(KeyCode::Fn))
            }
            "launchcontacts" => {
                (Key::Named(LaunchContacts), PhysicalKey::Code(KeyCode::Fn))
            }
            "launchmail" => (
                Key::Named(LaunchMail),
                PhysicalKey::Code(KeyCode::LaunchMail),
            ),
            "launchmediaplayer" => (
                Key::Named(LaunchMediaPlayer),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchmusicplayer" => (
                Key::Named(LaunchMusicPlayer),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchphone" => {
                (Key::Named(LaunchPhone), PhysicalKey::Code(KeyCode::Fn))
            }
            "launchscreensaver" => (
                Key::Named(LaunchScreenSaver),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchspreadsheet" => (
                Key::Named(LaunchSpreadsheet),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "launchwebbrowser" => {
                (Key::Named(LaunchWebBrowser), PhysicalKey::Code(KeyCode::Fn))
            }
            "launchwebcam" => {
                (Key::Named(LaunchWebCam), PhysicalKey::Code(KeyCode::Fn))
            }
            "launchwordprocessor" => (
                Key::Named(LaunchWordProcessor),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "browserback" => (
                Key::Named(BrowserBack),
                PhysicalKey::Code(KeyCode::BrowserBack),
            ),
            "browserfavorites" => (
                Key::Named(BrowserFavorites),
                PhysicalKey::Code(KeyCode::BrowserFavorites),
            ),
            "browserforward" => (
                Key::Named(BrowserForward),
                PhysicalKey::Code(KeyCode::BrowserForward),
            ),
            "browserhome" => (
                Key::Named(BrowserHome),
                PhysicalKey::Code(KeyCode::BrowserHome),
            ),
            "browserrefresh" => (
                Key::Named(BrowserRefresh),
                PhysicalKey::Code(KeyCode::BrowserRefresh),
            ),
            "browsersearch" => (
                Key::Named(BrowserSearch),
                PhysicalKey::Code(KeyCode::BrowserSearch),
            ),
            "browserstop" => (
                Key::Named(BrowserStop),
                PhysicalKey::Code(KeyCode::BrowserStop),
            ),
            "appswitch" => (Key::Named(AppSwitch), PhysicalKey::Code(KeyCode::Fn)),
            "call" => (Key::Named(Call), PhysicalKey::Code(KeyCode::Fn)),
            "camera" => (Key::Named(Camera), PhysicalKey::Code(KeyCode::Fn)),
            "camerafocus" => {
                (Key::Named(CameraFocus), PhysicalKey::Code(KeyCode::Fn))
            }
            "endcall" => (Key::Named(EndCall), PhysicalKey::Code(KeyCode::Fn)),
            "goback" => (Key::Named(GoBack), PhysicalKey::Code(KeyCode::Fn)),
            "gohome" => (Key::Named(GoHome), PhysicalKey::Code(KeyCode::Fn)),
            "headsethook" => {
                (Key::Named(HeadsetHook), PhysicalKey::Code(KeyCode::Fn))
            }
            "lastnumberredial" => {
                (Key::Named(LastNumberRedial), PhysicalKey::Code(KeyCode::Fn))
            }
            "notification" => {
                (Key::Named(Notification), PhysicalKey::Code(KeyCode::Fn))
            }
            "mannermode" => (Key::Named(MannerMode), PhysicalKey::Code(KeyCode::Fn)),
            "voicedial" => (Key::Named(VoiceDial), PhysicalKey::Code(KeyCode::Fn)),
            "tv" => (Key::Named(TV), PhysicalKey::Code(KeyCode::Fn)),
            "tv3dmode" => (Key::Named(TV3DMode), PhysicalKey::Code(KeyCode::Fn)),
            "tvantennacable" => {
                (Key::Named(TVAntennaCable), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvaudiodescription" => (
                Key::Named(TVAudioDescription),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvaudiodescriptionmixdown" => (
                Key::Named(TVAudioDescriptionMixDown),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvaudiodescriptionmixup" => (
                Key::Named(TVAudioDescriptionMixUp),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvcontentsmenu" => {
                (Key::Named(TVContentsMenu), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvdataservice" => {
                (Key::Named(TVDataService), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvinput" => (Key::Named(TVInput), PhysicalKey::Code(KeyCode::Fn)),
            "tvinputcomponent1" => (
                Key::Named(TVInputComponent1),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvinputcomponent2" => (
                Key::Named(TVInputComponent2),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvinputcomposite1" => (
                Key::Named(TVInputComposite1),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvinputcomposite2" => (
                Key::Named(TVInputComposite2),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvinputhdmi1" => {
                (Key::Named(TVInputHDMI1), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvinputhdmi2" => {
                (Key::Named(TVInputHDMI2), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvinputhdmi3" => {
                (Key::Named(TVInputHDMI3), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvinputhdmi4" => {
                (Key::Named(TVInputHDMI4), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvinputvga1" => {
                (Key::Named(TVInputVGA1), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvmediacontext" => {
                (Key::Named(TVMediaContext), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvnetwork" => (Key::Named(TVNetwork), PhysicalKey::Code(KeyCode::Fn)),
            "tvnumberentry" => {
                (Key::Named(TVNumberEntry), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvpower" => (Key::Named(TVPower), PhysicalKey::Code(KeyCode::Fn)),
            "tvradioservice" => {
                (Key::Named(TVRadioService), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvsatellite" => {
                (Key::Named(TVSatellite), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvsatellitebs" => {
                (Key::Named(TVSatelliteBS), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvsatellitecs" => {
                (Key::Named(TVSatelliteCS), PhysicalKey::Code(KeyCode::Fn))
            }
            "tvsatellitetoggle" => (
                Key::Named(TVSatelliteToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvterrestrialanalog" => (
                Key::Named(TVTerrestrialAnalog),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvterrestrialdigital" => (
                Key::Named(TVTerrestrialDigital),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "tvtimer" => (Key::Named(TVTimer), PhysicalKey::Code(KeyCode::Fn)),
            "avrinput" => (Key::Named(AVRInput), PhysicalKey::Code(KeyCode::Fn)),
            "avrpower" => (Key::Named(AVRPower), PhysicalKey::Code(KeyCode::Fn)),
            "colorf0red" => (Key::Named(ColorF0Red), PhysicalKey::Code(KeyCode::Fn)),
            "colorf1green" => {
                (Key::Named(ColorF1Green), PhysicalKey::Code(KeyCode::Fn))
            }
            "colorf2yellow" => {
                (Key::Named(ColorF2Yellow), PhysicalKey::Code(KeyCode::Fn))
            }
            "colorf3blue" => {
                (Key::Named(ColorF3Blue), PhysicalKey::Code(KeyCode::Fn))
            }
            "colorf4grey" => {
                (Key::Named(ColorF4Grey), PhysicalKey::Code(KeyCode::Fn))
            }
            "colorf5brown" => {
                (Key::Named(ColorF5Brown), PhysicalKey::Code(KeyCode::Fn))
            }
            "closedcaptiontoggle" => (
                Key::Named(ClosedCaptionToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "dimmer" => (Key::Named(Dimmer), PhysicalKey::Code(KeyCode::Fn)),
            "displayswap" => {
                (Key::Named(DisplaySwap), PhysicalKey::Code(KeyCode::Fn))
            }
            "dvr" => (Key::Named(DVR), PhysicalKey::Code(KeyCode::Fn)),
            "exit" => (Key::Named(Exit), PhysicalKey::Code(KeyCode::Fn)),
            "favoriteclear0" => {
                (Key::Named(FavoriteClear0), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriteclear1" => {
                (Key::Named(FavoriteClear1), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriteclear2" => {
                (Key::Named(FavoriteClear2), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriteclear3" => {
                (Key::Named(FavoriteClear3), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriterecall0" => {
                (Key::Named(FavoriteRecall0), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriterecall1" => {
                (Key::Named(FavoriteRecall1), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriterecall2" => {
                (Key::Named(FavoriteRecall2), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoriterecall3" => {
                (Key::Named(FavoriteRecall3), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoritestore0" => {
                (Key::Named(FavoriteStore0), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoritestore1" => {
                (Key::Named(FavoriteStore1), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoritestore2" => {
                (Key::Named(FavoriteStore2), PhysicalKey::Code(KeyCode::Fn))
            }
            "favoritestore3" => {
                (Key::Named(FavoriteStore3), PhysicalKey::Code(KeyCode::Fn))
            }
            "guide" => (Key::Named(Guide), PhysicalKey::Code(KeyCode::Fn)),
            "guidenextday" => {
                (Key::Named(GuideNextDay), PhysicalKey::Code(KeyCode::Fn))
            }
            "guidepreviousday" => {
                (Key::Named(GuidePreviousDay), PhysicalKey::Code(KeyCode::Fn))
            }
            "info" => (Key::Named(Info), PhysicalKey::Code(KeyCode::Fn)),
            "instantreplay" => {
                (Key::Named(InstantReplay), PhysicalKey::Code(KeyCode::Fn))
            }
            "link" => (Key::Named(Link), PhysicalKey::Code(KeyCode::Fn)),
            "listprogram" => {
                (Key::Named(ListProgram), PhysicalKey::Code(KeyCode::Fn))
            }
            "livecontent" => {
                (Key::Named(LiveContent), PhysicalKey::Code(KeyCode::Fn))
            }
            "lock" => (Key::Named(Lock), PhysicalKey::Code(KeyCode::Fn)),
            "mediaapps" => (Key::Named(MediaApps), PhysicalKey::Code(KeyCode::Fn)),
            "mediaaudiotrack" => {
                (Key::Named(MediaAudioTrack), PhysicalKey::Code(KeyCode::Fn))
            }
            "medialast" => (Key::Named(MediaLast), PhysicalKey::Code(KeyCode::Fn)),
            "mediaskipbackward" => (
                Key::Named(MediaSkipBackward),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "mediaskipforward" => {
                (Key::Named(MediaSkipForward), PhysicalKey::Code(KeyCode::Fn))
            }
            "mediastepbackward" => (
                Key::Named(MediaStepBackward),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "mediastepforward" => {
                (Key::Named(MediaStepForward), PhysicalKey::Code(KeyCode::Fn))
            }
            "mediatopmenu" => {
                (Key::Named(MediaTopMenu), PhysicalKey::Code(KeyCode::Fn))
            }
            "navigatein" => (Key::Named(NavigateIn), PhysicalKey::Code(KeyCode::Fn)),
            "navigatenext" => {
                (Key::Named(NavigateNext), PhysicalKey::Code(KeyCode::Fn))
            }
            "navigateout" => {
                (Key::Named(NavigateOut), PhysicalKey::Code(KeyCode::Fn))
            }
            "navigateprevious" => {
                (Key::Named(NavigatePrevious), PhysicalKey::Code(KeyCode::Fn))
            }
            "nextfavoritechannel" => (
                Key::Named(NextFavoriteChannel),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "nextuserprofile" => {
                (Key::Named(NextUserProfile), PhysicalKey::Code(KeyCode::Fn))
            }
            "ondemand" => (Key::Named(OnDemand), PhysicalKey::Code(KeyCode::Fn)),
            "pairing" => (Key::Named(Pairing), PhysicalKey::Code(KeyCode::Fn)),
            "pinpdown" => (Key::Named(PinPDown), PhysicalKey::Code(KeyCode::Fn)),
            "pinpmove" => (Key::Named(PinPMove), PhysicalKey::Code(KeyCode::Fn)),
            "pinptoggle" => (Key::Named(PinPToggle), PhysicalKey::Code(KeyCode::Fn)),
            "pinpup" => (Key::Named(PinPUp), PhysicalKey::Code(KeyCode::Fn)),
            "playspeeddown" => {
                (Key::Named(PlaySpeedDown), PhysicalKey::Code(KeyCode::Fn))
            }
            "playspeedreset" => {
                (Key::Named(PlaySpeedReset), PhysicalKey::Code(KeyCode::Fn))
            }
            "playspeedup" => {
                (Key::Named(PlaySpeedUp), PhysicalKey::Code(KeyCode::Fn))
            }
            "randomtoggle" => {
                (Key::Named(RandomToggle), PhysicalKey::Code(KeyCode::Fn))
            }
            "rclowbattery" => {
                (Key::Named(RcLowBattery), PhysicalKey::Code(KeyCode::Fn))
            }
            "recordspeednext" => {
                (Key::Named(RecordSpeedNext), PhysicalKey::Code(KeyCode::Fn))
            }
            "rfbypass" => (Key::Named(RfBypass), PhysicalKey::Code(KeyCode::Fn)),
            "scanchannelstoggle" => (
                Key::Named(ScanChannelsToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "screenmodenext" => {
                (Key::Named(ScreenModeNext), PhysicalKey::Code(KeyCode::Fn))
            }
            "settings" => (Key::Named(Settings), PhysicalKey::Code(KeyCode::Fn)),
            "splitscreentoggle" => (
                Key::Named(SplitScreenToggle),
                PhysicalKey::Code(KeyCode::Fn),
            ),
            "stbinput" => (Key::Named(STBInput), PhysicalKey::Code(KeyCode::Fn)),
            "stbpower" => (Key::Named(STBPower), PhysicalKey::Code(KeyCode::Fn)),
            "subtitle" => (Key::Named(Subtitle), PhysicalKey::Code(KeyCode::Fn)),
            "teletext" => (Key::Named(Teletext), PhysicalKey::Code(KeyCode::Fn)),
            "videomodenext" => {
                (Key::Named(VideoModeNext), PhysicalKey::Code(KeyCode::Fn))
            }
            "wink" => (Key::Named(Wink), PhysicalKey::Code(KeyCode::Fn)),
            "zoomtoggle" => (Key::Named(ZoomToggle), PhysicalKey::Code(KeyCode::Fn)),

            // Custom key name mappings
            "esc" => (Key::Named(Escape), PhysicalKey::Code(KeyCode::Escape)),
            "space" => (Key::Named(Space), PhysicalKey::Code(KeyCode::Space)),
            "bs" => (Key::Named(Backspace), PhysicalKey::Code(KeyCode::Backspace)),
            "up" => (Key::Named(ArrowUp), PhysicalKey::Code(KeyCode::ArrowUp)),
            "down" => (Key::Named(ArrowDown), PhysicalKey::Code(KeyCode::ArrowDown)),
            "right" => (
                Key::Named(ArrowRight),
                PhysicalKey::Code(KeyCode::ArrowRight),
            ),
            "left" => (Key::Named(ArrowLeft), PhysicalKey::Code(KeyCode::ArrowLeft)),
            "del" => (Key::Named(Delete), PhysicalKey::Code(KeyCode::Delete)),

            _ => return None,
        })
    }

    fn mouse_from_str(s: &str) -> Option<floem::pointer::PointerButton> {
        use floem::pointer::PointerButton as B;

        Some(match s {
            "mousemiddle" => B::Auxiliary,
            "mouseforward" => B::X2,
            "mousebackward" => B::X1,
            _ => return None,
        })
    }
}

impl Display for KeyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use floem::pointer::PointerButton as B;

        match self {
            Self::Keyboard(_key, key_code) => match key_code {
                PhysicalKey::Unidentified(_) => f.write_str("Unidentified"),
                PhysicalKey::Code(KeyCode::Backquote) => f.write_str("`"),
                PhysicalKey::Code(KeyCode::Backslash) => f.write_str("\\"),
                PhysicalKey::Code(KeyCode::BracketLeft) => f.write_str("["),
                PhysicalKey::Code(KeyCode::BracketRight) => f.write_str("]"),
                PhysicalKey::Code(KeyCode::Comma) => f.write_str(","),
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
                PhysicalKey::Code(KeyCode::Equal) => f.write_str("="),
                PhysicalKey::Code(KeyCode::IntlBackslash) => f.write_str("<"),
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
                PhysicalKey::Code(KeyCode::Minus) => f.write_str("-"),
                PhysicalKey::Code(KeyCode::Period) => f.write_str("."),
                PhysicalKey::Code(KeyCode::Quote) => f.write_str("'"),
                PhysicalKey::Code(KeyCode::Semicolon) => f.write_str(";"),
                PhysicalKey::Code(KeyCode::Slash) => f.write_str("/"),
                PhysicalKey::Code(KeyCode::AltLeft) => f.write_str("Alt"),
                PhysicalKey::Code(KeyCode::AltRight) => f.write_str("Alt"),
                PhysicalKey::Code(KeyCode::Backspace) => f.write_str("backspace"),
                PhysicalKey::Code(KeyCode::CapsLock) => f.write_str("CapsLock"),
                PhysicalKey::Code(KeyCode::ContextMenu) => {
                    f.write_str("ContextMenu")
                }
                PhysicalKey::Code(KeyCode::ControlLeft) => f.write_str("Ctrl"),
                PhysicalKey::Code(KeyCode::ControlRight) => f.write_str("Ctrl"),
                PhysicalKey::Code(KeyCode::Enter) => f.write_str("Enter"),
                PhysicalKey::Code(KeyCode::SuperLeft) => {
                    match std::env::consts::OS {
                        "macos" => f.write_str("Cmd"),
                        "windows" => f.write_str("Win"),
                        _ => f.write_str("Meta"),
                    }
                }
                PhysicalKey::Code(KeyCode::SuperRight) => match std::env::consts::OS
                {
                    "macos" => f.write_str("Cmd"),
                    "windows" => f.write_str("Win"),
                    _ => f.write_str("Meta"),
                },
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
                PhysicalKey::Code(KeyCode::NonConvert) => f.write_str("NonConvert"),
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
                PhysicalKey::Code(KeyCode::NumpadAdd) => f.write_str("NumpadAdd"),
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
                PhysicalKey::Code(KeyCode::NumpadHash) => f.write_str("NumpadHash"),
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
                PhysicalKey::Code(KeyCode::NumpadStar) => f.write_str("NumpadStar"),
                PhysicalKey::Code(KeyCode::NumpadSubtract) => {
                    f.write_str("NumpadSubtract")
                }
                PhysicalKey::Code(KeyCode::Escape) => f.write_str("Escape"),
                PhysicalKey::Code(KeyCode::Fn) => f.write_str("Fn"),
                PhysicalKey::Code(KeyCode::FnLock) => f.write_str("FnLock"),
                PhysicalKey::Code(KeyCode::PrintScreen) => {
                    f.write_str("PrintScreen")
                }
                PhysicalKey::Code(KeyCode::ScrollLock) => f.write_str("ScrollLock"),
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
                PhysicalKey::Code(KeyCode::LaunchApp1) => f.write_str("LaunchApp1"),
                PhysicalKey::Code(KeyCode::LaunchApp2) => f.write_str("LaunchApp2"),
                PhysicalKey::Code(KeyCode::LaunchMail) => f.write_str("LaunchMail"),
                PhysicalKey::Code(KeyCode::MediaPlayPause) => {
                    f.write_str("MediaPlayPause")
                }
                PhysicalKey::Code(KeyCode::MediaSelect) => {
                    f.write_str("MediaSelect")
                }
                PhysicalKey::Code(KeyCode::MediaStop) => f.write_str("MediaStop"),
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
            },
            Self::Pointer(B::Auxiliary) => f.write_str("MouseMiddle"),
            Self::Pointer(B::X2) => f.write_str("MouseForward"),
            Self::Pointer(B::X1) => f.write_str("MouseBackward"),
            Self::Pointer(_) => f.write_str("MouseUnimplemented"),
        }
    }
}

impl FromStr for KeyInput {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();

        KeyInput::keyboard_from_str(&s)
            .map(|key| KeyInput::Keyboard(key.0, key.1))
            .or_else(|| KeyInput::mouse_from_str(&s).map(KeyInput::Pointer))
            .ok_or(())
    }
}

impl Hash for KeyInput {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Keyboard(_key, key_code) => key_code.hash(state),
            // TODO: Implement `Hash` for `druid::MouseButton`
            Self::Pointer(btn) => (*btn as u8).hash(state),
        }
    }
}

impl PartialEq for KeyInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                KeyInput::Keyboard(_key_a, key_code_a),
                KeyInput::Keyboard(_key_b, key_code_b),
            ) => key_code_a.eq(key_code_b),
            (KeyInput::Pointer(a), KeyInput::Pointer(b)) => a.eq(b),
            _ => false,
        }
    }
}
