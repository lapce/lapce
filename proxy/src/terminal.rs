use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    fs::File,
    io::{self, ErrorKind, Read, Write},
    sync::atomic::{self, AtomicU64},
};

use alacritty_terminal::{
    ansi,
    event::OnResize,
    event_loop::Msg,
    term::SizeInfo,
    tty::{self, EventedPty, EventedReadWrite},
};
use mio::{
    channel::{channel, Receiver, Sender},
    unix::UnixReady,
    Events, PollOpt, Ready,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;

use crate::dispatch::Dispatcher;

const READ_BUFFER_SIZE: usize = 0x10_0000;

pub struct Counter(AtomicU64);

impl Counter {
    pub const fn new() -> Counter {
        Counter(AtomicU64::new(1))
    }

    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, atomic::Ordering::Relaxed)
    }
}

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct TermId(pub u64);

impl TermId {
    pub fn next() -> Self {
        static TERMINAL_ID_COUNTER: Counter = Counter::new();
        Self(TERMINAL_ID_COUNTER.next())
    }
}

pub type TermConfig = alacritty_terminal::config::Config<HashMap<String, String>>;

pub struct Terminal {
    term_id: TermId,
    poll: mio::Poll,
    pty: alacritty_terminal::tty::Pty,
    rx: Receiver<Msg>,
    pub tx: Sender<Msg>,
}

impl Terminal {
    pub fn new(term_id: TermId, width: usize, height: usize) -> Terminal {
        let poll = mio::Poll::new().unwrap();
        let config = TermConfig::default();
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);
        let mut pty = alacritty_terminal::tty::new(&config, &size, None);

        let (tx, rx) = channel();

        Terminal {
            term_id,
            poll,
            pty,
            tx,
            rx,
        }
    }

    pub fn run(&mut self, dispatcher: Dispatcher) {
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
            self.poll.poll(&mut events, None);
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
                            dispatcher.send_notification(
                                "close_terminal",
                                json!({
                                    "term_id": self.term_id,
                                }),
                            );
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
                                    dispatcher.send_notification(
                                        "update_terminal",
                                        json!({
                                            "term_id": self.term_id,
                                            "content": base64::encode(&buf[..n]),
                                        }),
                                    );
                                }
                                Err(e) => (),
                            }
                        }

                        if event.readiness().is_writable() {
                            if let Err(err) = self.pty_write(&mut state) {}
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
    }

    /// Drain the channel.
    ///
    /// Returns `false` when a shutdown message was received.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Shutdown => return false,
                Msg::Resize(size) => self.pty.on_resize(&size),
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
    parser: ansi::Processor,
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
