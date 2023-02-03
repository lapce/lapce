#[cfg(target_os = "linux")]
use std::process::Command;
use std::{
    borrow::Cow,
    collections::VecDeque,
    io::{self, ErrorKind, Read, Write},
    path::PathBuf,
};

use alacritty_terminal::{
    config::Program,
    event::{OnResize, WindowSize},
    event_loop::Msg,
    tty::{self, setup_env, EventedPty, EventedReadWrite},
};
use directories::BaseDirs;
use lapce_rpc::{core::CoreRpcHandler, terminal::TermId};
#[cfg(not(windows))]
use mio::unix::UnixReady;
#[allow(deprecated)]
use mio::{
    channel::{channel, Receiver, Sender},
    Events, PollOpt, Ready,
};

const READ_BUFFER_SIZE: usize = 0x10_0000;

pub type TermConfig = alacritty_terminal::config::Config;

pub struct Terminal {
    term_id: TermId,
    poll: mio::Poll,
    pty: alacritty_terminal::tty::Pty,

    #[allow(deprecated)]
    rx: Receiver<Msg>,

    #[allow(deprecated)]
    pub tx: Sender<Msg>,
}

impl Terminal {
    pub fn new(
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
        width: usize,
        height: usize,
    ) -> Terminal {
        let poll = mio::Poll::new().unwrap();
        let mut config = TermConfig::default();
        config.pty_config.working_directory =
            if cwd.is_some() && cwd.clone().unwrap().exists() {
                cwd
            } else {
                BaseDirs::new().map(|d| PathBuf::from(d.home_dir()))
            };
        let shell = shell.trim();
        let flatpak_use_host_terminal = flatpak_should_use_host_terminal();

        if !shell.is_empty() || flatpak_use_host_terminal {
            let mut parts = shell.split(' ');

            if flatpak_use_host_terminal {
                let flatpak_spawn_path = "/usr/bin/flatpak-spawn".to_string();
                let host_shell = flatpak_get_default_host_shell();

                let args = if shell.is_empty() {
                    vec!["--host".to_string(), host_shell]
                } else {
                    vec![
                        "--host".to_string(),
                        host_shell,
                        "-c".to_string(),
                        shell.to_string(),
                    ]
                };

                config.pty_config.shell = Some(Program::WithArgs {
                    program: flatpak_spawn_path,
                    args,
                })
            } else {
                let program = parts.next().unwrap();
                if let Ok(p) = which::which(program) {
                    config.pty_config.shell = Some(Program::WithArgs {
                        program: p.to_str().unwrap().to_string(),
                        args: parts.map(|p| p.to_string()).collect::<Vec<String>>(),
                    })
                }
            }
        }
        setup_env(&config);

        #[cfg(target_os = "macos")]
        set_locale_environment();

        let size = WindowSize {
            num_lines: height as u16,
            num_cols: width as u16,
            cell_width: 1,
            cell_height: 1,
        };
        let pty = alacritty_terminal::tty::new(&config.pty_config, size, 0).unwrap();

        #[allow(deprecated)]
        let (tx, rx) = channel();

        Terminal {
            term_id,
            poll,
            pty,
            tx,
            rx,
        }
    }

    pub fn run(&mut self, core_rpc: CoreRpcHandler) {
        let mut tokens = (0..).map(Into::into);
        let poll_opts = PollOpt::edge() | PollOpt::oneshot();

        let channel_token = tokens.next().unwrap();
        self.poll
            .register(&self.rx, channel_token, Ready::readable(), poll_opts)
            .unwrap();

        self.pty
            .register(&self.poll, &mut tokens, Ready::readable(), poll_opts)
            .unwrap();

        let mut buf = [0u8; READ_BUFFER_SIZE];
        let mut events = Events::with_capacity(1024);
        let mut state = State::default();

        'event_loop: loop {
            let _ = self.poll.poll(&mut events, None);
            for event in events.iter() {
                match event.token() {
                    token if token == channel_token => {
                        if !self.channel_event(channel_token, &mut state) {
                            break 'event_loop;
                        }
                    }

                    token if token == self.pty.child_event_token() => {
                        if let Some(tty::ChildEvent::Exited) =
                            self.pty.next_child_event()
                        {
                            core_rpc.close_terminal(self.term_id);
                            break 'event_loop;
                        }
                    }
                    token
                        if token == self.pty.read_token()
                            || token == self.pty.write_token() =>
                    {
                        #[cfg(unix)]
                        if UnixReady::from(event.readiness()).is_hup() {
                            // Don't try to do I/O on a dead PTY.
                            continue;
                        }

                        if event.readiness().is_readable() {
                            match self.pty.reader().read(&mut buf) {
                                Ok(n) => {
                                    core_rpc.update_terminal(
                                        self.term_id,
                                        buf[..n].to_vec(),
                                    );
                                }
                                Err(_e) => (),
                            }
                        }

                        if event.readiness().is_writable() {
                            if let Err(_err) = self.pty_write(&mut state) {}
                        }
                    }

                    _ => (),
                }
            }
            // Register write interest if necessary.
            let mut interest = Ready::readable();
            if state.needs_write() {
                interest.insert(Ready::writable());
            }
            // Reregister with new interest.
            self.pty
                .reregister(&self.poll, interest, poll_opts)
                .unwrap();
        }
        let _ = self.poll.deregister(&self.rx);
        let _ = self.pty.deregister(&self.poll);
    }

    /// Drain the channel.
    ///
    /// Returns `false` when a shutdown message was received.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        #[allow(deprecated)]
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Shutdown => return false,
                Msg::Resize(size) => self.pty.on_resize(size),
            }
        }

        true
    }

    #[inline]
    fn channel_event(&mut self, token: mio::Token, state: &mut State) -> bool {
        if !self.drain_recv_channel(state) {
            return false;
        }

        self.poll
            .reregister(
                &self.rx,
                token,
                Ready::readable(),
                PollOpt::edge() | PollOpt::oneshot(),
            )
            .unwrap();

        true
    }

    #[inline]
    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        break 'write_many;
                    }
                    Ok(n) => {
                        current.advance(n);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(err) => {
                        state.set_current(Some(current));
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                                break 'write_many
                            }
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    #[inline]
    fn new(c: Cow<'static, [u8]>) -> Writing {
        Writing {
            source: c,
            written: 0,
        }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.written += n;
    }

    #[inline]
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    #[inline]
    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

#[derive(Default)]
pub struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
}

impl State {
    #[inline]
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    #[inline]
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    #[inline]
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    #[inline]
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    #[inline]
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

/// Code taken (and slightly modified) from wezterm's env-bootstrap
/// Source: https://github.com/wez/wezterm/blob/691ec187ba29fcae45ddf4e4f88fd02c49988c86/env-bootstrap/src/lib.rs#L86
/// License: https://github.com/wez/wezterm/blob/691ec187ba29fcae45ddf4e4f88fd02c49988c86/LICENSE.md
/// SPDX: MIT
#[cfg(target_os = "macos")]
fn set_locale_environment() {
    use cocoa::base::id;
    use cocoa::foundation::NSString;
    use objc::runtime::Object;
    use objc::*;

    fn lang_is_set() -> bool {
        match std::env::var_os("LANG") {
            None => false,
            Some(lang) => !lang.is_empty(),
        }
    }

    if !lang_is_set() {
        unsafe fn nsstring_to_str<'a>(ns: *mut Object) -> &'a str {
            let data = NSString::UTF8String(ns as id) as *const u8;
            let len = NSString::len(ns as id);
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8_unchecked(bytes)
        }

        unsafe {
            let locale: *mut Object =
                msg_send![class!(NSLocale), autoupdatingCurrentLocale];
            let lang_code_obj: *mut Object = msg_send![locale, languageCode];
            let country_code_obj: *mut Object = msg_send![locale, countryCode];

            {
                let lang_code = nsstring_to_str(lang_code_obj);
                let country_code = nsstring_to_str(country_code_obj);

                let candidate = format!("{lang_code}_{country_code}.UTF-8");
                let candidate_cstr =
                    std::ffi::CString::new(<&[u8]>::clone(&candidate.as_bytes()))
                        .expect("make cstr from str");

                // If this looks like a working locale then export it to
                // the environment so that our child processes inherit it.
                let old = libc::setlocale(libc::LC_CTYPE, std::ptr::null());
                if !libc::setlocale(libc::LC_CTYPE, candidate_cstr.as_ptr())
                    .is_null()
                {
                    std::env::set_var("LANG", &candidate);
                } else {
                    log::warn!(
                        "setlocale({}) failed, fall back to en_US.UTF-8",
                        candidate
                    );
                    std::env::set_var("LANG", "en_US.UTF-8");
                }
                libc::setlocale(libc::LC_CTYPE, old);
            }

            let _: () = msg_send![lang_code_obj, release];
            let _: () = msg_send![country_code_obj, release];
            let _: () = msg_send![locale, release];
        }
    }
}

#[inline]
#[cfg(not(target_os = "linux"))]
fn flatpak_get_default_host_shell() -> String {
    panic!(
        "This should never be reached. If it is, ensure you don't have a file
        called .flatpak-info in your root directory"
    );
}

#[inline]
#[cfg(target_os = "linux")]
fn flatpak_get_default_host_shell() -> String {
    let env_string = Command::new("flatpak-spawn")
        .arg("--host")
        .arg("printenv")
        .output()
        .unwrap()
        .stdout;

    let env_string = String::from_utf8(env_string).unwrap();

    for env_pair in env_string.split('\n') {
        let name_value: Vec<&str> = env_pair.split('=').collect();

        if name_value[0] == "SHELL" {
            return name_value[1].to_string();
        }
    }

    // In case SHELL isn't set for whatever reason, fall back to this
    "/bin/sh".to_string()
}

#[inline]
#[cfg(not(target_os = "linux"))]
fn flatpak_should_use_host_terminal() -> bool {
    false // Flatpak is only available on Linux
}

#[inline]
#[cfg(target_os = "linux")]
fn flatpak_should_use_host_terminal() -> bool {
    use std::path::Path;

    const FLATPAK_INFO_PATH: &str = "/.flatpak-info";

    // The de-facto way of checking whether one is inside of a Flatpak container is by checking for
    // the presence of /.flatpak-info in the filesystem
    if !Path::new(FLATPAK_INFO_PATH).exists() {
        return false;
    }

    // Ensure flatpak-spawn --host can execute a basic command
    Command::new("flatpak-spawn")
        .arg("--host")
        .arg("true")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
