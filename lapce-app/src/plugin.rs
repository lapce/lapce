use std::{
    collections::HashSet,
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
};

use anyhow::Result;
use floem::{
    action::show_context_menu,
    ext_event::create_ext_action,
    keyboard::ModifiersState,
    kurbo::Rect,
    menu::{Menu, MenuItem},
    reactive::{
        create_effect, create_memo, create_rw_signal, use_context, RwSignal, Scope,
    },
    style::CursorStyle,
    view::View,
    views::{
        container_box, dyn_container, dyn_stack, empty, img, label, rich_text,
        scroll, stack, svg, text, Decorators,
    },
};
use indexmap::IndexMap;
use lapce_core::{directory::Directory, mode::Mode};
use lapce_proxy::plugin::{download_volt, volt_icon, wasi::find_all_volts};
use lapce_rpc::plugin::{VoltID, VoltInfo, VoltMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    command::{CommandExecuted, CommandKind},
    config::{color::LapceColor, LapceConfig},
    db::LapceDb,
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    markdown::{parse_markdown, MarkdownContent},
    panel::plugin_view::VOLT_DEFAULT_PNG,
    web_link::web_link,
    window_tab::CommonData,
};

type PluginInfo = Option<(
    Option<VoltMetadata>,
    VoltInfo,
    Option<VoltIcon>,
    Option<VoltInfo>,
    Option<RwSignal<bool>>,
)>;

#[derive(Clone, PartialEq, Eq)]
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
    pub available: AvailableVoltList,
    pub all: RwSignal<im::HashMap<VoltID, AvailableVoltData>>,
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
                return self
                    .available
                    .query_editor
                    .run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::No
    }

    fn receive_char(&self, c: &str) {
        self.available.query_editor.receive_char(c);
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
        let available = AvailableVoltList {
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
            available,
            all: cx.create_rw_signal(im::HashMap::new()),
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
                    .available
                    .query_editor
                    .view
                    .doc
                    .get()
                    .buffer
                    .with(|buffer| buffer.to_string());
                if s.as_ref() == Some(&query) {
                    return query;
                }
                plugin.available.query_id.update(|id| *id += 1);
                plugin.available.loading.set(false);
                plugin.available.volts.update(|v| v.clear());
                plugin.load_available_volts(&query, 0);
                query
            });
        }

        plugin
    }

    pub fn volt_installed(&self, volt: &VoltMetadata, icon: &Option<Vec<u8>>) {
        let volt_id = volt.id();
        let (existing, is_latest, volt_data) = self
            .installed
            .try_update(|installed| {
                if let Some(v) = installed.get(&volt_id) {
                    (true, true, v.to_owned())
                } else {
                    let (info, is_latest) = if let Some(volt) = self
                        .available
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
                    installed.insert(volt_id, data.clone());

                    (false, is_latest, data)
                }
            })
            .unwrap();

        if existing {
            volt_data.meta.set(volt.clone());
            volt_data.icon.set(
                icon.as_ref()
                    .and_then(|icon| VoltIcon::from_bytes(icon).ok()),
            );
        }

        let latest = volt_data.latest;
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
        if self.available.loading.get_untracked() {
            return;
        }
        self.available.loading.set(true);

        let volts = self.available.volts;
        let volts_total = self.available.total;
        let cx = self.common.scope;
        let loading = self.available.loading;
        let query_id = self.available.query_id;
        let current_query_id = self.available.query_id.get_untracked();
        let all = self.all;
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

                            let data = AvailableVoltData {
                                info: cx.create_rw_signal(volt.clone()),
                                icon,
                                installing: cx.create_rw_signal(false),
                            };
                            all.update(|all| {
                                all.insert(volt.id(), data.clone());
                            });

                            (volt.id(), data)
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

    fn download_readme(
        volt: &VoltInfo,
        config: &LapceConfig,
    ) -> Result<Vec<MarkdownContent>> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins/{}/{}/{}/readme",
            volt.author, volt.name, volt.version
        );
        let resp = reqwest::blocking::get(url)?;
        if resp.status() != 200 {
            let text = parse_markdown("Plugin doesn't have a README", 2.0, config);
            return Ok(text);
        }
        let text = resp.text()?;
        let text = parse_markdown(&text, 2.0, config);
        Ok(text)
    }

    fn query_volts(query: &str, offset: usize) -> Result<VoltsInfo> {
        let url = format!(
            "https://plugins.lapce.dev/api/v1/plugins?q={query}&offset={offset}"
        );
        let plugins: VoltsInfo = reqwest::blocking::get(url)?.json()?;
        Ok(plugins)
    }

    fn all_loaded(&self) -> bool {
        self.available.volts.with_untracked(|v| v.len())
            >= self.available.total.get_untracked()
    }

    pub fn load_more_available(&self) {
        if self.all_loaded() {
            return;
        }

        let query = self
            .available
            .query_editor
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let offset = self.available.volts.with_untracked(|v| v.len());
        self.load_available_volts(&query, offset);
    }

    pub fn install_volt(&self, info: VoltInfo) {
        self.available.volts.with_untracked(|volts| {
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

    pub fn plugin_controls(&self, meta: VoltMetadata, latest: VoltInfo) -> Menu {
        let volt_id = meta.id();
        let mut menu = Menu::new("");
        if meta.version != latest.version {
            menu = menu
                .entry(MenuItem::new("Upgrade Plugin").action({
                    let plugin = self.clone();
                    let info = latest.clone();
                    move || {
                        plugin.install_volt(info.clone());
                    }
                }))
                .separator();
        }
        menu = menu
            .entry(MenuItem::new("Reload Plugin").action({
                let plugin = self.clone();
                let meta = meta.clone();
                move || {
                    plugin.reload_volt(meta.clone());
                }
            }))
            .separator()
            .entry(
                MenuItem::new("Enable")
                    .enabled(
                        self.disabled
                            .with_untracked(|disabled| disabled.contains(&volt_id)),
                    )
                    .action({
                        let plugin = self.clone();
                        let volt = meta.info();
                        move || {
                            plugin.enable_volt(volt.clone());
                        }
                    }),
            )
            .entry(
                MenuItem::new("Disable")
                    .enabled(
                        self.disabled
                            .with_untracked(|disabled| !disabled.contains(&volt_id)),
                    )
                    .action({
                        let plugin = self.clone();
                        let volt = meta.info();
                        move || {
                            plugin.disable_volt(volt.clone());
                        }
                    }),
            )
            .separator()
            .entry(
                MenuItem::new("Enable For Workspace")
                    .enabled(
                        self.workspace_disabled
                            .with_untracked(|disabled| disabled.contains(&volt_id)),
                    )
                    .action({
                        let plugin = self.clone();
                        let volt = meta.info();
                        move || {
                            plugin.enable_volt_for_ws(volt.clone());
                        }
                    }),
            )
            .entry(
                MenuItem::new("Disable For Workspace")
                    .enabled(
                        self.workspace_disabled
                            .with_untracked(|disabled| !disabled.contains(&volt_id)),
                    )
                    .action({
                        let plugin = self.clone();
                        let volt = meta.info();
                        move || {
                            plugin.disable_volt_for_ws(volt.clone());
                        }
                    }),
            )
            .separator()
            .entry(MenuItem::new("Uninstall").action({
                let plugin = self.clone();
                move || {
                    plugin.uninstall_volt(meta.clone());
                }
            }));
        menu
    }
}

pub fn plugin_info_view(plugin: PluginData, volt: VoltID) -> impl View {
    let config = plugin.common.config;
    let header_rect = create_rw_signal(Rect::ZERO);
    let scroll_width: RwSignal<f64> = create_rw_signal(0.0);
    let internal_command = plugin.common.internal_command;
    let local_plugin = plugin.clone();
    let plugin_info = create_memo(move |_| {
        plugin
            .installed
            .with(|volts| {
                volts.get(&volt).map(|v| {
                    (
                        Some(v.meta.get()),
                        v.meta.get().info(),
                        v.icon.get(),
                        Some(v.latest.get()),
                        None,
                    )
                })
            })
            .or_else(|| {
                plugin.all.with(|volts| {
                    volts.get(&volt).map(|v| {
                        (None, v.info.get(), v.icon.get(), None, Some(v.installing))
                    })
                })
            })
    });

    let version_view = move |plugin: PluginData, plugin_info: PluginInfo| {
        let version_info = plugin_info.as_ref().map(|(_, volt, _, latest, _)| {
            (
                volt.version.clone(),
                latest.as_ref().map(|i| i.version.clone()),
            )
        });
        let installing = plugin_info
            .as_ref()
            .and_then(|(_, _, _, _, installing)| *installing);
        let local_version_info = version_info.clone();
        let control = {
            move |version_info: Option<(String, Option<String>)>| match version_info
                .as_ref()
                .map(|(v, l)| match l {
                    Some(l) => (true, l == v),
                    None => (false, false),
                }) {
                Some((true, true)) => "Installed ▼",
                Some((true, false)) => "Upgrade ▼",
                _ => {
                    if installing.map(|i| i.get()).unwrap_or(false) {
                        "Installing"
                    } else {
                        "Install"
                    }
                }
            }
        };
        let local_plugin_info = plugin_info.clone();
        let local_plugin = plugin.clone();
        stack((
            text(
                version_info
                    .as_ref()
                    .map(|(v, _)| format!("v{v}"))
                    .unwrap_or_default(),
            ),
            label(move || control(local_version_info.clone()))
                .style(move |s| {
                    let config = config.get();
                    s.margin_left(10)
                        .padding_horiz(10)
                        .border_radius(6.0)
                        .color(
                            config
                                .color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND),
                        )
                        .background(
                            config
                                .color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND),
                        )
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config
                                    .color(
                                        LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND,
                                    )
                                    .with_alpha_factor(0.8),
                            )
                        })
                        .active(|s| {
                            s.background(
                                config
                                    .color(
                                        LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND,
                                    )
                                    .with_alpha_factor(0.6),
                            )
                        })
                        .disabled(|s| {
                            s.background(config.color(LapceColor::EDITOR_DIM))
                        })
                })
                .disabled(move || installing.map(|i| i.get()).unwrap_or(false))
                .on_click_stop(move |_| {
                    if let Some((meta, info, _, latest, _)) =
                        local_plugin_info.as_ref()
                    {
                        if let Some(meta) = meta {
                            let menu = local_plugin.plugin_controls(
                                meta.to_owned(),
                                latest.clone().unwrap_or_else(|| info.to_owned()),
                            );
                            show_context_menu(menu, None);
                        } else {
                            local_plugin.install_volt(info.to_owned());
                        }
                    }
                }),
        ))
    };

    scroll(
        dyn_container(
            move || plugin_info.get(),
            move |plugin_info| {
                Box::new(
                    stack((
                        stack((
                            match plugin_info
                                .as_ref()
                                .and_then(|(_, _, icon, _, _)| icon.clone())
                            {
                                None => container_box(
                                    img(move || VOLT_DEFAULT_PNG.to_vec())
                                        .style(|s| s.size_full()),
                                ),
                                Some(VoltIcon::Svg(svg_str)) => container_box(
                                    svg(move || svg_str.clone())
                                        .style(|s| s.size_full()),
                                ),
                                Some(VoltIcon::Img(buf)) => container_box(
                                    img(move || buf.clone())
                                        .style(|s| s.size_full()),
                                ),
                            }
                            .style(|s| {
                                s.min_size(150.0, 150.0)
                                    .size(150.0, 150.0)
                                    .padding(20)
                            }),
                            stack((
                                text(
                                    plugin_info
                                        .as_ref()
                                        .map(|(_, volt, _, _, _)| {
                                            volt.display_name.as_str()
                                        })
                                        .unwrap_or(""),
                                )
                                .style(move |s| {
                                    s.font_bold().font_size(
                                        (config.get().ui.font_size() as f32 * 1.6)
                                            .round(),
                                    )
                                }),
                                text(
                                    plugin_info
                                        .as_ref()
                                        .map(|(_, volt, _, _, _)| {
                                            volt.description.as_str()
                                        })
                                        .unwrap_or(""),
                                )
                                .style(move |s| {
                                    let scroll_width = scroll_width.get();
                                    s.max_width(
                                        scroll_width
                                            .max(200.0 + 60.0 * 2.0 + 200.0)
                                            .min(800.0)
                                            - 60.0 * 2.0
                                            - 200.0,
                                    )
                                }),
                                {
                                    let repo = plugin_info
                                        .as_ref()
                                        .and_then(|(_, volt, _, _, _)| {
                                            volt.repository.as_deref()
                                        })
                                        .unwrap_or("")
                                        .to_string();
                                    let local_repo = repo.clone();
                                    stack((
                                        text("Repository: "),
                                        web_link(
                                            move || repo.clone(),
                                            move || local_repo.clone(),
                                            move || {
                                                config
                                                    .get()
                                                    .color(LapceColor::EDITOR_LINK)
                                            },
                                            internal_command,
                                        ),
                                    ))
                                },
                                text(
                                    plugin_info
                                        .as_ref()
                                        .map(|(_, volt, _, _, _)| {
                                            volt.author.as_str()
                                        })
                                        .unwrap_or(""),
                                )
                                .style(move |s| {
                                    s.color(
                                        config.get().color(LapceColor::EDITOR_DIM),
                                    )
                                }),
                                version_view(
                                    local_plugin.clone(),
                                    plugin_info.clone(),
                                ),
                            ))
                            .style(|s| s.flex_col().line_height(1.6)),
                        ))
                        .style(|s| s.absolute())
                        .on_resize(move |rect| {
                            if header_rect.get_untracked() != rect {
                                header_rect.set(rect);
                            }
                        }),
                        empty().style(move |s| {
                            let rect = header_rect.get();
                            s.size(rect.width(), rect.height())
                        }),
                        empty().style(move |s| {
                            s.margin_vert(6).height(1).width_full().background(
                                config.get().color(LapceColor::LAPCE_BORDER),
                            )
                        }),
                        {
                            let readme = create_rw_signal(None);
                            let info = plugin_info
                                .as_ref()
                                .map(|(_, info, _, _, _)| info.to_owned());
                            create_effect(move |_| {
                                let config = config.get();
                                let info = info.clone();
                                if let Some(info) = info {
                                    let cx = Scope::current();
                                    let send =
                                        create_ext_action(cx, move |result| {
                                            if let Ok(md) = result {
                                                readme.set(Some(md));
                                            }
                                        });
                                    std::thread::spawn(move || {
                                        let result = PluginData::download_readme(
                                            &info, &config,
                                        );
                                        send(result);
                                    });
                                }
                            });
                            {
                                let id = AtomicU64::new(0);
                                dyn_stack(
                                    move || {
                                        readme.get().unwrap_or_else(|| {
                                            parse_markdown(
                                                "Loading README",
                                                2.0,
                                                &config.get(),
                                            )
                                        })
                                    },
                                    move |_| {
                                        id.fetch_add(
                                            1,
                                            std::sync::atomic::Ordering::Relaxed,
                                        )
                                    },
                                    move |content| match content {
                                        MarkdownContent::Text(text_layout) => {
                                            container_box(
                                                rich_text(move || {
                                                    text_layout.clone()
                                                })
                                                .style(|s| s.width_full()),
                                            )
                                            .style(|s| s.width_full())
                                        }
                                        MarkdownContent::Image { .. } => {
                                            container_box(empty())
                                        }
                                        MarkdownContent::Separator => {
                                            container_box(empty().style(move |s| {
                                                s.width_full()
                                                    .margin_vert(5.0)
                                                    .height(1.0)
                                                    .background(config.get().color(
                                                        LapceColor::LAPCE_BORDER,
                                                    ))
                                            }))
                                        }
                                    },
                                )
                                .style(|s| s.flex_col().width_full())
                            }
                        },
                    ))
                    .style(move |s| {
                        let padding = 60.0;
                        s.flex_col()
                            .width(
                                scroll_width
                                    .get()
                                    .min(800.0)
                                    .max(header_rect.get().width() + padding * 2.0),
                            )
                            .padding(padding)
                    }),
                )
            },
        )
        .style(|s| s.min_width_full().justify_center()),
    )
    .on_resize(move |rect| {
        if scroll_width.get_untracked() != rect.width() {
            scroll_width.set(rect.width());
        }
    })
    .style(|s| s.absolute().size_full())
}
