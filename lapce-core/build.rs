use std::{env, fs, path::Path};

use anyhow::Result;

#[derive(Debug)]
struct ReleaseInfo {
    version: String,
    branch: String,
}

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DISTRIBUTION");
    println!("cargo:rerun-if-env-changed=RELEASE_TAG_NAME");

    let release_info = get_info()?;

    // Print info to terminal during compilation
    println!("cargo::warning=Compiling meta: {release_info:?}");

    let meta_file = Path::new(&env::var("OUT_DIR")?).join("meta.rs");

    let ReleaseInfo { version, branch } = release_info;

    #[rustfmt::skip]
    let meta = format!(r#"
        pub const NAME: &str = "Lapce-{branch}";
        pub const VERSION: &str = "{version}";
        pub const RELEASE: ReleaseType = ReleaseType::{branch};
    "#);

    fs::write(meta_file, meta)?;

    Ok(())
}

fn get_info() -> Result<ReleaseInfo> {
    // CARGO_PKG_* are always available, even in build scripts
    let cargo_tag = env!("CARGO_PKG_VERSION");

    // For any downstream that complains about us doing magic
    if env::var("CARGO_FEATURE_DISTRIBUTION").is_ok() {
        return Ok(ReleaseInfo {
            version: cargo_tag.to_string(),
            branch: String::from("Stable"),
        });
    }

    let release_info = {
        let release_tag = env::var("RELEASE_TAG_NAME").unwrap_or_default();

        if release_tag.starts_with('v') {
            ReleaseInfo {
                version: cargo_tag.to_string(),
                branch: "Stable".to_string(),
            }
        } else {
            #[cfg(not(debug_assertions))]
            let release = "Nightly";
            #[cfg(debug_assertions)]
            let release = "Debug";

            let tag = format!(
                "{cargo_tag}+{release}.{}",
                get_head().unwrap_or("unknown".to_string())
            );
            ReleaseInfo {
                version: tag,
                branch: release.to_string(),
            }
        }
    };

    Ok(release_info)
}

#[cfg(not(target_os = "linux"))]
fn get_head() -> Option<String> {
    let repo = match git2::Repository::discover(format!(
        "{}/..",
        env::var("CARGO_MANIFEST_DIR").ok()?
    )) {
        Ok(v) => v,
        Err(err) => {
            println!("cargo::warning=Failed to obtain git repo: {err}");
            return None;
        }
    };
    let reference = match repo.head() {
        Ok(v) => v,
        Err(err) => {
            println!("cargo::warning=Failed to obtain head: {err}");
            return None;
        }
    };
    let commit = reference.target();
    println!("cargo::warning=Commit found: {commit:?}");
    commit.map(|s| s.to_string().split_at(7).0.to_owned())
}

#[cfg(target_os = "linux")]
fn get_head() -> Option<String> {
    let cmd = std::process::Command::new("git")
        .args(["show", "--pretty=format:%h", "--no-patch"])
        .output()
        .ok()?;

    let commit = String::from_utf8_lossy(&cmd.stdout);
    let commit = commit.trim();

    Some(commit.to_string())
}
