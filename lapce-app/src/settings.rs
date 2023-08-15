use std::sync::Arc;

use floem::{
    event::EventListener,
    peniko::{
        kurbo::{Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, ReadSignal, RwSignal, Scope,
    },
    style::{CursorStyle, Style},
    view::View,
    views::{
        container, container_box, empty, label, list, scroll, stack, svg,
        virtual_list, Decorators, VirtualListDirection, VirtualListVector,
    },
};
use indexmap::IndexMap;
use inflector::Inflector;
use lapce_core::mode::Mode;
use lapce_rpc::plugin::VoltID;
use lapce_xi_rope::Rope;
use serde::Serialize;

use crate::{
    command::CommandExecuted,
    config::{
        color::LapceColor, core::CoreConfig, editor::EditorConfig, icon::LapceIcons,
        terminal::TerminalConfig, ui::UIConfig, DropdownInfo, LapceConfig,
    },
    editor::EditorData,
    id::EditorId,
    keypress::KeyPressFocus,
    plugin::InstalledVoltData,
    text_input::text_input,
    window_tab::CommonData,
};

#[derive(Debug, Clone)]
pub enum SettingsValue {
    Float(f64),
    Integer(i64),
    String(String),
    Bool(bool),
    Dropdown(DropdownInfo),
    Empty,
}

impl From<serde_json::Value> for SettingsValue {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Number(n) => {
                if n.is_f64() {
                    SettingsValue::Float(n.as_f64().unwrap())
                } else {
                    SettingsValue::Integer(n.as_i64().unwrap())
                }
            }
            serde_json::Value::String(s) => SettingsValue::String(s),
            serde_json::Value::Bool(b) => SettingsValue::Bool(b),
            _ => SettingsValue::Empty,
        }
    }
}

#[derive(Clone)]
struct SettingsItem {
    kind: String,
    name: String,
    field: String,
    description: String,
    filter_text: String,
    value: SettingsValue,
    pos: RwSignal<Point>,
    size: RwSignal<Size>,
    // this is only the header that give an visual sepeartion between different type of settings
    header: bool,
}

#[derive(Clone)]
struct SettingsData {
    items: im::Vector<SettingsItem>,
    kinds: im::Vector<(String, RwSignal<Point>)>,
    plugin_items: RwSignal<im::Vector<SettingsItem>>,
    plugin_kinds: RwSignal<im::Vector<(String, RwSignal<Point>)>>,
    filtered_items: RwSignal<im::Vector<SettingsItem>>,
    common: CommonData,
}

impl KeyPressFocus for SettingsData {
    fn get_mode(&self) -> lapce_core::mode::Mode {
        Mode::Insert
    }

    fn check_condition(
        &self,
        _condition: crate::keypress::condition::Condition,
    ) -> bool {
        false
    }

    fn run_command(
        &self,
        _command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        CommandExecuted::No
    }

    fn receive_char(&self, _c: &str) {}
}

impl VirtualListVector<SettingsItem> for SettingsData {
    type ItemIterator = Box<dyn Iterator<Item = SettingsItem>>;

    fn total_len(&self) -> usize {
        self.filtered_items.get_untracked().len()
    }

    fn slice(&mut self, _range: std::ops::Range<usize>) -> Self::ItemIterator {
        Box::new(self.filtered_items.get().into_iter())
    }
}

impl SettingsData {
    pub fn new(
        cx: Scope,
        installed_plugin: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
        common: CommonData,
    ) -> Self {
        fn into_settings_map(
            data: &impl Serialize,
        ) -> serde_json::Map<String, serde_json::Value> {
            match serde_json::to_value(data).unwrap() {
                serde_json::Value::Object(h) => h,
                _ => serde_json::Map::default(),
            }
        }

        let config = common.config.get_untracked();
        let mut items = im::Vector::new();
        let mut kinds = im::Vector::new();
        let mut item_height_accum = 0.0;

        for (kind, fields, descs, mut settings_map) in [
            (
                "Core",
                &CoreConfig::FIELDS[..],
                &CoreConfig::DESCS[..],
                into_settings_map(&config.core),
            ),
            (
                "Editor",
                &EditorConfig::FIELDS[..],
                &EditorConfig::DESCS[..],
                into_settings_map(&config.editor),
            ),
            (
                "UI",
                &UIConfig::FIELDS[..],
                &UIConfig::DESCS[..],
                into_settings_map(&config.ui),
            ),
            (
                "Terminal",
                &TerminalConfig::FIELDS[..],
                &TerminalConfig::DESCS[..],
                into_settings_map(&config.terminal),
            ),
        ] {
            let pos = cx.create_rw_signal(Point::new(0.0, item_height_accum));
            items.push_back(SettingsItem {
                kind: kind.to_string(),
                name: "".to_string(),
                field: "".to_string(),
                filter_text: "".to_string(),
                description: "".to_string(),
                value: SettingsValue::Empty,
                pos,
                size: cx.create_rw_signal(Size::ZERO),
                header: true,
            });
            kinds.push_back((format!("{kind} Settings"), pos));
            for (name, desc) in fields.iter().zip(descs.iter()) {
                let field = name.replace('_', "-");

                let value = if let Some(dropdown) =
                    config.get_dropdown_info(&kind.to_lowercase(), &field)
                {
                    SettingsValue::Dropdown(dropdown)
                } else {
                    let value = settings_map.remove(&field).unwrap();
                    SettingsValue::from(value)
                };

                let name =
                    format!("{kind}: {}", name.replace('_', " ").to_title_case());
                let kind = kind.to_lowercase();
                let filter_text = format!("{kind} {name} {desc}").to_lowercase();
                let filter_text =
                    format!("{filter_text}{}", filter_text.replace(' ', ""));
                items.push_back(SettingsItem {
                    kind,
                    name,
                    field,
                    filter_text,
                    description: desc.to_string(),
                    value,
                    pos: cx.create_rw_signal(Point::ZERO),
                    size: cx.create_rw_signal(Size::ZERO),
                    header: false,
                });
                item_height_accum += 50.0;
            }
        }

        let plugin_items = cx.create_rw_signal(im::Vector::new());
        let plugin_kinds = cx.create_rw_signal(im::Vector::new());

        cx.create_effect(move |_| {
            let mut item_height_accum = item_height_accum;
            let plugins = installed_plugin.get();
            let mut items = im::Vector::new();
            let mut kinds = im::Vector::new();
            for (_, volt) in plugins {
                let meta = volt.meta.get();
                let kind = meta.name;
                let plugin_config = config.plugins.get(&kind);
                if let Some(config) = meta.config {
                    let pos =
                        cx.create_rw_signal(Point::new(0.0, item_height_accum));
                    items.push_back(SettingsItem {
                        kind: meta.display_name.clone(),
                        name: "".to_string(),
                        field: "".to_string(),
                        filter_text: "".to_string(),
                        description: "".to_string(),
                        value: SettingsValue::Empty,
                        pos,
                        size: cx.create_rw_signal(Size::ZERO),
                        header: true,
                    });
                    kinds.push_back((meta.display_name.clone(), pos));

                    {
                        let mut local_items = Vec::new();
                        for (name, config) in config {
                            let field = name.clone();

                            let name = format!(
                                "{}: {}",
                                meta.display_name,
                                name.replace('_', " ").to_title_case()
                            );
                            let desc = config.description;
                            let filter_text =
                                format!("{kind} {name} {desc}").to_lowercase();
                            let filter_text = format!(
                                "{filter_text}{}",
                                filter_text.replace(' ', "")
                            );

                            let value = plugin_config
                                .and_then(|config| config.get(&field).cloned())
                                .unwrap_or(config.default);
                            let value = SettingsValue::from(value);

                            let item = SettingsItem {
                                kind: kind.clone(),
                                name,
                                field,
                                filter_text,
                                description: desc.to_string(),
                                value,
                                pos: cx.create_rw_signal(Point::ZERO),
                                size: cx.create_rw_signal(Size::ZERO),
                                header: false,
                            };
                            local_items.push(item);
                            item_height_accum += 50.0;
                        }
                        local_items.sort_by_key(|i| i.name.clone());
                        items.extend(local_items.into_iter());
                    }
                }
            }
            plugin_items.set(items);
            plugin_kinds.set(kinds);
        });

        Self {
            filtered_items: cx.create_rw_signal(items.clone()),
            plugin_items,
            plugin_kinds,
            items,
            kinds,
            common,
        }
    }
}

pub fn settings_view(
    installed_plugins: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
    common: CommonData,
) -> impl View {
    let config = common.config;

    let cx = Scope::current();
    let settings_data = SettingsData::new(cx, installed_plugins, common.clone());
    let view_settings_data = settings_data.clone();
    let plugin_kinds = settings_data.plugin_kinds;

    let search_editor = EditorData::new_local(cx, EditorId::next(), common);
    let doc = search_editor.view.doc;

    let items = settings_data.items.clone();
    let kinds = settings_data.kinds.clone();
    let filtered_items_signal = settings_data.filtered_items;
    create_effect(move |_| {
        let pattern = doc.with(|doc| doc.buffer().to_string().to_lowercase());
        let plugin_items = settings_data.plugin_items.get();

        if pattern.is_empty() {
            let mut items = items.clone();
            items.extend(plugin_items.into_iter());
            filtered_items_signal.set(items);
            return;
        }

        let mut filtered_items = im::Vector::new();
        for item in &items {
            if item.header || item.filter_text.contains(&pattern) {
                filtered_items.push_back(item.clone());
            }
        }
        for item in plugin_items {
            if item.header || item.filter_text.contains(&pattern) {
                filtered_items.push_back(item);
            }
        }
        filtered_items_signal.set(filtered_items);
    });

    let ensure_visible = create_rw_signal(Rect::ZERO);
    let settings_content_size = create_rw_signal(Size::ZERO);
    let scroll_pos = create_rw_signal(Point::ZERO);

    let current_kind = {
        let kinds = kinds.clone();
        create_memo(move |_| {
            let scroll_pos = scroll_pos.get();
            let scroll_y = scroll_pos.y + 30.0;

            let plugin_kinds = plugin_kinds.get_untracked();
            for (kind, pos) in plugin_kinds.iter().rev() {
                if pos.get_untracked().y < scroll_y {
                    return kind.to_string();
                }
            }

            for (kind, pos) in kinds.iter().rev() {
                if pos.get_untracked().y < scroll_y {
                    return kind.to_string();
                }
            }

            kinds.get(0).unwrap().0.to_string()
        })
    };

    let switcher_item = move |k: String,
                              pos: Box<dyn Fn() -> Option<RwSignal<Point>>>,
                              margin: f32| {
        let kind = k.clone();
        container(|| {
            label(move || k.clone())
                .style(move || Style::BASE.text_ellipsis().padding_left_px(margin))
        })
        .on_click(move |_| {
            if let Some(pos) = pos() {
                ensure_visible.set(
                    settings_content_size
                        .get_untracked()
                        .to_rect()
                        .with_origin(pos.get_untracked()),
                );
            }
            true
        })
        .style(move || {
            Style::BASE
                .padding_horiz_px(20.0)
                .width_pct(100.0)
                .apply_if(kind == current_kind.get(), |s| {
                    s.background(
                        *config
                            .get()
                            .get_color(LapceColor::PANEL_CURRENT_BACKGROUND),
                    )
                })
        })
        .hover_style(move || {
            Style::BASE.cursor(CursorStyle::Pointer).background(
                *config.get().get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
            )
        })
        .active_style(move || {
            Style::BASE.background(
                *config
                    .get()
                    .get_color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
            )
        })
    };

    let switcher = || {
        stack(move || {
            (
                list(
                    move || kinds.clone(),
                    |(k, _)| k.clone(),
                    move |(k, pos)| {
                        switcher_item(k, Box::new(move || Some(pos)), 0.0)
                    },
                )
                .style(|| Style::BASE.flex_col().width_pct(100.0)),
                stack(|| {
                    (
                        switcher_item(
                            "Plugin Settings".to_string(),
                            Box::new(move || {
                                plugin_kinds.with_untracked(|k| {
                                    k.get(0).map(|(_, pos)| *pos)
                                })
                            }),
                            0.0,
                        ),
                        list(
                            move || plugin_kinds.get(),
                            |(k, _)| k.clone(),
                            move |(k, pos)| {
                                switcher_item(k, Box::new(move || Some(pos)), 10.0)
                            },
                        )
                        .style(|| Style::BASE.flex_col().width_pct(100.0)),
                    )
                })
                .style(move || {
                    Style::BASE
                        .width_pct(100.0)
                        .flex_col()
                        .apply_if(plugin_kinds.with(|k| k.is_empty()), |s| s.hide())
                }),
            )
        })
        .style(move || {
            Style::BASE
                .width_pct(100.0)
                .flex_col()
                .line_height(1.6)
                .font_size(config.get().ui.font_size() as f32 + 1.0)
        })
    };

    stack(move || {
        (
            container(|| {
                scroll(|| {
                    container(switcher)
                        .style(|| Style::BASE.padding_vert_px(20.0).width_pct(100.0))
                })
                .scroll_bar_color(move || {
                    *config.get().get_color(LapceColor::LAPCE_BORDER)
                })
                .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
            })
            .style(move || {
                Style::BASE
                    .height_pct(100.0)
                    .width_px(200.0)
                    .border_right(1.0)
                    .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
            }),
            stack(move || {
                (
                    container(|| {
                        text_input(search_editor, || false)
                            .placeholder(|| "Search Settings".to_string())
                            .keyboard_navigatable()
                            .style(move || {
                                Style::BASE
                                    .width_pct(100.0)
                                    .border_radius(6.0)
                                    .border(1.0)
                                    .border_color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::LAPCE_BORDER),
                                    )
                            })
                    })
                    .style(|| {
                        Style::BASE.padding_horiz_px(50.0).padding_vert_px(20.0)
                    }),
                    container(|| {
                        scroll(|| {
                            virtual_list(
                                VirtualListDirection::Vertical,
                                floem::views::VirtualListItemSize::Fn(Box::new(
                                    |item: &SettingsItem| {
                                        item.size.get().height.max(50.0)
                                    },
                                )),
                                move || settings_data.clone(),
                                |item| (item.kind.clone(), item.name.clone()),
                                move |item| {
                                    settings_item_view(
                                        view_settings_data.clone(),
                                        item,
                                    )
                                },
                            )
                            .style(|| {
                                Style::BASE
                                    .flex_col()
                                    .padding_horiz_px(50.0)
                                    .min_width_pct(100.0)
                                    .max_width_px(400.0)
                            })
                        })
                        .on_scroll(move |rect| {
                            scroll_pos.set(rect.origin());
                        })
                        .on_ensure_visible(move || ensure_visible.get())
                        .on_resize(move |_, rect| {
                            settings_content_size.set(rect.size());
                        })
                        .scroll_bar_color(move || {
                            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
                        })
                        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
                    })
                    .style(|| Style::BASE.size_pct(100.0, 100.0)),
                )
            })
            .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0)),
        )
    })
    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
}

fn settings_item_view(settings_data: SettingsData, item: SettingsItem) -> impl View {
    let config = settings_data.common.config;

    let is_ticked = if let SettingsValue::Bool(is_ticked) = &item.value {
        Some(*is_ticked)
    } else {
        None
    };

    let editor_value = match &item.value {
        SettingsValue::Float(n) => Some(n.to_string()),
        SettingsValue::Integer(n) => Some(n.to_string()),
        SettingsValue::String(s) => Some(s.to_string()),
        SettingsValue::Bool(_) => None,
        SettingsValue::Dropdown(_) => None,
        SettingsValue::Empty => None,
    };

    let view = {
        let item = item.clone();
        move || {
            let cx = Scope::current();
            if let Some(editor_value) = editor_value {
                let editor = EditorData::new_local(
                    cx,
                    EditorId::next(),
                    settings_data.common,
                );
                let doc = editor.view.doc;
                doc.update(|doc| doc.reload(Rope::from(editor_value), true));

                let kind = item.kind.clone();
                let field = item.field.clone();
                create_effect(move |last| {
                    let rev = doc.with(|doc| doc.buffer().rev());
                    if last.is_none() {
                        return rev;
                    }
                    if last == Some(rev) {
                        return rev;
                    }
                    let value = doc.with_untracked(|doc| doc.buffer().to_string());

                    if let Ok(value) = serde::Serialize::serialize(
                        &value,
                        toml_edit::ser::ValueSerializer::new(),
                    ) {
                        LapceConfig::update_file(&kind, &field, value);
                    }

                    rev
                });

                container_box(move || {
                    Box::new(
                        text_input(editor, || false).keyboard_navigatable().style(
                            move || {
                                Style::BASE
                                    .width_px(300.0)
                                    .border(1.0)
                                    .border_radius(6.0)
                                    .border_color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::LAPCE_BORDER),
                                    )
                            },
                        ),
                    )
                })
            } else if let SettingsValue::Dropdown(dropdown) = item.value {
                let expanded = create_rw_signal(false);
                let current_value = dropdown
                    .items
                    .get(dropdown.active_index)
                    .or_else(|| dropdown.items.last())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let current_value = create_rw_signal(current_value);

                let kind = item.kind.clone();
                let field = item.field.clone();
                let view_fn = move |item_string: String| {
                    let kind = kind.clone();
                    let field = field.clone();
                    let local_item_string = item_string.clone();
                    label(move || local_item_string.clone())
                        .on_click(move |_| {
                            current_value.set(item_string.clone());
                            if let Ok(value) = serde::Serialize::serialize(
                                &item_string,
                                toml_edit::ser::ValueSerializer::new(),
                            ) {
                                LapceConfig::update_file(&kind, &field, value);
                            }
                            expanded.set(false);
                            true
                        })
                        .style(|| Style::BASE.text_ellipsis().padding_horiz_px(10.0))
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                };
                container_box(move || {
                    Box::new(
                        stack(move || {
                            (
                                stack(|| {
                                    (
                                        label(move || current_value.get()).style(
                                            move || {
                                                Style::BASE
                                                    .text_ellipsis()
                                                    .width_pct(100.0)
                                                    .padding_horiz_px(10.0)
                                            },
                                        ),
                                        container(|| {
                                            svg(move || {
                                                config.get().ui_svg(
                                                    LapceIcons::DROPDOWN_ARROW,
                                                )
                                            })
                                            .style(move || {
                                                let config = config.get();
                                                let size =
                                                    config.ui.icon_size() as f32;
                                                Style::BASE
                                                    .size_px(size, size)
                                                    .color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                            })
                                        })
                                        .style(|| Style::BASE.padding_right_px(4.0)),
                                    )
                                })
                                .on_click(move |_| {
                                    expanded.update(|expanded| {
                                        *expanded = !*expanded;
                                    });
                                    true
                                })
                                .style(move || {
                                    Style::BASE
                                        .items_center()
                                        .width_pct(100.0)
                                        .cursor(CursorStyle::Pointer)
                                        .border_color(Color::TRANSPARENT)
                                        .border(1.0)
                                        .border_radius(6.0)
                                        .apply_if(!expanded.get(), |s| {
                                            s.border_color(
                                                *config.get().get_color(
                                                    LapceColor::LAPCE_BORDER,
                                                ),
                                            )
                                        })
                                }),
                                stack(move || {
                                    (
                                        label(|| " ".to_string()),
                                        scroll(|| {
                                            list(
                                                move || dropdown.items.clone(),
                                                |item| item.to_string(),
                                                view_fn,
                                            )
                                            .style(|| {
                                                Style::BASE
                                                    .flex_col()
                                                    .width_pct(100.0)
                                                    .cursor(CursorStyle::Pointer)
                                            })
                                        })
                                        .scroll_bar_color(move || {
                                            *config.get().get_color(
                                                LapceColor::LAPCE_SCROLL_BAR,
                                            )
                                        })
                                        .style(move || {
                                            let config = config.get();
                                            Style::BASE
                                                .background(*config.get_color(
                                                    LapceColor::EDITOR_BACKGROUND,
                                                ))
                                                .width_pct(100.0)
                                                .max_height_px(300.0)
                                                .z_index(1)
                                                .border_top(1.0)
                                                .border_radius(6.0)
                                                .border_color(*config.get_color(
                                                    LapceColor::LAPCE_BORDER,
                                                ))
                                                .apply_if(!expanded.get(), |s| {
                                                    s.hide()
                                                })
                                        }),
                                    )
                                })
                                .keyboard_navigatable()
                                .on_event(EventListener::FocusLost, move |_| {
                                    if expanded.get_untracked() {
                                        expanded.set(false);
                                    }
                                    true
                                })
                                .style(move || {
                                    Style::BASE
                                        .absolute()
                                        .flex_col()
                                        .width_pct(100.0)
                                        .border(1.0)
                                        .border_radius(6.0)
                                        .border_color(
                                            *config
                                                .get()
                                                .get_color(LapceColor::LAPCE_BORDER),
                                        )
                                        .apply_if(!expanded.get(), |s| {
                                            s.border_color(Color::TRANSPARENT)
                                        })
                                }),
                            )
                        })
                        .style(move || Style::BASE.width_px(250.0).line_height(1.8)),
                    )
                })
            } else if item.header {
                container_box(|| {
                    Box::new(label(move || item.kind.clone()).style(move || {
                        let config = config.get();
                        Style::BASE
                            .line_height(2.0)
                            .font_bold()
                            .width_pct(100.0)
                            .padding_horiz_px(10.0)
                            .font_size(config.ui.font_size() as f32 + 2.0)
                            .background(
                                *config.get_color(LapceColor::PANEL_BACKGROUND),
                            )
                    }))
                })
            } else {
                container_box(|| Box::new(empty()))
            }
        }
    };

    stack(move || {
        (
            label(move || item.name.clone()).style(move || {
                Style::BASE
                    .font_bold()
                    .text_ellipsis()
                    .min_width_px(0.0)
                    .max_width_pct(100.0)
                    .line_height(1.6)
                    .font_size(config.get().ui.font_size() as f32 + 1.0)
            }),
            stack(move || {
                (
                    label(move || item.description.clone()).style(move || {
                        Style::BASE
                            .min_width_px(0.0)
                            .max_width_pct(100.0)
                            .line_height(1.6)
                            .apply_if(is_ticked.is_some(), |s| {
                                s.margin_left_px(
                                    config.get().ui.font_size() as f32 + 8.0,
                                )
                            })
                            .apply_if(item.header, |s| s.hide())
                    }),
                    if let Some(is_ticked) = is_ticked {
                        let checked = create_rw_signal(is_ticked);

                        let kind = item.kind.clone();
                        let field = item.field.clone();
                        create_effect(move |last| {
                            let checked = checked.get();
                            if last.is_none() {
                                return;
                            }
                            if let Ok(value) = serde::Serialize::serialize(
                                &checked,
                                toml_edit::ser::ValueSerializer::new(),
                            ) {
                                LapceConfig::update_file(&kind, &field, value);
                            }
                        });

                        container_box(|| {
                            Box::new(
                                stack(|| {
                                    (
                                        checkbox(move || checked.get(), config),
                                        label(|| " ".to_string())
                                            .style(|| Style::BASE.line_height(1.6)),
                                    )
                                })
                                .style(|| Style::BASE.items_center()),
                            )
                        })
                        .on_click(move |_| {
                            checked.update(|checked| {
                                *checked = !*checked;
                            });
                            true
                        })
                        .style(|| {
                            Style::BASE
                                .absolute()
                                .cursor(CursorStyle::Pointer)
                                .size_pct(100.0, 100.0)
                                .items_start()
                        })
                    } else {
                        container_box(|| Box::new(empty()))
                            .style(|| Style::BASE.hide())
                    },
                )
            }),
            view().style(move || {
                Style::BASE.apply_if(!item.header, |s| s.margin_top_px(6.0))
            }),
        )
    })
    .on_resize(move |_, rect| {
        if item.header {
            item.pos.set(rect.origin());
        }
        let old_size = item.size.get_untracked();
        let new_size = rect.size();
        if old_size != new_size {
            item.size.set(new_size);
        }
    })
    .style(|| {
        Style::BASE
            .flex_col()
            .padding_vert_px(10.0)
            .min_width_pct(100.0)
            .max_width_px(300.0)
    })
}

pub fn checkbox(
    checked: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    const CHECKBOX_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 16 16"><polygon points="5.19,11.83 0.18,7.44 1.82,5.56 4.81,8.17 10,1.25 12,2.75" /></svg>"#;
    let svg_str = move || if checked() { CHECKBOX_SVG } else { "" }.to_string();

    svg(svg_str).base_style(move || {
        let config = config.get();
        let size = config.ui.font_size() as f32;
        let color = *config.get_color(LapceColor::EDITOR_FOREGROUND);

        Style::BASE
            .min_width_px(size)
            .size_px(size, size)
            .color(color)
            .border_color(color)
            .border(1.)
            .border_radius(4.)
    })
}
