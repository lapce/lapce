use std::{path::PathBuf, sync::Arc};

use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    selection::{Selection, SelectionType},
    term::{test::TermSize, TermMode},
    vi_mode::ViMotion,
    Term,
};
use floem::{
    glazier::{keyboard_types::Key, KeyEvent, Modifiers},
    reactive::{create_rw_signal, RwSignal, Scope},
};
use lapce_core::{
    command::{EditCommand, FocusCommand},
    mode::{Mode, VisualMode},
    movement::{LinePosition, Movement},
    register::Clipboard,
};
use lapce_rpc::{dap_types::RunDebugConfig, terminal::TermId};
use parking_lot::RwLock;

use super::{
    event::TermEvent,
    raw::{EventProxy, RawTerminal},
};
use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand},
    debug::RunDebugProcess,
    doc::SystemClipboard,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
    workspace::LapceWorkspace,
};

#[derive(Clone)]
pub struct TerminalData {
    pub scope: Scope,
    pub term_id: TermId,
    pub workspace: Arc<LapceWorkspace>,
    pub title: RwSignal<String>,
    pub mode: RwSignal<Mode>,
    pub visual_mode: RwSignal<VisualMode>,
    pub raw: RwSignal<Arc<RwLock<RawTerminal>>>,
    pub run_debug: RwSignal<Option<RunDebugProcess>>,
    pub common: CommonData,
}

impl KeyPressFocus for TerminalData {
    fn get_mode(&self) -> Mode {
        self.mode.get_untracked()
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::TerminalFocus | Condition::PanelFocus)
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        _mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        self.common.view_id.get_untracked().request_paint();
        let config = self.common.config.get_untracked();
        match &command.kind {
            CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                let raw = self.raw.get_untracked();
                let mut raw = raw.write();
                let term = &mut raw.term;
                match movement {
                    Movement::Left => {
                        term.vi_motion(ViMotion::Left);
                    }
                    Movement::Right => {
                        term.vi_motion(ViMotion::Right);
                    }
                    Movement::Up => {
                        term.vi_motion(ViMotion::Up);
                    }
                    Movement::Down => {
                        term.vi_motion(ViMotion::Down);
                    }
                    Movement::FirstNonBlank => {
                        term.vi_motion(ViMotion::FirstOccupied);
                    }
                    Movement::StartOfLine => {
                        term.vi_motion(ViMotion::First);
                    }
                    Movement::EndOfLine => {
                        term.vi_motion(ViMotion::Last);
                    }
                    Movement::WordForward => {
                        term.vi_motion(ViMotion::SemanticRight);
                    }
                    Movement::WordEndForward => {
                        term.vi_motion(ViMotion::SemanticRightEnd);
                    }
                    Movement::WordBackward => {
                        term.vi_motion(ViMotion::SemanticLeft);
                    }
                    Movement::Line(line) => {
                        match line {
                            LinePosition::First => {
                                term.scroll_display(Scroll::Top);
                                term.vi_mode_cursor.point.line = term.topmost_line();
                            }
                            LinePosition::Last => {
                                term.scroll_display(Scroll::Bottom);
                                term.vi_mode_cursor.point.line =
                                    term.bottommost_line();
                            }
                            LinePosition::Line(_) => {}
                        };
                    }
                    _ => (),
                };
            }
            CommandKind::Edit(cmd) => match cmd {
                EditCommand::NormalMode => {
                    if !config.core.modal {
                        return CommandExecuted::Yes;
                    }
                    self.mode.set(Mode::Normal);
                    let raw = self.raw.get_untracked();
                    let mut raw = raw.write();
                    let term = &mut raw.term;
                    if !term.mode().contains(TermMode::VI) {
                        term.toggle_vi_mode();
                    }
                    term.selection = None;
                }
                EditCommand::ToggleVisualMode => {
                    self.toggle_visual(VisualMode::Normal);
                }
                EditCommand::ToggleLinewiseVisualMode => {
                    self.toggle_visual(VisualMode::Linewise);
                }
                EditCommand::ToggleBlockwiseVisualMode => {
                    self.toggle_visual(VisualMode::Blockwise);
                }
                EditCommand::InsertMode => {
                    self.mode.set(Mode::Terminal);
                    let raw = self.raw.get_untracked();
                    let mut raw = raw.write();
                    let term = &mut raw.term;
                    if term.mode().contains(TermMode::VI) {
                        term.toggle_vi_mode();
                    }
                    let scroll = alacritty_terminal::grid::Scroll::Bottom;
                    term.scroll_display(scroll);
                    term.selection = None;
                }
                EditCommand::ClipboardCopy => {
                    let mut clipboard = SystemClipboard {};
                    if self.mode.get_untracked() == Mode::Visual {
                        self.mode.set(Mode::Normal);
                    }
                    let raw = self.raw.get_untracked();
                    let mut raw = raw.write();
                    let term = &mut raw.term;
                    if let Some(content) = term.selection_to_string() {
                        clipboard.put_string(content);
                    }
                    if self.mode.get_untracked() != Mode::Terminal {
                        term.selection = None;
                    }
                }
                EditCommand::ClipboardPaste => {
                    let clipboard = SystemClipboard {};
                    let mut check_bracketed_paste: bool = false;
                    if self.mode.get_untracked() == Mode::Terminal {
                        let raw = self.raw.get_untracked();
                        let mut raw = raw.write();
                        let term = &mut raw.term;
                        term.selection = None;
                        if term.mode().contains(TermMode::BRACKETED_PASTE) {
                            check_bracketed_paste = true;
                        }
                    }
                    if let Some(s) = clipboard.get_string() {
                        if check_bracketed_paste {
                            self.receive_char("\x1b[200~");
                            self.receive_char(&s.replace('\x1b', ""));
                            self.receive_char("\x1b[201~");
                        } else {
                            self.receive_char(&s);
                        }
                    }
                }
                _ => return CommandExecuted::No,
            },
            CommandKind::Focus(cmd) => match cmd {
                FocusCommand::PageUp => {
                    let raw = self.raw.get_untracked();
                    let mut raw = raw.write();
                    let term = &mut raw.term;
                    let scroll_lines = term.screen_lines() as i32 / 2;
                    term.vi_mode_cursor =
                        term.vi_mode_cursor.scroll(term, scroll_lines);

                    term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                        scroll_lines,
                    ));
                }
                FocusCommand::PageDown => {
                    let raw = self.raw.get_untracked();
                    let mut raw = raw.write();
                    let term = &mut raw.term;
                    let scroll_lines = -(term.screen_lines() as i32 / 2);
                    term.vi_mode_cursor =
                        term.vi_mode_cursor.scroll(term, scroll_lines);

                    term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                        scroll_lines,
                    ));
                }
                FocusCommand::SplitVertical => {
                    self.common.internal_command.send(
                        InternalCommand::SplitTerminal {
                            term_id: self.term_id,
                        },
                    );
                }
                FocusCommand::SplitHorizontal => {
                    self.common.internal_command.send(
                        InternalCommand::SplitTerminal {
                            term_id: self.term_id,
                        },
                    );
                }
                FocusCommand::SplitLeft => {
                    self.common.internal_command.send(
                        InternalCommand::SplitTerminalPrevious {
                            term_id: self.term_id,
                        },
                    );
                }
                FocusCommand::SplitRight => {
                    self.common.internal_command.send(
                        InternalCommand::SplitTerminalNext {
                            term_id: self.term_id,
                        },
                    );
                }
                FocusCommand::SplitExchange => {
                    self.common.internal_command.send(
                        InternalCommand::SplitTerminalExchange {
                            term_id: self.term_id,
                        },
                    );
                }
                FocusCommand::SearchForward => {
                    // if let Some(search_string) = self.find.search_string.as_ref() {
                    //     let mut raw = self.terminal.raw.lock();
                    //     let term = &mut raw.term;
                    //     self.terminal.search_next(
                    //         term,
                    //         search_string,
                    //         Direction::Right,
                    //     );
                    // }
                }
                FocusCommand::SearchBackward => {
                    // if let Some(search_string) = self.find.search_string.as_ref() {
                    //     let mut raw = self.terminal.raw.lock();
                    //     let term = &mut raw.term;
                    //     self.terminal.search_next(
                    //         term,
                    //         search_string,
                    //         Direction::Left,
                    //     );
                    // }
                }
                _ => return CommandExecuted::No,
            },
            _ => return CommandExecuted::No,
        };
        CommandExecuted::Yes
    }

    fn receive_char(&self, c: &str) {
        if self.mode.get_untracked() == Mode::Terminal {
            self.common
                .proxy
                .terminal_write(self.term_id, c.to_string());
            self.raw
                .get_untracked()
                .write()
                .term
                .scroll_display(Scroll::Bottom);
        }
    }
}

impl TerminalData {
    pub fn new(
        cx: Scope,
        workspace: Arc<LapceWorkspace>,
        run_debug: Option<RunDebugProcess>,
        common: CommonData,
    ) -> Self {
        let (cx, _) = cx.run_child_scope(|cx| cx);
        let term_id = TermId::next();

        let title = cx.create_rw_signal("title".to_string());

        let raw = Self::new_raw_terminal(
            workspace.clone(),
            term_id,
            run_debug.as_ref().map(|r| &r.config),
            common.clone(),
        );

        let run_debug = cx.create_rw_signal(run_debug);
        let mode = cx.create_rw_signal(Mode::Terminal);
        let visual_mode = cx.create_rw_signal(VisualMode::Normal);
        let raw = cx.create_rw_signal(raw);

        Self {
            scope: cx,
            term_id,
            workspace,
            raw,
            title,
            run_debug,
            mode,
            visual_mode,
            common,
        }
    }

    pub fn new_raw_terminal(
        workspace: Arc<LapceWorkspace>,
        term_id: TermId,
        run_debug: Option<&RunDebugConfig>,
        common: CommonData,
    ) -> Arc<RwLock<RawTerminal>> {
        let raw = Arc::new(RwLock::new(RawTerminal::new(
            term_id,
            common.proxy.clone(),
            common.term_notification_tx.clone(),
        )));

        let mut cwd = workspace.path.as_ref().cloned();
        let mut env = None;
        let shell = if let Some(run_debug) = run_debug {
            if let Some(path) = run_debug.cwd.as_ref() {
                cwd = Some(PathBuf::from(path));
                if path.contains("${workspace}") {
                    if let Some(workspace) = workspace
                        .path
                        .as_ref()
                        .and_then(|workspace| workspace.to_str())
                    {
                        cwd = Some(PathBuf::from(
                            &path.replace("${workspace}", workspace),
                        ));
                    }
                }
            }

            env = run_debug.env.clone();

            if let Some(debug_command) = run_debug.debug_command.as_ref() {
                debug_command.clone()
            } else {
                format!("{} {}", run_debug.program, run_debug.args.join(" "))
            }
        } else {
            common.config.get_untracked().terminal.shell.clone()
        };

        {
            let raw = raw.clone();
            let _ = common.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
            common.proxy.new_terminal(term_id, cwd, env, shell);
        }
        raw
    }

    pub fn send_keypress(&self, key: &KeyEvent) {
        if let Some(command) = Self::resolve_key_event(key) {
            self.receive_char(command);
        }
    }

    pub fn resolve_key_event(key: &KeyEvent) -> Option<&str> {
        let mut key = key.clone();
        key.mods = (Modifiers::ALT
            | Modifiers::CONTROL
            | Modifiers::SHIFT
            | Modifiers::META)
            & key.mods;

        // Generates a `Modifiers` value to check against.
        macro_rules! modifiers {
            (ctrl) => {
                Modifiers::CONTROL
            };

            (alt) => {
                Modifiers::ALT
            };

            (shift) => {
                Modifiers::SHIFT
            };

            ($mod:ident $(| $($mods:ident)|+)?) => {
                modifiers!($mod) $(| modifiers!($($mods)|+) )?
            };
        }

        // Generates modifier values for ANSI sequences.
        macro_rules! modval {
            (shift) => {
                // 1
                "2"
            };
            (alt) => {
                // 2
                "3"
            };
            (alt | shift) => {
                // 1 + 2
                "4"
            };
            (ctrl) => {
                // 4
                "5"
            };
            (ctrl | shift) => {
                // 1 + 4
                "6"
            };
            (alt | ctrl) => {
                // 2 + 4
                "7"
            };
            (alt | ctrl | shift) => {
                // 1 + 2 + 4
                "8"
            };
        }

        // Generates ANSI sequences to move the cursor by one position.
        macro_rules! term_sequence {
            // Generate every modifier combination (except meta)
            ([all], $evt:ident, $no_mod:literal, $pre:literal, $post:literal) => {
                {
                    term_sequence!([], $evt, $no_mod);
                    term_sequence!([shift, alt, ctrl], $evt, $pre, $post);
                    term_sequence!([alt | shift, ctrl | shift, alt | ctrl], $evt, $pre, $post);
                    term_sequence!([alt | ctrl | shift], $evt, $pre, $post);
                    return None;
                }
            };
            // No modifiers
            ([], $evt:ident, $no_mod:literal) => {
                if $evt.mods.is_empty() {
                    return Some($no_mod);
                }
            };
            // A single modifier combination
            ([$($mod:ident)|+], $evt:ident, $pre:literal, $post:literal) => {
                if $evt.mods == modifiers!($($mod)|+) {
                    return Some(concat!($pre, modval!($($mod)|+), $post));
                }
            };
            // Break down multiple modifiers into a series of single combination branches
            ([$($($mod:ident)|+),+], $evt:ident, $pre:literal, $post:literal) => {
                $(
                    term_sequence!([$($mod)|+], $evt, $pre, $post);
                )+
            };
        }

        match key.key {
            Key::Character(ref c) => {
                if key.mods == Modifiers::CONTROL {
                    // Convert the character into its index (into a control character).
                    // In essence, this turns `ctrl+h` into `^h`
                    let str = match c.as_str() {
                        "@" => "\x00",
                        "a" => "\x01",
                        "b" => "\x02",
                        "c" => "\x03",
                        "d" => "\x04",
                        "e" => "\x05",
                        "f" => "\x06",
                        "g" => "\x07",
                        "h" => "\x08",
                        "i" => "\x09",
                        "j" => "\x0a",
                        "k" => "\x0b",
                        "l" => "\x0c",
                        "m" => "\x0d",
                        "n" => "\x0e",
                        "o" => "\x0f",
                        "p" => "\x10",
                        "q" => "\x11",
                        "r" => "\x12",
                        "s" => "\x13",
                        "t" => "\x14",
                        "u" => "\x15",
                        "v" => "\x16",
                        "w" => "\x17",
                        "x" => "\x18",
                        "y" => "\x19",
                        "z" => "\x1a",
                        "[" => "\x1b",
                        "\\" => "\x1c",
                        "]" => "\x1d",
                        "^" => "\x1e",
                        "_" => "\x1f",
                        _ => return None,
                    };

                    Some(str)
                } else {
                    None
                }
            }
            Key::Backspace => {
                Some(if key.mods.ctrl() {
                    "\x08" // backspace
                } else if key.mods.alt() {
                    "\x1b\x7f"
                } else {
                    "\x7f"
                })
            }

            Key::Tab => Some("\x09"),
            Key::Enter => Some("\r"),
            Key::Escape => Some("\x1b"),

            // The following either expands to `\x1b[X` or `\x1b[1;NX` where N is a modifier value
            Key::ArrowUp => term_sequence!([all], key, "\x1b[A", "\x1b[1;", "A"),
            Key::ArrowDown => term_sequence!([all], key, "\x1b[B", "\x1b[1;", "B"),
            Key::ArrowRight => term_sequence!([all], key, "\x1b[C", "\x1b[1;", "C"),
            Key::ArrowLeft => term_sequence!([all], key, "\x1b[D", "\x1b[1;", "D"),
            Key::Home => term_sequence!([all], key, "\x1bOH", "\x1b[1;", "H"),
            Key::End => term_sequence!([all], key, "\x1bOF", "\x1b[1;", "F"),
            Key::Insert => term_sequence!([all], key, "\x1b[2~", "\x1b[2;", "~"),
            Key::Delete => term_sequence!([all], key, "\x1b[3~", "\x1b[3;", "~"),
            _ => None,
        }
    }

    pub fn wheel_scroll(&self, delta: f64) {
        let config = self.common.config.get_untracked();
        let step = config.terminal_line_height() as f64;
        let raw = self.raw.get_untracked();
        let mut raw = raw.write();
        raw.scroll_delta -= delta;
        let delta = (raw.scroll_delta / step) as i32;
        raw.scroll_delta -= delta as f64 * step;
        if delta != 0 {
            let scroll = alacritty_terminal::grid::Scroll::Delta(delta);
            raw.term.scroll_display(scroll);
        }
    }

    fn toggle_visual(&self, visual_mode: VisualMode) {
        let config = self.common.config.get_untracked();
        if !config.core.modal {
            return;
        }

        match self.mode.get_untracked() {
            Mode::Normal => {
                self.mode.set(Mode::Visual);
                self.visual_mode.set(visual_mode);
            }
            Mode::Visual => {
                if self.visual_mode.get_untracked() == visual_mode {
                    self.mode.set(Mode::Normal);
                } else {
                    self.visual_mode.set(visual_mode);
                }
            }
            _ => (),
        }

        let raw = self.raw.get_untracked();
        let mut raw = raw.write();
        let term = &mut raw.term;
        if !term.mode().contains(TermMode::VI) {
            term.toggle_vi_mode();
        }
        let ty = match visual_mode {
            VisualMode::Normal => SelectionType::Simple,
            VisualMode::Linewise => SelectionType::Lines,
            VisualMode::Blockwise => SelectionType::Block,
        };
        let point = term.renderable_content().cursor.point;
        self.toggle_selection(
            term,
            ty,
            point,
            alacritty_terminal::index::Side::Left,
        );
        if let Some(selection) = term.selection.as_mut() {
            selection.include_all();
        }
    }

    pub fn toggle_selection(
        &self,
        term: &mut Term<EventProxy>,
        ty: SelectionType,
        point: alacritty_terminal::index::Point,
        side: alacritty_terminal::index::Side,
    ) {
        match &mut term.selection {
            Some(selection) if selection.ty == ty && !selection.is_empty() => {
                term.selection = None;
            }
            Some(selection) if !selection.is_empty() => {
                selection.ty = ty;
            }
            _ => self.start_selection(term, ty, point, side),
        }
    }

    fn start_selection(
        &self,
        term: &mut Term<EventProxy>,
        ty: SelectionType,
        point: alacritty_terminal::index::Point,
        side: alacritty_terminal::index::Side,
    ) {
        term.selection = Some(Selection::new(ty, point, side));
    }

    pub fn new_process(&self, run_debug: Option<RunDebugProcess>) {
        let (width, height) = {
            let raw = self.raw.get_untracked();
            let raw = raw.read();
            let width = raw.term.columns();
            let height = raw.term.screen_lines();
            (width, height)
        };

        let raw = Self::new_raw_terminal(
            self.workspace.clone(),
            self.term_id,
            run_debug.as_ref().map(|r| &r.config),
            self.common.clone(),
        );

        self.raw.set(raw);
        self.run_debug.set(run_debug);

        let term_size = TermSize::new(width, height);
        self.raw.get_untracked().write().term.resize(term_size);
        self.common
            .proxy
            .terminal_resize(self.term_id, width, height);
    }
}
