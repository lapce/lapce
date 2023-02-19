use std::{fs, path::PathBuf};

use anyhow::{anyhow, Result};
use directories::ProjectDirs;

use crate::meta::NAME;
pub struct Directory {}

impl Directory {
    fn project_dirs() -> Result<ProjectDirs> {
        ProjectDirs::from("dev", "lapce", &NAME)
            .ok_or_else(|| anyhow!("Failed to obtained Lapce data directories"))
    }

    // Get path of local data directory
    // Local data directory differs from data directory
    // on some platforms and is not transferred across
    // machines
    pub fn data_local_directory() -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.data_local_dir();
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        Ok(dir.to_path_buf())
    }

    /// Get the path to logs directory
    /// Each log file is for individual application startup
    pub fn logs_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("logs");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }

    /// Get the path to cache directory
    pub fn cache_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("cache");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }

    /// Directory to store proxy executables used on local
    /// host as well, as ones uploaded to remote host when
    /// connecting
    pub fn proxy_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("proxy");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }
    /// Get the path to the themes folder
    /// Themes are stored within as individual toml files
    pub fn themes_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("themes");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }
    // Get the path to plugins directory
    // Each plugin has own directory that contains
    // metadata file and plugin wasm
    pub fn plugins_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("plugins");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }

    // Config directory contain only configuration files
    pub fn config_directory() -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.config_dir();
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
        Ok(dir.to_path_buf())
    }

    pub fn local_socket() -> Result<PathBuf> {
        Ok(Self::data_local_directory()?.join("local.sock"))
    }

    pub fn updates_directory() -> Result<PathBuf> {
        let dir = Self::data_local_directory()?.join("updates");
        if !dir.exists() {
            fs::create_dir(&dir)?;
        }
        Ok(dir)
    }
}
