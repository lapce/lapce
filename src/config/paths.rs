//! Path resolution for DeepSeek Carp.
//!
//! All config, data, logs, and cache live under `~/.deepseek-carp/`.
//!
//! ## Directory Layout
//!
//! ```text
//! ~/.deepseek-carp/
//! ├── config.toml         — User configuration
//! ├── credentials.toml    — API keys (permission 600)
//! ├── sessions/           — Conversation session data
//! ├── logs/               — Daily log files
//! ├── cache/              — Model cache, embeddings, etc.
//! ├── memory/             — Persistent conversation memory
//! ├── builds/             — Self-update builds
//! └── enterprise/         — Enterprise mode state
//! ```

use std::path::PathBuf;

const ROOT_DIR_NAME: &str = ".deepseek-carp";

/// Get the root config/data directory (`~/.deepseek-carp/`).
///
/// Creates the directory if it doesn't exist.
pub fn root_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    let root = home.join(ROOT_DIR_NAME);
    if !root.exists() {
        let _ = std::fs::create_dir_all(&root);
    }
    root
}

/// Get the config file path (`~/.deepseek-carp/config.toml`).
pub fn config_file() -> PathBuf {
    root_dir().join("config.toml")
}

/// Get the credentials file path (`~/.deepseek-carp/credentials.toml`).
pub fn credentials_file() -> PathBuf {
    root_dir().join("credentials.toml")
}

/// Get the sessions directory (`~/.deepseek-carp/sessions/`).
pub fn sessions_dir() -> PathBuf {
    let dir = root_dir().join("sessions");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Get the logs directory (`~/.deepseek-carp/logs/`).
pub fn logs_dir() -> PathBuf {
    let dir = root_dir().join("logs");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Get the cache directory (`~/.deepseek-carp/cache/`).
pub fn cache_dir() -> PathBuf {
    let dir = root_dir().join("cache");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Get the memory directory (`~/.deepseek-carp/memory/`).
pub fn memory_dir() -> PathBuf {
    let dir = root_dir().join("memory");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Get the builds directory (`~/.deepseek-carp/builds/`).
pub fn builds_dir() -> PathBuf {
    let dir = root_dir().join("builds");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Get the enterprise state directory (`~/.deepseek-carp/enterprise/`).
pub fn enterprise_dir() -> PathBuf {
    let dir = root_dir().join("enterprise");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Find a project-local config file (`<project>/.deepseek-carp/config.toml`).
pub fn project_config_file(project_root: &PathBuf) -> Option<PathBuf> {
    let path = project_root.join(".deepseek-carp").join("config.toml");
    path.exists().then_some(path)
}

/// Ensure the root directory and all standard subdirectories exist.
pub fn ensure_dirs() {
    root_dir();
    sessions_dir();
    logs_dir();
    cache_dir();
    memory_dir();
    builds_dir();
    enterprise_dir();
}
