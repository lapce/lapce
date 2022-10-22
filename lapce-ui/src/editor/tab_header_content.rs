use std::{
    collections::HashSet, iter::Iterator, ops::Sub, path::PathBuf, str::FromStr,
    sync::Arc,
};

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontStyle, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId,
};
use im::HashMap;
use lapce_core::{command::FocusCommand, meta};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{DragContent, EditorTabChild, LapceTabData},
    document::BufferContent,
    editor::TabRect,
};

use crate::editor::tab::TabRectRenderer;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseAction {
    Drag,
    CloseViaIcon,
    CloseViaMiddleClick,
}

pub struct LapceEditorTabHeaderContent {
    pub widget_id: WidgetId,
    pub rects: Vec<TabRect>,
    mouse_pos: Option<Point>,
    mouse_down_target: Option<(MouseAction, usize)>,
    dedup_paths: HashMap<PathBuf, PathBuf>,
}

impl LapceEditorTabHeaderContent {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            rects: Vec::new(),
            mouse_pos: None,
            mouse_down_target: None,
            dedup_paths: HashMap::default(),
        }
    }

    fn tab_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for tab_idx in 0..self.rects.len() {
            if self.is_tab_hit(tab_idx, mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn cancel_pending_drag(&mut self, data: &mut LapceTabData) {
        if data.drag.is_none() {
            return;
        }
        *Arc::make_mut(&mut data.drag) = None;
    }

    fn is_close_icon_hit(&self, tab: usize, mouse_pos: Point) -> bool {
        self.rects[tab].close_rect.contains(mouse_pos)
    }

    fn is_tab_hit(&self, tab: usize, mouse_pos: Point) -> bool {
        self.rects[tab].rect.contains(mouse_pos)
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        for tab_idx in 0..self.rects.len() {
            if !self.is_tab_hit(tab_idx, mouse_event.pos) {
                continue;
            }

            if mouse_event.button.is_left() {
                if self.is_close_icon_hit(tab_idx, mouse_event.pos) {
                    self.mouse_down_target =
                        Some((MouseAction::CloseViaIcon, tab_idx));
                    return;
                }

                let editor_tab = data
                    .main_split
                    .editor_tabs
                    .get_mut(&self.widget_id)
                    .unwrap();
                let editor_tab = Arc::make_mut(editor_tab);

                if *data.main_split.active_tab != Some(self.widget_id)
                    || editor_tab.active != tab_idx
                {
                    data.main_split.active_tab = Arc::new(Some(self.widget_id));
                    editor_tab.active = tab_idx;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(editor_tab.children[tab_idx].widget_id()),
                    ));
                }
                self.mouse_pos = Some(mouse_event.pos);
                self.mouse_down_target = Some((MouseAction::Drag, tab_idx));

                ctx.request_paint();

                return;
            }

            if mouse_event.button.is_middle() {
                self.mouse_down_target =
                    Some((MouseAction::CloseViaMiddleClick, tab_idx));
                return;
            }
        }
    }

    fn mouse_move(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        self.mouse_pos = Some(mouse_event.pos);
        if self.tab_hit_test(mouse_event) {
            ctx.set_cursor(&druid::Cursor::Pointer);
        } else {
            ctx.clear_cursor();
        }

        if !mouse_event.buttons.contains(MouseButton::Left) {
            // If drag data exists, mouse was released outside of the view.
            self.cancel_pending_drag(data);
            return;
        }

        if data.drag.is_none() {
            if let Some((MouseAction::Drag, target)) = self.mouse_down_target {
                self.mouse_down_target = None;

                let editor_tab =
                    data.main_split.editor_tabs.get(&self.widget_id).unwrap();
                let tab_rect = &self.rects[target];

                let offset =
                    mouse_event.pos.to_vec2() - tab_rect.rect.origin().to_vec2();
                *Arc::make_mut(&mut data.drag) = Some((
                    offset,
                    mouse_event.window_pos.to_vec2(),
                    DragContent::EditorTab(
                        editor_tab.widget_id,
                        target,
                        editor_tab.children[target].clone(),
                        Box::new(tab_rect.clone()),
                    ),
                ));
            }
        }
    }

    fn mouse_up(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        let editor_tab = data
            .main_split
            .editor_tabs
            .get_mut(&self.widget_id)
            .unwrap();

        let mut close_tab = |tab_idx: usize, was_active: bool| {
            if was_active {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ActiveFileChanged { path: None },
                    Target::Widget(data.file_explorer.widget_id),
                ));
            }

            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::SplitClose),
                    data: None,
                },
                Target::Widget(editor_tab.children[tab_idx].widget_id()),
            ));
        };

        match self.mouse_down_target.take() {
            // Was the left button released on the close icon that started the close?
            Some((MouseAction::CloseViaIcon, target))
                if self.is_close_icon_hit(target, mouse_event.pos)
                    && mouse_event.button.is_left() =>
            {
                close_tab(target, target == editor_tab.active);
            }

            // Was the middle button released on the tab that started the close?
            Some((MouseAction::CloseViaMiddleClick, target))
                if self.is_tab_hit(target, mouse_event.pos)
                    && mouse_event.button.is_middle() =>
            {
                close_tab(target, target == editor_tab.active);
            }

            None if mouse_event.button.is_left() => {
                let mouse_index = self.drag_target_idx(mouse_event.pos);
                self.handle_drag(mouse_index, ctx, data)
            }

            _ => {}
        }
    }

    fn after_last_tab_index(&self) -> usize {
        self.rects.len()
    }

    fn drag_target_idx(&self, mouse_pos: Point) -> usize {
        for (i, tab_rect) in self.rects.iter().enumerate() {
            if tab_rect.rect.contains(mouse_pos) {
                return if mouse_pos.x
                    <= tab_rect.rect.x0 + tab_rect.rect.size().width / 2.0
                {
                    i
                } else {
                    i + 1
                };
            }
        }
        self.after_last_tab_index()
    }

    fn handle_drag(
        &mut self,
        mouse_index: usize,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
    ) {
        if let Some((_, _, DragContent::EditorTab(from_id, from_index, child, _))) =
            Arc::make_mut(&mut data.drag).take()
        {
            let editor_tab = data
                .main_split
                .editor_tabs
                .get(&self.widget_id)
                .unwrap()
                .clone();

            if editor_tab.widget_id == from_id {
                // Take the removed tab into account.
                let mouse_index = if mouse_index > from_index {
                    mouse_index.saturating_sub(1)
                } else {
                    mouse_index
                };

                if mouse_index != from_index {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EditorTabSwap(from_index, mouse_index),
                        Target::Widget(editor_tab.widget_id),
                    ));
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(child.widget_id()),
                    ));
                }
                return;
            }

            let mut child = child;
            child.set_editor_tab(data, editor_tab.widget_id);
            let editor_tab = data
                .main_split
                .editor_tabs
                .get_mut(&self.widget_id)
                .unwrap();
            let editor_tab = Arc::make_mut(editor_tab);
            editor_tab.children.insert(mouse_index, child.clone());
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::EditorTabAdd(mouse_index, child.clone()),
                Target::Widget(editor_tab.widget_id),
            ));
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(child.widget_id()),
            ));
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::EditorTabRemove(from_index, false, false),
                Target::Widget(from_id),
            ));
        }
    }

    pub fn update_dedup_paths(&mut self, data: &LapceTabData) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        // Collect all the filenames we currently have
        let mut dup_filenames: HashMap<&str, Vec<PathBuf>> = HashMap::default();
        for child in &editor_tab.children {
            if let EditorTabChild::Editor(view_id, _, _) = child {
                let editor = data.main_split.editors.get(view_id).unwrap();
                if let BufferContent::File(path) = &editor.content {
                    if let Some(file_name) = path.file_name() {
                        dup_filenames
                            .entry(file_name.to_str().unwrap())
                            .and_modify(|v| v.push(path.clone()))
                            .or_insert(vec![path.clone()]);
                    }
                }
            }
        }

        // Clear dedup paths so that we dont store closed files
        // TODO: Can be optimized by listening to close file events?
        self.dedup_paths.clear();

        // Resolve each group of duplicates and insert into `self.dedup_paths`.
        for (_, dup_paths) in dup_filenames.iter().filter(|v| v.1.len() > 1) {
            for (original_path, truncated_path) in
                dup_paths.iter().zip(get_truncated_path(dup_paths).iter())
            {
                self.dedup_paths.insert(
                    original_path.to_path_buf(),
                    truncated_path.to_path_buf(),
                );
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorTabHeaderContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_move(ctx, data, mouse_event);
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, data, mouse_event);
            }
            Event::MouseUp(mouse_event) => {
                self.mouse_up(ctx, data, mouse_event);
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
        _ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let old_editor_tab = old_data
            .main_split
            .editor_tabs
            .get(&self.widget_id)
            .unwrap();

        if !editor_tab.children.ptr_eq(&old_editor_tab.children) {
            self.update_dedup_paths(data);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let height = bc.max().height;

        self.rects.clear();
        let mut x = 0.0;

        for child in editor_tab.children.iter() {
            let mut text = "".to_string();
            let mut svg = data.config.ui_svg(LapceIcons::FILE);
            let mut svg_color = Some(
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
            );
            let mut file_path = None;
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        (svg, svg_color) = data.config.file_svg(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                                if let Some(dedup_path) = self.dedup_paths.get(path)
                                {
                                    file_path = Some(
                                        dedup_path.to_string_lossy().to_string(),
                                    );
                                }
                            }
                        }
                    } else if let BufferContent::Scratch(..) = &editor.content {
                        text = editor.content.file_name().to_string();
                    }
                }
                EditorTabChild::Settings { .. } => {
                    text = format!("Settings (ver. {})", *meta::VERSION);
                }
                EditorTabChild::Plugin { volt_name, .. } => {
                    text = format!("Plugin: {volt_name}");
                }
            }
            let font_size = data.config.ui.font_size() as f64;
            let text_layout = ctx
                .text()
                .new_text_layout(text)
                .font(data.config.ui.font_family(), font_size)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let path_layout = file_path.map(|f| {
                ctx.text()
                    .new_text_layout(f)
                    .default_attribute(FontStyle::Italic)
                    .font(data.config.ui.font_family(), font_size.sub(2.0).max(1.0))
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap()
            });
            let text_size = text_layout.size()
                + path_layout.as_ref().map(|p| p.size()).unwrap_or(Size::ZERO);
            let width =
                (text_size.width + height + (height - font_size) / 2.0 + font_size)
                    .max(data.config.ui.tab_min_width() as f64);
            let close_size = 24.0;
            let inflate = (height - close_size) / 2.0;
            let tab_rect = TabRect {
                svg,
                svg_color: svg_color.cloned(),
                rect: Size::new(width, height)
                    .to_rect()
                    .with_origin(Point::new(x, 0.0)),
                close_rect: Size::new(height, height)
                    .to_rect()
                    .with_origin(Point::new(x + width - height, 0.0))
                    .inflate(-inflate, -inflate),
                text_layout,
                path_layout,
            };
            x += width;
            self.rects.push(tab_rect);
        }

        Size::new(bc.max().width.max(x), height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();

        for (tab_idx, tab_rect) in self.rects.iter().enumerate() {
            tab_rect.paint(ctx, data, self.widget_id, tab_idx, size, self.mouse_pos);
        }

        if ctx.is_hot() && data.is_drag_editor() {
            // SAFETY: unwrap here is safe because `ctx.is_hot` is true if mouse is hovered over it.
            let mouse_index = self.drag_target_idx(self.mouse_pos.unwrap());

            let tab_rect;
            let x = if mouse_index == self.after_last_tab_index() {
                tab_rect = self.rects.last().unwrap();
                tab_rect.rect.x1
            } else {
                tab_rect = &self.rects[mouse_index];
                if mouse_index == 0 {
                    tab_rect.rect.x0 + 2.0
                } else {
                    tab_rect.rect.x0
                }
            };
            ctx.stroke(
                Line::new(
                    Point::new(x, tab_rect.rect.y0),
                    Point::new(x, tab_rect.rect.y1),
                ),
                data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                4.0,
            );
        }
    }
}

fn get_truncated_path(full_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut skip_left = 0;
    'stop_left: loop {
        if full_paths
            .iter()
            .map(|p| p.iter().nth(skip_left))
            .collect::<Option<HashSet<_>>>()
            .map(|h| h.len() == 1)
            .unwrap_or(false)
        {
            skip_left += 1;
        } else {
            break 'stop_left;
        }
    }

    let mut skip_right = 0;
    'stop_right: loop {
        if full_paths
            .iter()
            .map(|p| p.iter().rev().nth(skip_right))
            .collect::<Option<HashSet<_>>>()
            .map(|h| h.len() == 1)
            .unwrap_or(false)
        {
            skip_right += 1;
        } else {
            break 'stop_right;
        }
    }

    let skip_left = if skip_left == 1 { 0 } else { skip_left };

    let truncated_paths = full_paths
        .iter()
        .map(|p| {
            let length = p.iter().count();
            let mut result = p
                .iter()
                .skip(skip_left)
                .take(length.saturating_sub(skip_left).saturating_sub(skip_right))
                .collect::<PathBuf>();

            if skip_left > 0 {
                result = PathBuf::from_str("...").unwrap().join(result);
            }

            if skip_right > 1 {
                result.push("...")
            }

            result
        })
        .collect::<Vec<_>>();

    truncated_paths
}

#[allow(unused_imports)]
mod test {
    use std::{path::PathBuf, str::FromStr};

    use druid::WidgetId;

    use crate::editor::tab_header_content::{
        get_truncated_path, LapceEditorTabHeaderContent,
    };

    #[test]
    fn test_all_truncated_paths() {
        let f1 = PathBuf::from("/home/user/myproject/folder1/file.rs");
        let f2 = PathBuf::from("/home/user/myproject/folder2/file.rs");
        let f3 = PathBuf::from("/file.rs");
        let f4 = PathBuf::from("/home/user/proj/file.rs");
        let f5 = PathBuf::from("/home/user/toolongprojectshouldtruncate/file.rs");
        let f6 = PathBuf::from("/home/user/myproject/file.rs");

        let result = get_truncated_path(&[f1, f2, f3, f4, f5, f6]);

        assert_eq!(
            result[0],
            PathBuf::from_str("/home/user/myproject/folder1").unwrap()
        );
        assert_eq!(
            result[1],
            PathBuf::from_str("/home/user/myproject/folder2").unwrap()
        );
        assert_eq!(result[2], PathBuf::from_str("/").unwrap());
        assert_eq!(result[3], PathBuf::from_str("/home/user/proj").unwrap());
        assert_eq!(
            result[4],
            PathBuf::from_str("/home/user/toolongprojectshouldtruncate").unwrap()
        );
        assert_eq!(
            result[5],
            PathBuf::from_str("/home/user/myproject/").unwrap()
        );
    }

    #[test]
    fn test_almost_same_paths() {
        let f1 = PathBuf::from("/home/user/myproject/folder1/file.rs");
        let f2 = PathBuf::from("/home/user/myproject/folder2/file.rs");

        let result = get_truncated_path(&[f1, f2]);

        assert_eq!(result[0], PathBuf::from_str(".../folder1").unwrap());
        assert_eq!(result[1], PathBuf::from_str(".../folder2").unwrap());
    }
}
