use std::sync::mpsc::Sender;

use alacritty_terminal::{
    Term,
    event::EventListener,
    grid::Dimensions,
    index::{Column, Direction, Line, Point},
    term::{
        cell::{Flags, LineLength},
        search::{Match, RegexIter, RegexSearch},
        test::TermSize,
    },
    vte::ansi,
};
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

    pub fn output(&self, line_num: usize) -> Vec<String> {
        let grid = self.term.grid();
        let mut lines = Vec::with_capacity(5);
        let mut rows = Vec::new();
        for line in (grid.topmost_line().0..=grid.bottommost_line().0)
            .map(Line)
            .rev()
        {
            let row_cell = &grid[line];
            if row_cell[Column(row_cell.len() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
            {
                rows.push(row_cell);
            } else {
                if !rows.is_empty() {
                    let mut new_line = Vec::new();
                    std::mem::swap(&mut rows, &mut new_line);
                    let line_str: String = new_line
                        .into_iter()
                        .rev()
                        .flat_map(|x| {
                            x.into_iter().take(x.line_length().0).map(|x| x.c)
                        })
                        .collect();
                    lines.push(line_str);
                    if lines.len() >= line_num {
                        break;
                    }
                }
                rows.push(row_cell);
            }
        }
        for line in &lines {
            tracing::info!("{}", line);
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
