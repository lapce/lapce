use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
    recommended_watcher,
};
use parking_lot::Mutex;

/// Wrapper around a `notify::Watcher`. It runs the inner watcher
/// in a separate thread, and communicates with it via a [crossbeam channel].
/// [crossbeam channel]: https://docs.rs/crossbeam-channel
pub struct FileWatcher {
    rx_event: Option<Receiver<Result<Event, notify::Error>>>,
    tx_event: Sender<Result<Event, notify::Error>>,
    inner: RecommendedWatcher,
    /// Recursive watches each get their own watcher, registered on a
    /// background thread. Registering a large tree can take a long time
    /// (the inotify backend walks every directory), and a watcher handles
    /// one request at a time, so doing it on `inner` would block the caller
    /// and every other watch until it finished.
    recursive: Arc<Mutex<Vec<(PathBuf, RecommendedWatcher)>>>,
    state: Arc<Mutex<WatcherState>>,
}

#[derive(Debug, Default)]
struct WatcherState {
    events: EventQueue,
    watchees: Vec<Watchee>,
}

/// Tracks a registered 'that-which-is-watched'.
#[doc(hidden)]
struct Watchee {
    path: PathBuf,
    recursive: bool,
    token: WatchToken,
    filter: Option<Box<PathFilter>>,
}

/// Token provided to `FileWatcher`, to associate events with
/// interested parties.
///
/// Note: `WatchToken`s are assumed to correspond with an
/// 'area of interest'; that is, they are used to route delivery
/// of events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchToken(pub usize);

/// A trait for types which can be notified of new events.
/// New events are accessible through the `FileWatcher` instance.
pub trait Notify: Send {
    fn notify(&self, events: Vec<(WatchToken, Event)>);
}

pub type EventQueue = VecDeque<(WatchToken, Event)>;

pub type PathFilter = dyn Fn(&Path) -> bool + Send + 'static;

impl FileWatcher {
    pub fn new() -> Self {
        let (tx_event, rx_event) = unbounded();

        let state = Arc::new(Mutex::new(WatcherState::default()));

        let inner =
            recommended_watcher(tx_event.clone()).expect("watcher should spawn");

        FileWatcher {
            rx_event: Some(rx_event),
            tx_event,
            inner,
            recursive: Arc::new(Mutex::new(Vec::new())),
            state,
        }
    }

    pub fn notify<T: Notify + 'static>(&mut self, peer: T) {
        let rx_event = self.rx_event.take().unwrap();
        let state = self.state.clone();
        std::thread::spawn(move || {
            while let Ok(Ok(event)) = rx_event.recv() {
                let mut events = Vec::new();
                {
                    let mut state = state.lock();
                    let WatcherState {
                        ref mut watchees, ..
                    } = *state;

                    watchees
                        .iter()
                        .filter(|w| w.wants_event(&event))
                        .map(|w| w.token)
                        .for_each(|t| events.push((t, event.clone())));
                }

                peer.notify(events);
            }
        });
    }

    /// Begin watching `path`. As `Event`s (documented in the
    /// [notify](https://docs.rs/notify) crate) arrive, they are stored
    /// with the associated `token` and a task is added to the runloop's
    /// idle queue.
    ///
    /// Delivery of events then requires that the runloop's handler
    /// correctly forward the `handle_idle` call to the interested party.
    pub fn watch(&mut self, path: &Path, recursive: bool, token: WatchToken) {
        self.watch_impl(path, recursive, token, None);
    }

    /// Like `watch`, but taking a predicate function that filters delivery
    /// of events based on their path.
    pub fn watch_filtered<F>(
        &mut self,
        path: &Path,
        recursive: bool,
        token: WatchToken,
        filter: F,
    ) where
        F: Fn(&Path) -> bool + Send + 'static,
    {
        let filter = Box::new(filter) as Box<PathFilter>;
        self.watch_impl(path, recursive, token, Some(filter));
    }

    fn watch_impl(
        &mut self,
        path: &Path,
        recursive: bool,
        token: WatchToken,
        filter: Option<Box<PathFilter>>,
    ) {
        let path = match path.canonicalize() {
            Ok(ref p) => p.to_owned(),
            Err(_) => {
                return;
            }
        };

        let mut state = self.state.lock();

        let w = Watchee {
            path,
            recursive,
            token,
            filter,
        };
        let mode = mode_from_bool(w.recursive);

        if !state.watchees.iter().any(|w2| w.path == w2.path) {
            if w.recursive {
                self.watch_recursive_in_background(w.path.clone());
            } else if let Err(err) = self.inner.watch(&w.path, mode) {
                tracing::error!("{:?}", err);
            }
        }

        state.watchees.push(w);
    }

    fn watch_recursive_in_background(&self, path: PathBuf) {
        let tx_event = self.tx_event.clone();
        let recursive = self.recursive.clone();
        let state = self.state.clone();
        std::thread::spawn(move || {
            let mut watcher = match recommended_watcher(tx_event) {
                Ok(watcher) => watcher,
                Err(err) => {
                    tracing::error!("{:?}", err);
                    return;
                }
            };
            if let Err(err) = watcher.watch(&path, RecursiveMode::Recursive) {
                tracing::error!("{:?}", err);
                return;
            }

            // The path may have been unwatched, or watched again, while
            // this was registering
            let state = state.lock();
            let mut recursive = recursive.lock();
            if state.watchees.iter().any(|w| w.recursive && w.path == path)
                && !recursive.iter().any(|(p, _)| *p == path)
            {
                recursive.push((path, watcher));
            }
        });
    }

    /// Removes the provided token/path pair from the watch list.
    /// Does not stop watching this path, if it is associated with
    /// other tokens.
    pub fn unwatch(&mut self, path: &Path, token: WatchToken) {
        let mut state = self.state.lock();

        let idx = state
            .watchees
            .iter()
            .position(|w| w.token == token && w.path == path);

        if let Some(idx) = idx {
            let removed = state.watchees.remove(idx);
            if !state.watchees.iter().any(|w| w.path == removed.path) {
                if removed.recursive {
                    // Dropping the watcher stops it. Child paths are
                    // registered on other watchers, so they are unaffected.
                    self.recursive.lock().retain(|(p, _)| *p != removed.path);
                } else if let Err(err) = self.inner.unwatch(&removed.path) {
                    tracing::error!("{:?}", err);
                }
            }
        }
    }

    /// Takes ownership of this `Watcher`'s current event queue.
    pub fn take_events(&self) -> VecDeque<(WatchToken, Event)> {
        let mut state = self.state.lock();
        let WatcherState { ref mut events, .. } = *state;
        std::mem::take(events)
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchee {
    fn wants_event(&self, event: &Event) -> bool {
        match &event.kind {
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                if event.paths.len() == 2 {
                    //There will be two paths. First is "from" and other is "to".
                    self.applies_to_path(&event.paths[0])
                        || self.applies_to_path(&event.paths[1])
                } else {
                    false
                }
            }
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => {
                if event.paths.len() == 1 {
                    self.applies_to_path(&event.paths[0])
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn applies_to_path(&self, path: &Path) -> bool {
        let general_case = if path.starts_with(&self.path) {
            (self.recursive || self.path == path)
                || path.parent() == Some(self.path.as_path())
        } else {
            false
        };

        if let Some(ref filter) = self.filter {
            general_case && filter(path)
        } else {
            general_case
        }
    }
}
impl std::fmt::Debug for Watchee {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Watchee path: {:?}, r {}, t {} f {}",
            self.path,
            self.recursive,
            self.token.0,
            self.filter.is_some()
        )
    }
}

fn mode_from_bool(is_recursive: bool) -> RecursiveMode {
    if is_recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossbeam_channel::{Sender, unbounded};
    use notify::Event;

    use super::{FileWatcher, Notify, WatchToken};

    struct ChannelNotify(Sender<Vec<(WatchToken, Event)>>);

    impl Notify for ChannelNotify {
        fn notify(&self, events: Vec<(WatchToken, Event)>) {
            let _ = self.0.send(events);
        }
    }

    #[test]
    fn recursive_watch_delivers_events() {
        let dir = std::env::temp_dir()
            .join(format!("lapce-watcher-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();

        let (tx, rx) = unbounded();
        let mut watcher = FileWatcher::new();
        watcher.notify(ChannelNotify(tx));
        watcher.watch(&dir, true, WatchToken(1));

        // The watch is registered in the background, so keep writing until
        // an event for the nested file arrives
        let file = dir.join("nested").join("file");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut received = false;
        while !received && Instant::now() < deadline {
            std::fs::write(&file, "lapce").unwrap();
            if let Ok(events) = rx.recv_timeout(Duration::from_millis(100)) {
                received = events.iter().any(|(token, event)| {
                    *token == WatchToken(1)
                        && event.paths.iter().any(|p| p.ends_with("nested/file"))
                });
            }
        }

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(received, "no event from the recursive watch");
    }
}
