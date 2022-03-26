use std::cmp::Ordering;
use std::{iter::Iterator, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetId,
};
use lapce_data::{
    buffer::BufferContent,
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{DragContent, EditorTabChild, LapceTabData},
    editor::TabRect,
};

use crate::editor::tab::TabRectRenderer;
use crate::svg::{file_svg_new, get_svg};

pub struct LapceEditorTabHeaderContent {
    pub widget_id: WidgetId,
    pub rects: Vec<TabRect>,
    mouse_pos: Point,
}

impl LapceEditorTabHeaderContent {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            rects: Vec::new(),
            mouse_pos: Point::ZERO,
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for tab_rect in self.rects.iter() {
            if tab_rect.close_rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        for (i, tab_rect) in self.rects.iter().enumerate() {
            // Only react to left button clicks
            if mouse_event.button.is_left()
                && tab_rect.rect.contains(mouse_event.pos)
            {
                let editor_tab = data
                    .main_split
                    .editor_tabs
                    .get_mut(&self.widget_id)
                    .unwrap();
                let editor_tab = Arc::make_mut(editor_tab);
                if tab_rect.close_rect.contains(mouse_event.pos) {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EditorTabRemove(i, true, true),
                        Target::Widget(self.widget_id),
                    ));
                    return;
                }
                if editor_tab.active != i {
                    editor_tab.active = i;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(editor_tab.children[i].widget_id()),
                    ));
                }

                let offset =
                    mouse_event.pos.to_vec2() - tab_rect.rect.origin().to_vec2();
                *Arc::make_mut(&mut data.drag) = Some((
                    offset,
                    DragContent::EditorTab(
                        editor_tab.widget_id,
                        i,
                        editor_tab.children[i].clone(),
                        tab_rect.clone(),
                    ),
                ));
                return;
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
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, data, mouse_event);
            }
            Event::MouseUp(mouse_event) => {
                if let Some((_, drag_content)) = data.drag.clone().as_ref() {
                    match drag_content {
                        DragContent::EditorTab(from_id, from_index, child, _) => {
                            let mut mouse_index = self.rects.len();
                            for (i, tab_rect) in self.rects.iter().enumerate() {
                                if tab_rect.rect.contains(mouse_event.pos) {
                                    if mouse_event.pos.x
                                        <= tab_rect.rect.x0
                                            + tab_rect.rect.size().width / 2.0
                                    {
                                        mouse_index = i;
                                    } else {
                                        mouse_index = i + 1;
                                    }
                                    break;
                                }
                            }
                            let editor_tab = data
                                .main_split
                                .editor_tabs
                                .get(&self.widget_id)
                                .unwrap()
                                .clone();
                            if &editor_tab.widget_id == from_id {
                                let new_index = match mouse_index.cmp(from_index) {
                                    Ordering::Greater => Some(mouse_index - 1),
                                    Ordering::Equal => Some(mouse_index),
                                    Ordering::Less => None,
                                };
                                if let Some(new_index) = new_index {
                                    if new_index != *from_index {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::EditorTabSwap(
                                                *from_index,
                                                new_index,
                                            ),
                                            Target::Widget(editor_tab.widget_id),
                                        ));
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::Focus,
                                            Target::Widget(child.widget_id()),
                                        ));
                                    }
                                }
                            } else {
                                child.set_editor_tab(data, editor_tab.widget_id);
                                let editor_tab = data
                                    .main_split
                                    .editor_tabs
                                    .get_mut(&self.widget_id)
                                    .unwrap();
                                let editor_tab = Arc::make_mut(editor_tab);
                                editor_tab
                                    .children
                                    .insert(mouse_index, child.clone());
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::EditorTabAdd(
                                        mouse_index,
                                        child.clone(),
                                    ),
                                    Target::Widget(editor_tab.widget_id),
                                ));
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(child.widget_id()),
                                ));
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::EditorTabRemove(
                                        *from_index,
                                        false,
                                        false,
                                    ),
                                    Target::Widget(*from_id),
                                ));
                            }
                        }
                    };
                }
                if data.drag.is_some() {
                    *Arc::make_mut(&mut data.drag) = None;
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
                EditorTabChild::Editor(view_id, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        svg = file_svg_new(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                            }
                        }
                    }
                }
            }
            let text_layout = ctx
                .text()
                .new_text_layout(text)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let width = (text_size.width + height * 2.0).max(100.0);
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
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let size = ctx.size();

        let mut mouse_index = self.rects.len() - 1;
        for (i, tab_rect) in self.rects.iter().enumerate() {
            if i != editor_tab.active {
                tab_rect.paint(ctx, data, self.widget_id, i, size, self.mouse_pos);
            }
            if tab_rect.rect.contains(self.mouse_pos) {
                mouse_index = i;
            }
        }

        self.rects.get(editor_tab.active).unwrap().paint(
            ctx,
            data,
            self.widget_id,
            editor_tab.active,
            size,
            self.mouse_pos,
        );

        if ctx.is_hot() && data.drag.is_some() {
            let tab_rect = &self.rects[mouse_index];
            let x = if self.mouse_pos.x
                <= tab_rect.rect.x0 + tab_rect.rect.size().width / 2.0
            {
                if mouse_index == 0 {
                    tab_rect.rect.x0 + 2.0
                } else {
                    tab_rect.rect.x0
                }
            } else {
                tab_rect.rect.x1
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
