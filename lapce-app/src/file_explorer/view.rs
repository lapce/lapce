use std::sync::Arc;

use floem::{
    peniko::Color,
    reactive::{ReadSignal, SignalGet},
    style::Style,
    view::View,
    views::{container, container_box, label, list, scroll, stack, svg, Decorators},
    AppContext,
};
use indexmap::IndexMap;
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    panel::{position::PanelPosition, view::panel_header},
    window_tab::WindowTabData,
};

use super::{data::FileExplorerData, node::FileNode};

pub fn file_explorer_panel(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let root_file_node = window_tab_data.file_explorer.root.clone();
    let proxy = window_tab_data.common.proxy.clone();
    stack(cx, |cx| {
        (
            stack(cx, move |cx| {
                (panel_header(cx, "Open Editors".to_string(), config),)
            })
            .style(cx, || {
                Style::BASE.width_pct(100.0).flex_col().height_px(150.0)
            }),
            stack(cx, |cx| {
                (
                    panel_header(cx, "File Explorer".to_string(), config),
                    container(cx, |cx| {
                        scroll(cx, move |cx| {
                            file_node_view(
                                cx,
                                root_file_node.clone(),
                                proxy.clone(),
                                0,
                                config,
                            )
                        })
                        .scroll_bar_color(cx, move || {
                            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
                        })
                        .style(cx, || Style::BASE.absolute().size_pct(100.0, 100.0))
                    })
                    .style(cx, || {
                        Style::BASE.size_pct(100.0, 100.0).line_height(1.6)
                    }),
                )
            })
            .style(cx, || {
                Style::BASE.width_pct(100.0).height_pct(100.0).flex_col()
            }),
        )
    })
    .style(cx, move || {
        Style::BASE
            .width_pct(100.0)
            .apply_if(!position.is_bottom(), |s| s.flex_col())
    })
}

fn file_node_view(
    cx: AppContext,
    file_node: FileNode,
    proxy: ProxyRpcHandler,
    level: usize,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let children = file_node.children;
    let expanded = file_node.expanded;
    list(
        cx,
        move || {
            if expanded.get() {
                children.get()
            } else {
                IndexMap::new()
            }
        },
        |(path, _)| path.to_owned(),
        move |cx, (path, file_node)| {
            let proxy = proxy.clone();
            stack(cx, move |cx| {
                (
                    {
                        let file_node = file_node.clone();
                        let proxy = proxy.clone();
                        let expanded = file_node.expanded;
                        let is_dir = file_node.is_dir;
                        stack(cx, |cx| {
                            (
                                {
                                    let path = path.clone();
                                    let path_for_style = path.clone();
                                    svg(cx, move || {
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
                                        cx,
                                        move || {
                                            let config = config.get();
                                            let size = config.ui.icon_size() as f32;

                                            let color = if is_dir {
                                                *config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                )
                                            } else {
                                                let color = config
                                                    .file_svg(&path_for_style)
                                                    .1;
                                                color.cloned().unwrap_or_else(|| {
                                                    *config.get_color(
                                                    LapceColor::LAPCE_ICON_ACTIVE,
                                                )
                                                })
                                            };
                                            Style::BASE
                                                .size_px(size, size)
                                                .margin_horiz_px(10.0)
                                                .color(color)
                                        },
                                    )
                                },
                                label(cx, move || {
                                    path.file_name()
                                        .map(|f| f.to_string_lossy().to_string())
                                        .unwrap_or_default()
                                }),
                            )
                        })
                        .on_click(move |_| {
                            file_node.toggle_expand(&proxy);
                            true
                        })
                        .style(cx, move || {
                            Style::BASE
                                .items_center()
                                .padding_right_px(10.0)
                                .padding_left_px((level * 10) as f32)
                        })
                    },
                    container_box(cx, move |cx| {
                        Box::new(file_node_view(
                            cx,
                            file_node,
                            proxy.clone(),
                            level + 1,
                            config,
                        ))
                    }),
                )
            })
            .style(cx, || Style::BASE.flex_col())
        },
    )
    .style(cx, || Style::BASE.flex_col())
}
