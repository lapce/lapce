use std::{cmp::Ordering, iter::Iterator, path::PathBuf, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontStyle, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId,
};
use hashbrown::HashMap;
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{DragContent, EditorTabChild, LapceTabData},
    document::BufferContent,
    editor::TabRect,
    proxy::VERSION,
};

use crate::{
    editor::tab::TabRectRenderer,
    svg::{file_svg, get_svg},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseAction {
    Drag,
    CloseViaIcon,
    CloseViaMiddleClick,
}

pub struct LapceEditorTabHeaderContent {
    pub widget_id: WidgetId,
    pub rects: Vec<TabRect>,
    mouse_pos: Point,
    mouse_down_target: Option<(MouseAction, usize)>,
}

impl LapceEditorTabHeaderContent {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            rects: Vec::new(),
            mouse_pos: Point::ZERO,
            mouse_down_target: None,
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
                self.mouse_pos = mouse_event.pos;
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
        self.mouse_pos = mouse_event.pos;
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

    fn get_effective_path(
        &mut self,
        workspace_path: Option<PathBuf>,
        file_path: &PathBuf,
    ) -> String {
        let workspace_path = match workspace_path {
            Some(p) => p,
            None => {
                // file_path.truncate(20);
                let file_path = file_path
                    .to_string_lossy()
                    .char_indices()
                    .filter(|(i, _)| *i < 20)
                    .map(|(_, ch)| ch)
                    .collect::<String>();
                return file_path;
            }
        };

        // file is a workspace file: we can truncate to workspace folders
        if file_path.starts_with(&workspace_path) {
            let mut reversed_path: Vec<String> = Vec::<String>::new();
            // let mut parent = file_path.parent();
            let mut file_path_mut = file_path.clone();

            // We dont need to keep the file name, only the parents.
            file_path_mut.pop();

            while file_path_mut != workspace_path {
                let file_name = file_path_mut.file_name().unwrap();
                let file_name_str = file_name.to_str().unwrap().to_string();

                reversed_path.push(file_name_str);
                if !file_path_mut.pop() {
                    break;
                }
            }

            // File is at workspace root
            if reversed_path.len() == 0 {
                return "./".to_string();
            }

            reversed_path.reverse();
            let mut new_path = PathBuf::new();

            for i in reversed_path.iter() {
                new_path.push(i);
            }

            return new_path.to_str().unwrap().to_string();
        }

        // file is not a workspace file: we keep root but we try to declutter
        // as much as possible by clamping to 20 character length
        let components = match file_path.parent() {
            Some(v) => v.components(),
            None => return "/".to_string(),
        };

        let mut new_path = PathBuf::new();

        for c in components {
            if new_path.as_os_str().len() + c.as_os_str().len() > 20 {
                break;
            }

            new_path.push(c);
        }

        return new_path.to_str().unwrap().to_string();
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
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let _child_min_width = 200.0;
        let height = bc.max().height;

        self.rects.clear();
        let mut x = 0.0;
        let mut name_count: HashMap<String, i32> = HashMap::new();
        for (_i, child) in editor_tab.children.iter().enumerate() {
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        if let Some(file_name) = path.file_name() {
                            let name = file_name.to_str().unwrap();
                            let nb = match name_count.get(name) {
                                Some(name) => *name,
                                None => 0,
                            };

                            name_count.insert(String::from(name), nb + 1);
                        }
                    }
                }
                EditorTabChild::Settings(_, _) => {}
            }
        }

        for (_i, child) in editor_tab.children.iter().enumerate() {
            let mut text = "".to_string();
            let mut svg = get_svg("default_file.svg").unwrap();
            let mut file_path = "".to_string();
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        (svg, _) = file_svg(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                                let nb = name_count
                                    .get(file_name.to_str().unwrap())
                                    .unwrap();
                                if *nb > 1 {
                                    file_path = format!(
                                        " {}",
                                        self.get_effective_path(
                                            data.workspace.path.clone(),
                                            path
                                        )
                                    );
                                }
                            }
                        }
                    } else if let BufferContent::Scratch(..) = &editor.content {
                        text = editor.content.file_name().to_string();
                    }
                }
                EditorTabChild::Settings { .. } => {
                    text = format!("Settings (ver. {})", *VERSION);
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
            let path_layout = ctx
                .text()
                .new_text_layout(file_path)
                .default_attribute(FontStyle::Italic)
                .font(data.config.ui.font_family(), font_size)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size() + path_layout.size();
            let width =
                (text_size.width + height + (height - font_size) / 2.0 + font_size)
                    .max(data.config.ui.tab_min_width() as f64);
            let close_size = 24.0;
            let inflate = (height - close_size) / 2.0;
            let tab_rect = TabRect {
                svg,
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
            let mouse_index = self.drag_target_idx(self.mouse_pos);

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

mod test {
    use super::*;

    #[test]
    fn test_effective_path() {
        let workspace_path = Some(PathBuf::from("/home/user/myproject/"));
        let f1 = PathBuf::from("/home/user/myproject/folder1/file.rs");
        let f2 = PathBuf::from("/home/user/myproject/folder2/file.rs");
        let f3 = PathBuf::from("/file.rs");
        let f4 = PathBuf::from("/home/user/proj/file.rs");
        let f5 = PathBuf::from("/home/user/toolongprojectshouldtruncate/file.rs");
        let f6 = PathBuf::from("/home/user/myproject/file.rs");
        let f7 = PathBuf::from(
            "/home/user/myproject/🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆",
        );
        let f8 = PathBuf::from(
            "/home/user/proj/🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆",
        );
        let f9 = PathBuf::from(
            "/home/user/myproject/🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆/🎉🎉🎉🎉🎉🎉🎉/🎊",
        );

        let mut tab = LapceEditorTabHeaderContent::new(WidgetId::next());

        let r1 = tab.get_effective_path(workspace_path.clone(), &f1);
        let r2 = tab.get_effective_path(workspace_path.clone(), &f2);
        let r3 = tab.get_effective_path(workspace_path.clone(), &f3);
        let r4 = tab.get_effective_path(workspace_path.clone(), &f4);
        let r5 = tab.get_effective_path(workspace_path.clone(), &f5);
        let r6 = tab.get_effective_path(workspace_path.clone(), &f6);
        let r7 = tab.get_effective_path(workspace_path.clone(), &f7);
        let r8 = tab.get_effective_path(workspace_path.clone(), &f8);
        let r9 = tab.get_effective_path(workspace_path.clone(), &f9);

        assert_eq!(r1, "folder1");
        assert_eq!(r2, "folder2");
        assert_eq!(r3, "/");
        assert_eq!(r4, "/home/user/proj");
        assert_eq!(r5, "/home/user");
        assert_eq!(r6, "./");
        assert_eq!(r7, "./");
        assert_eq!(r8, "/home/user/proj");
        assert_eq!(r9, "🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆🎆/🎉🎉🎉🎉🎉🎉🎉");
    }
}
