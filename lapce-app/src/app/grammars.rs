use std::{
    env,
    fs::{self, File},
    path::PathBuf,
};

use anyhow::{anyhow, Result};
use lapce_core::directory::Directory;

use crate::{tracing::*, update::ReleaseInfo};

fn get_github_api(url: &str) -> Result<String> {
    let resp = reqwest::blocking::ClientBuilder::new()
        .user_agent(format!("Lapce/{}", lapce_core::meta::VERSION))
        .build()?
        .get(url)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }

    Ok(resp.text()?)
}

pub fn find_release() -> Result<ReleaseInfo> {
    let releases: Vec<ReleaseInfo> = serde_json::from_str(&get_github_api(
        "https://api.github.com/repos/lapce/tree-sitter-grammars/releases?per_page=100",
    )?)?;

    use lapce_core::meta::{ReleaseType, RELEASE, VERSION};

    let releases = releases
        .into_iter()
        .filter_map(|f| {
            let tag_name = if f.tag_name.starts_with('v') {
                f.tag_name.trim_start_matches('v')
            } else {
                f.tag_name.as_str()
            };

            use semver::Version;
            use std::cmp::Ordering;

            let sv = Version::parse(tag_name).ok()?;
            let version = Version::parse(VERSION).ok()?;

            if matches!(sv.cmp_precedence(&version), Ordering::Equal)
                || matches!(RELEASE, ReleaseType::Debug | ReleaseType::Nightly)
            {
                Some(f)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let Some(release) = releases.first() else {
        return Err(anyhow!("Couldn't find any release"));
    };

    Ok(release.to_owned())
}

pub fn describe_release(id: &str) -> Result<ReleaseInfo> {
    let url = format!(
        "https://api.github.com/repos/lapce/tree-sitter-grammars/releases/{id}"
    );

    let resp = get_github_api(&url)?;
    let release: ReleaseInfo = serde_json::from_str(&resp)?;

    Ok(release)
}

pub fn fetch_grammars(release: &ReleaseInfo) -> Result<()> {
    let dir = Directory::grammars_directory()
        .ok_or_else(|| anyhow!("can't get grammars directory"))?;

    let file_name =
        format!("grammars-{}-{}.zip", env::consts::OS, env::consts::ARCH);

    download_release(dir, release, &file_name)?;

    Ok(())
}

pub fn fetch_queries(release: &ReleaseInfo) -> Result<()> {
    let dir = Directory::queries_directory()
        .ok_or_else(|| anyhow!("can't get queries directory"))?;

    let file_name = "queries.zip";

    download_release(dir, release, file_name)?;

    Ok(())
}

fn download_release(
    dir: PathBuf,
    release: &ReleaseInfo,
    file_name: &str,
) -> Result<()> {
    if !dir.exists() {
        fs::create_dir(&dir)?;
    }

    let current_version =
        fs::read_to_string(dir.join("version")).unwrap_or_default();

    if release.tag_name == current_version {
        return Ok(());
    }

    for asset in &release.assets {
        if asset.name == file_name {
            let mut resp = reqwest::blocking::get(&asset.browser_download_url)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }
            {
                let mut out = File::create(dir.join(file_name))?;
                resp.copy_to(&mut out)?;
            }

            let mut archive =
                zip::ZipArchive::new(File::open(dir.join(file_name))?)?;
            archive.extract(&dir)?;
            if let Err(err) = fs::remove_file(dir.join(file_name)) {
                trace!(TraceLevel::ERROR, "Failed to remove file: {err}");
            };
            fs::write(dir.join("version"), release.tag_name.clone())?;
        }
    }
    Ok(())
}
