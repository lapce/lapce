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
        LapceUICommand, LapceWorkbenchCommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{EditorConfig, LapceTheme},
    data::{EditorTabChild, EditorView, FocusArea, LapceTabData},
    document::{BufferContent, LocalBufferKind},
    editor::LapceEditorBufferData,
    keypress::KeyPressFocus,
    palette::PaletteStatus,
    panel::{PanelData, PanelKind},
};

use crate::{
    editor::{
        container::LapceEditorContainer, header::LapceEditorHeader, LapceEditor,
    },
    find::FindBox,
    ime::ImeComponent,
    plugin::PluginInfo,
    settings::LapceSettingsPanel,
};

pub struct LapceEditorView {
    pub view_id: WidgetId,
    pub header: WidgetPod<LapceTabData, LapceEditorHeader>,
    pub editor: WidgetPod<LapceTabData, LapceEditorContainer>,
    pub find: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    cursor_blink_timer: TimerToken,
    autosave_timer: TimerToken,
    display_border: bool,
    background_color_name: &'static str,
    ime: ImeComponent,
}

pub fn editor_tab_child_widget(
    child: &EditorTabChild,
    data: &LapceTabData,
) -> Box<dyn Widget<LapceTabData>> {
    match child {
        EditorTabChild::Editor(view_id, editor_id, find_view_id) => {
            LapceEditorView::new(*view_id, *editor_id, *find_view_id).boxed()
        }
        EditorTabChild::Settings {
            settings_widget_id,
            editor_tab_id,
            keymap_input_view_id,
        } => LapceSettingsPanel::new(
            data,
            *settings_widget_id,
            *editor_tab_id,
            *keymap_input_view_id,
        )
        .boxed(),
        EditorTabChild::Plugin {
            widget_id,
            editor_tab_id,
            volt_id,
            ..
        } => PluginInfo::new_scroll(*widget_id, *editor_tab_id, volt_id.clone())
            .boxed(),
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
            autosave_timer: TimerToken::INVALID,
            display_border: true,
            background_color_name: LapceTheme::EDITOR_BACKGROUND,
            ime: ImeComponent::default(),
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

    pub fn hide_border(mut self) -> Self {
        self.display_border = false;
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

    pub fn set_background_color(
        mut self,
        background_color_name: &'static str,
    ) -> Self {
        self.background_color_name = background_color_name;
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
        data.focus = Arc::new(self.view_id);
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
                LocalBufferKind::PluginSeach => {}
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
                LocalBufferKind::PathName => {}
                LocalBufferKind::Rename => {
                    data.focus_area = FocusArea::Rename;
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
        panel: &PanelData,
        env: &Env,
    ) {
        match cmd {
            LapceUICommand::RunCodeAction(action, plugin_id) => {
                data.run_code_action(ctx, action, plugin_id);
            }
            LapceUICommand::ApplyWorkspaceEdit(edit) => {
                data.apply_workspace_edit(ctx, edit);
            }
            LapceUICommand::EnsureCursorVisible(position) => {
                self.ensure_cursor_visible(ctx, data, panel, position.as_ref(), env);
            }
            LapceUICommand::EnsureCursorPosition(position) => {
                self.ensure_cursor_position(ctx, data, panel, position, env);
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
                                * data.config.editor.line_height() as f64,
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
        let line_height = editor_config.line_height();

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
        panel: &PanelData,
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

        let size = LapceEditor::get_size(data, ctx.text(), editor_size, panel, env);
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
        panel: &PanelData,
        position: Option<&EnsureVisiblePosition>,
        env: &Env,
    ) {
        let line_height = data.config.editor.line_height() as f64;
        let editor_size = *data.editor.size.borrow();
        let size = LapceEditor::get_size(data, ctx.text(), editor_size, panel, env);

        let sticky_header_height = data.editor.sticky_header.borrow().height;
        let rect = Self::cursor_region(data, ctx.text());
        let rect =
            Rect::new(rect.x0, rect.y0 - sticky_header_height, rect.x1, rect.y1);
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
                self.ensure_cursor_position(ctx, data, panel, position, env);
            } else {
                let scroll_offset = scroll.offset();
                if (scroll_offset.y - old_scroll_offset.y).abs() > line_height * 2.0
                {
                    self.ensure_cursor_position(
                        ctx,
                        data,
                        panel,
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
        let inlay_hints = data.doc.line_phantom_text(&data.config, line);
        let col = inlay_hints.col_after(col, false);

        let width = data.config.editor_char_width(text);
        let cursor_x = data
            .doc
            .line_point_of_line_col(
                text,
                line,
                col,
                data.config.editor.font_size,
                &data.config,
            )
            .x;
        let line_height = data.config.editor.line_height() as f64;

        let y = if data.editor.is_code_lens() {
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

        let surrounding_lines_height =
            (data.config.editor.cursor_surrounding_lines as f64 * line_height)
                .min(data.editor.size.borrow().height / 2.);

        Rect::ZERO
            .with_size(Size::new(width, line_height))
            .with_origin(Point::new(cursor_x, y))
            .inflate(width, surrounding_lines_height)
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
                    if event.should_propagate_to_hidden() || data.find.visual {
                        find.event(ctx, event, data, env);
                    }
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
                    let editor_data = data.editor_view_content(self.view_id);
                    if data.config.editor.blink_interval > 0 {
                        self.cursor_blink_timer = ctx.request_timer(
                            Duration::from_millis(data.config.editor.blink_interval),
                            None,
                        );
                        *editor_data.editor.last_cursor_instant.borrow_mut() =
                            Instant::now();
                        ctx.request_paint();
                    }
                    self.request_focus(ctx, data, true);
                    self.ensure_cursor_visible(
                        ctx,
                        &editor_data,
                        &data.panel,
                        None,
                        env,
                    );
                }
            }
            Event::Timer(id) if self.cursor_blink_timer == *id => {
                ctx.set_handled();
                if data.config.editor.blink_interval > 0 {
                    if ctx.is_focused() {
                        ctx.request_paint();
                        self.cursor_blink_timer = ctx.request_timer(
                            Duration::from_millis(data.config.editor.blink_interval),
                            None,
                        );
                    } else {
                        self.cursor_blink_timer = TimerToken::INVALID;
                    }
                }
            }
            Event::Timer(id) if self.autosave_timer == *id => {
                ctx.set_handled();
                if let Some(editor) = data
                    .main_split
                    .active
                    .and_then(|active| data.main_split.editors.get(&active))
                    .cloned()
                {
                    // If autosave is enabled, and the content is a file that we can save,
                    if data.config.editor.autosave_interval > 0
                        && editor.content.is_file()
                    {
                        if ctx.is_focused() {
                            let doc = data.main_split.editor_doc(self.view_id);
                            if !doc.buffer().is_pristine() {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Focus(FocusCommand::Save),
                                        data: None,
                                    },
                                    Target::Widget(editor.view_id),
                                ));
                            }
                            self.autosave_timer = ctx.request_timer(
                                Duration::from_millis(
                                    data.config.editor.autosave_interval,
                                ),
                                None,
                            );
                        } else {
                            self.cursor_blink_timer = TimerToken::INVALID;
                        }
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
                if key_event.is_composing {
                    if data.config.editor.blink_interval > 0 {
                        self.cursor_blink_timer = ctx.request_timer(
                            Duration::from_millis(data.config.editor.blink_interval),
                            None,
                        );
                        *editor_data.editor.last_cursor_instant.borrow_mut() =
                            Instant::now();
                    }
                    if let Some(text) = self.ime.get_input_text() {
                        Arc::make_mut(&mut editor_data.doc).clear_ime_text();
                        editor_data.receive_char(ctx, &text);
                    } else if !self.ime.borrow().text().is_empty() {
                        let offset = editor_data.editor.cursor.offset();
                        let (line, col) =
                            editor_data.doc.buffer().offset_to_line_col(offset);
                        let doc = Arc::make_mut(&mut editor_data.doc);
                        doc.set_ime_pos(line, col, self.ime.get_shift());
                        doc.set_ime_text(self.ime.borrow().text().to_string());
                    } else {
                        Arc::make_mut(&mut editor_data.doc).clear_ime_text();
                    }
                } else {
                    Arc::make_mut(&mut editor_data.doc).clear_ime_text();
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
                            &data.panel,
                            None,
                            env,
                        );
                    }
                    editor_data.sync_buffer_position(
                        self.editor.widget().editor.widget().inner().offset(),
                    );
                    editor_data.get_code_actions(ctx);

                    data.keypress = keypress.clone();
                }
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

                // We don't want to send this on `FocusCommand::Save`, especially when autosave is enabled.
                if command.kind != CommandKind::Focus(FocusCommand::Save) {
                    self.ensure_cursor_visible(
                        ctx,
                        &editor_data,
                        &data.panel,
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
                    &data.panel,
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
                ctx.register_text_input(self.ime.ime_handler());
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                if editor.cursor.is_insert() {
                    self.ime.set_active(true);
                } else {
                    self.ime.set_active(false);
                }
            }
            LifeCycle::FocusChanged(is_focus) => {
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                if !*is_focus {
                    match editor.content {
                        BufferContent::Local(LocalBufferKind::Palette) => {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Focus(
                                        FocusCommand::ModalClose,
                                    ),
                                    data: None,
                                },
                                Target::Widget(data.palette.widget_id),
                            ));
                        }
                        BufferContent::Local(LocalBufferKind::Rename) => {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Focus(
                                        FocusCommand::ModalClose,
                                    ),
                                    data: None,
                                },
                                Target::Widget(data.rename.view_id),
                            ));
                        }
                        BufferContent::Local(LocalBufferKind::PathName) => {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ExplorerEndNaming {
                                    apply_naming: true,
                                },
                                Target::Auto,
                            ));
                        }
                        _ => {}
                    }
                } else {
                    let editor_data = data.editor_view_content(self.view_id);
                    let offset = editor_data.editor.cursor.offset();
                    let (_, origin) = editor_data.doc.points_of_offset(
                        ctx.text(),
                        offset,
                        &editor_data.editor.view,
                        &editor_data.config,
                    );
                    self.ime.set_origin(
                        *editor_data.editor.window_origin.borrow()
                            + (origin.x, origin.y),
                    );

                    if editor.content.is_palette()
                        && data.palette.status == PaletteStatus::Inactive
                    {
                        let cmd = if data.workspace.path.is_none() {
                            LapceWorkbenchCommand::PaletteWorkspace
                        } else {
                            LapceWorkbenchCommand::Palette
                        };
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Workbench(cmd),
                                data: None,
                            },
                            Target::Auto,
                        ));
                    }
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

        let offset = editor_data.editor.cursor.offset();
        let old_offset = old_editor_data.editor.cursor.offset();

        if data.config.editor.blink_interval > 0 && *data.focus == self.view_id {
            let reset = if *old_data.focus != self.view_id {
                true
            } else {
                let mode = editor_data.editor.cursor.get_mode();
                let old_mode = old_editor_data.editor.cursor.get_mode();
                let (line, col) =
                    editor_data.doc.buffer().offset_to_line_col(offset);
                let (old_line, old_col) =
                    old_editor_data.doc.buffer().offset_to_line_col(old_offset);
                mode != old_mode || line != old_line || col != old_col
            };

            if reset {
                self.cursor_blink_timer = ctx.request_timer(
                    Duration::from_millis(data.config.editor.blink_interval),
                    None,
                );
                *editor_data.editor.last_cursor_instant.borrow_mut() =
                    Instant::now();
                ctx.request_paint();
            }
        }

        if data.config.editor.autosave_interval > 0
            && editor_data.doc.rev() != old_editor_data.doc.rev()
        {
            self.autosave_timer = ctx.request_timer(
                Duration::from_millis(data.config.editor.autosave_interval),
                None,
            );
        }

        if old_data.config.core.modal != data.config.core.modal
            && !editor_data.doc.content().is_input()
        {
            if !data.config.core.modal {
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
            if syntax.line_height != data.config.editor.line_height()
                || syntax.lens_height != data.config.editor.code_lens_font_size
            {
                let content = editor_data.doc.content().clone();
                let tab_id = data.id;
                let event_sink = ctx.get_external_handle();
                let mut syntax = syntax.clone();
                let line_height = data.config.editor.line_height();
                let lens_height = data.config.editor.code_lens_font_size;
                rayon::spawn(move || {
                    syntax.update_lens_height(line_height, lens_height);
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSyntax {
                            content,
                            syntax: SingleUse::new(syntax),
                        },
                        Target::Widget(tab_id),
                    );
                });
            }
        }

        let mut update_ime_origin = false;
        match (
            old_editor_data.editor.cursor.is_insert(),
            editor_data.editor.cursor.is_insert(),
        ) {
            (true, false) => {
                self.ime.set_active(false);
            }
            (false, true) => {
                self.ime.set_active(true);
                update_ime_origin = true;
            }
            (false, false) | (true, true) => {}
        }

        if offset != old_offset
            || editor_data.editor.scroll_offset
                != old_editor_data.editor.scroll_offset
        {
            update_ime_origin = true;
        }

        if update_ime_origin {
            let (_, origin) = editor_data.doc.points_of_offset(
                ctx.text(),
                offset,
                &editor_data.editor.view,
                &editor_data.config,
            );
            self.ime.set_origin(
                *editor_data.editor.window_origin.borrow() + (origin.x, origin.y),
            );
        }

        if editor_data.editor.content != old_editor_data.editor.content {
            ctx.request_layout();
            if let Some(editor_tab_id) = editor_data.editor.tab_id.as_ref() {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EditorContentChanged,
                    Target::Widget(*editor_tab_id),
                ));
            }
        }
        if editor_data.editor.view != old_editor_data.editor.view {
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
                    data.config.get_color_unchecked(self.background_color_name),
                );
                if self.display_border {
                    ctx.stroke(
                        rect.inflate(4.5, -0.5),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            } else {
                ctx.fill(
                    rect.inflate(5.0, 5.0),
                    data.config.get_color_unchecked(self.background_color_name),
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
