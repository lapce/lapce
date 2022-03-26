use std::{collections::HashMap, sync::Arc};

use druid::{
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontDescriptor, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TextLayout, UpdateCtx, Widget,
};
use lapce_data::{
    buffer::{BufferContent, EditType},
    command::{CommandExecuted, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{LapceMainSplitData, LapceTabData},
    keypress::KeyPressFocus,
    movement::{Movement, Selection},
    proxy::LapceProxy,
    state::Mode,
};
use lsp_types::{
    CodeActionOrCommand, DocumentChangeOperation, DocumentChanges, OneOf, TextEdit,
    Url, WorkspaceEdit,
};

pub struct CodeAction {}

#[derive(Clone, Data)]
pub struct CodeActionData {
    pub main_split: LapceMainSplitData,
    pub proxy: Arc<LapceProxy>,
    pub config: Arc<Config>,
}

impl KeyPressFocus for CodeActionData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(
            condition,
            "list_focus" | "code_actions_focus" | "modal_focus"
        )
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        match command {
            LapceCommand::ModalClose => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CancelCodeActions,
                    Target::Auto,
                ));
            }
            LapceCommand::ListNext => {
                self.next(ctx);
            }
            LapceCommand::ListPrevious => {
                self.previous(ctx);
            }
            LapceCommand::ListSelect => {
                self.select();
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CancelCodeActions,
                    Target::Auto,
                ));
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
}

impl CodeActionData {
    pub fn next(&mut self, _ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };
        if let BufferContent::File(path) = &editor.content {
            let buffer = self.main_split.open_files.get(path).unwrap();
            let offset = editor.cursor.offset();
            let prev_offset = buffer.prev_code_boundary(offset);
            let empty_vec = Vec::new();
            let code_actions =
                buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

            self.main_split.current_code_actions = Movement::Down.update_index(
                self.main_split.current_code_actions,
                code_actions.len(),
                1,
                true,
            );
        }
    }

    pub fn select(&mut self) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };
        if let BufferContent::File(path) = &editor.content {
            let buffer = self.main_split.open_files.get(path).unwrap();
            let offset = editor.cursor.offset();
            let prev_offset = buffer.prev_code_boundary(offset);
            let empty_vec = Vec::new();
            let code_actions =
                buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

            let action = match code_actions.get(self.main_split.current_code_actions)
            {
                Some(action) => action,
                None => return,
            };

            match action {
                CodeActionOrCommand::Command(_cmd) => {}
                CodeActionOrCommand::CodeAction(action) => {
                    if let Some(edit) = action.edit.as_ref() {
                        if let Some(edits) = workspce_edits(edit) {
                            if let Some(edits) =
                                edits.get(&Url::from_file_path(&path).unwrap())
                            {
                                let path = path.clone();
                                let buffer = self
                                    .main_split
                                    .open_files
                                    .get_mut(&path)
                                    .unwrap();
                                let edits: Vec<(Selection, String)> = edits
                                    .iter()
                                    .map(|edit| {
                                        let selection = Selection::region(
                                            buffer.offset_of_position(
                                                &edit.range.start,
                                                self.config.editor.tab_width,
                                            ),
                                            buffer.offset_of_position(
                                                &edit.range.end,
                                                self.config.editor.tab_width,
                                            ),
                                        );
                                        (selection, edit.new_text.clone())
                                    })
                                    .collect();
                                self.main_split.edit(
                                    &path,
                                    &edits
                                        .iter()
                                        .map(|(s, c)| (s, c.as_str()))
                                        .collect::<Vec<(&Selection, &str)>>(),
                                    EditType::Other,
                                    &self.config,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[allow(unused_variables)]
    pub fn previous(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };
        if let BufferContent::File(path) = &editor.content {
            let buffer = self.main_split.open_files.get(path).unwrap();
            let offset = editor.cursor.offset();
            let prev_offset = buffer.prev_code_boundary(offset);
            let empty_vec = Vec::new();
            let code_actions =
                buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

            self.main_split.current_code_actions = Movement::Up.update_index(
                self.main_split.current_code_actions,
                code_actions.len(),
                1,
                true,
            );
        }
    }
}

impl CodeAction {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for CodeAction {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for CodeAction {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                let mut code_action_data = CodeActionData {
                    main_split: data.main_split.clone(),
                    proxy: data.proxy.clone(),
                    config: data.config.clone(),
                };
                mut_keypress.key_down(ctx, key_event, &mut code_action_data, env);
                data.keypress = keypress;
                data.main_split = code_action_data.main_split.clone();
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::ShowCodeActions => {
                        data.main_split.show_code_actions = true;
                        data.main_split.current_code_actions = 0;
                        ctx.request_focus();
                        ctx.set_handled();
                    }
                    LapceUICommand::CancelCodeActions => {
                        data.main_split.show_code_actions = false;
                        ctx.resign_focus();
                        ctx.set_handled();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::FocusChanged(false) = event {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::CancelCodeActions,
                Target::Auto,
            ));
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        let old_editor = old_data.main_split.active_editor();
        let old_editor = match old_editor {
            Some(editor) => editor,
            None => return,
        };
        let editor = data.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        if !old_data.main_split.show_code_actions
            && data.main_split.show_code_actions
        {
            ctx.request_local_layout();
        }

        if editor.window_origin != old_editor.window_origin {
            ctx.request_local_layout();
        }

        if old_data.main_split.current_code_actions
            != data.main_split.current_code_actions
        {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        data.code_action_size(ctx.text(), env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !data.main_split.show_code_actions {
            return;
        }

        let rect = ctx.size().to_rect();
        let shadow_width = 5.0;
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
        );

        let editor = data.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        if let BufferContent::File(path) = &editor.content {
            let buffer = data.main_split.open_files.get(path).unwrap();
            let offset = editor.cursor.offset();
            let prev_offset = buffer.prev_code_boundary(offset);
            let empty_vec = Vec::new();
            let code_actions =
                buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

            let action_text_layouts: Vec<TextLayout<String>> = code_actions
                .iter()
                .map(|code_action| {
                    let title = match code_action {
                        CodeActionOrCommand::Command(cmd) => cmd.title.to_string(),
                        CodeActionOrCommand::CodeAction(action) => {
                            action.title.to_string()
                        }
                    };
                    let mut text_layout = TextLayout::<String>::from_text(title);
                    text_layout.set_font(
                        FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(14.0),
                    );
                    text_layout.set_text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    );
                    text_layout.rebuild_if_needed(ctx.text(), env);
                    text_layout
                })
                .collect();

            let line_height = data.config.editor.line_height as f64;

            let line_rect = Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    data.main_split.current_code_actions as f64 * line_height,
                ))
                .with_size(Size::new(ctx.size().width, line_height));
            ctx.fill(
                line_rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );

            for (i, text_layout) in action_text_layouts.iter().enumerate() {
                text_layout.draw(ctx, Point::new(5.0, i as f64 * line_height + 5.0));
            }
        }
    }
}

fn workspce_edits(edit: &WorkspaceEdit) -> Option<HashMap<Url, Vec<TextEdit>>> {
    if let Some(changes) = edit.changes.as_ref() {
        return Some(changes.clone());
    }

    let changes = edit.document_changes.as_ref()?;
    let edits = match changes {
        DocumentChanges::Edits(edits) => edits
            .iter()
            .map(|e| {
                (
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
        DocumentChanges::Operations(ops) => ops
            .iter()
            .filter_map(|o| match o {
                DocumentChangeOperation::Op(_op) => None,
                DocumentChangeOperation::Edit(e) => Some((
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )),
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
    };
    Some(edits)
}
