use std::{
    collections::HashMap,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    time::Instant,
};

use lapce_rpc::terminal::TermId;
use parking_lot::RwLock;

use super::raw::RawTerminal;

/// The notifications for terminals to send back to main thread
pub enum TermNotification {
    SetTitle { term_id: TermId, title: String },
    RequestPaint,
}

pub enum TermEvent {
    NewTerminal(Arc<RwLock<RawTerminal>>),
    UpdateContent(Vec<u8>),
    CloseTerminal,
}

pub fn terminal_update_process(
    receiver: Receiver<(TermId, TermEvent)>,
    term_notification_tx: Sender<TermNotification>,
) {
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
                if let Some(raw) = terminals.get(&term_id) {
                    {
                        raw.write().update_content(content);
                    }
                    last_event = receiver.try_recv().ok();
                    if last_event.is_some() {
                        if last_redraw.elapsed().as_millis() > 10 {
                            last_redraw = Instant::now();
                            if let Err(err) = term_notification_tx
                                .send(TermNotification::RequestPaint)
                            {
                                tracing::error!("{:?}", err);
                            }
                        }
                    } else {
                        last_redraw = Instant::now();
                        if let Err(err) =
                            term_notification_tx.send(TermNotification::RequestPaint)
                        {
                            tracing::error!("{:?}", err);
                        }
                    }
                }
            }
        }
    }
}
