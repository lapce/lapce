use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceEditorData, LapceEditorLens, LapceTabData},
    editor::Editor,
    editor::EditorState,
    editor::{EditorLocation, EditorView, LapceEditorView},
    scroll::LapceScroll,
    state::LapceTabState,
    state::LapceUIState,
    state::LAPCE_APP_STATE,
};
use std::{cmp::Ordering, sync::Arc};

use druid::{
    kurbo::{Line, Rect},
    widget::IdentityWrapper,
    Command, Target, WidgetId, WindowId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
    WidgetExt, WidgetPod,
};

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

pub struct LapceSplitNew {
    split_id: WidgetId,
    children: Vec<ChildWidgetNew>,
}

pub struct ChildWidgetNew {
    pub widget: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
}

impl LapceSplitNew {
    pub fn new(split_id: WidgetId) -> Self {
        Self {
            split_id,
            children: Vec::new(),
        }
    }

    pub fn with_flex_child(
        mut self,
        child: Box<dyn Widget<LapceTabData>>,
        params: f64,
    ) -> Self {
        let child = ChildWidgetNew {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children.push(child);
        self
    }

    pub fn insert_flex_child(
        &mut self,
        index: usize,
        child: Box<dyn Widget<LapceTabData>>,
        params: f64,
    ) {
        let child = ChildWidgetNew {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children.insert(index, child);
    }

    pub fn even_flex_children(&mut self) {
        for child in self.children.iter_mut() {
            if child.flex {
                child.params = 1.0;
            }
        }
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

    pub fn split_editor_close(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        view_id: WidgetId,
    ) {
        if self.children.len() <= 1 {
            return;
        }

        let mut index = 0;
        for (i, child) in self.children.iter().enumerate() {
            if child.widget.id() == view_id {
                index = i;
                break;
            }
        }

        let new_index = if index >= self.children.len() - 1 {
            index - 1
        } else {
            index + 1
        };
        let new_view_id = self.children[new_index].widget.id();
        let new_editor = data.main_split.editors.get(&new_view_id).unwrap();
        data.main_split.focus = Arc::new(new_editor.editor_id);
        ctx.set_focus(new_editor.editor_id);
        data.main_split.editors.remove(&view_id);
        self.children.remove(index);

        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_editor_exchange(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        view_id: WidgetId,
    ) {
        if self.children.len() <= 1 {
            return;
        }

        let mut index = 0;
        for (i, child) in self.children.iter().enumerate() {
            if child.widget.id() == view_id {
                index = i;
                break;
            }
        }
        if index >= self.children.len() - 1 {
            return;
        }

        let new_view_id = self.children[index + 1].widget.id();
        let new_editor = data.main_split.editors.get(&new_view_id).unwrap();
        data.main_split.focus = Arc::new(new_editor.editor_id);
        ctx.set_focus(new_editor.editor_id);

        self.children.swap(index, index + 1);

        ctx.request_layout();
    }

    pub fn split_editor_move(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        direction: &SplitMoveDirection,
        view_id: WidgetId,
    ) {
        let mut index = 0;
        for (i, child) in self.children.iter().enumerate() {
            if child.widget.id() == view_id {
                index = i;
                break;
            }
        }

        let new_index = match direction {
            SplitMoveDirection::Left => {
                if index == 0 {
                    return;
                }
                index - 1
            }
            SplitMoveDirection::Right => {
                if index >= self.children.len() - 1 {
                    return;
                }
                index + 1
            }
            _ => index,
        };

        let new_view_id = self.children[new_index].widget.id();
        let new_editor = data.main_split.editors.get(&new_view_id).unwrap();
        data.main_split.focus = Arc::new(new_editor.editor_id);
        ctx.set_focus(new_editor.editor_id);
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureCursorVisible,
            Target::Widget(new_editor.editor_id),
        ));
    }

    pub fn split_editor(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        vertical: bool,
        view_id: WidgetId,
    ) {
        let mut index = 0;
        for (i, child) in self.children.iter().enumerate() {
            if child.widget.id() == view_id {
                index = i;
                break;
            }
        }

        let from_editor = data.main_split.editors.get(&view_id).unwrap();
        let mut editor_data =
            LapceEditorData::new(self.split_id, from_editor.buffer.clone());
        editor_data.cursor = from_editor.cursor.clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ForceScrollTo(
                from_editor.scroll_offset.x,
                from_editor.scroll_offset.y,
            ),
            Target::Widget(editor_data.editor_id),
        ));

        let editor =
            LapceEditorView::new(editor_data.view_id, editor_data.editor_id);
        self.insert_flex_child(
            index + 1,
            editor.lens(LapceEditorLens(editor_data.view_id)).boxed(),
            1.0,
        );
        self.even_flex_children();
        ctx.children_changed();
        data.main_split
            .editors
            .insert(editor_data.view_id, Arc::new(editor_data));
    }
}

impl Widget<LapceTabData> for LapceSplitNew {
    fn id(&self) -> Option<WidgetId> {
        Some(self.split_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.widget.event(ctx, event, data, env);
        }
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::SplitEditor(vertical, view_id) => {
                        self.split_editor(ctx, data, *vertical, *view_id);
                    }
                    LapceUICommand::SplitEditorMove(direction, view_id) => {
                        self.split_editor_move(ctx, data, direction, *view_id);
                    }
                    LapceUICommand::SplitEditorExchange(view_id) => {
                        self.split_editor_exchange(ctx, data, *view_id);
                    }
                    LapceUICommand::SplitEditorClose(view_id) => {
                        self.split_editor_close(ctx, data, *view_id);
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.widget.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.widget.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let my_size = bc.max();

        let children_len = self.children.len();
        if children_len == 0 {
            return my_size;
        }

        let mut non_flex_total = 0.0;
        for child in self.children.iter() {
            if !child.flex {
                non_flex_total += child.params;
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
            let width = if child.flex {
                (flex_total / flex_sum * child.params).round()
            } else {
                child.params
            };
            let size = Size::new(width, my_size.height);
            child
                .widget
                .layout(ctx, &BoxConstraints::new(size, size), data, env);
            child.widget.set_origin(ctx, data, env, Point::new(x, 0.0));
            child.layout_rect = child
                .layout_rect
                .with_size(size)
                .with_origin(Point::new(x, 0.0));
            x += width;
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for child in self.children.iter_mut() {
            child.widget.paint(ctx, data, env);
        }
        self.paint_bar(ctx, env);
    }
}

pub struct LapceSplit {
    window_id: WindowId,
    tab_id: WidgetId,
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
    pub fn new(window_id: WindowId, tab_id: WidgetId, vertical: bool) -> Self {
        LapceSplit {
            window_id,
            tab_id,
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
                        LapceUICommand::ApplyEdits(offset, rev, edits) => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
                            if *offset
                                != editor_split
                                    .editors
                                    .get(&editor_split.active)
                                    .unwrap()
                                    .selection
                                    .get_cursor_offset()
                            {
                                return;
                            }
                            editor_split.apply_edits(ctx, data, *rev, edits);
                        }
                        LapceUICommand::ApplyEditsAndSave(offset, rev, result) => {
                            LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id)
                                .editor_split
                                .lock()
                                .apply_edits_and_save(
                                    ctx, data, *offset, *rev, result,
                                );
                        }
                        LapceUICommand::GotoLocation(location) => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
                            editor_split.go_to_location(ctx, data, location, env);
                        }
                        LapceUICommand::Split(vertical) => {
                            if self.children.len() <= 1 {
                                self.vertical = *vertical;
                            }
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
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

                                let mut new_editor = editor_split
                                    .editors
                                    .get(&active)
                                    .unwrap()
                                    .clone();
                                new_editor.view_id = WidgetId::next();
                                new_editor.editor_id = WidgetId::next();
                                let scroll_offset = new_editor.scroll_offset;
                                let new_editor_id = new_editor.editor_id.clone();
                                let new_view_id = new_editor.view_id.clone();
                                let split_id = new_editor.split_id.clone();
                                let tab_id = new_editor.tab_id.clone();
                                editor_split
                                    .editors
                                    .insert(new_view_id.clone(), new_editor);

                                let new_editor_ui = data.get_editor(&active).clone();
                                Arc::make_mut(&mut data.editors)
                                    .insert(new_view_id.clone(), new_editor_ui);

                                let mut new_editor_view = EditorView::new(
                                    self.window_id,
                                    tab_id,
                                    split_id,
                                    new_view_id,
                                    new_editor_id,
                                );
                                new_editor_view.editor.widget_mut().force_scroll_to(
                                    scroll_offset.x,
                                    scroll_offset.y,
                                );

                                let new_child = ChildWidget {
                                    widget: WidgetPod::new(new_editor_view).boxed(),
                                    flex: true,
                                    params: 1.0,
                                    layout_rect: Rect::ZERO,
                                };
                                self.children.insert(index + 1, new_child);
                                self.even_flex_children();
                                ctx.children_changed();
                            }
                        }
                        LapceUICommand::SplitClose => {
                            if self.children.len() == 1 {
                                return;
                            }
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
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
                            Arc::make_mut(&mut data.editors).remove(&active);
                            editor_split.active = new_active;
                            if let Some(buffer_id) = buffer_id {
                                editor_split
                                    .clear_buffer_text_layouts(data, buffer_id);
                            }

                            self.even_flex_children();
                            ctx.children_changed();
                        }
                        LapceUICommand::SplitExchange => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
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
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut editor_split = state.editor_split.lock();
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
                if child.widget.is_initialized() {
                    child.widget.event(ctx, event, data, env);
                }
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
                if child.widget.is_initialized() {
                    child.widget.event(ctx, event, data, env);
                }
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
