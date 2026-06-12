//! Audio processing — voice input, speech-to-text.
//!
//! P3 feature: Voice Input for hands-free coding assistance.
//! Supports cloud Whisper API and local model fallback.

pub mod stt;

pub use stt::{SttEngine, SttConfig, Transcript, AudioSegment, SttBackend};
