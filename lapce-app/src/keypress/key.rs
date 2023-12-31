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
                Key::Named(n) => {
                    let text = n.to_text();
                    match text {
                        Some(t) => f.write_str(t),
                        None => f.write_str("Unidentified")
                    }
                }
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
