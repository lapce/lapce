use std::sync::Arc;

use druid::{
    BoxConstraints, Color, Command, Data, Env, Event, EventCtx, FontDescriptor,
    FontFamily, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext,
    Size, Target, TextLayout, UpdateCtx, Widget,
};
use lsp_types::CodeActionOrCommand;

use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceMainSplitData, LapceTabData},
    keypress::{KeyPressData, KeyPressFocus},
    state::Mode,
    theme::LapceTheme,
};

pub struct CodeAction {}

#[derive(Clone, Data)]
pub struct CodeActionData {
    pub main_split: LapceMainSplitData,
}

impl KeyPressFocus for CodeActionData {
    fn get_mode(&self) -> crate::state::Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "list_focus" => true,
            "code_actions_focus" => true,
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
        match command {
            LapceCommand::CodeActionsCancel => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CancelCodeActions,
                    Target::Auto,
                ));
            }
            _ => {}
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {}
}

impl CodeAction {
    pub fn new() -> Self {
        Self {}
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
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        let old_editor = old_data.main_split.active_editor();
        let editor = data.main_split.active_editor();

        if !old_data.main_split.show_code_actions
            && data.main_split.show_code_actions
        {
            ctx.request_local_layout();
        }

        if editor.window_origin != old_editor.window_origin {
            ctx.request_local_layout();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
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
        let blur_color = Color::grey8(180);
        let shadow_width = 5.0;
        ctx.blurred_rect(rect, shadow_width, &blur_color);
        ctx.fill(rect, &env.get(LapceTheme::EDITOR_SELECTION_COLOR));

        let editor = data.main_split.active_editor();
        let buffer = data.main_split.open_files.get(&editor.buffer).unwrap();
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
                let mut text_layout = TextLayout::<String>::from_text(title.clone());
                text_layout.set_font(
                    FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(14.0),
                );
                text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
                text_layout.rebuild_if_needed(ctx.text(), env);
                text_layout
            })
            .collect();

        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        for (i, text_layout) in action_text_layouts.iter().enumerate() {
            text_layout.draw(ctx, Point::new(5.0, i as f64 * line_height + 5.0));
        }
    }
}
