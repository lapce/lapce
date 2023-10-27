use std::sync::atomic::AtomicU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(u64);

impl Id {
    /// Allocate a new, unique `Id`.
    pub fn next() -> Id {
        static TIMER_COUNTER: AtomicU64 = AtomicU64::new(0);
        Id(TIMER_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    pub fn to_raw(self) -> u64 {
        self.0
    }
}

pub type SplitId = Id;
pub type WindowTabId = Id;
pub type EditorTabId = Id;
pub type SettingsId = Id;
pub type KeymapId = Id;
pub type ThemeColorSettingsId = Id;
pub type VoltViewId = Id;
pub type EditorId = Id;
pub type DiffEditorId = Id;
pub type TerminalTabId = Id;
