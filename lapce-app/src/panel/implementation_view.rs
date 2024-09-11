use std::rc::Rc;

use floem::{
    reactive::{Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        container, label, scroll, stack, svg, virtual_stack, Decorators,
        VirtualDirection, VirtualItemSize,
    },
    IntoView, View, ViewId,
};
use im::HashMap;
use lsp_types::{request::GotoImplementationResponse, SymbolKind};

use super::position::PanelPosition;
use crate::{
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons},
    editor::location::EditorLocation,
    panel::references_view::{Reference, ReferenceLocation, ReferencesRoot},
    window_tab::WindowTabData,
};

pub fn implementation_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    scroll(
        virtual_stack(
            VirtualDirection::Vertical,
            VirtualItemSize::Fixed(Box::new(move || ui_line_height.get())),
            move || main_split.implementations.get(),
            move |(_, _, data)| data.view_id(),
            move |(_, level, rw_data)| {
                match rw_data {
                    ReferenceLocation::File { path, open, .. } => {
                        stack((
                            container(svg(move || {
                                    let config = config.get();
                                    let svg_str = match open.get() {
                                        true => LapceIcons::ITEM_OPENED,
                                        false => LapceIcons::ITEM_CLOSED,
                                    };
                                    config.ui_svg(svg_str)
                                })
                                    .style(move |s| {
                                        let config = config.get();
                                        let size = config.ui.icon_size() as f32;
                                        s.size(size, size)
                                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                                    })
                                )
                                .style(|s| s.padding(4.0).margin_left(6.0).margin_right(2.0))
                                .on_click_stop({
                                    move |_x| {
                                        open.update(|x| {
                                            *x = !*x;
                                        });
                                    }
                                }),
                            svg(move || {
                                let config = config.get();
                                config
                                    .symbol_svg(&SymbolKind::FILE)
                                    .unwrap_or_else(|| config.ui_svg(LapceIcons::FILE))
                            }).style(move |s| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                s.min_width(size)
                                    .size(size, size)
                                    .margin_right(5.0)
                                    .color(config.symbol_color(&SymbolKind::FILE).unwrap_or_else(|| {
                                        config.color(LapceColor::LAPCE_ICON_ACTIVE)
                                    }))
                            }),
                                label(move || {
                                    format!("{:?}", path)
                                }).style(move |s| s.margin_left(6.0)
                                    .color(config.get().color(LapceColor::EDITOR_DIM))
                                ).into_any()
                        ))
                            .style(move |s| {
                                s.padding_right(5.0)
                                    .height(ui_line_height.get())
                                    .padding_left((level * 10) as f32)
                                    .items_center()
                                    .hover(|s| {
                                        s.background(
                                            config
                                                .get()
                                                .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                        )
                                            .cursor(CursorStyle::Pointer)
                                    })
                            })
                    }
                    ReferenceLocation::Line { path, range, .. } => {
                    stack(
                        (
                            container(
                                label(
                                    move || {
                                        format!("{}", range.line + 1)
                                    })
                                .style(move |s| s.margin_left(6.0)
                                    .color(config.get().color(LapceColor::EDITOR_DIM))
                                ).into_any()
                            )
                            .style(move |s| {
                                s.padding_right(5.0)
                                    .height(ui_line_height.get())
                                    .padding_left((level * 20) as f32)
                                    .items_center()
                                    .hover(|s| {
                                        s.background(
                                            config
                                                .get()
                                                .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                        )
                                            .cursor(CursorStyle::Pointer)
                                    })
                            }),
                        )
                    ).on_click_stop({
                        let window_tab_data = window_tab_data.clone();
                        move |_|
                        {
                            let range = range;
                            window_tab_data
                                .common
                                .internal_command
                                .send(InternalCommand::GoToLocation {
                                    location: EditorLocation {
                                        path: path.clone(),
                                        position: Some(crate::editor::location::EditorPosition::Position(range)),
                                        scroll_offset: None,
                                        ignore_unconfirmed: false,
                                        same_editor_tab: false,
                                    }
                                });
                        }
                    })
                    }
                }.style(move |s| {
                    s.padding_right(5.0)
                        .height(ui_line_height.get())
                        .padding_left((level * 10) as f32)
                        .items_center()
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                                .cursor(CursorStyle::Pointer)
                        })
                })
            },
        )
        .style(|s| s.flex_col().absolute().min_width_full()),
    )
    .style(|s| s.absolute().size_full())
    .debug_name("references panel")
}

pub fn init_implementation_root(
    resp: Option<GotoImplementationResponse>,
    scope: Scope,
) -> ReferencesRoot {
    let Some(resp) = resp else {
        return ReferencesRoot::default();
    };
    let mut refs_map = HashMap::new();
    match resp {
        GotoImplementationResponse::Scalar(local) => {
            if let Ok(path) = local.uri.to_file_path() {
                let entry = refs_map.entry(path.clone()).or_insert(Vec::new());
                (*entry).push(Reference::Line {
                    location: ReferenceLocation::Line {
                        view_id: ViewId::new(),
                        path,
                        range: local.range.start,
                    },
                })
            }
        }
        GotoImplementationResponse::Array(items) => {
            for item in items {
                if let Ok(path) = item.uri.to_file_path() {
                    let entry = refs_map.entry(path.clone()).or_insert(Vec::new());
                    (*entry).push(Reference::Line {
                        location: ReferenceLocation::Line {
                            view_id: ViewId::new(),
                            path,
                            range: item.range.start,
                        },
                    })
                }
            }
        }
        GotoImplementationResponse::Link(items) => {
            for item in items {
                if let Ok(path) = item.target_uri.to_file_path() {
                    let entry = refs_map.entry(path.clone()).or_insert(Vec::new());
                    (*entry).push(Reference::Line {
                        location: ReferenceLocation::Line {
                            view_id: ViewId::new(),
                            path,
                            range: item.target_range.start,
                        },
                    })
                }
            }
        }
    }

    let mut refs = Vec::new();
    for (path, items) in refs_map {
        let open = scope.create_rw_signal(true);
        let ref_item = Reference::File {
            location: ReferenceLocation::File {
                open,
                path,
                view_id: ViewId::new(),
            },
            children: items,
            open,
        };
        refs.push(ref_item);
    }
    tracing::debug!("children {}", refs.len());
    ReferencesRoot { children: refs }
}
