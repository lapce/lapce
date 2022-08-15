use std::collections::HashSet;

use anyhow::Result;
use druid::{ExtEventSink, Target, WidgetId};
use im::HashMap;
use indexmap::IndexMap;
use lapce_rpc::plugin::VoltInfo;
use strum_macros::Display;

use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};

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
    pub installed: VoltsList,
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
            installed: VoltsList::new(),
            disabled: HashSet::from_iter(disabled.into_iter()),
        }
    }

    pub fn load(event_sink: ExtEventSink) {
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

    pub fn plugin_status(&self, id: &str, version: &str) -> PluginStatus {
        if self.disabled.contains(id) {
            return PluginStatus::Disabled;
        }

        let status = match self
            .installed
            .volts
            .get(id)
            .map(|installed| version == installed.version)
        {
            Some(true) => PluginStatus::Installed,
            Some(false) => PluginStatus::Upgrade,
            None => PluginStatus::Install,
        };
        status
    }

    fn load_volts() -> Result<Vec<VoltInfo>> {
        let volts: Vec<VoltInfo> =
            reqwest::blocking::get("https://lapce.dev/volts")?.json()?;
        Ok(volts)
    }
}

#[derive(Display, PartialEq, Eq)]
pub enum PluginStatus {
    Installed,
    Install,
    Upgrade,
    Disabled,
}
