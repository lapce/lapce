#[derive(strum_macros::AsRefStr)]
pub enum ReleaseType {
    Debug,
    Stable,
    Nightly,
}

include!(concat!(env!("OUT_DIR"), "/meta.rs"));
