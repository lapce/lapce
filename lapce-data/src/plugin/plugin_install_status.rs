#[derive(Clone, Debug)]
pub enum PluginInstallType {
    INSTALLATION,
    UNINSTALLATION,
}

#[derive(Clone, Debug)]
pub struct PluginInstallStatus {
    progress: f32,
    plugin_name: String,
    install_type: PluginInstallType
}

impl PluginInstallStatus {
    pub fn new(install_type: PluginInstallType, plugin_name: &str) -> Self {
        Self {
            progress: 0.0,
            plugin_name: plugin_name.to_string(),
            install_type,
        }
    }

    pub fn set_progress(&mut self, val: f32) {
        if val > 0.0 && val <= 100.0 {
            self.progress = val;
        }
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn install_type(&self) -> &PluginInstallType {
        &self.install_type
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }
}