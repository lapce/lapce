use std::{iter::Iterator, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId,
};
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
        ctx.request_paint();

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
                    DragContent::EditorTab(
                        editor_tab.widget_id,
                        target,
                        editor_tab.children[target].clone(),
                        tab_rect.clone(),
                    ),
                ));
            }
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
        if let Some((_, DragContent::EditorTab(from_id, from_index, child, _))) =
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
        for (_i, child) in editor_tab.children.iter().enumerate() {
            let mut text = "".to_string();
            let mut svg = get_svg("default_file.svg").unwrap();
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        svg = file_svg(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                            }
                        }
                    } else if let BufferContent::Scratch(..) = &editor.content {
                        text = editor.content.file_name().to_string();
                    }
                }
                EditorTabChild::Settings(_, _) => {
                    text = format!("Settings v{}", VERSION);
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
            let text_size = text_layout.size();
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
            };
            x += width;
            self.rects.push(tab_rect);
        }

        Size::new(bc.max().width.max(x), height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();

        for (i, tab_rect) in self.rects.iter().enumerate() {
            tab_rect.paint(ctx, data, self.widget_id, i, size, self.mouse_pos);
        }

        if ctx.is_hot() && data.drag.is_some() {
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
