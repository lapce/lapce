use std::{collections::HashMap, path::PathBuf, sync::Arc, thread};

use druid::{
    theme, BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, Insets,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Size, Target, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use lsp_types::{CallHierarchyOptions, DiagnosticSeverity};

use crate::{
    buffer::{BufferId, BufferNew, BufferState, BufferUpdate, UpdateEvent},
    code_action::CodeAction,
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    completion::{CompletionContainer, CompletionNew, CompletionStatus},
    data::{
        EditorDiagnostic, EditorKind, LapceEditorLens, LapceMainSplitData,
        LapceTabData,
    },
    editor::{EditorLocationNew, LapceEditorView},
    palette::{NewPalette, PaletteViewLens},
    panel::{PanelPosition, PanelResizePosition},
    scroll::LapceScrollNew,
    source_control::SourceControlNew,
    split::LapceSplitNew,
    state::{LapceWorkspace, LapceWorkspaceType},
    status::LapceStatusNew,
};

pub struct LapceTabNew {
    id: WidgetId,
    main_split: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    completion: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    palette: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    code_action: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    status: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    panels:
        HashMap<WidgetId, WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    current_bar_hover: Option<PanelResizePosition>,
}

impl LapceTabNew {
    pub fn new(data: &LapceTabData) -> Self {
        let editor = data.main_split.active_editor();
        let main_split = LapceSplitNew::new(*data.main_split.split_id)
            .with_flex_child(
                LapceEditorView::new(
                    editor.view_id,
                    editor.container_id,
                    editor.editor_id,
                )
                .lens(LapceEditorLens(editor.view_id))
                .boxed(),
                1.0,
            );
        let completion = CompletionContainer::new(&data.completion);
        let palette = NewPalette::new(
            &data.palette,
            data.main_split
                .editors
                .get(&data.palette.preview_editor)
                .unwrap(),
        );
        let status = LapceStatusNew::new();
        let code_action = CodeAction::new();

        let mut panels = HashMap::new();
        let source_control = SourceControlNew::new(&data);
        panels.insert(
            data.source_control.widget_id,
            WidgetPod::new(source_control.boxed()),
        );

        Self {
            id: data.id,
            main_split: WidgetPod::new(main_split.boxed()),
            completion: WidgetPod::new(completion.boxed()),
            code_action: WidgetPod::new(code_action.boxed()),
            palette: WidgetPod::new(palette.boxed()),
            status: WidgetPod::new(status.boxed()),
            panels,
            current_bar_hover: None,
        }
    }

    fn update_split_point(&mut self, data: &mut LapceTabData, mouse_pos: Point) {
        if let Some(position) = self.current_bar_hover.as_ref() {
            match position {
                PanelResizePosition::Left => {
                    data.panel_size.left = mouse_pos.x.round().max(50.0);
                }
                PanelResizePosition::LeftSplit => (),
            }
        }
    }

    fn bar_hit_test(
        &self,
        data: &LapceTabData,
        mouse_pos: Point,
    ) -> Option<PanelResizePosition> {
        let panel_left_top_shown = data
            .panels
            .get(&PanelPosition::LeftTop)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_bottom_shown = data
            .panels
            .get(&PanelPosition::LeftBottom)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        if panel_left_bottom_shown || panel_left_top_shown {
            let left = data.panel_size.left;
            if mouse_pos.x >= left - 3.0 && mouse_pos.x <= left + 3.0 {
                return Some(PanelResizePosition::Left);
            }
        }
        None
    }
}

impl Widget<LapceTabData> for LapceTabNew {
    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    if let Some(position) = self.bar_hit_test(data, mouse.pos) {
                        self.current_bar_hover = Some(position);
                        ctx.set_active(true);
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if mouse.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                }
            }
            Event::MouseMove(mouse) => {
                if ctx.is_active() {
                    self.update_split_point(data, mouse.pos);
                    ctx.request_layout();
                    ctx.set_handled();
                } else {
                    match self.bar_hit_test(data, mouse.pos) {
                        Some(PanelResizePosition::Left) => {
                            ctx.set_cursor(&Cursor::ResizeLeftRight)
                        }
                        Some(PanelResizePosition::LeftSplit) => {
                            ctx.set_cursor(&Cursor::ResizeUpDown)
                        }
                        None => ctx.clear_cursor(),
                    }
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdateWindowOrigin => {
                        data.window_origin = ctx.window_origin();
                    }
                    LapceUICommand::LoadBuffer { path, content } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).load_content(content);
                        data.main_split.notify_update_text_layouts(ctx, path);
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateDiffFiles(files) => {
                        Arc::make_mut(&mut data.source_control).diff_files =
                            files.to_owned();
                        ctx.set_handled();
                    }
                    LapceUICommand::PublishDiagnostics(diagnostics) => {
                        let path = PathBuf::from(diagnostics.uri.path());
                        let diagnostics = diagnostics
                            .diagnostics
                            .iter()
                            .map(|d| EditorDiagnostic {
                                range: None,
                                diagnositc: d.clone(),
                            })
                            .collect();
                        data.main_split
                            .diagnostics
                            .insert(path, Arc::new(diagnostics));

                        let mut errors = 0;
                        let mut warnings = 0;
                        for (_, diagnositics) in data.main_split.diagnostics.iter() {
                            for diagnositic in diagnositics.iter() {
                                if let Some(severity) =
                                    diagnositic.diagnositc.severity
                                {
                                    match severity {
                                        DiagnosticSeverity::Error => errors += 1,
                                        DiagnosticSeverity::Warning => warnings += 1,
                                        _ => (),
                                    }
                                }
                            }
                        }
                        data.main_split.error_count = errors;
                        data.main_split.warning_count = warnings;

                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentFormatAndSave(path, rev, result) => {
                        data.main_split
                            .document_format_and_save(ctx, path, *rev, result);
                        ctx.set_handled();
                    }
                    LapceUICommand::BufferSave(path, rev) => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        if buffer.rev == *rev {
                            Arc::make_mut(buffer).dirty = false;
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::LoadBufferAndGoToPosition {
                        path,
                        content,
                        editor_view_id,
                        location,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).load_content(content);
                        data.main_split.notify_update_text_layouts(ctx, path);
                        data.main_split.go_to_location(
                            ctx,
                            *editor_view_id,
                            location.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::OpenFile(path) => {
                        data.main_split.open_file(ctx, path);
                        ctx.set_handled();
                    }
                    LapceUICommand::GoToLocationNew(editor_view_id, location) => {
                        data.main_split.go_to_location(
                            ctx,
                            *editor_view_id,
                            location.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToPosition(kind, position) => {
                        data.main_split.jump_to_position(ctx, kind, *position);
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLocation(kind, location) => {
                        data.main_split.jump_to_location(
                            ctx,
                            kind,
                            location.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLine(kind, line) => {
                        data.main_split.jump_to_line(ctx, kind, *line);
                        ctx.set_handled();
                    }
                    LapceUICommand::GotoDefinition(offset, location) => {
                        if *offset == data.main_split.active_editor().cursor.offset()
                        {
                            data.main_split.jump_to_location(
                                ctx,
                                &EditorKind::SplitActive,
                                location.clone(),
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::GotoReference(offset, location) => {
                        if *offset == data.main_split.active_editor().cursor.offset()
                        {
                            data.main_split.jump_to_location(
                                ctx,
                                &EditorKind::SplitActive,
                                location.clone(),
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateCodeActions(path, rev, offset, resp) => {
                        if let Some(buffer) =
                            data.main_split.open_files.get_mut(path)
                        {
                            if buffer.rev == *rev {
                                Arc::make_mut(buffer)
                                    .code_actions
                                    .insert(*offset, resp.clone());
                            }
                        }
                    }
                    LapceUICommand::PaletteReferences(offset, locations) => {
                        if *offset == data.main_split.active_editor().cursor.offset()
                        {
                            let locations = locations
                                .iter()
                                .map(|l| EditorLocationNew {
                                    path: PathBuf::from(l.uri.path()),
                                    position: l.range.start.clone(),
                                    scroll_offset: None,
                                })
                                .collect();
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::RunPaletteReferences(locations),
                                Target::Widget(data.palette.widget_id),
                            ));
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadBuffer(id, rev, new_content) => {
                        for (_, buffer) in data.main_split.open_files.iter_mut() {
                            if &buffer.id == id {
                                if buffer.rev + 1 == *rev {
                                    let buffer = Arc::make_mut(buffer);
                                    buffer.load_content(new_content);
                                    buffer.rev = *rev;
                                    let path = buffer.path.clone();
                                    data.main_split
                                        .notify_update_text_layouts(ctx, &path);
                                }
                                break;
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSemanticTokens(id, rev, tokens) => {
                        for (_, buffer) in data.main_split.open_files.iter() {
                            if &buffer.id == id {
                                if buffer.rev == *rev {
                                    if let Some(language) = buffer.language.as_ref()
                                    {
                                        data.update_sender.send(
                                            UpdateEvent::SemanticTokens(
                                                BufferUpdate {
                                                    id: buffer.id,
                                                    path: buffer.path.clone(),
                                                    rope: buffer.rope.clone(),
                                                    rev: *rev,
                                                    language: *language,
                                                    highlights: buffer
                                                        .styles
                                                        .clone(),
                                                    semantic_tokens: true,
                                                },
                                                tokens.to_owned(),
                                            ),
                                        );
                                    }
                                }
                            }
                            break;
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowCodeActions
                    | LapceUICommand::CancelCodeActions => {
                        self.code_action.event(ctx, event, data, env);
                    }
                    LapceUICommand::UpdateStyle {
                        id,
                        path,
                        rev,
                        highlights,
                        semantic_tokens,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).update_styles(
                            *rev,
                            highlights.to_owned(),
                            *semantic_tokens,
                        );
                        data.main_split.notify_update_text_layouts(ctx, path);
                        ctx.set_handled();
                    }
                    LapceUICommand::FocusSourceControl => {
                        for (_, panel) in data.panels.iter_mut() {
                            for widget_id in panel.widgets.clone() {
                                if widget_id == data.source_control.widget_id {
                                    let panel = Arc::make_mut(panel);
                                    panel.active = widget_id;
                                    panel.shown = true;
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::Focus,
                                        Target::Widget(widget_id),
                                    ));
                                }
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSyntaxTree {
                        id,
                        path,
                        rev,
                        tree,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer)
                            .update_syntax_tree(*rev, tree.to_owned());
                        ctx.set_handled();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.palette.event(ctx, event, data, env);
        self.completion.event(ctx, event, data, env);
        self.code_action.event(ctx, event, data, env);
        self.main_split.event(ctx, event, data, env);
        self.status.event(ctx, event, data, env);
        for (_, panel) in data.panels.clone().iter() {
            if panel.is_shown() {
                self.panels
                    .get_mut(&panel.active)
                    .unwrap()
                    .event(ctx, event, data, env);
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
        self.palette.lifecycle(ctx, event, data, env);
        self.main_split.lifecycle(ctx, event, data, env);
        self.code_action.lifecycle(ctx, event, data, env);
        self.status.lifecycle(ctx, event, data, env);
        self.completion.lifecycle(ctx, event, data, env);

        for (_, panel) in self.panels.iter_mut() {
            panel.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.completion.status != CompletionStatus::Inactive {
            let old_completion = &old_data.completion;
            let completion = &data.completion;
            let old_editor = old_data.main_split.active_editor();
            let editor = data.main_split.active_editor();
            if old_editor.window_origin != editor.window_origin
                || old_completion.input != completion.input
                || old_completion.request_id != completion.request_id
                || !old_completion
                    .current_items()
                    .same(&completion.current_items())
                || !old_completion
                    .filtered_items
                    .same(&completion.filtered_items)
            {
                let completion_origin = data.completion_origin(ctx.size(), env);
                let rect = completion.size.to_rect().with_origin(completion_origin)
                    + Insets::new(10.0, 10.0, 10.0, 10.0);
                ctx.request_paint_rect(rect);
            }
        }

        if old_data.main_split.show_code_actions || data.main_split.show_code_actions
        {
            let old_editor = old_data.main_split.active_editor();
            let editor = data.main_split.active_editor();
            if data.main_split.show_code_actions
                != old_data.main_split.show_code_actions
                || old_editor.window_origin != editor.window_origin
            {
                let origin = old_data.code_action_origin(ctx.size(), env);
                let rect = old_data
                    .code_action_size(ctx.text(), env)
                    .to_rect()
                    .with_origin(origin)
                    .inset(10.0);
                ctx.request_paint_rect(rect);

                let origin = data.code_action_origin(ctx.size(), env);
                let rect = data
                    .code_action_size(ctx.text(), env)
                    .to_rect()
                    .with_origin(origin)
                    .inset(10.0);
                ctx.request_paint_rect(rect);
            }
        }

        if !old_data.panels.same(&data.panels) {
            ctx.request_layout();
        }

        self.palette.update(ctx, data, env);
        self.main_split.update(ctx, data, env);
        self.completion.update(ctx, data, env);
        self.code_action.update(ctx, data, env);
        self.status.update(ctx, data, env);
        for (_, panel) in data.panels.iter() {
            if panel.is_shown() {
                self.panels
                    .get_mut(&panel.active)
                    .unwrap()
                    .update(ctx, data, env);
            }
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

        let status_size = self.status.layout(ctx, bc, data, env);
        self.status.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, self_size.height - status_size.height),
        );

        let panel_left_top_shown = data
            .panels
            .get(&PanelPosition::LeftTop)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_bottom_shown = data
            .panels
            .get(&PanelPosition::LeftBottom)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_width = if panel_left_top_shown || panel_left_bottom_shown {
            let left_width = data.panel_size.left;
            if panel_left_top_shown && panel_left_bottom_shown {
                let top_height = (self_size.height - status_size.height)
                    * data.panel_size.left_split;
                let bottom_height =
                    self_size.height - status_size.height - top_height;

                let panel_left_top = self
                    .panels
                    .get_mut(
                        &data.panels.get(&PanelPosition::LeftTop).unwrap().active,
                    )
                    .unwrap();
                panel_left_top.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, top_height)),
                    data,
                    env,
                );
                panel_left_top.set_origin(ctx, data, env, Point::ZERO);

                let panel_left_bottom = self
                    .panels
                    .get_mut(
                        &data.panels.get(&PanelPosition::LeftBottom).unwrap().active,
                    )
                    .unwrap();
                panel_left_bottom.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, bottom_height)),
                    data,
                    env,
                );
                panel_left_bottom.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(0.0, top_height),
                );
            } else if panel_left_top_shown {
                let top_height = self_size.height - status_size.height;
                let panel_left_top = self
                    .panels
                    .get_mut(
                        &data.panels.get(&PanelPosition::LeftTop).unwrap().active,
                    )
                    .unwrap();
                panel_left_top.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, top_height)),
                    data,
                    env,
                );
                panel_left_top.set_origin(ctx, data, env, Point::ZERO);
            } else if panel_left_bottom_shown {
                let bottom_height = self_size.height - status_size.height;
                let panel_left_bottom = self
                    .panels
                    .get_mut(
                        &data.panels.get(&PanelPosition::LeftBottom).unwrap().active,
                    )
                    .unwrap();
                panel_left_bottom.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, bottom_height)),
                    data,
                    env,
                );
                panel_left_bottom.set_origin(ctx, data, env, Point::ZERO);
            }
            left_width
        } else {
            0.0
        };

        let main_split_size = Size::new(
            self_size.width - panel_left_width,
            self_size.height - status_size.height,
        );
        let main_split_bc = BoxConstraints::tight(main_split_size);
        self.main_split.layout(ctx, &main_split_bc, data, env);
        self.main_split.set_origin(
            ctx,
            data,
            env,
            Point::new(panel_left_width, 0.0),
        );

        let completion_origin = data.completion_origin(self_size.clone(), env);
        self.completion.layout(ctx, bc, data, env);
        self.completion
            .set_origin(ctx, data, env, completion_origin);

        let code_action_origin = data.code_action_origin(self_size.clone(), env);
        self.code_action.layout(ctx, bc, data, env);
        self.code_action
            .set_origin(ctx, data, env, code_action_origin);

        let palette_size = self.palette.layout(ctx, bc, data, env);
        self.palette.set_origin(
            ctx,
            data,
            env,
            Point::new((self_size.width - palette_size.width) / 2.0, 0.0),
        );

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for (_, panel) in data.panels.iter() {
            if panel.is_shown() {
                self.panels
                    .get_mut(&panel.active)
                    .unwrap()
                    .paint(ctx, data, env);
            }
        }
        self.main_split.paint(ctx, data, env);
        self.status.paint(ctx, data, env);
        self.completion.paint(ctx, data, env);
        self.code_action.paint(ctx, data, env);
        self.palette.paint(ctx, data, env);
    }
}
