use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

use crate::meta::NAME;

pub struct Directory {}

impl Directory {
    pub fn home_dir() -> Option<PathBuf> {
        BaseDirs::new().map(|d| PathBuf::from(d.home_dir()))
    }

    #[cfg(not(feature = "portable"))]
    fn project_dirs() -> Option<ProjectDirs> {
        ProjectDirs::from("dev", "lapce", NAME)
    }

    /// Return path adjacent to lapce executable when built as portable
    #[cfg(feature = "portable")]
    fn project_dirs() -> Option<ProjectDirs> {
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                return ProjectDirs::from_path(parent.join("lapce-data"));
            }
            unreachable!("Couldn't obtain current process parent path");
        }
        unreachable!("Couldn't obtain current process path");
    }

    // Get path of local data directory
    // Local data directory differs from data directory
    // on some platforms and is not transferred across
    // machines
    pub fn data_local_directory() -> Option<PathBuf> {
        match Self::project_dirs() {
            Some(dir) => {
                let dir = dir.data_local_dir();
                if !dir.exists() {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        tracing::error!("{:?}", err);
                    }
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
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
            }
            Some(dir)
        } else {
            None
        }
    }

    /// Get the path to cache directory
    pub fn cache_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("cache");
            if !dir.exists() {
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
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
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
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
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
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
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
            }
            Some(dir)
        } else {
            None
        }
    }

    // Config directory contain only configuration files
    pub fn config_directory() -> Option<PathBuf> {
        match Self::project_dirs() {
            Some(dir) => {
                let dir = dir.config_dir();
                if !dir.exists() {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        tracing::error!("{:?}", err);
                    }
                }
                Some(dir.to_path_buf())
            }
            None => None,
        }
    }

    pub fn local_socket() -> Option<PathBuf> {
        Self::data_local_directory().map(|dir| dir.join("local.sock"))
    }

    pub fn updates_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("updates");
            if !dir.exists() {
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
            }
            Some(dir)
        } else {
            None
        }
    }

    pub fn queries_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::config_directory() {
            let dir = dir.join("queries");
            if !dir.exists() {
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
            }

            Some(dir)
        } else {
            None
        }
    }

    pub fn grammars_directory() -> Option<PathBuf> {
        if let Some(dir) = Self::data_local_directory() {
            let dir = dir.join("grammars");
            if !dir.exists() {
                if let Err(err) = std::fs::create_dir(&dir) {
                    tracing::error!("{:?}", err);
                }
            }

            Some(dir)
        } else {
            None
        }
    }
}
