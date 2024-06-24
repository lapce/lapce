// Re-export `tracing` crate under own name to not collide and as convenient import
pub use tracing::{event as trace, instrument, Instrument, Level as TraceLevel};
