use std::{collections::HashSet, io::Write, sync::Arc};

use anyhow::Result;
use druid::{ExtEventSink, Target, WidgetId};
use indexmap::IndexMap;
use lapce_proxy::plugin::{
    download_volt,
    wasi::{find_all_volts, load_volt},
};
use lapce_rpc::plugin::{VoltInfo, VoltMetadata};
use strum_macros::Display;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
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
    pub installed: IndexMap<String, VoltMetadata>,
    pub disabled: HashSet<String>,
}

#[derive(Clone)]
pub enum PluginLoadStatus {
    Loading,
    Failed,
    Success,
}

impl PluginData {
    pub fn new(disabled: Vec<String>) -> Self {
        Self {
            widget_id: WidgetId::next(),
            installed_id: WidgetId::next(),
            uninstalled_id: WidgetId::next(),
            volts: VoltsList::new(),
            installed: IndexMap::new(),
            disabled: HashSet::from_iter(disabled.into_iter()),
        }
    }

    pub fn load(event_sink: ExtEventSink) {
        for meta in find_all_volts() {
            if meta.wasm.is_none() {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalled(meta),
                    Target::Auto,
                );
            }
        }

        match Self::load_volts() {
            Ok(volts) => {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPlugins(volts),
                    Target::Auto,
                );
            }
            Err(_) => {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPluginsFailed,
                    Target::Auto,
                );
            }
        }
    }

    pub fn plugin_status(&self, id: &str) -> PluginStatus {
        if self.disabled.contains(id) {
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
            reqwest::blocking::get("https://lapce.dev/volts")?.json()?;
        Ok(volts)
    }

    pub fn install_volt(proxy: Arc<LapceProxy>, volt: VoltInfo) -> Result<()> {
        let meta_str = reqwest::blocking::get(&volt.meta)?.text()?;
        let meta: VoltMetadata = toml_edit::easy::from_str(&meta_str)?;
        if meta.wasm.is_some() {
            proxy.proxy_rpc.install_volt(volt);
        } else {
            std::thread::spawn(move || -> Result<()> {
                let meta = download_volt(volt, false)?;
                proxy.core_rpc.volt_installed(meta);
                Ok(())
            });
        }
        Ok(())
    }

    pub fn remove_volt(proxy: Arc<LapceProxy>, meta: VoltMetadata) -> Result<()> {
        if meta.wasm.is_some() {
            proxy.proxy_rpc.remove_volt(meta);
        } else {
            std::thread::spawn(move || -> Result<()> {
                let path = meta
                    .dir
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("don't have dir"))?;
                std::fs::remove_dir_all(path)?;
                proxy.core_rpc.volt_removed(meta.info());
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
