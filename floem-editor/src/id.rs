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

pub type EditorId = Id;
