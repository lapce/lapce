use std::{borrow::Cow, collections::HashMap, sync::Arc};

use alacritty_terminal::{
    config::Config,
    event::{Event, EventListener, Notify},
    event_loop::{EventLoop, Notifier},
    grid::GridCell,
    sync::FairMutex,
    term::SizeInfo,
    tty, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use lsp_types::notification;

#[derive(Clone)]
pub struct Terminal {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: Sender<Event>,
}

pub type TermConfig = Config<HashMap<String, String>>;

#[derive(Clone)]
pub struct EventProxy {
    sender: Sender<Event>,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        self.sender.send(event);
    }
}

impl Terminal {
    pub fn new() -> Self {
        let config = TermConfig::default();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let event_proxy = EventProxy {
            sender: sender.clone(),
        };
        let size = SizeInfo::new(10.0, 10.0, 1.0, 1.0, 0.0, 0.0, true);
        let pty = tty::new(&config, &size, None);
        let terminal = Term::new(&config, size, event_proxy.clone());
        let terminal = Arc::new(FairMutex::new(terminal));
        let event_loop =
            EventLoop::new(terminal.clone(), event_proxy, pty, false, false);
        let loop_tx = event_loop.channel();
        let notifier = Notifier(loop_tx.clone());
        event_loop.spawn();

        let terminal = Terminal {
            term: terminal,
            sender,
        };
        terminal.run(receiver, notifier);
        terminal
    }

    fn run(&self, receiver: Receiver<Event>, notifier: Notifier) {
        let term = self.term.clone();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let event = receiver.recv()?;
                match event {
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
                        for cell in term.lock().renderable_content().display_iter {
                            if !cell.is_empty() {
                                println!("{:?}", cell);
                            }
                        }
                    }
                    Event::Bell => {}
                    Event::Exit => {}
                }
            }
        });
    }

    pub fn insert<B: Into<String>>(&self, data: B) {
        self.sender.send(Event::PtyWrite(data.into()));
    }
}
