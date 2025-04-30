use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

use crate::config::LOGO;

const CODICONS_ICONS_DIR: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../icons/codicons");
const LAPCE_ICONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../icons/lapce");

#[derive(Debug, Clone)]
pub struct SvgStore {
    svgs: HashMap<String, String>,
    svgs_on_disk: HashMap<PathBuf, Option<String>>,
}

impl Default for SvgStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SvgStore {
    fn new() -> Self {
        let mut svgs = HashMap::new();
        svgs.insert("lapce_logo".to_string(), LOGO.to_string());

        Self {
            svgs,
            svgs_on_disk: HashMap::new(),
        }
    }

    pub fn logo_svg(&self) -> String {
        self.svgs.get("lapce_logo").unwrap().clone()
    }

    pub fn get_default_svg(&mut self, name: &str) -> String {
        if !self.svgs.contains_key(name) {
            let file = if name == "lapce_remote.svg" || name == "lapce_logo.svg" {
                LAPCE_ICONS_DIR.get_file(name).unwrap()
            } else {
                CODICONS_ICONS_DIR
                    .get_file(name)
                    .unwrap_or_else(|| panic!("Failed to unwrap {name}"))
            };
            let content = file.contents_utf8().unwrap();
            self.svgs.insert(name.to_string(), content.to_string());
        }
        self.svgs.get(name).unwrap().clone()
    }

    pub fn get_svg_on_disk(&mut self, path: &Path) -> Option<String> {
        if !self.svgs_on_disk.contains_key(path) {
            let svg = fs::read_to_string(path).ok();
            self.svgs_on_disk.insert(path.to_path_buf(), svg);
        }

        self.svgs_on_disk.get(path).unwrap().clone()
    }
}
