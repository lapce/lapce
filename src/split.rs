use crate::{editor::EditorView, scroll::LapceScroll, state::LapceState};
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
    children: Vec<WidgetPod<LapceState, Box<dyn Widget<LapceState>>>>,
    children_sizes: Vec<f64>,
    current_bar_hover: usize,
}

impl LapceSplit {
    pub fn new(vertical: bool) -> Self {
        LapceSplit {
            vertical,
            children: Vec::new(),
            children_sizes: Vec::new(),
            current_bar_hover: 0,
        }
    }

    pub fn even_child_sizes(&mut self) {
        let children_len = self.children.len();
        let child_size = 1.0 / children_len as f64;
        self.children_sizes = (0..children_len - 1)
            .into_iter()
            .map(|i| child_size)
            .collect();
        self.children_sizes
            .push(1.0 - self.children_sizes.iter().sum::<f64>());
    }

    pub fn add_child(&mut self, child: impl Widget<LapceState> + 'static) {
        let child = WidgetPod::new(child).boxed();
        self.children.push(child);
        self.even_child_sizes();
    }

    pub fn with_child(
        mut self,
        child: impl Widget<LapceState> + 'static,
    ) -> Self {
        let child = WidgetPod::new(child).boxed();
        self.children.push(child);
        self.even_child_sizes();
        self
    }

    fn update_split_point(&mut self, size: Size, mouse_pos: Point) {
        let limit = 50.0;
        let left = self.children_sizes[..self.current_bar_hover]
            .iter()
            .sum::<f64>()
            * size.width;

        let right = self.children_sizes[..self.current_bar_hover + 2]
            .iter()
            .sum::<f64>()
            * size.width;

        if mouse_pos.x < left + limit || mouse_pos.x > right - limit {
            return;
        }

        let old_size = self.children_sizes[self.current_bar_hover];
        let new_size = mouse_pos.x / size.width
            - self.children_sizes[..self.current_bar_hover]
                .iter()
                .sum::<f64>();
        self.children_sizes[self.current_bar_hover] = new_size;
        self.children_sizes[self.current_bar_hover + 1] += old_size - new_size;
    }

    fn bar_hit_test(&self, size: Size, mouse_pos: Point) -> Option<usize> {
        let children_len = self.children.len();
        if children_len <= 1 {
            return None;
        }
        for i in 0..children_len - 1 {
            let x =
                self.children_sizes[..i + 1].iter().sum::<f64>() * size.width;
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
        for i in 0..children_len - 1 {
            let x =
                self.children_sizes[..i + 1].iter().sum::<f64>() * size.width;
            let line =
                Line::new(Point::new(x, 0.0), Point::new(x, size.height));
            let color = env.get(theme::BORDER_LIGHT);
            ctx.stroke(line, &color, 1.0);
        }
    }
}

impl Widget<LapceState> for LapceSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => {
                for child in self.children.as_mut_slice() {
                    child.event(ctx, event, data, env);
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
                            let active = data.editor_split.active;
                            if &self.vertical != vertical {
                                for child in &self.children {
                                    if child.id() == active {}
                                }
                            } else {
                                let mut index = 0;
                                for (i, child) in
                                    self.children.iter().enumerate()
                                {
                                    if child.id() == active {
                                        index = i;
                                    }
                                }

                                let old_editor = data
                                    .editor_split
                                    .editors
                                    .get(&active)
                                    .unwrap();

                                let split_id = old_editor.split_id.clone();
                                let buffer_id = old_editor.buffer_id.clone();
                                let selection = old_editor.selection.clone();
                                let scroll_offset =
                                    old_editor.scroll_offset.clone();

                                let editor_split =
                                    Arc::make_mut(&mut data.editor_split);
                                let new_editor = editor_split.new_editor(
                                    split_id,
                                    buffer_id,
                                    selection.clone(),
                                );

                                let new_editor_view = EditorView::new(
                                    new_editor.split_id,
                                    new_editor.view_id,
                                    new_editor.editor_id,
                                );
                                let new_child =
                                    WidgetPod::new(new_editor_view).boxed();
                                self.children.insert(index + 1, new_child);
                                self.even_child_sizes();
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
                            let active = data.editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.id() == active {
                                    index = i;
                                }
                            }
                            let new_index = if index >= self.children.len() - 1
                            {
                                index - 1
                            } else {
                                index + 1
                            };
                            let new_active = self.children[new_index].id();
                            self.children.remove(index);
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            editor_split.editors.remove(&active);
                            editor_split.active = new_active;

                            self.even_child_sizes();
                            ctx.request_layout();
                        }
                        LapceUICommand::SplitExchange => {
                            let active = data.editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.id() == active {
                                    index = i;
                                }
                            }
                            if index >= self.children.len() - 1 {
                            } else {
                                let editor_split =
                                    Arc::make_mut(&mut data.editor_split);
                                editor_split.active =
                                    self.children[index + 1].id();
                                self.children.swap(index, index + 1);
                                self.children_sizes.swap(index, index + 1);
                                ctx.request_layout();
                            }
                        }
                        LapceUICommand::SplitMove(direction) => {
                            let active = data.editor_split.active;
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.id() == active {
                                    index = i;
                                }
                            }
                            let editor_split =
                                Arc::make_mut(&mut data.editor_split);
                            match direction {
                                SplitMoveDirection::Left => {
                                    if index == 0 {
                                        return;
                                    }
                                    editor_split.active =
                                        self.children[index - 1].id();
                                }
                                SplitMoveDirection::Right => {
                                    if index >= self.children.len() - 1 {
                                        return;
                                    }
                                    editor_split.active =
                                        self.children[index + 1].id();
                                }
                                _ => (),
                            }
                            let editor = editor_split
                                .editors
                                .get_mut(&editor_split.active)
                                .unwrap();
                            let buffer = editor_split
                                .buffers
                                .get(editor.buffer_id.as_ref().unwrap())
                                .unwrap();
                            editor.ensure_cursor_visible(ctx, buffer, env);

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
            if child.is_active() {
                child.event(ctx, event, data, env);
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
            if !child.is_active() {
                child.event(ctx, event, data, env);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceState,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        if data.editor_split.same(&old_data.editor_split) {
            return;
        }

        for child in self.children.as_mut_slice() {
            child.update(ctx, &data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        let my_size = bc.max();

        let children_len = self.children.len();
        if children_len == 0 {
            return my_size;
        }

        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = Size::new(
                self.children_sizes[i] * my_size.width,
                my_size.height,
            );
            let child_bc =
                BoxConstraints::new(child_size.clone(), child_size.clone());
            child.layout(ctx, &child_bc, data, env);
            child.set_layout_rect(
                ctx,
                data,
                env,
                Rect::ZERO
                    .with_origin(Point::new(
                        self.children_sizes[..i].iter().sum::<f64>()
                            * my_size.width,
                        0.0,
                    ))
                    .with_size(Size::new(
                        my_size.width * self.children_sizes[i],
                        my_size.height,
                    )),
            );
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        self.paint_bar(ctx, env);
        for child in self.children.as_mut_slice() {
            child.paint(ctx, &data, env);
        }
    }
}
