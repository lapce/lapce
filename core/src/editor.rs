use crate::config::LapceTheme;
use crate::data::{EditorType, LapceEditorData, LapceEditorLens, LapceTabData};
use crate::find::Find;
use crate::scroll::LapceIdentityWrapper;
use crate::signature::SignatureState;
use crate::split::LapceSplitNew;
use crate::svg::{file_svg_new, get_svg};
use crate::theme::OldLapceTheme;
use crate::{buffer::get_word_property, state::LapceFocus};
use crate::{buffer::matching_char, data::LapceEditorViewData};
use crate::{buffer::previous_has_unmatched_pair, movement::Cursor};
use crate::{buffer::WordProperty, movement::CursorMode};
use crate::{
    buffer::{matching_pair_direction, BufferNew},
    scroll::LapceScrollNew,
};
use crate::{
    buffer::{next_has_unmatched_pair, BufferState},
    scroll::LapcePadding,
};
use crate::{
    buffer::{BufferId, BufferUIState, InvalLines},
    command::{
        EnsureVisiblePosition, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND,
    },
    completion::ScoredCompletionItem,
    movement::{ColPosition, LinePosition, Movement, SelRegion, Selection},
    scroll::LapceScroll,
    split::SplitMoveDirection,
    state::Mode,
    state::VisualMode,
};
use anyhow::{anyhow, Result};
use bit_vec::BitVec;
use crossbeam_channel::{self, bounded};
use druid::widget::{LensWrap, WidgetWrapper};
use druid::{
    kurbo::Line, piet::PietText, theme, widget::Flex, widget::IdentityWrapper,
    widget::Padding, widget::Scroll, widget::SvgData, Affine, BoxConstraints, Color,
    Command, Data, Env, Event, EventCtx, FontDescriptor, FontFamily, Insets,
    KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2, Widget, WidgetExt,
    WidgetId, WidgetPod, WindowId,
};
use druid::{menu, Application, FileDialogOptions, Menu};
use druid::{
    piet::{
        PietTextLayout, Text, TextAttribute, TextLayout as TextLayoutTrait,
        TextLayoutBuilder,
    },
    FontWeight,
};
use fzyr::has_match;
use lsp_types::CompletionTextEdit;
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DocumentChanges, GotoDefinitionResponse,
    Location, Position, SignatureHelp, TextEdit, Url, WorkspaceEdit,
};
use serde_json::Value;
use std::thread;
use std::{cmp::Ordering, iter::Iterator, path::PathBuf};
use std::{collections::HashMap, sync::Arc};
use std::{str::FromStr, time::Duration};
use unicode_width::UnicodeWidthStr;
use xi_core_lib::selection::InsertDrift;
use xi_rope::{Interval, RopeDelta};

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
    last_movement: Movement,
}

#[derive(Clone, Debug)]
pub struct EditorLocationNew {
    pub path: PathBuf,
    pub position: Position,
    pub scroll_offset: Option<Vec2>,
}

#[derive(Clone, Debug)]
pub struct EditorLocation {
    pub path: String,
    pub offset: usize,
    pub scroll_offset: Option<Vec2>,
}

pub enum LapceEditorContainerKind {
    Container(WidgetPod<LapceEditorViewData, LapceEditorContainer>),
    DiffSplit(LapceSplitNew),
}

pub struct EditorDiffSplit {
    left: WidgetPod<LapceEditorViewData, LapceEditorContainer>,
    right: WidgetPod<LapceEditorViewData, LapceEditorContainer>,
}

impl Widget<LapceEditorViewData> for EditorDiffSplit {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
        self.left.event(ctx, event, data, env);
        self.right.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        self.left.lifecycle(ctx, event, data, env);
        self.right.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceEditorViewData,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        self.left.update(ctx, data, env);
        self.right.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceEditorViewData,
        env: &Env,
    ) -> Size {
        self.left.layout(ctx, bc, data, env);
        self.right.layout(ctx, bc, data, env);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceEditorViewData, env: &Env) {
        self.left.paint(ctx, data, env);
        self.right.paint(ctx, data, env);
    }
}

pub struct LapceEditorView {
    pub view_id: WidgetId,
    pub header: WidgetPod<
        LapceTabData,
        LensWrap<
            LapceTabData,
            LapceEditorViewData,
            LapceEditorLens,
            LapceEditorHeader,
        >,
    >,
    pub editor: WidgetPod<
        LapceTabData,
        LensWrap<
            LapceTabData,
            LapceEditorViewData,
            LapceEditorLens,
            LapceEditorContainer,
        >,
    >,
}

impl LapceEditorView {
    pub fn new(data: &LapceEditorData) -> LapceEditorView {
        let header = LapceEditorHeader::new().lens(LapceEditorLens(data.view_id));
        let editor = LapceEditorContainer::new(
            data.view_id,
            data.container_id,
            data.editor_id,
        )
        .lens(LapceEditorLens(data.view_id));
        Self {
            view_id: data.view_id,
            header: WidgetPod::new(header),
            editor: WidgetPod::new(editor),
        }
    }

    pub fn hide_header(mut self) -> Self {
        self.header.widget_mut().wrapped_mut().display = false;
        self
    }

    pub fn hide_gutter(mut self) -> Self {
        self.editor.widget_mut().wrapped_mut().display_gutter = false;
        self
    }

    pub fn set_placeholder(mut self, placehoder: String) -> Self {
        self.editor
            .widget_mut()
            .wrapped_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .child_mut()
            .placeholder = Some(placehoder);
        self
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
        self.header.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
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
        self.header.update(ctx, data, env);
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
        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);
        let editor_size =
            Size::new(self_size.width, self_size.height - header_size.height);
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        self.editor.layout(ctx, &editor_bc, data, env);
        self.editor
            .set_origin(ctx, data, env, Point::new(0.0, header_size.height));
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rects = ctx.region().rects().to_vec();
        for rect in &rects {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        let start = std::time::SystemTime::now();
        self.editor.paint(ctx, data, env);
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("editor paint took {}", duration);
        self.header.paint(ctx, data, env);
    }
}

pub struct LapceEditorContainer {
    pub view_id: WidgetId,
    pub container_id: WidgetId,
    pub editor_id: WidgetId,
    pub scroll_id: WidgetId,
    pub display_gutter: bool,
    pub gutter: WidgetPod<
        LapceEditorViewData,
        LapcePadding<LapceEditorViewData, LapceEditorGutter>,
    >,
    pub editor: WidgetPod<
        LapceEditorViewData,
        LapceIdentityWrapper<LapceScrollNew<LapceEditorViewData, LapceEditor>>,
    >,
}

impl LapceEditorContainer {
    pub fn new(
        view_id: WidgetId,
        container_id: WidgetId,
        editor_id: WidgetId,
    ) -> Self {
        let scroll_id = WidgetId::next();
        let gutter = LapceEditorGutter::new(view_id, container_id);
        let gutter = LapcePadding::new((10.0, 0.0, 0.0, 0.0), gutter);
        let editor = LapceEditor::new(view_id, container_id, editor_id);
        let editor = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(editor).vertical().horizontal(),
            scroll_id,
        );
        Self {
            view_id,
            container_id,
            editor_id,
            scroll_id,
            display_gutter: true,
            gutter: WidgetPod::new(gutter),
            editor: WidgetPod::new(editor),
        }
    }

    fn set_focus(&self, ctx: &mut EventCtx, data: &mut LapceEditorViewData) {
        if data.editor.editor_type != EditorType::SourceControl {
            data.main_split.active = Arc::new(self.view_id);
        }
        ctx.request_focus();
    }

    pub fn handle_lapce_ui_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceUICommand,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
        match cmd {
            LapceUICommand::Focus => {
                self.set_focus(ctx, data);
                ctx.set_handled();
            }
            LapceUICommand::EnsureCursorVisible(position) => {
                self.ensure_cursor_visible(ctx, data, position.as_ref(), env);
            }
            LapceUICommand::EnsureCursorCenter => {
                self.ensure_cursor_center(ctx, data, env);
            }
            LapceUICommand::EnsureRectVisible(rect) => {
                self.ensure_rect_visible(ctx, data, *rect, env);
            }
            LapceUICommand::UpdateSize => {
                Arc::make_mut(&mut data.editor).size = ctx.size();
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
                data.apply_completion_item(ctx, item);
            }
            LapceUICommand::Scroll((x, y)) => {
                self.editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_by(Vec2::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.scroll_id),
                ));
            }
            LapceUICommand::ForceScrollTo(x, y) => {
                self.editor
                    .widget_mut()
                    .inner_mut()
                    .force_scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.scroll_id),
                ));
            }
            LapceUICommand::ScrollTo((x, y)) => {
                self.editor
                    .widget_mut()
                    .inner_mut()
                    .scroll_to(Point::new(*x, *y));
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ResetFade,
                    Target::Widget(self.scroll_id),
                ));
            }
            LapceUICommand::FocusTab => {
                if *data.main_split.active == self.view_id {
                    ctx.request_focus();
                }
            }
            _ => (),
        }
    }

    pub fn ensure_cursor_center(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let offset = data.editor.cursor.offset();
        let (line, col) = data.buffer.offset_to_line_col(offset);
        let width = data.config.editor_text_width(ctx.text(), "W");
        let cursor_x = col as f64 * width - width;
        let cursor_x = if cursor_x < 0.0 { 0.0 } else { cursor_x };
        let rect = Rect::ZERO
            .with_origin(Point::new(
                cursor_x.floor(),
                line as f64 * line_height + line_height / 2.0,
            ))
            .with_size(Size::new((width * 3.0).ceil(), 0.0))
            .inflate(0.0, (data.editor.size.height / 2.0).ceil());

        let size = Size::new(
            (width * data.buffer.max_len as f64).max(data.editor.size.width),
            line_height * data.buffer.text_layouts.borrow().len() as f64
                + data.editor.size.height
                - line_height,
        );
        let scroll = self.editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    pub fn ensure_rect_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorViewData,
        rect: Rect,
        env: &Env,
    ) {
        if self
            .editor
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }

    pub fn ensure_cursor_visible(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorViewData,
        position: Option<&EnsureVisiblePosition>,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let width = data.config.editor_text_width(ctx.text(), "W");
        let size = Size::new(
            (width * data.buffer.max_len as f64).max(data.editor.size.width),
            line_height * data.buffer.text_layouts.borrow().len() as f64
                + data.editor.size.height
                - line_height,
        );

        let rect = data.cusor_region(&data.config);
        let scroll = self.editor.widget_mut().inner_mut();
        scroll.set_child_size(size);
        let old_scroll_offset = scroll.offset();
        if scroll.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
            if let Some(position) = position {
                match position {
                    EnsureVisiblePosition::CenterOfWindow => {
                        self.ensure_cursor_center(ctx, data, env);
                    }
                }
            } else {
                let scroll_offset = scroll.offset();
                if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
                {
                    self.ensure_cursor_center(ctx, data, env);
                }
            }
        }
    }
}

impl Widget<LapceEditorViewData> for LapceEditorContainer {
    fn id(&self) -> Option<WidgetId> {
        Some(self.container_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
        match event {
            Event::WindowConnected => {
                if *data.main_split.active == self.view_id {
                    ctx.request_focus();
                }
            }
            Event::KeyDown(key_event) => {
                if data.key_down(ctx, key_event, env) {
                    self.ensure_cursor_visible(ctx, data, None, env);
                }
                data.sync_buffer_position(self.editor.widget().inner().offset());
                ctx.set_handled();
                data.get_code_actions(ctx);
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                self.handle_lapce_ui_command(ctx, &command, data, env);
            }
            _ => (),
        }
        self.gutter.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);
        let offset = self.editor.widget().inner().offset();
        if data.editor.scroll_offset != offset {
            Arc::make_mut(&mut data.editor).scroll_offset = offset;
            data.fill_text_layouts(ctx, &data.theme.clone(), env);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateWindowOrigin,
                Target::Widget(self.editor_id),
            ));
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        match event {
            LifeCycle::FocusChanged(_) => {
                ctx.request_paint();
            }
            LifeCycle::Size(size) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSize,
                    Target::Widget(self.container_id),
                ));
            }
            _ => (),
        }
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceEditorViewData,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        if old_data.editor.scroll_offset != data.editor.scroll_offset {
            ctx.request_paint();
        }

        self.gutter.update(ctx, data, env);
        let start = std::time::SystemTime::now();
        self.editor.update(ctx, data, env);
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("editor update took {}", duration);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceEditorViewData,
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
        self.editor.layout(ctx, &editor_bc, data, env);
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
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceEditorViewData, env: &Env) {
        self.editor.widget_mut().inner_mut().child_mut().is_focused =
            ctx.is_focused();
        let rects = ctx.region().rects().to_vec();
        for rect in &rects {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        }
        self.editor.paint(ctx, data, env);
        if self.display_gutter {
            self.gutter.paint(ctx, data, env);
        }
    }
}

pub struct LapceEditorHeader {
    pub display: bool,
}

impl LapceEditorHeader {
    pub fn new() -> Self {
        Self { display: true }
    }
}

impl Widget<LapceEditorViewData> for LapceEditorHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceEditorViewData,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        if data.buffer.path != old_data.buffer.path {
            ctx.request_paint();
        }
        if data.buffer.dirty != old_data.buffer.dirty {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceEditorViewData,
        env: &Env,
    ) -> Size {
        ctx.set_paint_insets((0.0, 0.0, 0.0, 10.0));
        if self.display {
            Size::new(bc.max().width, 30.0)
        } else {
            Size::new(bc.max().width, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceEditorViewData, env: &Env) {
        if !self.display {
            return;
        }
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

        let mut path = data.editor.buffer.clone();
        let svg = file_svg_new(
            &path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        );

        let line_height = data.config.editor.line_height as f64;
        if let Some(svg) = svg.as_ref() {
            let width = 13.0;
            let height = 13.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (30.0 - width) / 2.0,
                (30.0 - height) / 2.0,
            ));
            ctx.draw_svg(&svg, rect, None);
        }

        let mut file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if data.buffer.dirty {
            file_name = "*".to_string() + &file_name;
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

        if let Some(workspace) = data.workspace.as_ref() {
            path = path
                .strip_prefix(&workspace.path)
                .unwrap_or(&path)
                .to_path_buf();
        }
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if folder != "" {
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
    }
}

pub struct LapceEditorGutter {
    view_id: WidgetId,
    container_id: WidgetId,
    width: f64,
}

impl LapceEditorGutter {
    pub fn new(view_id: WidgetId, container_id: WidgetId) -> Self {
        Self {
            view_id,
            container_id,
            width: 0.0,
        }
    }

    fn paint_code_actions_hint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        if let Some(actions) = data.current_code_actions() {
            if actions.len() > 0 {
                let line_height = data.config.editor.line_height as f64;
                let offset = data.editor.cursor.offset();
                let (line, _) = data.buffer.offset_to_line_col(offset);
                let svg = get_svg("lightbulb.svg").unwrap();
                let width = 16.0;
                let height = 16.0;
                let char_width = data.config.editor_text_width(ctx.text(), "W");
                let rect =
                    Size::new(width, height).to_rect().with_origin(Point::new(
                        self.width + char_width + 3.0,
                        (line_height - height) / 2.0 + line_height * line as f64
                            - data.editor.scroll_offset.y,
                    ));
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)),
                );
            }
        }
    }
}

impl Widget<LapceEditorViewData> for LapceEditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceEditorViewData,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let old_last_line = old_data.buffer.last_line() + 1;
        let last_line = data.buffer.last_line() + 1;
        if old_last_line.to_string().len() != last_line.to_string().len() {
            ctx.request_layout();
            return;
        }

        if (*old_data.main_split.active == self.view_id
            && *data.main_split.active != self.view_id)
            || (*old_data.main_split.active != self.view_id
                && *data.main_split.active == self.view_id)
        {
            ctx.request_paint();
        }

        if old_data.editor.cursor.current_line(&old_data.buffer)
            != data.editor.cursor.current_line(&data.buffer)
        {
            ctx.request_paint();
        }

        if old_data.current_code_actions().is_some()
            != data.current_code_actions().is_some()
        {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceEditorViewData,
        env: &Env,
    ) -> Size {
        let last_line = data.buffer.last_line() + 1;
        let width = data.config.editor_text_width(ctx.text(), "W");
        self.width = (width * last_line.to_string().len() as f64).ceil();
        let width = self.width + 16.0 + width * 2.0;
        Size::new(width, bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceEditorViewData, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );
        let line_height = data.config.editor.line_height as f64;
        let scroll_offset = data.editor.scroll_offset;
        let start_line = (scroll_offset.y / line_height).floor() as usize;
        let num_lines = (ctx.size().height / line_height).floor() as usize;
        let last_line = data.buffer.last_line();
        let current_line = data.editor.cursor.current_line(&data.buffer);
        for line in start_line..start_line + num_lines + 1 {
            if line > last_line {
                break;
            }
            let content = if *data.main_split.active != self.view_id {
                line + 1
            } else {
                if line == current_line {
                    line + 1
                } else if line > current_line {
                    line - current_line
                } else {
                    current_line - line
                }
            };
            let width = data.config.editor_text_width(ctx.text(), "W");
            let x = ((last_line + 1).to_string().len() - content.to_string().len())
                as f64
                * width;
            let y = line_height * line as f64 + 5.0 - scroll_offset.y;
            let pos = Point::new(x, y);
            let content = content.to_string();

            let text_layout = ctx
                .text()
                .new_text_layout(content)
                .font(
                    data.config.editor.font_family(),
                    data.config.editor.font_size as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, pos);

            if let Some(line_change) = data.buffer.line_changes.get(&line) {
                let x = self.width + width;
                let y = line as f64 * line_height - scroll_offset.y;
                let origin = Point::new(x, y);
                let size = Size::new(3.0, line_height);
                let rect = Rect::ZERO.with_origin(origin).with_size(size);
                match line_change {
                    'm' => {
                        ctx.fill(rect, &Color::rgba8(1, 132, 188, 180));
                    }
                    '+' => {
                        ctx.fill(rect, &Color::rgba8(80, 161, 79, 180));
                    }
                    '-' => {
                        let size = Size::new(3.0, 10.0);
                        let x = self.width + width;
                        let y = line as f64 * line_height
                            - size.height / 2.0
                            - scroll_offset.y;
                        let origin = Point::new(x, y);
                        let rect = Rect::ZERO.with_origin(origin).with_size(size);
                        ctx.fill(rect, &Color::rgba8(228, 86, 73, 180));
                    }
                    _ => {}
                }
            }
        }

        if *data.main_split.active == self.view_id {
            self.paint_code_actions_hint(ctx, data, env);
        }
    }
}

pub struct LapceEditor {
    editor_id: WidgetId,
    view_id: WidgetId,
    container_id: WidgetId,
    placeholder: Option<String>,
    is_focused: bool,
}

impl LapceEditor {
    pub fn new(
        view_id: WidgetId,
        container_id: WidgetId,
        editor_id: WidgetId,
    ) -> Self {
        Self {
            editor_id,
            view_id,
            container_id,
            placeholder: None,
            is_focused: false,
        }
    }

    fn paint_cursor_line(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceEditorViewData,
        line: usize,
        env: &Env,
    ) {
        let active = self.is_focused;
        if !active && data.buffer.len() == 0 && self.placeholder.is_some() {
            return;
        }
        let line_height = data.config.editor.line_height as f64;
        let size = ctx.size();
        ctx.fill(
            Rect::ZERO
                .with_origin(Point::new(0.0, line as f64 * line_height))
                .with_size(Size::new(size.width, line_height)),
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
        );
    }

    fn paint_diagnostics(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.height + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;

        let width = data.config.editor_text_width(ctx.text(), "W");
        let mut current = None;
        let cursor_offset = data.editor.cursor.offset();
        for diagnostic in data.diagnostics.iter() {
            let start = diagnostic.diagnositc.range.start;
            let end = diagnostic.diagnositc.range.end;
            if (start.line as usize) <= end_line || (end.line as usize) >= start_line
            {
                let start_offset = if let Some(range) = diagnostic.range {
                    range.0
                } else {
                    data.buffer.offset_of_position(&start)
                };
                if start_offset == cursor_offset {
                    current = Some(diagnostic.clone());
                }
                for line in start.line as usize..end.line as usize + 1 {
                    if line < start_line {
                        continue;
                    }
                    if line > end_line {
                        break;
                    }

                    let x0 = if line == start.line as usize {
                        start.character as f64 * width
                    } else {
                        let (_, col) = data.buffer.offset_to_line_col(
                            data.buffer.first_non_blank_character_on_line(line),
                        );
                        col as f64 * width
                    };
                    let x1 = if line == end.line as usize {
                        end.character as f64 * width
                    } else {
                        data.buffer.line_len(line) as f64 * width
                    };
                    let y1 = (line + 1) as f64 * line_height;
                    let y0 = (line + 1) as f64 * line_height - 2.0;

                    let severity = diagnostic
                        .diagnositc
                        .severity
                        .as_ref()
                        .unwrap_or(&DiagnosticSeverity::Information);
                    let color = match severity {
                        DiagnosticSeverity::Error => {
                            data.config.get_color_unchecked(LapceTheme::LAPCE_ERROR)
                        }
                        DiagnosticSeverity::Warning => {
                            data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)
                        }
                        _ => data.config.get_color_unchecked(LapceTheme::LAPCE_WARN),
                    };
                    ctx.fill(Rect::new(x0, y0, x1, y1), color);
                }
            }
        }

        if let Some(diagnostic) = current {
            if data.editor.cursor.is_normal() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(diagnostic.diagnositc.message.clone())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let size = ctx.size();
                let start = diagnostic.diagnositc.range.start;
                let rect = Rect::ZERO
                    .with_origin(Point::new(
                        0.0,
                        (start.line + 1) as f64 * line_height,
                    ))
                    .with_size(Size::new(size.width, text_size.height + 20.0));
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );

                let severity = diagnostic
                    .diagnositc
                    .severity
                    .as_ref()
                    .unwrap_or(&DiagnosticSeverity::Information);
                let color = match severity {
                    DiagnosticSeverity::Error => {
                        data.config.get_color_unchecked(LapceTheme::LAPCE_ERROR)
                    }
                    DiagnosticSeverity::Warning => {
                        data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)
                    }
                    _ => data.config.get_color_unchecked(LapceTheme::LAPCE_WARN),
                };
                ctx.stroke(rect, color, 1.0);
                ctx.draw_text(
                    &text_layout,
                    Point::new(10.0, (start.line + 1) as f64 * line_height + 10.0),
                );
            }
        }
    }

    fn paint_snippet(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.height + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = data.config.editor_text_width(ctx.text(), "W");
        if let Some(snippet) = data.editor.snippet.as_ref() {
            for (_, (start, end)) in snippet {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    data.buffer.offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    data.buffer.offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = data.buffer.line_content(line);
                    let left_col = match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match line {
                        _ if line == end_line => {
                            let max_col = data.buffer.line_end_col(line, true);
                            end_col.min(max_col)
                        }
                        _ => data.buffer.line_end_col(line, true),
                    };
                    if line_content.len() > 0 {
                        let x1 = right_col as f64 * width;
                        let y0 = line as f64 * line_height;
                        let y1 = y0 + line_height;
                        ctx.stroke(
                            Rect::new(x0, y0, x1, y1).inflate(1.0, -0.5),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                            1.0,
                        );
                    }
                }
            }
        }
    }

    fn paint_cursor(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height as f64;
        let active = self.is_focused;
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.height + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = data.config.editor_text_width(ctx.text(), "W");
        match &data.editor.cursor.mode {
            CursorMode::Normal(offset) => {
                let (line, col) = data.buffer.offset_to_line_col(*offset);
                self.paint_cursor_line(ctx, data, line, env);

                if active {
                    let cursor_x = col as f64 * width;
                    let next = data.buffer.next_grapheme_offset(
                        *offset,
                        1,
                        data.buffer.len(),
                    );
                    let char = data.buffer.slice_to_cow(*offset..next).to_string();
                    let char_width = UnicodeWidthStr::width(char.as_str()).max(1);
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(
                                cursor_x,
                                line as f64 * line_height,
                            ))
                            .with_size(Size::new(
                                width * char_width as f64,
                                line_height,
                            )),
                        data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                    );
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    data.buffer.offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    data.buffer.offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = data
                        .buffer
                        .slice_to_cow(
                            data.buffer.offset_of_line(line)
                                ..data.buffer.offset_of_line(line + 1),
                        )
                        .to_string();
                    let left_col = match mode {
                        &VisualMode::Normal => match line {
                            _ if line == start_line => start_col,
                            _ => 0,
                        },
                        &VisualMode::Linewise => 0,
                        &VisualMode::Blockwise => {
                            let max_col = data.buffer.line_end_col(line, false);
                            let left = start_col.min(end_col);
                            if left > max_col {
                                continue;
                            }
                            left
                        }
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match mode {
                        &VisualMode::Normal => match line {
                            _ if line == end_line => {
                                let max_col = data.buffer.line_end_col(line, true);
                                (end_col + 1).min(max_col)
                            }
                            _ => data.buffer.line_end_col(line, true) + 1,
                        },
                        &VisualMode::Linewise => {
                            data.buffer.line_end_col(line, true) + 1
                        }
                        &VisualMode::Blockwise => {
                            let max_col = data.buffer.line_end_col(line, true);
                            let right = match data.editor.cursor.horiz.as_ref() {
                                Some(&ColPosition::End) => max_col,
                                _ => (end_col.max(start_col) + 1).min(max_col),
                            };
                            right
                        }
                    };
                    if line_content.len() > 0 {
                        let x1 = right_col as f64 * width;

                        let y0 = line as f64 * line_height;
                        let y1 = y0 + line_height;
                        ctx.fill(
                            Rect::new(x0, y0, x1, y1),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                        );
                    }

                    let (line, col) = data.buffer.offset_to_line_col(*end);
                    let cursor_x = col as f64 * width;
                    let next =
                        data.buffer.next_grapheme_offset(*end, 1, data.buffer.len());
                    let char = data.buffer.slice_to_cow(*end..next).to_string();
                    let char_width = UnicodeWidthStr::width(char.as_str()).max(1);
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(
                                cursor_x,
                                line as f64 * line_height,
                            ))
                            .with_size(Size::new(
                                width * char_width as f64,
                                line_height,
                            )),
                        data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                    );
                }
            }
            CursorMode::Insert(selection) => {
                let offset = selection.get_cursor_offset();
                let line = data.buffer.line_of_offset(offset);
                if active {
                    let last_line = data.buffer.last_line();
                    let end_line = if end_line > last_line {
                        last_line
                    } else {
                        end_line
                    };
                    let start = data.buffer.offset_of_line(start_line);
                    let end = data.buffer.offset_of_line(end_line + 1);
                    let regions = selection.regions_in_range(start, end);
                    for region in regions {
                        if region.start() == region.end() {
                            let line = data.buffer.line_of_offset(region.start());
                            self.paint_cursor_line(ctx, data, line, env);
                        } else {
                            let start = region.start();
                            let end = region.end();
                            let paint_start_line = start_line;
                            let paint_end_line = end_line;
                            let (start_line, start_col) =
                                data.buffer.offset_to_line_col(start.min(end));
                            let (end_line, end_col) =
                                data.buffer.offset_to_line_col(start.max(end));
                            for line in paint_start_line..paint_end_line {
                                if line < start_line || line > end_line {
                                    continue;
                                }

                                let line_content = data.buffer.line_content(line);
                                let left_col = match line {
                                    _ if line == start_line => start_col,
                                    _ => 0,
                                };
                                let x0 = left_col as f64 * width;

                                let right_col = match line {
                                    _ if line == end_line => {
                                        let max_col =
                                            data.buffer.line_end_col(line, true);
                                        end_col.min(max_col)
                                    }
                                    _ => data.buffer.line_end_col(line, true),
                                };

                                if line_content.len() > 0 {
                                    let x1 = right_col as f64 * width;
                                    let y0 = line as f64 * line_height;
                                    let y1 = y0 + line_height;
                                    ctx.fill(
                                        Rect::new(x0, y0, x1, y1),
                                        data.config.get_color_unchecked(
                                            LapceTheme::EDITOR_SELECTION,
                                        ),
                                    );
                                }
                            }
                        }

                        let (line, col) =
                            data.buffer.offset_to_line_col(region.end());
                        let x = (col as f64 * width).round();
                        let y = line as f64 * line_height;
                        ctx.stroke(
                            Line::new(
                                Point::new(x, y),
                                Point::new(x, y + line_height),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        )
                    }
                }
            }
        }
    }
}

impl Widget<LapceEditorViewData> for LapceEditor {
    fn id(&self) -> Option<WidgetId> {
        Some(self.editor_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceEditorViewData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_cursor(&druid::Cursor::IBeam);
                if ctx.is_active() {
                    let new_offset = data.offset_of_mouse(
                        ctx.text(),
                        mouse_event.pos,
                        &data.config,
                    );
                    match data.editor.cursor.mode.clone() {
                        CursorMode::Normal(offset) => {
                            if new_offset != offset {
                                data.set_cursor(Cursor::new(
                                    CursorMode::Visual {
                                        start: offset,
                                        end: new_offset,
                                        mode: VisualMode::Normal,
                                    },
                                    None,
                                ));
                            }
                        }
                        CursorMode::Visual { start, end, mode } => {
                            let mode = mode.clone();
                            let editor = Arc::make_mut(&mut data.editor);
                            editor.cursor.mode = CursorMode::Visual {
                                start,
                                end: new_offset,
                                mode,
                            };
                            editor.cursor.horiz = None;
                        }
                        CursorMode::Insert(selection) => {
                            let mut new_selection = Selection::new();
                            if let Some(region) = selection.first() {
                                let new_regoin =
                                    SelRegion::new(region.start(), new_offset, None);
                                new_selection.add_region(new_regoin);
                            } else {
                                new_selection.add_region(SelRegion::new(
                                    new_offset, new_offset, None,
                                ));
                            }
                            data.set_cursor(Cursor::new(
                                CursorMode::Insert(new_selection),
                                None,
                            ));
                        }
                    }
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EnsureCursorVisible(None),
                        Target::Widget(self.container_id),
                    ));
                }
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                ctx.set_active(false);
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_active(true);
                let line_height = data.config.editor.line_height as f64;
                let line = (mouse_event.pos.y / line_height).floor() as usize;
                let last_line = data.buffer.last_line();
                let (line, col) = if line > last_line {
                    (last_line, 0)
                } else {
                    let line_end = data
                        .buffer
                        .line_end_col(line, !data.editor.cursor.is_normal());
                    let width = data.config.editor_text_width(ctx.text(), "W");

                    let col = (if data.editor.cursor.is_insert() {
                        (mouse_event.pos.x / width).round() as usize
                    } else {
                        (mouse_event.pos.x / width).floor() as usize
                    })
                    .min(line_end);
                    (line, col)
                };
                let new_offset = data.buffer.offset_of_line_col(line, col);
                match data.editor.cursor.mode.clone() {
                    CursorMode::Normal(offset) => {
                        if mouse_event.mods.shift() {
                            data.set_cursor(Cursor::new(
                                CursorMode::Visual {
                                    start: offset,
                                    end: new_offset,
                                    mode: VisualMode::Normal,
                                },
                                None,
                            ));
                        } else {
                            let editor = Arc::make_mut(&mut data.editor);
                            editor.cursor.mode = CursorMode::Normal(new_offset);
                            editor.cursor.horiz = None;
                        }
                    }
                    CursorMode::Visual { start, end, mode } => {
                        if mouse_event.mods.shift() {
                            data.set_cursor(Cursor::new(
                                CursorMode::Visual {
                                    start,
                                    end: new_offset,
                                    mode: VisualMode::Normal,
                                },
                                None,
                            ));
                        } else {
                            data.set_cursor(Cursor::new(
                                CursorMode::Normal(new_offset),
                                None,
                            ));
                        }
                    }
                    CursorMode::Insert(selection) => {
                        if mouse_event.mods.shift() {
                            let mut new_selection = Selection::new();
                            if let Some(region) = selection.first() {
                                let new_regoin =
                                    SelRegion::new(region.start(), new_offset, None);
                                new_selection.add_region(new_regoin);
                            } else {
                                new_selection.add_region(SelRegion::new(
                                    new_offset, new_offset, None,
                                ));
                            }
                            data.set_cursor(Cursor::new(
                                CursorMode::Insert(new_selection),
                                None,
                            ));
                        } else {
                            data.set_cursor(Cursor::new(
                                CursorMode::Insert(Selection::caret(new_offset)),
                                None,
                            ));
                        }
                    }
                }
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(self.container_id),
                ));
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdateWindowOrigin => {
                        let window_origin = ctx.window_origin();
                        if data.editor.window_origin != window_origin {
                            Arc::make_mut(&mut data.editor).window_origin =
                                window_origin;
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
        data: &LapceEditorViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceEditorViewData,
        data: &LapceEditorViewData,
        env: &Env,
    ) {
        let buffer = &data.buffer;
        let old_buffer = &old_data.buffer;

        let line_height = data.config.editor.line_height as f64;

        if data.editor.size != old_data.editor.size {
            ctx.request_paint();
            return;
        }

        if !old_buffer.same(buffer) {
            if buffer.max_len != old_buffer.max_len
                || buffer.num_lines != old_buffer.num_lines
            {
                ctx.request_layout();
                ctx.request_paint();
                return;
            }

            if !buffer.styles.same(&old_buffer.styles) {
                ctx.request_paint();
            }

            if buffer.rev != old_buffer.rev {
                ctx.request_paint();
            }
        }

        if old_data.editor.cursor != data.editor.cursor {
            ctx.request_paint();
        }

        if old_data.current_code_actions().is_some()
            != data.current_code_actions().is_some()
        {
            ctx.request_paint();
        }

        if old_data.on_diagnostic() != data.on_diagnostic() {
            ctx.request_paint();
        }

        if old_data.diagnostics.len() != data.diagnostics.len() {
            ctx.request_paint();
        }

        if (*old_data.main_split.active == self.view_id
            && *data.main_split.active != self.view_id)
            || (*old_data.main_split.active != self.view_id
                && *data.main_split.active == self.view_id)
        {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceEditorViewData,
        env: &Env,
    ) -> Size {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateWindowOrigin,
            Target::Widget(self.editor_id),
        ));

        let line_height = data.config.editor.line_height as f64;
        let width = data.config.editor_text_width(ctx.text(), "W");
        Size::new(
            (width * data.buffer.max_len as f64).max(bc.max().width),
            line_height * data.buffer.text_layouts.borrow().len() as f64
                + bc.max().height
                - line_height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceEditorViewData, env: &Env) {
        let line_height = data.config.editor.line_height as f64;
        self.paint_cursor(ctx, data, env);
        let rect = ctx.region().bounding_box();
        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;

        let text_layout = ctx
            .text()
            .new_text_layout("W")
            .font(
                data.config.editor.font_family(),
                data.config.editor.font_size as f64,
            )
            .build()
            .unwrap();
        let y_shift = (line_height - text_layout.size().height) / 2.0;

        let start_offset = data.buffer.offset_of_line(start_line);
        let end_offset = data.buffer.offset_of_line(end_line + 1);
        for (i, line_content) in data
            .buffer
            .slice_to_cow(start_offset..end_offset)
            .split('\n')
            .enumerate()
        {
            let line = i + start_line;
            let text_layout = data.buffer.new_text_layout(
                ctx,
                line,
                line_content,
                [rect.x0, rect.x1],
                &data.config,
            );
            ctx.draw_text(
                &text_layout,
                Point::new(0.0, line_height * line as f64 + y_shift),
            );
        }

        self.paint_snippet(ctx, data, env);
        self.paint_diagnostics(ctx, data, env);
        if data.buffer.len() == 0 {
            if let Some(placeholder) = self.placeholder.as_ref() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(placeholder.to_string())
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(0.0, y_shift));
            }
        }
    }
}

#[derive(Clone)]
pub struct RegisterContent {
    kind: VisualMode,
    content: Vec<String>,
}

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

fn get_workspace_edit_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    if let Some(edits) = get_workspace_edit_changes_edits(&url, workspace_edit) {
        Some(edits)
    } else {
        get_workspace_edit_document_changes_edits(&url, workspace_edit)
    }
}

fn get_workspace_edit_changes_edits<'a>(
    url: &Url,
    workspace_edit: &'a WorkspaceEdit,
) -> Option<Vec<&'a TextEdit>> {
    let changes = workspace_edit.changes.as_ref()?;
    changes.get(url).map(|c| c.iter().map(|t| t).collect())
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

fn next_in_file_errors_offset(
    position: Position,
    path: &String,
    file_diagnostics: &Vec<(&String, Vec<Position>)>,
) -> (String, Position) {
    for (current_path, positions) in file_diagnostics {
        if &path == current_path {
            for error_position in positions {
                if error_position.line > position.line
                    || (error_position.line == position.line
                        && error_position.character > position.character)
                {
                    return ((*current_path).clone(), *error_position);
                }
            }
        }
        if current_path > &path {
            return ((*current_path).clone(), positions[0]);
        }
    }
    ((*file_diagnostics[0].0).clone(), file_diagnostics[0].1[0])
}
