use std::{
    collections::HashMap, convert::TryFrom, fmt::Debug, io::Read, ops::Index,
    path::PathBuf, sync::Arc,
};

use alacritty_terminal::{
    ansi::{self, CursorShape, Handler},
    config::Program,
    event::{EventListener, Notify, OnResize},
    event_loop::{EventLoop, Notifier},
    sync::FairMutex,
    term::{
        cell::{Cell, Flags},
        RenderableCursor, SizeInfo,
    },
    tty::{self, EventedReadWrite},
    Grid, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    BoxConstraints, Color, Command, Data, Env, Event, EventCtx, ExtEventSink,
    FontWeight, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx,
    Point, RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use lapce_proxy::terminal::TermId;
use serde::{Deserialize, Deserializer, Serialize};
use unicode_width::UnicodeWidthChar;

use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{FocusArea, LapceTabData},
    keypress::KeyPressFocus,
    proxy::LapceProxy,
    scroll::LapcePadding,
    split::{LapceSplitNew, SplitMoveDirection},
    state::{Counter, LapceWorkspace, LapceWorkspaceType, Mode},
};

const CTRL_CHARS: &'static [char] = &[
    '@', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '[', '\\', ']', '^', '_',
];

#[derive(Clone)]
pub struct TerminalSplitData {
    pub active: WidgetId,
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

    fn get_color(&self, color: &ansi::Color, config: &Config) -> Color {
        match color {
            ansi::Color::Named(color) => self.get_named_color(color, config),
            ansi::Color::Spec(rgb) => Color::rgb8(rgb.r, rgb.g, rgb.b),
            ansi::Color::Indexed(index) => {
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
        let color = match color {
            ansi::NamedColor::Cursor => LapceTheme::TERMINAL_CURSOR,
            ansi::NamedColor::Foreground => LapceTheme::TERMINAL_FOREGROUND,
            ansi::NamedColor::Background => LapceTheme::TERMINAL_BACKGROUND,
            ansi::NamedColor::Blue => LapceTheme::TERMINAL_BLUE,
            ansi::NamedColor::Green => LapceTheme::TERMINAL_GREEN,
            ansi::NamedColor::Yellow => LapceTheme::TERMINAL_YELLOW,
            ansi::NamedColor::Red => LapceTheme::TERMINAL_RED,
            ansi::NamedColor::White => LapceTheme::TERMINAL_WHITE,
            ansi::NamedColor::Black => LapceTheme::TERMINAL_BLACK,
            ansi::NamedColor::Cyan => LapceTheme::TERMINAL_CYAN,
            ansi::NamedColor::Magenta => LapceTheme::TERMINAL_MAGENTA,
            ansi::NamedColor::BrightBlue => LapceTheme::TERMINAL_BRIGHT_BLUE,
            ansi::NamedColor::BrightGreen => LapceTheme::TERMINAL_BRIGHT_GREEN,
            ansi::NamedColor::BrightYellow => LapceTheme::TERMINAL_BRIGHT_YELLOW,
            ansi::NamedColor::BrightRed => LapceTheme::TERMINAL_BRIGHT_RED,
            ansi::NamedColor::BrightWhite => LapceTheme::TERMINAL_BRIGHT_WHITE,
            ansi::NamedColor::BrightBlack => LapceTheme::TERMINAL_BRIGHT_BLACK,
            ansi::NamedColor::BrightCyan => LapceTheme::TERMINAL_BRIGHT_CYAN,
            ansi::NamedColor::BrightMagenta => LapceTheme::TERMINAL_BRIGHT_MAGENTA,
            ansi::NamedColor::DimBlack => LapceTheme::TERMINAL_BLACK,
            ansi::NamedColor::DimRed => LapceTheme::TERMINAL_RED,
            ansi::NamedColor::DimGreen => LapceTheme::TERMINAL_GREEN,
            ansi::NamedColor::DimYellow => LapceTheme::TERMINAL_YELLOW,
            ansi::NamedColor::DimBlue => LapceTheme::TERMINAL_BLUE,
            ansi::NamedColor::DimMagenta => LapceTheme::TERMINAL_MAGENTA,
            ansi::NamedColor::DimCyan => LapceTheme::TERMINAL_CYAN,
            ansi::NamedColor::DimWhite => LapceTheme::TERMINAL_WHITE,
            ansi::NamedColor::BrightForeground => LapceTheme::TERMINAL_FOREGROUND,
            ansi::NamedColor::DimForeground => LapceTheme::TERMINAL_FOREGROUND,
        };
        config.get_color_unchecked(color).clone()
    }
}

pub struct LapceTerminalViewData {
    terminal: Arc<LapceTerminalData>,
    term_tx: Sender<(TermId, TerminalEvent)>,
}

impl KeyPressFocus for LapceTerminalViewData {
    fn is_terminal(&self) -> bool {
        true
    }

    fn get_mode(&self) -> Mode {
        Mode::Insert
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
        match command {
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
            _ => (),
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
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
        // let (terminal, receiver) = Terminal::new(50, 20, shell, cwd);
        let widget_id = WidgetId::next();

        let term_id = TermId::next();
        proxy.new_terminal(term_id, cwd);

        let local_split_id = split_id;
        let local_widget_id = widget_id;
        let local_panel_widget_id = panel_widget_id.clone();
        //       std::thread::spawn(move || -> Result<()> {
        //    loop {
        //        let event = receiver.recv()?;
        //        match event {
        //            TerminalHostEvent::UpdateContent { cursor, content } => {
        //                event_sink.submit_command(
        //                    LAPCE_UI_COMMAND,
        //                    LapceUICommand::TerminalUpdateContent(
        //                        widget_id, content,
        //                    ),
        //                    Target::Auto,
        //                );
        //            }
        //            TerminalHostEvent::Exit => {
        //                event_sink.submit_command(
        //                    LAPCE_UI_COMMAND,
        //                    LapceUICommand::SplitTerminalClose(
        //                        local_widget_id,
        //                        local_panel_widget_id,
        //                    ),
        //                    Target::Widget(local_split_id),
        //                );
        //            }
        //        }
        //    }
        //      });

        Self {
            term_id,
            widget_id,
            split_id,
            panel_widget_id,
            content: Arc::new(TerminalContent::new()),
        }
    }
}

pub struct TerminalParser {
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

    pub fn run(&mut self, receiver: Receiver<TerminalEvent>) -> Result<()> {
        loop {
            let event = receiver.recv()?;
            match event {
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
                    }
                    alacritty_terminal::event::Event::CursorBlinkingChange(_) => {}
                    alacritty_terminal::event::Event::Wakeup => {
                        let mut cells = Vec::new();
                        for cell in self.term.grid().display_iter() {
                            let c = cell.cell.c;
                            if (c != ' ' && c != '\t')
                                || cell.bg
                                    != ansi::Color::Named(
                                        ansi::NamedColor::Background,
                                    )
                            {
                                cells.push((cell.point.clone(), cell.cell.clone()));
                            }
                        }
                        let display_offset = self.term.grid().display_offset();
                        let content = Arc::new(TerminalContent {
                            cells,
                            cursor_point: self.term.grid().cursor.point,
                            display_offset,
                            colors: self.term.renderable_content().colors.clone(),
                        });
                        self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::TerminalUpdateContent(
                                self.term_id,
                                content,
                            ),
                            Target::Auto,
                        );
                    }
                    alacritty_terminal::event::Event::Bell => {}
                    alacritty_terminal::event::Event::Exit => {
                        self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::CloseTerminal(self.term_id),
                            Target::Auto,
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
                        self.sender.send(TerminalEvent::Event(
                            alacritty_terminal::event::Event::Wakeup,
                        ));
                    }
                }
                TerminalEvent::UpdateContent(content) => {
                    self.update_content(content);
                    self.sender.send(TerminalEvent::Event(
                        alacritty_terminal::event::Event::Wakeup,
                    ));
                }
            }
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
                        KbKey::Enter => "\x0a".to_string(),
                        KbKey::Escape => "\x1b".to_string(),
                        _ => "".to_string(),
                    };
                    if s != "" {
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

        let cursor_point = &terminal.content.cursor_point;
        let rect =
            Size::new(char_width, line_height)
                .to_rect()
                .with_origin(Point::new(
                    cursor_point.column.0 as f64 * char_width,
                    (cursor_point.line.0 as f64
                        + terminal.content.display_offset as f64)
                        * line_height,
                ));
        if ctx.is_focused() {
            ctx.fill(
                rect,
                data.config.get_color_unchecked(LapceTheme::TERMINAL_CURSOR),
            );
        } else {
            ctx.stroke(
                rect,
                data.config.get_color_unchecked(LapceTheme::TERMINAL_CURSOR),
                1.0,
            );
        }

        let term_bg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND)
            .clone();
        for (point, cell) in &terminal.content.cells {
            let x = point.column.0 as f64 * char_width;
            let y = (point.line.0 as f64 + terminal.content.display_offset as f64)
                * line_height;

            let bg = data.terminal.get_color(&cell.bg, &data.config);
            if term_bg != bg {
                let rect = Size::new(char_width, line_height)
                    .to_rect()
                    .with_origin(Point::new(x, y));
                ctx.fill(rect, &bg);
            }

            let fg = data.terminal.get_color(&cell.fg, &data.config);
            let bold = cell.flags.contains(Flags::BOLD);

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
