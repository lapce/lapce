#[derive(Clone, Debug)]
pub enum PluginInstallType {
    INSTALLATION,
    UNINSTALLATION,
}

#[derive(Clone, Debug)]
pub struct PluginInstallStatus {
    progress: f32,
    install_type: PluginInstallType
}

impl PluginInstallStatus {
    pub fn new(install_type: PluginInstallType) -> Self {
        Self {
            progress: 0.0,
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
}