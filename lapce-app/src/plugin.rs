use std::{collections::HashSet, rc::Rc, sync::Arc};

use anyhow::Result;
use floem::{
    ext_event::create_ext_action,
    keyboard::ModifiersState,
    reactive::{use_context, RwSignal, Scope},
};
use indexmap::IndexMap;
use lapce_core::{directory::Directory, mode::Mode};
use lapce_proxy::plugin::{download_volt, volt_icon, wasi::find_all_volts};
use lapce_rpc::plugin::{VoltID, VoltInfo, VoltMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    command::{CommandExecuted, CommandKind},
    db::LapceDb,
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

#[derive(Clone)]
pub enum VoltIcon {
    Svg(String),
    Img(Vec<u8>),
}

impl VoltIcon {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if let Ok(s) = std::str::from_utf8(buf) {
            Ok(VoltIcon::Svg(s.to_string()))
        } else {
            Ok(VoltIcon::Img(buf.to_vec()))
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct VoltsInfo {
    pub plugins: Vec<VoltInfo>,
    pub total: usize,
}

#[derive(Clone)]
pub struct InstalledVoltData {
    pub meta: RwSignal<VoltMetadata>,
    pub icon: RwSignal<Option<VoltIcon>>,
    pub latest: RwSignal<VoltInfo>,
}

#[derive(Clone, PartialEq)]
pub struct AvailableVoltData {
    pub info: RwSignal<VoltInfo>,
    pub icon: RwSignal<Option<VoltIcon>>,
    pub installing: RwSignal<bool>,
}

#[derive(Clone)]
pub struct AvailableVoltList {
    pub loading: RwSignal<bool>,
    pub query_id: RwSignal<usize>,
    pub query_editor: EditorData,
    pub volts: RwSignal<IndexMap<VoltID, AvailableVoltData>>,
    pub total: RwSignal<usize>,
}

#[derive(Clone)]
pub struct PluginData {
    pub installed: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
    pub all: AvailableVoltList,
    pub disabled: RwSignal<HashSet<VoltID>>,
    pub workspace_disabled: RwSignal<HashSet<VoltID>>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for PluginData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::PanelFocus)
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                return self.all.query_editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::No
    }

    fn receive_char(&self, c: &str) {
        self.all.query_editor.receive_char(c);
    }
}

impl PluginData {
    pub fn new(
        cx: Scope,
        disabled: HashSet<VoltID>,
        workspace_disabled: HashSet<VoltID>,
        common: Rc<CommonData>,
    ) -> Self {
        let installed = cx.create_rw_signal(IndexMap::new());
        let all = AvailableVoltList {
            loading: cx.create_rw_signal(false),
            volts: cx.create_rw_signal(IndexMap::new()),
            total: cx.create_rw_signal(0),
            query_id: cx.create_rw_signal(0),
            query_editor: EditorData::new_local(
                cx,
                EditorId::next(),
                common.clone(),
            ),
        };
        let disabled = cx.create_rw_signal(disabled);
        let workspace_disabled = cx.create_rw_signal(workspace_disabled);

        let plugin = Self {
            installed,
            all,
            disabled,
            workspace_disabled,
            common,
        };

        plugin.load_available_volts("", 0);

        {
            let plugin = plugin.clone();
            let send = create_ext_action(
                cx,
                move |volts: Vec<(Option<Vec<u8>>, VoltMetadata)>| {
                    for (icon, meta) in volts {
                        plugin.volt_installed(&meta, &icon);
                    }
                },
            );
            std::thread::spawn(move || {
                let volts = find_all_volts();
                let volts = volts
                    .into_iter()
                    .filter_map(|meta| {
                        if meta.wasm.is_none() {
                            Some((volt_icon(&meta), meta))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                send(volts);
            });
        }

        {
            let plugin = plugin.clone();
            cx.create_effect(move |s| {
                let query = plugin
                    .all
                    .query_editor
                    .view
                    .doc
                    .get()
                    .buffer
                    .with(|buffer| buffer.to_string());
                if s.as_ref() == Some(&query) {
                    return query;
                }
                plugin.all.query_id.update(|id| *id += 1);
                plugin.all.loading.set(false);
                plugin.all.volts.update(|v| v.clear());
                plugin.load_available_volts(&query, 0);
                query
            });
        }

        plugin
    }

    pub fn volt_installed(&self, volt: &VoltMetadata, icon: &Option<Vec<u8>>) {
        let volt_id = volt.id();
        let (is_latest, latest) = self
            .installed
            .try_update(|installed| {
                if let Some(v) = installed.get_mut(&volt_id) {
                    v.meta.set(volt.clone());
                    v.icon.set(
                        icon.as_ref()
                            .and_then(|icon| VoltIcon::from_bytes(icon).ok()),
                    );
                    (true, v.latest)
                } else {
                    let (info, is_latest) = if let Some(volt) = self
                        .all
                        .volts
                        .with_untracked(|all| all.get(&volt_id).cloned())
                    {
                        (volt.info.get_untracked(), true)
                    } else {
                        (volt.info(), false)
                    };

                    let latest = self.common.scope.create_rw_signal(info);
                    let data = InstalledVoltData {
                        meta: self.common.scope.create_rw_signal(volt.clone()),
                        icon: self.common.scope.create_rw_signal(
                            icon.as_ref()
                                .and_then(|icon| VoltIcon::from_bytes(icon).ok()),
                        ),
                        latest,
                    };
                    installed.insert(volt_id, data);

                    (is_latest, latest)
                }
            })
            .unwrap();

        if !is_latest {
            let url = format!(
                "https://plugins.lapce.dev/api/v1/plugins/{}/{}/latest",
                volt.author, volt.name
            );
            let send = create_ext_action(self.common.scope, move |info| {
                if let Some(info) = info {
                    latest.set(info);
                }
            });
            std::thread::spawn(move || {
                let info: Option<VoltInfo> =
                    reqwest::blocking::get(url).ok().and_then(|r| r.json().ok());
                send(info);
            });
        }
    }

    pub fn volt_removed(&self, volt: &VoltInfo) {
        let id = volt.id();
        self.installed.update(|installed| {
            installed.remove(&id);
        });

        if self.disabled.with_untracked(|d| d.contains(&id)) {
            self.disabled.update(|d| {
                d.remove(&id);
            });
            let db: Arc<LapceDb> = use_context().unwrap();
            db.save_disabled_volts(
                self.disabled.get_untracked().into_iter().collect(),
            );
        }

        if self.workspace_disabled.with_untracked(|d| d.contains(&id)) {
            self.workspace_disabled.update(|d| {
                d.remove(&id);
            });
            let db: Arc<LapceDb> = use_context().unwrap();
            db.save_workspace_disabled_volts(
                self.common.workspace.clone(),
                self.workspace_disabled
                    .get_untracked()
                    .into_iter()
                    .collect(),
            );
        }
    }

    fn load_available_volts(&self, query: &str, offset: usize) {
        if self.all.loading.get_untracked() {
            return;
        }
        self.all.loading.set(true);

        let volts = self.all.volts;
        let volts_total = self.all.total;
        let cx = self.common.scope;
        let loading = self.all.loading;
        let query_id = self.all.query_id;
        let current_query_id = self.all.query_id.get_untracked();
        let send =
            create_ext_action(self.common.scope, move |new: Result<VoltsInfo>| {
                loading.set(false);
                if query_id.get_untracked() != current_query_id {
                    return;
                }

                if let Ok(new) = new {
                    volts.update(|volts| {
                        volts.extend(new.plugins.into_iter().map(|volt| {
                            let icon = cx.create_rw_signal(None);
                            let send = create_ext_action(cx, move |result| {
                                if let Ok(i) = result {
                                    icon.set(Some(i));
                                }
                            });
                            {
                                let volt = volt.clone();
                                std::thread::spawn(move || {
                                    let result = Self::load_icon(&volt);
                                    send(result);
                                });
                            }

                            (
                                volt.id(),
                                AvailableVoltData {
                                    info: cx.create_rw_signal(volt),
                                    icon,
                                    installing: cx.create_rw_signal(false),
                                },
                            )
                        }));
                    });
                    volts_total.set(new.total);
                }
            });

        let query = query.to_string();
        std::thread::spawn(move || {
            let volts = Self::query_volts(&query, offset);
            send(volts);
        });
    }

    fn load_icon(volt: &VoltInfo) -> Result<VoltIcon> {
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

        VoltIcon::from_bytes(&content)
    }

    fn query_volts(query: &str, offset: usize) -> Result<VoltsInfo> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins?q={query}&offset={offset}"
        );
        let plugins: VoltsInfo = reqwest::blocking::get(url)?.json()?;
        Ok(plugins)
    }

    fn all_loaded(&self) -> bool {
        self.all.volts.with_untracked(|v| v.len()) >= self.all.total.get_untracked()
    }

    pub fn load_more_available(&self) {
        if self.all_loaded() {
            return;
        }

        let query = self
            .all
            .query_editor
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let offset = self.all.volts.with_untracked(|v| v.len());
        self.load_available_volts(&query, offset);
    }

    pub fn install_volt(&self, info: VoltInfo) {
        self.all.volts.with_untracked(|volts| {
            if let Some(volt) = volts.get(&info.id()) {
                volt.installing.set(true);
            };
        });
        if info.wasm {
            self.common.proxy.install_volt(info);
        } else {
            let plugin = self.clone();
            let send = create_ext_action(self.common.scope, move |result| {
                if let Ok((meta, icon)) = result {
                    plugin.volt_installed(&meta, &icon);
                }
            });
            std::thread::spawn(move || {
                let download = || -> Result<(VoltMetadata, Option<Vec<u8>>)> {
                    let download_volt_result = download_volt(&info);
                    let meta = download_volt_result?;
                    let icon = volt_icon(&meta);
                    Ok((meta, icon))
                };
                send(download());
            });
        }
    }

    pub fn plugin_disabled(&self, id: &VoltID) -> bool {
        self.disabled.with_untracked(|d| d.contains(id))
            || self.workspace_disabled.with_untracked(|d| d.contains(id))
    }

    pub fn enable_volt(&self, volt: VoltInfo) {
        let id = volt.id();
        self.disabled.update(|d| {
            d.remove(&id);
        });
        if !self.plugin_disabled(&id) {
            self.common.proxy.enable_volt(volt);
        }
        let db: Arc<LapceDb> = use_context().unwrap();
        db.save_disabled_volts(self.disabled.get_untracked().into_iter().collect());
    }

    pub fn disable_volt(&self, volt: VoltInfo) {
        let id = volt.id();
        self.disabled.update(|d| {
            d.insert(id);
        });
        self.common.proxy.disable_volt(volt);
        let db: Arc<LapceDb> = use_context().unwrap();
        db.save_disabled_volts(self.disabled.get_untracked().into_iter().collect());
    }

    pub fn enable_volt_for_ws(&self, volt: VoltInfo) {
        let id = volt.id();
        self.workspace_disabled.update(|d| {
            d.remove(&id);
        });
        if !self.plugin_disabled(&id) {
            self.common.proxy.enable_volt(volt);
        }
        let db: Arc<LapceDb> = use_context().unwrap();
        db.save_workspace_disabled_volts(
            self.common.workspace.clone(),
            self.disabled.get_untracked().into_iter().collect(),
        );
    }

    pub fn disable_volt_for_ws(&self, volt: VoltInfo) {
        let id = volt.id();
        self.workspace_disabled.update(|d| {
            d.insert(id);
        });
        self.common.proxy.disable_volt(volt);
        let db: Arc<LapceDb> = use_context().unwrap();
        db.save_workspace_disabled_volts(
            self.common.workspace.clone(),
            self.disabled.get_untracked().into_iter().collect(),
        );
    }

    pub fn uninstall_volt(&self, volt: VoltMetadata) {
        if volt.wasm.is_some() {
            self.common.proxy.remove_volt(volt);
        } else {
            let plugin = self.clone();
            let info = volt.info();
            let send =
                create_ext_action(self.common.scope, move |result: Result<()>| {
                    if let Ok(()) = result {
                        plugin.volt_removed(&info);
                    }
                });
            std::thread::spawn(move || {
                let uninstall = || -> Result<()> {
                    let path = volt
                        .dir
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("don't have dir"))?;
                    std::fs::remove_dir_all(path)?;
                    Ok(())
                };
                send(uninstall());
            });
        }
    }

    pub fn reload_volt(&self, volt: VoltMetadata) {
        self.common.proxy.reload_volt(volt);
    }
}
