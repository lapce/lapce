use crate::{
    editor::{EditorLocation, EditorView},
    scroll::LapceScroll,
    state::LapceState,
    state::LapceUIState,
    state::LAPCE_STATE,
};
use std::{cmp::Ordering, sync::Arc};

use druid::{
    kurbo::{Line, Rect},
    widget::IdentityWrapper,
    Command, Target, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
    WidgetExt, WidgetPod,
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
    id: Option<WidgetId>,
    vertical: bool,
    pub children: Vec<ChildWidget>,
    current_bar_hover: usize,
}

pub struct ChildWidget {
    pub widget: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
}

impl LapceSplit {
    pub fn new(vertical: bool) -> Self {
        LapceSplit {
            id: None,
            vertical,
            children: Vec::new(),
            current_bar_hover: 0,
        }
    }

    pub fn with_id(mut self, id: WidgetId) -> Self {
        self.id = Some(id);
        self
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
            self.children[self.current_bar_hover - 1].params = mouse_pos.x - left;
        } else {
            if !self.children[self.current_bar_hover].flex {
                self.children[self.current_bar_hover].params = right - mouse_pos.x;
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
            let line = Line::new(Point::new(x, 0.0), Point::new(x, size.height));
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
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::ApplyEdits(rev, edits) => {
                            LAPCE_STATE
                                .editor_split
                                .lock()
                                .apply_edits(ctx, data, *rev, edits);
                        }
                        LapceUICommand::GotoLocation(location) => {
                            let mut editor_split = LAPCE_STATE.editor_split.lock();
                            editor_split.save_jump_location();
                            let path = location.uri.path().to_string();
                            let buffer =
                                editor_split.get_buffer_from_path(ctx, data, &path);
                            let location = EditorLocation {
                                path,
                                offset: buffer.offset_of_line(
                                    location.range.start.line as usize,
                                ) + location.range.start.character as usize,
                                scroll_offset: None,
                            };
                            editor_split.jump_to_location(ctx, data, &location, env);
                        }
                        LapceUICommand::Split(vertical) => {
                            if self.children.len() <= 1 {
                                self.vertical = *vertical;
                            }
                            let mut editor_split = LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            if &self.vertical != vertical {
                                for child in &self.children {
                                    if child.widget.id() == active {}
                                }
                            } else {
                                let mut index = 0;
                                for (i, child) in self.children.iter().enumerate() {
                                    if child.widget.id() == active {
                                        index = i;
                                    }
                                }

                                let old_editor =
                                    editor_split.editors.get(&active).unwrap();

                                let split_id = old_editor.split_id.clone();
                                let buffer_id = old_editor.buffer_id.clone();
                                let selection = old_editor.selection.clone();
                                let scroll_offset = old_editor.scroll_offset.clone();
                                let locations = old_editor.locations.clone();
                                let current_location = old_editor.current_location;
                                let scroll_size = old_editor.scroll_size;

                                let mut new_editor = editor_split.new_editor(
                                    split_id,
                                    buffer_id,
                                    selection.clone(),
                                );
                                new_editor.locations = locations;
                                new_editor.current_location = current_location;
                                new_editor.scroll_size = scroll_size;
                                data.new_editor(&new_editor.view_id);
                                let editor_ui = data.get_editor(&active);
                                let selection = editor_ui.selection.clone();
                                let visual_mode = editor_ui.visual_mode.clone();
                                let mode = editor_ui.mode.clone();
                                let selection_start_line =
                                    editor_ui.selection_start_line;
                                let selection_end_line =
                                    editor_ui.selection_end_line;
                                let new_editor_ui =
                                    data.get_editor_mut(&new_editor.view_id);
                                new_editor_ui.selection = selection;
                                new_editor_ui.visual_mode = visual_mode;
                                new_editor_ui.mode = mode;
                                new_editor_ui.selection_start_line =
                                    selection_start_line;
                                new_editor_ui.selection_end_line =
                                    selection_end_line;

                                let mut new_editor_view = EditorView::new(
                                    new_editor.split_id,
                                    new_editor.view_id,
                                    new_editor.editor_id,
                                );
                                println!("scroll_offset is {:?}", scroll_offset);
                                new_editor_view
                                    .editor
                                    .widget_mut()
                                    .scroll_to(scroll_offset.x, scroll_offset.y);
                                new_editor.scroll_offset = scroll_offset;

                                let new_child = ChildWidget {
                                    widget: WidgetPod::new(new_editor_view).boxed(),
                                    flex: true,
                                    params: 1.0,
                                    layout_rect: Rect::ZERO,
                                };
                                self.children.insert(index + 1, new_child);
                                self.even_flex_children();
                                ctx.children_changed();
                                //ctx.request_layout();
                                //ctx.submit_command(Command::new(
                                //    LAPCE_UI_COMMAND,
                                //    LapceUICommand::ScrollTo((
                                //        scroll_offset.x,
                                //        scroll_offset.y,
                                //    )),
                                //    Target::Widget(new_editor.view_id),
                                //));
                            }
                        }
                        LapceUICommand::SplitClose => {
                            if self.children.len() == 1 {
                                return;
                            }
                            let mut editor_split = LAPCE_STATE.editor_split.lock();
                            let active = editor_split.active;
                            let buffer_id = editor_split
                                .editors
                                .get(&active)
                                .unwrap()
                                .buffer_id
                                .clone();
                            let mut index = 0;
                            for (i, child) in self.children.iter().enumerate() {
                                if child.widget.id() == active {
                                    index = i;
                                }
                            }
                            let new_index = if index >= self.children.len() - 1 {
                                index - 1
                            } else {
                                index + 1
                            };
                            let new_active = self.children[new_index].widget.id();
                            self.children.remove(index);
                            editor_split.editors.remove(&active);
                            editor_split.active = new_active;
                            if let Some(buffer_id) = buffer_id {
                                editor_split
                                    .clear_buffer_text_layouts(data, buffer_id);
                            }

                            self.even_flex_children();
                            ctx.children_changed();
                        }
                        LapceUICommand::SplitExchange => {
                            let mut editor_split = LAPCE_STATE.editor_split.lock();
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
                            let mut editor_split = LAPCE_STATE.editor_split.lock();
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
                            editor.ensure_cursor_visible(ctx, buffer, env, None);

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

                if ctx.is_hot() && self.bar_hit_test(ctx.size(), mouse.pos).is_some()
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

    fn id(&self) -> Option<WidgetId> {
        self.id
    }
}
