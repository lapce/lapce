use std::path::Path;

use anyhow::Result;

#[cfg(target_os = "macos")]
pub fn update(_process_id: &str, src: &Path, dest: &Path) -> Result<()> {
    let info = dmg::Attach::new(src).with()?;
    if dest.file_name().and_then(|s| s.to_str()) == Some("MacOS") {
        dest.parent().unwrap().parent().unwrap().parent().unwrap()
    } else {
        dest
    };
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

#[cfg(target_os = "linux")]
pub fn update(_process_id: &str, _src: &Path, _dest: &Path) -> Result<()> {
    let tar_gz = std::fs::File::open(src)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(src.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn update(_process_id: &str, _src: &Path, _dest: &Path) -> Result<()> {
    Ok(())
}
