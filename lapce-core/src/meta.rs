use once_cell::sync::Lazy;

pub static NAME: Lazy<&str> = Lazy::new(name);
pub static CHANNEL: Lazy<&str> = Lazy::new(channel);
pub static VERSION: Lazy<&str> = Lazy::new(version);

fn name() -> &'static str {
    if cfg!(debug_assertions) {
        "Lapce-Debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        "Lapce-Nightly"
    } else {
        "Lapce-Stable"
    }
}

fn channel() -> &'static str {
    if cfg!(debug_assertions) {
        "Debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        "Nightly"
    } else {
        "Stable"
    }
}

fn version() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        option_env!("RELEASE_TAG_NAME").unwrap()
    } else {
        env!("CARGO_PKG_VERSION")
    }
}

pub static RELEASE: Lazy<&str> = Lazy::new(release_type);

fn release_type() -> &'static str {
    if cfg!(debug_assertions) {
        "Debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        "Nightly"
    } else {
        "Stable"
    }
}
