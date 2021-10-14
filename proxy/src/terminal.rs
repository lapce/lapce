use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        atomic::{self, AtomicU64},
        Arc,
    },
};

use alacritty_terminal::{
    ansi,
    config::Config,
    event::{Event, EventListener, Notify, OnResize},
    event_loop::{EventLoop, Msg, Notifier},
    grid::GridCell,
    index::Point,
    sync::FairMutex,
    term::{cell::Cell, RenderableCursor, SizeInfo},
    tty, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use lsp_types::notification;
use serde::{Deserialize, Deserializer, Serialize};

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
        static TERM_ID_COUNTER: Counter = Counter::new();
        Self(TERM_ID_COUNTER.next())
    }
}

pub enum TerminalHostEvent {
    UpdateContent {
        cursor: RenderableCursor,
        content: Vec<(Point, Cell)>,
    },
}

pub enum TerminalEvent {
    resize(SizeInfo),
    event(Event),
}

#[derive(Clone)]
pub struct Terminal {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: Sender<TerminalEvent>,
    host_sender: Sender<TerminalHostEvent>,
}

pub type TermConfig = Config<HashMap<String, String>>;

#[derive(Clone)]
pub struct EventProxy {
    sender: Sender<TerminalEvent>,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        self.sender.send(TerminalEvent::event(event));
    }
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> (Self, Receiver<TerminalHostEvent>) {
        let config = TermConfig::default();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let event_proxy = EventProxy {
            sender: sender.clone(),
        };
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);
        let pty = tty::new(&config, &size, None);
        let terminal = Term::new(&config, size, event_proxy.clone());
        let terminal = Arc::new(FairMutex::new(terminal));
        let event_loop =
            EventLoop::new(terminal.clone(), event_proxy, pty, false, false);
        let loop_tx = event_loop.channel();
        let notifier = Notifier(loop_tx.clone());
        event_loop.spawn();

        let (host_sender, host_receiver) = crossbeam_channel::unbounded();
        let terminal = Terminal {
            term: terminal,
            sender,
            host_sender,
        };
        terminal.run(receiver, notifier);
        (terminal, host_receiver)
    }

    pub fn resize(&self, width: usize, height: usize) {
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);
        self.sender.send(TerminalEvent::resize(size));
        self.term.lock().resize(size.clone());
    }

    fn run(&self, receiver: Receiver<TerminalEvent>, mut notifier: Notifier) {
        let term = self.term.clone();
        let host_sender = self.host_sender.clone();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let event = receiver.recv()?;
                match event {
                    TerminalEvent::resize(size) => {
                        notifier.on_resize(&size);
                    }
                    TerminalEvent::event(event) => match event {
                        Event::MouseCursorDirty => {}
                        Event::Title(_) => {}
                        Event::ResetTitle => {}
                        Event::ClipboardStore(_, _) => {}
                        Event::ClipboardLoad(_, _) => {}
                        Event::ColorRequest(_, _) => {}
                        Event::PtyWrite(s) => {
                            notifier.notify(s.into_bytes());
                        }
                        Event::CursorBlinkingChange(_) => {}
                        Event::Wakeup => {
                            let cursor =
                                term.lock().renderable_content().cursor.clone();
                            let content = term
                                .lock()
                                .renderable_content()
                                .display_iter
                                .filter_map(|c| {
                                    if (c.c == ' ' || c.c == '\t')
                                        && c.bg
                                            == ansi::Color::Named(
                                                ansi::NamedColor::Background,
                                            )
                                    {
                                        None
                                    } else {
                                        Some((c.point, c.cell.clone()))
                                    }
                                })
                                .collect::<Vec<(Point, Cell)>>();
                            let event = TerminalHostEvent::UpdateContent {
                                content: content,
                                cursor: cursor,
                            };
                            host_sender.send(event);
                        }
                        Event::Bell => {}
                        Event::Exit => {}
                    },
                }
            }
        });
    }

    pub fn insert<B: Into<String>>(&self, data: B) {
        self.sender
            .send(TerminalEvent::event(Event::PtyWrite(data.into())));
    }
}
