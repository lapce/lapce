use std::sync::Arc;

use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    index::{Column, Direction, Line, Side},
    selection::{Selection, SelectionType},
    term::{cell::Flags, search::RegexSearch, Term},
};
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_core::{mode::Mode, register::Clipboard};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{FocusArea, LapceTabData},
    document::SystemClipboard,
    panel::PanelKind,
    terminal::{EventProxy, LapceTerminalData, LapceTerminalViewData},
};
use lapce_rpc::terminal::TermId;
use unicode_width::UnicodeWidthChar;

use crate::{
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::LapcePadding,
    split::LapceSplit,
    svg::get_svg,
    tab::LapceIcon,
};

pub type TermConfig = alacritty_terminal::config::Config;

/// This struct represents the main body of the terminal, i.e. the part
/// where the shell is presented.
pub struct TerminalPanel {
    widget_id: WidgetId,
    split: WidgetPod<LapceTabData, LapceSplit>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let split = LapceSplit::new(data.terminal.split_id);
        Self {
            widget_id: data.terminal.widget_id,
            split: WidgetPod::new(split),
        }
    }

    pub fn new_panel(data: &LapceTabData) -> LapcePanel {
        let split_id = WidgetId::next();
        LapcePanel::new(
            PanelKind::Terminal,
            data.terminal.widget_id,
            split_id,
            vec![(
                split_id,
                PanelHeaderKind::None,
                Self::new(data).boxed(),
                PanelSizing::Flex(true),
            )],
        )
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
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    if !data.terminal.terminals.is_empty() {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.terminal.active),
                        ));
                    }
                }
            }
            _ => (),
        }
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
        if data.terminal.terminals.is_empty() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::InitTerminalPanel(true),
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
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND),
        );
        self.split.paint(ctx, data, env);
    }
}

pub struct LapceTerminalView {
    header: WidgetPod<LapceTabData, LapceTerminalHeader>,
    terminal: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl LapceTerminalView {
    pub fn new(data: &LapceTerminalData) -> Self {
        let header = LapceTerminalHeader::new(data);
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(data));
        Self {
            header: WidgetPod::new(header),
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
        if let LifeCycle::HotChanged(is_hot) = event {
            self.header.widget_mut().view_is_hot = *is_hot;
            ctx.request_paint();
        }
        self.header.lifecycle(ctx, event, data, env);
        self.terminal.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
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
        let self_rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.clip(self_rect.inflate(0.0, 50.0));
            let rect = self.header.layout_rect();
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            }
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND),
            );
        });

        self.header.paint(ctx, data, env);
        self.terminal.paint(ctx, data, env);
    }
}

struct LapceTerminalHeader {
    term_id: TermId,
    height: f64,
    icon_size: f64,
    icon_padding: f64,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
    view_is_hot: bool,
    hover_rect: Option<Rect>,
}

impl LapceTerminalHeader {
    pub fn new(data: &LapceTerminalData) -> Self {
        Self {
            term_id: data.term_id,
            height: 30.0,
            icon_size: 24.0,
            mouse_pos: Point::ZERO,
            icon_padding: 4.0,
            icons: Vec::new(),
            view_is_hot: false,
            hover_rect: None,
        }
    }

    fn get_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let gap = (self.height - self.icon_size) / 2.0;

        let terminal_data = data.terminal.terminals.get(&self.term_id).unwrap();

        let mut icons = Vec::new();
        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "close.svg",
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::CloseTerminal(self.term_id),
                Target::Widget(data.id),
            ),
        };
        icons.push(icon);

        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "split-horizontal.svg",
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitTerminal(true, terminal_data.widget_id),
                Target::Widget(terminal_data.split_id),
            ),
        };
        icons.push(icon);

        icons
    }

    fn icon_hit_test(&mut self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                self.hover_rect = Some(icon.rect);
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                let hover_rect = self.hover_rect;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    self.hover_rect = None;
                    ctx.clear_cursor();
                }
                if hover_rect != self.hover_rect {
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let self_size = Size::new(bc.max().width, self.height);
        self.icons = self.get_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let mut clip_rect = ctx.size().to_rect();
        if self.view_is_hot {
            if let Some(icon) = self.icons.iter().rev().next().as_ref() {
                clip_rect.x1 = icon.rect.x0;
            }
        }

        ctx.with_save(|ctx| {
            ctx.clip(clip_rect);
            let svg = get_svg("terminal.svg").unwrap();
            let width = data.config.terminal_font_size() as f64;
            let height = data.config.terminal_font_size() as f64;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (self.height - width) / 2.0,
                (self.height - height) / 2.0,
            ));
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
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let y = text_layout.y_offset(self.height);
            ctx.draw_text(&text_layout, Point::new(self.height, y));
        });

        if self.view_is_hot {
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        &icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                if let Some(svg) = get_svg(icon.icon) {
                    ctx.draw_svg(
                        &svg,
                        icon.rect.inflate(-self.icon_padding, -self.icon_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        }
    }
}

struct LapceTerminal {
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
        data.focus = Arc::new(self.widget_id);
        data.focus_area = FocusArea::Panel(PanelKind::Terminal);
        if let Some((index, position)) =
            data.panel.panel_position(&PanelKind::Terminal)
        {
            let panel = Arc::make_mut(&mut data.panel);
            if let Some(style) = panel.style.get_mut(&position) {
                style.active = index;
            }
            panel.active = position;
        }
    }

    fn select(
        &self,
        term: &mut Term<EventProxy>,
        mouse_event: &MouseEvent,
        ty: SelectionType,
    ) {
        let row_size = self.height / term.screen_lines() as f64;
        let col_size = self.width / term.columns() as f64;
        let offset = term.grid().display_offset();
        let column = Column((mouse_event.pos.x / col_size) as usize);
        let line = Line((mouse_event.pos.y / row_size) as i32 - offset as i32);
        match &mut term.selection {
            Some(selection) => selection.update(
                alacritty_terminal::index::Point { line, column },
                Direction::Left,
            ),
            None => {
                term.selection = Some(Selection::new(
                    ty,
                    alacritty_terminal::index::Point { line, column },
                    Direction::Left,
                ));
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
        ctx.set_cursor(&Cursor::IBeam);
        match event {
            Event::MouseDown(mouse_event) => {
                self.request_focus(ctx, data);
                let terminal = data.terminal.terminals.get(&self.term_id).unwrap();
                let term = &mut terminal.raw.lock().term;
                if mouse_event.button.is_right() {
                    let mut clipboard = SystemClipboard {};
                    match term.selection_to_string() {
                        Some(selection) => {
                            clipboard.put_string(selection);
                            term.selection = None;
                        }
                        None => {
                            if let Some(string) = clipboard.get_string() {
                                terminal.proxy.proxy_rpc.terminal_write(
                                    terminal.term_id,
                                    string.as_str(),
                                );
                                term.scroll_display(Scroll::Bottom);
                            }
                        }
                    }
                } else if mouse_event.button.is_left() {
                    match mouse_event.count {
                        2 => self.select(term, mouse_event, SelectionType::Semantic),
                        _ => {
                            term.selection = None;
                            if mouse_event.count == 3 {
                                self.select(term, mouse_event, SelectionType::Lines);
                            }
                        }
                    }
                }
            }
            Event::MouseMove(mouse_event) => {
                if mouse_event.buttons.has_left() {
                    let term = &mut data
                        .terminal
                        .terminals
                        .get(&self.term_id)
                        .unwrap()
                        .raw
                        .lock()
                        .term;
                    self.select(term, mouse_event, SelectionType::Simple);
                    ctx.request_paint();
                }
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
                ) && term_data.terminal.mode == Mode::Terminal
                {
                    term_data.send_keypress(key_event);
                }
                ctx.set_handled();
                data.keypress = keypress.clone();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx, data);
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
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::FocusChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let size = bc.max();
        if self.width != size.width || self.height != size.height {
            self.width = size.width;
            self.height = size.height;
            let width = data.config.terminal_char_width(ctx.text());
            let line_height = data.config.terminal_line_height() as f64;
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let char_size = data.config.terminal_text_size(ctx.text(), "W");
        let char_width = char_size.width;
        let line_height = data.config.terminal_line_height() as f64;

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
                let left_col = if selection.is_block || line == start_line {
                    start_col
                } else {
                    0
                };
                let right_col = if selection.is_block || line == end_line {
                    end_col + 1
                } else {
                    term.last_column().0
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
        } else if terminal.mode != Mode::Terminal {
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

        let cursor_point = &content.cursor.point;

        let term_bg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND)
            .clone();
        let _term_fg = data
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
                    .get_color(&cell.bg, content.colors, &data.config);
            let mut fg =
                data.terminal
                    .get_color(&cell.fg, content.colors, &data.config);
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
                        data.config.terminal_font_family(),
                        data.config.terminal_font_size() as f64,
                    )
                    .text_color(fg);
                if bold {
                    builder = builder
                        .default_attribute(TextAttribute::Weight(FontWeight::BOLD));
                }
                let text_layout = builder.build().unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(x, y + text_layout.y_offset(line_height)),
                );
            }
        }
        if data.find.visual {
            if let Some(search_string) = data.find.search_string.as_ref() {
                if let Ok(dfas) = RegexSearch::new(&regex::escape(search_string)) {
                    let mut start = alacritty_terminal::index::Point::new(
                        alacritty_terminal::index::Line(
                            -(content.display_offset as i32),
                        ),
                        alacritty_terminal::index::Column(0),
                    );
                    let end_line = (start.line + term.screen_lines())
                        .min(term.bottommost_line());
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
                        let rect = Rect::ZERO
                            .with_origin(Point::new(x, y))
                            .with_size(Size::new(
                                (m.end().column.0 - m.start().column.0
                                    + term.grid()[*m.end()].c.width().unwrap_or(1))
                                    as f64
                                    * char_width,
                                line_height,
                            ));
                        ctx.stroke(
                            rect,
                            data.config.get_color_unchecked(
                                LapceTheme::TERMINAL_FOREGROUND,
                            ),
                            1.0,
                        );
                        start = *m.end();
                        if start.column.0 < term.last_column() {
                            start.column.0 += 1;
                        } else if start.line.0 < term.bottommost_line() {
                            start.column.0 = 0;
                            start.line.0 += 1;
                        }
                        max_lines = (end_line.0 - start.line.0) as usize;
                    }
                }
            }
        }
    }
}
