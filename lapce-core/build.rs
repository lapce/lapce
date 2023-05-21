#![allow(clippy::redundant_clone)]
use std::{env, fs, path::Path};

use anyhow::Result;

#[cfg(not(debug_assertions))]
const RELEASE_TYPE: &str = "Nightly";

#[cfg(debug_assertions)]
const RELEASE_TYPE: &str = "Debug";

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RELEASE_TAG_NAME");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    let (tag, release, version, commit, full_version) = {
        let tag = env::var("RELEASE_TAG_NAME").unwrap_or(String::from("nightly"));

        if tag.starts_with('v') {
            (
                tag.clone(),
                "Stable",
                env::var("CARGO_PKG_VERSION")?,
                git2::Oid::zero(),
                tag.clone(),
            )
        } else {
            let (commit, full_version) = match get_commit() {
                Some(id) => (id, format!("{tag}-{} {id}", &id.to_string()[..7])),
                None => (git2::Oid::zero(), tag.to_string()),
            };

            (tag.clone(), RELEASE_TYPE, tag.clone(), commit, full_version)
        }
    };

    let meta_file = Path::new(&env::var("OUT_DIR")?).join("meta.rs");

    #[rustfmt::skip]
    let meta = format!(r#"
        pub const TAG: &str = "{tag}";
        pub const COMMIT: &str = "{commit}";
        pub const VERSION: &str = "{version}";
        pub const FULL_VERSION: &str = "{full_version}";
        pub const NAME: &str = "Lapce-{release}";
        pub const RELEASE: ReleaseType = ReleaseType::{release};
    "#);

    fs::write(meta_file, meta)?;

    Ok(())
}

fn get_commit() -> Option<git2::Oid> {
    if let Ok(repo) = git2::Repository::discover(".") {
        if let Ok(head) = repo.find_reference("HEAD") {
            let path = repo.path().to_path_buf();

            if let Ok(resolved) = head.resolve() {
                if let Some(name) = resolved.name() {
                    let path = path.join(name);
                    if path.exists() {
                        println!(
                            "cargo:rerun-if-changed={}",
                            path.canonicalize().unwrap().display()
                        );
                    }
                }
                return Some(resolved.peel_to_commit().ok()?.id());
            }
        }
    }
    None
}
