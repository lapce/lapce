use std::{
    cell::RefCell, convert::TryFrom, fmt::Debug, io::Read, ops::Index,
    path::PathBuf, rc::Rc, sync::Arc,
};

use alacritty_terminal::{
    ansi::{self, CursorShape, Handler},
    config::Program,
    event::{EventListener, Notify, OnResize},
    event_loop::{EventLoop, Notifier},
    grid::{Dimensions, Scroll},
    index::{Direction, Side},
    selection::{Selection, SelectionRange, SelectionType},
    sync::FairMutex,
    term::{
        cell::{Cell, Flags},
        search::RegexSearch,
        RenderableCursor, SizeInfo, TermMode,
    },
    tty::{self, EventedReadWrite},
    vi_mode::ViMotion,
    Grid, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use druid::{
    piet::{Text, TextAttribute, TextLayout, TextLayoutBuilder},
    Application, BoxConstraints, Color, Command, Data, Env, Event, EventCtx,
    ExtEventSink, FontFamily, FontWeight, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx,
    Modifiers, PaintCtx, Point, Rect, Region, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use hashbrown::HashMap;
use itertools::Itertools;
use lapce_proxy::terminal::TermId;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use unicode_width::UnicodeWidthChar;

use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{FocusArea, LapceTabData},
    find::Find,
    keypress::KeyPressFocus,
    movement::{LinePosition, Movement},
    palette::{NewPaletteItem, PaletteItem, PaletteItemContent},
    proxy::LapceProxy,
    scroll::LapcePadding,
    split::{LapceSplitNew, SplitMoveDirection},
    state::{Counter, LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
    svg::get_svg,
};

const CTRL_CHARS: &'static [char] = &[
    '@', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '[', '\\', ']', '^', '_',
];

pub type TermConfig = alacritty_terminal::config::Config<HashMap<String, String>>;

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
    config: Arc<Config>,
    find: Arc<Find>,
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
        ctx.request_paint();
        if let Some(movement) = command.move_command(count) {
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
                            term.vi_mode_cursor.point.line = term.bottommost_line();
                        }
                        LinePosition::Line(_) => {}
                    };
                }
                _ => (),
            };
            return;
        }
        match command {
            LapceCommand::NormalMode => {
                if !self.config.lapce.modal {
                    return;
                }
                self.terminal_mut().mode = Mode::Normal;
                let mut raw = self.terminal.raw.lock();
                let term = &mut raw.term;
                if !term.mode().contains(TermMode::VI) {
                    term.toggle_vi_mode();
                }
                self.terminal.clear_selection(term);
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
                let mut raw = self.terminal.raw.lock();
                let term = &mut raw.term;
                if term.mode().contains(TermMode::VI) {
                    term.toggle_vi_mode();
                }
                let scroll = alacritty_terminal::grid::Scroll::Bottom;
                term.scroll_display(scroll);
                self.terminal.clear_selection(term);
            }
            LapceCommand::PageUp => {
                let mut raw = self.terminal.raw.lock();
                let term = &mut raw.term;
                let scroll_lines = term.screen_lines() as i32 / 2;
                term.vi_mode_cursor =
                    term.vi_mode_cursor.scroll(&term, scroll_lines);

                term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                    scroll_lines,
                ));
            }
            LapceCommand::PageDown => {
                let mut raw = self.terminal.raw.lock();
                let term = &mut raw.term;
                let scroll_lines = -(term.screen_lines() as i32 / 2);
                term.vi_mode_cursor =
                    term.vi_mode_cursor.scroll(&term, scroll_lines);

                term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
                    scroll_lines,
                ));
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
            LapceCommand::ClipboardPaste => {
                if let Some(s) = Application::global().clipboard().get_string() {
                    self.receive_char(ctx, &s);
                }
            }
            LapceCommand::SearchForward => {
                if let Some(search_string) = self.find.search_string.as_ref() {
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    self.terminal
                        .search_next(term, search_string, Direction::Right);
                }
            }
            LapceCommand::SearchBackward => {
                if let Some(search_string) = self.find.search_string.as_ref() {
                    let mut raw = self.terminal.raw.lock();
                    let term = &mut raw.term;
                    self.terminal
                        .search_next(term, search_string, Direction::Left);
                }
            }
            _ => (),
        }
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.terminal.mode == Mode::Terminal {
            self.terminal.proxy.terminal_write(self.terminal.term_id, c);
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
            proxy: proxy.clone(),
            event_sink,
            term_id,
        };

        let term = Term::new(&config, size, event_proxy.clone());
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
    pub panel_widget_id: Option<WidgetId>,
    pub mode: Mode,
    pub visual_mode: VisualMode,
    pub raw: Arc<Mutex<RawTerminal>>,
    pub proxy: Arc<LapceProxy>,
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
        let view_id = WidgetId::next();
        let term_id = TermId::next();
        let raw = Arc::new(Mutex::new(RawTerminal::new(
            term_id,
            proxy.clone(),
            event_sink,
        )));
        proxy.new_terminal(term_id, cwd, raw.clone());

        Self {
            term_id,
            widget_id,
            view_id,
            split_id,
            title: "".to_string(),
            panel_widget_id,
            mode: Mode::Terminal,
            visual_mode: VisualMode::Normal,
            raw,
            proxy,
        }
    }

    pub fn resize(&self, width: usize, height: usize) {
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);
        self.raw.lock().term.resize(size);
        self.proxy.terminal_resize(self.term_id, width, height);
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

    fn search_next(
        &self,
        term: &mut Term<EventProxy>,
        search_string: &str,
        direction: Direction,
    ) {
        if let Ok(dfas) = RegexSearch::new(&search_string) {
            let mut point = term.renderable_content().cursor.point;
            if direction == Direction::Right {
                if point.column.0 < term.last_column() {
                    point.column.0 += 1;
                } else {
                    if point.line.0 < term.bottommost_line() {
                        point.column.0 = 0;
                        point.line.0 += 1;
                    }
                }
            } else {
                if point.column.0 > 0 {
                    point.column.0 -= 1;
                } else {
                    if point.line.0 > term.topmost_line() {
                        point.column.0 = term.last_column().0;
                        point.line.0 -= 1;
                    }
                }
            }
            if let Some(m) =
                term.search_next(&dfas, point, direction, Side::Left, None)
            {
                term.vi_goto_point(*m.start());
            }
        }
    }

    fn clear_selection(&self, term: &mut Term<EventProxy>) {
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

    fn toggle_selection(
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

pub struct LapceTerminalView {
    header: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    terminal: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl LapceTerminalView {
    pub fn new(data: &LapceTerminalData) -> Self {
        let header = LapcePadding::new((10.0, 8.0), LapceTerminalHeader::new(data));
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(data));
        Self {
            header: WidgetPod::new(header.boxed()),
            terminal: WidgetPod::new(terminal.boxed()),
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalView {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.header.event(ctx, event, data, env);
        self.terminal.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.lifecycle(ctx, event, data, env);
        self.terminal.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.update(ctx, data, env);
        self.terminal.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);

        if self_size.height > header_size.height {
            let terminal_size =
                Size::new(self_size.width, self_size.height - header_size.height);
            let bc = BoxConstraints::new(Size::ZERO, terminal_size);
            self.terminal.layout(ctx, &bc, data, env);
            self.terminal.set_origin(
                ctx,
                data,
                env,
                Point::new(0.0, header_size.height),
            );
        }

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let shadow_width = 5.0;
        let self_rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.clip(self_rect);
            let rect = self.header.layout_rect();
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
        });

        self.header.paint(ctx, data, env);
        self.terminal.paint(ctx, data, env);
    }
}

pub struct LapceTerminalHeader {
    term_id: TermId,
}

impl LapceTerminalHeader {
    pub fn new(data: &LapceTerminalData) -> Self {
        Self {
            term_id: data.term_id,
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
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
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        Size::new(bc.max().width, data.config.editor.font_size as f64)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let svg = get_svg("terminal.svg").unwrap();
        let width = data.config.editor.font_size as f64;
        let height = data.config.editor.font_size as f64;
        let rect = Size::new(width, height).to_rect();
        ctx.draw_svg(
            &svg,
            rect,
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );

        let term = data.terminal.terminals.get(&self.term_id).unwrap();
        let text_layout = ctx
            .text()
            .new_text_layout(term.title.clone())
            .font(FontFamily::SYSTEM_UI, data.config.editor.font_size as f64)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let y =
            (data.config.editor.font_size as f64 - text_layout.size().height) / 2.0;
        ctx.draw_text(
            &text_layout,
            Point::new(data.config.editor.font_size as f64 + 5.0, y),
        );
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
            config: data.config.clone(),
            find: data.find.clone(),
        };
        match event {
            Event::MouseDown(mouse_event) => {
                self.request_focus(ctx, data);
            }
            Event::Wheel(wheel_event) => {
                data.terminal
                    .terminals
                    .get(&self.term_id)
                    .unwrap()
                    .wheel_scroll(wheel_event.wheel_delta.y);
                ctx.request_paint();
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
                        term_data.receive_char(ctx, &s);
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
            let width = if width > 0.0 {
                (self.width / width).floor() as usize
            } else {
                0
            };
            let height = (self.height / line_height).floor() as usize;
            data.terminal
                .terminals
                .get(&self.term_id)
                .unwrap()
                .resize(width, height);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let char_size = data.config.editor_text_size(ctx.text(), "W");
        let char_width = char_size.width;
        let line_height = data.config.editor.line_height as f64;
        let y_shift = (line_height - char_size.height) / 2.0;

        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
        let raw = terminal.raw.lock();
        let term = &raw.term;
        let content = term.renderable_content();

        if let Some(selection) = content.selection.as_ref() {
            let start_line = selection.start.line.0 + content.display_offset as i32;
            let start_line = if start_line < 0 {
                0
            } else {
                start_line as usize
            };
            let start_col = selection.start.column.0;

            let end_line = selection.end.line.0 + content.display_offset as i32;
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
                        term.last_column().0
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
        } else {
            if terminal.mode != Mode::Terminal {
                let y = (content.cursor.point.line.0 as f64
                    + content.display_offset as f64)
                    * line_height;
                let size = ctx.size();
                ctx.fill(
                    Rect::new(0.0, y, size.width, y + line_height),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
        }

        let cursor_point = &content.cursor.point;

        let term_bg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND)
            .clone();
        let term_fg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_FOREGROUND)
            .clone();
        for item in content.display_iter {
            let point = item.point;
            let cell = item.cell;
            let inverse = cell.flags.contains(Flags::INVERSE);

            let x = point.column.0 as f64 * char_width;
            let y =
                (point.line.0 as f64 + content.display_offset as f64) * line_height;

            let mut bg =
                data.terminal
                    .get_color(&cell.bg, &content.colors, &data.config);
            let mut fg =
                data.terminal
                    .get_color(&cell.fg, &content.colors, &data.config);
            if cell.flags.contains(Flags::DIM)
                || cell.flags.contains(Flags::DIM_BOLD)
            {
                fg = fg.with_alpha(0.66);
            }

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

            if cursor_point == &point {
                let rect = Size::new(
                    char_width * cell.c.width().unwrap_or(1) as f64,
                    line_height,
                )
                .to_rect()
                .with_origin(Point::new(
                    cursor_point.column.0 as f64 * char_width,
                    (cursor_point.line.0 as f64 + content.display_offset as f64)
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

            let bold = cell.flags.contains(Flags::BOLD)
                || cell.flags.contains(Flags::DIM_BOLD);

            if &point == cursor_point && ctx.is_focused() {
                fg = term_bg.clone();
            }

            if cell.c != ' ' && cell.c != '\t' {
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
        if let Some(search_string) = data.find.search_string.as_ref() {
            if let Ok(dfas) = RegexSearch::new(search_string) {
                let mut start = alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(
                        -(content.display_offset as i32),
                    ),
                    alacritty_terminal::index::Column(0),
                );
                let end_line =
                    (start.line + term.screen_lines()).min(term.bottommost_line());
                let mut max_lines = (end_line.0 - start.line.0) as usize;

                while let Some(m) = term.search_next(
                    &dfas,
                    start,
                    Direction::Right,
                    Side::Left,
                    Some(max_lines),
                ) {
                    let match_start = m.start();
                    if match_start.line.0 < start.line.0
                        || (match_start.line.0 == start.line.0
                            && match_start.column.0 < start.column.0)
                    {
                        break;
                    }
                    let x = match_start.column.0 as f64 * char_width;
                    let y = (match_start.line.0 as f64
                        + content.display_offset as f64)
                        * line_height;
                    let rect = Rect::ZERO.with_origin(Point::new(x, y)).with_size(
                        Size::new(
                            (m.end().column.0 - m.start().column.0
                                + term.grid()[*m.end()].c.width().unwrap_or(1))
                                as f64
                                * char_width,
                            line_height,
                        ),
                    );
                    ctx.stroke(
                        rect,
                        data.config
                            .get_color_unchecked(LapceTheme::TERMINAL_FOREGROUND),
                        1.0,
                    );
                    start = *m.end();
                    if start.column.0 < term.last_column() {
                        start.column.0 += 1;
                    } else {
                        if start.line.0 < term.bottommost_line() {
                            start.column.0 = 0;
                            start.line.0 += 1;
                        }
                    }
                    max_lines = (end_line.0 - start.line.0) as usize;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct EventProxy {
    term_id: TermId,
    proxy: Arc<LapceProxy>,
    event_sink: ExtEventSink,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        match event {
            alacritty_terminal::event::Event::PtyWrite(s) => {
                println!("pyt write {}", s);
                self.proxy.terminal_write(self.term_id, &s);
            }
            alacritty_terminal::event::Event::Title(title) => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateTerminalTitle(self.term_id, title),
                    Target::Widget(self.proxy.tab_id),
                );
            }
            _ => (),
        }
    }
}
