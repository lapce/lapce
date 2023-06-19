use alacritty_terminal::{ansi, event::EventListener, term::test::TermSize, Term};
use crossbeam_channel::Sender;
use lapce_proxy::terminal::TermConfig;
use lapce_rpc::{proxy::ProxyRpcHandler, terminal::TermId};

use super::event::TermNotification;

pub struct EventProxy {
    term_id: TermId,
    proxy: ProxyRpcHandler,
    term_notification_tx: Sender<TermNotification>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        match event {
            alacritty_terminal::event::Event::PtyWrite(s) => {
                self.proxy.terminal_write(self.term_id, s);
            }
            alacritty_terminal::event::Event::MouseCursorDirty => {
                let _ = self
                    .term_notification_tx
                    .send(TermNotification::RequestPaint);
            }
            alacritty_terminal::event::Event::Title(s) => {
                let _ = self.term_notification_tx.send(TermNotification::SetTitle {
                    term_id: self.term_id,
                    title: s,
                });
            }
            _ => (),
        }
    }
}

pub struct RawTerminal {
    pub parser: ansi::Processor,
    pub term: Term<EventProxy>,
    pub scroll_delta: f64,
}

impl RawTerminal {
    pub fn new(
        term_id: TermId,
        proxy: ProxyRpcHandler,
        term_notification_tx: Sender<TermNotification>,
    ) -> Self {
        let config = TermConfig::default();
        let event_proxy = EventProxy {
            term_id,
            proxy,
            term_notification_tx,
        };

        let size = TermSize::new(50, 30);
        let term = Term::new(&config, &size, event_proxy);
        let parser = ansi::Processor::new();

        Self {
            parser,
            term,
            scroll_delta: 0.0,
        }
    }

    pub fn update_content(&mut self, content: Vec<u8>) {
        for byte in content {
            self.parser.advance(&mut self.term, byte);
        }
    }
}
