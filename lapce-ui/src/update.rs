use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use lapce_data::{data::ReleaseInfo, proxy::VERSION};
use serde::Deserialize;
use tempdir::TempDir;

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
