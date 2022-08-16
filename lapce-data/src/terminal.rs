use std::sync::Arc;

use alacritty_terminal::{
    ansi,
    event::EventListener,
    grid::{Dimensions, Scroll},
    index::{Direction, Side},
    selection::{Selection, SelectionType},
    term::{search::RegexSearch, SizeInfo, TermMode},
    vi_mode::ViMotion,
    Term,
};
use druid::{
    keyboard_types::Key, Application, Color, Command, Env, EventCtx, ExtEventSink,
    KeyEvent, Modifiers, Target, WidgetId,
};
use hashbrown::HashMap;
use lapce_core::{
    command::{EditCommand, FocusCommand},
    mode::{Mode, VisualMode},
    movement::{LinePosition, Movement},
};
use lapce_rpc::terminal::TermId;
use parking_lot::Mutex;

use crate::{
    command::{
        CommandExecuted, CommandKind, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::LapceWorkspace,
    find::Find,
    keypress::KeyPressFocus,
    proxy::LapceProxy,
    split::SplitMoveDirection,
};

pub type TermConfig = alacritty_terminal::config::Config;

#[derive(Clone)]
pub struct TerminalSplitData {
    pub active: WidgetId,
    pub active_term_id: TermId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub terminals: im::HashMap<TermId, Arc<LapceTerminalData>>,
    pub indexed_colors: Arc<HashMap<u8, Color>>,
}

impl TerminalSplitData {
    pub fn new(_proxy: Arc<LapceProxy>) -> Self {
        let split_id = WidgetId::next();
        let terminals = im::HashMap::new();

        Self {
            active_term_id: TermId::next(),
            active: WidgetId::next(),
            widget_id: WidgetId::next(),
            split_id,
            terminals,
            indexed_colors: Arc::new(Self::get_indexed_colors()),
        }
    }

    pub fn get_indexed_colors() -> HashMap<u8, Color> {
        let mut indexed_colors = HashMap::new();
        // Build colors.
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    // Override colors 16..232 with the config (if present).
                    let index = 16 + r * 36 + g * 6 + b;
                    let color = Color::rgb8(
                        if r == 0 { 0 } else { r * 40 + 55 },
                        if g == 0 { 0 } else { g * 40 + 55 },
                        if b == 0 { 0 } else { b * 40 + 55 },
                    );
                    indexed_colors.insert(index, color);
                }
            }
        }

        let index: u8 = 232;

        for i in 0..24 {
            // Override colors 232..256 with the config (if present).

            let value = i * 10 + 8;
            indexed_colors.insert(index + i, Color::rgb8(value, value, value));
        }
        indexed_colors
    }

    pub fn get_color(
        &self,
        color: &ansi::Color,
        colors: &alacritty_terminal::term::color::Colors,
        config: &Config,
    ) -> Color {
        match color {
            ansi::Color::Named(color) => self.get_named_color(color, config),
            ansi::Color::Spec(rgb) => Color::rgb8(rgb.r, rgb.g, rgb.b),
            ansi::Color::Indexed(index) => {
                if let Some(rgb) = colors[*index as usize] {
                    return Color::rgb8(rgb.r, rgb.g, rgb.b);
                }
                const NAMED_COLORS: [ansi::NamedColor; 16] = [
                    ansi::NamedColor::Black,
                    ansi::NamedColor::Red,
                    ansi::NamedColor::Green,
                    ansi::NamedColor::Yellow,
                    ansi::NamedColor::Blue,
                    ansi::NamedColor::Magenta,
                    ansi::NamedColor::Cyan,
                    ansi::NamedColor::White,
                    ansi::NamedColor::BrightBlack,
                    ansi::NamedColor::BrightRed,
                    ansi::NamedColor::BrightGreen,
                    ansi::NamedColor::BrightYellow,
                    ansi::NamedColor::BrightBlue,
                    ansi::NamedColor::BrightMagenta,
                    ansi::NamedColor::BrightCyan,
                    ansi::NamedColor::BrightWhite,
                ];
                if (*index as usize) < NAMED_COLORS.len() {
                    self.get_named_color(&NAMED_COLORS[*index as usize], config)
                } else {
                    self.indexed_colors.get(index).cloned().unwrap()
                }
            }
        }
    }

    fn get_named_color(&self, color: &ansi::NamedColor, config: &Config) -> Color {
        let (color, alpha) = match color {
            ansi::NamedColor::Cursor => (LapceTheme::TERMINAL_CURSOR, 1.0),
            ansi::NamedColor::Foreground => (LapceTheme::TERMINAL_FOREGROUND, 1.0),
            ansi::NamedColor::Background => (LapceTheme::TERMINAL_BACKGROUND, 1.0),
            ansi::NamedColor::Blue => (LapceTheme::TERMINAL_BLUE, 1.0),
            ansi::NamedColor::Green => (LapceTheme::TERMINAL_GREEN, 1.0),
            ansi::NamedColor::Yellow => (LapceTheme::TERMINAL_YELLOW, 1.0),
            ansi::NamedColor::Red => (LapceTheme::TERMINAL_RED, 1.0),
            ansi::NamedColor::White => (LapceTheme::TERMINAL_WHITE, 1.0),
            ansi::NamedColor::Black => (LapceTheme::TERMINAL_BLACK, 1.0),
            ansi::NamedColor::Cyan => (LapceTheme::TERMINAL_CYAN, 1.0),
            ansi::NamedColor::Magenta => (LapceTheme::TERMINAL_MAGENTA, 1.0),
            ansi::NamedColor::BrightBlue => (LapceTheme::TERMINAL_BRIGHT_BLUE, 1.0),
            ansi::NamedColor::BrightGreen => {
                (LapceTheme::TERMINAL_BRIGHT_GREEN, 1.0)
            }
            ansi::NamedColor::BrightYellow => {
                (LapceTheme::TERMINAL_BRIGHT_YELLOW, 1.0)
            }
            ansi::NamedColor::BrightRed => (LapceTheme::TERMINAL_BRIGHT_RED, 1.0),
            ansi::NamedColor::BrightWhite => {
                (LapceTheme::TERMINAL_BRIGHT_WHITE, 1.0)
            }
            ansi::NamedColor::BrightBlack => {
                (LapceTheme::TERMINAL_BRIGHT_BLACK, 1.0)
            }
            ansi::NamedColor::BrightCyan => (LapceTheme::TERMINAL_BRIGHT_CYAN, 1.0),
            ansi::NamedColor::BrightMagenta => {
                (LapceTheme::TERMINAL_BRIGHT_MAGENTA, 1.0)
            }
            ansi::NamedColor::BrightForeground => {
                (LapceTheme::TERMINAL_FOREGROUND, 1.0)
            }
            ansi::NamedColor::DimBlack => (LapceTheme::TERMINAL_BLACK, 0.66),
            ansi::NamedColor::DimRed => (LapceTheme::TERMINAL_RED, 0.66),
            ansi::NamedColor::DimGreen => (LapceTheme::TERMINAL_GREEN, 0.66),
            ansi::NamedColor::DimYellow => (LapceTheme::TERMINAL_YELLOW, 0.66),
            ansi::NamedColor::DimBlue => (LapceTheme::TERMINAL_BLUE, 0.66),
            ansi::NamedColor::DimMagenta => (LapceTheme::TERMINAL_MAGENTA, 0.66),
            ansi::NamedColor::DimCyan => (LapceTheme::TERMINAL_CYAN, 0.66),
            ansi::NamedColor::DimWhite => (LapceTheme::TERMINAL_WHITE, 0.66),
            ansi::NamedColor::DimForeground => {
                (LapceTheme::TERMINAL_FOREGROUND, 0.66)
            }
        };
        config.get_color_unchecked(color).clone().with_alpha(alpha)
    }
}

pub struct LapceTerminalViewData {
    pub terminal: Arc<LapceTerminalData>,
    pub config: Arc<Config>,
    pub find: Arc<Find>,
}

impl LapceTerminalViewData {
    fn terminal_mut(&mut self) -> &mut LapceTerminalData {
        Arc::make_mut(&mut self.terminal)
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        if !self.config.lapce.modal {
            return;
        }

        let terminal = self.terminal_mut();
        match terminal.mode {
            Mode::Normal => {
                terminal.mode = Mode::Visual;
                terminal.visual_mode = visual_mode;
            }
            Mode::Visual => {
                if terminal.visual_mode == visual_mode {
                    terminal.mode = Mode::Normal;
                } else {
                    terminal.visual_mode = visual_mode;
                }
            }
            _ => (),
        }

        let mut raw = self.terminal.raw.lock();
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
        self.terminal.toggle_selection(
            term,
            ty,
            point,
            alacritty_terminal::index::Side::Left,
        );
        if let Some(selection) = term.selection.as_mut() {
            selection.include_all();
        }
    }

    pub fn send_keypress(&mut self, key: &KeyEvent) {
        if let Some(command) = LapceTerminalData::resolve_key_event(key) {
            self.terminal
                .proxy
                .proxy_rpc
                .terminal_write(self.terminal.term_id, command.as_ref());
            self.terminal.raw.lock().term.scroll_display(Scroll::Bottom);
        }
    }
}

impl KeyPressFocus for LapceTerminalViewData {
    fn get_mode(&self) -> Mode {
        self.terminal.mode
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "terminal_focus" | "panel_focus")
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        ctx.request_paint();
        match &command.kind {
            CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                let mut raw = self.terminal.raw.lock();
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
                    if !self.config.lapce.modal {
                        return CommandExecuted::Yes;
                    }
                    self.terminal_mut().mode = Mode::Normal;
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    if !term.mode().contains(TermMode::VI) {
                        term.toggle_vi_mode();
                    }
                    self.terminal.clear_selection(term);
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
                    self.terminal_mut().mode = Mode::Terminal;
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    if term.mode().contains(TermMode::VI) {
                        term.toggle_vi_mode();
                    }
                    let scroll = alacritty_terminal::grid::Scroll::Bottom;
                    term.scroll_display(scroll);
                    self.terminal.clear_selection(term);
                }
                EditCommand::ClipboardCopy => {
                    if self.terminal.mode == Mode::Visual {
                        self.terminal_mut().mode = Mode::Normal;
                    }
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    if let Some(content) = term.selection_to_string() {
                        Application::global().clipboard().put_string(content);
                    }
                    self.terminal.clear_selection(term);
                }
                EditCommand::ClipboardPaste => {
                    if let Some(s) = Application::global().clipboard().get_string() {
                        self.receive_char(ctx, &s);
                    }
                }
                _ => return CommandExecuted::No,
            },
            CommandKind::Focus(cmd) => match cmd {
                FocusCommand::PageUp => {
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    let scroll_lines = term.screen_lines() as i32 / 2;
                    term.vi_mode_cursor =
                        term.vi_mode_cursor.scroll(term, scroll_lines);

                    term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                        scroll_lines,
                    ));
                }
                FocusCommand::PageDown => {
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    let scroll_lines = -(term.screen_lines() as i32 / 2);
                    term.vi_mode_cursor =
                        term.vi_mode_cursor.scroll(term, scroll_lines);

                    term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                        scroll_lines,
                    ));
                }
                FocusCommand::SplitVertical => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitTerminal(true, self.terminal.widget_id),
                        Target::Widget(self.terminal.split_id),
                    ));
                }
                FocusCommand::SplitLeft => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Left,
                            self.terminal.widget_id,
                        ),
                        Target::Widget(self.terminal.split_id),
                    ));
                }
                FocusCommand::SplitRight => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Right,
                            self.terminal.widget_id,
                        ),
                        Target::Widget(self.terminal.split_id),
                    ));
                }
                FocusCommand::SplitExchange => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorExchange(self.terminal.widget_id),
                        Target::Widget(self.terminal.split_id),
                    ));
                }
                FocusCommand::SearchForward => {
                    if let Some(search_string) = self.find.search_string.as_ref() {
                        let mut raw = self.terminal.raw.lock();
                        let term = &mut raw.term;
                        self.terminal.search_next(
                            term,
                            search_string,
                            Direction::Right,
                        );
                    }
                }
                FocusCommand::SearchBackward => {
                    if let Some(search_string) = self.find.search_string.as_ref() {
                        let mut raw = self.terminal.raw.lock();
                        let term = &mut raw.term;
                        self.terminal.search_next(
                            term,
                            search_string,
                            Direction::Left,
                        );
                    }
                }
                _ => return CommandExecuted::No,
            },
            _ => return CommandExecuted::No,
        };
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, c: &str) {
        if self.terminal.mode == Mode::Terminal {
            self.terminal
                .proxy
                .proxy_rpc
                .terminal_write(self.terminal.term_id, c);
            self.terminal.raw.lock().term.scroll_display(Scroll::Bottom);
        }
    }
}

pub struct RawTerminal {
    pub parser: ansi::Processor,
    pub term: Term<EventProxy>,
    pub scroll_delta: f64,
}

impl RawTerminal {
    pub fn update_content(&mut self, content: &str) {
        if let Ok(content) = base64::decode(content) {
            for byte in content {
                self.parser.advance(&mut self.term, byte);
            }
        }
    }
}

impl RawTerminal {
    pub fn new(
        term_id: TermId,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
    ) -> Self {
        let config = TermConfig::default();
        let size = SizeInfo::new(50.0, 30.0, 1.0, 1.0, 0.0, 0.0, true);
        let event_proxy = EventProxy {
            proxy,
            event_sink,
            term_id,
        };

        let term = Term::new(&config, size, event_proxy);
        let parser = ansi::Processor::new();

        Self {
            parser,
            term,
            scroll_delta: 0.0,
        }
    }
}

#[derive(Clone)]
pub struct LapceTerminalData {
    pub term_id: TermId,
    pub view_id: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub title: String,
    pub mode: Mode,
    pub visual_mode: VisualMode,
    pub raw: Arc<Mutex<RawTerminal>>,
    pub proxy: Arc<LapceProxy>,
}

impl LapceTerminalData {
    pub fn new(
        workspace: Arc<LapceWorkspace>,
        split_id: WidgetId,
        event_sink: ExtEventSink,
        proxy: Arc<LapceProxy>,
        config: &Config,
    ) -> Self {
        let cwd = workspace.path.as_ref().cloned();
        let widget_id = WidgetId::next();
        let view_id = WidgetId::next();
        let term_id = TermId::next();
        let raw = Arc::new(Mutex::new(RawTerminal::new(
            term_id,
            proxy.clone(),
            event_sink,
        )));

        let local_proxy = proxy.clone();
        let local_raw = raw.clone();
        let shell = config.terminal.shell.clone();
        std::thread::spawn(move || {
            local_proxy.new_terminal(term_id, cwd, shell, local_raw);
        });

        Self {
            term_id,
            widget_id,
            view_id,
            split_id,
            title: "".to_string(),
            mode: Mode::Terminal,
            visual_mode: VisualMode::Normal,
            raw,
            proxy,
        }
    }

    pub fn resize(&self, width: usize, height: usize) {
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);

        let raw = self.raw.clone();
        let proxy = self.proxy.clone();
        let term_id = self.term_id;
        std::thread::spawn(move || {
            raw.lock().term.resize(size);
            proxy.proxy_rpc.terminal_resize(term_id, width, height);
        });
    }

    pub fn wheel_scroll(&self, delta: f64) {
        let mut raw = self.raw.lock();
        let step = 25.0;
        raw.scroll_delta -= delta;
        let delta = (raw.scroll_delta / step) as i32;
        raw.scroll_delta -= delta as f64 * step;
        if delta != 0 {
            let scroll = alacritty_terminal::grid::Scroll::Delta(delta);
            raw.term.scroll_display(scroll);
        }
    }

    pub fn search_next(
        &self,
        term: &mut Term<EventProxy>,
        search_string: &str,
        direction: Direction,
    ) {
        if let Ok(dfas) = RegexSearch::new(search_string) {
            let mut point = term.renderable_content().cursor.point;
            if direction == Direction::Right {
                if point.column.0 < term.last_column() {
                    point.column.0 += 1;
                } else if point.line.0 < term.bottommost_line() {
                    point.column.0 = 0;
                    point.line.0 += 1;
                }
            } else if point.column.0 > 0 {
                point.column.0 -= 1;
            } else if point.line.0 > term.topmost_line() {
                point.column.0 = term.last_column().0;
                point.line.0 -= 1;
            }
            if let Some(m) =
                term.search_next(&dfas, point, direction, Side::Left, None)
            {
                term.vi_goto_point(*m.start());
            }
        }
    }

    pub fn clear_selection(&self, term: &mut Term<EventProxy>) {
        term.selection = None;
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

    pub fn toggle_selection(
        &self,
        term: &mut Term<EventProxy>,
        ty: SelectionType,
        point: alacritty_terminal::index::Point,
        side: alacritty_terminal::index::Side,
    ) {
        match &mut term.selection {
            Some(selection) if selection.ty == ty && !selection.is_empty() => {
                self.clear_selection(term);
            }
            Some(selection) if !selection.is_empty() => {
                selection.ty = ty;
            }
            _ => self.start_selection(term, ty, point, side),
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
                } else {
                    "\x7f" // DEL
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
}

#[derive(Clone)]
pub struct EventProxy {
    pub term_id: TermId,
    pub proxy: Arc<LapceProxy>,
    pub event_sink: ExtEventSink,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        match event {
            alacritty_terminal::event::Event::PtyWrite(s) => {
                self.proxy.proxy_rpc.terminal_write(self.term_id, &s);
            }
            alacritty_terminal::event::Event::Title(title) => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateTerminalTitle(self.term_id, title),
                    Target::Widget(self.proxy.tab_id),
                );
            }
            _ => (),
        }
    }
}

#[cfg(test)]
mod test {
    use druid::{KbKey, KeyEvent, Modifiers};

    use crate::terminal::LapceTerminalData;

    #[test]
    fn test_arrow_without_modifier() {
        assert_eq!(
            Some("\x1b[D"),
            LapceTerminalData::resolve_key_event(&KeyEvent::for_test(
                Modifiers::empty(),
                KbKey::ArrowLeft
            ))
        );
    }

    #[test]
    fn test_arrow_with_one_modifier() {
        assert_eq!(
            Some("\x1b[1;5A"),
            LapceTerminalData::resolve_key_event(&KeyEvent::for_test(
                Modifiers::CONTROL,
                KbKey::ArrowUp
            ))
        );
    }

    #[test]
    fn test_arrow_with_two_modifiers() {
        assert_eq!(
            Some("\x1b[1;6C"),
            LapceTerminalData::resolve_key_event(&KeyEvent::for_test(
                Modifiers::CONTROL | Modifiers::SHIFT,
                KbKey::ArrowRight
            ))
        );
    }
}
