pub mod plugin_install_status;

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use druid::{ExtEventSink, Target, WidgetId};
use indexmap::IndexMap;
use lapce_proxy::plugin::{download_volt, wasi::find_all_volts};
use lapce_rpc::plugin::{VoltInfo, VoltMetadata};
use lsp_types::Url;
use plugin_install_status::PluginInstallStatus;
use strum_macros::Display;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceConfig,
    markdown::parse_markdown,
    proxy::LapceProxy,
};

#[derive(Clone)]
pub struct VoltsList {
    pub volts: IndexMap<String, VoltInfo>,
    pub status: PluginLoadStatus,
}

impl VoltsList {
    pub fn new() -> Self {
        Self {
            volts: IndexMap::new(),
            status: PluginLoadStatus::Loading,
        }
    }

    pub fn update_volts(&mut self, volts: &[VoltInfo]) {
        self.volts.clear();
        for v in volts {
            self.volts.insert(v.id(), v.clone());
        }
        self.status = PluginLoadStatus::Success;
    }

    pub fn failed(&mut self) {
        self.status = PluginLoadStatus::Failed;
    }

    pub fn update(&mut self, volt: &VoltInfo) {
        let volt_id = volt.id();
        if let Some(v) = self.volts.get_mut(&volt_id) {
            *v = volt.clone();
        } else {
            self.volts.insert(volt_id, volt.clone());
        }
        self.status = PluginLoadStatus::Success;
    }

    pub fn remove(&mut self, volt: &VoltInfo) {
        let volt_id = volt.id();
        self.volts.remove(&volt_id);
    }
}

impl Default for VoltsList {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct PluginData {
    pub widget_id: WidgetId,
    pub installed_id: WidgetId,
    pub uninstalled_id: WidgetId,

    pub volts: VoltsList,
    pub installing: IndexMap<String, PluginInstallStatus>,
    pub installed: IndexMap<String, VoltMetadata>,
    pub disabled: HashSet<String>,
    pub workspace_disabled: HashSet<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PluginLoadStatus {
    Loading,
    Failed,
    Success,
}

impl PluginData {
    pub fn new(
        tab_id: WidgetId,
        disabled: Vec<String>,
        workspace_disabled: Vec<String>,
        event_sink: ExtEventSink,
    ) -> Self {
        std::thread::spawn(move || {
            Self::load(tab_id, event_sink);
        });

        Self {
            widget_id: WidgetId::next(),
            installed_id: WidgetId::next(),
            uninstalled_id: WidgetId::next(),
            volts: VoltsList::new(),
            installing: IndexMap::new(),
            installed: IndexMap::new(),
            disabled: HashSet::from_iter(disabled.into_iter()),
            workspace_disabled: HashSet::from_iter(workspace_disabled.into_iter()),
        }
    }

    fn load(tab_id: WidgetId, event_sink: ExtEventSink) {
        for meta in find_all_volts() {
            if meta.wasm.is_none() {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalled(meta, false),
                    Target::Widget(tab_id),
                );
            }
        }

        match Self::load_volts() {
            Ok(volts) => {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPlugins(volts),
                    Target::Widget(tab_id),
                );
            }
            Err(_) => {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPluginsFailed,
                    Target::Widget(tab_id),
                );
            }
        }
    }

    pub fn plugin_disabled(&self, id: &str) -> bool {
        self.disabled.contains(id) || self.workspace_disabled.contains(id)
    }

    pub fn plugin_status(&self, id: &str) -> PluginStatus {
        if self.plugin_disabled(id) {
            return PluginStatus::Disabled;
        }

        if let Some(meta) = self.installed.get(id) {
            if let Some(volt) = self.volts.volts.get(id) {
                if meta.version == volt.version {
                    PluginStatus::Installed
                } else {
                    PluginStatus::Upgrade(volt.meta.clone())
                }
            } else {
                PluginStatus::Installed
            }
        } else {
            PluginStatus::Install
        }
    }

    fn load_volts() -> Result<Vec<VoltInfo>> {
        let volts: Vec<VoltInfo> =
            reqwest::blocking::get("https://lapce.dev/volts2")?.json()?;
        Ok(volts)
    }

    pub fn download_readme(
        widget_id: WidgetId,
        volt: &VoltInfo,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> Result<()> {
        let url = Url::parse(&volt.meta)?;
        let url = url.join("./README.md")?;
        let text = reqwest::blocking::get(url)?.text()?;
        let text = parse_markdown(&text, config);
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateVoltReadme(text),
            Target::Widget(widget_id),
        );
        Ok(())
    }

    pub fn install_volt(proxy: Arc<LapceProxy>, volt: VoltInfo) -> Result<()> {
        let meta_str = reqwest::blocking::get(&volt.meta)?.text()?;
        let meta: VoltMetadata = toml_edit::easy::from_str(&meta_str)?;

        proxy.core_rpc.volt_installing(meta.clone(), "".to_string());

        if meta.wasm.is_some() {
            proxy.proxy_rpc.install_volt(volt);
        } else {
            std::thread::spawn(move || -> Result<()> {
                let download_volt_result =
                    download_volt(volt, false, &meta, &meta_str);
                if let Err(err) = download_volt_result {
                    log::warn!("download_volt err: {err:?}");
                    proxy.core_rpc.volt_installing(
                        meta.clone(),
                        "Could not download Volt".to_string(),
                    );
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        proxy.core_rpc.volt_installed(meta, true);
                    });
                    return Ok(());
                }

                let meta = download_volt_result?;
                proxy.core_rpc.volt_installed(meta, false);
                Ok(())
            });
        }
        Ok(())
    }

    pub fn remove_volt(proxy: Arc<LapceProxy>, meta: VoltMetadata) -> Result<()> {
        proxy.core_rpc.volt_removing(meta.clone(), "".to_string());
        if meta.wasm.is_some() {
            proxy.proxy_rpc.remove_volt(meta);
        } else {
            std::thread::spawn(move || -> Result<()> {
                let path = meta.dir.as_ref().ok_or_else(|| {
                    proxy.core_rpc.volt_removing(
                        meta.clone(),
                        "Plugin Directory does not exist".to_string(),
                    );
                    anyhow::anyhow!("don't have dir")
                })?;
                if std::fs::remove_dir_all(path).is_err() {
                    proxy.core_rpc.volt_removing(
                        meta.clone(),
                        "Could not remove Plugin Directory".to_string(),
                    );
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        proxy.core_rpc.volt_removed(meta.info(), true);
                    });
                } else {
                    proxy.core_rpc.volt_removed(meta.info(), false);
                }
                Ok(())
            });
        }
        Ok(())
    }
}

#[derive(Display, PartialEq, Eq, Clone)]
pub enum PluginStatus {
    Installed,
    Install,
    Upgrade(String),
    Disabled,
}
