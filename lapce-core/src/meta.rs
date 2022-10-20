use once_cell::sync::Lazy;

pub static NAME: Lazy<&str> = Lazy::new(application_name);

fn application_name() -> &'static str {
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

pub static VERSION: Lazy<&str> = Lazy::new(version);

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
