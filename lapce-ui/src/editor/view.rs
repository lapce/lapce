use std::{
    iter::Iterator,
    ops::Sub,
    sync::Arc,
    time::{Duration, Instant},
};

use druid::{
    piet::PietText, BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, Rect, RenderContext,
    SingleUse, Size, Target, TimerToken, Vec2, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use lapce_core::command::{EditCommand, FocusCommand};
use lapce_data::{
    command::{
        CommandExecuted, CommandKind, EnsureVisiblePosition, LapceCommand,
        LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{EditorConfig, LapceTheme},
    data::{
        EditorTabChild, EditorView, FocusArea, LapceTabData, PanelData, PanelKind,
    },
    document::{BufferContent, LocalBufferKind},
    editor::LapceEditorBufferData,
    keypress::KeyPressFocus,
    panel::PanelPosition,
    settings::SettingsValueKind,
};

use crate::{
    editor::{
        container::LapceEditorContainer, header::LapceEditorHeader, LapceEditor,
    },
    find::FindBox,
    settings::LapceSettingsPanel,
};

pub struct LapceEditorView {
    pub view_id: WidgetId,
    pub header: WidgetPod<LapceTabData, LapceEditorHeader>,
    pub editor: WidgetPod<LapceTabData, LapceEditorContainer>,
    pub find: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    cursor_blink_timer: TimerToken,
    last_idle_timer: TimerToken,
}

pub fn editor_tab_child_widget(
    child: &EditorTabChild,
    data: &LapceTabData,
) -> Box<dyn Widget<LapceTabData>> {
    match child {
        EditorTabChild::Editor(view_id, editor_id, find_view_id) => {
            LapceEditorView::new(*view_id, *editor_id, *find_view_id).boxed()
        }
        EditorTabChild::Settings(widget_id, editor_tab_id) => {
            LapceSettingsPanel::new(data, *widget_id, *editor_tab_id).boxed()
        }
    }
}

impl LapceEditorView {
    pub fn new(
        view_id: WidgetId,
        editor_id: WidgetId,
        find_view_id: Option<(WidgetId, WidgetId)>,
    ) -> LapceEditorView {
        let header = LapceEditorHeader::new(view_id);
        let editor = LapceEditorContainer::new(view_id, editor_id);
        let find = find_view_id.map(|(find_view_id, find_editor_id)| {
            WidgetPod::new(FindBox::new(find_view_id, find_editor_id, view_id))
                .boxed()
        });
        Self {
            view_id,
            header: WidgetPod::new(header),
            editor: WidgetPod::new(editor),
            find,
            cursor_blink_timer: TimerToken::INVALID,
            last_idle_timer: TimerToken::INVALID,
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

    pub fn set_placeholder(mut self, placeholder: String) -> Self {
        self.editor
            .widget_mut()
            .editor
            .widget_mut()
            .inner_mut()
            .child_mut()
            .placeholder = Some(placeholder);
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
                LapceUICommand::EnsureEditorTabActiveVisible,
                Target::Widget(editor_tab_id),
            ));
        }
        match &editor.content {
            BufferContent::File(_) | BufferContent::Scratch(..) => {
                data.focus_area = FocusArea::Editor;
                data.main_split.active = Arc::new(Some(self.view_id));
                data.main_split.active_tab = Arc::new(editor.tab_id);
            }
            BufferContent::Local(kind) => match kind {
                LocalBufferKind::Keymap => {}
                LocalBufferKind::Settings => {}
                LocalBufferKind::Palette => {
                    data.focus_area = FocusArea::Palette;
                }
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
            BufferContent::SettingsValue(..) => {}
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
            LapceUICommand::RunCodeAction(action) => {
                data.run_code_action(action);
            }
            LapceUICommand::EnsureCursorVisible(position) => {
                self.ensure_cursor_visible(
                    ctx,
                    data,
                    panels,
                    position.as_ref(),
                    env,
                );
            }
            LapceUICommand::EnsureCursorPosition(position) => {
                self.ensure_cursor_position(ctx, data, panels, position, env);
            }
            LapceUICommand::EnsureRectVisible(rect) => {
                self.ensure_rect_visible(ctx, data, *rect, env);
            }
            LapceUICommand::ResolveCompletion(buffer_id, rev, offset, item) => {
                if data.doc.id() != *buffer_id {
                    return;
                }
                if data.doc.rev() != *rev {
                    return;
                }
                if data.editor.cursor.offset() != *offset {
                    return;
                }
                let offset = data.editor.cursor.offset();
                let line = data.doc.buffer().line_of_offset(offset);
                let _ = data.apply_completion_item(item);
                let new_offset = data.editor.cursor.offset();
                let new_line = data.doc.buffer().line_of_offset(new_offset);
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
                let offset = self.editor.widget().editor.widget().inner().offset();
                if data.editor.scroll_offset != offset {
                    self.editor
                        .widget_mut()
                        .editor
                        .widget_mut()
                        .inner_mut()
                        .child_mut()
                        .mouse_pos += offset - data.editor.scroll_offset;
                }
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

    // Calculate the new view (as a Rect) for cursor to be at `position`.
    // `cursor_center` is where the cursor is currently.
    fn view_rect_for_position(
        position: &EnsureVisiblePosition,
        cursor_center: Point,
        editor_size: &Size,
        editor_config: &EditorConfig,
    ) -> Rect {
        // TODO: scroll margin (in number of lines) should be configurable.
        const MARGIN_LINES: usize = 1;
        let line_height = editor_config.line_height;

        // The origin of a rect is its top-left corner.  Inflating a point
        // creates a rect that centers at the point.
        let half_width = (editor_size.width / 2.0).ceil();
        let half_height = (editor_size.height / 2.0).ceil();

        // Find the top edge of the cursor.
        let cursor_top =
            cursor_center.sub((0.0, ((line_height as f64) * 0.5).floor()));

        // Find where the center of the rect to show in the editor view.
        let view_center = match position {
            EnsureVisiblePosition::CenterOfWindow => {
                // Cursor line will be at the center of the view.
                cursor_top
            }
            EnsureVisiblePosition::TopOfWindow => {
                // Cursor line will be at the top edge of the view, thus the
                // view center will be below the current cursor.y by
                // `half_height` minus `margin`.
                let h = (half_height as usize)
                    .saturating_sub(MARGIN_LINES * line_height);
                Point::new(cursor_top.x, cursor_top.y + (h as f64))
                // TODO: When the cursor is near the top of the *buffer*, the
                // view will not move for this command.  We need an ephemeral
                // message, on the status bar for example, to inform the user.
                // This is not an error or warning.
            }
            EnsureVisiblePosition::BottomOfWindow => {
                // Cursor line will be shown at the bottom edge of the window,
                // thus the view center will be above the current cursor.y by
                // `half_height` minus `margin`.
                let h = (half_height as usize)
                    // Plus 1 to compensate for cursor_top.
                    .saturating_sub((MARGIN_LINES + 1) * line_height);
                let y = cursor_top.y as usize;
                let y = if y > h { y - h } else { y };
                Point::new(cursor_top.x, y as f64)
                // TODO: See above for when cursor is near the top of the
                // *buffer*.
            }
        };
        Rect::ZERO
            .with_origin(view_center)
            .inflate(half_width, half_height)
    }

    pub fn ensure_cursor_position(
        &mut self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        position: &EnsureVisiblePosition,
        env: &Env,
    ) {
        // This is where the cursor currently is, relative to the buffer's
        // origin.
        let cursor_center = Self::cursor_region(data, ctx.text()).center();

        let editor_size = *data.editor.size.borrow();
        let rect = Self::view_rect_for_position(
            position,
            cursor_center,
            &editor_size,
            &data.config.editor,
        );

        let size = LapceEditor::get_size(data, ctx.text(), editor_size, panels, env);
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
        let size = LapceEditor::get_size(data, ctx.text(), editor_size, panels, env);

        let rect = Self::cursor_region(data, ctx.text());
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
                self.ensure_cursor_position(ctx, data, panels, position, env);
            } else {
                let scroll_offset = scroll.offset();
                if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
                {
                    self.ensure_cursor_position(
                        ctx,
                        data,
                        panels,
                        &EnsureVisiblePosition::CenterOfWindow,
                        env,
                    );
                }
            }
        }
    }

    fn cursor_region(data: &LapceEditorBufferData, text: &mut PietText) -> Rect {
        let offset = data.editor.cursor.offset();
        let (line, col) = data.doc.buffer().offset_to_line_col(offset);
        let width = data.config.editor_char_width(text);
        let cursor_x = data
            .doc
            .point_of_line_col(
                text,
                line,
                col,
                data.config.editor.font_size,
                &data.config,
            )
            .x;
        let line_height = data.config.editor.line_height as f64;

        let y = if data.editor.code_lens {
            let empty_vec = Vec::new();
            let normal_lines = data
                .doc
                .syntax()
                .map(|s| &s.normal_lines)
                .unwrap_or(&empty_vec);

            let mut y = 0.0;
            let mut current_line = 0;
            let mut normal_lines = normal_lines.iter();
            loop {
                match normal_lines.next() {
                    Some(next_normal_line) => {
                        let next_normal_line = *next_normal_line;
                        if next_normal_line < line {
                            let chunk_height = data.config.editor.code_lens_font_size
                                as f64
                                * (next_normal_line - current_line) as f64
                                + line_height;
                            y += chunk_height;
                            current_line = next_normal_line + 1;
                            continue;
                        };
                        y += (line - current_line) as f64
                            * data.config.editor.code_lens_font_size as f64;
                        break;
                    }
                    None => {
                        y += (line - current_line) as f64
                            * data.config.editor.code_lens_font_size as f64;
                        break;
                    }
                }
            }
            y
        } else {
            let line = if let EditorView::Diff(version) = &data.editor.view {
                data.doc.history_visual_line(version, line)
            } else {
                line
            };
            line as f64 * line_height
        };

        Rect::ZERO
            .with_size(Size::new(width, line_height))
            .with_origin(Point::new(cursor_x, y))
            .inflate(width, line_height)
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
                Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {}
                _ => {
                    find.event(ctx, event, data, env);
                }
            }
        }

        if ctx.is_handled() {
            return;
        }

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
                    let editor_data = data.editor_view_content(self.view_id);
                    self.ensure_cursor_visible(
                        ctx,
                        &editor_data,
                        &data.panels,
                        None,
                        env,
                    );
                }
            }
            Event::Timer(id) if self.cursor_blink_timer == *id => {
                ctx.set_handled();
                if data.focus == self.view_id {
                    ctx.request_paint();
                    self.cursor_blink_timer =
                        ctx.request_timer(Duration::from_millis(500), None);
                } else {
                    self.cursor_blink_timer = TimerToken::INVALID;
                }
            }
            Event::Timer(id) if self.last_idle_timer == *id => {
                ctx.set_handled();
                let editor_data = data.editor_view_content(self.view_id);
                if let BufferContent::SettingsValue(_, kind, parent, key) =
                    &editor_data.editor.content
                {
                    let content = editor_data.doc.buffer().text().to_string();
                    let new_value = match kind {
                        SettingsValueKind::String => {
                            Some(serde_json::json!(content))
                        }
                        SettingsValueKind::Number => {
                            content.parse::<i64>().ok().map(|n| serde_json::json!(n))
                        }
                        SettingsValueKind::Bool => None,
                    };
                    if let Some(new_value) = new_value {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSettingsFile(
                                parent.to_string(),
                                key.to_string(),
                                new_value,
                            ),
                            Target::Widget(data.id),
                        ));
                    }
                }
            }
            _ => {}
        }

        let editor = data.main_split.editors.get(&self.view_id).unwrap().clone();
        let mut editor_data = data.editor_view_content(self.view_id);
        let doc = editor_data.doc.clone();
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
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_COMMAND);
                if editor_data.run_command(
                    ctx,
                    command,
                    None,
                    Modifiers::empty(),
                    env,
                ) == CommandExecuted::Yes
                {
                    ctx.set_handled();
                }
                self.ensure_cursor_visible(
                    ctx,
                    &editor_data,
                    &data.panels,
                    None,
                    env,
                );
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
        data.update_from_editor_buffer_data(editor_data, &editor, &doc);

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
                if editor.scroll_offset.x > 0.0 || editor.scroll_offset.y > 0.0 {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ForceScrollTo(
                            editor.scroll_offset.x,
                            editor.scroll_offset.y,
                        ),
                        Target::Widget(editor.view_id),
                    ));
                }
            }
            LifeCycle::FocusChanged(is_focus) => {
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                if !*is_focus
                    && editor.content
                        == BufferContent::Local(LocalBufferKind::Palette)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ModalClose),
                            data: None,
                        },
                        Target::Widget(data.palette.widget_id),
                    ));
                }
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

        let old_editor_data = old_data.editor_view_content(self.view_id);
        let editor_data = data.editor_view_content(self.view_id);

        if let BufferContent::SettingsValue(..) = &editor_data.editor.content {
            if !editor_data.doc.buffer().is_pristine()
                && (editor_data.doc.buffer().len()
                    != old_editor_data.doc.buffer().len()
                    || editor_data.doc.buffer().text().slice_to_cow(..)
                        != old_editor_data.doc.buffer().text().slice_to_cow(..))
            {
                self.last_idle_timer =
                    ctx.request_timer(Duration::from_millis(500), None);
            }
        }

        if data.focus == self.view_id {
            let reset = if old_data.focus != self.view_id {
                true
            } else {
                let offset = editor_data.editor.cursor.offset();
                let old_offset = old_editor_data.editor.cursor.offset();
                let (line, col) =
                    editor_data.doc.buffer().offset_to_line_col(offset);
                let (old_line, old_col) =
                    old_editor_data.doc.buffer().offset_to_line_col(old_offset);
                line != old_line || col != old_col
            };

            if reset {
                self.cursor_blink_timer =
                    ctx.request_timer(Duration::from_millis(500), None);
                *editor_data.editor.last_cursor_instant.borrow_mut() =
                    Instant::now();
                ctx.request_paint();
            }
        }

        if old_data.config.lapce.modal != data.config.lapce.modal
            && !editor_data.doc.content().is_input()
        {
            if !data.config.lapce.modal {
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Edit(EditCommand::InsertMode),
                        data: None,
                    },
                    Target::Widget(self.view_id),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Edit(EditCommand::NormalMode),
                        data: None,
                    },
                    Target::Widget(self.view_id),
                ));
            }
        }

        if let Some(syntax) = editor_data.doc.syntax() {
            if syntax.line_height != data.config.editor.line_height
                || syntax.lens_height != data.config.editor.code_lens_font_size
            {
                let content = editor_data.doc.content().clone();
                let tab_id = data.id;
                let event_sink = ctx.get_external_handle();
                let mut syntax = syntax.clone();
                let line_height = data.config.editor.line_height;
                let lens_height = data.config.editor.code_lens_font_size;
                let rev = editor_data.doc.rev();
                rayon::spawn(move || {
                    syntax.update_lens_height(line_height, lens_height);
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSyntax {
                            content,
                            rev,
                            syntax: SingleUse::new(syntax),
                        },
                        Target::Widget(tab_id),
                    );
                });
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
        if let EditorView::Diff(version) = &editor_data.editor.view {
            let old_history = old_editor_data.doc.get_history(version);
            let history = editor_data.doc.get_history(version);
            match (history, old_history) {
                (None, None) => {}
                (None, Some(_)) | (Some(_), None) => {
                    ctx.request_layout();
                }
                (Some(history), Some(old_history)) => {
                    if !history.same(old_history) {
                        ctx.request_layout();
                    }
                }
            }
        }
        if editor_data.doc.buffer().is_pristine()
            != old_editor_data.doc.buffer().is_pristine()
        {
            ctx.request_paint();
        }
        if editor_data.editor.cursor != old_editor_data.editor.cursor {
            ctx.request_paint();
        }

        let doc = &editor_data.doc;
        let old_doc = &old_editor_data.doc;
        if doc.buffer().max_len() != old_doc.buffer().max_len()
            || doc.buffer().num_lines() != old_doc.buffer().num_lines()
        {
            ctx.request_layout();
        }

        match (doc.styles(), old_doc.styles()) {
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

        if doc.buffer().rev() != old_doc.buffer().rev() {
            ctx.request_paint();
        }

        if old_editor_data.current_code_actions().is_some()
            != editor_data.current_code_actions().is_some()
        {
            ctx.request_paint();
        }
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
            let rect = ctx.size().to_rect();
            if editor.content.is_input() {
                ctx.fill(
                    rect.inflate(5.0, 0.0),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                );
                ctx.stroke(
                    rect.inflate(4.5, -0.5),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            } else {
                ctx.fill(
                    rect.inflate(5.0, 5.0),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                );
            }
        }

        self.editor.paint(ctx, data, env);
        self.header.paint(ctx, data, env);
        if let Some(find) = self.find.as_mut() {
            find.paint(ctx, data, env);
        }
    }
}
