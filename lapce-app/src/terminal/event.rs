use std::{collections::HashMap, sync::Arc, time::Instant};

use crossbeam_channel::Receiver;
use lapce_rpc::terminal::TermId;
use parking_lot::RwLock;

use super::raw::RawTerminal;

pub enum TermEvent {
    NewTerminal(Arc<RwLock<RawTerminal>>),
    UpdateContent(Vec<u8>),
    CloseTerminal,
}

pub fn terminal_update_process(receiver: Receiver<(TermId, TermEvent)>) {
    let mut terminals = HashMap::new();
    let mut last_redraw = Instant::now();
    let mut last_event = None;
    loop {
        let (term_id, event) = if let Some((term_id, event)) = last_event.take() {
            (term_id, event)
        } else {
            match receiver.recv() {
                Ok((term_id, event)) => (term_id, event),
                Err(_) => return,
            }
        };
        match event {
            TermEvent::CloseTerminal => {
                terminals.remove(&term_id);
            }
            TermEvent::NewTerminal(raw) => {
                terminals.insert(term_id, raw);
            }
            TermEvent::UpdateContent(content) => {
                if let Some(raw) = terminals.get_mut(&term_id) {
                    raw.write().update_content(content);
                    last_event = receiver.try_recv().ok();
                    if last_event.is_some() {
                        if last_redraw.elapsed().as_millis() > 10 {
                            last_redraw = Instant::now();
                            // redraw now
                            // let _ = event_sink.submit_command(
                            //     LAPCE_UI_COMMAND,
                            //     LapceUICommand::RequestPaint,
                            //     Target::Widget(tab_id),
                            // );
                        }
                    } else {
                        last_redraw = Instant::now();
                        // redraw now
                        // let _ = event_sink.submit_command(
                        //     LAPCE_UI_COMMAND,
                        //     LapceUICommand::RequestPaint,
                        //     Target::Widget(tab_id),
                        // );
                    }
                }
            }
        }
    }
}
