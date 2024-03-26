use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::{
    event::EventListener, term::test::TermSize, vte::ansi, Term,
};
use crossbeam_channel::Sender;
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
        let config = alacritty_terminal::term::Config::default();
        let event_proxy = EventProxy {
            term_id,
            proxy,
            term_notification_tx,
        };

        let size = TermSize::new(50, 30);
        let term = Term::new(config, &size, event_proxy);
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

pub fn visible_regex_match_iter<'a, EventProxy>(
    term: &'a Term<EventProxy>,
    regex: &'a mut RegexSearch,
) -> impl Iterator<Item = Match> + 'a {
    let viewport_start = Line(-(term.grid().display_offset() as i32));
    let viewport_end = viewport_start + term.bottommost_line();
    let mut start = term.line_search_left(Point::new(viewport_start, Column(0)));
    let mut end = term.line_search_right(Point::new(viewport_end, Column(0)));
    start.line = start.line.max(viewport_start - MAX_SEARCH_LINES);
    end.line = end.line.min(viewport_end + MAX_SEARCH_LINES);

    RegexIter::new(start, end, Direction::Right, term, regex)
        .skip_while(move |rm| rm.end().line < viewport_start)
        .take_while(move |rm| rm.start().line <= viewport_end)
}
/// todo:should be improved
pub const MAX_SEARCH_LINES: usize = 100;
