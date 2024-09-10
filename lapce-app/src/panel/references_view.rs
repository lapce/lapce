use std::{ops::AddAssign, path::PathBuf, rc::Rc};

use floem::{
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        container, label, scroll, stack, svg, virtual_stack, Decorators,
        VirtualDirection, VirtualItemSize, VirtualVector,
    },
    IntoView, View, ViewId,
};
use im::HashMap;
use lsp_types::{Location, Position, SymbolKind};

use super::position::PanelPosition;
use crate::{
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons},
    editor::location::EditorLocation,
    window_tab::WindowTabData,
};

pub fn references_panel(
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
            move || main_split.references.get(),
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
                                        tracing::debug!("open = {:?} {}", SignalGet::id(&open), open.get_untracked());
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
                            tracing::info!("go to location: {:?}", range);
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
    .debug_name("references_section")
}

pub fn init_references_root(items: Vec<Location>, scope: Scope) -> ReferencesRoot {
    tracing::debug!("get_items {}", items.len());
    let mut refs_map = HashMap::new();
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
    let mut refs = Vec::new();
    for (path, items) in refs_map {
        let open = scope.create_rw_signal(true);
        let ref_item = Reference::File {
            location: crate::panel::references_view::ReferenceLocation::File {
                open,
                path,
                view_id: ViewId::new(),
            },
            children: items,
            open,
        };
        refs.push(ref_item);
    }
    ReferencesRoot { children: refs }
}

#[derive(Clone, Default)]
pub struct ReferencesRoot {
    children: Vec<Reference>,
}
impl VirtualVector<(usize, usize, ReferenceLocation)> for ReferencesRoot {
    fn total_len(&self) -> usize {
        self.total()
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (usize, usize, ReferenceLocation)> {
        let min = range.start;
        let max = range.end;
        let children = self.get_children(&mut 0, min, max, 0);
        children.into_iter()
    }
}

impl ReferencesRoot {
    pub fn total(&self) -> usize {
        let mut total = 0;
        for child in &self.children {
            total += child.total_len()
        }
        total
    }

    fn get_children(
        &self,
        next: &mut usize,
        min: usize,
        max: usize,
        level: usize,
    ) -> Vec<(usize, usize, ReferenceLocation)> {
        let mut children = Vec::new();
        for child in &self.children {
            let child_children = child.get_children(next, min, max, level + 1);
            if !child_children.is_empty() {
                children.extend(child_children);
            }
            if *next > max {
                break;
            }
        }
        children
    }
}

#[derive(Clone)]
enum Reference {
    File {
        location: ReferenceLocation,
        open: RwSignal<bool>,
        children: Vec<Reference>,
    },
    Line {
        location: ReferenceLocation,
    },
}

#[derive(Clone)]
pub enum ReferenceLocation {
    File {
        path: PathBuf,
        open: RwSignal<bool>,
        view_id: ViewId,
    },
    Line {
        path: PathBuf,
        range: Position,
        view_id: ViewId,
    },
}

impl ReferenceLocation {
    pub fn view_id(&self) -> ViewId {
        match self {
            ReferenceLocation::File { view_id, .. } => *view_id,
            ReferenceLocation::Line { view_id, .. } => *view_id,
        }
    }
}

impl Reference {
    pub fn location(&self) -> ReferenceLocation {
        match self {
            Reference::File { location, .. } => location.clone(),
            Reference::Line { location } => location.clone(),
        }
    }
    pub fn total_len(&self) -> usize {
        match self {
            Reference::File { children, .. } => {
                let mut total = 1;
                for child in children {
                    total += child.total_len()
                }
                total
            }
            Reference::Line { .. } => 1,
        }
    }
    pub fn children(&self) -> Option<&Vec<Reference>> {
        match self {
            Reference::File { children, open, .. } => {
                if open.get() {
                    return Some(children);
                }
                None
            }
            Reference::Line { .. } => None,
        }
    }

    fn get_children(
        &self,
        next: &mut usize,
        min: usize,
        max: usize,
        level: usize,
    ) -> Vec<(usize, usize, ReferenceLocation)> {
        let mut children = Vec::new();
        if *next >= min && *next < max {
            children.push((*next, level, self.location()));
        } else if *next >= max {
            return children;
        }
        next.add_assign(1);
        if let Some(children_tmp) = self.children() {
            for child in children_tmp {
                let child_children = child.get_children(next, min, max, level + 1);
                if !child_children.is_empty() {
                    children.extend(child_children);
                }
                if *next > max {
                    break;
                }
            }
        }
        children
    }
}
//
// fn file_view(
//     workspace: Arc<LapceWorkspace>,
//     path: PathBuf,
//     diagnostic_data: DiagnosticData,
//     severity: DiagnosticSeverity,
//     internal_command: Listener<InternalCommand>,
//     config: ReadSignal<Arc<LapceConfig>>,
// ) -> impl View {
//     let collpased = create_rw_signal(false);
//
//     let diagnostics = create_rw_signal(im::Vector::new());
//     create_effect(move |_| {
//         let span = diagnostic_data.diagnostics_span.get();
//         let d = if !span.is_empty() {
//             span.iter()
//                 .filter_map(|(iv, diag)| {
//                     if diag.severity == Some(severity) {
//                         Some(EditorDiagnostic {
//                             range: Some((iv.start, iv.end)),
//                             diagnostic: diag.to_owned(),
//                         })
//                     } else {
//                         None
//                     }
//                 })
//                 .collect::<im::Vector<EditorDiagnostic>>()
//         } else {
//             let diagnostics = diagnostic_data.diagnostics.get();
//             let diagnostics: im::Vector<EditorDiagnostic> = diagnostics
//                 .into_iter()
//                 .filter_map(|d| {
//                     if d.severity == Some(severity) {
//                         Some(EditorDiagnostic {
//                             range: None,
//                             diagnostic: d,
//                         })
//                     } else {
//                         None
//                     }
//                 })
//                 .collect();
//             diagnostics
//         };
//         diagnostics.set(d);
//     });
//
//     let full_path = path.clone();
//     let path = if let Some(workspace_path) = workspace.path.as_ref() {
//         path.strip_prefix(workspace_path)
//             .unwrap_or(&full_path)
//             .to_path_buf()
//     } else {
//         path
//     };
//     let style_path = path.clone();
//
//     let icon = match severity {
//         DiagnosticSeverity::ERROR => LapceIcons::ERROR,
//         _ => LapceIcons::WARNING,
//     };
//     let icon_color = move || {
//         let config = config.get();
//         match severity {
//             DiagnosticSeverity::ERROR => config.color(LapceColor::LAPCE_ERROR),
//             _ => config.color(LapceColor::LAPCE_WARN),
//         }
//     };
//
//     let file_name = path
//         .file_name()
//         .and_then(|s| s.to_str())
//         .unwrap_or("")
//         .to_string();
//
//     let folder = path
//         .parent()
//         .and_then(|s| s.to_str())
//         .unwrap_or("")
//         .to_string();
//
//     stack((
//         stack((
//             container(
//                 stack((
//                     label(move || file_name.clone()).style(|s| {
//                         s.margin_right(6.0)
//                             .max_width_pct(100.0)
//                             .text_ellipsis()
//                             .selectable(false)
//                     }),
//                     label(move || folder.clone()).style(move |s| {
//                         s.color(config.get().color(LapceColor::EDITOR_DIM))
//                             .min_width(0.0)
//                             .text_ellipsis()
//                             .selectable(false)
//                     }),
//                 ))
//                 .style(move |s| s.width_pct(100.0).min_width(0.0)),
//             )
//             .on_click_stop(move |_| {
//                 collpased.update(|collpased| *collpased = !*collpased);
//             })
//             .style(move |s| {
//                 let config = config.get();
//                 s.width_pct(100.0)
//                     .min_width(0.0)
//                     .padding_left(10.0 + (config.ui.icon_size() as f32 + 6.0) * 2.0)
//                     .padding_right(10.0)
//                     .hover(|s| {
//                         s.cursor(CursorStyle::Pointer).background(
//                             config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
//                         )
//                     })
//             }),
//             stack((
//                 svg(move || {
//                     config.get().ui_svg(if collpased.get() {
//                         LapceIcons::ITEM_CLOSED
//                     } else {
//                         LapceIcons::ITEM_OPENED
//                     })
//                 })
//                 .style(move |s| {
//                     let config = config.get();
//                     let size = config.ui.icon_size() as f32;
//                     s.margin_right(6.0)
//                         .size(size, size)
//                         .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
//                 }),
//                 svg(move || config.get().file_svg(&path).0).style(move |s| {
//                     let config = config.get();
//                     let size = config.ui.icon_size() as f32;
//                     let color = config.file_svg(&style_path).1;
//                     s.min_width(size)
//                         .size(size, size)
//                         .apply_opt(color, Style::color)
//                 }),
//                 label(|| " ".to_string()).style(move |s| s.selectable(false)),
//             ))
//             .style(|s| s.absolute().items_center().margin_left(10.0)),
//         ))
//         .style(move |s| s.width_pct(100.0).min_width(0.0)),
//         dyn_stack(
//             move || {
//                 if collpased.get() {
//                     im::Vector::new()
//                 } else {
//                     diagnostics.get()
//                 }
//             },
//             |d| (d.range, d.diagnostic.range),
//             move |d| {
//                 item_view(
//                     full_path.clone(),
//                     d,
//                     icon,
//                     icon_color,
//                     internal_command,
//                     config,
//                 )
//             },
//         )
//         .style(|s| s.flex_col().width_pct(100.0).min_width_pct(0.0)),
//     ))
//     .style(move |s| {
//         s.width_pct(100.0)
//             .items_start()
//             .flex_col()
//             .apply_if(diagnostics.with(|d| d.is_empty()), |s| s.hide())
//     })
// }
//
// fn item_view(
//     path: PathBuf,
//     d: EditorDiagnostic,
//     icon: &'static str,
//     icon_color: impl Fn() -> Color + 'static,
//     internal_command: Listener<InternalCommand>,
//     config: ReadSignal<Arc<LapceConfig>>,
// ) -> impl View {
//     let related = d.diagnostic.related_information.unwrap_or_default();
//     let position = if let Some((start, _)) = d.range {
//         EditorPosition::Offset(start)
//     } else {
//         EditorPosition::Position(d.diagnostic.range.start)
//     };
//     let location = EditorLocation {
//         path,
//         position: Some(position),
//         scroll_offset: None,
//         ignore_unconfirmed: false,
//         same_editor_tab: false,
//     };
//     stack((
//         container({
//             stack((
//                 label(move || d.diagnostic.message.clone()).style(move |s| {
//                     s.width_pct(100.0)
//                         .min_width(0.0)
//                         .padding_left(
//                             10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 3.0,
//                         )
//                         .padding_right(10.0)
//                 }),
//                 stack((
//                     svg(move || config.get().ui_svg(icon)).style(move |s| {
//                         let config = config.get();
//                         let size = config.ui.icon_size() as f32;
//                         s.size(size, size).color(icon_color())
//                     }),
//                     label(|| " ".to_string()).style(move |s| s.selectable(false)),
//                 ))
//                 .style(move |s| {
//                     s.absolute().items_center().margin_left(
//                         10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 2.0,
//                     )
//                 }),
//             ))
//             .style(move |s| {
//                 s.width_pct(100.0).min_width(0.0).hover(|s| {
//                     s.cursor(CursorStyle::Pointer).background(
//                         config.get().color(LapceColor::PANEL_HOVERED_BACKGROUND),
//                     )
//                 })
//             })
//         })
//         .on_click_stop(move |_| {
//             internal_command.send(InternalCommand::JumpToLocation {
//                 location: location.clone(),
//             });
//         })
//         .style(|s| s.width_pct(100.0).min_width_pct(0.0)),
//         related_view(related, internal_command, config),
//     ))
//     .style(|s| s.width_pct(100.0).min_width_pct(0.0).flex_col())
// }
//
// fn related_view(
//     related: Vec<DiagnosticRelatedInformation>,
//     internal_command: Listener<InternalCommand>,
//     config: ReadSignal<Arc<LapceConfig>>,
// ) -> impl View {
//     let is_empty = related.is_empty();
//     stack((
//         dyn_stack(
//             move || related.clone(),
//             |_| 0,
//             move |related| {
//                 let full_path = path_from_url(&related.location.uri);
//                 let path = full_path
//                     .file_name()
//                     .and_then(|f| f.to_str())
//                     .map(|f| {
//                         format!(
//                             "{f} [{}, {}]: ",
//                             related.location.range.start.line,
//                             related.location.range.start.character
//                         )
//                     })
//                     .unwrap_or_default();
//                 let location = EditorLocation {
//                     path: full_path,
//                     position: Some(EditorPosition::Position(
//                         related.location.range.start,
//                     )),
//                     scroll_offset: None,
//                     ignore_unconfirmed: false,
//                     same_editor_tab: false,
//                 };
//                 let message = format!("{path}{}", related.message);
//                 container(
//                     label(move || message.clone())
//                         .style(move |s| s.width_pct(100.0).min_width(0.0)),
//                 )
//                 .on_click_stop(move |_| {
//                     internal_command.send(InternalCommand::JumpToLocation {
//                         location: location.clone(),
//                     });
//                 })
//                 .style(move |s| {
//                     let config = config.get();
//                     s.padding_left(10.0 + (config.ui.icon_size() as f32 + 6.0) * 4.0)
//                         .padding_right(10.0)
//                         .width_pct(100.0)
//                         .min_width(0.0)
//                         .hover(|s| {
//                             s.cursor(CursorStyle::Pointer).background(
//                                 config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
//                             )
//                         })
//                 })
//             },
//         )
//         .style(|s| s.width_pct(100.0).min_width(0.0).flex_col()),
//         stack((
//             svg(move || config.get().ui_svg(LapceIcons::LINK)).style(move |s| {
//                 let config = config.get();
//                 let size = config.ui.icon_size() as f32;
//                 s.size(size, size)
//                     .color(config.color(LapceColor::EDITOR_DIM))
//             }),
//             label(|| " ".to_string()).style(move |s| s.selectable(false)),
//         ))
//         .style(move |s| {
//             s.absolute()
//                 .items_center()
//                 .margin_left(10.0 + (config.get().ui.icon_size() as f32 + 6.0) * 3.0)
//         }),
//     ))
//     .style(move |s| {
//         s.width_pct(100.0)
//             .min_width(0.0)
//             .items_start()
//             .color(config.get().color(LapceColor::EDITOR_DIM))
//             .apply_if(is_empty, |s| s.hide())
//     })
// }
