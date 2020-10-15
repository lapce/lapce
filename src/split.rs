use crate::{
    editor::EditorView, scroll::LapceScroll, state::LapceState,
    state::LapceUIState, state::LAPCE_STATE,
};
use std::{cmp::Ordering, sync::Arc};

use druid::{
    kurbo::{Line, Rect},
    widget::IdentityWrapper,
    Command, Target, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    editor::Editor,
    editor::EditorState,
};

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

pub struct LapceSplit {
    vertical: bool,
    children: Vec<ChildWidget>,
    current_bar_hover: usize,
}

struct ChildWidget {
    widget: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
}

impl LapceSplit {
    pub fn new(vertical: bool) -> Self {
        LapceSplit {
            vertical,
            children: Vec::new(),
            current_bar_hover: 0,
        }
    }

    pub fn with_child(
        mut self,
        child: impl Widget<LapceUIState> + 'static,
        params: f64,
    ) -> Self {
        let child = ChildWidget {
            widget: WidgetPod::new(child).boxed(),
            flex: false,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children.push(child);
        self
    }

    pub fn with_flex_child(
        mut self,
        child: impl Widget<LapceUIState> + 'static,
        params: f64,
    ) -> Self {
        let child = ChildWidget {
            widget: WidgetPod::new(child).boxed(),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children.push(child);
        self
    }

    pub fn even_flex_children(&mut self) {
        for child in self.children.iter_mut() {
            if child.flex {
                child.params = 1.0;
            }
        }
    }

    fn update_split_point(&mut self, size: Size, mouse_pos: Point) {
        let limit = 50.0;
        let left = self.children[self.current_bar_hover - 1].layout_rect.x0;
        let right = self.children[self.current_bar_hover].layout_rect.x1;

        if mouse_pos.x < left + limit || mouse_pos.x > right - limit {
            return;
        }

        if !self.children[self.current_bar_hover - 1].flex {
            self.children[self.current_bar_hover - 1].params =
                mouse_pos.x - left;
        } else {
            if !self.children[self.current_bar_hover].flex {
                self.children[self.current_bar_hover].params =
                    right - mouse_pos.x;
            }
            for (i, child) in self.children.iter_mut().enumerate() {
                if child.flex {
                    if i == self.current_bar_hover - 1 {
                        child.params = (mouse_pos.x - left) / size.width;
                    } else if i == self.current_bar_hover {
                        child.params = (right - mouse_pos.x) / size.width;
                    } else {
                        child.params = child.layout_rect.width() / size.width;
                    }
                }
            }
        }

        // let old_size = self.children_sizes[self.current_bar_hover];
        // let new_size = mouse_pos.x / size.width
        //     - self.children_sizes[..self.current_bar_hover]
        //         .iter()
        //         .sum::<f64>();
        // self.children_sizes[self.current_bar_hover] = new_size;
        // self.children_sizes[self.current_bar_hover + 1] += old_size - new_size;
    }

    fn bar_hit_test(&self, size: Size, mouse_pos: Point) -> Option<usize> {
        let children_len = self.children.len();
        if children_len <= 1 {
            return None;
        }
        for i in 1..children_len {
            let x = self.children[i].layout_rect.x0;
            if mouse_pos.x >= x - 3.0 && mouse_pos.x <= x + 3.0 {
                return Some(i);
            }
        }
        None
    }

    fn paint_bar(&mut self, ctx: &mut PaintCtx, env: &Env) {
        let children_len = self.children.len();
        if children_len <= 1 {
            return;
        }

        let size = ctx.size();
        for i in 1..children_len {
            let x = self.children[i].layout_rect.x0;
            let line =
                Line::new(Point::new(x, 0.0), Point::new(x, size.height));
            let color = env.get(theme::BORDER_LIGHT);
            ctx.stroke(line, &color, 1.0);
        }
    }
}

impl Widget<LapceUIState> for LapceSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                for child in self.children.as_mut_slice() {
                    child.widget.event(ctx, event, data, env);
                }
                return;
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::Split(vertical) => {
                            if self.children.len() <= 1 {
                                self.vertical = *vertical;
                            }
                            let mut editor_split =
                                LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            if &self.vertical != vertical {
                                for child in &self.children {
                                    if child.widget.id() == active {}
                                }
                            } else {
                                let mut index = 0;
                                for (i, child) in
                                    self.children.iter().enumerate()
                                {
                                    if child.widget.id() == active {
                                        index = i;
                                    }
                                }

                                let old_editor =
                                    editor_split.editors.get(&active).unwrap();

                                let split_id = old_editor.split_id.clone();
                                let buffer_id = old_editor.buffer_id.clone();
                                let selection = old_editor.selection.clone();
                                let scroll_offset =
                                    old_editor.scroll_offset.clone();

                                let new_editor = editor_split.new_editor(
                                    split_id,
                                    buffer_id,
                                    selection.clone(),
                                );
                                data.new_editor(&new_editor.view_id);

                                let new_editor_view = EditorView::new(
                                    new_editor.split_id,
                                    new_editor.view_id,
                                    new_editor.editor_id,
                                );
                                let new_child = ChildWidget {
                                    widget: WidgetPod::new(new_editor_view)
                                        .boxed(),
                                    flex: true,
                                    params: 1.0,
                                    layout_rect: Rect::ZERO,
                                };
                                self.children.insert(index + 1, new_child);
                                self.even_flex_children();
                                ctx.request_layout();
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::ScrollTo((
                                        scroll_offset.x,
                                        scroll_offset.y,
                                    )),
                                    Target::Widget(new_editor.view_id),
                                ));
                            }
                        }
                        LapceUICommand::SplitClose => {
                            if self.children.len() == 1 {
                                return;
                            }
                            let mut editor_split =
                                LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.widget.id() == active {
                                    index = i;
                                }
                            }
                            let new_index = if index >= self.children.len() - 1
                            {
                                index - 1
                            } else {
                                index + 1
                            };
                            let new_active =
                                self.children[new_index].widget.id();
                            self.children.remove(index);
                            editor_split.editors.remove(&active);
                            editor_split.active = new_active;

                            self.even_flex_children();
                            ctx.request_layout();
                        }
                        LapceUICommand::SplitExchange => {
                            let mut editor_split =
                                LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.widget.id() == active {
                                    index = i;
                                }
                            }
                            if index >= self.children.len() - 1 {
                            } else {
                                editor_split.active =
                                    self.children[index + 1].widget.id();
                                self.children.swap(index, index + 1);
                                ctx.request_layout();
                            }
                        }
                        LapceUICommand::SplitMove(direction) => {
                            let mut editor_split =
                                LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.widget.id() == active {
                                    index = i;
                                }
                            }
                            match direction {
                                SplitMoveDirection::Left => {
                                    if index == 0 {
                                        return;
                                    }
                                    editor_split.active =
                                        self.children[index - 1].widget.id();
                                }
                                SplitMoveDirection::Right => {
                                    if index >= self.children.len() - 1 {
                                        return;
                                    }
                                    editor_split.active =
                                        self.children[index + 1].widget.id();
                                }
                                _ => (),
                            }
                            let editor = editor_split
                                .editors
                                .get(&editor_split.active)
                                .unwrap();
                            let buffer = editor_split
                                .buffers
                                .get(editor.buffer_id.as_ref().unwrap())
                                .unwrap();
                            editor
                                .ensure_cursor_visible(ctx, buffer, env, None);

                            ctx.request_paint();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
        for child in self.children.as_mut_slice() {
            if child.widget.is_active() {
                child.widget.event(ctx, event, data, env);
                if ctx.is_handled() {
                    return;
                }
            }
        }

        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    if let Some(bar_number) =
                        self.bar_hit_test(ctx.size(), mouse.pos)
                    {
                        self.current_bar_hover = bar_number;
                        ctx.set_active(true);
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if mouse.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                    self.update_split_point(ctx.size(), mouse.pos);
                    ctx.request_paint();
                }
            }
            Event::MouseMove(mouse) => {
                if ctx.is_active() {
                    self.update_split_point(ctx.size(), mouse.pos);
                    ctx.request_layout();
                }

                if ctx.is_hot()
                    && self.bar_hit_test(ctx.size(), mouse.pos).is_some()
                    || ctx.is_active()
                {
                    match self.vertical {
                        true => ctx.set_cursor(&Cursor::ResizeLeftRight),
                        false => ctx.set_cursor(&Cursor::ResizeUpDown),
                    }
                }
            }
            _ => (),
        }

        for child in self.children.as_mut_slice() {
            if !child.widget.is_active() {
                child.widget.event(ctx, event, data, env);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.widget.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.widget.update(ctx, &data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let my_size = bc.max();

        let children_len = self.children.len();
        if children_len == 0 {
            return my_size;
        }

        let mut non_flex_total = 0.0;
        for child in self.children.iter_mut() {
            if !child.flex {
                let size = Size::new(child.params, my_size.height);
                let child_size = child.widget.layout(
                    ctx,
                    &BoxConstraints::new(size, size),
                    data,
                    env,
                );
                child.layout_rect = child.layout_rect.with_size(child_size);
                non_flex_total += child_size.width;
            }
        }

        let mut flex_sum = 0.0;
        for child in &self.children {
            if child.flex {
                flex_sum += child.params;
            }
        }

        let flex_total = my_size.width - non_flex_total;
        let mut x = 0.0;
        let mut y = 0.0;
        for child in self.children.iter_mut() {
            child.layout_rect = child.layout_rect.with_origin(Point::new(x, y));
            if !child.flex {
                x += child.layout_rect.width();
            } else {
                let width = flex_total / flex_sum * child.params;
                let size = Size::new(width, my_size.height);
                child.widget.layout(
                    ctx,
                    &BoxConstraints::new(size, size),
                    data,
                    env,
                );
                child.layout_rect = child.layout_rect.with_size(size);
                x += width;
            }
            child
                .widget
                .set_layout_rect(ctx, data, env, child.layout_rect);
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        for child in self.children.as_mut_slice() {
            child.widget.paint(ctx, &data, env);
        }
        self.paint_bar(ctx, env);
    }
}
