use std::{
    fmt::Display,
    hash::{Hash, Hasher},
    str::FromStr,
};

use floem::keyboard::{Key, NativeKey};

#[derive(Clone, Debug, Eq)]
pub(crate) enum KeyInput {
    Keyboard(floem::keyboard::Key),
    Pointer(floem::pointer::PointerButton),
}

impl KeyInput {
    fn keyboard_from_str(s: &str) -> Option<Key> {
        // Checks if it is a character key
        fn is_a_character_key(s: &str) -> bool {
            s.chars().all(|c| !c.is_control())
                && s.chars().skip(1).all(|c| !c.is_ascii())
        }

        // Import into scope to reduce noise
        use floem::keyboard::NamedKey::*;
        Some(match s {
            s if is_a_character_key(s) => {
                let char = Key::Character(s.into());
                char.clone()
            }
            "unidentified" => Key::Unidentified(NativeKey::Unidentified),
            "alt" => Key::Named(Alt),
            "altgraph" => Key::Named(AltGraph),
            "capslock" => Key::Named(CapsLock),
            "control" => Key::Named(Control),
            "fn" => Key::Named(Fn),
            "fnlock" => Key::Named(FnLock),
            "meta" => Key::Named(Meta),
            "numlock" => Key::Named(NumLock),
            "scrolllock" => Key::Named(ScrollLock),
            "shift" => Key::Named(Shift),
            "symbol" => Key::Named(Symbol),
            "symbollock" => Key::Named(SymbolLock),
            "hyper" => Key::Named(Hyper),
            "super" => Key::Named(Super),
            "enter" => Key::Named(Enter),
            "tab" => Key::Named(Tab),
            "arrowdown" => Key::Named(ArrowDown),
            "arrowleft" => Key::Named(ArrowLeft),
            "arrowright" => Key::Named(ArrowRight),
            "arrowup" => Key::Named(ArrowUp),
            "end" => Key::Named(End),
            "home" => Key::Named(Home),
            "pagedown" => Key::Named(PageDown),
            "pageup" => Key::Named(PageUp),
            "backspace" => Key::Named(Backspace),
            "clear" => Key::Named(Clear),
            "copy" => Key::Named(Copy),
            "crsel" => Key::Named(CrSel),
            "cut" => Key::Named(Cut),
            "delete" => Key::Named(Delete),
            "eraseeof" => Key::Named(EraseEof),
            "exsel" => Key::Named(ExSel),
            "insert" => Key::Named(Insert),
            "paste" => Key::Named(Paste),
            "redo" => Key::Named(Redo),
            "undo" => Key::Named(Undo),
            "accept" => Key::Named(Accept),
            "again" => Key::Named(Again),
            "attn" => Key::Named(Attn),
            "cancel" => Key::Named(Cancel),
            "contextmenu" => Key::Named(ContextMenu),
            "escape" => Key::Named(Escape),
            "execute" => Key::Named(Execute),
            "find" => Key::Named(Find),
            "help" => Key::Named(Help),
            "pause" => Key::Named(Pause),
            "play" => Key::Named(Play),
            "props" => Key::Named(Props),
            "select" => Key::Named(Select),
            "zoomin" => Key::Named(ZoomIn),
            "zoomout" => Key::Named(ZoomOut),
            "brightnessdown" => Key::Named(BrightnessDown),
            "brightnessup" => Key::Named(BrightnessUp),
            "eject" => Key::Named(Eject),
            "logoff" => Key::Named(LogOff),
            "power" => Key::Named(Power),
            "poweroff" => Key::Named(PowerOff),
            "printscreen" => Key::Named(PrintScreen),
            "hibernate" => Key::Named(Hibernate),
            "standby" => Key::Named(Standby),
            "wakeup" => Key::Named(WakeUp),
            "allcandidates" => Key::Named(AllCandidates),
            "alphanumeric" => Key::Named(Alphanumeric),
            "codeinput" => Key::Named(CodeInput),
            "compose" => Key::Named(Compose),
            "convert" => Key::Named(Convert),
            "dead" => Key::Dead(None),
            "finalmode" => Key::Named(FinalMode),
            "groupfirst" => Key::Named(GroupFirst),
            "grouplast" => Key::Named(GroupLast),
            "groupnext" => Key::Named(GroupNext),
            "groupprevious" => Key::Named(GroupPrevious),
            "modechange" => Key::Named(ModeChange),
            "nextcandidate" => Key::Named(NextCandidate),
            "nonconvert" => Key::Named(NonConvert),
            "previouscandidate" => Key::Named(PreviousCandidate),
            "process" => Key::Named(Process),
            "singlecandidate" => Key::Named(SingleCandidate),
            "hangulmode" => Key::Named(HangulMode),
            "hanjamode" => Key::Named(HanjaMode),
            "junjamode" => Key::Named(JunjaMode),
            "eisu" => Key::Named(Eisu),
            "hankaku" => Key::Named(Hankaku),
            "hiragana" => Key::Named(Hiragana),
            "hiraganakatakana" => Key::Named(HiraganaKatakana),
            "kanamode" => Key::Named(KanaMode),
            "kanjimode" => Key::Named(KanjiMode),
            "katakana" => Key::Named(Katakana),
            "romaji" => Key::Named(Romaji),
            "zenkaku" => Key::Named(Zenkaku),
            "zenkakuhankaku" => Key::Named(ZenkakuHankaku),
            "f1" => Key::Named(F1),
            "f2" => Key::Named(F2),
            "f3" => Key::Named(F3),
            "f4" => Key::Named(F4),
            "f5" => Key::Named(F5),
            "f6" => Key::Named(F6),
            "f7" => Key::Named(F7),
            "f8" => Key::Named(F8),
            "f9" => Key::Named(F9),
            "f10" => Key::Named(F10),
            "f11" => Key::Named(F11),
            "f12" => Key::Named(F12),
            "soft1" => Key::Named(Soft1),
            "soft2" => Key::Named(Soft2),
            "soft3" => Key::Named(Soft3),
            "soft4" => Key::Named(Soft4),
            "channeldown" => Key::Named(ChannelDown),
            "channelup" => Key::Named(ChannelUp),
            "close" => Key::Named(Close),
            "mailforward" => Key::Named(MailForward),
            "mailreply" => Key::Named(MailReply),
            "mailsend" => Key::Named(MailSend),
            "mediaclose" => Key::Named(MediaClose),
            "mediafastforward" => Key::Named(MediaFastForward),
            "mediapause" => Key::Named(MediaPause),
            "mediaplay" => Key::Named(MediaPlay),
            "mediaplaypause" => Key::Named(MediaPlayPause),
            "mediarecord" => Key::Named(MediaRecord),
            "mediarewind" => Key::Named(MediaRewind),
            "mediastop" => Key::Named(MediaStop),
            "mediatracknext" => Key::Named(MediaTrackNext),
            "mediatrackprevious" => Key::Named(MediaTrackPrevious),
            "new" => Key::Named(New),
            "open" => Key::Named(Open),
            "print" => Key::Named(Print),
            "save" => Key::Named(Save),
            "spellcheck" => Key::Named(SpellCheck),
            "key11" => Key::Named(Key11),
            "key12" => Key::Named(Key12),
            "audiobalanceleft" => Key::Named(AudioBalanceLeft),
            "audiobalanceright" => Key::Named(AudioBalanceRight),
            "audiobassboostdown" => Key::Named(AudioBassBoostDown),
            "audiobassboosttoggle" => Key::Named(AudioBassBoostToggle),
            "audiobassboostup" => Key::Named(AudioBassBoostUp),
            "audiofaderfront" => Key::Named(AudioFaderFront),
            "audiofaderrear" => Key::Named(AudioFaderRear),
            "audiosurroundmodenext" => Key::Named(AudioSurroundModeNext),
            "audiotrebledown" => Key::Named(AudioTrebleDown),
            "audiotrebleup" => Key::Named(AudioTrebleUp),
            "audiovolumedown" => Key::Named(AudioVolumeDown),
            "audiovolumeup" => Key::Named(AudioVolumeUp),
            "audiovolumemute" => Key::Named(AudioVolumeMute),
            "microphonetoggle" => Key::Named(MicrophoneToggle),
            "microphonevolumedown" => Key::Named(MicrophoneVolumeDown),
            "microphonevolumeup" => Key::Named(MicrophoneVolumeUp),
            "microphonevolumemute" => Key::Named(MicrophoneVolumeMute),
            "speechcorrectionlist" => Key::Named(SpeechCorrectionList),
            "speechinputtoggle" => Key::Named(SpeechInputToggle),
            "launchapplication1" => Key::Named(LaunchApplication1),
            "launchapplication2" => Key::Named(LaunchApplication2),
            "launchcalendar" => Key::Named(LaunchCalendar),
            "launchcontacts" => Key::Named(LaunchContacts),
            "launchmail" => Key::Named(LaunchMail),
            "launchmediaplayer" => Key::Named(LaunchMediaPlayer),
            "launchmusicplayer" => Key::Named(LaunchMusicPlayer),
            "launchphone" => Key::Named(LaunchPhone),
            "launchscreensaver" => Key::Named(LaunchScreenSaver),
            "launchspreadsheet" => Key::Named(LaunchSpreadsheet),
            "launchwebbrowser" => Key::Named(LaunchWebBrowser),
            "launchwebcam" => Key::Named(LaunchWebCam),
            "launchwordprocessor" => Key::Named(LaunchWordProcessor),
            "browserback" => Key::Named(BrowserBack),
            "browserfavorites" => Key::Named(BrowserFavorites),
            "browserforward" => Key::Named(BrowserForward),
            "browserhome" => Key::Named(BrowserHome),
            "browserrefresh" => Key::Named(BrowserRefresh),
            "browsersearch" => Key::Named(BrowserSearch),
            "browserstop" => Key::Named(BrowserStop),
            "appswitch" => Key::Named(AppSwitch),
            "call" => Key::Named(Call),
            "camera" => Key::Named(Camera),
            "camerafocus" => Key::Named(CameraFocus),
            "endcall" => Key::Named(EndCall),
            "goback" => Key::Named(GoBack),
            "gohome" => Key::Named(GoHome),
            "headsethook" => Key::Named(HeadsetHook),
            "lastnumberredial" => Key::Named(LastNumberRedial),
            "notification" => Key::Named(Notification),
            "mannermode" => Key::Named(MannerMode),
            "voicedial" => Key::Named(VoiceDial),
            "tv" => Key::Named(TV),
            "tv3dmode" => Key::Named(TV3DMode),
            "tvantennacable" => Key::Named(TVAntennaCable),
            "tvaudiodescription" => Key::Named(TVAudioDescription),
            "tvaudiodescriptionmixdown" => Key::Named(TVAudioDescriptionMixDown),
            "tvaudiodescriptionmixup" => Key::Named(TVAudioDescriptionMixUp),
            "tvcontentsmenu" => Key::Named(TVContentsMenu),
            "tvdataservice" => Key::Named(TVDataService),
            "tvinput" => Key::Named(TVInput),
            "tvinputcomponent1" => Key::Named(TVInputComponent1),
            "tvinputcomponent2" => Key::Named(TVInputComponent2),
            "tvinputcomposite1" => Key::Named(TVInputComposite1),
            "tvinputcomposite2" => Key::Named(TVInputComposite2),
            "tvinputhdmi1" => Key::Named(TVInputHDMI1),
            "tvinputhdmi2" => Key::Named(TVInputHDMI2),
            "tvinputhdmi3" => Key::Named(TVInputHDMI3),
            "tvinputhdmi4" => Key::Named(TVInputHDMI4),
            "tvinputvga1" => Key::Named(TVInputVGA1),
            "tvmediacontext" => Key::Named(TVMediaContext),
            "tvnetwork" => Key::Named(TVNetwork),
            "tvnumberentry" => Key::Named(TVNumberEntry),
            "tvpower" => Key::Named(TVPower),
            "tvradioservice" => Key::Named(TVRadioService),
            "tvsatellite" => Key::Named(TVSatellite),
            "tvsatellitebs" => Key::Named(TVSatelliteBS),
            "tvsatellitecs" => Key::Named(TVSatelliteCS),
            "tvsatellitetoggle" => Key::Named(TVSatelliteToggle),
            "tvterrestrialanalog" => Key::Named(TVTerrestrialAnalog),
            "tvterrestrialdigital" => Key::Named(TVTerrestrialDigital),
            "tvtimer" => Key::Named(TVTimer),
            "avrinput" => Key::Named(AVRInput),
            "avrpower" => Key::Named(AVRPower),
            "colorf0red" => Key::Named(ColorF0Red),
            "colorf1green" => Key::Named(ColorF1Green),
            "colorf2yellow" => Key::Named(ColorF2Yellow),
            "colorf3blue" => Key::Named(ColorF3Blue),
            "colorf4grey" => Key::Named(ColorF4Grey),
            "colorf5brown" => Key::Named(ColorF5Brown),
            "closedcaptiontoggle" => Key::Named(ClosedCaptionToggle),
            "dimmer" => Key::Named(Dimmer),
            "displayswap" => Key::Named(DisplaySwap),
            "dvr" => Key::Named(DVR),
            "exit" => Key::Named(Exit),
            "favoriteclear0" => Key::Named(FavoriteClear0),
            "favoriteclear1" => Key::Named(FavoriteClear1),
            "favoriteclear2" => Key::Named(FavoriteClear2),
            "favoriteclear3" => Key::Named(FavoriteClear3),
            "favoriterecall0" => Key::Named(FavoriteRecall0),
            "favoriterecall1" => Key::Named(FavoriteRecall1),
            "favoriterecall2" => Key::Named(FavoriteRecall2),
            "favoriterecall3" => Key::Named(FavoriteRecall3),
            "favoritestore0" => Key::Named(FavoriteStore0),
            "favoritestore1" => Key::Named(FavoriteStore1),
            "favoritestore2" => Key::Named(FavoriteStore2),
            "favoritestore3" => Key::Named(FavoriteStore3),
            "guide" => Key::Named(Guide),
            "guidenextday" => Key::Named(GuideNextDay),
            "guidepreviousday" => Key::Named(GuidePreviousDay),
            "info" => Key::Named(Info),
            "instantreplay" => Key::Named(InstantReplay),
            "link" => Key::Named(Link),
            "listprogram" => Key::Named(ListProgram),
            "livecontent" => Key::Named(LiveContent),
            "lock" => Key::Named(Lock),
            "mediaapps" => Key::Named(MediaApps),
            "mediaaudiotrack" => Key::Named(MediaAudioTrack),
            "medialast" => Key::Named(MediaLast),
            "mediaskipbackward" => Key::Named(MediaSkipBackward),
            "mediaskipforward" => Key::Named(MediaSkipForward),
            "mediastepbackward" => Key::Named(MediaStepBackward),
            "mediastepforward" => Key::Named(MediaStepForward),
            "mediatopmenu" => Key::Named(MediaTopMenu),
            "navigatein" => Key::Named(NavigateIn),
            "navigatenext" => Key::Named(NavigateNext),
            "navigateout" => Key::Named(NavigateOut),
            "navigateprevious" => Key::Named(NavigatePrevious),
            "nextfavoritechannel" => Key::Named(NextFavoriteChannel),
            "nextuserprofile" => Key::Named(NextUserProfile),
            "ondemand" => Key::Named(OnDemand),
            "pairing" => Key::Named(Pairing),
            "pinpdown" => Key::Named(PinPDown),
            "pinpmove" => Key::Named(PinPMove),
            "pinptoggle" => Key::Named(PinPToggle),
            "pinpup" => Key::Named(PinPUp),
            "playspeeddown" => Key::Named(PlaySpeedDown),
            "playspeedreset" => Key::Named(PlaySpeedReset),
            "playspeedup" => Key::Named(PlaySpeedUp),
            "randomtoggle" => Key::Named(RandomToggle),
            "rclowbattery" => Key::Named(RcLowBattery),
            "recordspeednext" => Key::Named(RecordSpeedNext),
            "rfbypass" => Key::Named(RfBypass),
            "scanchannelstoggle" => Key::Named(ScanChannelsToggle),
            "screenmodenext" => Key::Named(ScreenModeNext),
            "settings" => Key::Named(Settings),
            "splitscreentoggle" => Key::Named(SplitScreenToggle),
            "stbinput" => Key::Named(STBInput),
            "stbpower" => Key::Named(STBPower),
            "subtitle" => Key::Named(Subtitle),
            "teletext" => Key::Named(Teletext),
            "videomodenext" => Key::Named(VideoModeNext),
            "wink" => Key::Named(Wink),
            "zoomtoggle" => Key::Named(ZoomToggle),

            // Custom key name mappings
            "esc" => Key::Named(Escape),
            "space" => Key::Named(Space),
            "bs" => Key::Named(Backspace),
            "up" => Key::Named(ArrowUp),
            "down" => Key::Named(ArrowDown),
            "right" => Key::Named(ArrowRight),
            "left" => Key::Named(ArrowLeft),
            "del" => Key::Named(Delete),

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
            Self::Keyboard(key) => match key {
                Key::Character(c) => f.write_str(c),
                Key::Named(n) => match n {
                    floem::keyboard::NamedKey::Alt => f.write_str("Alt"),
                    floem::keyboard::NamedKey::AltGraph => f.write_str("AltGraph"),
                    floem::keyboard::NamedKey::CapsLock => f.write_str("CapsLock"),
                    floem::keyboard::NamedKey::Control => f.write_str("Control"),
                    floem::keyboard::NamedKey::Fn => f.write_str("Fn"),
                    floem::keyboard::NamedKey::FnLock => f.write_str("FnLock"),
                    floem::keyboard::NamedKey::NumLock => f.write_str("NumLock"),
                    floem::keyboard::NamedKey::ScrollLock => {
                        f.write_str("ScrollLock")
                    }
                    floem::keyboard::NamedKey::Shift => f.write_str("Shift"),
                    floem::keyboard::NamedKey::Symbol => f.write_str("Symbol"),
                    floem::keyboard::NamedKey::SymbolLock => {
                        f.write_str("SymbolLock")
                    }
                    floem::keyboard::NamedKey::Meta => f.write_str("Meta"),
                    floem::keyboard::NamedKey::Hyper => f.write_str("Hyper"),
                    floem::keyboard::NamedKey::Super => f.write_str("Super"),
                    floem::keyboard::NamedKey::Enter => f.write_str("Enter"),
                    floem::keyboard::NamedKey::Tab => f.write_str("Tab"),
                    floem::keyboard::NamedKey::Space => f.write_str("Space"),
                    floem::keyboard::NamedKey::ArrowDown => f.write_str("ArrowDown"),
                    floem::keyboard::NamedKey::ArrowLeft => f.write_str("ArrowLeft"),
                    floem::keyboard::NamedKey::ArrowRight => {
                        f.write_str("ArrowRight")
                    }
                    floem::keyboard::NamedKey::ArrowUp => f.write_str("ArrowUp"),
                    floem::keyboard::NamedKey::End => f.write_str("End"),
                    floem::keyboard::NamedKey::Home => f.write_str("Home"),
                    floem::keyboard::NamedKey::PageDown => f.write_str("PageDown"),
                    floem::keyboard::NamedKey::PageUp => f.write_str("PageUp"),
                    floem::keyboard::NamedKey::Backspace => f.write_str("Backspace"),
                    floem::keyboard::NamedKey::Clear => f.write_str("Clear"),
                    floem::keyboard::NamedKey::Copy => f.write_str("Copy"),
                    floem::keyboard::NamedKey::CrSel => f.write_str("CrSel"),
                    floem::keyboard::NamedKey::Cut => f.write_str("Cut"),
                    floem::keyboard::NamedKey::Delete => f.write_str("Delete"),
                    floem::keyboard::NamedKey::EraseEof => f.write_str("EraseEof"),
                    floem::keyboard::NamedKey::ExSel => f.write_str("ExSel"),
                    floem::keyboard::NamedKey::Insert => f.write_str("Insert"),
                    floem::keyboard::NamedKey::Paste => f.write_str("Paste"),
                    floem::keyboard::NamedKey::Redo => f.write_str("Redo"),
                    floem::keyboard::NamedKey::Undo => f.write_str("Undo"),
                    floem::keyboard::NamedKey::Accept => f.write_str("Accept"),
                    floem::keyboard::NamedKey::Again => f.write_str("Again"),
                    floem::keyboard::NamedKey::Attn => f.write_str("Attn"),
                    floem::keyboard::NamedKey::Cancel => f.write_str("Cancel"),
                    floem::keyboard::NamedKey::ContextMenu => {
                        f.write_str("ContextMenu")
                    }
                    floem::keyboard::NamedKey::Escape => f.write_str("Escape"),
                    floem::keyboard::NamedKey::Execute => f.write_str("Execute"),
                    floem::keyboard::NamedKey::Find => f.write_str("Find"),
                    floem::keyboard::NamedKey::Help => f.write_str("Help"),
                    floem::keyboard::NamedKey::Pause => f.write_str("Pause"),
                    floem::keyboard::NamedKey::Play => f.write_str("Play"),
                    floem::keyboard::NamedKey::Props => f.write_str("Props"),
                    floem::keyboard::NamedKey::Select => f.write_str("Select"),
                    floem::keyboard::NamedKey::ZoomIn => f.write_str("ZoomIn"),
                    floem::keyboard::NamedKey::ZoomOut => f.write_str("ZoomOut"),
                    floem::keyboard::NamedKey::BrightnessDown => {
                        f.write_str("BrightnessDown")
                    }
                    floem::keyboard::NamedKey::BrightnessUp => {
                        f.write_str("BrightnessUp")
                    }
                    floem::keyboard::NamedKey::Eject => f.write_str("Eject"),
                    floem::keyboard::NamedKey::LogOff => f.write_str("LogOff"),
                    floem::keyboard::NamedKey::Power => f.write_str("Power"),
                    floem::keyboard::NamedKey::PowerOff => f.write_str("PowerOff"),
                    floem::keyboard::NamedKey::PrintScreen => {
                        f.write_str("PrintScreen")
                    }
                    floem::keyboard::NamedKey::Hibernate => f.write_str("Hibernate"),
                    floem::keyboard::NamedKey::Standby => f.write_str("Standby"),
                    floem::keyboard::NamedKey::WakeUp => f.write_str("WakeUp"),
                    floem::keyboard::NamedKey::AllCandidates => {
                        f.write_str("AllCandidates")
                    }
                    floem::keyboard::NamedKey::Alphanumeric => {
                        f.write_str("Alphanumeric")
                    }
                    floem::keyboard::NamedKey::CodeInput => f.write_str("CodeInput"),
                    floem::keyboard::NamedKey::Compose => f.write_str("Compose"),
                    floem::keyboard::NamedKey::Convert => f.write_str("Convert"),
                    floem::keyboard::NamedKey::FinalMode => f.write_str("FinalMode"),
                    floem::keyboard::NamedKey::GroupFirst => {
                        f.write_str("GroupFirst")
                    }
                    floem::keyboard::NamedKey::GroupLast => f.write_str("GroupLast"),
                    floem::keyboard::NamedKey::GroupNext => f.write_str("GroupNext"),
                    floem::keyboard::NamedKey::GroupPrevious => {
                        f.write_str("GroupPrevious")
                    }
                    floem::keyboard::NamedKey::ModeChange => {
                        f.write_str("ModeChange")
                    }
                    floem::keyboard::NamedKey::NextCandidate => {
                        f.write_str("NextCandidate")
                    }
                    floem::keyboard::NamedKey::NonConvert => {
                        f.write_str("NonConvert")
                    }
                    floem::keyboard::NamedKey::PreviousCandidate => {
                        f.write_str("PreviousCandidate")
                    }
                    floem::keyboard::NamedKey::Process => f.write_str("Process"),
                    floem::keyboard::NamedKey::SingleCandidate => {
                        f.write_str("SingleCandidate")
                    }
                    floem::keyboard::NamedKey::HangulMode => {
                        f.write_str("HangulMode")
                    }
                    floem::keyboard::NamedKey::HanjaMode => f.write_str("HanjaMode"),
                    floem::keyboard::NamedKey::JunjaMode => f.write_str("JunjaMode"),
                    floem::keyboard::NamedKey::Eisu => f.write_str("Eisu"),
                    floem::keyboard::NamedKey::Hankaku => f.write_str("Hankaku"),
                    floem::keyboard::NamedKey::Hiragana => f.write_str("Hiragana"),
                    floem::keyboard::NamedKey::HiraganaKatakana => {
                        f.write_str("HiraganaKatakana")
                    }
                    floem::keyboard::NamedKey::KanaMode => f.write_str("KanaMode"),
                    floem::keyboard::NamedKey::KanjiMode => f.write_str("KanjiMode"),
                    floem::keyboard::NamedKey::Katakana => f.write_str("Katakana"),
                    floem::keyboard::NamedKey::Romaji => f.write_str("Romaji"),
                    floem::keyboard::NamedKey::Zenkaku => f.write_str("Zenkaku"),
                    floem::keyboard::NamedKey::ZenkakuHankaku => {
                        f.write_str("ZenkakuHankaku")
                    }
                    floem::keyboard::NamedKey::Soft1 => f.write_str("Soft1"),
                    floem::keyboard::NamedKey::Soft2 => f.write_str("Soft2"),
                    floem::keyboard::NamedKey::Soft3 => f.write_str("Soft3"),
                    floem::keyboard::NamedKey::Soft4 => f.write_str("Soft4"),
                    floem::keyboard::NamedKey::ChannelDown => {
                        f.write_str("ChannelDown")
                    }
                    floem::keyboard::NamedKey::ChannelUp => f.write_str("ChannelUp"),
                    floem::keyboard::NamedKey::Close => f.write_str("Close"),
                    floem::keyboard::NamedKey::MailForward => {
                        f.write_str("MailForward")
                    }
                    floem::keyboard::NamedKey::MailReply => f.write_str("MailReply"),
                    floem::keyboard::NamedKey::MailSend => f.write_str("MailSend"),
                    floem::keyboard::NamedKey::MediaClose => {
                        f.write_str("MediaClose")
                    }
                    floem::keyboard::NamedKey::MediaFastForward => {
                        f.write_str("MediaFastForward")
                    }
                    floem::keyboard::NamedKey::MediaPause => {
                        f.write_str("MediaPause")
                    }
                    floem::keyboard::NamedKey::MediaPlay => f.write_str("MediaPlay"),
                    floem::keyboard::NamedKey::MediaPlayPause => {
                        f.write_str("MediaPlayPause")
                    }
                    floem::keyboard::NamedKey::MediaRecord => {
                        f.write_str("MediaRecord")
                    }
                    floem::keyboard::NamedKey::MediaRewind => {
                        f.write_str("MediaRewind")
                    }
                    floem::keyboard::NamedKey::MediaStop => f.write_str("MediaStop"),
                    floem::keyboard::NamedKey::MediaTrackNext => {
                        f.write_str("MediaTrackNext")
                    }
                    floem::keyboard::NamedKey::MediaTrackPrevious => {
                        f.write_str("MediaTrackPrevious")
                    }
                    floem::keyboard::NamedKey::New => f.write_str("New"),
                    floem::keyboard::NamedKey::Open => f.write_str("Open"),
                    floem::keyboard::NamedKey::Print => f.write_str("Print"),
                    floem::keyboard::NamedKey::Save => f.write_str("Save"),
                    floem::keyboard::NamedKey::SpellCheck => {
                        f.write_str("SpellCheck")
                    }
                    floem::keyboard::NamedKey::Key11 => f.write_str("Key11"),
                    floem::keyboard::NamedKey::Key12 => f.write_str("Key12"),
                    floem::keyboard::NamedKey::AudioBalanceLeft => {
                        f.write_str("AudioBalanceLeft")
                    }
                    floem::keyboard::NamedKey::AudioBalanceRight => {
                        f.write_str("AudioBalanceRight")
                    }
                    floem::keyboard::NamedKey::AudioBassBoostDown => {
                        f.write_str("AudioBassBoostDown")
                    }
                    floem::keyboard::NamedKey::AudioBassBoostToggle => {
                        f.write_str("AudioBassBoostToggle")
                    }
                    floem::keyboard::NamedKey::AudioBassBoostUp => {
                        f.write_str("AudioBassBoostUp")
                    }
                    floem::keyboard::NamedKey::AudioFaderFront => {
                        f.write_str("AudioFaderFront")
                    }
                    floem::keyboard::NamedKey::AudioFaderRear => {
                        f.write_str("AudioFaderRear")
                    }
                    floem::keyboard::NamedKey::AudioSurroundModeNext => {
                        f.write_str("AudioSurroundModeNext")
                    }
                    floem::keyboard::NamedKey::AudioTrebleDown => {
                        f.write_str("AudioTrebleDown")
                    }
                    floem::keyboard::NamedKey::AudioTrebleUp => {
                        f.write_str("AudioTrebleUp")
                    }
                    floem::keyboard::NamedKey::AudioVolumeDown => {
                        f.write_str("AudioVolumeDown")
                    }
                    floem::keyboard::NamedKey::AudioVolumeUp => {
                        f.write_str("AudioVolumeUp")
                    }
                    floem::keyboard::NamedKey::AudioVolumeMute => {
                        f.write_str("AudioVolumeMute")
                    }
                    floem::keyboard::NamedKey::MicrophoneToggle => {
                        f.write_str("MicrophoneToggle")
                    }
                    floem::keyboard::NamedKey::MicrophoneVolumeDown => {
                        f.write_str("MicrophoneVolumeDown")
                    }
                    floem::keyboard::NamedKey::MicrophoneVolumeUp => {
                        f.write_str("MicrophoneVolumeUp")
                    }
                    floem::keyboard::NamedKey::MicrophoneVolumeMute => {
                        f.write_str("MicrophoneVolumeMute")
                    }
                    floem::keyboard::NamedKey::SpeechCorrectionList => {
                        f.write_str("SpeechCorrectionList")
                    }
                    floem::keyboard::NamedKey::SpeechInputToggle => {
                        f.write_str("SpeechInputToggle")
                    }
                    floem::keyboard::NamedKey::LaunchApplication1 => {
                        f.write_str("LaunchApplication1")
                    }
                    floem::keyboard::NamedKey::LaunchApplication2 => {
                        f.write_str("LaunchApplication2")
                    }
                    floem::keyboard::NamedKey::LaunchCalendar => {
                        f.write_str("LaunchCalendar")
                    }
                    floem::keyboard::NamedKey::LaunchContacts => {
                        f.write_str("LaunchContacts")
                    }
                    floem::keyboard::NamedKey::LaunchMail => {
                        f.write_str("LaunchMail")
                    }
                    floem::keyboard::NamedKey::LaunchMediaPlayer => {
                        f.write_str("LaunchMediaPlayer")
                    }
                    floem::keyboard::NamedKey::LaunchMusicPlayer => {
                        f.write_str("LaunchMusicPlayer")
                    }
                    floem::keyboard::NamedKey::LaunchPhone => {
                        f.write_str("LaunchPhone")
                    }
                    floem::keyboard::NamedKey::LaunchScreenSaver => {
                        f.write_str("LaunchScreenSaver")
                    }
                    floem::keyboard::NamedKey::LaunchSpreadsheet => {
                        f.write_str("LaunchSpreadsheet")
                    }
                    floem::keyboard::NamedKey::LaunchWebBrowser => {
                        f.write_str("LaunchWebBrowser")
                    }
                    floem::keyboard::NamedKey::LaunchWebCam => {
                        f.write_str("LaunchWebCam")
                    }
                    floem::keyboard::NamedKey::LaunchWordProcessor => {
                        f.write_str("LaunchWordProcessor")
                    }
                    floem::keyboard::NamedKey::BrowserBack => {
                        f.write_str("BrowserBack")
                    }
                    floem::keyboard::NamedKey::BrowserFavorites => {
                        f.write_str("BrowserFavorites")
                    }
                    floem::keyboard::NamedKey::BrowserForward => {
                        f.write_str("BrowserForward")
                    }
                    floem::keyboard::NamedKey::BrowserHome => {
                        f.write_str("BrowserHome")
                    }
                    floem::keyboard::NamedKey::BrowserRefresh => {
                        f.write_str("BrowserRefresh")
                    }
                    floem::keyboard::NamedKey::BrowserSearch => {
                        f.write_str("BrowserSearch")
                    }
                    floem::keyboard::NamedKey::BrowserStop => {
                        f.write_str("BrowserStop")
                    }
                    floem::keyboard::NamedKey::AppSwitch => f.write_str("AppSwitch"),
                    floem::keyboard::NamedKey::Call => f.write_str("Call"),
                    floem::keyboard::NamedKey::Camera => f.write_str("Camera"),
                    floem::keyboard::NamedKey::CameraFocus => {
                        f.write_str("CameraFocus")
                    }
                    floem::keyboard::NamedKey::EndCall => f.write_str("EndCall"),
                    floem::keyboard::NamedKey::GoBack => f.write_str("GoBack"),
                    floem::keyboard::NamedKey::GoHome => f.write_str("GoHome"),
                    floem::keyboard::NamedKey::HeadsetHook => {
                        f.write_str("HeadsetHook")
                    }
                    floem::keyboard::NamedKey::LastNumberRedial => {
                        f.write_str("LastNumberRedial")
                    }
                    floem::keyboard::NamedKey::Notification => {
                        f.write_str("Notification")
                    }
                    floem::keyboard::NamedKey::MannerMode => {
                        f.write_str("MannerMode")
                    }
                    floem::keyboard::NamedKey::VoiceDial => f.write_str("VoiceDial"),
                    floem::keyboard::NamedKey::TV => f.write_str("TV"),
                    floem::keyboard::NamedKey::TV3DMode => f.write_str("TV3DMode"),
                    floem::keyboard::NamedKey::TVAntennaCable => {
                        f.write_str("TVAntennaCable")
                    }
                    floem::keyboard::NamedKey::TVAudioDescription => {
                        f.write_str("TVAudioDescription")
                    }
                    floem::keyboard::NamedKey::TVAudioDescriptionMixDown => {
                        f.write_str("TVAudioDescriptionMixDown")
                    }
                    floem::keyboard::NamedKey::TVAudioDescriptionMixUp => {
                        f.write_str("TVAudioDescriptionMixUp")
                    }
                    floem::keyboard::NamedKey::TVContentsMenu => {
                        f.write_str("TVContentsMenu")
                    }
                    floem::keyboard::NamedKey::TVDataService => {
                        f.write_str("TVDataService")
                    }
                    floem::keyboard::NamedKey::TVInput => f.write_str("TVInput"),
                    floem::keyboard::NamedKey::TVInputComponent1 => {
                        f.write_str("TVInputComponent1")
                    }
                    floem::keyboard::NamedKey::TVInputComponent2 => {
                        f.write_str("TVInputComponent2")
                    }
                    floem::keyboard::NamedKey::TVInputComposite1 => {
                        f.write_str("TVInputComposite1")
                    }
                    floem::keyboard::NamedKey::TVInputComposite2 => {
                        f.write_str("TVInputComposite2")
                    }
                    floem::keyboard::NamedKey::TVInputHDMI1 => {
                        f.write_str("TVInputHDMI1")
                    }
                    floem::keyboard::NamedKey::TVInputHDMI2 => {
                        f.write_str("TVInputHDMI2")
                    }
                    floem::keyboard::NamedKey::TVInputHDMI3 => {
                        f.write_str("TVInputHDMI3")
                    }
                    floem::keyboard::NamedKey::TVInputHDMI4 => {
                        f.write_str("TVInputHDMI4")
                    }
                    floem::keyboard::NamedKey::TVInputVGA1 => {
                        f.write_str("TVInputVGA1")
                    }
                    floem::keyboard::NamedKey::TVMediaContext => {
                        f.write_str("TVMediaContext")
                    }
                    floem::keyboard::NamedKey::TVNetwork => f.write_str("TVNetwork"),
                    floem::keyboard::NamedKey::TVNumberEntry => {
                        f.write_str("TVNumberEntry")
                    }
                    floem::keyboard::NamedKey::TVPower => f.write_str("TVPower"),
                    floem::keyboard::NamedKey::TVRadioService => {
                        f.write_str("TVRadioService")
                    }
                    floem::keyboard::NamedKey::TVSatellite => {
                        f.write_str("TVSatellite")
                    }
                    floem::keyboard::NamedKey::TVSatelliteBS => {
                        f.write_str("TVSatelliteBS")
                    }
                    floem::keyboard::NamedKey::TVSatelliteCS => {
                        f.write_str("TVSatelliteCS")
                    }
                    floem::keyboard::NamedKey::TVSatelliteToggle => {
                        f.write_str("TVSatelliteToggle")
                    }
                    floem::keyboard::NamedKey::TVTerrestrialAnalog => {
                        f.write_str("TVTerrestrialAnalog")
                    }
                    floem::keyboard::NamedKey::TVTerrestrialDigital => {
                        f.write_str("TVTerrestrialDigital")
                    }
                    floem::keyboard::NamedKey::TVTimer => f.write_str("TVTimer"),
                    floem::keyboard::NamedKey::AVRInput => f.write_str("AVRInput"),
                    floem::keyboard::NamedKey::AVRPower => f.write_str("AVRPower"),
                    floem::keyboard::NamedKey::ColorF0Red => {
                        f.write_str("ColorF0Red")
                    }
                    floem::keyboard::NamedKey::ColorF1Green => {
                        f.write_str("ColorF1Green")
                    }
                    floem::keyboard::NamedKey::ColorF2Yellow => {
                        f.write_str("ColorF2Yellow")
                    }
                    floem::keyboard::NamedKey::ColorF3Blue => {
                        f.write_str("ColorF3Blue")
                    }
                    floem::keyboard::NamedKey::ColorF4Grey => {
                        f.write_str("ColorF4Grey")
                    }
                    floem::keyboard::NamedKey::ColorF5Brown => {
                        f.write_str("ColorF5Brown")
                    }
                    floem::keyboard::NamedKey::ClosedCaptionToggle => {
                        f.write_str("ClosedCaptionToggle")
                    }
                    floem::keyboard::NamedKey::Dimmer => f.write_str("Dimmer"),
                    floem::keyboard::NamedKey::DisplaySwap => {
                        f.write_str("DisplaySwap")
                    }
                    floem::keyboard::NamedKey::DVR => f.write_str("DVR"),
                    floem::keyboard::NamedKey::Exit => f.write_str("Exit"),
                    floem::keyboard::NamedKey::FavoriteClear0 => {
                        f.write_str("FavoriteClear0")
                    }
                    floem::keyboard::NamedKey::FavoriteClear1 => {
                        f.write_str("FavoriteClear1")
                    }
                    floem::keyboard::NamedKey::FavoriteClear2 => {
                        f.write_str("FavoriteClear2")
                    }
                    floem::keyboard::NamedKey::FavoriteClear3 => {
                        f.write_str("FavoriteClear3")
                    }
                    floem::keyboard::NamedKey::FavoriteRecall0 => {
                        f.write_str("FavoriteRecall0")
                    }
                    floem::keyboard::NamedKey::FavoriteRecall1 => {
                        f.write_str("FavoriteRecall1")
                    }
                    floem::keyboard::NamedKey::FavoriteRecall2 => {
                        f.write_str("FavoriteRecall2")
                    }
                    floem::keyboard::NamedKey::FavoriteRecall3 => {
                        f.write_str("FavoriteRecall3")
                    }
                    floem::keyboard::NamedKey::FavoriteStore0 => {
                        f.write_str("FavoriteStore0")
                    }
                    floem::keyboard::NamedKey::FavoriteStore1 => {
                        f.write_str("FavoriteStore1")
                    }
                    floem::keyboard::NamedKey::FavoriteStore2 => {
                        f.write_str("FavoriteStore2")
                    }
                    floem::keyboard::NamedKey::FavoriteStore3 => {
                        f.write_str("FavoriteStore3")
                    }
                    floem::keyboard::NamedKey::Guide => f.write_str("Guide"),
                    floem::keyboard::NamedKey::GuideNextDay => {
                        f.write_str("GuideNextDay")
                    }
                    floem::keyboard::NamedKey::GuidePreviousDay => {
                        f.write_str("GuidePreviousDay")
                    }
                    floem::keyboard::NamedKey::Info => f.write_str("Info"),
                    floem::keyboard::NamedKey::InstantReplay => {
                        f.write_str("InstantReplay")
                    }
                    floem::keyboard::NamedKey::Link => f.write_str("Link"),
                    floem::keyboard::NamedKey::ListProgram => {
                        f.write_str("ListProgram")
                    }
                    floem::keyboard::NamedKey::LiveContent => {
                        f.write_str("LiveContent")
                    }
                    floem::keyboard::NamedKey::Lock => f.write_str("Lock"),
                    floem::keyboard::NamedKey::MediaApps => f.write_str("MediaApps"),
                    floem::keyboard::NamedKey::MediaAudioTrack => {
                        f.write_str("MediaAudioTrack")
                    }
                    floem::keyboard::NamedKey::MediaLast => f.write_str("MediaLast"),
                    floem::keyboard::NamedKey::MediaSkipBackward => {
                        f.write_str("MediaSkipBackward")
                    }
                    floem::keyboard::NamedKey::MediaSkipForward => {
                        f.write_str("MediaSkipForward")
                    }
                    floem::keyboard::NamedKey::MediaStepBackward => {
                        f.write_str("MediaStepBackward")
                    }
                    floem::keyboard::NamedKey::MediaStepForward => {
                        f.write_str("MediaStepForward")
                    }
                    floem::keyboard::NamedKey::MediaTopMenu => {
                        f.write_str("MediaTopMenu")
                    }
                    floem::keyboard::NamedKey::NavigateIn => {
                        f.write_str("NavigateIn")
                    }
                    floem::keyboard::NamedKey::NavigateNext => {
                        f.write_str("NavigateNext")
                    }
                    floem::keyboard::NamedKey::NavigateOut => {
                        f.write_str("NavigateOut")
                    }
                    floem::keyboard::NamedKey::NavigatePrevious => {
                        f.write_str("NavigatePrevious")
                    }
                    floem::keyboard::NamedKey::NextFavoriteChannel => {
                        f.write_str("NextFavoriteChannel")
                    }
                    floem::keyboard::NamedKey::NextUserProfile => {
                        f.write_str("NextUserProfile")
                    }
                    floem::keyboard::NamedKey::OnDemand => f.write_str("OnDemand"),
                    floem::keyboard::NamedKey::Pairing => f.write_str("Pairing"),
                    floem::keyboard::NamedKey::PinPDown => f.write_str("PinPDown"),
                    floem::keyboard::NamedKey::PinPMove => f.write_str("PinPMove"),
                    floem::keyboard::NamedKey::PinPToggle => {
                        f.write_str("PinPToggle")
                    }
                    floem::keyboard::NamedKey::PinPUp => f.write_str("PinPUp"),
                    floem::keyboard::NamedKey::PlaySpeedDown => {
                        f.write_str("PlaySpeedDown")
                    }
                    floem::keyboard::NamedKey::PlaySpeedReset => {
                        f.write_str("PlaySpeedReset")
                    }
                    floem::keyboard::NamedKey::PlaySpeedUp => {
                        f.write_str("PlaySpeedUp")
                    }
                    floem::keyboard::NamedKey::RandomToggle => {
                        f.write_str("RandomToggle")
                    }
                    floem::keyboard::NamedKey::RcLowBattery => {
                        f.write_str("RcLowBattery")
                    }
                    floem::keyboard::NamedKey::RecordSpeedNext => {
                        f.write_str("RecordSpeedNext")
                    }
                    floem::keyboard::NamedKey::RfBypass => f.write_str("RfBypass"),
                    floem::keyboard::NamedKey::ScanChannelsToggle => {
                        f.write_str("ScanChannelsToggle")
                    }
                    floem::keyboard::NamedKey::ScreenModeNext => {
                        f.write_str("ScreenModeNext")
                    }
                    floem::keyboard::NamedKey::Settings => f.write_str("Settings"),
                    floem::keyboard::NamedKey::SplitScreenToggle => {
                        f.write_str("SplitScreenToggle")
                    }
                    floem::keyboard::NamedKey::STBInput => f.write_str("STBInput"),
                    floem::keyboard::NamedKey::STBPower => f.write_str("STBPower"),
                    floem::keyboard::NamedKey::Subtitle => f.write_str("Subtitle"),
                    floem::keyboard::NamedKey::Teletext => f.write_str("Teletext"),
                    floem::keyboard::NamedKey::VideoModeNext => {
                        f.write_str("VideoModeNext")
                    }
                    floem::keyboard::NamedKey::Wink => f.write_str("Wink"),
                    floem::keyboard::NamedKey::ZoomToggle => {
                        f.write_str("ZoomToggle")
                    }
                    floem::keyboard::NamedKey::F1 => f.write_str("F1"),
                    floem::keyboard::NamedKey::F2 => f.write_str("F2"),
                    floem::keyboard::NamedKey::F3 => f.write_str("F3"),
                    floem::keyboard::NamedKey::F4 => f.write_str("F4"),
                    floem::keyboard::NamedKey::F5 => f.write_str("F5"),
                    floem::keyboard::NamedKey::F6 => f.write_str("F6"),
                    floem::keyboard::NamedKey::F7 => f.write_str("F7"),
                    floem::keyboard::NamedKey::F8 => f.write_str("F8"),
                    floem::keyboard::NamedKey::F9 => f.write_str("F9"),
                    floem::keyboard::NamedKey::F10 => f.write_str("F10"),
                    floem::keyboard::NamedKey::F11 => f.write_str("F11"),
                    floem::keyboard::NamedKey::F12 => f.write_str("F12"),
                    floem::keyboard::NamedKey::F13 => f.write_str("F13"),
                    floem::keyboard::NamedKey::F14 => f.write_str("F14"),
                    floem::keyboard::NamedKey::F15 => f.write_str("F15"),
                    floem::keyboard::NamedKey::F16 => f.write_str("F16"),
                    floem::keyboard::NamedKey::F17 => f.write_str("F17"),
                    floem::keyboard::NamedKey::F18 => f.write_str("F18"),
                    floem::keyboard::NamedKey::F19 => f.write_str("F19"),
                    floem::keyboard::NamedKey::F20 => f.write_str("F20"),
                    floem::keyboard::NamedKey::F21 => f.write_str("F21"),
                    floem::keyboard::NamedKey::F22 => f.write_str("F22"),
                    floem::keyboard::NamedKey::F23 => f.write_str("F23"),
                    floem::keyboard::NamedKey::F24 => f.write_str("F24"),
                    floem::keyboard::NamedKey::F25 => f.write_str("F25"),
                    floem::keyboard::NamedKey::F26 => f.write_str("F26"),
                    floem::keyboard::NamedKey::F27 => f.write_str("F27"),
                    floem::keyboard::NamedKey::F28 => f.write_str("F28"),
                    floem::keyboard::NamedKey::F29 => f.write_str("F29"),
                    floem::keyboard::NamedKey::F30 => f.write_str("F30"),
                    floem::keyboard::NamedKey::F31 => f.write_str("F31"),
                    floem::keyboard::NamedKey::F32 => f.write_str("F32"),
                    floem::keyboard::NamedKey::F33 => f.write_str("F33"),
                    floem::keyboard::NamedKey::F34 => f.write_str("F34"),
                    floem::keyboard::NamedKey::F35 => f.write_str("F35"),
                    _ => f.write_str("_"),
                },
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
        // it should only do stuff with named keys, cause one letter keys are case sensitive
        let s = if s.len() > 1 {
            s.to_lowercase()
        } else {
            s.to_string()
        };

        KeyInput::keyboard_from_str(&s)
            .map(|key| KeyInput::Keyboard(key))
            .or_else(|| KeyInput::mouse_from_str(&s).map(KeyInput::Pointer))
            .ok_or(())
    }
}

impl Hash for KeyInput {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Keyboard(key) => key.hash(state),
            // TODO: Implement `Hash` for `druid::MouseButton`
            Self::Pointer(btn) => (*btn as u8).hash(state),
        }
    }
}

impl PartialEq for KeyInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (KeyInput::Keyboard(key_a), KeyInput::Keyboard(key_b)) => {
                key_a.eq(key_b)
            }
            (KeyInput::Pointer(a), KeyInput::Pointer(b)) => a.eq(b),
            _ => false,
        }
    }
}
