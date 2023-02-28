use std::{cmp::Ordering, path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    glazier::Modifiers,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, UntrackedGettableSignal, WriteSignal,
    },
};
use lapce_core::{
    command::{EditCommand, FocusCommand},
    cursor::{Cursor, CursorMode},
    mode::Mode,
    register::Register,
    selection::Selection,
};
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    command::{CommandExecuted, InternalCommand},
    config::LapceConfig,
    doc::Document,
    editor_tab::EditorTabChild,
    id::{EditorId, EditorTabId},
    keypress::KeyPressFocus,
    main_split::{SplitDirection, SplitMoveDirection},
};

#[derive(Clone)]
pub struct EditorData {
    pub editor_tab_id: Option<EditorTabId>,
    pub editor_id: EditorId,
    pub doc: RwSignal<Document>,
    pub cursor: RwSignal<Cursor>,
    register: RwSignal<Register>,
    internal_command: WriteSignal<Option<InternalCommand>>,
    pub viewport: RwSignal<Rect>,
    pub scroll: RwSignal<Vec2>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl EditorData {
    pub fn new(
        cx: AppContext,
        editor_tab_id: EditorTabId,
        editor_id: EditorId,
        doc: RwSignal<Document>,
        register: RwSignal<Register>,
        internal_command: WriteSignal<Option<InternalCommand>>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None);
        let cursor = create_rw_signal(cx.scope, cursor);
        let scroll = create_rw_signal(cx.scope, Vec2::ZERO);
        let viewport = create_rw_signal(cx.scope, Rect::ZERO);
        Self {
            editor_tab_id: Some(editor_tab_id),
            editor_id,
            doc,
            cursor,
            register,
            internal_command,
            viewport,
            scroll,
            config,
        }
    }

    pub fn new_local(
        cx: AppContext,
        editor_id: EditorId,
        register: RwSignal<Register>,
        internal_command: WriteSignal<Option<InternalCommand>>,
        proxy: ProxyRpcHandler,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let doc = Document::new_local(cx, proxy, config);
        let doc = create_rw_signal(cx.scope, doc);
        let cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None);
        let cursor = create_rw_signal(cx.scope, cursor);
        let scroll = create_rw_signal(cx.scope, Vec2::ZERO);
        let viewport = create_rw_signal(cx.scope, Rect::ZERO);
        Self {
            editor_tab_id: None,
            editor_id,
            doc,
            cursor,
            register,
            internal_command,
            viewport,
            scroll,
            config,
        }
    }

    fn run_edit_command(
        &self,
        cx: AppContext,
        cmd: &EditCommand,
    ) -> CommandExecuted {
        let modal = self.config.with_untracked(|config| config.core.modal)
            && !self.doc.with_untracked(|doc| doc.content.is_local());
        let mut cursor = self.cursor.get_untracked();
        self.doc.update(|doc| {
            self.register.update(|register| {
                let yank_data =
                    if let lapce_core::cursor::CursorMode::Visual { .. } =
                        &cursor.mode
                    {
                        Some(cursor.yank(doc.buffer()))
                    } else {
                        None
                    };
                let deltas = doc.do_edit(&mut cursor, cmd, modal, register);
                if !deltas.is_empty() {
                    if let Some(data) = yank_data {
                        register.add_delete(data);
                    }
                }
            });
        });
        self.cursor.set(cursor);
        CommandExecuted::Yes
    }

    fn run_move_command(
        &self,
        cx: AppContext,
        movement: &lapce_core::movement::Movement,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        let mut cursor = self.cursor.get_untracked();
        let config = self.config.get_untracked();
        self.doc.update(|doc| {
            self.register.update(|register| {
                doc.move_cursor(
                    &mut cursor,
                    movement,
                    count.unwrap_or(1),
                    mods.shift(),
                    register,
                    &config,
                );
            });
        });
        self.cursor.set(cursor);
        CommandExecuted::Yes
    }

    fn run_focus_command(
        &self,
        cx: AppContext,
        cmd: &FocusCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match cmd {
            FocusCommand::SplitVertical => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::Split {
                        direction: SplitDirection::Vertical,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitHorizontal => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::Split {
                        direction: SplitDirection::Horizontal,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitRight => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Right,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitLeft => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Left,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitUp => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Up,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitDown => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(InternalCommand::SplitMove {
                        direction: SplitMoveDirection::Down,
                        editor_tab_id,
                    }));
                }
            }
            FocusCommand::SplitExchange => {
                println!("split exchage");
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command
                        .set(Some(InternalCommand::SplitExchange { editor_tab_id }));
                }
            }
            FocusCommand::SplitClose => {
                if let Some(editor_tab_id) = self.editor_tab_id {
                    self.internal_command.set(Some(
                        InternalCommand::EditorTabChildClose {
                            editor_tab_id,
                            child: EditorTabChild::Editor(self.editor_id),
                        },
                    ));
                }
            }
            FocusCommand::PageUp => {
                self.page_move(cx, false, mods);
            }
            FocusCommand::PageDown => {
                self.page_move(cx, true, mods);
            }
            FocusCommand::ScrollUp => {
                self.scroll(cx, false, count.unwrap_or(1), mods);
            }
            FocusCommand::ScrollDown => {
                self.scroll(cx, true, count.unwrap_or(1), mods);
            }
            _ => {}
        }
        CommandExecuted::Yes
    }

    fn page_move(&self, cx: AppContext, down: bool, mods: Modifiers) {
        let config = self.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let lines = (viewport.height() / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.run_move_command(
            cx,
            if down {
                &lapce_core::movement::Movement::Down
            } else {
                &lapce_core::movement::Movement::Up
            },
            Some(lines),
            mods,
        );
        self.scroll
            .set(Vec2::new(0.0, if down { distance } else { -distance }));
    }

    fn scroll(&self, cx: AppContext, down: bool, count: usize, mods: Modifiers) {
        let config = self.config.get_untracked();
        let viewport = self.viewport.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.cursor.with_untracked(|cursor| cursor.offset());
        let (line, _col) = self
            .doc
            .with_untracked(|doc| doc.buffer().offset_to_line_col(offset));
        let top = viewport.y0 + diff;
        let bottom = top + viewport.height();

        let new_line = if (line + 1) as f64 * line_height + line_height > bottom {
            let line = (bottom / line_height).floor() as usize;
            if line > 2 {
                line - 2
            } else {
                0
            }
        } else if line as f64 * line_height - line_height < top {
            let line = (top / line_height).ceil() as usize;
            line + 1
        } else {
            line
        };

        self.scroll.set(Vec2::new(0.0, diff));

        match new_line.cmp(&line) {
            Ordering::Greater => {
                self.run_move_command(
                    cx,
                    &lapce_core::movement::Movement::Down,
                    Some(new_line - line),
                    mods,
                );
            }
            Ordering::Less => {
                self.run_move_command(
                    cx,
                    &lapce_core::movement::Movement::Up,
                    Some(line - new_line),
                    mods,
                );
            }
            _ => (),
        };
    }
}

impl KeyPressFocus for EditorData {
    fn get_mode(&self) -> lapce_core::mode::Mode {
        self.cursor.with_untracked(|c| c.get_mode())
    }

    fn check_condition(
        &self,
        condition: crate::keypress::condition::Condition,
    ) -> bool {
        false
    }

    fn run_command(
        &self,
        cx: AppContext,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            crate::command::CommandKind::Workbench(_) => CommandExecuted::No,
            crate::command::CommandKind::Edit(cmd) => self.run_edit_command(cx, cmd),
            crate::command::CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                self.run_move_command(cx, &movement, count, mods)
            }
            crate::command::CommandKind::Focus(cmd) => {
                self.run_focus_command(cx, cmd, count, mods)
            }
            crate::command::CommandKind::MotionMode(_) => CommandExecuted::No,
            crate::command::CommandKind::MultiSelection(_) => CommandExecuted::No,
        }
    }

    fn receive_char(&self, cx: AppContext, c: &str) {
        if self.get_mode() == Mode::Insert {
            let mut cursor = self.cursor.get_untracked();
            let config = self.config.get_untracked();
            self.doc.update(|doc| {
                let deltas = doc.do_insert(&mut cursor, c, &config);
            });
            self.cursor.set(cursor);
        }
    }
}
