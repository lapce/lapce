use std::{path::Path, rc::Rc};

use floem::{
    cosmic_text::Style as FontStyle,
    event::{Event, EventListener},
    peniko::Color,
    reactive::{create_rw_signal, RwSignal},
    style::{AlignItems, CursorStyle, Position, Style},
    view::View,
    views::{
        container, container_box, dyn_stack, label, scroll, stack, svg,
        virtual_stack, Decorators, VirtualDirection, VirtualItemSize,
    },
    EventPropagation,
};
use lapce_core::selection::Selection;
use lapce_rpc::file::{FileNodeViewData, IsRenaming, RenameState};
use lapce_xi_rope::Rope;

use super::{data::FileExplorerData, node::FileNodeVirtualList};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons},
    editor_tab::{EditorTabChild, EditorTabData},
    panel::{kind::PanelKind, position::PanelPosition, view::panel_header},
    plugin::PluginData,
    text_input::text_input,
    window_tab::{Focus, WindowTabData},
};

/// Blends `foreground` with `background`.
///
/// Uses the alpha channel from `foreground` - if `foreground` is opaque, `foreground` will be
/// returned unchanged.
///
/// The result is always opaque regardless of the transparency of the inputs.
fn blend_colors(background: Color, foreground: Color) -> Color {
    let Color {
        r: background_r,
        g: background_g,
        b: background_b,
        ..
    } = background;
    let Color {
        r: foreground_r,
        g: foreground_g,
        b: foreground_b,
        a,
    } = foreground;
    let a: u16 = a.into();

    let [r, g, b] = [
        [background_r, foreground_r],
        [background_g, foreground_g],
        [background_b, foreground_b],
    ]
    .map(|x| x.map(u16::from))
    .map(|[b, f]| (a * f + (255 - a) * b) / 255)
    .map(|x| x as u8);

    Color { r, g, b, a: 255 }
}

pub fn file_explorer_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let data = window_tab_data.file_explorer.clone();
    stack((
        stack((
            panel_header("Open Editors".to_string(), config),
            container(open_editors_view(window_tab_data.clone()))
                .style(|s| s.size_pct(100.0, 100.0)),
        ))
        .style(|s| s.width_pct(100.0).flex_col().height(150.0)),
        stack((
            panel_header("File Explorer".to_string(), config),
            container(
                scroll(new_file_node_view(data))
                    .style(|s| s.absolute().size_pct(100.0, 100.0)),
            )
            .style(|s| s.size_pct(100.0, 100.0).line_height(1.6)),
        ))
        .style(|s| s.width_pct(100.0).height_pct(100.0).flex_col()),
    ))
    .style(move |s| {
        s.width_pct(100.0)
            .apply_if(!position.is_bottom(), |s| s.flex_col())
    })
}

fn initialize_rename_editor(data: &FileExplorerData, path: &Path) {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    // Start with the part of the file or directory name before the extension
    // selected.
    let selection_end = {
        let without_leading_dot = file_name.strip_prefix('.').unwrap_or(&file_name);
        let idx = without_leading_dot
            .find('.')
            .unwrap_or(without_leading_dot.len());

        idx + file_name.len() - without_leading_dot.len()
    };

    let doc = data.rename_editor_data.view.doc.get_untracked();
    doc.reload(Rope::from(&file_name), true);
    data.rename_editor_data
        .cursor
        .update(|cursor| cursor.set_insert(Selection::region(0, selection_end)));

    data.rename_state
        .update(|rename_state| rename_state.set_editor_needs_reset(false));
}

fn file_node_text_view(
    data: FileExplorerData,
    node: FileNodeViewData,
    path: &Path,
) -> impl View {
    let ui_line_height = data.common.ui_line_height;

    let view = if let IsRenaming::Renaming { err } = node.is_renaming {
        let rename_editor_data = data.rename_editor_data.clone();
        let text_input_file_explorer_data = data.clone();
        let focus = data.common.focus;
        let config = data.common.config;

        if data
            .rename_state
            .with_untracked(RenameState::editor_needs_reset)
        {
            initialize_rename_editor(&data, path);
        }

        let text_input_view = text_input(rename_editor_data.clone(), move || {
            focus.with_untracked(|focus| {
                focus == &Focus::Panel(PanelKind::FileExplorer)
            })
        })
        .on_event_stop(EventListener::FocusLost, move |_| {
            data.finish_rename();
            data.rename_state
                .set(lapce_rpc::file::RenameState::NotRenaming);
        })
        .on_event(EventListener::KeyDown, move |event| {
            if let Event::KeyDown(event) = event {
                let keypress = rename_editor_data.common.keypress.get_untracked();
                if keypress.key_down(event, &text_input_file_explorer_data) {
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            } else {
                EventPropagation::Continue
            }
        })
        .style(move |s| {
            s.width_full()
                .height(ui_line_height.get())
                .padding(0.0)
                .margin(0.0)
                .border_radius(6.0)
                .border(1.0)
                .border_color(config.get().color(LapceColor::LAPCE_BORDER))
        });

        let text_input_id = text_input_view.id();
        text_input_id.request_focus();

        if let Some(err) = err {
            container_box(
                stack((
                    text_input_view,
                    label(move || err.clone()).style(move |s| {
                        let config = config.get();

                        let editor_background_color =
                            config.color(LapceColor::PANEL_CURRENT_BACKGROUND);
                        let error_background_color =
                            config.color(LapceColor::ERROR_LENS_ERROR_BACKGROUND);

                        let background_color = blend_colors(
                            editor_background_color,
                            error_background_color,
                        );

                        s.position(Position::Absolute)
                            .inset_top(ui_line_height.get())
                            .width_full()
                            .color(
                                config
                                    .color(LapceColor::ERROR_LENS_ERROR_FOREGROUND),
                            )
                            .background(background_color)
                            .z_index(100)
                    }),
                ))
                .style(|s| s.flex_grow(1.0)),
            )
        } else {
            container_box(text_input_view)
        }
    } else {
        container_box(
            label(move || {
                node.path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .style(move |s| s.flex_grow(1.0).height(ui_line_height.get())),
        )
    };

    view.style(|s| s.flex_grow(1.0).padding(0.0).margin(0.0))
}

fn new_file_node_view(data: FileExplorerData) -> impl View {
    let root = data.root;
    let ui_line_height = data.common.ui_line_height;
    let config = data.common.config;
    virtual_stack(
        VirtualDirection::Vertical,
        VirtualItemSize::Fixed(Box::new(move || ui_line_height.get())),
        move || FileNodeVirtualList::new(root.get(), data.rename_state.get()),
        move |node| {
            (
                node.path.clone(),
                node.is_dir,
                node.open,
                node.is_renaming.clone(),
                node.level,
            )
        },
        move |node| {
            let level = node.level;
            let data = data.clone();
            let click_data = data.clone();
            let double_click_data = data.clone();
            let secondary_click_data = data.clone();
            let aux_click_data = data.clone();
            let path = node.path.clone();
            let click_path = node.path.clone();
            let double_click_path = node.path.clone();
            let secondary_click_path = node.path.clone();
            let aux_click_path = path.clone();
            let open = node.open;
            let is_dir = node.is_dir;
            let is_renaming = node.is_renaming.clone();

            let view = stack((
                svg(move || {
                    let config = config.get();
                    let svg_str = match open {
                        true => LapceIcons::ITEM_OPENED,
                        false => LapceIcons::ITEM_CLOSED,
                    };
                    config.ui_svg(svg_str)
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;

                    let color = if is_dir {
                        config.color(LapceColor::LAPCE_ICON_ACTIVE)
                    } else {
                        Color::TRANSPARENT
                    };
                    s.size(size, size)
                        .flex_shrink(0.0)
                        .margin_left(10.0)
                        .color(color)
                }),
                {
                    let path = path.clone();
                    let path_for_style = path.clone();
                    svg(move || {
                        let config = config.get();
                        if is_dir {
                            let svg_str = match open {
                                true => LapceIcons::DIRECTORY_OPENED,
                                false => LapceIcons::DIRECTORY_CLOSED,
                            };
                            config.ui_svg(svg_str)
                        } else {
                            config.file_svg(&path).0
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;

                        s.size(size, size)
                            .flex_shrink(0.0)
                            .margin_horiz(6.0)
                            .apply_if(is_dir, |s| {
                                s.color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                            })
                            .apply_if(!is_dir, |s| {
                                s.apply_opt(
                                    config.file_svg(&path_for_style).1,
                                    Style::color,
                                )
                            })
                    })
                },
                file_node_text_view(data, node, &path),
            ))
            .style(move |s| {
                s.padding_right(5.0)
                    .padding_left((level * 10) as f32)
                    .align_items(AlignItems::Center)
                    .hover(|s| {
                        s.background(
                            config.get().color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                        .cursor(CursorStyle::Pointer)
                    })
            });

            if let IsRenaming::NotRenaming = is_renaming {
                view.on_click_stop(move |_| {
                    click_data.click(&click_path);
                })
                .on_double_click(move |_| {
                    double_click_data.double_click(&double_click_path)
                })
                .on_secondary_click_stop(move |_| {
                    secondary_click_data.secondary_click(&secondary_click_path);
                })
                .on_event_stop(
                    EventListener::PointerDown,
                    move |event| {
                        if let Event::PointerDown(pointer_event) = event {
                            if pointer_event.button.is_auxiliary() {
                                aux_click_data.middle_click(&aux_click_path);
                            }
                        }
                    },
                )
            } else {
                view
            }
        },
    )
    .style(|s| s.flex_col().align_items(AlignItems::Stretch).width_full())
}

fn open_editors_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let diff_editors = window_tab_data.main_split.diff_editors;
    let editors = window_tab_data.main_split.editors;
    let editor_tabs = window_tab_data.main_split.editor_tabs;
    let config = window_tab_data.common.config;
    let internal_command = window_tab_data.common.internal_command;
    let active_editor_tab = window_tab_data.main_split.active_editor_tab;
    let plugin = window_tab_data.plugin.clone();

    let child_view = move |plugin: PluginData,
                           editor_tab: RwSignal<EditorTabData>,
                           child_index: RwSignal<usize>,
                           child: EditorTabChild| {
        let editor_tab_id =
            editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
        let child_for_close = child.clone();
        let info = child.view_info(editors, diff_editors, plugin, config);
        let hovered = create_rw_signal(false);

        stack((
            clickable_icon(
                move || {
                    if hovered.get() || info.with(|info| info.is_pristine) {
                        LapceIcons::CLOSE
                    } else {
                        LapceIcons::UNSAVED
                    }
                },
                move || {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);
                    internal_command.send(InternalCommand::EditorTabChildClose {
                        editor_tab_id,
                        child: child_for_close.clone(),
                    });
                },
                || false,
                || false,
                config,
            )
            .on_event_stop(EventListener::PointerEnter, move |_| {
                hovered.set(true);
            })
            .on_event_stop(EventListener::PointerLeave, move |_| {
                hovered.set(false);
            })
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(|s| s.margin_left(10.0)),
            container(svg(move || info.with(|info| info.icon.clone())).style(
                move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size)
                        .apply_opt(info.with(|info| info.color), |s, c| s.color(c))
                },
            ))
            .style(|s| s.padding_horiz(6.0)),
            label(move || info.with(|info| info.path.clone())).style(move |s| {
                s.apply_if(
                    !info
                        .with(|info| info.confirmed)
                        .map(|confirmed| confirmed.get())
                        .unwrap_or(true),
                    |s| s.font_style(FontStyle::Italic),
                )
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .width_pct(100.0)
                .apply_if(
                    active_editor_tab.get() == Some(editor_tab_id)
                        && editor_tab.with(|editor_tab| editor_tab.active)
                            == child_index.get(),
                    |s| {
                        s.background(
                            config.color(LapceColor::PANEL_CURRENT_BACKGROUND),
                        )
                    },
                )
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
        .on_event_cont(EventListener::PointerDown, move |_| {
            editor_tab.update(|editor_tab| {
                editor_tab.active = child_index.get_untracked();
            });
            active_editor_tab.set(Some(editor_tab_id));
        })
    };

    scroll(
        dyn_stack(
            move || editor_tabs.get().into_iter().enumerate(),
            move |(index, (editor_tab_id, _))| (*index, *editor_tab_id),
            move |(index, (_, editor_tab))| {
                let plugin = plugin.clone();
                stack((
                    label(move || format!("Group {}", index + 1))
                        .style(|s| s.margin_left(10.0)),
                    dyn_stack(
                        move || editor_tab.get().children,
                        move |(_, _, child)| child.id(),
                        move |(child_index, _, child)| {
                            child_view(
                                plugin.clone(),
                                editor_tab,
                                child_index,
                                child,
                            )
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                ))
                .style(|s| s.flex_col())
            },
        )
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(|s| s.absolute().size_pct(100.0, 100.0).line_height(1.6))
}
