use std::sync::Arc;

use alacritty_terminal::{
    grid::Dimensions,
    term::{cell::Flags, test::TermSize},
};
use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout, Weight},
    id::Id,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{create_effect, ReadSignal, RwSignal},
    view::{View, ViewData},
    Renderer,
};
use lapce_core::mode::Mode;
use lapce_rpc::{proxy::ProxyRpcHandler, terminal::TermId};
use parking_lot::RwLock;
use unicode_width::UnicodeWidthChar;

use super::{panel::TerminalPanelData, raw::RawTerminal};
use crate::{
    config::{color::LapceColor, LapceConfig},
    debug::RunDebugProcess,
    panel::kind::PanelKind,
    window_tab::Focus,
};

enum TerminalViewState {
    Config,
    Focus(bool),
    Raw(Arc<RwLock<RawTerminal>>),
}

pub struct TerminalView {
    id: Id,
    data: ViewData,
    term_id: TermId,
    raw: Arc<RwLock<RawTerminal>>,
    mode: ReadSignal<Mode>,
    size: Size,
    is_focused: bool,
    config: ReadSignal<Arc<LapceConfig>>,
    run_config: ReadSignal<Option<RunDebugProcess>>,
    proxy: ProxyRpcHandler,
    launch_error: RwSignal<Option<String>>,
}

pub fn terminal_view(
    term_id: TermId,
    raw: ReadSignal<Arc<RwLock<RawTerminal>>>,
    mode: ReadSignal<Mode>,
    run_config: ReadSignal<Option<RunDebugProcess>>,
    terminal_panel_data: TerminalPanelData,
    launch_error: RwSignal<Option<String>>,
) -> TerminalView {
    let id = Id::next();

    create_effect(move |_| {
        let raw = raw.get();
        id.update_state(TerminalViewState::Raw(raw), false);
    });

    create_effect(move |_| {
        launch_error.track();
        id.request_paint();
    });

    let config = terminal_panel_data.common.config;
    create_effect(move |_| {
        config.with(|_c| {});
        id.update_state(TerminalViewState::Config, false);
    });

    let proxy = terminal_panel_data.common.proxy.clone();

    create_effect(move |last| {
        let focus = terminal_panel_data.common.focus.get();

        let mut is_focused = false;
        if let Focus::Panel(PanelKind::Terminal) = focus {
            let tab = terminal_panel_data.active_tab(true);
            if let Some(tab) = tab {
                let terminal = tab.active_terminal(true);
                is_focused = terminal.map(|t| t.term_id) == Some(term_id);
            }
        }

        if last != Some(is_focused) {
            id.update_state(TerminalViewState::Focus(is_focused), false);
        }

        is_focused
    });

    TerminalView {
        id,
        data: ViewData::new(id),
        term_id,
        raw: raw.get_untracked(),
        mode,
        config,
        proxy,
        run_config,
        size: Size::ZERO,
        is_focused: false,
        launch_error,
    }
}

impl TerminalView {
    fn char_size(&self) -> Size {
        let config = self.config.get_untracked();
        let font_family = config.terminal_font_family();
        let font_size = config.terminal_font_size();
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(font_family).collect();
        let attrs = Attrs::new().family(&family).font_size(font_size as f32);
        let attrs_list = AttrsList::new(attrs);
        let mut text_layout = TextLayout::new();
        text_layout.set_text("W", attrs_list);
        text_layout.size()
    }

    fn terminal_size(&self) -> (usize, usize) {
        let config = self.config.get_untracked();
        let line_height = config.terminal_line_height() as f64;
        let char_width = self.char_size().width;
        let width = (self.size.width / char_width).floor() as usize;
        let height = (self.size.height / line_height).floor() as usize;
        (width.max(1), height.max(1))
    }
}

impl Drop for TerminalView {
    fn drop(&mut self) {
        self.proxy.terminal_close(self.term_id);
    }
}

impl View for TerminalView {
    fn id(&self) -> Id {
        self.id
    }

    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast() {
            match *state {
                TerminalViewState::Config => {}
                TerminalViewState::Focus(is_focused) => {
                    self.is_focused = is_focused;
                }
                TerminalViewState::Raw(raw) => {
                    self.raw = raw;
                }
            }
            cx.app_state_mut().request_paint(self.id);
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, false, |_cx| Vec::new())
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        let layout = cx.get_layout(self.id).unwrap();
        let size = layout.size;
        let size = Size::new(size.width as f64, size.height as f64);
        if size.is_empty() {
            return None;
        }
        if size != self.size {
            self.size = size;
            let (width, height) = self.terminal_size();
            let term_size = TermSize::new(width, height);
            self.raw.write().term.resize(term_size);
            self.proxy.terminal_resize(self.term_id, width, height);
        }

        None
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let config = self.config.get_untracked();
        let mode = self.mode.get_untracked();
        let line_height = config.terminal_line_height() as f64;
        let font_family = config.terminal_font_family();
        let font_size = config.terminal_font_size();
        let char_size = self.char_size();
        let char_width = char_size.width;

        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(font_family).collect();
        let attrs = Attrs::new().family(&family).font_size(font_size as f32);

        if let Some(error) = self.launch_error.get() {
            let mut text_layout = TextLayout::new();
            text_layout.set_text(
                &format!("Terminal failed to launch. Error: {error}"),
                AttrsList::new(
                    attrs.color(config.color(LapceColor::EDITOR_FOREGROUND)),
                ),
            );
            cx.draw_text(
                &text_layout,
                Point::new(6.0, 0.0 + (line_height - char_size.height) / 2.0),
            );
            return;
        }

        let raw = self.raw.read();
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
                cx.fill(
                    &Rect::new(x0, y0, x1, y1),
                    config.color(LapceColor::EDITOR_SELECTION),
                    0.0,
                );
            }
        } else if mode != Mode::Terminal {
            let y = (content.cursor.point.line.0 as f64
                + content.display_offset as f64)
                * line_height;
            cx.fill(
                &Rect::new(0.0, y, self.size.width, y + line_height),
                config.color(LapceColor::EDITOR_CURRENT_LINE),
                0.0,
            );
        }

        let cursor_point = &content.cursor.point;

        let term_bg = config.color(LapceColor::TERMINAL_BACKGROUND);
        let mut text_layout = TextLayout::new();
        for item in content.display_iter {
            let point = item.point;
            let cell = item.cell;
            let inverse = cell.flags.contains(Flags::INVERSE);

            let x = point.column.0 as f64 * char_width;
            let y =
                (point.line.0 as f64 + content.display_offset as f64) * line_height;

            let mut bg = config.terminal_get_color(&cell.bg, content.colors);
            let mut fg = config.terminal_get_color(&cell.fg, content.colors);
            if cell.flags.contains(Flags::DIM)
                || cell.flags.contains(Flags::DIM_BOLD)
            {
                fg = fg.with_alpha_factor(0.66);
            }

            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            if term_bg != bg {
                let rect = Size::new(char_width, line_height)
                    .to_rect()
                    .with_origin(Point::new(x, y));
                cx.fill(&rect, bg, 0.0);
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
                let cursor_color = if mode == Mode::Terminal {
                    if self.run_config.with_untracked(|run_config| {
                        run_config.as_ref().map(|r| r.stopped).unwrap_or(false)
                    }) {
                        config.color(LapceColor::LAPCE_ERROR)
                    } else {
                        config.color(LapceColor::TERMINAL_CURSOR)
                    }
                } else {
                    config.color(LapceColor::EDITOR_CARET)
                };
                if self.is_focused {
                    cx.fill(&rect, cursor_color, 0.0);
                } else {
                    cx.stroke(&rect, cursor_color, 1.0);
                }
            }

            let bold = cell.flags.contains(Flags::BOLD)
                || cell.flags.contains(Flags::DIM_BOLD);

            if &point == cursor_point && self.is_focused {
                fg = term_bg;
            }

            if cell.c != ' ' && cell.c != '\t' {
                let mut attrs = attrs.color(fg);
                if bold {
                    attrs = attrs.weight(Weight::BOLD);
                }
                text_layout.set_text(&cell.c.to_string(), AttrsList::new(attrs));
                cx.draw_text(
                    &text_layout,
                    Point::new(x, y + (line_height - char_size.height) / 2.0),
                );
            }
        }
        // if data.find.visual {
        //     if let Some(search_string) = data.find.search_string.as_ref() {
        //         if let Ok(dfas) = RegexSearch::new(&regex::escape(search_string)) {
        //             let mut start = alacritty_terminal::index::Point::new(
        //                 alacritty_terminal::index::Line(
        //                     -(content.display_offset as i32),
        //                 ),
        //                 alacritty_terminal::index::Column(0),
        //             );
        //             let end_line = (start.line + term.screen_lines())
        //                 .min(term.bottommost_line());
        //             let mut max_lines = (end_line.0 - start.line.0) as usize;

        //             while let Some(m) = term.search_next(
        //                 &dfas,
        //                 start,
        //                 Direction::Right,
        //                 Side::Left,
        //                 Some(max_lines),
        //             ) {
        //                 let match_start = m.start();
        //                 if match_start.line.0 < start.line.0
        //                     || (match_start.line.0 == start.line.0
        //                         && match_start.column.0 < start.column.0)
        //                 {
        //                     break;
        //                 }
        //                 let x = match_start.column.0 as f64 * char_width;
        //                 let y = (match_start.line.0 as f64
        //                     + content.display_offset as f64)
        //                     * line_height;
        //                 let rect = Rect::ZERO
        //                     .with_origin(Point::new(x, y))
        //                     .with_size(Size::new(
        //                         (m.end().column.0 - m.start().column.0
        //                             + term.grid()[*m.end()].c.width().unwrap_or(1))
        //                             as f64
        //                             * char_width,
        //                         line_height,
        //                     ));
        //                 cx.stroke(
        //                     &rect,
        //                     config.get_color(LapceColor::TERMINAL_FOREGROUND),
        //                     1.0,
        //                 );
        //                 start = *m.end();
        //                 if start.column.0 < term.last_column() {
        //                     start.column.0 += 1;
        //                 } else if start.line.0 < term.bottommost_line() {
        //                     start.column.0 = 0;
        //                     start.line.0 += 1;
        //                 }
        //                 max_lines = (end_line.0 - start.line.0) as usize;
        //             }
        //         }
        //     }
        // }
    }
}
