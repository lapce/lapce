use std::{cmp, path::PathBuf};
use std::{str::FromStr, sync::Arc};

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

pub struct FileExplorer {
    window_id: WindowId,
    tab_id: WidgetId,
    widget_id: WidgetId,
}

impl FileExplorer {
    pub fn new(window_id: WindowId, tab_id: WidgetId, widget_id: WidgetId) -> Self {
        Self {
            window_id,
            tab_id,
            widget_id,
        }
    }
}

// impl Widget<LapceUIState> for FileExplorer {
//     fn id(&self) -> Option<WidgetId> {
//         Some(self.widget_id)
//     }
//
//     fn event(
//         &mut self,
//         ctx: &mut EventCtx,
//         event: &Event,
//         data: &mut LapceUIState,
//         env: &Env,
//     ) {
//         match event {
//             Event::Command(cmd) => match cmd {
//                 _ if cmd.is(LAPCE_UI_COMMAND) => {
//                     let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
//                     match command {
//                         LapceUICommand::RequestPaint => {
//                             ctx.request_paint();
//                         }
//                         _ => (),
//                     }
//                 }
//                 _ => (),
//             },
//             _ => (),
//         }
//     }
//
//     fn lifecycle(
//         &mut self,
//         ctx: &mut LifeCycleCtx,
//         event: &LifeCycle,
//         data: &LapceUIState,
//         env: &Env,
//     ) {
//     }
//
//     fn update(
//         &mut self,
//         ctx: &mut UpdateCtx,
//         old_data: &LapceUIState,
//         data: &LapceUIState,
//         env: &Env,
//     ) {
//     }
//
//     fn layout(
//         &mut self,
//         ctx: &mut LayoutCtx,
//         bc: &BoxConstraints,
//         data: &LapceUIState,
//         env: &Env,
//     ) -> Size {
//         bc.max()
//     }
//
//     fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
//         let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
//         let explorer = state.file_explorer.lock();
//         explorer.paint(ctx, data, env);
//     }
// }

// impl PanelProperty for FileExplorerState {
//     fn widget_id(&self) -> WidgetId {
//         self.widget_id
//     }
//
//     fn position(&self) -> &PanelPosition {
//         &self.position
//     }
//
//     fn active(&self) -> usize {
//         0
//     }
//
//     fn size(&self) -> (f64, f64) {
//         (300.0, 0.5)
//     }
//
//     fn paint(&self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
//         let line_height = env.get(OldLapceTheme::EDITOR_LINE_HEIGHT);
//
//         let size = ctx.size();
//         let header_height = line_height;
//         let header_rect = Rect::ZERO.with_size(Size::new(size.width, header_height));
//         if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
//             ctx.fill(header_rect, background);
//         }
//         ctx.fill(
//             Size::new(size.width, size.height - header_height)
//                 .to_rect()
//                 .with_origin(Point::new(0.0, header_height)),
//             &env.get(OldLapceTheme::EDITOR_CURRENT_LINE_BACKGROUND),
//         );
//
//         let text_layout = ctx
//             .text()
//             .new_text_layout("Explorer")
//             .font(FontFamily::SYSTEM_UI, 14.0)
//             .text_color(env.get(OldLapceTheme::EDITOR_FOREGROUND));
//         let text_layout = text_layout.build().unwrap();
//         ctx.draw_text(&text_layout, Point::new(20.0, 5.0));
//
//         let rects = ctx.region().rects().to_vec();
//         let size = ctx.size();
//         let width = size.width;
//         let index = self.index;
//
//         for rect in rects {
//             let min = (rect.y0 / line_height).floor() as usize;
//             let max = (rect.y1 / line_height) as usize + 1;
//             let mut i = 0;
//             let level = 0;
//             for item in self.items.iter() {
//                 i = self.paint_item(
//                     ctx,
//                     min,
//                     max,
//                     line_height,
//                     width,
//                     level,
//                     i,
//                     index,
//                     item,
//                     env,
//                 );
//                 i += 1;
//                 if i > max {
//                     break;
//                 }
//             }
//         }
//     }
// }

// impl FileExplorerState {
//     pub fn new(window_id: WindowId, tab_id: WidgetId) -> FileExplorerState {
//         let items = Vec::new();
//         FileExplorerState {
//             window_id,
//             tab_id,
//             widget_id: WidgetId::next(),
//             items,
//             index: 0,
//             count: 0,
//             position: PanelPosition::LeftTop,
//         }
//     }
//
//     pub fn get_item(&mut self, index: usize) -> Option<&mut FileNodeItem> {
//         let mut i = 0;
//         for item in self.items.iter_mut() {
//             let result = get_item_children(i, index, item);
//             if result.0 == index {
//                 return result.1;
//             }
//             i = result.0 + 1;
//         }
//         None
//     }
//
//     pub fn update_count(&mut self) {
//         let mut count = 0;
//         for item in self.items.iter() {
//             count += get_item_count(item);
//         }
//         self.count = count;
//     }
//
//     pub fn run_command(
//         &mut self,
//         ctx: &mut EventCtx,
//         data: &mut LapceUIState,
//         count: Option<usize>,
//         command: LapceCommand,
//     ) -> LapceFocus {
//         self.request_paint(ctx);
//         match command {
//             LapceCommand::Up => {
//                 self.index = Movement::Up.update_index(
//                     self.index,
//                     self.count,
//                     count.unwrap_or(1),
//                     false,
//                 );
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::Down => {
//                 self.index = Movement::Down.update_index(
//                     self.index,
//                     self.count,
//                     count.unwrap_or(1),
//                     false,
//                 );
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::ListNext => {
//                 self.index = Movement::Down.update_index(
//                     self.index,
//                     self.count,
//                     count.unwrap_or(1),
//                     true,
//                 );
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::ListPrevious => {
//                 self.index = Movement::Up.update_index(
//                     self.index,
//                     self.count,
//                     count.unwrap_or(1),
//                     true,
//                 );
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::GotoLineDefaultFirst => {
//                 self.index = match count {
//                     Some(n) => Movement::Line(LinePosition::Line(n)),
//                     None => Movement::Line(LinePosition::First),
//                 }
//                 .update_index(self.index, self.count, 1, false);
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::GotoLineDefaultLast => {
//                 self.index = match count {
//                     Some(n) => Movement::Line(LinePosition::Line(n)),
//                     None => Movement::Line(LinePosition::Last),
//                 }
//                 .update_index(self.index, self.count, 1, false);
//                 LapceFocus::FileExplorer
//             }
//             LapceCommand::ListSelect => {
//                 let index = self.index;
//                 let state =
//                     LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
//                 let item = self.get_item(index).unwrap();
//                 let path_buf = item.path_buf.clone();
//                 let is_dir = item.is_dir;
//                 if !is_dir {
//                     state.editor_split.lock().open_file(
//                         ctx,
//                         data,
//                         path_buf.to_str().unwrap(),
//                     );
//                     LapceFocus::Editor
//                 } else {
//                     if item.read {
//                         item.open = !item.open;
//                         self.update_count();
//                         self.request_paint(ctx);
//                     } else {
//                         let mut item = item.clone();
//                         state.clone().proxy.lock().as_ref().unwrap().read_dir(
//                             &path_buf,
//                             Box::new(move |result| {
//                                 std::thread::spawn(move || {
//                                     let mut file_explorer =
//                                         state.file_explorer.lock();
//                                     let current_item = file_explorer.get_item(index);
//                                     if current_item != Some(&mut item) {
//                                         return;
//                                     }
//                                     let current_item = current_item.unwrap();
//                                     current_item.open = true;
//                                     current_item.read = true;
//                                     if let Ok(res) = result {
//                                         let resp: Result<
//                                             Vec<FileNodeItem>,
//                                             serde_json::Error,
//                                         > = serde_json::from_value(res);
//                                         if let Ok(items) = resp {
//                                             current_item.children = items;
//                                         }
//                                     }
//                                     file_explorer.update_count();
//                                     LAPCE_APP_STATE.submit_ui_command(
//                                         LapceUICommand::RequestPaint,
//                                         file_explorer.widget_id(),
//                                     );
//                                 });
//                             }),
//                         );
//                     }
//                     LapceFocus::FileExplorer
//                 }
//             }
//             _ => LapceFocus::FileExplorer,
//         }
//     }
//
//     fn request_paint(&self, ctx: &mut EventCtx) {
//         ctx.submit_command(Command::new(
//             LAPCE_UI_COMMAND,
//             LapceUICommand::RequestPaint,
//             Target::Widget(self.widget_id),
//         ));
//     }
//
//     fn paint_item(
//         &self,
//         ctx: &mut PaintCtx,
//         min: usize,
//         max: usize,
//         line_height: f64,
//         width: f64,
//         level: usize,
//         i: usize,
//         index: usize,
//         item: &FileNodeItem,
//         env: &druid::Env,
//     ) -> usize {
//         if i > max {
//             return i;
//         }
//         if i >= min && i <= max {
//             if i == index {
//                 if let Some(color) = LAPCE_APP_STATE.theme.get("selection") {
//                     ctx.fill(
//                         Rect::ZERO
//                             .with_origin(Point::new(
//                                 0.0,
//                                 i as f64 * line_height + line_height,
//                             ))
//                             .with_size(Size::new(width, line_height)),
//                         color,
//                     );
//                 }
//             }
//             let y = i as f64 * line_height + line_height;
//             let svg_y = y + 4.0;
//             let mut text_layout = TextLayout::<String>::from_text(
//                 item.path_buf.file_name().unwrap().to_str().unwrap(),
//             );
//             let padding = 15.0 * level as f64;
//             if item.is_dir {
//                 let icon_name = if item.open {
//                     "chevron-down.svg"
//                 } else {
//                     "chevron-right.svg"
//                 };
//                 let svg = SvgData::from_str(
//                     ICONS_DIR
//                         .get_file(icon_name)
//                         .unwrap()
//                         .contents_utf8()
//                         .unwrap(),
//                 )
//                 .unwrap();
//                 svg.to_piet(Affine::translate(Vec2::new(1.0 + padding, svg_y)), ctx);
//
//                 let icon_name = if item.open {
//                     "default_folder_opened.svg"
//                 } else {
//                     "default_folder.svg"
//                 };
//                 let svg = SvgData::from_str(
//                     ICONS_DIR
//                         .get_file(icon_name)
//                         .unwrap()
//                         .contents_utf8()
//                         .unwrap(),
//                 )
//                 .unwrap();
//                 let scale = 0.5;
//                 let affine = Affine::new([
//                     scale,
//                     0.0,
//                     0.0,
//                     scale,
//                     1.0 + 16.0 + padding,
//                     svg_y + 1.0,
//                 ]);
//                 svg.to_piet(affine, ctx);
//             } else {
//                 if let Some(exten) = item.path_buf.extension() {
//                     if let Some(exten) = exten.to_str() {
//                         let exten = match exten {
//                             "rs" => "rust",
//                             "md" => "markdown",
//                             "cc" => "cpp",
//                             _ => exten,
//                         };
//                     }
//                 }
//             }
//             text_layout.set_text_color(OldLapceTheme::EDITOR_FOREGROUND);
//             text_layout.rebuild_if_needed(ctx.text(), env);
//             text_layout.draw(ctx, Point::new(38.0 + padding, y + 3.0));
//         }
//         let mut i = i;
//         if item.open {
//             for item in &item.children {
//                 i = self.paint_item(
//                     ctx,
//                     min,
//                     max,
//                     line_height,
//                     width,
//                     level + 1,
//                     i + 1,
//                     index,
//                     item,
//                     env,
//                 );
//                 if i > max {
//                     return i;
//                 }
//             }
//         }
//         i
//     }
// }
//
// fn get_item_count(item: &FileNodeItem) -> usize {
//     let mut count = 1;
//     if item.open {
//         for child in item.children.iter() {
//             count += get_item_count(child);
//         }
//     }
//     count
// }
//
// fn get_item_children<'a>(
//     i: usize,
//     index: usize,
//     item: &'a mut FileNodeItem,
// ) -> (usize, Option<&'a mut FileNodeItem>) {
//     if i == index {
//         return (i, Some(item));
//     }
//     let mut i = i;
//     if item.open {
//         for child in item.children.iter_mut() {
//             let (new_index, node) = get_item_children(i + 1, index, child);
//             if new_index == index {
//                 return (new_index, node);
//             }
//             i = new_index;
//         }
//     }
//     (i, None)
// }

// pub struct FileExplorer {
//     window_id: WindowId,
//     tab_id: WidgetId,
// }

// impl FileExplorer {
//     pub fn new(window_id: WindowId, tab_id: WidgetId) -> FileExplorer {
//         FileExplorer { window_id, tab_id }
//     }
//
//     fn paint_item(
//         &self,
//         ctx: &mut druid::PaintCtx,
//         min: usize,
//         max: usize,
//         line_height: f64,
//         width: f64,
//         level: usize,
//         i: usize,
//         index: usize,
//         item: &FileNodeItem,
//         env: &druid::Env,
//     ) -> usize {
//         if i > max {
//             return i;
//         }
//         if i >= min && i <= max {
//             if i == index {
//                 ctx.fill(
//                     Rect::ZERO
//                         .with_origin(Point::new(0.0, i as f64 * line_height))
//                         .with_size(Size::new(width, line_height)),
//                     &env.get(LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND),
//                 );
//             }
//             let y = i as f64 * line_height;
//             let svg_y = y + 4.0;
//             let mut text_layout = TextLayout::<String>::from_text(
//                 item.path_buf.file_name().unwrap().to_str().unwrap(),
//             );
//             let padding = 15.0 * level as f64;
//             if item.is_dir {
//                 let icon_name = if item.open {
//                     "chevron-down.svg"
//                 } else {
//                     "chevron-right.svg"
//                 };
//                 let svg = SvgData::from_str(
//                     ICONS_DIR
//                         .get_file(icon_name)
//                         .unwrap()
//                         .contents_utf8()
//                         .unwrap(),
//                 )
//                 .unwrap();
//                 svg.to_piet(Affine::translate(Vec2::new(1.0 + padding, svg_y)), ctx);
//
//                 let icon_name = if item.open {
//                     "default_folder_opened.svg"
//                 } else {
//                     "default_folder.svg"
//                 };
//                 let svg = SvgData::from_str(
//                     ICONS_DIR
//                         .get_file(icon_name)
//                         .unwrap()
//                         .contents_utf8()
//                         .unwrap(),
//                 )
//                 .unwrap();
//                 let scale = 0.5;
//                 let affine = Affine::new([
//                     scale,
//                     0.0,
//                     0.0,
//                     scale,
//                     1.0 + 16.0 + padding,
//                     svg_y + 1.0,
//                 ]);
//                 svg.to_piet(affine, ctx);
//             } else {
//                 if let Some(exten) = item.path_buf.extension() {
//                     if let Some(exten) = exten.to_str() {
//                         let exten = match exten {
//                             "rs" => "rust",
//                             "md" => "markdown",
//                             "cc" => "cpp",
//                             _ => exten,
//                         };
//                         if let Some((svg, svg_tree)) = file_svg(exten) {
//                             let svg_size = svg_tree_size(&svg_tree);
//                             let scale = 13.0 / svg_size.height;
//                             let affine = Affine::new([
//                                 scale,
//                                 0.0,
//                                 0.0,
//                                 scale,
//                                 1.0 + 18.0 + padding,
//                                 svg_y + 2.0,
//                             ]);
//                             svg.to_piet(affine, ctx);
//                         }
//                     }
//                 }
//             }
//             text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
//             text_layout.rebuild_if_needed(ctx.text(), env);
//             text_layout.draw(ctx, Point::new(38.0 + padding, y + 3.0));
//         }
//         let mut i = i;
//         if item.open {
//             for item in &item.children {
//                 i = self.paint_item(
//                     ctx,
//                     min,
//                     max,
//                     line_height,
//                     width,
//                     level + 1,
//                     i + 1,
//                     index,
//                     item,
//                     env,
//                 );
//                 if i > max {
//                     return i;
//                 }
//             }
//         }
//         i
//     }
// }
//
// impl Widget<LapceUIState> for FileExplorer {
//     fn event(
//         &mut self,
//         ctx: &mut EventCtx,
//         event: &Event,
//         data: &mut LapceUIState,
//         env: &druid::Env,
//     ) {
//         match event {
//             Event::Command(cmd) => match cmd {
//                 _ if cmd.is(LAPCE_UI_COMMAND) => {
//                     let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
//                     match command {
//                         LapceUICommand::RequestLayout => {
//                             ctx.request_layout();
//                         }
//                         LapceUICommand::RequestPaint => {
//                             ctx.request_paint();
//                         }
//                         _ => (),
//                     }
//                 }
//                 _ => (),
//             },
//             _ => (),
//         }
//     }
//
//     fn lifecycle(
//         &mut self,
//         ctx: &mut druid::LifeCycleCtx,
//         event: &druid::LifeCycle,
//         data: &LapceUIState,
//         env: &druid::Env,
//     ) {
//     }
//
//     fn update(
//         &mut self,
//         ctx: &mut druid::UpdateCtx,
//         old_data: &LapceUIState,
//         data: &LapceUIState,
//         env: &druid::Env,
//     ) {
//         // let file_explorer = &data.file_explorer;
//         // let old_file_explorer = &old_data.file_explorer;
//         // if file_explorer.index != old_file_explorer.index {
//         //     ctx.request_paint();
//         // }
//     }
//
//     fn layout(
//         &mut self,
//         ctx: &mut druid::LayoutCtx,
//         bc: &druid::BoxConstraints,
//         data: &LapceUIState,
//         env: &druid::Env,
//     ) -> druid::Size {
//         bc.max()
//     }
//
//     fn paint(
//         &mut self,
//         ctx: &mut druid::PaintCtx,
//         data: &LapceUIState,
//         env: &druid::Env,
//     ) {
//         let rects = ctx.region().rects().to_vec();
//         let size = ctx.size();
//         let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
//         let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
//         let file_explorer = state.file_explorer.lock();
//         let width = size.width;
//         let index = file_explorer.index;
//
//         for rect in rects {
//             if let Some(background) = LAPCE_APP_STATE.theme.get("background") {
//                 ctx.fill(rect, background);
//             }
//             let min = (rect.y0 / line_height).floor() as usize;
//             let max = (rect.y1 / line_height) as usize + 1;
//             let mut i = 0;
//             let level = 0;
//             for item in file_explorer.items.iter() {
//                 i = self.paint_item(
//                     ctx,
//                     min,
//                     max,
//                     line_height,
//                     width,
//                     level,
//                     i,
//                     index,
//                     item,
//                     env,
//                 );
//                 i += 1;
//                 if i > max {
//                     break;
//                 }
//             }
//         }
//     }
// }
