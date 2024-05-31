use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use git2::{build::CheckoutBuilder, Delta, DiffDelta, DiffOptions, ErrorCode, Oid};
use lapce_rpc::source_control::{DiffInfo, FileDiff};
use url::Url;

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
}

pub struct Repository {
    repo: git2::Repository,
}

impl Repository {
    pub fn init(path: &Path) -> Result<Self> {
        let repo = git2::Repository::init(path)?;
        Ok(Repository { repo })
    }

    pub fn discover(path: &Path) -> Result<Self> {
        let repo = git2::Repository::discover(path)?;
        Ok(Repository { repo })
    }

    pub fn discover_or_init(path: &Path) -> Result<Self> {
        Ok(Self::discover(path).unwrap_or(Self::init(path)?))
    }

    pub fn commit(&self, message: &str, diffs: Vec<FileDiff>) -> Result<()> {
        let mut index = self.repo.index()?;
        for diff in diffs {
            match diff {
                FileDiff::Modified(p) | FileDiff::Added(p) => {
                    index.add_path(p.strip_prefix(self.repo.path())?)?;
                }
                FileDiff::Renamed(a, d) => {
                    index.add_path(a.strip_prefix(self.repo.path())?)?;
                    index.remove_path(d.strip_prefix(self.repo.path())?)?;
                }
                FileDiff::Deleted(p) => {
                    index.remove_path(p.strip_prefix(self.repo.path())?)?;
                }
            }
        }
        index.write()?;
        let tree = index.write_tree()?;
        let tree = self.repo.find_tree(tree)?;

        match self.repo.signature() {
            Ok(signature) => {
                let parents = self.repo
                    .head()
                    .and_then(|head| Ok(vec![head.peel_to_commit()?]))
                    .unwrap_or(vec![]);
                let parents_refs = parents.iter().collect::<Vec<_>>();

                self.repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    message,
                    &tree,
                    &parents_refs,
                )?;
                Ok(())
            }
            Err(e) => match e.code() {
                ErrorCode::NotFound => Err(anyhow!(
                    "No user.name and/or user.email configured for this git repository."
                )),
                _ => Err(anyhow!(
                    "Error while creating commit's signature: {}",
                    e.message()
                )),
            },
        }
    }

    pub fn checkout(&self, reference: &str) -> Result<()> {
        let (object, reference) = self.repo.revparse_ext(reference)?;
        self.repo.checkout_tree(&object, None)?;
        self.repo.set_head(reference.unwrap().name().unwrap())?;
        Ok(())
    }

    pub fn discard_files_changes<'a>(
        &self,
        files: impl Iterator<Item = &'a Path>,
    ) -> Result<()> {
        let mut checkout_b = CheckoutBuilder::new();
        checkout_b.update_only(false).force();

        let mut had_path = false;
        for path in files {
            // Remove the workspace path so it is relative to the folder
            if let Ok(path) = path.strip_prefix(self.repo.path()) {
                had_path = true;
                checkout_b.path(path);
            }
        }

        if !had_path {
            // If there we no paths then we do nothing
            // because the default behavior of checkout builder is to select all files
            // if it is not given a path
            return Ok(());
        }

        self.repo.checkout_index(None, Some(&mut checkout_b))?;

        Ok(())
    }

    pub fn discard_workspace_changes(&self) -> Result<()> {
        let mut checkout_b = CheckoutBuilder::new();
        checkout_b.force();

        self.repo.checkout_index(None, Some(&mut checkout_b))?;

        Ok(())
    }

    pub fn delta_format(&self, delta: &DiffDelta) -> Option<(Delta, Oid, PathBuf)> {
        match delta.status() {
            Delta::Added | Delta::Untracked => Some((
                Delta::Added,
                delta.new_file().id(),
                delta.new_file().path().map(|p| self.repo.path().join(p))?,
            )),
            Delta::Deleted => Some((
                Delta::Deleted,
                delta.old_file().id(),
                delta.old_file().path().map(|p| self.repo.path().join(p))?,
            )),
            Delta::Modified => Some((
                Delta::Modified,
                delta.new_file().id(),
                delta.new_file().path().map(|p| self.repo.path().join(p))?,
            )),
            _ => None,
        }
    }

    pub fn diff_new(&self) -> Option<DiffInfo> {
        let name = match self.repo.head() {
            Ok(head) => head.shorthand()?.to_string(),
            _ => "(No branch)".to_owned(),
        };

        let mut branches = Vec::new();
        for branch in self.repo.branches(None).ok()? {
            branches.push(branch.ok()?.0.name().ok()??.to_string());
        }

        let mut tags = Vec::new();
        if let Ok(git_tags) = self.repo.tag_names(None) {
            for tag in git_tags.into_iter().flatten() {
                tags.push(tag.to_owned());
            }
        }

        let mut deltas = Vec::new();
        let mut diff_options = DiffOptions::new();
        let diff = self
            .repo
            .diff_index_to_workdir(
                None,
                Some(
                    diff_options
                        .include_untracked(true)
                        .recurse_untracked_dirs(true),
                ),
            )
            .ok()?;
        for delta in diff.deltas() {
            if let Some(delta) = self.delta_format(&delta) {
                deltas.push(delta);
            }
        }

        let oid = match self.repo.revparse_single("HEAD^{tree}") {
            Ok(obj) => obj.id(),
            _ => Oid::zero(),
        };

        let cached_diff = self
            .repo
            .diff_tree_to_index(self.repo.find_tree(oid).ok().as_ref(), None, None)
            .ok();

        if let Some(cached_diff) = cached_diff {
            for delta in cached_diff.deltas() {
                if let Some(delta) = self.delta_format(&delta) {
                    deltas.push(delta);
                }
            }
        }
        let mut renames = Vec::new();
        let mut renamed_deltas = HashSet::new();

        for (added_index, delta) in deltas.iter().enumerate() {
            if delta.0 == git2::Delta::Added {
                for (deleted_index, d) in deltas.iter().enumerate() {
                    if d.0 == git2::Delta::Deleted && d.1 == delta.1 {
                        renames.push((added_index, deleted_index));
                        renamed_deltas.insert(added_index);
                        renamed_deltas.insert(deleted_index);
                        break;
                    }
                }
            }
        }

        let mut file_diffs = Vec::new();
        for (added_index, deleted_index) in renames.iter() {
            file_diffs.push(FileDiff::Renamed(
                deltas[*added_index].2.clone(),
                deltas[*deleted_index].2.clone(),
            ));
        }
        for (i, delta) in deltas.iter().enumerate() {
            if renamed_deltas.contains(&i) {
                continue;
            }
            let diff = match delta.0 {
                git2::Delta::Added => FileDiff::Added(delta.2.clone()),
                git2::Delta::Deleted => FileDiff::Deleted(delta.2.clone()),
                git2::Delta::Modified => FileDiff::Modified(delta.2.clone()),
                _ => continue,
            };
            file_diffs.push(diff);
        }
        file_diffs.sort_by_key(|d| match d {
            FileDiff::Modified(p)
            | FileDiff::Added(p)
            | FileDiff::Renamed(p, _)
            | FileDiff::Deleted(p) => p.clone(),
        });
        Some(DiffInfo {
            head: name,
            branches,
            tags,
            diffs: file_diffs,
        })
    }

    pub fn file_get_head(&self, path: &Path) -> Result<(String, String)> {
        let head = self.repo.head()?;
        let tree = head.peel_to_tree()?;
        let tree_entry = tree.get_path(path.strip_prefix(self.repo.path())?)?;
        let blob = self.repo.find_blob(tree_entry.id())?;
        let id = blob.id().to_string();
        let content = std::str::from_utf8(blob.content())
            .with_context(|| "content bytes to string")?
            .to_string();
        Ok((id, content))
    }

    pub fn get_remote_file_url(&self, file: &Path) -> Result<String> {
        let head = self.repo.head()?;
        let target_remote = self.repo.find_remote(
            self.repo
                .branch_upstream_remote(head.name().unwrap())?
                .as_str()
                .unwrap(),
        )?;

        // Grab URL part of remote
        let remote = target_remote
            .url()
            .ok_or(anyhow!("Failed to convert remote to str"))?;

        let remote_url = match Url::parse(remote) {
            Ok(url) => url,
            Err(_) => {
                // Parse URL as ssh
                Url::parse(&format!("ssh://{}", remote.replacen(':', "/", 1)))?
            }
        };

        // Get host part
        let host = remote_url
            .host_str()
            .ok_or(anyhow!("Couldn't find remote host"))?;
        // Get namespace (e.g. organisation/project in case of GitHub, org/team/team/team/../project on GitLab)
        let namespace =
            if let Some(stripped) = remote_url.path().strip_suffix(".git") {
                stripped
            } else {
                remote_url.path()
            };

        let commit = head.peel_to_commit()?.id();

        let file_path = file
            .strip_prefix(self.repo.path())?
            .to_str()
            .ok_or(anyhow!("Couldn't convert file path to str"))?;

        let url = format!("https://{host}{namespace}/blob/{commit}/{file_path}",);

        Ok(url)
    }
}
