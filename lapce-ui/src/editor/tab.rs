use std::{cell::RefCell, rc::Rc, sync::Arc};

use druid::{
    kurbo::Line, piet::TextLayout, BoxConstraints, Command, Env, Event, EventCtx,
    InternalEvent, LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point,
    Rect, RenderContext, Size, Target, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{
        DragContent, EditorTabChild, LapceEditorTabData, LapceTabData, SplitContent,
    },
    editor::TabRect,
    split::{SplitDirection, SplitMoveDirection},
    
};

use crate::editor::{
    tab_header::LapceEditorTabHeader, view::editor_tab_child_widget,
};

use crate::svg::get_svg;

pub struct LapceEditorTab {
    pub widget_id: WidgetId,
    header: WidgetPod<LapceTabData, LapceEditorTabHeader>,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    mouse_pos: Point,
}

impl LapceEditorTab {
    pub fn new(widget_id: WidgetId) -> Self {
        let header = LapceEditorTabHeader::new(widget_id);
        Self {
            widget_id,
            header: WidgetPod::new(header),
            children: Vec::new(),
            mouse_pos: Point::ZERO,
        }
    }

    pub fn with_child(mut self, child: Box<dyn Widget<LapceTabData>>) -> Self {
        self.children.push(WidgetPod::new(child));
        self
    }

    fn clear_child(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        self.children.clear();
        ctx.children_changed();

        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        for child in editor_tab.children.iter() {
            match child {
                EditorTabChild::Editor(view_id, _) => {
                    data.main_split.editors.remove(view_id);
                }
            }
        }
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::SplitRemove(SplitContent::EditorTab(
                editor_tab.widget_id,
            )),
            Target::Widget(editor_tab.split),
        ));
    }

    pub fn remove_child(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        i: usize,
        delete: bool,
        focus: bool,
    ) {
        self.children.remove(i);
        ctx.children_changed();

        let editor_tab = data
            .main_split
            .editor_tabs
            .get_mut(&self.widget_id)
            .unwrap();
        let editor_tab = Arc::make_mut(editor_tab);
        let removed_child = if editor_tab.children.len() == 1 {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitRemove(SplitContent::EditorTab(
                    editor_tab.widget_id,
                )),
                Target::Widget(editor_tab.split),
            ));
            editor_tab.children.remove(i)
        } else if editor_tab.active == i {
            let new_index = if i >= editor_tab.children.len() - 1 {
                editor_tab.active = i - 1;
                i - 1
            } else {
                i
            };
            if focus {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(editor_tab.children[new_index].widget_id()),
                ));
            }
            editor_tab.children.remove(i)
        } else {
            if editor_tab.active > i {
                editor_tab.active -= 1;
            }
            editor_tab.children.remove(i)
        };
        if delete {
            match removed_child {
                EditorTabChild::Editor(view_id, _) => {
                    data.main_split.editors.remove(&view_id);
                }
            }
        }
    }

    fn mouse_up(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        if let Some((_, drag_content)) = data.drag.clone().as_ref() {
            match drag_content {
                DragContent::EditorTab(from_id, from_index, child, _) => {
                    let size = ctx.size();
                    let width = size.width;
                    let header_rect = self.header.layout_rect();
                    let header_height = header_rect.height();
                    let content_height = size.height - header_height;
                    let content_rect = Size::new(width, content_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, header_height));

                    if content_rect.contains(mouse_event.pos) {
                        let direction = if self.mouse_pos.x < size.width / 3.0 {
                            Some(SplitMoveDirection::Left)
                        } else if self.mouse_pos.x > size.width / 3.0 * 2.0 {
                            Some(SplitMoveDirection::Right)
                        } else if self.mouse_pos.y
                            < header_height + content_height / 3.0
                        {
                            Some(SplitMoveDirection::Up)
                        } else if self.mouse_pos.y
                            > header_height + content_height / 3.0 * 2.0
                        {
                            Some(SplitMoveDirection::Down)
                        } else {
                            None
                        };
                        match direction {
                            Some(direction) => {
                                let (split_direction, shift_current) =
                                    match direction {
                                        SplitMoveDirection::Up => {
                                            (SplitDirection::Horizontal, true)
                                        }
                                        SplitMoveDirection::Down => {
                                            (SplitDirection::Horizontal, false)
                                        }
                                        SplitMoveDirection::Right => {
                                            (SplitDirection::Vertical, false)
                                        }
                                        SplitMoveDirection::Left => {
                                            (SplitDirection::Vertical, true)
                                        }
                                    };
                                let editor_tab = data
                                    .main_split
                                    .editor_tabs
                                    .get(&self.widget_id)
                                    .unwrap();
                                let split_id = editor_tab.split;
                                let mut new_editor_tab = LapceEditorTabData {
                                    widget_id: WidgetId::next(),
                                    split: split_id,
                                    active: 0,
                                    children: vec![child.clone()],
                                    layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                                    content_is_hot: Rc::new(RefCell::new(false)),
                                };
                                child.set_editor_tab(data, new_editor_tab.widget_id);

                                let new_split_id = data.main_split.split(
                                    ctx,
                                    split_id,
                                    SplitContent::EditorTab(self.widget_id),
                                    SplitContent::EditorTab(
                                        new_editor_tab.widget_id,
                                    ),
                                    split_direction,
                                    shift_current,
                                    true,
                                );
                                new_editor_tab.split = new_split_id;
                                if split_id != new_split_id {
                                    let editor_tab = data
                                        .main_split
                                        .editor_tabs
                                        .get_mut(&self.widget_id)
                                        .unwrap();
                                    let editor_tab = Arc::make_mut(editor_tab);
                                    editor_tab.split = new_split_id;
                                }

                                data.main_split.editor_tabs.insert(
                                    new_editor_tab.widget_id,
                                    Arc::new(new_editor_tab),
                                );
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
                            None => {
                                if from_id == &self.widget_id {
                                    return;
                                }
                                child.set_editor_tab(data, self.widget_id);
                                let editor_tab = data
                                    .main_split
                                    .editor_tabs
                                    .get_mut(&self.widget_id)
                                    .unwrap();
                                let editor_tab = Arc::make_mut(editor_tab);
                                editor_tab
                                    .children
                                    .insert(editor_tab.active + 1, child.clone());
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::EditorTabAdd(
                                        editor_tab.active + 1,
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
                    }
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorTab {
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
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                ctx.request_paint();
            }
            Event::MouseUp(mouse_event) => {
                self.mouse_up(ctx, data, mouse_event);
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::EditorTabAdd(index, content) => {
                        self.children.insert(
                            *index,
                            WidgetPod::new(editor_tab_child_widget(content)),
                        );
                        ctx.children_changed();
                        return;
                    }
                    LapceUICommand::EditorTabSwap(from_index, to_index) => {
                        let editor_tab = data
                            .main_split
                            .editor_tabs
                            .get_mut(&self.widget_id)
                            .unwrap();
                        let editor_tab = Arc::make_mut(editor_tab);

                        let child = self.children.remove(*from_index);
                        self.children.insert(*to_index, child);
                        let child = editor_tab.children.remove(*from_index);
                        editor_tab.children.insert(*to_index, child);
                        ctx.request_layout();
                        return;
                    }
                    LapceUICommand::EditorTabRemove(index, delete, focus) => {
                        self.remove_child(ctx, data, *index, *delete, *focus);
                        return;
                    }
                    LapceUICommand::SplitClose => {
                        self.clear_child(ctx, data);
                        return;
                    }
                    LapceUICommand::Focus => {
                        let tab = data
                            .main_split
                            .editor_tabs
                            .get(&self.widget_id)
                            .unwrap();
                        let widget_id = tab.children[tab.active].widget_id();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(widget_id),
                        ));
                        return;
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.header.event(ctx, event, data, env);
        let tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        match event {
            Event::Internal(InternalEvent::TargetedCommand(_)) => {
                for child in self.children.iter_mut() {
                    child.event(ctx, event, data, env);
                }
            }
            _ => {
                self.children[tab.active].event(ctx, event, data, env);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.lifecycle(ctx, event, data, env);
        for child in self.children.iter_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.update(ctx, data, env);
        for child in self.children.iter_mut() {
            child.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);

        let tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let child_size =
            Size::new(self_size.width, self_size.height - header_size.height);
        self.children[tab.active].layout(
            ctx,
            &BoxConstraints::tight(child_size),
            data,
            env,
        );
        self.children[tab.active].set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, header_size.height),
        );
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        ctx.fill(
            size.to_rect(),
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        self.header.paint(ctx, data, env);
        if ctx.is_hot() && data.drag.is_some() {
            let width = size.width;
            let header_rect = self.header.layout_rect();
            let header_height = header_rect.height();
            let header_size = header_rect.size();
            let content_height = size.height - header_height;
            let content_rect = Size::new(width, content_height)
                .to_rect()
                .with_origin(Point::new(0.0, header_height));

            if content_rect.contains(self.mouse_pos) {
                let rect = if self.mouse_pos.x < size.width / 3.0 {
                    Size::new(width / 2.0, content_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, header_height))
                } else if self.mouse_pos.x > size.width / 3.0 * 2.0 {
                    Size::new(width / 2.0, content_height)
                        .to_rect()
                        .with_origin(Point::new(width / 2.0, header_height))
                } else if self.mouse_pos.y
                    < header_size.height + content_height / 3.0
                {
                    Size::new(width, content_height / 2.0)
                        .to_rect()
                        .with_origin(Point::new(0.0, header_height))
                } else if self.mouse_pos.y
                    > header_size.height + content_height / 3.0 * 2.0
                {
                    Size::new(width, content_height / 2.0)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            header_height + content_height / 2.0,
                        ))
                } else {
                    Size::new(width, content_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, header_height))
                };
                ctx.fill(
                    rect,
                    &data
                        .config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE)
                        .clone()
                        .with_alpha(0.8),
                );
            }
        }
        let tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        self.children[tab.active].paint(ctx, data, env);
    }
}

pub trait TabRectRenderer {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        widget_id: WidgetId,
        i: usize,
        size: Size,
        mouse_pos: Point,
    );
}

impl TabRectRenderer for TabRect {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        widget_id: WidgetId,
        i: usize,
        size: Size,
        mouse_pos: Point,
    ) {
        let width = 13.0;
        let height = 13.0;
        let editor_tab = data.main_split.editor_tabs.get(&widget_id).unwrap();

        let rect = Size::new(width, height).to_rect().with_origin(Point::new(
            self.rect.x0 + (size.height - width) / 2.0,
            (size.height - height) / 2.0,
        ));
        if i == editor_tab.active {
            ctx.fill(
                self.rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        ctx.draw_svg(&self.svg, rect, None);
        let text_size = self.text_layout.size();
        ctx.draw_text(
            &self.text_layout,
            Point::new(
                self.rect.x0 + size.height,
                (size.height - text_size.height) / 2.0,
            ),
        );
        let x = self.rect.x1;
        ctx.stroke(
            Line::new(Point::new(x - 0.5, 0.0), Point::new(x - 0.5, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        if ctx.is_hot() {
            if self.close_rect.contains(mouse_pos) {
                ctx.fill(
                    &self.close_rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            if self.rect.contains(mouse_pos) {
                let svg = get_svg("close.svg").unwrap();
                ctx.draw_svg(
                    &svg,
                    self.close_rect.inflate(-4.0, -4.0),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                );
            }
        }

        // Only display dirty icon if focus is not on tab bar, so that the close svg can be shown
        if !(ctx.is_hot() && self.rect.contains(mouse_pos)) {
            // See if any of the children are dirty
            let is_dirty = match &editor_tab.children[i] {
                EditorTabChild::Editor(editor_id, _) => {
                    let buffer = data.main_split.editor_buffer(*editor_id);
                    buffer.dirty()
                }
            };

            if is_dirty {
                let svg = get_svg("unsaved.svg").unwrap();
                ctx.draw_svg(
                    &svg,
                    self.close_rect.inflate(-4.0, -4.0),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                )
            }
        }
    }
}
