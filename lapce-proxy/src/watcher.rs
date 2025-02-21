use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
};

use crossbeam_channel::{Receiver, unbounded};
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
    inner: RecommendedWatcher,
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

        let inner = recommended_watcher(tx_event).expect("watcher should spawn");

        FileWatcher {
            rx_event: Some(rx_event),
            inner,
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
            if let Err(err) = self.inner.watch(&w.path, mode) {
                tracing::error!("{:?}", err);
            }
        }

        state.watchees.push(w);
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
                if let Err(err) = self.inner.unwatch(&removed.path) {
                    tracing::error!("{:?}", err);
                }
            }
            //TODO: Ideally we would be tracking what paths we're watching with
            // some prefix-tree-like structure, which would let us keep track
            // of when some child path might need to be reregistered. How this
            // works and when registration would be required is dependent on
            // the underlying notification mechanism, however. There's an
            // in-progress rewrite of the Notify crate which use under the
            // hood, and a component of that rewrite is adding this
            // functionality; so until that lands we're using a fairly coarse
            // heuristic to determine if we need to re-watch subpaths.

            // if this was recursive, check if any child paths need to be
            // manually re-added
            if removed.recursive {
                // do this in two steps because we've borrowed mutably up top
                let to_add = state
                    .watchees
                    .iter()
                    .filter(|w| w.path.starts_with(&removed.path))
                    .map(|w| (w.path.to_owned(), mode_from_bool(w.recursive)))
                    .collect::<Vec<_>>();

                for (path, mode) in to_add {
                    if let Err(err) = self.inner.watch(&path, mode) {
                        tracing::error!("{:?}", err);
                    }
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
