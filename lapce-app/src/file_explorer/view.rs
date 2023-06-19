use std::{path::PathBuf, sync::Arc};

use floem::{
    peniko::Color,
    reactive::{ReadSignal, SignalGet},
    style::{CursorStyle, Style},
    view::View,
    views::{
        container, container_box, label, scroll, stack, svg, virtual_list,
        Decorators, VirtualListDirection, VirtualListItemSize, VirtualListVector,
    },
};
use lapce_rpc::proxy::ProxyRpcHandler;

use super::node::FileNode;
use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    panel::{position::PanelPosition, view::panel_header},
    window_tab::WindowTabData,
};

pub fn file_explorer_panel(
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let root_file_node = window_tab_data.file_explorer.root.clone();
    let proxy = window_tab_data.common.proxy.clone();
    stack(|| {
        (
            stack(move || (panel_header("Open Editors".to_string(), config),))
                .style(|| Style::BASE.width_pct(100.0).flex_col().height_px(150.0)),
            stack(|| {
                (
                    panel_header("File Explorer".to_string(), config),
                    container(|| {
                        scroll(move || {
                            file_node_view(
                                root_file_node.clone(),
                                proxy.clone(),
                                0,
                                config,
                            )
                        })
                        .scroll_bar_color(move || {
                            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
                        })
                        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
                    })
                    .style(|| Style::BASE.size_pct(100.0, 100.0).line_height(1.6)),
                )
            })
            .style(|| Style::BASE.width_pct(100.0).height_pct(100.0).flex_col()),
        )
    })
    .style(move || {
        Style::BASE
            .width_pct(100.0)
            .apply_if(!position.is_bottom(), |s| s.flex_col())
    })
}

fn file_node_view(
    file_node: FileNode,
    proxy: ProxyRpcHandler,
    level: usize,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    virtual_list(
        VirtualListDirection::Vertical,
        VirtualListItemSize::Fn(Box::new(|(_, file_node): &(PathBuf, FileNode)| {
            file_node.total_size().unwrap_or(0.0)
        })),
        move || file_node.clone(),
        |(path, _)| path.to_owned(),
        move |(path, file_node)| {
            let proxy = proxy.clone();
            stack(move || {
                (
                    {
                        let file_node = file_node.clone();
                        let proxy = proxy.clone();
                        let expanded = file_node.expanded;
                        let is_dir = file_node.is_dir;
                        stack(|| {
                            (
                                svg(move || {
                                    let config = config.get();
                                    let expanded = expanded.get();
                                    let svg_str = match expanded {
                                        true => LapceIcons::ITEM_OPENED,
                                        false => LapceIcons::ITEM_CLOSED,
                                    };
                                    config.ui_svg(svg_str)
                                })
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;

                                    let color = if is_dir {
                                        *config
                                            .get_color(LapceColor::LAPCE_ICON_ACTIVE)
                                    } else {
                                        Color::TRANSPARENT
                                    };
                                    Style::BASE
                                        .size_px(size, size)
                                        .margin_left_px(10.0)
                                        .color(color)
                                }),
                                {
                                    let path = path.clone();
                                    let path_for_style = path.clone();
                                    svg(move || {
                                        let config = config.get();
                                        if is_dir {
                                            let expanded = expanded.get();
                                            let svg_str = match expanded {
                                                true => LapceIcons::DIRECTORY_OPENED,
                                                false => {
                                                    LapceIcons::DIRECTORY_CLOSED
                                                }
                                            };
                                            config.ui_svg(svg_str)
                                        } else {
                                            config.file_svg(&path).0
                                        }
                                    })
                                    .style(
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;

                                            Style::BASE
                                                .size_px(size, size)
                                                .margin_horiz_px(6.0)
                                                .apply_if(is_dir, |s| {
                                                    s.color(*config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                ))
                                                })
                                                .apply_if(!is_dir, |s| {
                                                    s.apply_opt(
                                                        config
                                                            .file_svg(
                                                                &path_for_style,
                                                            )
                                                            .1
                                                            .cloned(),
                                                        Style::color,
                                                    )
                                                })
                                        },
                                    )
                                },
                                label(move || {
                                    path.file_name()
                                        .map(|f| f.to_string_lossy().to_string())
                                        .unwrap_or_default()
                                }),
                            )
                        })
                        .on_click(move |_| {
                            file_node.click(&proxy);
                            true
                        })
                        .style(move || {
                            Style::BASE
                                .items_center()
                                .padding_right_px(10.0)
                                .padding_left_px((level * 10) as f32)
                                .min_width_pct(100.0)
                        })
                        .hover_style(move || {
                            Style::BASE
                                .background(
                                    *config.get().get_color(
                                        LapceColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                                .cursor(CursorStyle::Pointer)
                        })
                    },
                    container_box(move || {
                        Box::new(file_node_view(
                            file_node,
                            proxy.clone(),
                            level + 1,
                            config,
                        ))
                    }),
                )
            })
            .style(|| Style::BASE.flex_col().min_width_pct(100.0))
        },
    )
    .style(|| Style::BASE.flex_col().min_width_pct(100.0))
}
