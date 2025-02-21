use std::{
    env,
    fs::{self},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow};
use lapce_core::directory::Directory;

use crate::{tracing::*, update::ReleaseInfo};

fn get_github_api(url: &str) -> Result<String> {
    let user_agent = format!("Lapce/{}", lapce_core::meta::VERSION);
    let resp = lapce_proxy::get_url(url, Some(user_agent.as_str()))?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }

    Ok(resp.text()?)
}

pub fn find_grammar_release() -> Result<ReleaseInfo> {
    let releases: Vec<ReleaseInfo> = serde_json::from_str(&get_github_api(
        "https://api.github.com/repos/lapce/tree-sitter-grammars/releases?per_page=100",
    ).context("Failed to retrieve releases for tree-sitter-grammars")?)?;

    use lapce_core::meta::{RELEASE, ReleaseType, VERSION};

    let releases = releases
        .into_iter()
        .filter_map(|f| {
            if matches!(RELEASE, ReleaseType::Debug | ReleaseType::Nightly) {
                return Some(f);
            }

            let tag_name = if f.tag_name.starts_with('v') {
                f.tag_name.trim_start_matches('v')
            } else {
                f.tag_name.as_str()
            };

            use std::cmp::Ordering;

            use semver::Version;

            let sv = Version::parse(tag_name).ok()?;
            let version = Version::parse(VERSION).ok()?;

            if matches!(sv.cmp_precedence(&version), Ordering::Equal) {
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

pub fn fetch_grammars(release: &ReleaseInfo) -> Result<bool> {
    let dir = Directory::grammars_directory()
        .ok_or_else(|| anyhow!("can't get grammars directory"))?;

    let file_name = format!("grammars-{}-{}", env::consts::OS, env::consts::ARCH);

    let updated = download_release(dir, release, &file_name)?;

    trace!(TraceLevel::INFO, "Successfully downloaded grammars");

    Ok(updated)
}

pub fn fetch_queries(release: &ReleaseInfo) -> Result<bool> {
    let dir = Directory::queries_directory()
        .ok_or_else(|| anyhow!("can't get queries directory"))?;

    let file_name = "queries";

    let updated = download_release(dir, release, file_name)?;

    trace!(TraceLevel::INFO, "Successfully downloaded queries");

    Ok(updated)
}

fn download_release(
    dir: PathBuf,
    release: &ReleaseInfo,
    file_name: &str,
) -> Result<bool> {
    if !dir.exists() {
        fs::create_dir(&dir)?;
    }

    let current_version =
        fs::read_to_string(dir.join("version")).unwrap_or_default();
    let release_version = if release.tag_name == "nightly" {
        format!("nightly-{}", &release.target_commitish[..7])
    } else {
        release.tag_name.clone()
    };

    if release_version == current_version {
        return Ok(false);
    }

    for asset in &release.assets {
        if asset.name.starts_with(file_name) {
            let mut resp = lapce_proxy::get_url(&asset.browser_download_url, None)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }

            let file = tempfile::tempfile()?;

            {
                use std::io::{Seek, Write};
                let file = &mut &file;
                resp.copy_to(file)?;
                file.flush()?;
                file.rewind()?;
            }

            if asset.name.ends_with(".zip") {
                let mut archive = zip::ZipArchive::new(file)?;
                archive.extract(&dir)?;
            } else if asset.name.ends_with(".tar.zst") {
                let mut archive =
                    tar::Archive::new(zstd::stream::read::Decoder::new(file)?);
                archive.unpack(&dir)?;
            }

            fs::write(dir.join("version"), &release_version)?;
        }
    }
    Ok(true)
}
