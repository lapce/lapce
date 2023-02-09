use std::{
    borrow::Borrow,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CanonPath(PathBuf);

impl CanonPath {
    pub fn from_pathbuf(path: PathBuf) -> CanonPath {
        CanonPath(
            path.canonicalize()
                .map_err(|e| format!("Cannot canonicalize '{path:?} because '{e}'"))
                .unwrap(),
        )
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub fn as_pathbuf(&self) -> &PathBuf {
        &self.0
    }

    pub fn to_pathbuf(self) -> PathBuf {
        self.0
    }

    pub fn exists(&self) -> bool {
        self.0.exists()
    }
}

impl AsRef<Path> for CanonPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<PathBuf> for CanonPath {
    fn as_ref(&self) -> &PathBuf {
        &self.0
    }
}

impl Borrow<PathBuf> for CanonPath {
    fn borrow(&self) -> &PathBuf {
        &self.0
    }
}

impl Borrow<Path> for CanonPath {
    fn borrow(&self) -> &Path {
        self.0.as_path()
    }
}
