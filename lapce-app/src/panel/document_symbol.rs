use std::{ops::AddAssign, path::PathBuf, rc::Rc};

use floem::{
    View,
    peniko::Color,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, VirtualVector, container, editor::id::Id, label, scroll, stack,
        svg, virtual_stack,
    },
};
use lsp_types::{DocumentSymbol, SymbolKind};

use super::position::PanelPosition;
use crate::{
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons},
    editor::location::EditorLocation,
    window_tab::WindowTabData,
};

#[derive(Clone, Debug)]
pub struct SymbolData {
    pub path: PathBuf,
    pub file: RwSignal<SymbolInformationItemData>,
}

impl SymbolData {
    pub fn new(
        items: Vec<RwSignal<SymbolInformationItemData>>,
        path: PathBuf,
        cx: Scope,
    ) -> Self {
        let name = path
            .file_name()
            .and_then(|x| x.to_str())
            .map(|x| x.to_string())
            .unwrap_or_default();
        #[allow(deprecated)]
        let file_ds = DocumentSymbol {
            name: name.clone(),
            detail: None,
            kind: SymbolKind::FILE,
            tags: None,
            deprecated: None,
            range: Default::default(),
            selection_range: Default::default(),
            children: None,
        };
        let file = cx.create_rw_signal(SymbolInformationItemData {
            id: Id::next(),
            name,
            detail: None,
            item: file_ds,
            open: cx.create_rw_signal(true),
            children: items,
        });
        Self { path, file }
    }
    fn get_children(
        &self,
        min: usize,
        max: usize,
    ) -> Vec<(
        usize,
        usize,
        Rc<PathBuf>,
        RwSignal<SymbolInformationItemData>,
    )> {
        let path = Rc::new(self.path.clone());
        let level: usize = 0;
        let mut next = 0;
        get_children(self.file, &mut next, min, max, level, path.clone())
    }
}

#[derive(Debug, Clone)]
pub struct SymbolInformationItemData {
    pub id: Id,
    pub name: String,
    pub detail: Option<String>,
    pub item: DocumentSymbol,
    pub open: RwSignal<bool>,
    pub children: Vec<RwSignal<SymbolInformationItemData>>,
}

impl From<(DocumentSymbol, Scope)> for SymbolInformationItemData {
    fn from((mut item, cx): (DocumentSymbol, Scope)) -> Self {
        let children = if let Some(children) = item.children.take() {
            children
                .into_iter()
                .map(|x| cx.create_rw_signal(Self::from((x, cx))))
                .collect()
        } else {
            Vec::with_capacity(0)
        };
        Self {
            id: Id::next(),
            name: item.name.clone(),
            detail: item.detail.clone(),
            item,
            open: cx.create_rw_signal(true),
            children,
        }
    }
}

impl SymbolInformationItemData {
    pub fn child_count(&self) -> usize {
        let mut count = 1;
        if self.open.get() {
            for child in &self.children {
                count += child.with(|x| x.child_count())
            }
        }
        count
    }
}

fn get_children(
    data: RwSignal<SymbolInformationItemData>,
    next: &mut usize,
    min: usize,
    max: usize,
    level: usize,
    path: Rc<PathBuf>,
) -> Vec<(
    usize,
    usize,
    Rc<PathBuf>,
    RwSignal<SymbolInformationItemData>,
)> {
    let mut children = Vec::new();
    if *next >= min && *next < max {
        children.push((*next, level, path.clone(), data));
    } else if *next >= max {
        return children;
    }
    next.add_assign(1);
    if data.get_untracked().open.get() {
        for child in data.get().children {
            let child_children =
                get_children(child, next, min, max, level + 1, path.clone());
            children.extend(child_children);
            if *next > max {
                break;
            }
        }
    }
    children
}

pub struct VirtualList {
    root: Option<RwSignal<Option<SymbolData>>>,
}

impl VirtualList {
    pub fn new(root: Option<RwSignal<Option<SymbolData>>>) -> Self {
        Self { root }
    }
}

impl
    VirtualVector<(
        usize,
        usize,
        Rc<PathBuf>,
        RwSignal<SymbolInformationItemData>,
    )> for VirtualList
{
    fn total_len(&self) -> usize {
        if let Some(root) = self.root.as_ref().and_then(|x| x.get()) {
            root.file.get_untracked().child_count()
        } else {
            0
        }
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<
        Item = (
            usize,
            usize,
            Rc<PathBuf>,
            RwSignal<SymbolInformationItemData>,
        ),
    > {
        if let Some(root) = self.root.as_ref().and_then(|x| x.get()) {
            let min = range.start;
            let max = range.end;
            let children = root.get_children(min, max);
            children.into_iter()
        } else {
            Vec::new().into_iter()
        }
    }
}

pub fn symbol_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    scroll(
        virtual_stack(
            {
                let window_tab_data = window_tab_data.clone();
                move || {
                    let editor = window_tab_data.main_split.get_active_editor();
                    VirtualList::new(editor.map(|x| x.doc().document_symbol_data))
                }
            },
            move |(_, _, _, item)| item.get_untracked().id,
            move |(_, level, path,  rw_data)| {
                let data = rw_data.get_untracked();
                let open = data.open;
                let has_child = !data.children.is_empty();
                let kind = data.item.kind;
                stack((
                    container(
                        svg(move || {
                            let config = config.get();
                            let svg_str = match open.get() {
                                true => LapceIcons::ITEM_OPENED,
                                false => LapceIcons::ITEM_CLOSED,
                            };
                            config.ui_svg(svg_str)
                        })
                        .style(move |s| {
                            let config = config.get();
                            let color = if has_child {
                                config.color(LapceColor::LAPCE_ICON_ACTIVE)
                            } else {
                                Color::TRANSPARENT
                            };
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                 .color(color)
                        })
                    ).style(|s| s.padding(4.0).margin_left(6.0).margin_right(2.0))
                    .on_click_stop({
                        move |_x| {
                            if has_child {
                                open.update(|x| {
                                    *x = !*x;
                                });
                            }
                        }
                    }),
                    svg(move || {
                        let config = config.get();
                        config
                            .symbol_svg(&kind)
                            .unwrap_or_else(|| config.ui_svg(LapceIcons::FILE))
                    }).style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.min_width(size)
                                .size(size, size)
                                .margin_right(5.0)
                                .color(config.symbol_color(&kind).unwrap_or_else(|| {
                                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                                }))
                        }),
                    label(move || {
                            data.name.replace('\n', "↵")
                    })
                    .style(move |s| {
                        s.selectable(false)
                    }),
                    label(move || {
                        data.detail.clone().unwrap_or_default()
                    }).style(move |s| s.margin_left(6.0)
                                              .color(config.get().color(LapceColor::EDITOR_DIM))
                                              .selectable(false)
                                              .apply_if(
                                                data.item.detail.clone().is_none(),
                                                 |s| s.hide())
                    ),
                ))
                .style(move |s| {
                    s.padding_right(5.0)
                        .padding_left((level * 10) as f32)
                        .items_center()
                        .height(ui_line_height.get())
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                            .cursor(CursorStyle::Pointer)
                        })
                })
                .on_click_stop({
                    let window_tab_data = window_tab_data.clone();
                    let data = rw_data;
                    move |_| {
                        let data = data.get_untracked();
                            window_tab_data
                                .common
                                .internal_command
                                .send(InternalCommand::JumpToLocation { location: EditorLocation {
                                    path: path.to_path_buf(),
                                    position: Some(crate::editor::location::EditorPosition::Position(data.item.selection_range.start)),
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                } });
                    }
                })
            },
        ).item_size_fixed(move || ui_line_height.get())
        .style(|s| s.flex_col().absolute().min_width_full()),
    )
    .style(|s| s.absolute().size_full())
}
