use std::{path::PathBuf, sync::Arc, thread};

use druid::{
    theme, BoxConstraints, Command, Data, Env, Event, EventCtx, Insets, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Size, Target, Widget, WidgetExt,
    WidgetId, WidgetPod,
};

use crate::{
    buffer::{BufferId, BufferNew, BufferState, BufferUpdate, UpdateEvent},
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    completion::{CompletionContainer, CompletionNew, CompletionStatus},
    data::{EditorKind, LapceEditorLens, LapceMainSplitData, LapceTabData},
    editor::{EditorLocationNew, LapceEditorView},
    palette::{NewPalette, PaletteViewLens},
    scroll::LapceScrollNew,
    split::LapceSplitNew,
    state::{LapceWorkspace, LapceWorkspaceType},
};

pub struct LapceTabNew {
    id: WidgetId,
    main_split: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    completion: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    palette: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
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

        Self {
            id: data.id,
            main_split: WidgetPod::new(main_split.boxed()),
            completion: WidgetPod::new(completion.boxed()),
            palette: WidgetPod::new(palette.boxed()),
        }
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
            Event::WindowConnected => {
                for (_, buffer) in data.main_split.open_files.iter() {
                    if !buffer.loaded {
                        buffer.retrieve_file(
                            data.proxy.clone(),
                            ctx.get_external_handle(),
                        );
                    }
                }
                let receiver = data.update_receiver.take().unwrap();
                let event_sink = ctx.get_external_handle();
                let tab_id = self.id;
                thread::spawn(move || {
                    LapceTabData::buffer_update_process(
                        tab_id, receiver, event_sink,
                    );
                });
                data.proxy
                    .start((*data.workspace).clone(), ctx.get_external_handle());
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
                    LapceUICommand::PublishDiagnostics(diagnostics) => {
                        let path = PathBuf::from(diagnostics.uri.path());
                        data.diagnostics
                            .insert(path, Arc::new(diagnostics.diagnostics.clone()));
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
                    _ => (),
                }
            }
            _ => (),
        }
        self.palette.event(ctx, event, data, env);
        self.completion.event(ctx, event, data, env);
        self.main_split.event(ctx, event, data, env);
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
        self.completion.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.completion.status == CompletionStatus::Done {
            let old_completion = &old_data.completion;
            let completion = &data.completion;
            let old_editor = old_data.main_split.active_editor();
            let editor = data.main_split.active_editor();
            if old_editor.window_origin != editor.window_origin
                || old_completion.input != completion.input
                || old_completion.request_id != completion.request_id
                || !old_completion.items.same(&completion.items)
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

        self.palette.update(ctx, data, env);
        self.main_split.update(ctx, data, env);
        self.completion.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        self.main_split.layout(ctx, bc, data, env);
        self.main_split.set_origin(ctx, data, env, Point::ZERO);

        let completion_origin = data.completion_origin(self_size.clone(), env);
        self.completion.layout(ctx, bc, data, env);
        self.completion
            .set_origin(ctx, data, env, completion_origin);

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
        self.main_split.paint(ctx, data, env);
        self.completion.paint(ctx, data, env);
        self.palette.paint(ctx, data, env);
    }
}
