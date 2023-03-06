pub mod plugin_install_status;

use std::{collections::HashSet, str::FromStr, sync::Arc};

use anyhow::Result;
use druid::{
    piet::{PietImage, Svg},
    ExtEventSink, Target, WidgetId,
};
use indexmap::IndexMap;
use lapce_core::directory::Directory;
use lapce_proxy::plugin::{download_volt, volt_icon, wasi::find_all_volts};
use lapce_rpc::plugin::{VoltID, VoltInfo, VoltMetadata};
use parking_lot::Mutex;
use plugin_install_status::PluginInstallStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use strum_macros::Display;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceConfig,
    markdown::parse_markdown,
    proxy::LapceProxy,
};

#[derive(Clone)]
pub enum VoltIconKind {
    Svg(Arc<Svg>),
    Image(Arc<PietImage>),
}

impl VoltIconKind {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if let Ok(s) = std::str::from_utf8(buf) {
            let svg =
                Svg::from_str(s).map_err(|_| anyhow::anyhow!("can't parse svg"))?;
            Ok(VoltIconKind::Svg(Arc::new(svg)))
        } else {
            let image = PietImage::from_bytes(buf)
                .map_err(|_| anyhow::anyhow!("can't resolve image"))?;
            Ok(VoltIconKind::Image(Arc::new(image)))
        }
    }
}

#[derive(Clone)]
pub struct VoltsList {
    pub tab_id: WidgetId,
    pub total: usize,
    pub volts: IndexMap<VoltID, VoltInfo>,
    pub icons: im::HashMap<VoltID, VoltIconKind>,
    pub status: PluginLoadStatus,
    pub loading: Arc<Mutex<bool>>,
    pub event_sink: ExtEventSink,
    pub query: String,
}

impl VoltsList {
    pub fn new(tab_id: WidgetId, event_sink: ExtEventSink) -> Self {
        Self {
            tab_id,
            volts: IndexMap::new(),
            icons: im::HashMap::new(),
            total: 0,
            status: PluginLoadStatus::Loading,
            loading: Arc::new(Mutex::new(false)),
            query: "".to_string(),
            event_sink,
        }
    }

    pub fn update_query(&mut self, query: String) {
        if self.query == query {
            return;
        }

        self.query = query;
        self.volts.clear();
        self.total = 0;
        self.status = PluginLoadStatus::Loading;

        let event_sink = self.event_sink.clone();
        let query = self.query.clone();
        let tab_id = self.tab_id;
        std::thread::spawn(move || {
            let _ =
                PluginData::load_volts(tab_id, true, &query, 0, None, event_sink);
        });
    }

    pub fn load_more(&self) {
        if self.all_loaded() {
            return;
        }

        let mut loading = self.loading.lock();
        if *loading {
            return;
        }
        *loading = true;

        let offset = self.volts.len();
        let local_loading = self.loading.clone();
        let event_sink = self.event_sink.clone();
        let query = self.query.clone();
        let tab_id = self.tab_id;
        std::thread::spawn(move || {
            let _ = PluginData::load_volts(
                tab_id,
                false,
                &query,
                offset,
                Some(local_loading),
                event_sink,
            );
        });
    }

    fn all_loaded(&self) -> bool {
        self.volts.len() == self.total
    }

    pub fn update_volts(&mut self, info: &PluginsInfo) {
        self.total = info.total;
        for v in &info.plugins {
            self.volts.insert(v.id(), v.clone());
        }
        self.status = PluginLoadStatus::Success;
        *self.loading.lock() = false;
    }

    pub fn failed(&mut self) {
        self.status = PluginLoadStatus::Failed;
    }
}

#[derive(Clone)]
pub struct PluginData {
    pub widget_id: WidgetId,
    pub search_editor: WidgetId,
    pub installed_id: WidgetId,
    pub uninstalled_id: WidgetId,

    pub volts: VoltsList,
    pub installing: IndexMap<VoltID, PluginInstallStatus>,
    pub installed: IndexMap<VoltID, VoltMetadata>,
    pub installed_latest: IndexMap<VoltID, VoltInfo>,
    pub installed_icons: im::HashMap<VoltID, VoltIconKind>,
    pub disabled: HashSet<VoltID>,
    pub workspace_disabled: HashSet<VoltID>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PluginLoadStatus {
    Loading,
    Failed,
    Success,
}

#[derive(Deserialize, Serialize)]
pub struct PluginsInfo {
    pub plugins: Vec<VoltInfo>,
    pub total: usize,
}

impl PluginData {
    pub fn new(
        tab_id: WidgetId,
        disabled: Vec<VoltID>,
        workspace_disabled: Vec<VoltID>,
        event_sink: ExtEventSink,
    ) -> Self {
        {
            let event_sink = event_sink.clone();
            std::thread::spawn(move || {
                Self::load(tab_id, event_sink);
            });
        }

        Self {
            widget_id: WidgetId::next(),
            search_editor: WidgetId::next(),
            installed_id: WidgetId::next(),
            uninstalled_id: WidgetId::next(),
            volts: VoltsList::new(tab_id, event_sink),
            installing: IndexMap::new(),
            installed: IndexMap::new(),
            installed_latest: IndexMap::new(),
            installed_icons: im::HashMap::new(),
            disabled: HashSet::from_iter(disabled.into_iter()),
            workspace_disabled: HashSet::from_iter(workspace_disabled.into_iter()),
        }
    }

    fn load(tab_id: WidgetId, event_sink: ExtEventSink) {
        for meta in find_all_volts() {
            if meta.wasm.is_none() {
                let icon = volt_icon(&meta);
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalled(meta, icon),
                    Target::Widget(tab_id),
                );
            }
        }

        std::thread::spawn(move || {
            let _ = PluginData::load_volts(tab_id, true, "", 0, None, event_sink);
        });
    }

    pub fn plugin_disabled(&self, id: &VoltID) -> bool {
        self.disabled.contains(id) || self.workspace_disabled.contains(id)
    }

    pub fn plugin_status(&self, id: &VoltID) -> PluginStatus {
        if self.plugin_disabled(id) {
            return PluginStatus::Disabled;
        }

        if let Some(meta) = self.installed.get(id) {
            if let Some(volt) = self
                .installed_latest
                .get(id)
                .or_else(|| self.volts.volts.get(id))
            {
                if meta.version == volt.version {
                    PluginStatus::Installed
                } else {
                    PluginStatus::Upgrade(volt.version.clone())
                }
            } else {
                PluginStatus::Installed
            }
        } else {
            PluginStatus::Install
        }
    }

    fn load_icon(volt: &VoltInfo) -> Result<VoltIconKind> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins/{}/{}/{}/icon?id={}",
            volt.author, volt.name, volt.version, volt.updated_at_ts
        );

        let cache_file_path = Directory::cache_directory().map(|cache_dir| {
            let mut hasher = Sha256::new();
            hasher.update(url.as_bytes());
            let filename = format!("{:x}", hasher.finalize());
            cache_dir.join(filename)
        });

        let cache_content =
            cache_file_path.as_ref().and_then(|p| std::fs::read(p).ok());

        let content = match cache_content {
            Some(content) => content,
            None => {
                let resp = reqwest::blocking::get(&url)?;
                if !resp.status().is_success() {
                    return Err(anyhow::anyhow!("can't download icon"));
                }
                let buf = resp.bytes()?.to_vec();

                if let Some(path) = cache_file_path.as_ref() {
                    let _ = std::fs::write(path, &buf);
                }

                buf
            }
        };

        VoltIconKind::from_bytes(&content)
    }

    fn load_volts(
        tab_id: WidgetId,
        inital: bool,
        query: &str,
        offset: usize,
        loading: Option<Arc<Mutex<bool>>>,
        event_sink: ExtEventSink,
    ) -> Result<()> {
        match PluginData::query_volts(query, offset) {
            Ok(info) => {
                for v in info.plugins.iter() {
                    {
                        let volt = v.clone();
                        let event_sink = event_sink.clone();
                        std::thread::spawn(move || -> Result<()> {
                            let icon = Self::load_icon(&volt)?;
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::LoadPluginIcon(volt.id(), icon),
                                Target::Widget(tab_id),
                            );
                            Ok(())
                        });
                    }
                }

                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPlugins(info),
                    Target::Widget(tab_id),
                );
            }
            Err(_) => {
                if inital {
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::LoadPluginsFailed,
                        Target::Widget(tab_id),
                    );
                }
                if let Some(loading) = loading.as_ref() {
                    *loading.lock() = false;
                }
            }
        }
        Ok(())
    }

    fn query_volts(query: &str, offset: usize) -> Result<PluginsInfo> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins?q={query}&offset={offset}"
        );
        let plugins: PluginsInfo = reqwest::blocking::get(url)?.json()?;
        Ok(plugins)
    }

    pub fn download_readme(
        widget_id: WidgetId,
        volt: &VoltInfo,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> Result<()> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins/{}/{}/{}/readme",
            volt.author, volt.name, volt.version
        );
        let resp = reqwest::blocking::get(url)?;
        if resp.status() != 200 {
            let text = parse_markdown("Plugin doesn't have a README", 2.0, config);
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateVoltReadme(Arc::new(text)),
                Target::Widget(widget_id),
            );
            return Ok(());
        }
        let text = resp.text()?;
        let text = parse_markdown(&text, 2.0, config);
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateVoltReadme(Arc::new(text)),
            Target::Widget(widget_id),
        );
        Ok(())
    }

    pub fn install_volt(proxy: Arc<LapceProxy>, volt: VoltInfo) -> Result<()> {
        proxy.core_rpc.volt_installing(volt.clone(), "".to_string());

        if volt.wasm {
            proxy.proxy_rpc.install_volt(volt);
        } else {
            std::thread::spawn(move || -> Result<()> {
                let download_volt_result = download_volt(&volt);
                if let Err(err) = download_volt_result {
                    log::warn!("download_volt err: {err:?}");
                    proxy.core_rpc.volt_installing(
                        volt.clone(),
                        "Could not download Plugin".to_string(),
                    );
                    return Ok(());
                }

                let meta = download_volt_result?;
                let icon = volt_icon(&meta);
                proxy.core_rpc.volt_installed(meta, icon);
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
                } else {
                    proxy.core_rpc.volt_removed(meta.info(), false);
                }
                Ok(())
            });
        }
        Ok(())
    }

    pub fn volt_installed(
        &mut self,
        tab_id: WidgetId,
        volt: &VoltMetadata,
        icon: &Option<Vec<u8>>,
        event_sink: ExtEventSink,
    ) {
        let volt_id = volt.id();

        self.installing.remove(&volt_id);
        self.installed.insert(volt_id.clone(), volt.clone());

        if let Some(icon) = icon
            .as_ref()
            .and_then(|icon| VoltIconKind::from_bytes(icon).ok())
        {
            self.installed_icons.insert(volt_id.clone(), icon);
        }

        if !self.volts.volts.contains_key(&volt_id) {
            let url = format!(
                "https://plugins.lapce.dev/api/v1/plugins/{}/{}/latest",
                volt.author, volt.name
            );
            std::thread::spawn(move || -> Result<()> {
                let info: VoltInfo = reqwest::blocking::get(url)?.json()?;
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::LoadPluginLatest(info),
                    Target::Widget(tab_id),
                );
                Ok(())
            });
        }
    }
}

#[derive(Display, PartialEq, Eq, Clone)]
pub enum PluginStatus {
    Installed,
    Install,
    Upgrade(String),
    Disabled,
}
