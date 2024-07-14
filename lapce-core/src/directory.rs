use std::path::PathBuf;

use anyhow::{anyhow, Result};
use directories::{BaseDirs, ProjectDirs};
#[allow(unused_imports)]
use meta::NAME;

pub struct Directory {}

impl Directory {
    pub fn home_dir() -> Option<PathBuf> {
        BaseDirs::new().map(|d| PathBuf::from(d.home_dir()))
    }

    #[cfg(not(feature = "portable"))]
    fn project_dirs() -> Result<ProjectDirs> {
        match ProjectDirs::from("dev", "lapce", NAME) {
            Some(v) => Ok(v),
            None => Err(anyhow!("Failed to obtain project directories")),
        }
    }

    /// Return path adjacent to lapce executable when built as portable
    #[cfg(feature = "portable")]
    fn project_dirs() -> Result<ProjectDirs> {
        if let Some(parent) = std::env::current_exe()?.parent() {
            return ProjectDirs::from_path(parent.join("lapce-data"))
                .ok_or(anyhow!("Failed to obtain data directory path"));
        }
        Err(anyhow!("Failed to obtain current process path"))
    }

    // Get path of local data directory
    // Local data directory differs from data directory
    // on some platforms and is not transferred across
    // machines
    pub fn data_local_directory(join_dir: Option<&'static str>) -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.data_local_dir();
        let dir = join_dir.map(|d| dir.join(d)).unwrap_or(dir.to_path_buf());
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        };
        Ok(dir)
    }

    /// Get path of data directory
    /// Data directory differs from data local directory
    /// on some platforms and can be transferred across
    /// machines
    pub fn data_directory(join_dir: Option<&'static str>) -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.data_dir();
        let dir = join_dir.map(|d| dir.join(d)).unwrap_or(dir.to_path_buf());
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        };
        Ok(dir)
    }

    /// Get the path to cache directory
    pub fn cache_directory() -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.cache_dir();
        if !dir.exists() {
            std::fs::create_dir(dir)?;
        }
        Ok(dir.to_path_buf())
    }

    /// Get the path to logs directory
    /// Each log file is for individual application startup
    pub fn logs_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("logs"))
    }

    /// Directory to store proxy executables used on local
    /// host as well, as ones uploaded to remote host when
    /// connecting
    pub fn proxy_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("proxy"))
    }

    /// Get the path to the themes folder
    /// Themes are stored within as individual toml files
    pub fn themes_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("themes"))
    }

    // Get the path to plugins directory
    // Each plugin has own directory that contains
    // metadata file and plugin wasm
    pub fn plugins_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("plugins"))
    }

    // Config directory contain only configuration files
    pub fn config_directory(join_dir: Option<&'static str>) -> Result<PathBuf> {
        let dir = Self::project_dirs()?;
        let dir = dir.config_dir();
        let dir = join_dir.map(|d| dir.join(d)).unwrap_or(dir.to_path_buf());
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        };
        Ok(dir)
    }

    pub fn local_socket() -> Result<PathBuf> {
        Ok(Self::data_local_directory(None)?.join("local.sock"))
    }

    pub fn updates_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("updates"))
    }

    pub fn queries_directory() -> Result<PathBuf> {
        Self::config_directory(Some("queries"))
    }

    pub fn grammars_directory() -> Result<PathBuf> {
        Self::data_local_directory(Some("grammars"))
    }

    pub fn docs_dir() -> Result<PathBuf> {
        Self::data_local_directory(Some("files"))
    }
}
