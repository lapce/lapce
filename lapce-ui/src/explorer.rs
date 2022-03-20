
use std::collections::HashMap;
use std::sync::Arc;

use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, FontFamily, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use include_dir::{include_dir, Dir};
use lapce_data::explorer::FileExplorerData;
use lapce_data::{
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    config::{Config, LapceTheme},
    data::LapceTabData,
};
use lapce_proxy::dispatch::FileNodeItem;

use crate::{
    scroll::LapceScrollNew,
    svg::{file_svg_new, get_svg},
};

#[allow(dead_code)]
const ICONS_DIR: Dir = include_dir!("../icons");

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
    toggle_rects: &mut HashMap<usize, Rect>,
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
            toggle_rects.insert(i, rect);

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
        ctx.draw_text(
            &text_layout,
            Point::new(
                38.0 + padding,
                y + (line_height - text_layout.size().height) / 2.0,
            ),
        );
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
                toggle_rects,
            );
            if i > max {
                return i;
            }
        }
    }
    i
}

pub fn get_item_children(
    i: usize,
    index: usize,
    item: &FileNodeItem,
) -> (usize, Option<&FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children() {
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

pub fn get_item_children_mut(
    i: usize,
    index: usize,
    item: &mut FileNodeItem,
) -> (usize, Option<&mut FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children_mut() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children_mut(i + 1, index, child);
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
        _old_data: &LapceTabData,
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

pub struct FileExplorerFileList {
    line_height: f64,
}

impl FileExplorerFileList {
    pub fn new() -> Self {
        Self { line_height: 25.0 }
    }
}

impl Default for FileExplorerFileList {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for FileExplorerFileList {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if let Some(workspace) = data.file_explorer.workspace.as_ref() {
                    let y = mouse_event.pos.y;
                    if y <= self.line_height
                        * (workspace.children_open_count + 1 + 1) as f64
                    {
                        ctx.set_cursor(&Cursor::Pointer);
                    } else {
                        ctx.clear_cursor();
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                let file_explorer = Arc::make_mut(&mut data.file_explorer);
                let index = ((mouse_event.pos.y + self.line_height)
                    / self.line_height) as usize;
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
                                            let _ = event_sink.submit_command(
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
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
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
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let height = data
            .file_explorer
            .workspace
            .as_ref()
            .map(|w| w.children_open_count)
            .unwrap_or(0) as f64
            * self.line_height;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let rect = ctx.region().bounding_box();
        let size = ctx.size();
        let width = size.width;
        let index = data.file_explorer.index;
        let min = (rect.y0 / self.line_height).floor() as usize;
        let max = (rect.y1 / self.line_height) as usize + 2;
        let level = 0;

        if let Some(item) = data.file_explorer.workspace.as_ref() {
            let mut i = 0;
            for item in item.sorted_children() {
                i = paint_file_node_item(
                    ctx,
                    item,
                    min,
                    max,
                    self.line_height,
                    width,
                    level + 1,
                    i + 1,
                    index,
                    &data.config,
                    &mut HashMap::new(),
                );
                if i > max {
                    return;
                }
            }
        }
    }
}
