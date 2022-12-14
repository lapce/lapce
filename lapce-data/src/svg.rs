use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use druid::piet::Svg;
use include_dir::{include_dir, Dir};

use crate::config::LOGO;

const CODICONS_ICONS_DIR: Dir = include_dir!("../icons/codicons");
const LAPCE_ICONS_DIR: Dir = include_dir!("../icons/lapce");

pub struct SvgStore {
    svgs: HashMap<String, Svg>,
    svgs_on_disk: HashMap<PathBuf, Option<Svg>>,
}

impl Default for SvgStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SvgStore {
    fn new() -> Self {
        let mut svgs = HashMap::new();
        svgs.insert("lapce_logo".to_string(), Svg::from_str(LOGO).unwrap());

        Self {
            svgs,
            svgs_on_disk: HashMap::new(),
        }
    }

    pub fn logo_svg(&self) -> Svg {
        self.svgs.get("lapce_logo").unwrap().clone()
    }

    pub fn get_default_svg(&mut self, name: &str) -> Svg {
        if !self.svgs.contains_key(name) {
            let file = if name == "lapce_remote.svg" {
                LAPCE_ICONS_DIR.get_file(name).unwrap()
            } else {
                CODICONS_ICONS_DIR
                    .get_file(name)
                    .unwrap_or_else(|| panic!("Failed to unwrap {name}"))
            };
            let content = file.contents_utf8().unwrap();
            let svg = Svg::from_str(content).unwrap();
            self.svgs.insert(name.to_string(), svg);
        }
        self.svgs.get(name).unwrap().clone()
    }

    pub fn get_svg_on_disk(&mut self, path: &Path) -> Option<Svg> {
        if !self.svgs_on_disk.contains_key(path) {
            let svg = fs::read_to_string(path)
                .ok()
                .and_then(|content| Svg::from_str(&content).ok());
            self.svgs_on_disk.insert(path.to_path_buf(), svg);
        }

        self.svgs_on_disk.get(path).unwrap().clone()
    }
}
