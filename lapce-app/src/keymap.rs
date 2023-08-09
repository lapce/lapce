use std::sync::Arc;

use floem::{
    reactive::{create_memo, ReadSignal, Scope},
    style::Style,
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
    keypress::keymap::KeyMap,
    text_input::text_input,
    window_tab::CommonData,
};

pub fn keymap_view(common: CommonData) -> impl View {
    let config = common.config;
    let keypress = common.keypress;
    let ui_line_height = move || common.ui_line_height.get() * 1.2;
    let modal = create_memo(move |_| config.get().core.modal);

    let cx = Scope::current();
    let editor = EditorData::new_local(cx, EditorId::next(), common.clone());
    let doc = editor.view.doc;

    let items = move || {
        let pattern = doc.with(|doc| doc.buffer().to_string().to_lowercase());
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
            stack(|| {
                (
                    container(|| {
                        label(move || {
                            cmd.kind
                                .desc()
                                .map(|desc| desc.to_string())
                                .unwrap_or_else(|| cmd.kind.str().replace('_', " "))
                        })
                        .style(|| {
                            Style::BASE
                                .text_ellipsis()
                                .absolute()
                                .items_center()
                                .min_width_px(0.0)
                                .padding_horiz_px(10.0)
                                .size_pct(100.0, 100.0)
                        })
                    })
                    .style(move || {
                        Style::BASE
                            .height_pct(100.0)
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
                                label(move || key.clone()).style(move || {
                                    Style::BASE
                                        .padding_horiz_px(5.0)
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
                        .style(move || {
                            Style::BASE
                                .items_center()
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
                                label(move || mode.clone()).style(move || {
                                    Style::BASE
                                        .padding_horiz_px(5.0)
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
                        .style(move || {
                            Style::BASE
                                .items_center()
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
                        .style(|| {
                            Style::BASE
                                .text_ellipsis()
                                .absolute()
                                .items_center()
                                .min_width_px(0.0)
                                .padding_horiz_px(10.0)
                                .size_pct(100.0, 100.0)
                        })
                    })
                    .style(move || {
                        Style::BASE
                            .height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                    }),
                )
            })
            .style(move || {
                let config = config.get();
                Style::BASE
                    .items_center()
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
                .style(|| Style::BASE.padding_bottom_px(10.0).width_pct(100.0))
            },
            stack(move || {
                (
                    container(|| {
                        label(|| "Command".to_string()).style(move || {
                            Style::BASE
                                .text_ellipsis()
                                .padding_horiz_px(10.0)
                                .min_width_px(0.0)
                        })
                    })
                    .style(move || {
                        Style::BASE
                            .items_center()
                            .height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                    }),
                    label(|| "Key Binding".to_string()).style(move || {
                        Style::BASE
                            .width_px(200.0)
                            .items_center()
                            .padding_horiz_px(10.0)
                            .height_pct(100.0)
                            .border_right(1.0)
                            .border_color(
                                *config.get().get_color(LapceColor::LAPCE_BORDER),
                            )
                    }),
                    label(|| "Modes".to_string()).style(move || {
                        Style::BASE
                            .width_px(200.0)
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
                        label(|| "When".to_string()).style(move || {
                            Style::BASE
                                .text_ellipsis()
                                .padding_horiz_px(10.0)
                                .min_width_px(0.0)
                        })
                    })
                    .style(move || {
                        Style::BASE
                            .items_center()
                            .height_pct(100.0)
                            .min_width_px(0.0)
                            .flex_basis_px(0.0)
                            .flex_grow(1.0)
                    }),
                )
            })
            .style(move || {
                let config = config.get();
                Style::BASE
                    .font_bold()
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
                        move || items(),
                        |(i, (cmd, keymap)): &(
                            usize,
                            (LapceCommand, Option<KeyMap>),
                        )| {
                            (*i, cmd.kind.str(), keymap.clone())
                        },
                        view_fn,
                    )
                    .style(|| Style::BASE.flex_col().width_pct(100.0))
                })
                .scroll_bar_color(move || {
                    *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
                })
                .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
            })
            .style(|| {
                Style::BASE
                    .width_pct(100.0)
                    .flex_basis_px(0.0)
                    .flex_grow(1.0)
            }),
            keyboard_picker_view(config),
        )
    })
    .style(|| {
        Style::BASE
            .absolute()
            .size_pct(100.0, 100.0)
            .flex_col()
            .padding_top_px(20.0)
            .padding_left_px(20.0)
            .padding_right_px(20.0)
    })
}

fn keyboard_picker_view(config: ReadSignal<Arc<LapceConfig>>) -> impl View {
    container(|| {
        stack(|| (label(|| "label".to_string()),)).style(move || {
            let config = config.get();
            Style::BASE
                .items_center()
                .flex_col()
                .padding_px(20.0)
                .width_px(400.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
        })
    })
    .style(|| {
        Style::BASE
            .absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
    })
}
