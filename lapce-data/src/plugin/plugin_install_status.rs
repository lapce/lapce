#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PluginInstallType {
    Installation,
    Uninstallation,
}

#[derive(Clone, Debug)]
pub struct PluginInstallStatus {
    error: String,
    plugin_name: String,
    install_type: PluginInstallType,
}

impl PluginInstallStatus {
    pub fn new(
        install_type: PluginInstallType,
        plugin_name: &str,
        error: String,
    ) -> Self {
        Self {
            error,
            plugin_name: plugin_name.to_string(),
            install_type,
        }
    }

    pub fn set_error(&mut self, error_string: &str) {
        self.error = error_string.to_string();
    }

    pub fn error_string(&self) -> &str {
        &self.error
    }

    pub fn install_type(&self) -> &PluginInstallType {
        &self.install_type
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }
}
