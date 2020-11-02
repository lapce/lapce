use std::collections::HashMap;
use std::path::PathBuf;

pub type PluginName = String;

pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub exec_path: PathBuf,
}

pub struct PluginCatalog {
    items: HashMap<PluginName, PluginDescription>,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            items: HashMap::new(),
        }
    }

    pub fn reload(&mut self) {
        println!("plugin reload from paths");
        self.items.clear();
        self.load();
    }

    pub fn load(&mut self) {}
}
