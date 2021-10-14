use std::sync::Arc;

use alacritty_terminal::ansi;
use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Color, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, Modifiers, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_proxy::terminal::TermId;

use crate::{
    command::LapceCommand,
    config::LapceTheme,
    data::LapceTabData,
    keypress::KeyPressFocus,
    proxy::{CursorShape, LapceProxy, TerminalContent},
    scroll::LapcePadding,
    split::LapceSplitNew,
    state::Mode,
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

        let terminal = Arc::new(LapceTerminalData::new(proxy));
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
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        self.proxy.terminal_insert(self.terminal.id, c);
    }
}

#[derive(Clone)]
pub struct LapceTerminalData {
    id: TermId,
    pub content: TerminalContent,
    pub cursor_point: alacritty_terminal::index::Point,
    pub cursor_shape: CursorShape,
}

impl LapceTerminalData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let id = TermId::next();
        std::thread::spawn(move || {
            proxy.new_terminal(id, 50, 20);
        });
        Self {
            id,
            content: TerminalContent::new(),
            cursor_point: alacritty_terminal::index::Point::default(),
            cursor_shape: CursorShape::Block,
        }
    }
}

pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let (term_id, _) = data.terminal.terminals.iter().next().unwrap();
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(*term_id));
        let split = LapceSplitNew::new(data.terminal.split_id).with_flex_child(
            terminal.boxed(),
            None,
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
    width: f64,
    height: f64,
}

impl LapceTerminal {
    pub fn new(term_id: TermId) -> Self {
        Self {
            term_id,
            width: 0.0,
            height: 0.0,
        }
    }
}

impl Widget<LapceTabData> for LapceTerminal {
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
                        data.proxy.terminal_insert(self.term_id, &s);
                    }
                }
                data.keypress = keypress.clone();
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
            data.proxy.terminal_resize(self.term_id, width, height);
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
