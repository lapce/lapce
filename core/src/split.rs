use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{EditorContent, EditorType, LapceEditorData, LapceTabData, PanelData},
    editor::{EditorLocation, LapceEditorView},
    scroll::{LapcePadding, LapceScroll},
    terminal::{LapceTerminal, LapceTerminalData},
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
    children_ids: Vec<WidgetId>,
    vertical: bool,
    show_border: bool,
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
            children_ids: Vec::new(),
            vertical: true,
            show_border: true,
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.vertical = false;
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
    ) -> Self {
        let child = ChildWidgetNew {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children_ids
            .push(child_id.unwrap_or(child.widget.id()));
        self.children.push(child);
        self
    }

    pub fn with_child(
        mut self,
        child: Box<dyn Widget<LapceTabData>>,
        child_id: Option<WidgetId>,
        params: f64,
    ) -> Self {
        let child = ChildWidgetNew {
            widget: WidgetPod::new(child),
            flex: false,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children_ids
            .push(child_id.unwrap_or(child.widget.id()));
        self.children.push(child);
        self
    }

    pub fn insert_flex_child(
        &mut self,
        index: usize,
        child: Box<dyn Widget<LapceTabData>>,
        child_id: Option<WidgetId>,
        params: f64,
    ) {
        let child = ChildWidgetNew {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
        };
        self.children_ids
            .insert(index, child_id.unwrap_or(child.widget.id()));
        self.children.insert(index, child);
    }

    pub fn even_flex_children(&mut self) {
        for child in self.children.iter_mut() {
            if child.flex {
                child.params = 1.0;
            }
        }
    }

    fn paint_bar(&mut self, ctx: &mut PaintCtx, config: &Config) {
        let children_len = self.children.len();
        if children_len <= 1 {
            return;
        }

        let size = ctx.size();
        for i in 1..children_len {
            let line = if self.vertical {
                let x = self.children[i].layout_rect.x0;
                let line = Line::new(Point::new(x, 0.0), Point::new(x, size.height));
                line
            } else {
                let y = self.children[i].layout_rect.y0;
                let line = Line::new(Point::new(0.0, y), Point::new(size.width, y));
                line
            };
            ctx.stroke(
                line,
                config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }

    pub fn split_editor_close(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        widget_id: WidgetId,
    ) {
        if self.children.len() == 0 {
            return;
        }

        if self.children.len() == 1 {
            let view_id = self.children[0].widget.id();
            let editor = data.main_split.editors.get_mut(&view_id).unwrap();
            Arc::make_mut(editor).content = EditorContent::None;
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
        let view_id = self.children[index].widget.id();
        let new_view_id = self.children[new_index].widget.id();
        let new_editor = data.main_split.editors.get(&new_view_id).unwrap();
        if *data.main_split.active == view_id {
            data.main_split.active = Arc::new(new_editor.view_id);
            data.focus = new_editor.view_id;
            ctx.set_focus(new_editor.view_id);
        }
        data.main_split.editors.remove(&view_id);
        self.children.remove(index);
        self.children_ids.remove(index);

        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_editor_exchange(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
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

        let new_view_id = self.children[index + 1].widget.id();
        let new_editor = data.main_split.editors.get(&new_view_id).unwrap();
        data.main_split.active = Arc::new(new_editor.view_id);
        data.focus = new_editor.view_id;
        ctx.set_focus(new_editor.view_id);

        self.children.swap(index, index + 1);
        self.children_ids.swap(index, index + 1);

        ctx.request_layout();
    }

    pub fn split_editor_move(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
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

        let new_index = if self.vertical {
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
        vertical: bool,
        widget_id: WidgetId,
        panel_widget_id: Option<WidgetId>,
    ) {
        let mut index = 0;
        for (i, child_id) in self.children_ids.iter().enumerate() {
            if child_id == &widget_id {
                index = i;
                break;
            }
        }

        let terminal_data = Arc::new(LapceTerminalData::new(
            self.split_id,
            ctx.get_external_handle(),
            panel_widget_id,
        ));
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(&terminal_data));
        Arc::make_mut(&mut data.terminal)
            .terminals
            .insert(terminal_data.widget_id, terminal_data.clone());

        self.insert_flex_child(
            index + 1,
            terminal.boxed(),
            Some(terminal_data.widget_id),
            1.0,
        );
        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_terminal_close(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        widget_id: WidgetId,
        panel_widget_id: Option<WidgetId>,
    ) {
        if self.children.len() == 0 {
            return;
        }

        if self.children.len() == 1 {
            Arc::make_mut(&mut data.terminal)
                .terminals
                .remove(&widget_id);
            self.children.remove(0);
            self.children_ids.remove(0);

            self.even_flex_children();
            ctx.children_changed();
            if let Some(panel_id) = panel_widget_id {
                for (pos, panel) in data.panels.iter_mut() {
                    if panel.active == panel_id {
                        Arc::make_mut(panel).shown = false;
                    }
                }
            }
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(*data.main_split.active),
            ));
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
        let terminal_id = self.children_ids[index];
        let new_terminal_id = self.children_ids[new_index];
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(new_terminal_id),
        ));

        Arc::make_mut(&mut data.terminal)
            .terminals
            .remove(&terminal_id);
        self.children.remove(index);
        self.children_ids.remove(index);

        self.even_flex_children();
        ctx.children_changed();
    }

    pub fn split_editor(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        vertical: bool,
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
            Some(self.split_id),
            from_editor.content.clone(),
            EditorType::Normal,
            &data.config,
        );
        editor_data.cursor = from_editor.cursor.clone();
        editor_data.locations = from_editor.locations.clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ForceScrollTo(
                from_editor.scroll_offset.x,
                from_editor.scroll_offset.y,
            ),
            Target::Widget(editor_data.view_id),
        ));

        let editor = LapceEditorView::new(&editor_data);
        self.insert_flex_child(
            index + 1,
            editor.boxed(),
            Some(editor_data.view_id),
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
                    LapceUICommand::SplitTerminal(
                        vertical,
                        widget_id,
                        panel_widget_id,
                    ) => {
                        self.split_terminal(
                            ctx,
                            data,
                            *vertical,
                            *widget_id,
                            panel_widget_id.to_owned(),
                        );
                    }
                    LapceUICommand::SplitTerminalClose(
                        widget_id,
                        panel_widget_id,
                    ) => {
                        self.split_terminal_close(
                            ctx,
                            data,
                            *widget_id,
                            panel_widget_id.to_owned(),
                        );
                    }
                    LapceUICommand::InitTerminalPanel => {
                        if data.terminal.terminals.len() == 0 {
                            let terminal_data = Arc::new(LapceTerminalData::new(
                                data.terminal.split_id,
                                ctx.get_external_handle(),
                                Some(data.terminal.widget_id),
                            ));
                            let terminal = LapcePadding::new(
                                10.0,
                                LapceTerminal::new(&terminal_data),
                            );
                            self.insert_flex_child(
                                0,
                                terminal.boxed(),
                                Some(terminal_data.widget_id),
                                1.0,
                            );
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(terminal_data.widget_id),
                            ));
                            let terminal_panel = Arc::make_mut(&mut data.terminal);
                            terminal_panel.active = terminal_panel.widget_id;
                            terminal_panel
                                .terminals
                                .insert(terminal_data.widget_id, terminal_data);
                            ctx.children_changed();
                        }
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

        let flex_total = if self.vertical {
            my_size.width
        } else {
            my_size.height
        } - non_flex_total;

        let mut x = 0.0;
        let mut y = 0.0;
        for child in self.children.iter_mut() {
            let (width, height) = if self.vertical {
                let width = if child.flex {
                    (flex_total / flex_sum * child.params).round()
                } else {
                    child.params
                };
                let height = my_size.height;
                (width, height)
            } else {
                let height = if child.flex {
                    (flex_total / flex_sum * child.params).round()
                } else {
                    child.params
                };
                let width = my_size.width;
                (width, height)
            };
            let size = Size::new(width, height);
            child
                .widget
                .layout(ctx, &BoxConstraints::new(size, size), data, env);
            child.widget.set_origin(ctx, data, env, Point::new(x, y));
            child.layout_rect = child
                .layout_rect
                .with_size(size)
                .with_origin(Point::new(x, y));
            if self.vertical {
                x += width;
            } else {
                y += height;
            }
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for child in self.children.iter_mut() {
            child.widget.paint(ctx, data, env);
        }
        if self.show_border {
            self.paint_bar(ctx, &data.config);
        }
    }
}
