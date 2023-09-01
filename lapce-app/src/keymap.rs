use std::{rc::Rc, sync::Arc};

use floem::{
    event::{Event, EventListener},
    reactive::{
        create_effect, create_memo, create_rw_signal, Memo, ReadSignal, RwSignal,
        Scope,
    },
    style::CursorStyle,
    view::View,
    views::{
        container, label, list, scroll, stack, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize,
    },
};
use lapce_core::mode::Modes;

use crate::{
    command::LapceCommand,
    config::{color::LapceColor, LapceConfig},
    editor::EditorData,
    id::EditorId,
    keypress::{keymap::KeyMap, KeyPress, KeyPressData},
    text_input::text_input,
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct KeymapPicker {
    cmd: RwSignal<Option<LapceCommand>>,
    keymap: RwSignal<Option<KeyMap>>,
    keys: RwSignal<Vec<KeyPress>>,
}

pub fn keymap_view(common: Rc<CommonData>) -> impl View {
    let config = common.config;
    let keypress = common.keypress;
    let ui_line_height_memo = common.ui_line_height;
    let ui_line_height = move || ui_line_height_memo.get() * 1.2;
    let modal = create_memo(move |_| config.get().core.modal);
    let picker = KeymapPicker {
        cmd: create_rw_signal(None),
        keymap: create_rw_signal(None),
        keys: create_rw_signal(Vec::new()),
    };

    let cx = Scope::current();
    let editor = EditorData::new_local(cx, EditorId::next(), common.clone());
    let doc = editor.view.doc;

    let items = move || {
        let doc = doc.get();
        let pattern = doc.buffer.with(|b| b.to_string().to_lowercase());
        let keypress = keypress.get();
        let mut items = keypress
            .commands_with_keymap
            .iter()
            .filter_map(|keymap| {
                let cmd = keypress.commands.get(&keymap.command).cloned()?;
                let match_pattern =
                    cmd.kind.str().replace('_', " ").contains(&pattern)
                        || cmd
                            .kind
                            .desc()
                            .map(|desc| desc.to_lowercase().contains(&pattern))
                            .unwrap_or(false);
                if !match_pattern {
                    return None;
                }
                Some((cmd, Some(keymap.clone())))
            })
            .collect::<im::Vector<(LapceCommand, Option<KeyMap>)>>();
        items.extend(keypress.commands_without_keymap.iter().filter_map(|cmd| {
            let match_pattern = cmd.kind.str().replace('_', " ").contains(&pattern)
                || cmd
                    .kind
                    .desc()
                    .map(|desc| desc.to_lowercase().contains(&pattern))
                    .unwrap_or(false);
            if !match_pattern {
                return None;
            }
            Some((cmd.clone(), None))
        }));
        items
            .into_iter()
            .enumerate()
            .collect::<im::Vector<(usize, (LapceCommand, Option<KeyMap>))>>()
    };

    let view_fn =
        move |(i, (cmd, keymap)): (usize, (LapceCommand, Option<KeyMap>))| {
            let local_keymap = keymap.clone();
            let local_cmd = cmd.clone();
            stack(|| {
                (
                    container(|| {
                        label(move || {
                            cmd.kind
                                .desc()
                                .map(|desc| desc.to_string())
                                .unwrap_or_else(|| cmd.kind.str().replace('_', " "))
                        })
                        .style(|s| {
                            s.text_ellipsis()
                                .absolute()
                                .items_center()
                                .min_width_px(0.0)
                                .padding_horiz_px(10.0)
                                .size_pct(100.0, 100.0)
                        })
                    })
                    .style(move |s| {
                        s.height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                    }),
                    {
                        let keymap = keymap.clone();
                        list(
                            move || {
                                keymap
                                    .as_ref()
                                    .map(|keymap| {
                                        keymap
                                            .key
                                            .iter()
                                            .map(|key| {
                                                key.label().trim().to_string()
                                            })
                                            .filter(|l| !l.is_empty())
                                            .collect::<Vec<String>>()
                                    })
                                    .unwrap_or_default()
                            },
                            |k| k.clone(),
                            move |key| {
                                label(move || key.clone()).style(move |s| {
                                    s.padding_horiz_px(5.0)
                                        .padding_vert_px(1.0)
                                        .margin_right_px(5.0)
                                        .border(1.0)
                                        .border_radius(3.0)
                                        .border_color(
                                            *config
                                                .get()
                                                .get_color(LapceColor::LAPCE_BORDER),
                                        )
                                })
                            },
                        )
                        .style(move |s| {
                            s.items_center()
                                .padding_horiz_px(10.0)
                                .min_width_px(200.0)
                                .height_pct(100.0)
                                .border_right(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                    },
                    {
                        let keymap = keymap.clone();
                        let bits = [
                            (Modes::INSERT, "Insert"),
                            (Modes::NORMAL, "Normal"),
                            (Modes::VISUAL, "Visual"),
                            (Modes::TERMINAL, "Terminal"),
                        ];
                        let modes = keymap
                            .as_ref()
                            .map(|keymap| {
                                bits.iter()
                                    .filter_map(|(bit, mode)| {
                                        if keymap.modes.contains(*bit) {
                                            Some(mode.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<String>>()
                            })
                            .unwrap_or_default();
                        list(
                            move || modes.clone(),
                            |m| m.clone(),
                            move |mode| {
                                label(move || mode.clone()).style(move |s| {
                                    s.padding_horiz_px(5.0)
                                        .padding_vert_px(1.0)
                                        .margin_right_px(5.0)
                                        .border(1.0)
                                        .border_radius(3.0)
                                        .border_color(
                                            *config
                                                .get()
                                                .get_color(LapceColor::LAPCE_BORDER),
                                        )
                                })
                            },
                        )
                        .style(move |s| {
                            s.items_center()
                                .padding_horiz_px(10.0)
                                .min_width_px(200.0)
                                .height_pct(100.0)
                                .border_right(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                                .apply_if(!modal.get(), |s| s.hide())
                        })
                    },
                    container(|| {
                        label(move || {
                            keymap
                                .as_ref()
                                .and_then(|keymap| keymap.when.clone())
                                .unwrap_or_default()
                        })
                        .style(|s| {
                            s.text_ellipsis()
                                .absolute()
                                .items_center()
                                .min_width_px(0.0)
                                .padding_horiz_px(10.0)
                                .size_pct(100.0, 100.0)
                        })
                    })
                    .style(move |s| {
                        s.height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                    }),
                )
            })
            .on_click(move |_| {
                let keymap = if let Some(keymap) = local_keymap.clone() {
                    keymap
                } else {
                    KeyMap {
                        command: local_cmd.kind.str().to_string(),
                        key: Vec::new(),
                        modes: Modes::empty(),
                        when: None,
                    }
                };
                picker.keymap.set(Some(keymap));
                picker.cmd.set(Some(local_cmd.clone()));
                picker.keys.update(|keys| {
                    keys.clear();
                });
                true
            })
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .height_px(ui_line_height() as f32)
                    .width_pct(100.0)
                    .apply_if(i % 2 > 0, |s| {
                        s.background(
                            *config.get_color(LapceColor::EDITOR_CURRENT_LINE),
                        )
                    })
                    .border_bottom(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            })
        };

    stack(|| {
        (
            {
                container(|| {
                    text_input(editor, || false)
                        .placeholder(|| "Search Key Bindings".to_string())
                        .keyboard_navigatable()
                        .style(move |s| {
                            s.width_pct(100.0)
                                .border_radius(6.0)
                                .border(1.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                })
                .style(|s| s.padding_bottom_px(10.0).width_pct(100.0))
            },
            stack(move || {
                (
                    container(|| {
                        label(|| "Command".to_string()).style(move |s| {
                            s.text_ellipsis()
                                .padding_horiz_px(10.0)
                                .min_width_px(0.0)
                        })
                    })
                    .style(move |s| {
                        s.items_center()
                            .height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                    }),
                    label(|| "Key Binding".to_string()).style(move |s| {
                        s.width_px(200.0)
                            .items_center()
                            .padding_horiz_px(10.0)
                            .height_pct(100.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                    }),
                    label(|| "Modes".to_string()).style(move |s| {
                        s.width_px(200.0)
                            .items_center()
                            .padding_horiz_px(10.0)
                            .height_pct(100.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                            .apply_if(!modal.get(), |s| s.hide())
                    }),
                    container(|| {
                        label(|| "When".to_string()).style(move |s| {
                            s.text_ellipsis()
                                .padding_horiz_px(10.0)
                                .min_width_px(0.0)
                        })
                    })
                    .style(move |s| {
                        s.items_center()
                            .height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                    }),
                )
            })
            .style(move |s| {
                let config = config.get();
                s.font_bold()
                    .height_px(ui_line_height() as f32)
                    .width_pct(100.0)
                    .border_top(1.0)
                    .border_bottom(1.0)
                    .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                    .background(*config.get_color(LapceColor::EDITOR_CURRENT_LINE))
            }),
            container(|| {
                scroll(|| {
                    virtual_list(
                        VirtualListDirection::Vertical,
                        VirtualListItemSize::Fixed(Box::new(ui_line_height)),
                        items,
                        |(i, (cmd, keymap)): &(
                            usize,
                            (LapceCommand, Option<KeyMap>),
                        )| {
                            (*i, cmd.kind.str(), keymap.clone())
                        },
                        view_fn,
                    )
                    .style(|s| s.flex_col().width_pct(100.0))
                })
                .style(|s| s.absolute().size_pct(100.0, 100.0))
            })
            .style(|s| s.width_pct(100.0).flex_basis_px(0.0).flex_grow(1.0)),
            keyboard_picker_view(picker, common.ui_line_height, config),
        )
    })
    .style(|s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .flex_col()
            .padding_top_px(20.0)
            .padding_left_px(20.0)
            .padding_right_px(20.0)
    })
}

fn keyboard_picker_view(
    picker: KeymapPicker,
    ui_line_height: Memo<f64>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let picker_cmd = picker.cmd;
    let view = container(|| {
        stack(|| {
            (
                label(move || {
                    picker_cmd.with(|cmd| {
                        cmd.as_ref()
                            .map(|cmd| {
                                cmd.kind
                                    .desc()
                                    .map(|desc| desc.to_string())
                                    .unwrap_or_else(|| {
                                        cmd.kind.str().replace('_', " ")
                                    })
                            })
                            .unwrap_or_default()
                    })
                }),
                list(
                    move || {
                        picker
                            .keys
                            .get()
                            .iter()
                            .map(|key| key.label().trim().to_string())
                            .filter(|l| !l.is_empty())
                            .enumerate()
                            .collect::<Vec<(usize, String)>>()
                    },
                    |(i, k)| (*i, k.clone()),
                    move |(_, key)| {
                        label(move || key.clone()).style(move |s| {
                            s.padding_horiz_px(5.0)
                                .padding_vert_px(1.0)
                                .margin_right_px(5.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    *config
                                        .get()
                                        .get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                    },
                )
                .style(move |s| {
                    let config = config.get();
                    s.items_center()
                        .justify_center()
                        .width_pct(100.0)
                        .margin_top_px(20.0)
                        .height_px(ui_line_height.get() as f32 + 16.0)
                        .border(1.0)
                        .border_radius(6.0)
                        .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                        .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
                }),
                stack(|| {
                    (
                        label(|| "Save".to_string())
                            .style(move |s| {
                                s.width_px(100.0)
                                    .justify_center()
                                    .padding_vert_px(8.0)
                                    .border(1.0)
                                    .border_radius(6.0)
                                    .border_color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::LAPCE_BORDER),
                                    )
                            })
                            .on_click(move |_| {
                                let keymap = picker.keymap.get_untracked();
                                if let Some(keymap) = keymap {
                                    let keys = picker.keys.get_untracked();
                                    picker.keymap.set(None);
                                    KeyPressData::update_file(&keymap, &keys);
                                }
                                true
                            })
                            .hover_style(move |s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    *config.get().get_color(
                                        LapceColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                            .active_style(move |s| {
                                s.background(*config.get().get_color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            }),
                        label(|| "Cancel".to_string())
                            .style(move |s| {
                                s.margin_left_px(20.0)
                                    .width_px(100.0)
                                    .justify_center()
                                    .padding_vert_px(8.0)
                                    .border(1.0)
                                    .border_radius(6.0)
                                    .border_color(
                                        *config
                                            .get()
                                            .get_color(LapceColor::LAPCE_BORDER),
                                    )
                            })
                            .on_click(move |_| {
                                picker.keymap.set(None);
                                true
                            })
                            .hover_style(move |s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    *config.get().get_color(
                                        LapceColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                            .active_style(move |s| {
                                s.background(*config.get().get_color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            }),
                    )
                })
                .style(move |s| {
                    let config = config.get();
                    s.items_center()
                        .justify_center()
                        .width_pct(100.0)
                        .margin_top_px(20.0)
                        .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                }),
            )
        })
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .flex_col()
                .padding_px(20.0)
                .width_px(400.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
        })
    })
    .keyboard_navigatable()
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            if let Some(keypress) = KeyPressData::keypress(key_event) {
                picker.keys.update(|keys| {
                    if keys.len() == 2 {
                        keys.clear();
                    }
                    keys.push(keypress);
                })
            }
        }
        true
    })
    .style(move |s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(picker.keymap.with(|keymap| keymap.is_none()), |s| s.hide())
    });

    let id = view.id();
    create_effect(move |_| {
        if picker.keymap.with(|k| k.is_some()) {
            id.request_focus();
        }
    });

    view
}
