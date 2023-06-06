use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use floem::{
    ext_event::create_ext_action,
    reactive::{
        create_effect, create_rw_signal, use_context, RwSignal, Scope,
        SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
        SignalWithUntracked,
    },
};
use indexmap::IndexMap;
use lapce_core::mode::Mode;
use lapce_proxy::plugin::{download_volt, volt_icon, wasi::find_all_volts};
use lapce_rpc::plugin::{VoltID, VoltInfo, VoltMetadata};
use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandExecuted, CommandKind},
    db::LapceDb,
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

#[derive(Deserialize, Serialize)]
pub struct VoltsInfo {
    pub plugins: Vec<VoltInfo>,
    pub total: usize,
}

#[derive(Clone)]
pub struct InstalledVoltData {
    pub meta: RwSignal<VoltMetadata>,
    pub icon: RwSignal<Option<Vec<u8>>>,
    pub latest: RwSignal<VoltInfo>,
}

#[derive(Clone, PartialEq)]
pub struct AvailableVoltData {
    pub info: RwSignal<VoltInfo>,
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
    pub common: CommonData,
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
        mods: floem::glazier::Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.all.query_editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
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
        common: CommonData,
    ) -> Self {
        let installed = create_rw_signal(cx, IndexMap::new());
        let all = AvailableVoltList {
            loading: create_rw_signal(cx, false),
            volts: create_rw_signal(cx, IndexMap::new()),
            total: create_rw_signal(cx, 0),
            query_id: create_rw_signal(cx, 0),
            query_editor: EditorData::new_local(
                cx,
                EditorId::next(),
                common.clone(),
            ),
        };
        let disabled = create_rw_signal(cx, disabled);
        let workspace_disabled = create_rw_signal(cx, workspace_disabled);

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
            create_effect(cx, move |s| {
                let query = plugin
                    .all
                    .query_editor
                    .doc
                    .with(|doc| doc.buffer().to_string());
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
        self.installed.update(|installed| {
            if let Some(v) = installed.get_mut(&volt_id) {
                v.meta.set(volt.clone());
                v.icon.set(icon.clone());
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

                let latest = create_rw_signal(self.common.scope, info);
                let data = InstalledVoltData {
                    meta: create_rw_signal(self.common.scope, volt.clone()),
                    icon: create_rw_signal(self.common.scope, icon.clone()),
                    latest,
                };
                installed.insert(volt_id, data);

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
                        let info: Option<VoltInfo> = reqwest::blocking::get(url)
                            .ok()
                            .and_then(|r| r.json().ok());
                        send(info);
                    });
                }
            }
        })
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
            let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
            db.save_disabled_volts(
                self.disabled.get_untracked().into_iter().collect(),
            );
        }

        if self.workspace_disabled.with_untracked(|d| d.contains(&id)) {
            self.workspace_disabled.update(|d| {
                d.remove(&id);
            });
            let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
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
                            (
                                volt.id(),
                                AvailableVoltData {
                                    info: create_rw_signal(cx, volt),
                                    installing: create_rw_signal(cx, false),
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
            .doc
            .with_untracked(|doc| doc.buffer().to_string());
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
        let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
        db.save_disabled_volts(self.disabled.get_untracked().into_iter().collect());
    }

    pub fn disable_volt(&self, volt: VoltInfo) {
        let id = volt.id();
        self.disabled.update(|d| {
            d.insert(id);
        });
        self.common.proxy.disable_volt(volt);
        let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
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
        let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
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
        let db: Arc<LapceDb> = use_context(self.common.scope).unwrap();
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
