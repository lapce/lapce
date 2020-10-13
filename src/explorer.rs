use std::str::FromStr;
use std::{cmp, path::PathBuf};

use druid::{
    piet::PietTextLayout, widget::SvgData, Affine, EventCtx, Point, Rect,
    RenderContext, Size, TextLayout, Vec2, Widget,
};
use include_dir::{include_dir, Dir};

use crate::{
    command::LapceCommand, editor::EditorSplitState, movement::LinePosition,
    movement::Movement, state::LapceFocus, state::LapceState,
    theme::LapceTheme,
};

const ICONS_DIR: Dir = include_dir!("icons");

#[derive(Clone)]
pub struct FileExplorerState {
    cwd: PathBuf,
    items: Vec<FileNodeItem>,
    index: usize,
}

#[derive(Eq, PartialEq, Ord, Clone)]
pub struct FileNodeItem {
    path_buf: PathBuf,
}

impl std::cmp::PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        let self_dir = self.path_buf.is_dir();
        let other_dir = other.path_buf.is_dir();
        if self_dir && !other_dir {
            return Some(cmp::Ordering::Less);
        }
        if !self_dir && other_dir {
            return Some(cmp::Ordering::Greater);
        }

        let self_file_name =
            self.path_buf.file_name()?.to_str()?.to_lowercase();
        let other_file_name =
            other.path_buf.file_name()?.to_str()?.to_lowercase();
        if self_file_name.starts_with(".") && !other_file_name.starts_with(".")
        {
            return Some(cmp::Ordering::Less);
        }
        if !self_file_name.starts_with(".") && other_file_name.starts_with(".")
        {
            return Some(cmp::Ordering::Greater);
        }
        self_file_name.partial_cmp(&other_file_name)
    }
}

impl FileExplorerState {
    pub fn new() -> FileExplorerState {
        let mut items = Vec::new();
        let cwd = std::env::current_dir().unwrap();
        // items.push(FileNodeItem {
        //     path_buf: std::env::current_dir().unwrap(),
        // });
        for entry in std::fs::read_dir(&cwd).unwrap() {
            items.push(FileNodeItem {
                path_buf: entry.unwrap().path(),
            });
        }
        items.sort();
        FileExplorerState {
            cwd,
            items,
            index: 0,
        }
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        editor_split: &mut EditorSplitState,
        count: Option<usize>,
        command: LapceCommand,
    ) -> LapceFocus {
        match command {
            LapceCommand::Up => {
                self.index = Movement::Up.update_index(
                    self.index,
                    self.items.len(),
                    count.unwrap_or(1),
                    false,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::Down => {
                self.index = Movement::Down.update_index(
                    self.index,
                    self.items.len(),
                    count.unwrap_or(1),
                    false,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::ListNext => {
                self.index = Movement::Down.update_index(
                    self.index,
                    self.items.len(),
                    count.unwrap_or(1),
                    true,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::ListPrevious => {
                self.index = Movement::Up.update_index(
                    self.index,
                    self.items.len(),
                    count.unwrap_or(1),
                    true,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::GotoLineDefaultFirst => {
                self.index = match count {
                    Some(n) => Movement::Line(LinePosition::Line(n)),
                    None => Movement::Line(LinePosition::First),
                }
                .update_index(
                    self.index,
                    self.items.len(),
                    1,
                    false,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::GotoLineDefaultLast => {
                self.index = match count {
                    Some(n) => Movement::Line(LinePosition::Line(n)),
                    None => Movement::Line(LinePosition::Last),
                }
                .update_index(
                    self.index,
                    self.items.len(),
                    1,
                    false,
                );
                LapceFocus::FileExplorer
            }
            LapceCommand::ListSelect => {
                let path_buf = &self.items[self.index].path_buf;
                if !path_buf.is_dir() {
                    editor_split.open_file(ctx, path_buf.to_str().unwrap());
                    LapceFocus::Editor
                } else {
                    LapceFocus::FileExplorer
                }
            }
            _ => LapceFocus::FileExplorer,
        }
    }
}

pub struct FileExplorer {}

impl FileExplorer {
    pub fn new() -> FileExplorer {
        FileExplorer {}
    }
}

impl Widget<LapceState> for FileExplorer {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut LapceState,
        env: &druid::Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceState,
        env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &druid::Env,
    ) {
        let file_explorer = &data.file_explorer;
        let old_file_explorer = &old_data.file_explorer;
        if file_explorer.index != old_file_explorer.index {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceState,
        env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceState,
        env: &druid::Env,
    ) {
        let rects = ctx.region().rects().to_vec();
        let size = ctx.size();
        for rect in rects {
            if let Some(background) = data.theme.get("background") {
                ctx.fill(rect, background);
            }
        }
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        for (i, item) in data.file_explorer.items.iter().enumerate() {
            if i == data.file_explorer.index {
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, i as f64 * line_height))
                        .with_size(Size::new(size.width, line_height)),
                    &env.get(LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND),
                );
            }
            let y = i as f64 * line_height;
            let mut text_layout = TextLayout::new(
                item.path_buf.file_name().unwrap().to_str().unwrap(),
            );
            if item.path_buf.is_dir() {
                let svg = SvgData::from_str(
                    ICONS_DIR
                        .get_file("chevron-right.svg")
                        .unwrap()
                        .contents_utf8()
                        .unwrap(),
                )
                .unwrap();
                svg.to_piet(Affine::translate(Vec2::new(1.0, y)), ctx);
            }
            text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
            text_layout.rebuild_if_needed(ctx.text(), env);
            text_layout.draw(ctx, Point::new(15.0, y));
        }
    }
}
