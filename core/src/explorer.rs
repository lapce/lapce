use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::{cmp, path::PathBuf};
use std::{str::FromStr, sync::Arc};

use druid::ExtEventSink;
use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll, SvgData},
    Affine, BoxConstraints, Color, Command, Cursor, Data, Env, Event, EventCtx,
    FontFamily, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2, Widget, WidgetExt,
    WidgetId, WidgetPod, WindowId,
};

use include_dir::{include_dir, Dir};
use lapce_proxy::dispatch::FileNodeItem;
use parking_lot::Mutex;

use crate::config::{Config, LapceTheme};
use crate::data::{LapceData, LapceTabData};
use crate::proxy::LapceProxy;
use crate::scroll::LapceScrollNew;
use crate::state::{LapceWorkspace, LapceWorkspaceType};
use crate::svg::{file_svg_new, get_svg};
use crate::theme::OldLapceTheme;
use crate::{
    command::LapceCommand, command::LapceUICommand, command::LAPCE_UI_COMMAND,
    movement::LinePosition, movement::Movement, palette::svg_tree_size,
    panel::PanelPosition, panel::PanelProperty, state::LapceFocus,
};

const ICONS_DIR: Dir = include_dir!("../icons");

#[derive(Clone)]
pub struct FileExplorerState {
    // pub widget_id: WidgetId,
    window_id: WindowId,
    tab_id: WidgetId,
    pub widget_id: WidgetId,
    // cwd: PathBuf,
    pub items: Vec<FileNodeItem>,
    index: usize,
    count: usize,
    position: PanelPosition,
}

#[derive(Clone)]
pub struct FileExplorerData {
    pub tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub workspace: Option<FileNodeItem>,
    index: usize,
    count: usize,
}

impl FileExplorerData {
    pub fn new(
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
    ) -> Self {
        let mut items = Vec::new();
        let widget_id = WidgetId::next();
        if let Some(path) = workspace.path.as_ref() {
            items.push(FileNodeItem {
                path_buf: path.clone(),
                is_dir: true,
                read: false,
                open: false,
                children: HashMap::new(),
                children_open_count: 0,
            });
            let index = 0;
            let path = path.clone();
            std::thread::spawn(move || {
                proxy.read_dir(
                    &path.clone(),
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            let resp: Result<Vec<FileNodeItem>, serde_json::Error> =
                                serde_json::from_value(res);
                            if let Ok(items) = resp {
                                event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::UpdateExplorerItems(
                                        index,
                                        path.clone(),
                                        items,
                                    ),
                                    Target::Widget(tab_id),
                                );
                            }
                        }
                    }),
                );
            });
        }
        Self {
            tab_id,
            widget_id,
            workspace: workspace.path.as_ref().map(|p| FileNodeItem {
                path_buf: p.clone(),
                is_dir: true,
                read: false,
                open: false,
                children: HashMap::new(),
                children_open_count: 0,
            }),
            index: 0,
            count: 0,
        }
    }

    pub fn update_node_count(&mut self, path: &PathBuf) -> Option<()> {
        let node = self.get_node_mut(path)?;
        if node.is_dir {
            if node.open {
                node.children_open_count = node
                    .children
                    .iter()
                    .map(|(_, item)| item.children_open_count + 1)
                    .sum::<usize>();
            } else {
                node.children_open_count = 0;
            }
        }
        None
    }

    pub fn node_tree(&mut self, path: &PathBuf) -> Option<Vec<PathBuf>> {
        let root = &self.workspace.as_ref()?.path_buf;
        let path = path.strip_prefix(root).ok()?;
        Some(
            path.ancestors()
                .map(|p| root.join(p))
                .collect::<Vec<PathBuf>>(),
        )
    }

    pub fn get_node_by_index(&mut self, index: usize) -> Option<&mut FileNodeItem> {
        let (_, node) = get_item_children(0, index, self.workspace.as_mut()?);
        node
    }

    pub fn get_node_mut(&mut self, path: &PathBuf) -> Option<&mut FileNodeItem> {
        let mut node = self.workspace.as_mut()?;
        if &node.path_buf == path {
            return Some(node);
        }
        let root = node.path_buf.clone();
        let path = path.strip_prefix(&root).ok()?;
        for path in path.ancestors().collect::<Vec<&Path>>().iter().rev() {
            if path.to_str()? == "" {
                continue;
            }
            node = node.children.get_mut(&root.join(path))?;
        }
        Some(node)
    }
}

pub fn paint_file_node_item(
    ctx: &mut PaintCtx,
    item: &FileNodeItem,
    min: usize,
    max: usize,
    line_height: f64,
    width: f64,
    level: usize,
    i: usize,
    index: usize,
    config: &Config,
) -> usize {
    if i > max {
        return i;
    }
    if i + item.children_open_count < min {
        return i + item.children_open_count;
    }
    if i >= min && i <= max {
        if i == index {
            ctx.fill(
                Rect::ZERO
                    .with_origin(Point::new(
                        0.0,
                        i as f64 * line_height - line_height,
                    ))
                    .with_size(Size::new(width, line_height)),
                config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
            );
        }
        let y = i as f64 * line_height - line_height;
        let svg_y = y + 4.0;
        let svg_size = 15.0;
        let padding = 15.0 * level as f64;
        if item.is_dir {
            let icon_name = if item.open {
                "chevron-down.svg"
            } else {
                "chevron-right.svg"
            };
            let svg = get_svg(icon_name).unwrap();
            let rect = Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(1.0 + padding, svg_y));
            ctx.draw_svg(
                &svg,
                rect,
                Some(config.get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)),
            );

            let icon_name = if item.open {
                "default_folder_opened.svg"
            } else {
                "default_folder.svg"
            };
            let svg = get_svg(icon_name).unwrap();
            let rect = Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(1.0 + 16.0 + padding, svg_y));
            ctx.draw_svg(&svg, rect, None);
        } else {
            let svg = file_svg_new(&item.path_buf);
            let rect = Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(1.0 + 16.0 + padding, svg_y));
            ctx.draw_svg(&svg, rect, None);
        }
        let text_layout = ctx
            .text()
            .new_text_layout(
                item.path_buf
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            )
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(&text_layout, Point::new(38.0 + padding, y + 3.0));
    }
    let mut i = i;
    if item.open {
        for item in item.sorted_children() {
            i = paint_file_node_item(
                ctx,
                item,
                min,
                max,
                line_height,
                width,
                level + 1,
                i + 1,
                index,
                config,
            );
            if i > max {
                return i;
            }
        }
    }
    i
}

fn get_item_children<'a>(
    i: usize,
    index: usize,
    item: &'a mut FileNodeItem,
) -> (usize, Option<&'a mut FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children_mut() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children(i + 1, index, child);
                if new_index == index {
                    return (new_index, node);
                }
            }
            i += count + 1;
        }
    }
    (i, None)
}

pub struct FileExplorer {
    widget_id: WidgetId,
    file_list: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl FileExplorer {
    pub fn new(data: &FileExplorerData) -> Self {
        let file_list = LapceScrollNew::new(FileExplorerFileList::new());
        Self {
            widget_id: data.widget_id,
            file_list: WidgetPod::new(file_list.boxed()),
        }
    }
}

impl Widget<LapceTabData> for FileExplorer {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.file_list.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.file_list.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.file_list.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let line_height = data.config.editor.line_height as f64;
        self.file_list.layout(ctx, bc, data, env);
        self.file_list
            .set_origin(ctx, data, env, Point::new(0.0, 0.0));
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.file_list.paint(ctx, data, env);

        //  let line_height = data.config.editor.line_height as f64;

        //  let shadow_width = 5.0;
        //  let rect = Size::new(ctx.size().width, line_height)
        //      .to_rect()
        //      .with_origin(Point::new(0.0, 0.0));
        //  ctx.blurred_rect(
        //      rect,
        //      shadow_width,
        //      data.config
        //          .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        //  );
        //  ctx.fill(
        //      rect,
        //      data.config
        //          .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        //  );

        //  let dir = data
        //      .workspace
        //      .path
        //      .as_ref()
        //      .map(|p| {
        //          let dir = p.file_name().unwrap().to_str().unwrap();
        //          let dir = match &data.workspace.kind {
        //              LapceWorkspaceType::Local => dir.to_string(),
        //              LapceWorkspaceType::RemoteSSH(user, host) => {
        //                  format!("{} [{}@{}]", dir, user, host)
        //              }
        //          };
        //          dir
        //      })
        //      .unwrap_or("Lapce".to_string());
        //  let text_layout = ctx
        //      .text()
        //      .new_text_layout(dir)
        //      .font(FontFamily::SYSTEM_UI, 13.0)
        //      .text_color(
        //          data.config
        //              .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
        //              .clone(),
        //      )
        //      .build()
        //      .unwrap();
        //  ctx.draw_text(&text_layout, Point::new(20.0, 4.0));
    }
}

pub struct FileExplorerFileList {}

impl FileExplorerFileList {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<LapceTabData> for FileExplorerFileList {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if let Some(workspace) = data.file_explorer.workspace.as_ref() {
                    let line_height = data.config.editor.line_height as f64;
                    let y = mouse_event.pos.y;
                    if y <= line_height
                        * (workspace.children_open_count + 1 + 1) as f64
                    {
                        ctx.set_cursor(&Cursor::Pointer);
                    } else {
                        ctx.clear_cursor();
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                let line_height = data.config.editor.line_height as f64;
                let file_explorer = Arc::make_mut(&mut data.file_explorer);
                let index =
                    ((mouse_event.pos.y + line_height) / line_height) as usize;
                if let Some(node) = file_explorer.get_node_by_index(index) {
                    if node.is_dir {
                        if node.read {
                            node.open = !node.open;
                        } else {
                            let tab_id = data.id;
                            let path = node.path_buf.clone();
                            let event_sink = ctx.get_external_handle();
                            data.proxy.read_dir(
                                &node.path_buf,
                                Box::new(move |result| {
                                    if let Ok(res) = result {
                                        let resp: Result<
                                            Vec<FileNodeItem>,
                                            serde_json::Error,
                                        > = serde_json::from_value(res);
                                        if let Ok(items) = resp {
                                            event_sink.submit_command(
                                                LAPCE_UI_COMMAND,
                                                LapceUICommand::UpdateExplorerItems(
                                                    index, path, items,
                                                ),
                                                Target::Widget(tab_id),
                                            );
                                        }
                                    }
                                }),
                            );
                        }
                        let path = node.path_buf.clone();
                        if let Some(paths) = file_explorer.node_tree(&path) {
                            for path in paths.iter() {
                                file_explorer.update_node_count(path);
                            }
                        }
                    } else {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::OpenFile(node.path_buf.clone()),
                            Target::Widget(data.id),
                        ));
                    }
                    file_explorer.index = index;
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data
            .file_explorer
            .workspace
            .as_ref()
            .map(|w| w.children_open_count)
            != old_data
                .file_explorer
                .workspace
                .as_ref()
                .map(|w| w.children_open_count)
        {
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let height = (data
            .file_explorer
            .workspace
            .as_ref()
            .map(|w| w.children_open_count)
            .unwrap_or(0)
            * data.config.editor.line_height) as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let line_height = data.config.editor.line_height as f64;

        let rect = ctx.region().bounding_box();
        let size = ctx.size();
        let width = size.width;
        let index = data.file_explorer.index;
        let min = (rect.y0 / line_height).floor() as usize;
        let max = (rect.y1 / line_height) as usize + 2;
        let level = 0;

        if let Some(item) = data.file_explorer.workspace.as_ref() {
            let mut i = 0;
            for item in item.sorted_children() {
                i = paint_file_node_item(
                    ctx,
                    item,
                    min,
                    max,
                    line_height,
                    width,
                    level + 1,
                    i + 1,
                    index,
                    &data.config,
                );
                if i > max {
                    return;
                }
            }
        }
    }
}
