use std::path::Path;

use anyhow::Result;

pub fn update(process_id: &str, src: &Path, dest: &Path) -> Result<()> {
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
