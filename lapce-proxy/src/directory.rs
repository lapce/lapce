use std::path::PathBuf;

use directories::ProjectDirs;

use crate::APPLICATION_NAME;

pub struct Directory {}

impl Directory {
    // Get path of local data directory
    // Local data directory differs from data directory
    // on some platforms and is not transferred across
    // machines
    pub fn data_local_directory() -> Option<PathBuf> {
        match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
            Some(dir) => {
                let dir = dir.data_local_dir();
                if !dir.exists() {
                    let _ = std::fs::create_dir_all(dir);
                }

                Some(dir.to_path_buf())
            }
            None => None,
        }
    }

    /// Get the path to logs directory
    /// Each log file is for individual application startup
    pub fn logs_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("logs");
            if !dir.exists() {
                let _ = std::fs::create_dir(&dir);
            }

            Some(dir)
        } else {
            None
        }
    }

    /// Directory to store proxy executables used on local
    /// host as well, as ones uploaded to remote host when
    /// connecting
    pub fn proxy_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("proxy");
            if !dir.exists() {
                let _ = std::fs::create_dir(&dir);
            }

            Some(dir)
        } else {
            None
        }
    }

    /// Get the path to the themes folder
    /// Themes are stored within as individual toml files
    pub fn themes_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("themes");
            if !dir.exists() {
                let _ = std::fs::create_dir(&dir);
            }

            Some(dir)
        } else {
            None
        }
    }

    // Get the path to plugins directory
    // Each plugin has own directory that contains
    // metadata file and plugin wasm
    pub fn plugins_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("plugins");
            if !dir.exists() {
                let _ = std::fs::create_dir(&dir);
            }

            Some(dir)
        } else {
            None
        }
    }

    // Config directory contain only configuration files
    pub fn config_directory() -> Option<PathBuf> {
        match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
            Some(dir) => {
                let dir = dir.config_dir();
                if !dir.exists() {
                    let _ = std::fs::create_dir_all(dir);
                }

                Some(dir.to_path_buf())
            }
            None => None,
        }
    }
}
