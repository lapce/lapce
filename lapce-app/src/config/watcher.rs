use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};

pub struct ConfigWatcher {
    tx: Sender<()>,
    delay_handler: Arc<AtomicBool>,
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        match event {
            Ok(event) => match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    if self
                        .delay_handler
                        .compare_exchange(
                            false,
                            true,
                            std::sync::atomic::Ordering::Relaxed,
                            std::sync::atomic::Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        let config_mutex = self.delay_handler.clone();
                        let tx = self.tx.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(
                                500,
                            ));
                            if let Err(err) = tx.send(()) {
                                tracing::error!("{:?}", err);
                            }
                            config_mutex
                                .store(false, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                }
                _ => {}
            },
            Err(err) => {
                tracing::error!("{:?}", err);
            }
        }
    }
}

impl ConfigWatcher {
    pub fn new(tx: Sender<()>) -> Self {
        Self {
            tx,
            delay_handler: Arc::new(AtomicBool::new(false)),
        }
    }
}
