use std::rc::Rc;

use floem::{
    cosmic_text::Style as FontStyle,
    event::{Event, EventListener},
    peniko::Color,
    reactive::{create_rw_signal, RwSignal},
    style::{CursorStyle, Style},
    view::View,
    views::{
        container, label, list, scroll, stack, svg, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize,
    },
    EventPropagation,
};

use super::{data::FileExplorerData, node::FileNodeVirtualList};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons},
    editor_tab::{EditorTabChild, EditorTabData},
    panel::{position::PanelPosition, view::panel_header},
    plugin::PluginData,
    window_tab::WindowTabData,
};

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

fn new_file_node_view(data: FileExplorerData) -> impl View {
    let root = data.root;
    let ui_line_height = data.common.ui_line_height;
    let config = data.common.config;
    virtual_list(
        VirtualListDirection::Vertical,
        VirtualListItemSize::Fixed(Box::new(move || ui_line_height.get())),
        move || FileNodeVirtualList(root.get()),
        move |node| (node.path.clone(), node.is_dir, node.open, node.level),
        move |node| {
            let level = node.level;
            let data = data.clone();
            let double_click_data = data.clone();
            let aux_click_data = data.clone();
            let path = node.path.clone();
            let click_path = node.path.clone();
            let double_click_path = node.path.clone();
            let aux_click_path = path.clone();
            let open = node.open;
            let is_dir = node.is_dir;
            stack((
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
                        *config.get_color(LapceColor::LAPCE_ICON_ACTIVE)
                    } else {
                        Color::TRANSPARENT
                    };
                    s.size(size, size).margin_left(10.0).color(color)
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
                            .margin_horiz(6.0)
                            .apply_if(is_dir, |s| {
                                s.color(
                                    *config.get_color(LapceColor::LAPCE_ICON_ACTIVE),
                                )
                            })
                            .apply_if(!is_dir, |s| {
                                s.apply_opt(
                                    config.file_svg(&path_for_style).1.cloned(),
                                    Style::color,
                                )
                            })
                    })
                },
                label(move || {
                    node.path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default()
                }),
            ))
            .style(move |s| {
                s.items_center()
                    .padding_right(10.0)
                    .padding_left((level * 10) as f32)
                    .min_width_pct(100.0)
                    .hover(|s| {
                        s.background(
                            *config
                                .get()
                                .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                        .cursor(CursorStyle::Pointer)
                    })
            })
            .on_click_stop(move |_| {
                data.click(&click_path);
            })
            .on_double_click(move |_| {
                if double_click_data.double_click(&double_click_path) {
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            })
            .on_event_stop(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    if pointer_event.button.is_auxiliary() {
                        aux_click_data.middle_click(&aux_click_path);
                    }
                }
            })
        },
    )
    .style(|s| s.flex_col().min_width_pct(100.0))
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
                            *config.get_color(LapceColor::PANEL_CURRENT_BACKGROUND),
                        )
                    },
                )
                .hover(|s| {
                    s.background(
                        *config.get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
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
        list(
            move || editor_tabs.get().into_iter().enumerate(),
            move |(index, (editor_tab_id, _))| (*index, *editor_tab_id),
            move |(index, (_, editor_tab))| {
                let plugin = plugin.clone();
                stack((
                    label(move || format!("Group {}", index + 1))
                        .style(|s| s.margin_left(10.0)),
                    list(
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
