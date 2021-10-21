use std::{
    collections::HashMap, convert::TryFrom, fmt::Debug, io::Read, ops::Index,
    path::PathBuf, sync::Arc,
};

use alacritty_terminal::{
    ansi::{self, CursorShape, Handler},
    config::Program,
    event::{EventListener, Notify, OnResize},
    event_loop::{EventLoop, Notifier},
    grid::{Dimensions, Scroll},
    selection::{Selection, SelectionRange, SelectionType},
    sync::FairMutex,
    term::{
        cell::{Cell, Flags},
        RenderableCursor, SizeInfo, TermMode,
    },
    tty::{self, EventedReadWrite},
    vi_mode::ViMotion,
    Grid, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    Application, BoxConstraints, Color, Command, Data, Env, Event, EventCtx,
    ExtEventSink, FontWeight, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers,
    PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use lapce_proxy::terminal::TermId;
use serde::{Deserialize, Deserializer, Serialize};
use unicode_width::UnicodeWidthChar;

use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{FocusArea, LapceTabData},
    keypress::KeyPressFocus,
    movement::{LinePosition, Movement},
    proxy::LapceProxy,
    scroll::LapcePadding,
    split::{LapceSplitNew, SplitMoveDirection},
    state::{Counter, LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
};

const CTRL_CHARS: &'static [char] = &[
    '@', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '[', '\\', ']', '^', '_',
];

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
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
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

    fn get_color(
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
                const named_colors: [ansi::NamedColor; 16] = [
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
                if (*index as usize) < named_colors.len() {
                    self.get_named_color(&named_colors[*index as usize], config)
                } else {
                    self.indexed_colors.get(index).map(|c| c.clone()).unwrap()
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
    terminal: Arc<LapceTerminalData>,
    term_tx: Sender<(TermId, TerminalEvent)>,
    config: Arc<Config>,
}

impl LapceTerminalViewData {
    fn send_event(&self, event: TerminalEvent) {
        self.term_tx.send((self.terminal.term_id, event));
    }

    fn terminal_mut(&mut self) -> &mut LapceTerminalData {
        Arc::make_mut(&mut self.terminal)
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        if !self.config.lapce.modal {
            return;
        }
        self.send_event(TerminalEvent::ToggleVisual(visual_mode.clone()));
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
    }
}

impl KeyPressFocus for LapceTerminalViewData {
    fn get_mode(&self) -> Mode {
        self.terminal.mode
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "terminal_focus" => true,
            "panel_focus" => self.terminal.panel_widget_id.is_some(),
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
        if let Some(movement) = command.move_command(count) {
            self.send_event(TerminalEvent::ViMovement(movement));
            return;
        }
        match command {
            LapceCommand::NormalMode => {
                if !self.config.lapce.modal {
                    return;
                }
                self.terminal_mut().mode = Mode::Normal;
                self.send_event(TerminalEvent::ViMode);
            }
            LapceCommand::ToggleVisualMode => {
                self.toggle_visual(VisualMode::Normal);
            }
            LapceCommand::ToggleLinewiseVisualMode => {
                self.toggle_visual(VisualMode::Linewise);
            }
            LapceCommand::ToggleBlockwiseVisualMode => {
                self.toggle_visual(VisualMode::Blockwise);
            }
            LapceCommand::InsertMode => {
                self.terminal_mut().mode = Mode::Terminal;
                self.send_event(TerminalEvent::TerminalMode);
            }
            LapceCommand::PageUp => {
                self.send_event(TerminalEvent::ScrollHalfPageUp);
            }
            LapceCommand::PageDown => {
                self.send_event(TerminalEvent::ScrollHalfPageDown);
            }
            LapceCommand::SplitVertical => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitTerminal(
                        true,
                        self.terminal.widget_id,
                        self.terminal.panel_widget_id.clone(),
                    ),
                    Target::Widget(self.terminal.split_id),
                ));
            }
            LapceCommand::SplitLeft => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorMove(
                        SplitMoveDirection::Left,
                        self.terminal.widget_id,
                    ),
                    Target::Widget(self.terminal.split_id),
                ));
            }
            LapceCommand::SplitRight => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorMove(
                        SplitMoveDirection::Right,
                        self.terminal.widget_id,
                    ),
                    Target::Widget(self.terminal.split_id),
                ));
            }
            LapceCommand::SplitExchange => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorExchange(self.terminal.widget_id),
                    Target::Widget(self.terminal.split_id),
                ));
            }
            LapceCommand::ClipboardCopy => {
                self.send_event(TerminalEvent::ClipyboardCopy);
                if self.terminal.mode == Mode::Visual {
                    self.terminal_mut().mode = Mode::Normal;
                }
            }
            LapceCommand::ClipboardPaste => {
                if let Some(s) = Application::global().clipboard().get_string() {
                    self.insert(ctx, &s);
                }
            }
            _ => (),
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        Arc::make_mut(&mut self.terminal).mode = Mode::Terminal;
        self.term_tx.send((
            self.terminal.term_id,
            TerminalEvent::Event(alacritty_terminal::event::Event::PtyWrite(
                c.to_string(),
            )),
        ));
    }
}

#[derive(Clone)]
pub struct LapceTerminalData {
    pub term_id: TermId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub panel_widget_id: Option<WidgetId>,
    pub content: Arc<TerminalContent>,
    pub mode: Mode,
    pub visual_mode: VisualMode,
}

impl LapceTerminalData {
    pub fn new(
        workspace: Option<Arc<LapceWorkspace>>,
        split_id: WidgetId,
        event_sink: ExtEventSink,
        panel_widget_id: Option<WidgetId>,
        proxy: Arc<LapceProxy>,
    ) -> Self {
        let cwd = workspace.map(|w| w.path.clone());
        let widget_id = WidgetId::next();

        let term_id = TermId::next();
        proxy.new_terminal(term_id, cwd);

        Self {
            term_id,
            widget_id,
            split_id,
            panel_widget_id,
            content: Arc::new(TerminalContent::new()),
            mode: Mode::Terminal,
            visual_mode: VisualMode::Normal,
        }
    }
}

pub struct TerminalParser {
    tab_id: WidgetId,
    term_id: TermId,
    term: Term<EventProxy>,
    scroll_delta: f64,
    sender: Sender<TerminalEvent>,
    parser: ansi::Processor,
    event_sink: ExtEventSink,
    proxy: Arc<LapceProxy>,
    indexed_colors: HashMap<u8, Color>,
}

impl TerminalParser {
    pub fn new(
        tab_id: WidgetId,
        term_id: TermId,
        event_sink: ExtEventSink,
        workspace: Option<Arc<LapceWorkspace>>,
        proxy: Arc<LapceProxy>,
    ) -> Sender<TerminalEvent> {
        let mut config = TermConfig::default();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let event_proxy = EventProxy {
            sender: sender.clone(),
        };
        let size = SizeInfo::new(50.0, 30.0, 1.0, 1.0, 0.0, 0.0, true);

        let term = Term::new(&config, size, event_proxy.clone());

        let mut parser = TerminalParser {
            tab_id,
            term_id,
            term,
            scroll_delta: 0.0,
            sender: sender.clone(),
            parser: ansi::Processor::new(),
            event_sink,
            proxy,
            indexed_colors: HashMap::new(),
        };

        std::thread::spawn(move || {
            parser.run(receiver);
        });

        sender
    }

    pub fn fill_cube(&mut self) {
        let mut index: u8 = 16;
        // Build colors.
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    // Override colors 16..232 with the config (if present).
                    self.indexed_colors.insert(
                        index,
                        Color::rgb8(
                            if r == 0 { 0 } else { r * 40 + 55 },
                            if b == 0 { 0 } else { b * 40 + 55 },
                            if g == 0 { 0 } else { g * 40 + 55 },
                        ),
                    );
                    index += 1;
                }
            }
        }
    }

    pub fn fill_gray_ramp(&mut self) {
        let mut index: u8 = 232;

        for i in 0..24 {
            // Override colors 232..256 with the config (if present).

            let value = i * 10 + 8;
            self.indexed_colors
                .insert(index, Color::rgb8(value, value, value));
            index += 1;
        }
    }

    pub fn update_terminal(&self) {
        let renderable_content = self.term.renderable_content();
        let cursor_point = renderable_content.cursor.point;

        let mut cells = Vec::new();
        for cell in renderable_content.display_iter {
            let inverse = cell.flags.contains(Flags::INVERSE);
            let c = cell.cell.c;
            if cursor_point == cell.point
                || (c != ' ' && c != '\t')
                || (cell.bg == ansi::Color::Named(ansi::NamedColor::Background)
                    && inverse)
                || (!inverse
                    && cell.bg != ansi::Color::Named(ansi::NamedColor::Background))
            {
                cells.push((cell.point.clone(), cell.cell.clone()));
            }
        }
        let content = Arc::new(TerminalContent {
            cells,
            cursor_point,
            display_offset: renderable_content.display_offset,
            colors: renderable_content.colors.clone(),
            selection: renderable_content.selection.clone(),
            last_col: self.term.last_column().0 as usize,
        });
        self.event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::TerminalUpdateContent(self.term_id, content),
            Target::Widget(self.tab_id),
        );
    }

    fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    fn start_selection(
        &mut self,
        ty: SelectionType,
        point: alacritty_terminal::index::Point,
        side: alacritty_terminal::index::Side,
    ) {
        self.term.selection = Some(Selection::new(ty, point, side));
    }

    fn toggle_selection(
        &mut self,
        ty: SelectionType,
        point: alacritty_terminal::index::Point,
        side: alacritty_terminal::index::Side,
    ) {
        match &mut self.term.selection {
            Some(selection) if selection.ty == ty && !selection.is_empty() => {
                self.clear_selection();
            }
            Some(selection) if !selection.is_empty() => {
                selection.ty = ty;
            }
            _ => self.start_selection(ty, point, side),
        }
    }

    pub fn run(&mut self, receiver: Receiver<TerminalEvent>) -> Result<()> {
        loop {
            let event = receiver.recv()?;
            match event {
                TerminalEvent::ViMode => {
                    if !self.term.mode().contains(TermMode::VI) {
                        self.term.toggle_vi_mode();
                    }
                    self.clear_selection();
                }
                TerminalEvent::TerminalMode => {
                    if self.term.mode().contains(TermMode::VI) {
                        self.term.toggle_vi_mode();
                    }
                    let scroll = alacritty_terminal::grid::Scroll::Bottom;
                    self.term.scroll_display(scroll);
                    self.clear_selection();
                }
                TerminalEvent::ToggleVisual(visual_mode) => {
                    if !self.term.mode().contains(TermMode::VI) {
                        self.term.toggle_vi_mode();
                    }
                    let ty = match visual_mode {
                        VisualMode::Normal => SelectionType::Simple,
                        VisualMode::Linewise => SelectionType::Lines,
                        VisualMode::Blockwise => SelectionType::Block,
                    };
                    let point = self.term.renderable_content().cursor.point;
                    self.toggle_selection(
                        ty,
                        point,
                        alacritty_terminal::index::Side::Left,
                    );
                    if let Some(selection) = self.term.selection.as_mut() {
                        selection.include_all();
                    }
                }
                TerminalEvent::ClipyboardCopy => {
                    if let Some(content) = self.term.selection_to_string() {
                        self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateClipboard(content),
                            Target::Widget(self.tab_id),
                        );
                    }
                    self.clear_selection();
                }
                TerminalEvent::ViMovement(movement) => {
                    match movement {
                        Movement::Left => {
                            self.term.vi_motion(ViMotion::Left);
                        }
                        Movement::Right => {
                            self.term.vi_motion(ViMotion::Right);
                        }
                        Movement::Up => {
                            self.term.vi_motion(ViMotion::Up);
                        }
                        Movement::Down => {
                            self.term.vi_motion(ViMotion::Down);
                        }
                        Movement::FirstNonBlank => {
                            self.term.vi_motion(ViMotion::FirstOccupied);
                        }
                        Movement::StartOfLine => {
                            self.term.vi_motion(ViMotion::First);
                        }
                        Movement::EndOfLine => {
                            self.term.vi_motion(ViMotion::Last);
                        }
                        Movement::WordForward => {
                            self.term.vi_motion(ViMotion::SemanticRight);
                        }
                        Movement::WordEndForward => {
                            self.term.vi_motion(ViMotion::SemanticRightEnd);
                        }
                        Movement::WordBackward => {
                            self.term.vi_motion(ViMotion::SemanticLeft);
                        }
                        Movement::Line(line) => {
                            match line {
                                LinePosition::First => {
                                    self.term.scroll_display(Scroll::Top);
                                    self.term.vi_mode_cursor.point.line =
                                        self.term.topmost_line();
                                }
                                LinePosition::Last => {
                                    self.term.scroll_display(Scroll::Bottom);
                                    self.term.vi_mode_cursor.point.line =
                                        self.term.bottommost_line();
                                }
                                LinePosition::Line(_) => {}
                            };
                        }
                        _ => (),
                    };
                }
                TerminalEvent::Resize(width, height) => {
                    let size = SizeInfo::new(
                        width as f32,
                        height as f32,
                        1.0,
                        1.0,
                        0.0,
                        0.0,
                        true,
                    );
                    self.term.resize(size);
                    self.proxy.terminal_resize(self.term_id, width, height);
                }
                TerminalEvent::Event(event) => match event {
                    alacritty_terminal::event::Event::MouseCursorDirty => {}
                    alacritty_terminal::event::Event::Title(_) => {}
                    alacritty_terminal::event::Event::ResetTitle => {}
                    alacritty_terminal::event::Event::ClipboardStore(_, _) => {}
                    alacritty_terminal::event::Event::ClipboardLoad(_, _) => {}
                    alacritty_terminal::event::Event::ColorRequest(_, _) => {}
                    alacritty_terminal::event::Event::PtyWrite(s) => {
                        self.proxy.terminal_write(self.term_id, &s);
                        let scroll = alacritty_terminal::grid::Scroll::Bottom;
                        self.term.scroll_display(scroll);
                        if self.term.mode().contains(TermMode::VI) {
                            self.term.toggle_vi_mode();
                        }
                    }
                    alacritty_terminal::event::Event::CursorBlinkingChange(_) => {}
                    alacritty_terminal::event::Event::Wakeup => {}
                    alacritty_terminal::event::Event::Bell => {}
                    alacritty_terminal::event::Event::Exit => {
                        self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::CloseTerminal(self.term_id),
                            Target::Widget(self.tab_id),
                        );
                    }
                },
                TerminalEvent::Scroll(delta) => {
                    let step = 25.0;
                    self.scroll_delta -= delta;
                    let delta = (self.scroll_delta / step) as i32;
                    self.scroll_delta -= delta as f64 * step;
                    if delta != 0 {
                        let scroll = alacritty_terminal::grid::Scroll::Delta(delta);
                        self.term.scroll_display(scroll);
                    }
                }
                TerminalEvent::UpdateContent(content) => {
                    self.update_content(content);
                }
                TerminalEvent::ScrollHalfPageUp => {
                    let scroll_lines = self.term.screen_lines() as i32 / 2;
                    self.term.vi_mode_cursor =
                        self.term.vi_mode_cursor.scroll(&self.term, scroll_lines);

                    self.term.scroll_display(
                        alacritty_terminal::grid::Scroll::Delta(scroll_lines),
                    );
                    self.update_terminal();
                }
                TerminalEvent::ScrollHalfPageDown => {
                    let scroll_lines = -(self.term.screen_lines() as i32 / 2);
                    self.term.vi_mode_cursor =
                        self.term.vi_mode_cursor.scroll(&self.term, scroll_lines);

                    self.term.scroll_display(
                        alacritty_terminal::grid::Scroll::Delta(scroll_lines),
                    );
                }
            }
            self.update_terminal();
        }
    }

    pub fn update_content(&mut self, content: String) {
        if let Ok(content) = base64::decode(content) {
            for byte in content {
                self.parser.advance(&mut self.term, byte);
            }
        }
    }
}

pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let split = LapceSplitNew::new(data.terminal.split_id);
        Self {
            widget_id: data.terminal.widget_id,
            split: WidgetPod::new(split),
        }
    }
}

impl Widget<LapceTabData> for TerminalPanel {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.terminal.terminals.len() == 0 {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::InitTerminalPanel(false),
                Target::Widget(data.terminal.split_id),
            ));
        }
        if !data.terminal.same(&old_data.terminal) {
            ctx.request_paint();
        }
        self.split.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.split.layout(ctx, bc, data, env);
        self.split.set_origin(ctx, data, env, Point::ZERO);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.split.paint(ctx, data, env);
    }
}

pub struct LapceTerminal {
    term_id: TermId,
    widget_id: WidgetId,
    width: f64,
    height: f64,
}

impl LapceTerminal {
    pub fn new(data: &LapceTerminalData) -> Self {
        Self {
            term_id: data.term_id,
            widget_id: data.widget_id,
            width: 0.0,
            height: 0.0,
        }
    }

    pub fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        ctx.request_focus();
        Arc::make_mut(&mut data.terminal).active = self.widget_id;
        Arc::make_mut(&mut data.terminal).active_term_id = self.term_id;
        data.focus = self.widget_id;
        data.focus_area = FocusArea::Terminal;
        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
        if let Some(widget_panel_id) = terminal.panel_widget_id.as_ref() {
            for (pos, panel) in data.panels.iter_mut() {
                if panel.widgets.contains(widget_panel_id) {
                    Arc::make_mut(panel).active = *widget_panel_id;
                    data.panel_active = pos.clone();
                    break;
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceTerminal {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        let old_terminal_data =
            data.terminal.terminals.get(&self.term_id).unwrap().clone();
        let mut term_data = LapceTerminalViewData {
            terminal: old_terminal_data.clone(),
            term_tx: data.term_tx.clone(),
            config: data.config.clone(),
        };
        match event {
            Event::MouseDown(mouse_event) => {
                self.request_focus(ctx, data);
            }
            Event::Wheel(wheel_event) => {
                data.term_tx.send((
                    self.term_id,
                    TerminalEvent::Scroll(wheel_event.wheel_delta.y),
                ));
            }
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                if !Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    &mut term_data,
                    env,
                ) {
                    let s = match &key_event.key {
                        KbKey::Character(c) => {
                            let mut s = "".to_string();
                            let mut mods = key_event.mods.clone();
                            if mods.ctrl() {
                                mods.set(Modifiers::CONTROL, false);
                                if mods.is_empty() && c.chars().count() == 1 {
                                    let c = c.chars().next().unwrap();
                                    if let Some(i) =
                                        CTRL_CHARS.iter().position(|e| &c == e)
                                    {
                                        s = char::from_u32(i as u32)
                                            .unwrap()
                                            .to_string()
                                    }
                                }
                            }

                            s
                        }
                        KbKey::Backspace => "\x08".to_string(),
                        KbKey::Tab => "\x09".to_string(),
                        KbKey::Enter => "\n".to_string(),
                        KbKey::Escape => "\x1b".to_string(),
                        _ => "".to_string(),
                    };
                    if term_data.terminal.mode == Mode::Terminal && s != "" {
                        term_data.insert(ctx, &s);
                    }
                }
                data.keypress = keypress.clone();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        self.request_focus(ctx, data);
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        if !term_data.terminal.same(&old_terminal_data) {
            Arc::make_mut(&mut data.terminal)
                .terminals
                .insert(term_data.terminal.term_id, term_data.terminal.clone());
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        match event {
            LifeCycle::FocusChanged(_) => {
                ctx.request_paint();
            }
            _ => (),
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = bc.max();
        if self.width != size.width || self.height != size.height {
            self.width = size.width;
            self.height = size.height;
            let width = data.config.editor_text_width(ctx.text(), "W");
            let line_height = data.config.editor.line_height as f64;
            let width = (self.width / width).floor() as usize;
            let height = (self.height / line_height).floor() as usize;
            data.term_tx
                .send((self.term_id, TerminalEvent::Resize(width, height)));
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let char_size = data.config.editor_text_size(ctx.text(), "W");
        let char_width = char_size.width;
        let line_height = data.config.editor.line_height as f64;
        let y_shift = (line_height - char_size.height) / 2.0;

        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();

        if let Some(selection) = terminal.content.selection.as_ref() {
            let start_line =
                selection.start.line.0 + terminal.content.display_offset as i32;
            let start_line = if start_line < 0 {
                0
            } else {
                start_line as usize
            };
            let start_col = selection.start.column.0;

            let end_line =
                selection.end.line.0 + terminal.content.display_offset as i32;
            let end_line = if end_line < 0 { 0 } else { end_line as usize };
            let end_col = selection.end.column.0;

            for line in start_line..end_line + 1 {
                let left_col = if selection.is_block {
                    start_col
                } else {
                    if line == start_line {
                        start_col
                    } else {
                        0
                    }
                };
                let right_col = if selection.is_block {
                    end_col + 1
                } else {
                    if line == end_line {
                        end_col + 1
                    } else {
                        terminal.content.last_col
                    }
                };
                let x0 = left_col as f64 * char_width;
                let x1 = right_col as f64 * char_width;
                let y0 = line as f64 * line_height;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );
            }
        }

        let cursor_point = &terminal.content.cursor_point;

        let term_bg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND)
            .clone();
        let term_fg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_FOREGROUND)
            .clone();
        for (point, cell) in &terminal.content.cells {
            if cursor_point == point {
                let rect = Size::new(
                    char_width * cell.c.width().unwrap_or(1) as f64,
                    line_height,
                )
                .to_rect()
                .with_origin(Point::new(
                    cursor_point.column.0 as f64 * char_width,
                    (cursor_point.line.0 as f64
                        + terminal.content.display_offset as f64)
                        * line_height,
                ));
                let cursor_color = if terminal.mode == Mode::Terminal {
                    data.config.get_color_unchecked(LapceTheme::TERMINAL_CURSOR)
                } else {
                    data.config.get_color_unchecked(LapceTheme::EDITOR_CARET)
                };
                if ctx.is_focused() {
                    ctx.fill(rect, cursor_color);
                } else {
                    ctx.stroke(rect, cursor_color, 1.0);
                }
            }

            let x = point.column.0 as f64 * char_width;
            let y = (point.line.0 as f64 + terminal.content.display_offset as f64)
                * line_height;

            let mut bg = data.terminal.get_color(
                &cell.bg,
                &terminal.content.colors,
                &data.config,
            );
            let mut fg = data.terminal.get_color(
                &cell.fg,
                &terminal.content.colors,
                &data.config,
            );
            if cell.flags.contains(Flags::DIM)
                || cell.flags.contains(Flags::DIM_BOLD)
            {
                fg = fg.with_alpha(0.66);
            }

            let inverse = cell.flags.contains(Flags::INVERSE);
            if inverse {
                let fg_clone = fg.clone();
                fg = bg;
                bg = fg_clone;
            }

            if term_bg != bg {
                let rect = Size::new(char_width, line_height)
                    .to_rect()
                    .with_origin(Point::new(x, y));
                ctx.fill(rect, &bg);
            }

            let bold = cell.flags.contains(Flags::BOLD)
                || cell.flags.contains(Flags::DIM_BOLD);

            if point == cursor_point && ctx.is_focused() {
                fg = term_bg.clone();
            }

            let mut builder = ctx
                .text()
                .new_text_layout(cell.c.to_string())
                .font(
                    data.config.editor.font_family(),
                    data.config.editor.font_size as f64,
                )
                .text_color(fg);
            if bold {
                builder = builder
                    .default_attribute(TextAttribute::Weight(FontWeight::BOLD));
            }
            let text_layout = builder.build().unwrap();
            ctx.draw_text(&text_layout, Point::new(x, y + y_shift));
        }
    }
}

pub enum TerminalEvent {
    Resize(usize, usize),
    Event(alacritty_terminal::event::Event),
    UpdateContent(String),
    Scroll(f64),
    ViMode,
    TerminalMode,
    ToggleVisual(VisualMode),
    ViMovement(Movement),
    ScrollHalfPageUp,
    ScrollHalfPageDown,
    ClipyboardCopy,
}

pub enum TerminalHostEvent {
    UpdateContent {
        cursor: RenderableCursor,
        content: Vec<(alacritty_terminal::index::Point, Cell)>,
    },
    Exit,
}

pub struct TerminalContent {
    cells: Vec<(alacritty_terminal::index::Point, Cell)>,
    cursor_point: alacritty_terminal::index::Point,
    display_offset: usize,
    colors: alacritty_terminal::term::color::Colors,
    selection: Option<SelectionRange>,
    last_col: usize,
}

impl Debug for TerminalContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{:?}", self.cells))?;
        f.write_str(&format!("{:?}", self.cursor_point))?;
        f.write_str(&format!("{:?}", self.display_offset))?;
        Ok(())
    }
}

impl TerminalContent {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            cursor_point: alacritty_terminal::index::Point::default(),
            display_offset: 0,
            colors: alacritty_terminal::term::color::Colors::default(),
            selection: None,
            last_col: 0,
        }
    }
}

pub type TermConfig = alacritty_terminal::config::Config<HashMap<String, String>>;

#[derive(Clone)]
pub struct EventProxy {
    sender: Sender<TerminalEvent>,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        self.sender.send(TerminalEvent::Event(event));
    }
}
