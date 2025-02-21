use std::path::PathBuf;

use tracing::{Level, event};
use url::Url;

// Rust-analyzer returns paths in the form of "file:///<drive>:/...", which gets parsed into URL
// as "/<drive>://" which is then interpreted by PathBuf::new() as a UNIX-like path from root.
// This function strips the additional / from the beginning, if the first segment is a drive letter.
#[cfg(windows)]
pub fn path_from_url(url: &Url) -> PathBuf {
    use percent_encoding::percent_decode_str;

    event!(Level::DEBUG, "Converting `{:?}` to path", url);

    if let Ok(path) = url.to_file_path() {
        return path;
    }

    let path = url.path();

    let path = if path.contains('%') {
        percent_decode_str(path)
            .decode_utf8()
            .unwrap_or(std::borrow::Cow::from(path))
    } else {
        std::borrow::Cow::from(path)
    };

    if let Some(path) = path.strip_prefix('/') {
        event!(Level::DEBUG, "Found `/` prefix");
        if let Some((maybe_drive_letter, path_second_part)) =
            path.split_once(['/', '\\'])
        {
            event!(Level::DEBUG, maybe_drive_letter);
            event!(Level::DEBUG, path_second_part);

            let b = maybe_drive_letter.as_bytes();

            if !b.is_empty() && !b[0].is_ascii_alphabetic() {
                event!(Level::ERROR, "First byte is not ascii alphabetic: {b:?}");
            }

            match maybe_drive_letter.len() {
                2 => match maybe_drive_letter.chars().nth(1) {
                    Some(':') => {
                        event!(Level::DEBUG, "Returning path `{:?}`", path);
                        return PathBuf::from(path);
                    }
                    v => {
                        event!(
                            Level::ERROR,
                            "Unhandled 'maybe_drive_letter' chars: {v:?}"
                        );
                    }
                },
                4 => {
                    if maybe_drive_letter.contains("%3A") {
                        let path = path.replace("%3A", ":");
                        event!(Level::DEBUG, "Returning path `{:?}`", path);
                        return PathBuf::from(path);
                    } else {
                        event!(
                            Level::ERROR,
                            "Unhandled 'maybe_drive_letter' pattern: {maybe_drive_letter:?}"
                        );
                    }
                }
                v => {
                    event!(
                        Level::ERROR,
                        "Unhandled 'maybe_drive_letter' length: {v}"
                    );
                }
            }
        }
    }

    event!(Level::DEBUG, "Returning unmodified path `{:?}`", path);
    PathBuf::from(path.into_owned())
}

#[cfg(not(windows))]
pub fn path_from_url(url: &Url) -> PathBuf {
    event!(Level::DEBUG, "Converting `{:?}` to path", url);
    url.to_file_path().unwrap_or_else(|_| {
        let path = url.path();
        if let Ok(path) = percent_encoding::percent_decode_str(path).decode_utf8() {
            return PathBuf::from(path.into_owned());
        }
        PathBuf::from(path)
    })
}
