use alacritty_terminal::{ansi, event::EventListener, term::test::TermSize, Term};
use lapce_proxy::terminal::TermConfig;
use lapce_rpc::{proxy::ProxyRpcHandler, terminal::TermId};

pub struct EventProxy {
    pub term_id: TermId,
    proxy: ProxyRpcHandler,
}

impl EventListener for EventProxy {
    fn send_event(&self, _event: alacritty_terminal::event::Event) {}
}

pub struct RawTerminal {
    pub parser: ansi::Processor,
    pub term: Term<EventProxy>,
    pub scroll_delta: f64,
}

impl RawTerminal {
    pub fn new(term_id: TermId, proxy: ProxyRpcHandler) -> Self {
        let config = TermConfig::default();
        let event_proxy = EventProxy { term_id, proxy };

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
