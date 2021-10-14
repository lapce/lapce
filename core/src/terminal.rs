use std::{collections::HashMap, sync::Arc};

use alacritty_terminal::{
    ansi::{self, CursorShape, Handler},
    event::{EventListener, Notify, OnResize},
    event_loop::{EventLoop, Notifier},
    sync::FairMutex,
    term::{cell::Cell, RenderableCursor, SizeInfo},
    tty, Term,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Color, Command, Data, Env, Event, EventCtx, KbKey, LayoutCtx,
    LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    keypress::KeyPressFocus,
    proxy::{LapceProxy, TerminalContent},
    scroll::LapcePadding,
    split::{LapceSplitNew, SplitMoveDirection},
    state::{Counter, Mode},
};

const CTRL_CHARS: &'static [char] = &[
    '@', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '[', '\\', ']', '^', '_',
];

#[derive(Clone)]
pub struct TerminalSplitData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub terminals: im::HashMap<TermId, Arc<LapceTerminalData>>,
}

impl TerminalSplitData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let split_id = WidgetId::next();
        let mut terminals = im::HashMap::new();

        let terminal = Arc::new(LapceTerminalData::new(split_id, proxy));
        terminals.insert(terminal.id, terminal);

        Self {
            widget_id: WidgetId::next(),
            split_id,
            terminals,
        }
    }
}

pub struct LapceTerminalViewData {
    terminal: Arc<LapceTerminalData>,
    proxy: Arc<LapceProxy>,
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
                    LapceUICommand::SplitTerminal(true, self.terminal.widget_id),
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
            _ => (),
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        self.terminal.terminal.insert(c);
    }
}

#[derive(Clone)]
pub struct LapceTerminalData {
    pub id: TermId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub content: TerminalContent,
    pub cursor_point: alacritty_terminal::index::Point,
    pub cursor_shape: CursorShape,
    pub terminal: Terminal,
    pub receiver: Option<Receiver<TerminalHostEvent>>,
}

impl LapceTerminalData {
    pub fn new(split_id: WidgetId, proxy: Arc<LapceProxy>) -> Self {
        let id = TermId::next();
        let (terminal, receiver) = Terminal::new(50, 20);
        Self {
            id,
            widget_id: WidgetId::next(),
            split_id,
            content: TerminalContent::new(),
            cursor_point: alacritty_terminal::index::Point::default(),
            cursor_shape: CursorShape::Block,
            terminal,
            receiver: Some(receiver),
        }
    }
}

pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let (term_id, terminal_data) =
            data.terminal.terminals.iter().next().unwrap();
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(terminal_data));
        let split = LapceSplitNew::new(data.terminal.split_id).with_flex_child(
            terminal.boxed(),
            Some(terminal_data.widget_id),
            1.0,
        );
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
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(
            rect,
            5.0,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
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
            term_id: data.id,
            widget_id: data.widget_id,
            width: 0.0,
            height: 0.0,
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
            proxy: data.proxy.clone(),
        };
        match event {
            Event::MouseDown(mouse_event) => {
                ctx.request_focus();
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
                        term_data.terminal.terminal.insert(s);
                    }
                }
                data.keypress = keypress.clone();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        ctx.request_focus();
                    }
                    LapceUICommand::StartTerminal => {
                        if let Some(receiver) =
                            Arc::make_mut(&mut term_data.terminal).receiver.take()
                        {
                            let term_id = term_data.terminal.id;
                            let event_sink = ctx.get_external_handle();
                            std::thread::spawn(move || -> Result<()> {
                                loop {
                                    let event = receiver.recv()?;
                                    match event {
                                        TerminalHostEvent::UpdateContent {
                                            cursor,
                                            content,
                                        } => {
                                            event_sink.submit_command(
                                                    LAPCE_UI_COMMAND,
                                                    LapceUICommand::TerminalUpdateContent(
                                                        term_id,
                                                        content,
                                                        cursor.point,
                                                        cursor.shape,
                                                    ),
                                                    Target::Auto,
                                                );
                                        }
                                    }
                                }
                            });
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        if !term_data.terminal.same(&old_terminal_data) {
            Arc::make_mut(&mut data.terminal)
                .terminals
                .insert(term_data.terminal.id, term_data.terminal.clone());
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
            LifeCycle::WidgetAdded => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::StartTerminal,
                    Target::Widget(self.widget_id),
                ));
            }
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
            let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
            terminal.terminal.resize(width, height);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let char_size = data.config.editor_text_size(ctx.text(), "W");
        let char_width = char_size.width;
        let line_height = data.config.editor.line_height as f64;
        let y_shift = (line_height - char_size.height) / 2.0;

        let terminal = data.terminal.terminals.get(&self.term_id).unwrap();

        let rect =
            Size::new(char_width, line_height)
                .to_rect()
                .with_origin(Point::new(
                    terminal.cursor_point.column.0 as f64 * char_width,
                    terminal.cursor_point.line.0 as f64 * line_height,
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

        for (p, cell) in terminal.content.iter() {
            let x = p.column.0 as f64 * char_width;
            let y = p.line.0 as f64 * line_height + y_shift;
            let fg = match cell.fg {
                ansi::Color::Named(color) => {
                    let color = match color {
                        ansi::NamedColor::Cursor => LapceTheme::TERMINAL_CURSOR,
                        ansi::NamedColor::Foreground => {
                            LapceTheme::TERMINAL_FOREGROUND
                        }
                        ansi::NamedColor::Background => {
                            LapceTheme::TERMINAL_BACKGROUND
                        }
                        _ => LapceTheme::TERMINAL_FOREGROUND,
                    };
                    data.config.get_color_unchecked(color).clone()
                }
                ansi::Color::Spec(rgb) => Color::rgb8(rgb.r, rgb.g, rgb.b),
                ansi::Color::Indexed(index) => data
                    .config
                    .get_color_unchecked(LapceTheme::TERMINAL_FOREGROUND)
                    .clone(),
            };
            let text_layout = ctx
                .text()
                .new_text_layout(cell.c.to_string())
                .font(
                    data.config.editor.font_family(),
                    data.config.editor.font_size as f64,
                )
                .text_color(fg)
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, Point::new(x, y));
        }
    }
}

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct TermId(pub u64);

impl TermId {
    pub fn next() -> Self {
        static TERM_ID_COUNTER: Counter = Counter::new();
        Self(TERM_ID_COUNTER.next())
    }
}

pub enum TerminalEvent {
    resize(usize, usize),
    event(alacritty_terminal::event::Event),
}

pub enum TerminalHostEvent {
    UpdateContent {
        cursor: RenderableCursor,
        content: Vec<(alacritty_terminal::index::Point, Cell)>,
    },
}

#[derive(Clone)]
pub struct Terminal {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: Sender<TerminalEvent>,
    host_sender: Sender<TerminalHostEvent>,
}

pub type TermConfig = alacritty_terminal::config::Config<HashMap<String, String>>;

#[derive(Clone)]
pub struct EventProxy {
    sender: Sender<TerminalEvent>,
}

impl EventProxy {}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        self.sender.send(TerminalEvent::event(event));
    }
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> (Self, Receiver<TerminalHostEvent>) {
        let config = TermConfig::default();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let event_proxy = EventProxy {
            sender: sender.clone(),
        };
        let size =
            SizeInfo::new(width as f32, height as f32, 1.0, 1.0, 0.0, 0.0, true);
        let pty = tty::new(&config, &size, None);
        let terminal = Term::new(&config, size, event_proxy.clone());
        let terminal = Arc::new(FairMutex::new(terminal));
        let event_loop =
            EventLoop::new(terminal.clone(), event_proxy, pty, false, false);
        let loop_tx = event_loop.channel();
        let notifier = Notifier(loop_tx.clone());
        event_loop.spawn();

        let (host_sender, host_receiver) = crossbeam_channel::unbounded();
        let terminal = Terminal {
            term: terminal,
            sender,
            host_sender,
        };
        terminal.run(receiver, notifier);
        (terminal, host_receiver)
    }

    pub fn resize(&self, width: usize, height: usize) {
        self.sender.send(TerminalEvent::resize(width, height));
    }

    fn run(&self, receiver: Receiver<TerminalEvent>, mut notifier: Notifier) {
        let term = self.term.clone();
        let host_sender = self.host_sender.clone();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let event = receiver.recv()?;
                match event {
                    TerminalEvent::resize(width, height) => {
                        let size = SizeInfo::new(
                            width as f32,
                            height as f32,
                            1.0,
                            1.0,
                            0.0,
                            0.0,
                            true,
                        );
                        term.lock().resize(size.clone());
                        notifier.on_resize(&size);
                    }
                    TerminalEvent::event(event) => match event {
                        alacritty_terminal::event::Event::MouseCursorDirty => {}
                        alacritty_terminal::event::Event::Title(_) => {}
                        alacritty_terminal::event::Event::ResetTitle => {}
                        alacritty_terminal::event::Event::ClipboardStore(_, _) => {}
                        alacritty_terminal::event::Event::ClipboardLoad(_, _) => {}
                        alacritty_terminal::event::Event::ColorRequest(_, _) => {}
                        alacritty_terminal::event::Event::PtyWrite(s) => {
                            notifier.notify(s.into_bytes());
                        }
                        alacritty_terminal::event::Event::CursorBlinkingChange(
                            _,
                        ) => {}
                        alacritty_terminal::event::Event::Wakeup => {
                            let cursor =
                                term.lock().renderable_content().cursor.clone();
                            let content = term
                                .lock()
                                .renderable_content()
                                .display_iter
                                .filter_map(|c| {
                                    if (c.c == ' ' || c.c == '\t')
                                        && c.bg
                                            == ansi::Color::Named(
                                                ansi::NamedColor::Background,
                                            )
                                    {
                                        None
                                    } else {
                                        Some((c.point, c.cell.clone()))
                                    }
                                })
                                .collect::<Vec<(alacritty_terminal::index::Point, Cell)>>();
                            let event = TerminalHostEvent::UpdateContent {
                                content: content,
                                cursor: cursor,
                            };
                            host_sender.send(event);
                        }
                        alacritty_terminal::event::Event::Bell => {}
                        alacritty_terminal::event::Event::Exit => {}
                    },
                }
            }
        });
    }

    pub fn insert<B: Into<String>>(&self, data: B) {
        self.sender.send(TerminalEvent::event(
            alacritty_terminal::event::Event::PtyWrite(data.into()),
        ));
    }
}
