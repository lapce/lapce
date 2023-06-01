#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(u64);

impl Id {
    /// Allocate a new, unique `Id`.
    pub fn next() -> Id {
        use floem::glazier::Counter;
        static ID_COUNTER: Counter = Counter::new();
        Id(ID_COUNTER.next())
    }

    pub fn to_raw(self) -> u64 {
        self.0
    }
}

pub type SplitId = Id;
pub type WindowTabId = Id;
pub type EditorTabId = Id;
pub type SettingsId = Id;
pub type EditorId = Id;
pub type TerminalTabId = Id;
