use std::sync::Arc;

use druid::{
    kurbo::{Line, Rect},
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target, UpdateCtx, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceConfig, LapceTheme},
    data::{FocusArea, LapceEditorData, LapceTabData, SplitContent, SplitData},
    keypress::{Alignment, DefaultKeyPressHandler, KeyMap},
    panel::PanelKind,
    split::{SplitDirection, SplitMoveDirection},
    terminal::LapceTerminalData,
};
use lapce_rpc::terminal::TermId;

use crate::{
    editor::{
        tab::LapceEditorTab,
        view::{editor_tab_child_widget, LapceEditorView},
    },
    terminal::LapceTerminalView,
};

struct LapceDynamicSplit {
    widget_id: WidgetId,
    children: Vec<ChildWidget>,
}

pub fn split_data_widget(split_data: &SplitData, data: &LapceTabData) -> LapceSplit {
    let mut split =
        LapceSplit::new(split_data.widget_id).direction(split_data.direction);
    for child in split_data.children.iter() {
        let child = split_content_widget(child, data);
        split = split.with_flex_child(child, None, 1.0, true);
    }
    split
}

pub fn split_content_widget(
    content: &SplitContent,
    data: &LapceTabData,
) -> Box<dyn Widget<LapceTabData>> {
    match content {
        SplitContent::EditorTab(widget_id) => {
            let editor_tab_data =
                data.main_split.editor_tabs.get(widget_id).unwrap();
            let mut editor_tab = LapceEditorTab::new(editor_tab_data.widget_id);
            for child in editor_tab_data.children.iter() {
                let child = editor_tab_child_widget(child, data);
                editor_tab = editor_tab.with_child(child);
            }
            editor_tab.boxed()
        }
        SplitContent::Split(widget_id) => {
            let split_data = data.main_split.splits.get(widget_id).unwrap();
            let mut split =
                LapceSplit::new(*widget_id).direction(split_data.direction);
            for content in split_data.children.iter() {
                split = split.with_flex_child(
                    split_content_widget(content, data),
                    None,
                    1.0,
                    true,
                );
            }
            split.boxed()
        }
    }
}

impl Widget<LapceTabData> for LapceDynamicSplit {
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
        env: &Env,
    ) -> Size {
        let my_size = bc.max();

        let split = data.main_split.splits.get(&self.widget_id).unwrap();
        let children_len = self.children.len();
        if children_len == 0 {
            return my_size;
        }

        let mut flex_sum = 0.0;
        for child in &self.children {
            flex_sum += child.params;
        }
        let flex_total = if split.direction == SplitDirection::Vertical {
            my_size.width
        } else {
            my_size.height
        };
        let flex_unit = flex_total / flex_sum;

        let mut x = 0.0;
        let mut y = 0.0;
        for child in self.children.iter_mut() {
            let flex = flex_unit * child.params;
            let (width, height) = match split.direction {
                SplitDirection::Vertical => (flex, my_size.height),
                SplitDirection::Horizontal => (my_size.width, flex),
            };
            let size = Size::new(width, height);
            child
                .widget
                .layout(ctx, &BoxConstraints::tight(size), data, env);
            child.widget.set_origin(ctx, data, env, Point::new(x, y));
            child.layout_rect = child
                .layout_rect
                .with_origin(Point::new(x, y))
                .with_size(size);
            match split.direction {
                SplitDirection::Vertical => x += child.layout_rect.size().width,
                SplitDirection::Horizontal => y += child.layout_rect.size().height,
            }
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for child in self.children.iter_mut() {
            child.widget.paint(ctx, data, env);
        }
    }
}

pub struct LapceSplit {
    split_id: WidgetId,
    children: Vec<ChildWidget>,
    children_ids: Vec<WidgetId>,
    direction: SplitDirection,
    show_border: bool,
    commands: Vec<(LapceCommand, PietTextLayout, Rect, Option<KeyMap>)>,
    panel: Option<PanelKind>,
    /// Whether the resize bar is hovered  
    /// Contains the [`WidgetId`] of the child we are resizing
    bar_hovered: Option<WidgetId>,
    /// The sum of the non flex child sizes  
    /// This is updated whenever we layout
    non_flex_total: f64,
    /// The total size of the split  
    /// This is updated whenever we layout
    total_size: f64,
}

struct ChildWidget {
    widget: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
    /// Whether it can be resized through the UI
    resizable: bool,
    /// The offset (in the direction, so x0 or y0) to use when resizing
    /// This is used to avoid noise accumulation in the floating point math when resizing
    resize_pos: f64,
    /// Whether we should update the resize pos, even if we are currently resizing.
    update_resize_pos: bool,
}

impl LapceSplit {
    pub fn new(split_id: WidgetId) -> Self {
        Self {
            split_id,
            children: Vec::new(),
            children_ids: Vec::new(),
            direction: SplitDirection::Vertical,
            show_border: true,
            commands: vec![],
            panel: None,
            bar_hovered: None,
            non_flex_total: 0.0,
            total_size: 0.0,
        }
    }

    pub fn direction(mut self, direction: SplitDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set the panel kind on the split, so that split can
    /// determine the split direction based on the position
    /// of the panel
    pub fn panel(mut self, panel: PanelKind) -> Self {
        self.panel = Some(panel);
        self
    }

    pub fn horizontal(mut self) -> Self {
        self.direction = SplitDirection::Horizontal;
        self
    }

    pub fn hide_border(mut self) -> Self {
        self.show_border = false;
        self
    }

    pub fn with_flex_child(
        mut self,
        child: Box<dyn Widget<LapceTabData>>,
        child_id: Option<WidgetId>,
        params: f64,
        resizable: bool,
    ) -> Self {
        let child = ChildWidget {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
            resizable,
            resize_pos: 0.0,
            update_resize_pos: true,
        };
        self.children_ids
            .push(child_id.unwrap_or_else(|| child.widget.id()));
        self.children.push(child);
        self
    }

    pub fn with_child(
        mut self,
        child: Box<dyn Widget<LapceTabData>>,
        child_id: Option<WidgetId>,
        params: f64,
    ) -> Self {
        let child = ChildWidget {
            widget: WidgetPod::new(child),
            flex: false,
            params,
            resizable: false,
            layout_rect: Rect::ZERO,
            resize_pos: 0.0,
            update_resize_pos: true,
        };
        self.children_ids
            .push(child_id.unwrap_or_else(|| child.widget.id()));
        self.children.push(child);
        self
    }

    pub fn replace_child(
        &mut self,
        index: usize,
        child: Box<dyn Widget<LapceTabData>>,
    ) {
        let _old_id = self.children[index].widget.id();
        let old_child = &mut self.children[index];
        old_child.widget = WidgetPod::new(child);
        let new_id = old_child.widget.id();
        self.children_ids[index] = new_id;
    }

    pub fn insert_flex_child(
        &mut self,
        index: usize,
        child: Box<dyn Widget<LapceTabData>>,
        child_id: Option<WidgetId>,
        params: f64,
        resizable: bool,
    ) -> WidgetId {
        let child = ChildWidget {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
            resizable,
            resize_pos: 0.0,
            update_resize_pos: true,
        };
        let child_id = child_id.unwrap_or_else(|| child.widget.id());
        self.children_ids.insert(index, child_id);
        self.children.insert(index, child);
        child_id
    }

    pub fn even_flex_children(&mut self) {
        for child in self.children.iter_mut() {
            if child.flex {
                child.params = 1.0;
            }
        }
    }

    /// Returns the child whose border we are resizing 'at'
    fn resize_bar_hit_test(&self, mouse_pos: Point) -> Option<&ChildWidget> {
        // Currently we don't support resizing splits with non flex children
        // This should be fixed.
        if self.has_non_flex_children() {
            return None;
        }

        // TODO: We probably aren't being as restrictive about what outofbounds positions are allowed as we should!
        // We only check the resize bar for the 'second' child, since it's left/upper
        // bar is the actual bar we want to use for resizing.
        // We currently only consider flex children, but this could perhaps be extended?
        for child in self
            .children
            .iter()
            .skip(1)
            .filter(|ch| ch.flex && ch.resizable)
        {
            // TODO: provide information about which child widget this is, so that we can actually resize it!
            if self.direction == SplitDirection::Vertical {
                let x = child.layout_rect.x0;
                if mouse_pos.x >= x - 2.0 && mouse_pos.x <= x + 2.0 {
                    return Some(child);
                }
            } else {
                let y = child.layout_rect.y0;
                if mouse_pos.y >= y - 2.0 && mouse_pos.y <= y + 2.0 {
                    return Some(child);
                }
            }
        }

        None
    }

    fn get_hovered_child_index(&self) -> Option<usize> {
        if let Some(child_id) = self.bar_hovered {
            self.children
                .iter()
                .enumerate()
                .find(|(_, c)| c.widget.id() == child_id)
                .map(|(i, _)| i)
        } else {
            None
        }
    }

    fn update_resize_point(&mut self, mouse_pos: Point) {
        if let Some(i) = self.get_hovered_child_index() {
            // We want to move the start edge of the editor to be where the mouse is, since they're dragging that edge
            let start = match self.direction {
                SplitDirection::Vertical => mouse_pos.x,
                SplitDirection::Horizontal => mouse_pos.y,
            };

            self.shift_start_of_child(i, start, true);
        }
    }

    /// Shift the x0/y0 (start) of the specific child at `i`  
    /// `allow_shifting` decides whether we should shift other children if the start is sufficiently far back
    /// this is for dragging an editor to the left and causing other editors to the left to also be dragged
    fn shift_start_of_child(&mut self, i: usize, start: f64, allow_shifting: bool) {
        // We can't move the start of the zeroth entry, and it isn't meaningful to move the start
        // of anything past the end
        if i == 0 || i >= self.children.len() {
            return;
        }

        // TODO: We should implement support for a mix of flex and non-flex children
        // though that complicates things somewhat, and for most cases that we need resizing for
        // they are all flex. We'd need to do some more logic to shift the indices to the flex
        // entries that we want to consider, and bound the movement of the splits to the non-flex entries.
        if self.has_non_flex_children() {
            return;
        }

        let start = start.round();
        let flex_total = self.total_size - self.non_flex_total;
        // TODO: let the margin be customizable? Also, is this the best margin we could use?
        // Limits how close the resize can get to another get to another editor or the edge
        let limit_margin = (0.05 * flex_total).max(5.0);

        // Constrain the start position to be within the bounds of the editor, and
        // after the previous editor.
        let prev_offset = self
            .children
            .get(i - 1)
            .map(|ch| ch.resize_pos)
            .unwrap_or(0.0);
        let next_offset = self
            .children
            .get(i + 1)
            .map(|ch| ch.resize_pos)
            .unwrap_or(flex_total);

        let prev_limit = prev_offset + limit_margin;
        let next_limit = next_offset - limit_margin;

        let start = if allow_shifting && start <= prev_limit {
            // If the start is before the previous offset, then we can start moving the previous editor
            start.max(limit_margin * i as f64).min(next_limit)
        } else if allow_shifting && start >= next_limit {
            start
                .max(prev_limit)
                .min(flex_total - limit_margin * (self.children.len() - i) as f64)
        } else {
            start.max(prev_limit).min(next_limit)
        };

        // Check if we're shifting a specific previous child
        let is_shifting_prev = |child_i: usize, child_offset: f64| -> bool {
            allow_shifting
                && child_i < i
                && child_i != 0
                && start <= child_offset + limit_margin * (i - child_i) as f64
        };

        // Check if we're shifting a specific child after us
        let is_shifting_after = |child_i: usize, child_offset: f64| -> bool {
            allow_shifting
                && child_i > i
                && start >= child_offset - limit_margin * (child_i - i) as f64
        };

        // Get the offset/position we want a child to start at. Skips non-flex entries as if they weren't there
        // This uses the existing position for every child except the one we are resizing
        let get_offset = |child_i: usize, children: &[ChildWidget]| -> f64 {
            let child = &children[child_i];
            // We use resize pos instead of the layout_rect.x0 because that gets rounded a bit and even if we didn't round
            // there is some noise inherent in the calculation done in the layout.
            // Thus we keep track of a separate position, which we only allow to update (to the layout_rect.x0 value) whenever
            // we actually would have caused a change.
            let offset = child.resize_pos;

            if child_i == i {
                // We return the position we want to put the editor at, rather than whatever its
                // actual position is
                start
            } else if is_shifting_prev(child_i, offset) {
                // If we're shifting an editor then we need to modify its position relative to the mouse
                // and shift it by the limit_margin so that it is shifted at a distance
                let shift_size = (i - child_i) as f64;
                (start - limit_margin * shift_size)
                    .max(0.0 + limit_margin * child_i as f64)
            } else if is_shifting_after(child_i, offset) {
                let shift_size = (child_i - i) as f64;
                let from_end_shift = (children.len() - child_i) as f64;
                (start + limit_margin * shift_size)
                    // .min(flex_total - limit_margin * shift_size + limit_margin)
                    .min(flex_total - limit_margin * from_end_shift)
            } else {
                // Otherwise, we just have the editor use its current position
                offset
            }
        };

        // For more than two children, we can't just update a single param to shift the start of any child
        // We have to update multiple. There might be a way to just update a few params, rather than every single
        // param, but given the amount of expected splits, it just isn't worth the extra effort.
        // The basic logic is that we have:
        // x_i = T * c_{i-1} / (sum c)
        // aka x_i = flex_total * children[i].params / children.iter().map(|ch| ch.params).sum()
        // So we have some x positions (existing x positions and the new one after resizing) and we want to
        // get the params that would produce those x positions.
        // However, this equation has an infinite number of solutions, so no single answer awaits us
        // but, with algebra we can get:
        // c_i = k * (x_{i + 1} - x_i)/T
        // where k is any positive non-zero value
        // so we can just choose k = 1
        // we could also have chosen k = T to just get: c_i = x_{i + 1} - xi
        // but normalizing by T gets us a nice percentage-like behavior (though, only after a resize!)
        // or we could have chosen a k s.t. the sum is always children.len(), which would
        // match the values it has when you initialize (since they default to a param of 1.0)
        // Currently we don't rely on that, but if we want to do that, then it is pretty simple.
        for child_i in 0..self.children.len() {
            let next_child_i = child_i + 1;

            // If we've caused a change, then allow the stored position to update.
            // This still allows a tiny bit of noise, but it is more of a tiny jitter rather than the
            // constant shifting to the side that we would get without this method.
            if child_i == i
                || child_i == 0
                || is_shifting_prev(child_i, self.children[child_i].resize_pos)
                || is_shifting_after(child_i, self.children[child_i].resize_pos)
            {
                self.children[child_i].update_resize_pos = true;
            }

            // x_i
            let start_offset = get_offset(child_i, &self.children);
            // x_{i+1}
            // If it is the last entry, just use the total
            let end_offset = if next_child_i >= self.children.len() {
                flex_total
            } else {
                get_offset(next_child_i, &self.children)
            };

            // x_{i+1} - x_i
            let diff = end_offset - start_offset;
            self.children[child_i].params = diff / flex_total;
        }

        // TODO: Post-sanity check that ensures everything is inside the editor bounds?
        // While this shouldn't occur, it would be good to ensure it simply doesn't happen.

        // TODO: We get negative box constraints in layouting, which is unfortunate.

        // TODO: While resizing the split arrows flicker since the mouse is constantly moving between the sides.
        // TODO: Write to db the sizes so it gets restored? Though this shouldn't be done every single call to this!
        //  Probably just when we reset the bar_hovered
    }

    fn has_non_flex_children(&self) -> bool {
        self.children.iter().any(|ch| !ch.flex)
    }

    fn paint_bar(&mut self, ctx: &mut PaintCtx, config: &LapceConfig) {
        let children_len = self.children.len();
        if children_len <= 1 {
            return;
        }

        let hover_i = if ctx.is_hot() || ctx.is_active() {
            self.get_hovered_child_index()
        } else {
            None
        };

        let size = ctx.size();
        for i in 1..children_len {
            let line = if self.direction == SplitDirection::Vertical {
                let x = self.children[i].layout_rect.x0;

                Line::new(Point::new(x, 0.0), Point::new(x, size.height))
            } else {
                let y = self.children[i].layout_rect.y0;

                Line::new(Point::new(0.0, y), Point::new(size.width, y))
            };

            // Match the panel resize bar if we're hovering over the editor split bar
            let (color, width) = if Some(i) == hover_i {
                (LapceTheme::EDITOR_CARET, 2.0)
            } else {
                (LapceTheme::LAPCE_BORDER, 1.0)
            };
            let color = config.get_color_unchecked(color);

            ctx.stroke(line, color, width);
        }
    }

    pub fn split_editor_close(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        widget_id: WidgetId,
    ) {
        if self.children.is_empty() {
            return;
        }

        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        if self.children.len() > 1 {
            let new_index = if index >= self.children.len() - 1 {
                index - 1
            } else {
                index + 1
            };
            let new_view_id = self.children[new_index].widget.id();
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(new_view_id),
            ));
        } else {
            data.main_split.active = Arc::new(None);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(self.split_id),
            ));
        }
        let view_id = self.children[index].widget.id();
        data.main_split.editors.remove(&view_id);
        self.children.remove(index);
        self.children_ids.remove(index);

        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_editor_exchange(
        &mut self,
        ctx: &mut EventCtx,
        _data: &mut LapceTabData,
        widget_id: WidgetId,
    ) {
        if self.children.len() <= 1 {
            return;
        }

        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }
        if index >= self.children.len() - 1 {
            return;
        }

        let new_child = self.children_ids[index + 1];
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(new_child),
        ));

        self.children.swap(index, index + 1);
        self.children_ids.swap(index, index + 1);

        ctx.request_layout();
    }

    pub fn split_editor_move(
        &mut self,
        ctx: &mut EventCtx,
        _data: &mut LapceTabData,
        direction: &SplitMoveDirection,
        widget_id: WidgetId,
    ) {
        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        let new_index = if self.direction == SplitDirection::Vertical {
            match direction {
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
            }
        } else {
            match direction {
                SplitMoveDirection::Up => {
                    if index == 0 {
                        return;
                    }
                    index - 1
                }
                SplitMoveDirection::Down => {
                    if index >= self.children.len() - 1 {
                        return;
                    }
                    index + 1
                }
                _ => index,
            }
        };

        if new_index != index {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(self.children_ids[new_index]),
            ));
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::EnsureCursorVisible(None),
                Target::Widget(self.children_ids[new_index]),
            ));
        }
    }

    pub fn split_terminal(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        _vertical: bool,
        widget_id: WidgetId,
    ) {
        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        let terminal_data = Arc::new(LapceTerminalData::new(
            data.workspace.clone(),
            self.split_id,
            ctx.get_external_handle(),
            data.proxy.clone(),
            &data.config,
            None,
        ));
        let terminal = LapceTerminalView::new(&terminal_data);
        Arc::make_mut(&mut data.terminal)
            .active_terminal_split_mut()
            .unwrap()
            .terminals
            .insert(terminal_data.term_id, terminal_data.clone());

        self.insert_flex_child(
            index + 1,
            terminal.boxed(),
            Some(terminal_data.widget_id),
            1.0,
            true,
        );
        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_terminal_close(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        term_id: TermId,
        widget_id: WidgetId,
    ) {
        if self.children.is_empty() {
            return;
        }

        if self.children.len() == 1 {
            Arc::make_mut(&mut data.terminal)
                .active_terminal_split_mut()
                .unwrap()
                .terminals
                .remove(&term_id);
            self.children.remove(0);
            self.children_ids.remove(0);

            self.even_flex_children();
            ctx.children_changed();
            return;
        }

        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        let new_index = if index >= self.children.len() - 1 {
            index - 1
        } else {
            index + 1
        };
        let _terminal_id = self.children_ids[index];
        let new_terminal_id = self.children_ids[new_index];
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(new_terminal_id),
        ));

        Arc::make_mut(&mut data.terminal)
            .active_terminal_split_mut()
            .unwrap()
            .terminals
            .remove(&term_id);
        self.children.remove(index);
        self.children_ids.remove(index);

        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_replace(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        index: usize,
        content: &SplitContent,
    ) {
        let new_widget = split_content_widget(content, data);
        self.replace_child(index, new_widget.boxed());
        ctx.children_changed();
    }

    pub fn split_exchange(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        content: &SplitContent,
    ) {
        let split_data = data.main_split.splits.get_mut(&self.split_id).unwrap();
        let split_data = Arc::make_mut(split_data);

        if split_data.children.len() <= 1 {
            return;
        }

        let mut index = 0;
        for (i, c) in split_data.children.iter().enumerate() {
            if c == content {
                index = i;
                break;
            }
        }

        if index >= split_data.children.len() - 1 {
            return;
        }

        split_data.children.swap(index, index + 1);
        self.children.swap(index, index + 1);

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(split_data.children[index].widget_id()),
        ));

        ctx.request_layout();
    }

    pub fn split_remove(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        content: &SplitContent,
    ) {
        let split_data = data.main_split.splits.get_mut(&self.split_id).unwrap();
        let split_data = Arc::make_mut(split_data);

        let mut index = 0;
        for (i, c) in split_data.children.iter().enumerate() {
            if c == content {
                index = i;
                break;
            }
        }

        self.children.remove(index);
        ctx.children_changed();

        let removed_child = split_data.children.remove(index);
        let is_active = match removed_child {
            SplitContent::EditorTab(tab_id) => {
                data.main_split.editor_tabs.remove(&tab_id);
                *data.main_split.active_tab == Some(tab_id)
            }
            SplitContent::Split(split_id) => {
                data.main_split.splits.remove(&split_id);
                false
            }
        };

        let split_data = data.main_split.splits.get_mut(&self.split_id).unwrap();
        let split_children_len = split_data.children.len();
        if split_children_len == 0 {
            if let Some(parent_split) = split_data.parent_split {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitRemove(SplitContent::Split(self.split_id)),
                    Target::Widget(parent_split),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(self.split_id),
                ));
                data.main_split.active = Arc::new(None);
                data.main_split.active_tab = Arc::new(None);
            }
        } else {
            if is_active {
                let new_index = if index > split_children_len - 1 {
                    split_children_len - 1
                } else {
                    index
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(split_data.children[new_index].widget_id()),
                ));
            }
            if split_children_len == 1 {
                let split_content = split_data.children[0];
                if let Some(parent_split_id) = split_data.parent_split {
                    let parent_split =
                        data.main_split.splits.get_mut(&parent_split_id).unwrap();
                    let parent_split = Arc::make_mut(parent_split);
                    if let Some(index) = parent_split
                        .children
                        .iter()
                        .position(|c| c == &SplitContent::Split(self.split_id))
                    {
                        parent_split.children[index] = split_content;
                        split_content
                            .set_split_id(&mut data.main_split, parent_split_id);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::SplitReplace(index, split_content),
                            Target::Widget(parent_split_id),
                        ));
                        data.main_split.splits.remove(&self.split_id);
                    }
                }
            }
        }
    }

    pub fn split_add(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        index: usize,
        content: &SplitContent,
        focus_new: bool,
    ) {
        let new_child = split_content_widget(content, data);
        self.insert_flex_child(index, new_child, None, 1.0, true);
        self.even_flex_children();
        ctx.children_changed();
        if focus_new {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(content.widget_id()),
            ));
        }
    }

    pub fn split_editor(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        _vertical: bool,
        widget_id: WidgetId,
    ) {
        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        let view_id = self.children[index].widget.id();
        let from_editor = data.main_split.editors.get(&view_id).unwrap();
        let mut editor_data = LapceEditorData::new(
            None,
            None,
            Some(self.split_id),
            from_editor.content.clone(),
            &data.config,
        );
        editor_data.cursor = from_editor.cursor.clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ForceScrollTo(
                from_editor.scroll_offset.x,
                from_editor.scroll_offset.y,
            ),
            Target::Widget(editor_data.view_id),
        ));

        let editor = LapceEditorView::new(
            editor_data.view_id,
            editor_data.editor_id,
            editor_data.find_view_id,
        );
        self.insert_flex_child(
            index + 1,
            editor.boxed(),
            Some(editor_data.view_id),
            1.0,
            true,
        );
        self.even_flex_children();
        ctx.children_changed();
        data.main_split
            .insert_editor(Arc::new(editor_data), &data.config);
    }
}

impl Widget<LapceTabData> for LapceSplit {
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
        match event {
            Event::MouseUp(mouse_event) => {
                if mouse_event.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                }
            }
            Event::MouseDown(mouse_event) => {
                if mouse_event.button.is_left() {
                    if let Some(child) = self.resize_bar_hit_test(mouse_event.pos) {
                        self.bar_hovered = Some(child.widget.id());
                        ctx.set_active(true);
                        ctx.set_handled();
                        return;
                    }
                }

                if self.children.is_empty() {
                    for (cmd, _, rect, _) in &self.commands {
                        if rect.contains(mouse_event.pos) {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                cmd.clone(),
                                Target::Auto,
                            ));
                            return;
                        }
                    }
                }
            }
            Event::KeyDown(key_event) => {
                if self.children.is_empty() {
                    ctx.set_handled();
                    let mut keypress = data.keypress.clone();
                    Arc::make_mut(&mut keypress).key_down(
                        ctx,
                        key_event,
                        &mut DefaultKeyPressHandler {},
                        env,
                    );
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        if let Some(split_data) =
                            data.main_split.splits.get(&self.split_id)
                        {
                            if !split_data.children.is_empty() {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(
                                        split_data.children[0].widget_id(),
                                    ),
                                ));
                            } else {
                                ctx.request_focus();
                                data.focus = Arc::new(self.split_id);
                                data.focus_area = FocusArea::Editor;
                            }
                        }
                    }
                    LapceUICommand::SplitAdd(usize, content, focus_new) => {
                        self.split_add(ctx, data, *usize, content, *focus_new);
                    }
                    LapceUICommand::SplitRemove(content) => {
                        self.split_remove(ctx, data, content);
                    }
                    LapceUICommand::SplitReplace(usize, content) => {
                        self.split_replace(ctx, data, *usize, content);
                    }
                    LapceUICommand::SplitChangeDirection(direction) => {
                        self.direction = *direction;
                    }
                    LapceUICommand::SplitExchange(content) => {
                        self.split_exchange(ctx, data, content);
                    }
                    LapceUICommand::SplitEditor(vertical, widget_id) => {
                        self.split_editor(ctx, data, *vertical, *widget_id);
                    }
                    LapceUICommand::SplitEditorMove(direction, widget_id) => {
                        self.split_editor_move(ctx, data, direction, *widget_id);
                    }
                    LapceUICommand::SplitEditorExchange(widget_id) => {
                        self.split_editor_exchange(ctx, data, *widget_id);
                    }
                    LapceUICommand::SplitEditorClose(widget_id) => {
                        self.split_editor_close(ctx, data, *widget_id);
                    }
                    LapceUICommand::SplitTerminal(vertical, widget_id) => {
                        self.split_terminal(ctx, data, *vertical, *widget_id);
                    }
                    LapceUICommand::SplitTerminalClose(term_id, widget_id) => {
                        self.split_terminal_close(ctx, data, *term_id, *widget_id);
                    }
                    _ => (),
                }
                return;
            }
            _ => (),
        }

        for child in self.children.iter_mut() {
            child.widget.event(ctx, event, data, env);
        }

        if let Event::MouseMove(mouse_event) = event {
            if self.children.is_empty() {
                ctx.clear_cursor();
                for (_, _, rect, _) in &self.commands {
                    if rect.contains(mouse_event.pos) {
                        ctx.set_cursor(&druid::Cursor::Pointer);
                        break;
                    }
                }
            } else if ctx.is_active() {
                // If we're active then we're probably being dragged
                self.update_resize_point(mouse_event.pos);
                ctx.request_layout();
                ctx.set_handled();
            } else if data.drag.is_none() {
                let has_active =
                    self.children.iter().any(|child| child.widget.has_active());
                if has_active {
                    self.bar_hovered = None;
                    ctx.clear_cursor();
                } else {
                    // TODO: We probably want to make so you don't get highlighting for more than one editor
                    // resize bar at once, and so that you don't get highlighting/dragging for an editor resize bar
                    // and a panel resize bar! That means we need the tab to know.
                    if let Some(child) = self.resize_bar_hit_test(mouse_event.pos) {
                        self.bar_hovered = Some(child.widget.id());

                        ctx.set_cursor(match self.direction {
                            SplitDirection::Vertical => {
                                &druid::Cursor::ResizeLeftRight
                            }
                            SplitDirection::Horizontal => {
                                &druid::Cursor::ResizeUpDown
                            }
                        });
                    } else {
                        if self.bar_hovered.is_some() {
                            self.bar_hovered = None;
                            ctx.request_paint();
                        }
                        ctx.clear_cursor();
                    }
                }
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
        for child in self.children.iter_mut() {
            child.widget.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &LapceTabData,
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
        if let Some(panel) = self.panel {
            if let Some((_, pos)) = data.panel.panel_position(&panel) {
                if pos.is_bottom() {
                    self.direction = SplitDirection::Vertical;
                } else {
                    self.direction = SplitDirection::Horizontal;
                }
            }
        }

        let split_data = data.main_split.splits.get(&self.split_id);

        let children_len = self.children.len();
        if children_len == 0 {
            let origin =
                Point::new(my_size.width / 2.0 - 30.0, my_size.height / 2.0 + 40.0);
            let line_height = 35.0;

            self.commands = empty_editor_commands(
                data.config.core.modal,
                data.workspace.path.is_some(),
            )
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let text_layout = ctx
                    .text()
                    .new_text_layout(
                        cmd.kind.desc().unwrap_or_else(|| cmd.kind.str()),
                    )
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_LINK)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let point =
                    origin - (text_layout.size().width, -line_height * i as f64);
                let rect = text_layout.size().to_rect().with_origin(point);
                let keymap = data
                    .keypress
                    .command_keymaps
                    .get(cmd.kind.str())
                    .and_then(|keymaps| keymaps.get(0))
                    .cloned();
                (cmd.clone(), text_layout, rect, keymap)
            })
            .collect();
            return my_size;
        }

        self.non_flex_total = 0.0;
        let mut max_other_axis = 0.0;
        for child in self.children.iter_mut() {
            if !child.flex {
                let (width, height) = match self.direction {
                    SplitDirection::Vertical => (child.params, my_size.height),
                    SplitDirection::Horizontal => (my_size.width, child.params),
                };
                let size = Size::new(width, height);
                let size = child.widget.layout(
                    ctx,
                    &BoxConstraints::new(Size::ZERO, size),
                    data,
                    env,
                );
                self.non_flex_total += self.direction.main_size(size);
                let cross_size = self.direction.cross_size(size);
                if cross_size > max_other_axis {
                    max_other_axis = cross_size;
                }
                child.layout_rect = size.to_rect();
            };
        }

        let flex_sum = self
            .children
            .iter()
            .filter_map(|child| child.flex.then_some(child.params))
            .sum::<f64>();

        self.total_size = self.direction.main_size(my_size);
        let flex_total = self.total_size - self.non_flex_total;

        let mut next_origin = Point::ZERO;
        let children_len = self.children.len();
        for (i, child) in self.children.iter_mut().enumerate() {
            child.widget.set_origin(ctx, data, env, next_origin);
            child.layout_rect = child.layout_rect.with_origin(next_origin);

            if child.flex {
                let flex = if i == children_len - 1 {
                    match self.direction {
                        SplitDirection::Vertical => self.total_size - next_origin.x,
                        SplitDirection::Horizontal => {
                            self.total_size - next_origin.y
                        }
                    }
                } else {
                    (flex_total / flex_sum * child.params).round()
                };

                let (width, height) = match self.direction {
                    SplitDirection::Vertical => (flex, my_size.height),
                    SplitDirection::Horizontal => (my_size.width, flex),
                };
                let size = Size::new(width, height);
                if let Some(split_data) = split_data.as_ref() {
                    let parent_origin =
                        split_data.layout_rect.borrow().origin().to_vec2();
                    data.main_split.update_split_content_layout_rect(
                        split_data.children[i],
                        size.to_rect().with_origin(next_origin + parent_origin),
                    );
                }
                let size = child.widget.layout(
                    ctx,
                    &BoxConstraints::new(Size::ZERO, size),
                    data,
                    env,
                );
                child.widget.set_origin(ctx, data, env, next_origin);
                let cross_size = self.direction.cross_size(size);
                if cross_size > max_other_axis {
                    max_other_axis = cross_size;
                }

                child.layout_rect = child.layout_rect.with_size(size);
            }

            if self.bar_hovered.is_none() || child.update_resize_pos {
                // TODO: There's probably some bugs lurking with this handling of resize pos
                // most likely to happen if we resize the window while resizing a split
                child.resize_pos = self.direction.start(child.layout_rect);
                child.update_resize_pos = false;
            }

            let child_size = child.layout_rect.size();
            match self.direction {
                SplitDirection::Vertical => next_origin.x += child_size.width,
                SplitDirection::Horizontal => next_origin.y += child_size.height,
            }
        }

        match self.direction {
            SplitDirection::Vertical => Size::new(next_origin.x, max_other_axis),
            SplitDirection::Horizontal => Size::new(max_other_axis, next_origin.y),
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if self.children.is_empty() {
            let rect = ctx.size().to_rect();
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
            ctx.with_save(|ctx| {
                ctx.clip(rect);
                let svg = data.config.logo_svg();
                let size = ctx.size();
                let svg_size = 100.0;
                let rect = Size::ZERO
                    .to_rect()
                    .with_origin(
                        Point::new(size.width / 2.0, size.height / 2.0)
                            + (0.0, -svg_size),
                    )
                    .inflate(svg_size, svg_size);
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(
                        &data
                            .config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone()
                            .with_alpha(0.5),
                    ),
                );

                for (_cmd, text, rect, keymap) in &self.commands {
                    ctx.draw_text(text, rect.origin());
                    if let Some(keymap) = keymap {
                        let origin = rect.origin()
                            + (20.0 + rect.width(), rect.height() / 2.0);
                        keymap.paint(ctx, origin, Alignment::Left, &data.config);
                    }
                }
            });

            return;
        }
        for child in self.children.iter_mut() {
            child.widget.paint(ctx, data, env);
        }
        if let Some(panel) = self.panel {
            if let Some((_, pos)) = data.panel.panel_position(&panel) {
                if pos.is_bottom() {
                    self.show_border = true
                } else {
                    self.show_border = false
                }
            }
        }
        if self.show_border {
            self.paint_bar(ctx, &data.config);
        }
    }
}

fn empty_editor_commands(modal: bool, has_workspace: bool) -> Vec<LapceCommand> {
    if !has_workspace {
        vec![
            LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::PaletteCommand),
                data: None,
            },
            LapceCommand {
                kind: CommandKind::Workbench(if modal {
                    LapceWorkbenchCommand::DisableModal
                } else {
                    LapceWorkbenchCommand::EnableModal
                }),
                data: None,
            },
            LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::OpenFolder),
                data: None,
            },
            LapceCommand {
                kind: CommandKind::Workbench(
                    LapceWorkbenchCommand::PaletteWorkspace,
                ),
                data: None,
            },
        ]
    } else {
        vec![
            LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::PaletteCommand),
                data: None,
            },
            if modal {
                LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::DisableModal,
                    ),
                    data: None,
                }
            } else {
                LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::EnableModal),
                    data: None,
                }
            },
            LapceCommand {
                kind: CommandKind::Workbench(LapceWorkbenchCommand::Palette),
                data: None,
            },
        ]
    }
}
