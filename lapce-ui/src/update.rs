use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use lapce_data::proxy::VERSION;
use serde::Deserialize;
use tempdir::TempDir;

#[derive(Deserialize)]
pub struct ReleaseInfo {
    tag_name: String,
    target_commitish: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
pub struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl ReleaseInfo {
    pub fn version(&self) -> String {
        match self.tag_name.as_str() {
            "nightly" => format!("nightly-{}", &self.target_commitish[..7]),
            _ => self.tag_name[1..].to_string(),
        }
    }
}

pub fn latest_release() -> Result<ReleaseInfo> {
    let version = *VERSION;
    let url = match version {
        "debug" => {
            return Err(anyhow!("no release for debug"));
        }
        version if version.starts_with("nightly") => {
            "https://api.github.com/repos/lapce/lapce/releases/tags/nightly"
        }
        _ => "https://api.github.com/repos/lapce/lapce/releases/latest",
    };

    let resp = reqwest::blocking::ClientBuilder::new()
        .user_agent("Lapce")
        .build()?
        .get(url)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }
    let release: ReleaseInfo = serde_json::from_str(&resp.text()?)?;

    Ok(release)
}

pub fn download_release(release: &ReleaseInfo) -> Result<PathBuf> {
    let dir = TempDir::new(&format!("lapce-update-{}", release.tag_name))?;
    let name = match std::env::consts::OS {
        "macos" => "Lapce-macos.dmg",
        "linux" => "Lapce-linux.tar.gz",
        "windows" => "Lapce-windows-portable.zip",
        _ => return Err(anyhow!("os not supported")),
    };
    let file_path = dir.path().join(name);

    for asset in &release.assets {
        if asset.name == name {
            let mut resp = reqwest::blocking::get(&asset.browser_download_url)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }
            let mut out = std::fs::File::create(&file_path)?;
            resp.copy_to(&mut out)?;
            return Ok(file_path);
        }
    }

    Err(anyhow!("can't download release"))
}

pub fn update(src: &Path, dest: &Path) -> Result<()> {
    let info = dmg::Attach::new(src).with()?;
    let dest = dest.parent().unwrap().parent().unwrap().parent().unwrap();
    let _ = std::fs::remove_dir_all(dest.join("Lapce.app"));
    fs_extra::copy_items(
        &[info.mount_point.join("Lapce.app")],
        dest,
        &fs_extra::dir::CopyOptions {
            overwrite: true,
            skip_exist: false,
            buffer_size: 64000,
            copy_inside: true,
            content_only: false,
            depth: 0,
        },
    )?;
    std::process::Command::new("open")
        .arg(dest.join("Lapce.app"))
        .output()?;
    Ok(())
}
