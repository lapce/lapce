use crate::{scroll::CraneScroll, state::CRANE_STATE};
use std::cmp::Ordering;

use druid::{
    kurbo::{Line, Rect},
    widget::IdentityWrapper,
    WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};

use crate::{
    command::{CraneUICommand, CRANE_UI_COMMAND},
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

pub struct CraneSplit<T> {
    vertical: bool,
    children: Vec<WidgetPod<T, Box<dyn Widget<T>>>>,
    children_sizes: Vec<f64>,
    current_bar_hover: usize,
}

impl<T> CraneSplit<T> {
    pub fn new(vertical: bool) -> Self {
        CraneSplit {
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

    pub fn with_child(mut self, child: impl Widget<T> + 'static) -> Self {
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

impl<T: Data> Widget<T> for CraneSplit<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
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
                _ if cmd.is(CRANE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(CRANE_UI_COMMAND);
                    match command {
                        CraneUICommand::Split(vertical, editor_id) => {
                            if self.children.len() == 1 {
                                self.vertical = *vertical;
                            }
                            if &self.vertical != vertical {
                                for child in &self.children {
                                    if &child.id() == editor_id {}
                                }
                            } else {
                                let mut index = 0;
                                for (i, child) in
                                    self.children.iter().enumerate()
                                {
                                    if &child.id() == editor_id {
                                        index = i;
                                    }
                                }

                                let (split_id, buffer_id) = {
                                    let state = CRANE_STATE
                                        .editor_split
                                        .lock()
                                        .unwrap();
                                    let editor =
                                        state.editors.get(editor_id).unwrap();
                                    (editor.split_id, editor.buffer_id.clone())
                                };

                                let new_editor_id = WidgetId::next();
                                let new_editor =
                                    Editor::new(new_editor_id.clone());
                                // let new_scroll_id = WidgetId::next();
                                let mut new_editor_state = EditorState::new(
                                    new_editor_id.clone(),
                                    // new_scroll_id,
                                    split_id.clone(),
                                );
                                new_editor_state.buffer_id = buffer_id;
                                CRANE_STATE
                                    .editor_split
                                    .lock()
                                    .unwrap()
                                    .editors
                                    .insert(
                                        new_editor_id.clone(),
                                        new_editor_state,
                                    );

                                let new_child =
                                    WidgetPod::new(IdentityWrapper::wrap(
                                        CraneScroll::new(
                                            new_editor.padding((
                                                10.0, 0.0, 10.0, 0.0,
                                            )),
                                        ),
                                        new_editor_id,
                                    ))
                                    .boxed();
                                self.children.insert(index + 1, new_child);
                                self.even_child_sizes();
                            }
                        }
                        CraneUICommand::SplitExchange(editor_id) => {
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if &child.id() == editor_id {
                                    index = i;
                                }
                            }
                            if index >= self.children.len() - 1 {
                            } else {
                                CRANE_STATE
                                    .editor_split
                                    .lock()
                                    .unwrap()
                                    .set_active(self.children[index + 1].id());
                                self.children.swap(index, index + 1);
                                self.children_sizes.swap(index, index + 1);
                                ctx.request_layout();
                            }
                        }
                        CraneUICommand::SplitMove(direction, editor_id) => {
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if &child.id() == editor_id {
                                    index = i;
                                }
                            }
                            match direction {
                                SplitMoveDirection::Left => {
                                    if index == 0 {
                                        return;
                                    }
                                    CRANE_STATE
                                        .editor_split
                                        .lock()
                                        .unwrap()
                                        .set_active(
                                            self.children[index - 1].id(),
                                        )
                                }
                                SplitMoveDirection::Right => {
                                    if index >= self.children.len() - 1 {
                                        return;
                                    }
                                    CRANE_STATE
                                        .editor_split
                                        .lock()
                                        .unwrap()
                                        .set_active(
                                            self.children[index + 1].id(),
                                        )
                                }
                                _ => (),
                            }
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
        data: &T,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &T,
        data: &T,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.update(ctx, &data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
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
            CRANE_STATE
                .editor_split
                .lock()
                .unwrap()
                .set_editor_size(child.id(), child_size.clone());
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        println!("split paint {:?}", ctx.region().rects());
        self.paint_bar(ctx, env);
        for child in self.children.as_mut_slice() {
            child.paint(ctx, &data, env);
        }
    }
}
