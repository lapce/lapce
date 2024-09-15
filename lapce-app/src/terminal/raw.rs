use alacritty_terminal::{
    event::EventListener,
    grid::{Dimensions, Row},
    index::{Column, Direction, Line, Point},
    term::{
        cell::{Cell, Flags, LineLength},
        search::{Match, RegexIter, RegexSearch},
        test::TermSize,
    },
    vte::ansi,
    Term,
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
                if let Err(err) = self
                    .term_notification_tx
                    .send(TermNotification::RequestPaint)
                {
                    tracing::error!("{:?}", err);
                }
            }
            alacritty_terminal::event::Event::Title(s) => {
                if let Err(err) =
                    self.term_notification_tx.send(TermNotification::SetTitle {
                        term_id: self.term_id,
                        title: s,
                    })
                {
                    tracing::error!("{:?}", err);
                }
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
        let config = alacritty_terminal::term::Config {
            semantic_escape_chars: ",│`|\"' ()[]{}<>\t".to_string(),
            ..Default::default()
        };
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

    pub fn output(&mut self) -> Vec<String> {
        let row_cells: Vec<&Row<Cell>> =
            self.term.grid_mut().raw_data().iter().rev().collect();
        let mut lines = Vec::with_capacity(row_cells.len());
        let mut line = String::new();
        for row_cell in row_cells {
            let len = row_cell.line_length();
            if row_cell[Column(row_cell.len() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
            {
                row_cell.into_iter().for_each(|x| line.push(x.c));
            } else {
                row_cell[Column(0)..Column(len.0)]
                    .iter()
                    .for_each(|x| line.push(x.c));
                let mut new_line = String::new();
                std::mem::swap(&mut line, &mut new_line);
                lines.push(new_line);
            }
        }
        lines
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
