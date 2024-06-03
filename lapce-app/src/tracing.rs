// Re-export `tracing` crate under own name to not collide and as convenient import
pub use tracing::event as trace;
pub use tracing::instrument;
pub use tracing::Instrument;
pub use tracing::Level as TraceLevel;
