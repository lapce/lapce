use std::{sync::Arc};

use druid::{Target, ExtEventSink};
use parking_lot::Mutex;
// use lapce_core::config::ConfigWatcher;

use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};
pub use lapce_core::config::*;

pub struct ConfigWatcher {
    event_sink: ExtEventSink,
    delay_handler: Arc<Mutex<Option<()>>>,
}

impl ConfigWatcher {
    pub fn new(event_sink: ExtEventSink) -> Self {
        Self {
            event_sink,
            delay_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(event) = event {
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    *self.delay_handler.lock() = Some(());
                    let delay_handler = self.delay_handler.clone();
                    let event_sink = self.event_sink.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if delay_handler.lock().take().is_some() {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ReloadConfig,
                                Target::Auto,
                            );
                        }
                    });
                }
                _ => (),
            }
        }
    }
}