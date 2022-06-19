use crate::{
    editor::{tab::LapceEditorTab, view::LapceEditorView},
    settings::LapceSettingsPanel,
    terminal::LapceTerminalView,
};
use std::sync::Arc;

use crate::svg::logo_svg;
use druid::{
    kurbo::{Line, Rect},
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    Command, Target, WidgetId,
};
use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::{
        EditorTabChild, FocusArea, LapceEditorData, LapceTabData, PanelKind,
        SplitContent, SplitData,
    },
    keypress::{Alignment, DefaultKeyPressHandler, KeyMap},
    split::{SplitDirection, SplitMoveDirection},
    terminal::LapceTerminalData,
};
use lapce_rpc::terminal::TermId;

struct LapceDynamicSplit {
    widget_id: WidgetId,
    children: Vec<ChildWidget>,
}

pub fn split_data_widget(split_data: &SplitData, data: &LapceTabData) -> LapceSplit {
    let mut split =
        LapceSplit::new(split_data.widget_id).direction(split_data.direction);
    for child in split_data.children.iter() {
        let child = split_content_widget(child, data);
        split = split.with_flex_child(child, None, 1.0);
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
                match child {
                    EditorTabChild::Editor(view_id, editor_id, find_view_id) => {
                        let editor = LapceEditorView::new(
                            *view_id,
                            *editor_id,
                            *find_view_id,
                        )
                        .boxed();
                        editor_tab = editor_tab.with_child(editor);
                    }
                    EditorTabChild::Settings(widget_id, editor_tab_id) => {
                        let settings = LapceSettingsPanel::new(
                            data,
                            *widget_id,
                            *editor_tab_id,
                        )
                        .boxed();
                        editor_tab = editor_tab.with_child(settings);
                    }
                }
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
}

struct ChildWidget {
    pub widget: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    flex: bool,
    params: f64,
    layout_rect: Rect,
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
        }
    }

    pub fn direction(mut self, direction: SplitDirection) -> Self {
        self.direction = direction;
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
    ) -> Self {
        let child = ChildWidget {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
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
            layout_rect: Rect::ZERO,
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
    ) -> WidgetId {
        let child = ChildWidget {
            widget: WidgetPod::new(child),
            flex: true,
            params,
            layout_rect: Rect::ZERO,
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

    fn paint_bar(&mut self, ctx: &mut PaintCtx, config: &Config) {
        let children_len = self.children.len();
        if children_len <= 1 {
            return;
        }

        let size = ctx.size();
        for i in 1..children_len {
            let line = if self.direction == SplitDirection::Vertical {
                let x = self.children[i].layout_rect.x0;

                Line::new(Point::new(x, 0.0), Point::new(x, size.height))
            } else {
                let y = self.children[i].layout_rect.y0;

                Line::new(Point::new(0.0, y), Point::new(size.width, y))
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
        ));
        let terminal = LapceTerminalView::new(&terminal_data);
        Arc::make_mut(&mut data.terminal)
            .terminals
            .insert(terminal_data.term_id, terminal_data.clone());

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
        term_id: TermId,
        widget_id: WidgetId,
    ) {
        if self.children.is_empty() {
            return;
        }

        if self.children.len() == 1 {
            Arc::make_mut(&mut data.terminal).terminals.remove(&term_id);
            self.children.remove(0);
            self.children_ids.remove(0);

            self.even_flex_children();
            ctx.children_changed();
            for (_pos, panel) in data.panels.iter_mut() {
                if panel.active == PanelKind::Terminal {
                    Arc::make_mut(panel).shown = false;
                }
            }
            if let Some(active) = *data.main_split.active_tab {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(active),
                ));
            }
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

        Arc::make_mut(&mut data.terminal).terminals.remove(&term_id);
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
        self.insert_flex_child(index, new_child, None, 1.0);
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
        editor_data.locations = from_editor.locations.clone();
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
            Event::MouseMove(mouse_event) => {
                if self.children.is_empty() {
                    let mut on_command = false;
                    for (_, _, rect, _) in &self.commands {
                        if rect.contains(mouse_event.pos) {
                            on_command = true;
                            break;
                        }
                    }
                    if on_command {
                        ctx.set_cursor(&druid::Cursor::Pointer);
                    } else {
                        ctx.clear_cursor();
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
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
                                data.focus = self.split_id;
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
                    LapceUICommand::InitTerminalPanel(focus) => {
                        if data.terminal.terminals.is_empty() {
                            let terminal_data = Arc::new(LapceTerminalData::new(
                                data.workspace.clone(),
                                data.terminal.split_id,
                                ctx.get_external_handle(),
                                data.proxy.clone(),
                                &data.config,
                            ));
                            let terminal = LapceTerminalView::new(&terminal_data);
                            self.insert_flex_child(
                                0,
                                terminal.boxed(),
                                Some(terminal_data.widget_id),
                                1.0,
                            );
                            if *focus {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(terminal_data.widget_id),
                                ));
                            }
                            let terminal_panel = Arc::make_mut(&mut data.terminal);
                            terminal_panel.active = terminal_data.widget_id;
                            terminal_panel.active_term_id = terminal_data.term_id;
                            terminal_panel
                                .terminals
                                .insert(terminal_data.term_id, terminal_data);
                            ctx.children_changed();
                        }
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

        let split_data = data.main_split.splits.get(&self.split_id);

        let children_len = self.children.len();
        if children_len == 0 {
            let origin =
                Point::new(my_size.width / 2.0 - 30.0, my_size.height / 2.0 + 40.0);
            let line_height = 35.0;

            self.commands = empty_editor_commands(
                data.config.lapce.modal,
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
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
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

        let mut non_flex_total = 0.0;
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
                non_flex_total += self.direction.main_size(size);
                let cross_size = self.direction.cross_size(size);
                if cross_size > max_other_axis {
                    max_other_axis = cross_size;
                }
                child.layout_rect = size.to_rect();
            };
        }
        let non_flex_total = non_flex_total;

        let flex_sum = self
            .children
            .iter()
            .filter_map(|child| child.flex.then(|| child.params))
            .sum::<f64>();

        let total_size = self.direction.main_size(my_size);
        let flex_total = total_size - non_flex_total;

        let mut next_origin = Point::ZERO;
        let children_len = self.children.len();
        for (i, child) in self.children.iter_mut().enumerate() {
            child.widget.set_origin(ctx, data, env, next_origin);
            child.layout_rect = child.layout_rect.with_origin(next_origin);

            if child.flex {
                let flex = if i == children_len - 1 {
                    match self.direction {
                        SplitDirection::Vertical => total_size - next_origin.x,
                        SplitDirection::Horizontal => total_size - next_origin.y,
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
                let svg = logo_svg();
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
