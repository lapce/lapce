use std::{collections::HashMap, sync::Arc};

use druid::{Command, Data, Env, EventCtx, Modifiers, Target};
use lsp_types::{
    CodeActionOrCommand, DocumentChangeOperation, DocumentChanges, OneOf, TextEdit,
    Url, WorkspaceEdit,
};

use crate::{
    buffer::{BufferContent, EditType},
    command::{CommandExecuted, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::Config,
    data::LapceMainSplitData,
    keypress::KeyPressFocus,
    movement::{Movement, Selection},
    proxy::LapceProxy,
    state::Mode,
};

#[derive(Clone, Data)]
pub struct CodeActionData {
    pub main_split: LapceMainSplitData,
    pub proxy: Arc<LapceProxy>,
    pub config: Arc<Config>,
}

impl KeyPressFocus for CodeActionData {
    fn get_mode(&self) -> crate::state::Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "list_focus" | "code_actions_focus")
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
            LapceCommand::CodeActionsCancel => {
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
                self.select(ctx);
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

    pub fn select(&mut self, ctx: &mut EventCtx) {
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

            let action = &code_actions[self.main_split.current_code_actions];
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
                                    ctx,
                                    &path,
                                    edits
                                        .iter()
                                        .map(|(s, c)| (s, c.as_ref()))
                                        .collect(),
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
