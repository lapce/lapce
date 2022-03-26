use std::{iter::Iterator, sync::Arc, time::Instant};

use druid::{
    piet::{PietText, PietTextLayout},
    BoxConstraints, Command, Env, Event, EventCtx, InternalLifeCycle, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point, Rect, Size,
    Target, TextLayout, UpdateCtx, Vec2, Widget, WidgetId,
};
use lapce_data::{
    buffer::{matching_pair_direction, BufferContent, DiffLines, LocalBufferKind},
    command::{
        CommandTarget, LapceCommand, LapceCommandNew, LapceUICommand,
        LapceWorkbenchCommand, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::{LapceTabData, PanelData, PanelKind},
    editor::{EditorLocation, LapceEditorBufferData},
    menu::MenuItem,
    movement::{Movement, Selection},
    panel::PanelPosition,
    state::{Mode, VisualMode},
};
use lapce_rpc::buffer::BufferId;
use lsp_types::{DocumentChanges, TextEdit, Url, WorkspaceEdit};
use strum::EnumMessage;

pub mod container;
pub mod diff_split;
pub mod gutter;
pub mod header;
pub mod tab;
pub mod tab_header;
pub mod tab_header_content;
pub mod view;

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
                editor_data.cancel_completion();
            }
            MouseButton::Right => {
                self.right_click(ctx, editor_data, mouse_event, config);
                editor_data.cancel_completion();
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
            LapceUICommand::ShowMenu(point.round(), Arc::new(menu_items)),
            Target::Auto,
        ));
    }

    pub fn get_size(
        data: &LapceEditorBufferData,
        text: &mut PietText,
        editor_size: Size,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        env: &Env,
    ) -> Size {
        let line_height = data.config.editor.line_height as f64;
        let width = data.config.editor_text_width(text, "W");
        match &data.editor.content {
            BufferContent::File(_) => {
                if data.editor.code_lens {
                    if let Some(syntax) = data.buffer.syntax.as_ref() {
                        let height =
                            syntax.lens.height_of_line(syntax.lens.len() + 1);
                        Size::new(
                            (width * data.buffer.max_len as f64)
                                .max(editor_size.width),
                            (height as f64 - line_height).max(0.0)
                                + editor_size.height,
                        )
                    } else {
                        let height = data.buffer.num_lines
                            * data.config.editor.code_lens_font_size;
                        Size::new(
                            (width * data.buffer.max_len as f64)
                                .max(editor_size.width),
                            (height as f64 - line_height).max(0.0)
                                + editor_size.height,
                        )
                    }
                } else if let Some(compare) = data.editor.compare.as_ref() {
                    let mut lines = 0;
                    if let Some(changes) = data.buffer.history_changes.get(compare) {
                        for change in changes.iter() {
                            match change {
                                DiffLines::Left(l) => lines += l.len(),
                                DiffLines::Both(_l, r) => lines += r.len(),
                                DiffLines::Skip(_l, _r) => lines += 1,
                                DiffLines::Right(r) => lines += r.len(),
                            }
                        }
                    }
                    Size::new(
                        (width * data.buffer.max_len as f64).max(editor_size.width),
                        (line_height * lines as f64 - line_height).max(0.0)
                            + editor_size.height,
                    )
                } else {
                    Size::new(
                        (width * data.buffer.max_len as f64).max(editor_size.width),
                        (line_height * data.buffer.num_lines as f64 - line_height)
                            .max(0.0)
                            + editor_size.height,
                    )
                }
            }
            BufferContent::Local(kind) => match kind {
                LocalBufferKind::FilePicker
                | LocalBufferKind::Search
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => Size::new(
                    editor_size.width.max(width * data.buffer.rope.len() as f64),
                    env.get(LapceTheme::INPUT_LINE_HEIGHT)
                        + env.get(LapceTheme::INPUT_LINE_PADDING) * 2.0,
                ),
                LocalBufferKind::SourceControl => {
                    for (pos, panels) in panels.iter() {
                        for panel_kind in panels.widgets.iter() {
                            if panel_kind == &PanelKind::SourceControl {
                                return match pos {
                                    PanelPosition::BottomLeft
                                    | PanelPosition::BottomRight => {
                                        let width = 200.0;
                                        Size::new(width, editor_size.height)
                                    }
                                    _ => {
                                        let height = 100.0f64;
                                        let height = height.max(
                                            line_height
                                                * data.buffer.num_lines() as f64,
                                        );
                                        Size::new(
                                            (width * data.buffer.max_len as f64)
                                                .max(editor_size.width),
                                            height,
                                        )
                                    }
                                };
                            }
                        }
                    }
                    Size::ZERO
                }
                LocalBufferKind::Empty => editor_size,
            },
            BufferContent::Value(_) => Size::new(
                editor_size.width.max(width * data.buffer.rope.len() as f64),
                env.get(LapceTheme::INPUT_LINE_HEIGHT)
                    + env.get(LapceTheme::INPUT_LINE_PADDING) * 2.0,
            ),
        }
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
        Self::get_size(&editor_data, ctx.text(), bc.max(), &data.panels, env)
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
    match get_workspace_edit_changes_edits(url, workspace_edit) {
        Some(x) => Some(x),
        None => get_workspace_edit_document_changes_edits(url, workspace_edit),
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
