use std::{
    borrow::Cow,
    collections::VecDeque,
    io::{self, ErrorKind, Read, Write},
    num::NonZeroUsize,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use alacritty_terminal::{
    event::{OnResize, WindowSize},
    event_loop::Msg,
    tty::{self, EventedPty, EventedReadWrite, Options, Shell, setup_env},
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use directories::BaseDirs;
use lapce_rpc::{
    core::CoreRpcHandler,
    terminal::{TermId, TerminalProfile},
};
use polling::PollMode;

const READ_BUFFER_SIZE: usize = 0x10_0000;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const PTY_READ_WRITE_TOKEN: usize = 0;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PTY_CHILD_EVENT_TOKEN: usize = 1;

#[cfg(target_os = "windows")]
const PTY_READ_WRITE_TOKEN: usize = 2;
#[cfg(target_os = "windows")]
const PTY_CHILD_EVENT_TOKEN: usize = 1;

pub struct TerminalSender {
    tx: Sender<Msg>,
    poller: Arc<polling::Poller>,
}

impl TerminalSender {
    pub fn new(tx: Sender<Msg>, poller: Arc<polling::Poller>) -> Self {
        Self { tx, poller }
    }

    pub fn send(&self, msg: Msg) {
        if let Err(err) = self.tx.send(msg) {
            tracing::error!("{:?}", err);
        }
        if let Err(err) = self.poller.notify() {
            tracing::error!("{:?}", err);
        }
    }
}

pub struct Terminal {
    term_id: TermId,
    pub(crate) poller: Arc<polling::Poller>,
    pub(crate) pty: alacritty_terminal::tty::Pty,
    rx: Receiver<Msg>,
    pub tx: Sender<Msg>,
}

impl Terminal {
    pub fn new(
        term_id: TermId,
        profile: TerminalProfile,
        width: usize,
        height: usize,
    ) -> Result<Terminal> {
        let poll = polling::Poller::new()?.into();

        let options = Options {
            shell: Terminal::program(&profile),
            working_directory: Terminal::workdir(&profile),
            hold: false,
            env: profile.environment.unwrap_or_default(),
        };

        setup_env();

        #[cfg(target_os = "macos")]
        set_locale_environment();

        let size = WindowSize {
            num_lines: height as u16,
            num_cols: width as u16,
            cell_width: 1,
            cell_height: 1,
        };
        let pty = alacritty_terminal::tty::new(&options, size, 0)?;

        let (tx, rx) = crossbeam_channel::unbounded();

        Ok(Terminal {
            term_id,
            poller: poll,
            pty,
            tx,
            rx,
        })
    }

    pub fn run(&mut self, core_rpc: CoreRpcHandler) {
        let mut state = State::default();
        let mut buf = [0u8; READ_BUFFER_SIZE];

        let poll_opts = PollMode::Level;
        let mut interest = polling::Event::readable(0);

        // Register TTY through EventedRW interface.
        unsafe {
            self.pty
                .register(&self.poller, interest, poll_opts)
                .unwrap();
        }

        let mut events =
            polling::Events::with_capacity(NonZeroUsize::new(1024).unwrap());

        let timeout = Some(Duration::from_secs(6));
        let mut exit_code = None;
        'event_loop: loop {
            events.clear();
            if let Err(err) = self.poller.wait(&mut events, timeout) {
                match err.kind() {
                    ErrorKind::Interrupted => continue,
                    _ => panic!("EventLoop polling error: {err:?}"),
                }
            }

            // Handle channel events, if there are any.
            if !self.drain_recv_channel(&mut state) {
                break;
            }

            for event in events.iter() {
                match event.key {
                    PTY_CHILD_EVENT_TOKEN => {
                        if let Some(tty::ChildEvent::Exited(exited_code)) =
                            self.pty.next_child_event()
                        {
                            if let Err(err) = self.pty_read(&core_rpc, &mut buf) {
                                tracing::error!("{:?}", err);
                            }
                            exit_code = exited_code;
                            break 'event_loop;
                        }
                    }

                    PTY_READ_WRITE_TOKEN => {
                        if event.is_interrupt() {
                            // Don't try to do I/O on a dead PTY.
                            continue;
                        }

                        if event.readable {
                            if let Err(err) = self.pty_read(&core_rpc, &mut buf) {
                                // On Linux, a `read` on the master side of a PTY can fail
                                // with `EIO` if the client side hangs up.  In that case,
                                // just loop back round for the inevitable `Exited` event.
                                // This sucks, but checking the process is either racy or
                                // blocking.
                                #[cfg(target_os = "linux")]
                                if err.raw_os_error() == Some(libc::EIO) {
                                    continue;
                                }

                                tracing::error!(
                                    "Error reading from PTY in event loop: {}",
                                    err
                                );
                                break 'event_loop;
                            }
                        }

                        if event.writable {
                            if let Err(_err) = self.pty_write(&mut state) {
                                // error!(
                                //     "Error writing to PTY in event loop: {}",
                                //     err
                                // );
                                break 'event_loop;
                            }
                        }
                    }
                    _ => (),
                }
            }

            // Register write interest if necessary.
            let needs_write = state.needs_write();
            if needs_write != interest.writable {
                interest.writable = needs_write;

                // Re-register with new interest.
                self.pty
                    .reregister(&self.poller, interest, poll_opts)
                    .unwrap();
            }
        }
        core_rpc.terminal_process_stopped(self.term_id, exit_code);
        if let Err(err) = self.pty.deregister(&self.poller) {
            tracing::error!("{:?}", err);
        }
    }

    /// Drain the channel.
    ///
    /// Returns `false` when a shutdown message was received.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
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
    fn pty_read(
        &mut self,
        core_rpc: &CoreRpcHandler,
        buf: &mut [u8],
    ) -> io::Result<()> {
        loop {
            match self.pty.reader().read(buf) {
                Ok(0) => break,
                Ok(n) => {
                    core_rpc.update_terminal(self.term_id, buf[..n].to_vec());
                }
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        break;
                    }
                    _ => return Err(err),
                },
            }
        }
        Ok(())
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
                                break 'write_many;
                            }
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn workdir(profile: &TerminalProfile) -> Option<PathBuf> {
        if let Some(cwd) = &profile.workdir {
            match cwd.to_file_path() {
                Ok(cwd) => {
                    if cwd.exists() {
                        return Some(cwd);
                    }
                }
                Err(err) => {
                    tracing::error!("{:?}", err);
                }
            }
        }

        BaseDirs::new().map(|d| PathBuf::from(d.home_dir()))
    }

    fn program(profile: &TerminalProfile) -> Option<Shell> {
        if let Some(command) = &profile.command {
            if let Some(arguments) = &profile.arguments {
                Some(Shell::new(command.to_owned(), arguments.to_owned()))
            } else {
                Some(Shell::new(command.to_owned(), Vec::new()))
            }
        } else {
            None
        }
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

#[cfg(target_os = "macos")]
fn set_locale_environment() {
    let locale = locale_config::Locale::global_default()
        .to_string()
        .replace('-', "_");
    unsafe {
        std::env::set_var("LC_ALL", locale + ".UTF-8");
    }
}
