use std::{cell::RefCell, rc::Rc, sync::Arc};

use druid::{
    kurbo::Line, BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{
        DragContent, EditorTabChild, FocusArea, LapceEditorTabData, LapceTabData,
        SplitContent,
    },
    db::EditorTabChildInfo,
    document::{BufferContent, LocalBufferKind},
    editor::TabRect,
    split::{SplitDirection, SplitMoveDirection},
};

use crate::editor::{
    tab_header::LapceEditorTabHeader, view::editor_tab_child_widget,
};

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

    fn close_all_children(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        for child in editor_tab.children.iter().rev() {
            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::SplitClose),
                    data: None,
                },
                Target::Widget(child.widget_id()),
            ));
        }
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
            if i >= editor_tab.children.len() - 1 {
                editor_tab.active -= 1;
            };
            if focus {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureEditorTabActiveVisible,
                    Target::Widget(editor_tab.widget_id),
                ));
            }
            editor_tab.children.remove(i)
        } else {
            if editor_tab.active > i {
                editor_tab.active -= 1;
            }
            editor_tab.children.remove(i)
        };
        if focus && !editor_tab.children.is_empty() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(editor_tab.children[editor_tab.active].widget_id()),
            ));
        }
        if delete {
            match removed_child {
                EditorTabChild::Editor(view_id, _, _) => {
                    if let Some(editor) = data.main_split.editors.remove(&view_id) {
                        if let BufferContent::Scratch(buffer_id, _) = editor.content
                        {
                            let exits_in_other_edits =
                                data.main_split.editors.iter().any(|(_, e)| {
                                    e.view_id != editor.view_id
                                        && e.content == editor.content
                                });
                            if !exits_in_other_edits {
                                data.main_split.scratch_docs.remove(&buffer_id);
                            }
                        }
                    }
                }
                EditorTabChild::Settings { .. } => {}
                EditorTabChild::Plugin { .. } => {}
            }
        }
    }

    fn mouse_up(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        if let Some((_, _, drag_content)) = data.drag.clone().as_ref() {
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

                                let new_editor_tab_id = WidgetId::next();
                                let mut child = child.clone();
                                child.set_editor_tab(data, new_editor_tab_id);
                                let mut new_editor_tab = LapceEditorTabData {
                                    widget_id: new_editor_tab_id,
                                    split: split_id,
                                    active: 0,
                                    children: vec![child.clone()].into(),
                                    layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                                    content_is_hot: Rc::new(RefCell::new(false)),
                                };

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
                                    LapceUICommand::EditorTabRemove(
                                        *from_index,
                                        false,
                                        false,
                                    ),
                                    Target::Widget(*from_id),
                                ));
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(child.widget_id()),
                                ));
                            }
                            None => {
                                if from_id == &self.widget_id {
                                    return;
                                }
                                let mut child = child.clone();
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
                                    LapceUICommand::EditorTabRemove(
                                        *from_index,
                                        false,
                                        false,
                                    ),
                                    Target::Widget(*from_id),
                                ));
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(child.widget_id()),
                                ));
                            }
                        }
                        *Arc::make_mut(&mut data.drag) = None;
                    }
                }
                DragContent::Panel(..) => {}
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
            }
            Event::MouseUp(mouse_event) => {
                self.mouse_up(ctx, data, mouse_event);
            }
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                ctx.set_handled();
                let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                if let CommandKind::Focus(FocusCommand::SplitVertical) = cmd.kind {
                    data.main_split.tab_split(
                        ctx,
                        self.widget_id,
                        SplitDirection::Vertical,
                        &data.config,
                    );
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                ctx.set_handled();
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::EditorContentChanged => {
                        self.header
                            .widget_mut()
                            .content
                            .widget_mut()
                            .child_mut()
                            .update_dedup_paths(data);
                    }
                    LapceUICommand::EditorTabAdd(index, content) => {
                        self.children.insert(
                            *index,
                            WidgetPod::new(editor_tab_child_widget(content, data)),
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
                        self.close_all_children(ctx, data);
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
                    LapceUICommand::EnsureEditorTabActiveVisible => {
                        if let Some(tab) =
                            data.main_split.editor_tabs.get(&self.widget_id)
                        {
                            if let Some(active) = tab.children.get(tab.active) {
                                match active.child_info(data) {
                                    EditorTabChildInfo::Editor(info) => {
                                        if info.content
                                            == BufferContent::Local(
                                                LocalBufferKind::Empty,
                                            )
                                        {
                                            // File has not yet been loaded, most likely.
                                            return;
                                        }

                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::ActiveFileChanged {
                                                path: if let BufferContent::File(
                                                    path,
                                                ) = info.content
                                                {
                                                    Some(path)
                                                } else {
                                                    None
                                                },
                                            },
                                            Target::Widget(
                                                data.file_explorer.widget_id,
                                            ),
                                        ));
                                    }
                                    EditorTabChildInfo::Settings => {}
                                    EditorTabChildInfo::Plugin { .. } => {}
                                }
                                return;
                            }
                        }
                    }
                    LapceUICommand::NextEditorTab => {
                        let editor_tab = data
                            .main_split
                            .editor_tabs
                            .get(&self.widget_id)
                            .unwrap();
                        if !editor_tab.children.is_empty() {
                            let new_index = if editor_tab.active
                                == editor_tab.children.len() - 1
                            {
                                0
                            } else {
                                editor_tab.active + 1
                            };

                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(
                                    editor_tab.children[new_index].widget_id(),
                                ),
                            ));
                        }
                    }
                    LapceUICommand::PreviousEditorTab => {
                        let editor_tab = data
                            .main_split
                            .editor_tabs
                            .get(&self.widget_id)
                            .unwrap();
                        if !editor_tab.children.is_empty() {
                            let new_index = if editor_tab.active == 0 {
                                editor_tab.children.len() - 1
                            } else {
                                editor_tab.active - 1
                            };

                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(
                                    editor_tab.children[new_index].widget_id(),
                                ),
                            ));
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.header.event(ctx, event, data, env);
        if event.should_propagate_to_hidden() {
            for child in self.children.iter_mut() {
                child.event(ctx, event, data, env);
            }
        } else {
            let tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
            self.children[tab.active].event(ctx, event, data, env);
        };
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

        let tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        self.children[tab.active].paint(ctx, data, env);
        if ctx.is_hot() && data.is_drag_editor() {
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
                ctx.with_save(|ctx| {
                    ctx.incr_alpha_depth();
                    ctx.fill(
                        rect,
                        data.config.get_color_unchecked(
                            LapceTheme::EDITOR_DRAG_DROP_BACKGROUND,
                        ),
                    );
                });
            }
        }
        ctx.with_save(|ctx| {
            ctx.incr_alpha_depth();
            self.header.paint(ctx, data, env);
        });
    }
}

pub trait TabRectRenderer {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        widget_id: WidgetId,
        tab_idx: usize,
        size: Size,
        mouse_pos: Option<Point>,
    );
}

impl TabRectRenderer for TabRect {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        widget_id: WidgetId,
        tab_idx: usize,
        size: Size,
        mouse_pos: Option<Point>,
    ) {
        let svg_size = data.config.ui.icon_size() as f64;
        let padding = 4.0;
        let editor_tab = data.main_split.editor_tabs.get(&widget_id).unwrap();

        // Draw tab separator before first tab
        if tab_idx == 0 {
            draw_tab_separator(ctx, data, self.rect.x0, size.height);
        }

        // Shrink our tab rect and move origin point by 1px so tab separator isn't drawn on the tab
        let self_rect = Size::new(self.rect.width() - 1., self.rect.height())
            .to_rect()
            .with_origin(Point::new(self.rect.x0 + 1., self.rect.y0));

        // Draw tab separator to the right of our tab rect
        draw_tab_separator(ctx, data, self_rect.x1, size.height);

        let svg_rect =
            Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(
                    self_rect.x0 + (svg_size) / 2.0,
                    self_rect.y0 + (size.height - svg_size) / 2.0,
                ));

        let is_active_tab = tab_idx == editor_tab.active;

        // Fill tab, simple
        ctx.fill(self_rect, get_tab_background_color(data, is_active_tab));
        // Stroke active tabs (active = selected but not necessarily focused)
        // e.g. a tab might be active in separate pane that is currently not focused
        if is_active_tab {
            let stroke = get_tab_stroke_color(data, widget_id);
            ctx.stroke(
                Line::new(
                    // p0 must be +(n+1) and p1 must be -(n) to have same offset
                    // don't ask me why
                    Point::new(self_rect.x0 + 3.0, self_rect.y1 - 2.0),
                    Point::new(self_rect.x1 - 2.0, self_rect.y1 - 2.0),
                ),
                stroke,
                2.0,
            );
        }
        ctx.draw_svg(&self.svg, svg_rect, self.svg_color.as_ref());
        ctx.draw_text(
            &self.text_layout,
            Point::new(svg_rect.x1 + 5.0, self.text_layout.y_offset(size.height)),
        );
        if let Some(path_layout) = &self.path_layout {
            ctx.draw_text(
                path_layout,
                Point::new(
                    svg_rect.x1 + 5.0 + self.text_layout.layout.width() as f64 + 5.0,
                    path_layout.y_offset(size.height),
                ),
            );
        }

        // See if any of the children have unsaved changes
        let is_pristine = match &editor_tab.children[tab_idx] {
            EditorTabChild::Editor(editor_id, _, _) => data
                .main_split
                .editor_doc(*editor_id)
                .buffer()
                .is_pristine(),
            EditorTabChild::Settings { .. } => true,
            EditorTabChild::Plugin { .. } => true,
        };

        if mouse_pos
            .map(|s| self.close_rect.contains(s))
            .unwrap_or(false)
        {
            ctx.fill(
                self.close_rect,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)
                    .clone()
                    .with_alpha(0.1),
            );
        }

        let mut draw_icon = |name: &'static str| {
            ctx.draw_svg(
                &data.config.ui_svg(name),
                self.close_rect.inflate(-padding, -padding),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                ),
            );
        };

        if mouse_pos.map(|s| self.rect.contains(s)).unwrap_or(false) {
            draw_icon(LapceIcons::CLOSE)
        } else if !is_pristine {
            draw_icon(LapceIcons::UNSAVED)
        } else if is_active_tab {
            if is_pristine {
                draw_icon(LapceIcons::CLOSE)
            } else {
                draw_icon(LapceIcons::UNSAVED)
            }
        }
    }
}

#[inline]
fn get_tab_background_color(data: &LapceTabData, active_tab: bool) -> &druid::Color {
    if active_tab {
        data.config
            .get_color_unchecked(LapceTheme::LAPCE_TAB_ACTIVE_BACKGROUND)
    } else {
        data.config
            .get_color_unchecked(LapceTheme::LAPCE_TAB_INACTIVE_BACKGROUND)
    }
}

#[inline]
fn get_tab_stroke_color(data: &LapceTabData, widget_id: WidgetId) -> &druid::Color {
    if data.focus_area == FocusArea::Editor
        && Some(widget_id) == *data.main_split.active_tab
    {
        data.config
            .get_color_unchecked(LapceTheme::LAPCE_TAB_ACTIVE_UNDERLINE)
    } else {
        data.config
            .get_color_unchecked(LapceTheme::LAPCE_TAB_INACTIVE_UNDERLINE)
    }
}

#[inline]
fn draw_tab_separator(
    ctx: &mut druid::PaintCtx<'_, '_, '_>,
    data: &LapceTabData,
    x: f64,
    y: f64,
) {
    ctx.stroke(
        Line::new(
            Point::new(x + 0.5, (y * 0.8).round()),
            Point::new(x + 0.5, y - (y * 0.8).round()),
        ),
        data.config
            .get_color_unchecked(LapceTheme::LAPCE_TAB_SEPARATOR),
        1.0,
    );
}
