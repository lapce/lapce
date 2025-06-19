use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use lapce_core::{directory::Directory, meta};
use serde::Deserialize;

#[derive(Clone, Deserialize, Debug)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub target_commitish: String,
    pub assets: Vec<ReleaseAsset>,
    #[serde(skip)]
    pub version: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

pub fn get_latest_release() -> Result<ReleaseInfo> {
    let url = match meta::RELEASE {
        meta::ReleaseType::Debug => {
            return Err(anyhow!("no release for debug"));
        }
        meta::ReleaseType::Nightly => {
            "https://api.github.com/repos/lapce/lapce/releases/tags/nightly"
        }
        _ => "https://api.github.com/repos/lapce/lapce/releases/latest",
    };

    let resp = lapce_proxy::get_url(url, Some("Lapce"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }
    let mut release: ReleaseInfo = serde_json::from_str(&resp.text()?)?;

    release.version = match release.tag_name.as_str() {
        "nightly" => format!(
            "{}+Nightly.{}",
            env!("CARGO_PKG_VERSION"),
            &release.target_commitish[..7]
        ),
        _ => release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_owned(),
    };

    Ok(release)
}

pub fn download_release(release: &ReleaseInfo) -> Result<PathBuf> {
    let dir =
        Directory::updates_directory().ok_or_else(|| anyhow!("no directory"))?;
    let name = match std::env::consts::OS {
        "macos" => "Lapce-macos.dmg",
        "linux" => match std::env::consts::ARCH {
            "aarch64" => "lapce-linux-arm64.tar.gz",
            "x86_64" => "lapce-linux-amd64.tar.gz",
            _ => return Err(anyhow!("arch not supported")),
        },
        #[cfg(feature = "portable")]
        "windows" => "Lapce-windows-portable.zip",
        #[cfg(not(feature = "portable"))]
        "windows" => "Lapce-windows.msi",
        _ => return Err(anyhow!("os not supported")),
    };
    let file_path = dir.join(name);

    for asset in &release.assets {
        if asset.name == name {
            let mut resp = lapce_proxy::get_url(&asset.browser_download_url, None)?;
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

#[cfg(target_os = "macos")]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let info = dmg::Attach::new(src).with()?;
    let dest = process_path.parent().ok_or_else(|| anyhow!("no parent"))?;
    let dest = if dest.file_name().and_then(|s| s.to_str()) == Some("MacOS") {
        dest.parent().unwrap().parent().unwrap().parent().unwrap()
    } else {
        dest
    };
    std::fs::remove_dir_all(dest.join("Lapce.app"))?;
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
    Ok(dest.join("Lapce.app"))
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let tar_gz = std::fs::File::open(src)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    archive.unpack(parent)?;
    std::fs::remove_file(process_path)?;
    std::fs::copy(parent.join("Lapce").join("lapce"), process_path)?;
    Ok(process_path.to_path_buf())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let parent = src
        .parent()
        .ok_or_else(|| anyhow::anyhow!("src has no parent"))?;
    let dst_parent = process_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("process_path has no parent"))?;

    {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(src)?)?;
        archive.extract(parent)?;
    }

    // TODO(dbuga): instead of replacing the exe, run the msi installer for non-portable
    // TODO(dbuga): there's a very slight chance the user might end up with a backup file without a working .exe
    std::fs::rename(process_path, dst_parent.join("lapce.exe.bak"))?;
    std::fs::copy(parent.join("lapce.exe"), process_path)?;

    Ok(process_path.to_path_buf())
}

#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn extract(src: &Path, _process_path: &Path) -> Result<PathBuf> {
    // We downloaded an uncompressed msi installer, nothing to extract.
    // On the other hand, we need to run this msi so pass its path back out.
    Ok(src.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let _ = std::process::Command::new("open")
        .arg("-n")
        .arg(path)
        .arg("--args")
        .arg("-n")
        .exec();
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let _ = std::process::Command::new(path).arg("-n").exec();
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let process_id = std::process::id();
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("can't get path to str"))?;
    std::process::Command::new("cmd")
        .raw_arg(format!(
            r#"/C taskkill /PID {process_id} & start "" "{path}""#
        ))
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let process_id = std::process::id();
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("can't get path to str"))?;

    let lapce_exe = std::env::current_exe()
        .map_err(|err| anyhow!("can't get path to exe").context(err))?;
    let lapce_exe = lapce_exe
        .to_str()
        .ok_or_else(|| anyhow!("can't convert exe path to str"))?;

    std::process::Command::new("cmd")
        .raw_arg(format!(
            r#"/C taskkill /PID {process_id} & msiexec /i "{path}" /qb & start "" "{lapce_exe}""#,
        ))
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn cleanup() {
    // Clean up backup exe after an update
    if let Ok(process_path) = std::env::current_exe() {
        if let Some(dst_parent) = process_path.parent() {
            if let Err(err) = std::fs::remove_file(dst_parent.join("lapce.exe.bak"))
            {
                tracing::error!("{:?}", err);
            }
        }
    }
}

#[cfg(any(
    not(target_os = "windows"),
    all(target_os = "windows", not(feature = "portable"))
))]
pub fn cleanup() {
    // Nothing to do yet
}
