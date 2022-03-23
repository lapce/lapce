use std::{
    cell::RefCell, collections::HashMap, iter::Iterator, path::PathBuf, rc::Rc,
    str::FromStr, sync::Arc, time::Instant,
};

use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontFamily, InternalEvent,
    InternalLifeCycle, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, MouseButton,
    MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout,
    UpdateCtx, Vec2, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    buffer::{matching_pair_direction, BufferContent, BufferId, LocalBufferKind},
    command::{
        CommandTarget, EnsureVisiblePosition, LapceCommand, LapceCommandNew,
        LapceUICommand, LapceWorkbenchCommand, LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::{
        DragContent, EditorTabChild, FocusArea, LapceEditorTabData, LapceTabData,
        PanelData, PanelKind, SplitContent,
    },
    editor::{EditorLocation, LapceEditorBufferData, TabRect},
    keypress::KeyPressFocus,
    menu::MenuItem,
    movement::{Movement, Selection},
    panel::PanelPosition,
    split::{SplitDirection, SplitMoveDirection},
    state::{Mode, VisualMode},
};
use lsp_types::{DocumentChanges, TextEdit, Url, WorkspaceEdit};
use strum::EnumMessage;

use crate::{
    find::FindBox,
    scroll::{LapceIdentityWrapper, LapcePadding, LapceScrollNew},
    split::LapceSplitNew,
    svg::{file_svg_new, get_svg},
    tab::LapceIcon,
};

pub struct LapceUI {}

#[derive(Copy, Clone)]
pub struct EditorCount(Option<usize>);

#[derive(Copy, Clone)]
pub enum EditorOperator {
    Delete(EditorCount),
    Yank(EditorCount),
}

#[derive(Clone)]
pub struct EditorUIState {
    pub buffer_id: BufferId,
    pub cursor: (usize, usize),
    pub mode: Mode,
    pub visual_mode: VisualMode,
    pub selection: Selection,
    pub selection_start_line: usize,
    pub selection_end_line: usize,
}

#[derive(Clone)]
pub struct EditorState {
    pub editor_id: WidgetId,
    pub view_id: WidgetId,
    pub split_id: WidgetId,
    pub tab_id: WidgetId,
    pub buffer_id: Option<BufferId>,
    pub char_width: f64,
    pub width: f64,
    pub height: f64,
    pub selection: Selection,
    pub scroll_offset: Vec2,
    pub scroll_size: Size,
    pub view_size: Size,
    pub gutter_width: f64,
    pub header_height: f64,
    pub locations: Vec<EditorLocation>,
    pub current_location: usize,
    pub saved_buffer_id: BufferId,
    pub saved_selection: Selection,
    pub saved_scroll_offset: Vec2,

    #[allow(dead_code)]
    last_movement: Movement,
}

// pub enum LapceEditorContainerKind {
//     Container(WidgetPod<LapceEditorViewData, LapceEditorContainer>),
//     DiffSplit(LapceSplitNew),
// }

pub struct EditorDiffSplit {
    left: WidgetPod<LapceTabData, LapceEditorContainer>,
    right: WidgetPod<LapceTabData, LapceEditorContainer>,
}

impl Widget<LapceTabData> for EditorDiffSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.left.event(ctx, event, data, env);
        self.right.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.lifecycle(ctx, event, data, env);
        self.right.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.left.update(ctx, data, env);
        self.right.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.left.layout(ctx, bc, data, env);
        self.right.layout(ctx, bc, data, env);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.left.paint(ctx, data, env);
        self.right.paint(ctx, data, env);
    }
}

pub struct LapceEditorTabHeaderContent {
    pub widget_id: WidgetId,
    rects: Vec<TabRect>,
    mouse_pos: Point,
}

impl LapceEditorTabHeaderContent {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            rects: Vec::new(),
            mouse_pos: Point::ZERO,
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for tab_rect in self.rects.iter() {
            if tab_rect.close_rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        for (i, tab_rect) in self.rects.iter().enumerate() {
            if tab_rect.rect.contains(mouse_event.pos) {
                let editor_tab = data
                    .main_split
                    .editor_tabs
                    .get_mut(&self.widget_id)
                    .unwrap();
                let editor_tab = Arc::make_mut(editor_tab);
                if tab_rect.close_rect.contains(mouse_event.pos) {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EditorTabRemove(i, true, true),
                        Target::Widget(self.widget_id),
                    ));
                    return;
                }
                if editor_tab.active != i {
                    editor_tab.active = i;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(editor_tab.children[i].widget_id()),
                    ));
                }

                let offset =
                    mouse_event.pos.to_vec2() - tab_rect.rect.origin().to_vec2();
                *Arc::make_mut(&mut data.drag) = Some((
                    offset,
                    DragContent::EditorTab(
                        editor_tab.widget_id,
                        i,
                        editor_tab.children[i].clone(),
                        tab_rect.clone(),
                    ),
                ));
                return;
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorTabHeaderContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, data, mouse_event);
            }
            Event::MouseUp(mouse_event) => {
                if let Some((_, drag_content)) = data.drag.clone().as_ref() {
                    match drag_content {
                        DragContent::EditorTab(from_id, from_index, child, _) => {
                            let mut mouse_index = self.rects.len();
                            for (i, tab_rect) in self.rects.iter().enumerate() {
                                if tab_rect.rect.contains(mouse_event.pos) {
                                    if mouse_event.pos.x
                                        <= tab_rect.rect.x0
                                            + tab_rect.rect.size().width / 2.0
                                    {
                                        mouse_index = i;
                                    } else {
                                        mouse_index = i + 1;
                                    }
                                    break;
                                }
                            }
                            let editor_tab = data
                                .main_split
                                .editor_tabs
                                .get(&self.widget_id)
                                .unwrap()
                                .clone();
                            if &editor_tab.widget_id == from_id {
                                let new_index = if mouse_index > *from_index {
                                    Some(mouse_index - 1)
                                } else if mouse_index < *from_index {
                                    Some(mouse_index)
                                } else {
                                    None
                                };
                                if let Some(new_index) = new_index {
                                    if new_index != *from_index {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::EditorTabSwap(
                                                *from_index,
                                                new_index,
                                            ),
                                            Target::Widget(editor_tab.widget_id),
                                        ));
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::Focus,
                                            Target::Widget(child.widget_id()),
                                        ));
                                    }
                                }
                            } else {
                                child.set_editor_tab(data, editor_tab.widget_id);
                                let editor_tab = data
                                    .main_split
                                    .editor_tabs
                                    .get_mut(&self.widget_id)
                                    .unwrap();
                                let editor_tab = Arc::make_mut(editor_tab);
                                editor_tab
                                    .children
                                    .insert(mouse_index, child.clone());
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::EditorTabAdd(
                                        mouse_index,
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
                    };
                }
                if data.drag.is_some() {
                    *Arc::make_mut(&mut data.drag) = None;
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
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
        _env: &Env,
    ) -> Size {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let _child_min_width = 200.0;
        let height = bc.max().height;

        self.rects.clear();
        let mut x = 0.0;
        for (_i, child) in editor_tab.children.iter().enumerate() {
            let mut text = "".to_string();
            let mut svg = get_svg("default_file.svg").unwrap();
            match child {
                EditorTabChild::Editor(view_id, _) => {
                    let editor = data.main_split.editors.get(view_id).unwrap();
                    if let BufferContent::File(path) = &editor.content {
                        svg = file_svg_new(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                            }
                        }
                    }
                }
            }
            let text_layout = ctx
                .text()
                .new_text_layout(text)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let width = (text_size.width + height * 2.0).max(100.0);
            let close_size = 24.0;
            let inflate = (height - close_size) / 2.0;
            let tab_rect = TabRect {
                svg,
                rect: Size::new(width, height)
                    .to_rect()
                    .with_origin(Point::new(x, 0.0)),
                close_rect: Size::new(height, height)
                    .to_rect()
                    .with_origin(Point::new(x + width - height, 0.0))
                    .inflate(-inflate, -inflate),
                text_layout,
            };
            x += width;
            self.rects.push(tab_rect);
        }

        Size::new(bc.max().width.max(x), height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let size = ctx.size();

        let mut mouse_index = self.rects.len() - 1;
        for (i, tab_rect) in self.rects.iter().enumerate() {
            if i != editor_tab.active {
                tab_rect.paint(ctx, data, self.widget_id, i, size, self.mouse_pos);
            }
            if tab_rect.rect.contains(self.mouse_pos) {
                mouse_index = i;
            }
        }

        self.rects.get(editor_tab.active).unwrap().paint(
            ctx,
            data,
            self.widget_id,
            editor_tab.active,
            size,
            self.mouse_pos,
        );

        if ctx.is_hot() && data.drag.is_some() {
            let tab_rect = &self.rects[mouse_index];
            let x = if self.mouse_pos.x
                <= tab_rect.rect.x0 + tab_rect.rect.size().width / 2.0
            {
                if mouse_index == 0 {
                    tab_rect.rect.x0 + 2.0
                } else {
                    tab_rect.rect.x0
                }
            } else {
                tab_rect.rect.x1
            };
            ctx.stroke(
                Line::new(
                    Point::new(x, tab_rect.rect.y0),
                    Point::new(x, tab_rect.rect.y1),
                ),
                data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                4.0,
            );
        }
    }
}

pub struct LapceEditorTabHeader {
    pub widget_id: WidgetId,
    pub content: WidgetPod<
        LapceTabData,
        LapceScrollNew<LapceTabData, LapceEditorTabHeaderContent>,
    >,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
    is_hot: bool,
}

impl LapceEditorTabHeader {
    pub fn new(widget_id: WidgetId) -> Self {
        let content =
            LapceScrollNew::new(LapceEditorTabHeaderContent::new(widget_id))
                .horizontal();
        Self {
            widget_id,
            content: WidgetPod::new(content),
            icons: Vec::new(),
            mouse_pos: Point::ZERO,
            is_hot: false,
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorTabHeader {
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
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::EnsureEditorTabActiveVisble = command {
                    let editor_tab =
                        data.main_split.editor_tabs.get(&self.widget_id).unwrap();
                    let active = editor_tab.active;
                    if active < self.content.widget().child().rects.len() {
                        let rect = self.content.widget().child().rects[active].rect;
                        if self.content.widget_mut().scroll_to_visible(rect, env) {
                            self.content
                                .widget_mut()
                                .scroll_component
                                .reset_scrollbar_fade(|d| ctx.request_timer(d), env);
                        }
                    }
                }
            }
            _ => (),
        }
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::HotChanged(is_hot) = event {
            self.is_hot = *is_hot;
            ctx.request_layout();
        }
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.icons.clear();

        let size = if data.config.editor.show_tab {
            let height = 30.0;
            let size = Size::new(bc.max().width, height);

            let editor_tab =
                data.main_split.editor_tabs.get(&self.widget_id).unwrap();
            if self.is_hot || *editor_tab.content_is_hot.borrow() {
                let icon_size = 24.0;
                let gap = (height - icon_size) / 2.0;
                let x =
                    size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
                let icon = LapceIcon {
                    icon: "close.svg".to_string(),
                    rect: Size::new(icon_size, icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitClose,
                        Target::Widget(self.widget_id),
                    ),
                };
                self.icons.push(icon);

                let x =
                    size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
                let icon = LapceIcon {
                    icon: "split-horizontal.svg".to_string(),
                    rect: Size::new(icon_size, icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceCommand::SplitVertical.to_string(),
                            data: None,
                            palette_desc: None,
                            target: CommandTarget::Focus,
                        },
                        Target::Widget(self.widget_id),
                    ),
                };
                self.icons.push(icon);
            }

            size
        } else {
            Size::new(bc.max().width, 0.0)
        };
        self.content.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                size.width - self.icons.len() as f64 * size.height,
                size.height,
            )),
            data,
            env,
        );
        self.content.set_origin(ctx, data, env, Point::ZERO);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        ctx.stroke(
            Line::new(
                Point::new(0.0, size.height - 0.5),
                Point::new(size.width, size.height - 0.5),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        self.content.paint(ctx, data, env);

        let svg_padding = 4.0;
        for icon in self.icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    &icon.rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            if let Some(svg) = get_svg(&icon.icon) {
                ctx.draw_svg(
                    &svg,
                    icon.rect.inflate(-svg_padding, -svg_padding),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                );
            }
        }
        if !self.icons.is_empty() {
            let x = size.width - self.icons.len() as f64 * size.height - 0.5;
            ctx.stroke(
                Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }
}

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

pub fn editor_tab_child_widget(
    child: &EditorTabChild,
) -> Box<dyn Widget<LapceTabData>> {
    match child {
        EditorTabChild::Editor(view_id, find_view_id) => {
            LapceEditorView::new(*view_id, *find_view_id).boxed()
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

pub struct LapceEditorView {
    pub view_id: WidgetId,
    pub header: WidgetPod<LapceTabData, LapceEditorHeader>,
    pub editor: WidgetPod<LapceTabData, LapceEditorContainer>,
    pub find: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceEditorView {
    pub fn new(
        view_id: WidgetId,
        find_view_id: Option<WidgetId>,
    ) -> LapceEditorView {
        let header = LapceEditorHeader::new(view_id);
        let editor = LapceEditorContainer::new(view_id);
        let find =
            find_view_id.map(|id| WidgetPod::new(FindBox::new(id, view_id)).boxed());
        Self {
            view_id,
            header: WidgetPod::new(header),
            editor: WidgetPod::new(editor),
            find,
        }
    }

    pub fn hide_header(mut self) -> Self {
        self.header.widget_mut().display = false;
        self
    }

    pub fn hide_gutter(mut self) -> Self {
        self.editor.widget_mut().display_gutter = false;
        self
    }

    pub fn set_placeholder(mut self, placehoder: String) -> Self {
        self.editor
            .widget_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .child_mut()
            .placeholder = Some(placehoder);
        self
    }

    pub fn request_focus(
        &self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        left_click: bool,
    ) {
        if left_click {
            ctx.request_focus();
        }
        data.focus = self.view_id;
        let editor = data.main_split.editors.get(&self.view_id).unwrap().clone();
        if let Some(editor_tab_id) = editor.tab_id {
            let editor_tab =
                data.main_split.editor_tabs.get_mut(&editor_tab_id).unwrap();
            let editor_tab = Arc::make_mut(editor_tab);
            if let Some(index) = editor_tab
                .children
                .iter()
                .position(|child| child.widget_id() == self.view_id)
            {
                editor_tab.active = index;
            }
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::EnsureEditorTabActiveVisble,
                Target::Widget(editor_tab_id),
            ));
        }
        match &editor.content {
            BufferContent::File(_) => {
                data.focus_area = FocusArea::Editor;
                data.main_split.active = Arc::new(Some(self.view_id));
                data.main_split.active_tab = Arc::new(editor.tab_id);
            }
            BufferContent::Local(kind) => match kind {
                LocalBufferKind::Keymap => {}
                LocalBufferKind::Settings => {}
                LocalBufferKind::FilePicker => {
                    data.focus_area = FocusArea::FilePicker;
                }
                LocalBufferKind::Search => {
                    data.focus_area = FocusArea::Panel(PanelKind::Search);
                }
                LocalBufferKind::SourceControl => {
                    data.focus_area = FocusArea::Panel(PanelKind::SourceControl);
                    Arc::make_mut(&mut data.source_control).active = self.view_id;
                }
                LocalBufferKind::Empty => {
                    data.focus_area = FocusArea::Editor;
                    data.main_split.active = Arc::new(Some(self.view_id));
                    data.main_split.active_tab = Arc::new(editor.tab_id);
                }
            },
            BufferContent::Value(_) => {}
        }
    }

    pub fn handle_lapce_ui_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceUICommand,
        data: &mut LapceEditorBufferData,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        env: &Env,
    ) {
        match cmd {
            LapceUICommand::EnsureCursorVisible(position) => {
                self.ensure_cursor_visible(
                    ctx,
                    data,
                    panels,
                    position.as_ref(),
                    env,
                );
            }
            LapceUICommand::EnsureCursorCenter => {
                self.ensure_cursor_center(ctx, data, panels, env);
            }
            LapceUICommand::EnsureRectVisible(rect) => {
                self.ensure_rect_visible(ctx, data, *rect, env);
            }
            LapceUICommand::ResolveCompletion(buffer_id, rev, offset, item) => {
                if data.buffer.id != *buffer_id {
                    return;
                }
                if data.buffer.rev != *rev {
                    return;
                }
                if data.editor.cursor.offset() != *offset {
                    return;
                }
                let offset = data.editor.cursor.offset();
                let line = data.buffer.line_of_offset(offset);
                let _ = data.apply_completion_item(ctx, item);
                let new_offset = data.editor.cursor.offset();
                let new_line = data.buffer.line_of_offset(new_offset);
                if line != new_line {
                    self.editor
                        .widget_mut()
                        .editor
                        .widget_mut()
                        .inner_mut()
                        .scroll_by(Vec2::new(
                            0.0,
                            (new_line as f64 - line as f64)
                                * data.config.editor.line_height as f64,
                        ));
                }
            }
            LapceUICommand::Scroll((x, y)) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_by(Vec2::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            LapceUICommand::ForceScrollTo(x, y) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .force_scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            LapceUICommand::ScrollTo((x, y)) => {
                self.editor
                    .widget_mut()
                    .editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.editor.widget().scroll_id),
                ));
            }
            _ => (),
        }
    }

    fn ensure_rect_visible(
        &mut self,
        ctx: &mut EventCtx,
        _data: &LapceEditorBufferData,
        rect: Rect,
        env: &Env,
    ) {
        if self
            .editor
            .widget_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.editor.widget().scroll_id),
            ));
        }
    }

    pub fn ensure_cursor_center(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        env: &Env,
    ) {
        let center = data.cursor_region(ctx.text(), &data.config).center();

        let rect = Rect::ZERO.with_origin(center).inflate(
            (data.editor.size.borrow().width / 2.0).ceil(),
            (data.editor.size.borrow().height / 2.0).ceil(),
        );

        let editor_size = *data.editor.size.borrow();
        let size = data.get_size(ctx.text(), editor_size, panels, env);
        let scroll = self.editor.widget_mut().editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.editor.widget().scroll_id),
            ));
        }
    }

    fn ensure_cursor_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        position: Option<&EnsureVisiblePosition>,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let editor_size = *data.editor.size.borrow();
        let size = data.get_size(ctx.text(), editor_size, panels, env);

        let rect = data.cursor_region(ctx.text(), &data.config);
        let scroll_id = self.editor.widget().scroll_id;
        let scroll = self.editor.widget_mut().editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        let old_scroll_offset = scroll.offset();
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(scroll_id),
            ));
            if let Some(position) = position {
                match position {
                    EnsureVisiblePosition::CenterOfWindow => {
                        self.ensure_cursor_center(ctx, data, panels, env);
                    }
                }
            } else {
                let scroll_offset = scroll.offset();
                if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
                {
                    self.ensure_cursor_center(ctx, data, panels, env);
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorView {
    fn id(&self) -> Option<WidgetId> {
        Some(self.view_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if let Some(find) = self.find.as_mut() {
            match event {
                Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {}
                Event::Command(cmd) if cmd.is(LAPCE_NEW_COMMAND) => {}
                _ => {
                    find.event(ctx, event, data, env);
                }
            }
        }

        if ctx.is_handled() {
            return;
        }

        let editor = data.main_split.editors.get(&self.view_id).unwrap().clone();
        match event {
            Event::MouseDown(mouse_event) => match mouse_event.button {
                druid::MouseButton::Left => {
                    self.request_focus(ctx, data, true);
                }
                druid::MouseButton::Right => {
                    self.request_focus(ctx, data, false);
                }
                _ => (),
            },
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx, data, true);
                }
            }
            _ => (),
        }

        let mut editor_data = data.editor_view_content(self.view_id);
        let buffer = editor_data.buffer.clone();

        match event {
            Event::KeyDown(key_event) => {
                ctx.set_handled();
                let mut keypress = data.keypress.clone();
                if Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    &mut editor_data,
                    env,
                ) {
                    self.ensure_cursor_visible(
                        ctx,
                        &editor_data,
                        &data.panels,
                        None,
                        env,
                    );
                }
                editor_data.sync_buffer_position(
                    self.editor.widget().editor.widget().inner().offset(),
                );
                editor_data.get_code_actions(ctx);

                data.keypress = keypress.clone();
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_NEW_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_NEW_COMMAND);
                if let Ok(command) = LapceCommand::from_str(&command.cmd) {
                    editor_data.run_command(
                        ctx,
                        &command,
                        None,
                        Modifiers::empty(),
                        env,
                    );
                    self.ensure_cursor_visible(
                        ctx,
                        &editor_data,
                        &data.panels,
                        None,
                        env,
                    );
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let cmd = cmd.get_unchecked(LAPCE_UI_COMMAND);
                self.handle_lapce_ui_command(
                    ctx,
                    cmd,
                    &mut editor_data,
                    &data.panels,
                    env,
                );
            }
            _ => (),
        }
        data.update_from_editor_buffer_data(editor_data, &editor, &buffer);

        self.header.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);

        let offset = self.editor.widget().editor.widget().inner().offset();
        if editor.scroll_offset != offset {
            Arc::make_mut(data.main_split.editors.get_mut(&self.view_id).unwrap())
                .scroll_offset = offset;
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let Some(find) = self.find.as_mut() {
            find.lifecycle(ctx, event, data, env);
        }

        match event {
            LifeCycle::WidgetAdded => {
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ForceScrollTo(
                        editor.scroll_offset.x,
                        editor.scroll_offset.y,
                    ),
                    Target::Widget(editor.view_id),
                ));
            }
            LifeCycle::HotChanged(is_hot) => {
                self.header.widget_mut().view_is_hot = *is_hot;
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                if let Some(editor_tab_id) = editor.tab_id.as_ref() {
                    let editor_tab =
                        data.main_split.editor_tabs.get(editor_tab_id).unwrap();
                    *editor_tab.content_is_hot.borrow_mut() = *is_hot;
                }
                ctx.request_layout();
            }
            _ => (),
        }
        self.header.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let Some(find) = self.find.as_mut() {
            find.update(ctx, data, env);
        }

        if old_data.config.lapce.modal != data.config.lapce.modal {
            if !data.config.lapce.modal {
                ctx.submit_command(Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::InsertMode.to_string(),
                        data: None,
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(self.view_id),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_NEW_COMMAND,
                    LapceCommandNew {
                        cmd: LapceCommand::NormalMode.to_string(),
                        data: None,
                        palette_desc: None,
                        target: CommandTarget::Focus,
                    },
                    Target::Widget(self.view_id),
                ));
            }
        }
        let old_editor_data = old_data.editor_view_content(self.view_id);
        let editor_data = data.editor_view_content(self.view_id);

        if let Some(syntax) = editor_data.buffer.syntax.as_ref() {
            if syntax.line_height != data.config.editor.line_height
                || syntax.lens_height != data.config.editor.code_lens_font_size
            {
                if let BufferContent::File(path) = &editor_data.buffer.content {
                    let tab_id = data.id;
                    let event_sink = ctx.get_external_handle();
                    let mut syntax = syntax.clone();
                    let line_height = data.config.editor.line_height;
                    let lens_height = data.config.editor.code_lens_font_size;
                    let rev = editor_data.buffer.rev;
                    let path = path.clone();
                    rayon::spawn(move || {
                        syntax.update_lens_height(line_height, lens_height);
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSyntax { path, rev, syntax },
                            Target::Widget(tab_id),
                        );
                    });
                }
            }
        }

        if editor_data.editor.content != old_editor_data.editor.content {
            ctx.request_layout();
        }
        if editor_data.editor.compare != old_editor_data.editor.compare {
            ctx.request_layout();
        }
        if editor_data.editor.code_lens != old_editor_data.editor.code_lens {
            ctx.request_layout();
        }
        if editor_data.editor.compare.is_some() {
            if !editor_data
                .buffer
                .histories
                .ptr_eq(&old_editor_data.buffer.histories)
            {
                ctx.request_layout();
            }
            if !editor_data
                .buffer
                .history_changes
                .ptr_eq(&old_editor_data.buffer.history_changes)
            {
                ctx.request_layout();
            }
        }
        if editor_data.buffer.dirty != old_editor_data.buffer.dirty {
            ctx.request_paint();
        }
        if editor_data.editor.cursor != old_editor_data.editor.cursor {
            ctx.request_paint();
        }

        let buffer = &editor_data.buffer;
        let old_buffer = &old_editor_data.buffer;
        if buffer.max_len != old_buffer.max_len
            || buffer.num_lines != old_buffer.num_lines
        {
            ctx.request_layout();
        }

        match (buffer.styles(), old_buffer.styles()) {
            (None, None) => {}
            (None, Some(_)) | (Some(_), None) => {
                ctx.request_paint();
            }
            (Some(new), Some(old)) => {
                if !new.same(old) {
                    ctx.request_paint();
                }
            }
        }

        if buffer.rev != old_buffer.rev {
            ctx.request_paint();
        }

        if old_editor_data.current_code_actions().is_some()
            != editor_data.current_code_actions().is_some()
        {
            ctx.request_paint();
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
        let editor_size = if self_size.height > header_size.height {
            let editor_size =
                Size::new(self_size.width, self_size.height - header_size.height);
            let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
            let size = self.editor.layout(ctx, &editor_bc, data, env);
            self.editor.set_origin(
                ctx,
                data,
                env,
                Point::new(0.0, header_size.height),
            );
            size
        } else {
            Size::ZERO
        };
        let size =
            Size::new(editor_size.width, editor_size.height + header_size.height);

        if let Some(find) = self.find.as_mut() {
            let find_size = find.layout(ctx, bc, data, env);
            find.set_origin(
                ctx,
                data,
                env,
                Point::new(size.width - find_size.width - 10.0, header_size.height),
            );
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let editor = data.main_split.editors.get(&self.view_id).unwrap();
        if editor.content.is_special() {
            let size = ctx.size();
            ctx.fill(
                size.to_rect().inflate(5.0, 5.0),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        if editor.content.is_input() {
            let size = ctx.size();
            ctx.stroke(
                size.to_rect().inflate(4.5, 4.5),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }

        self.editor.paint(ctx, data, env);
        self.header.paint(ctx, data, env);
        if let Some(find) = self.find.as_mut() {
            find.paint(ctx, data, env);
        }
    }
}

pub struct LapceEditorContainer {
    pub view_id: WidgetId,
    pub scroll_id: WidgetId,
    pub display_gutter: bool,
    pub gutter:
        WidgetPod<LapceTabData, LapcePadding<LapceTabData, LapceEditorGutter>>,
    pub editor: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScrollNew<LapceTabData, LapceEditor>>,
    >,
}

impl LapceEditorContainer {
    pub fn new(view_id: WidgetId) -> Self {
        let scroll_id = WidgetId::next();
        let gutter = LapceEditorGutter::new(view_id);
        let gutter = LapcePadding::new((10.0, 0.0, 0.0, 0.0), gutter);
        let editor = LapceEditor::new(view_id);
        let editor = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(editor).vertical().horizontal(),
            scroll_id,
        );
        Self {
            view_id,
            scroll_id,
            display_gutter: true,
            gutter: WidgetPod::new(gutter),
            editor: WidgetPod::new(editor),
        }
    }
}

impl Widget<LapceTabData> for LapceEditorContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.gutter.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);
        match event {
            Event::MouseDown(_) | Event::MouseUp(_) => {
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                let mut editor_data = data.editor_view_content(self.view_id);
                let buffer = editor_data.buffer.clone();
                editor_data
                    .sync_buffer_position(self.editor.widget().inner().offset());
                data.update_from_editor_buffer_data(editor_data, &editor, &buffer);
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
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.gutter.update(ctx, data, env);
        self.editor.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        self.gutter.set_origin(ctx, data, env, Point::ZERO);
        let editor_size = Size::new(
            self_size.width
                - if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
            self_size.height,
        );
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        let editor_size = self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_origin(
            ctx,
            data,
            env,
            Point::new(
                if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
                0.0,
            ),
        );
        *data
            .main_split
            .editors
            .get(&self.view_id)
            .unwrap()
            .size
            .borrow_mut() = editor_size;
        Size::new(
            if self.display_gutter {
                gutter_size.width
            } else {
                0.0
            } + editor_size.width,
            editor_size.height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.editor.paint(ctx, data, env);
        if self.display_gutter {
            self.gutter.paint(ctx, data, env);
        }
    }
}

pub struct LapceEditorHeader {
    view_id: WidgetId,
    pub display: bool,
    cross_rect: Rect,
    mouse_pos: Point,
    view_is_hot: bool,
    height: f64,
    icon_size: f64,
    icons: Vec<LapceIcon>,
    svg_padding: f64,
}

impl LapceEditorHeader {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            display: true,
            view_id,
            cross_rect: Rect::ZERO,
            mouse_pos: Point::ZERO,
            view_is_hot: false,
            height: 30.0,
            icon_size: 24.0,
            svg_padding: 4.0,
            icons: Vec::new(),
        }
    }

    pub fn get_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let _data = data.editor_view_content(self.view_id);
        let gap = (self.height - self.icon_size) / 2.0;

        let mut icons = Vec::new();
        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "close.svg".to_string(),
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_NEW_COMMAND,
                LapceCommandNew {
                    cmd: LapceCommand::SplitClose.to_string(),
                    data: None,
                    palette_desc: None,
                    target: CommandTarget::Focus,
                },
                Target::Widget(self.view_id),
            ),
        };
        icons.push(icon);

        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "split-horizontal.svg".to_string(),
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_NEW_COMMAND,
                LapceCommandNew {
                    cmd: LapceCommand::SplitVertical.to_string(),
                    data: None,
                    palette_desc: None,
                    target: CommandTarget::Focus,
                },
                Target::Widget(self.view_id),
            ),
        };
        icons.push(icon);

        icons
    }

    pub fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    pub fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    pub fn paint_buffer(&self, ctx: &mut PaintCtx, data: &LapceEditorBufferData) {
        let shadow_width = 5.0;
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        let mut clip_rect = ctx.size().to_rect();
        if self.view_is_hot {
            if let Some(icon) = self.icons.iter().rev().next().as_ref() {
                clip_rect.x1 = icon.rect.x0;
            }
        }
        if let BufferContent::File(path) = &data.buffer.content {
            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                let mut path = path.clone();
                let svg = file_svg_new(&path);

                let width = 13.0;
                let height = 13.0;
                let rect = Size::new(width, height).to_rect().with_origin(
                    Point::new((30.0 - width) / 2.0, (30.0 - height) / 2.0),
                );
                ctx.draw_svg(&svg, rect, None);

                let mut file_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if data.buffer.dirty {
                    file_name = "*".to_string() + &file_name;
                }
                if let Some(_compare) = data.editor.compare.as_ref() {
                    file_name += " (Working tree)";
                }
                let text_layout = ctx
                    .text()
                    .new_text_layout(file_name)
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(30.0, 7.0));

                if let Some(workspace_path) = data.workspace.path.as_ref() {
                    path = path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&path)
                        .to_path_buf();
                }
                let folder = path
                    .parent()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if !folder.is_empty() {
                    let x = text_layout.size().width;

                    let text_layout = ctx
                        .text()
                        .new_text_layout(folder)
                        .font(FontFamily::SYSTEM_UI, 13.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    ctx.draw_text(&text_layout, Point::new(30.0 + x + 5.0, 7.0));
                }
            });
        }

        if self.view_is_hot {
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        &icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                if let Some(svg) = get_svg(&icon.icon) {
                    ctx.draw_svg(
                        &svg,
                        icon.rect.inflate(-self.svg_padding, -self.svg_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
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
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        // ctx.set_paint_insets((0.0, 0.0, 0.0, 10.0));
        if self.display
            && (!data.config.editor.show_tab
                || self.view_id == data.palette.preview_editor)
        {
            let size = Size::new(bc.max().width, self.height);
            self.icons = self.get_icons(size, data);
            let cross_size = 20.0;
            let padding = (size.height - cross_size) / 2.0;
            let origin = Point::new(size.width - padding - cross_size, padding);
            self.cross_rect = Size::new(cross_size, cross_size)
                .to_rect()
                .with_origin(origin);
            size
        } else {
            Size::new(bc.max().width, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if !self.display {
            return;
        }
        self.paint_buffer(ctx, &data.editor_view_content(self.view_id));
    }
}

pub struct LapceEditorGutter {
    view_id: WidgetId,
    width: f64,
}

impl LapceEditorGutter {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            width: 0.0,
        }
    }
}

impl Widget<LapceTabData> for LapceEditorGutter {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        // let old_last_line = old_data.buffer.last_line() + 1;
        // let last_line = data.buffer.last_line() + 1;
        // if old_last_line.to_string().len() != last_line.to_string().len() {
        //     ctx.request_layout();
        //     return;
        // }

        // if (*old_data.main_split.active == self.view_id
        //     && *data.main_split.active != self.view_id)
        //     || (*old_data.main_split.active != self.view_id
        //         && *data.main_split.active == self.view_id)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.editor.cursor.current_line(&old_data.buffer)
        //     != data.editor.cursor.current_line(&data.buffer)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.current_code_actions().is_some()
        //     != data.current_code_actions().is_some()
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let data = data.editor_view_content(self.view_id);
        let last_line = data.buffer.last_line() + 1;
        let char_width = data.config.editor_text_width(ctx.text(), "W");
        self.width = (char_width * last_line.to_string().len() as f64).ceil();
        let mut width = self.width + 16.0 + char_width * 2.0;
        if data.editor.compare.is_some() {
            width += self.width + char_width * 2.0;
        }
        Size::new(width, bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let data = data.editor_view_content(self.view_id);
        data.paint_gutter(ctx, self.width);
    }
}

#[derive(Clone, Copy)]
enum ClickKind {
    Single,
    Double,
    Triple,
    Quadruple,
}

pub struct LapceEditor {
    view_id: WidgetId,
    placeholder: Option<String>,

    #[allow(dead_code)]
    commands: Vec<(LapceCommandNew, PietTextLayout, Rect, PietTextLayout)>,

    last_left_click: Option<(Instant, ClickKind, Point)>,
    mouse_pos: Point,
}

impl LapceEditor {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            placeholder: None,
            commands: vec![],
            last_left_click: None,
            mouse_pos: Point::ZERO,
        }
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        editor_data: &mut LapceEditorBufferData,
        config: &Config,
    ) {
        ctx.set_handled();
        match mouse_event.button {
            MouseButton::Left => {
                self.left_click(ctx, mouse_event, editor_data, config);
            }
            MouseButton::Right => {
                self.right_click(ctx, editor_data, mouse_event, config);
            }
            MouseButton::Middle => {}
            _ => (),
        }
    }

    fn left_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        editor_data: &mut LapceEditorBufferData,
        config: &Config,
    ) {
        ctx.set_active(true);
        let mut click_kind = ClickKind::Single;
        if let Some((instant, kind, pos)) = self.last_left_click.as_ref() {
            if pos == &mouse_event.pos && instant.elapsed().as_millis() < 500 {
                click_kind = match kind {
                    ClickKind::Single => ClickKind::Double,
                    ClickKind::Double => ClickKind::Triple,
                    ClickKind::Triple => ClickKind::Quadruple,
                    ClickKind::Quadruple => ClickKind::Quadruple,
                };
            }
        }
        self.last_left_click = Some((Instant::now(), click_kind, mouse_event.pos));
        match click_kind {
            ClickKind::Single => {
                editor_data.single_click(ctx, mouse_event, config);
            }
            ClickKind::Double => {
                editor_data.double_click(ctx, mouse_event, config);
            }
            ClickKind::Triple => {
                editor_data.triple_click(ctx, mouse_event, config);
            }
            ClickKind::Quadruple => {}
        }
    }

    fn right_click(
        &mut self,
        ctx: &mut EventCtx,
        editor_data: &mut LapceEditorBufferData,
        mouse_event: &MouseEvent,
        config: &Config,
    ) {
        editor_data.single_click(ctx, mouse_event, config);
        let menu_items = vec![
            MenuItem {
                text: LapceCommand::GotoDefinition
                    .get_message()
                    .unwrap()
                    .to_string(),
                command: LapceCommandNew {
                    cmd: LapceCommand::GotoDefinition.to_string(),
                    palette_desc: None,
                    data: None,
                    target: CommandTarget::Focus,
                },
            },
            MenuItem {
                text: "Command Palette".to_string(),
                command: LapceCommandNew {
                    cmd: LapceWorkbenchCommand::PaletteCommand.to_string(),
                    palette_desc: None,
                    data: None,
                    target: CommandTarget::Workbench,
                },
            },
        ];
        let point = mouse_event.pos + editor_data.editor.window_origin.to_vec2();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ShowMenu(point, Arc::new(menu_items)),
            Target::Auto,
        ));
    }
}

impl Widget<LapceTabData> for LapceEditor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_cursor(&druid::Cursor::IBeam);
                if mouse_event.pos != self.mouse_pos {
                    self.mouse_pos = mouse_event.pos;
                    if ctx.is_active() {
                        let editor_data = data.editor_view_content(self.view_id);
                        let new_offset = editor_data.offset_of_mouse(
                            ctx.text(),
                            mouse_event.pos,
                            &data.config,
                        );
                        let editor =
                            data.main_split.editors.get_mut(&self.view_id).unwrap();
                        let editor = Arc::make_mut(editor);
                        editor.cursor = editor.cursor.set_offset(
                            new_offset,
                            true,
                            mouse_event.mods.alt(),
                        );
                    }
                }
            }
            Event::MouseUp(_mouse_event) => {
                ctx.set_active(false);
            }
            Event::MouseDown(mouse_event) => {
                let buffer = data.main_split.editor_buffer(self.view_id);
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                let mut editor_data = data.editor_view_content(self.view_id);
                self.mouse_down(ctx, mouse_event, &mut editor_data, &data.config);
                data.update_from_editor_buffer_data(editor_data, &editor, &buffer);
                // match mouse_event.button {
                //     druid::MouseButton::Right => {
                //         let menu_items = vec![
                //             MenuItem {
                //                 text: LapceCommand::GotoDefinition
                //                     .get_message()
                //                     .unwrap()
                //                     .to_string(),
                //                 command: LapceCommandNew {
                //                     cmd: LapceCommand::GotoDefinition.to_string(),
                //                     palette_desc: None,
                //                     data: None,
                //                     target: CommandTarget::Focus,
                //                 },
                //             },
                //             MenuItem {
                //                 text: "Command Palette".to_string(),
                //                 command: LapceCommandNew {
                //                     cmd: LapceWorkbenchCommand::PaletteCommand
                //                         .to_string(),
                //                     palette_desc: None,
                //                     data: None,
                //                     target: CommandTarget::Workbench,
                //                 },
                //             },
                //         ];
                //         let point = mouse_event.pos + editor.window_origin.to_vec2();
                //         ctx.submit_command(Command::new(
                //             LAPCE_UI_COMMAND,
                //             LapceUICommand::ShowMenu(point, Arc::new(menu_items)),
                //             Target::Auto,
                //         ));
                //     }
                //     _ => {}
                // }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::UpdateWindowOrigin = command {
                    let window_origin = ctx.window_origin();
                    let editor =
                        data.main_split.editors.get_mut(&self.view_id).unwrap();
                    if editor.window_origin != window_origin {
                        Arc::make_mut(editor).window_origin = window_origin;
                    }
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
        _env: &Env,
    ) {
        if let LifeCycle::Internal(InternalLifeCycle::ParentWindowOrigin) = event {
            let editor = data.main_split.editors.get(&self.view_id).unwrap();
            if ctx.window_origin() != editor.window_origin {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateWindowOrigin,
                    Target::Widget(editor.view_id),
                ))
            }
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        // let buffer = &data.buffer;
        // let old_buffer = &old_data.buffer;

        // let line_height = data.config.editor.line_height as f64;

        // if data.editor.size != old_data.editor.size {
        //     ctx.request_paint();
        //     return;
        // }

        // if !old_buffer.same(buffer) {
        //     if buffer.max_len != old_buffer.max_len
        //         || buffer.num_lines != old_buffer.num_lines
        //     {
        //         ctx.request_layout();
        //         ctx.request_paint();
        //         return;
        //     }

        //     if !buffer.styles.same(&old_buffer.styles) {
        //         ctx.request_paint();
        //     }

        //     if buffer.rev != old_buffer.rev {
        //         ctx.request_paint();
        //     }
        // }

        // if old_data.editor.cursor != data.editor.cursor {
        //     ctx.request_paint();
        // }

        // if old_data.current_code_actions().is_some()
        //     != data.current_code_actions().is_some()
        // {
        //     ctx.request_paint();
        // }

        // if old_data.on_diagnostic() != data.on_diagnostic() {
        //     ctx.request_paint();
        // }

        // if old_data.diagnostics.len() != data.diagnostics.len() {
        //     ctx.request_paint();
        // }

        // if (*old_data.main_split.active == self.view_id
        //     && *data.main_split.active != self.view_id)
        //     || (*old_data.main_split.active != self.view_id
        //         && *data.main_split.active == self.view_id)
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let editor_data = data.editor_view_content(self.view_id);
        editor_data.get_size(ctx.text(), bc.max(), &data.panels, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let is_focused = data.focus == self.view_id;
        let data = data.editor_view_content(self.view_id);
        data.paint_content(
            ctx,
            is_focused,
            self.placeholder.as_ref(),
            &data.config,
            env,
        );
    }
}

#[derive(Clone)]
pub struct RegisterContent {
    #[allow(dead_code)]
    kind: VisualMode,

    #[allow(dead_code)]
    content: Vec<String>,
}

#[allow(dead_code)]
struct EditorTextLayout {
    layout: TextLayout<String>,
    text: String,
}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

#[allow(dead_code)]
fn get_workspace_edit_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    if let Some(edits) = get_workspace_edit_changes_edits(url, workspace_edit) {
        Some(edits)
    } else {
        get_workspace_edit_document_changes_edits(url, workspace_edit)
    }
}

fn get_workspace_edit_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.changes.as_ref()?;
    changes.get(url).map(|c| c.iter().collect())
}

fn get_workspace_edit_document_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.document_changes.as_ref()?;
    match changes {
        DocumentChanges::Edits(edits) => {
            for edit in edits {
                if &edit.text_document.uri == url {
                    let e = edit
                        .edits
                        .iter()
                        .filter_map(|e| match e {
                            lsp_types::OneOf::Left(edit) => Some(edit),
                            lsp_types::OneOf::Right(_) => None,
                        })
                        .collect();
                    return Some(e);
                }
            }
            None
        }
        DocumentChanges::Operations(_) => None,
    }
}

#[allow(dead_code)]
fn str_is_pair_right(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return !matching_pair_direction(c).unwrap_or(true);
    }
    false
}
