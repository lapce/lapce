use std::{cmp, path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    action::{set_ime_allowed, set_ime_cursor_area},
    context::{PaintCx, StyleCx},
    event::{Event, EventListener, EventPropagation},
    keyboard::Modifiers,
    peniko::{
        kurbo::{Line, Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, Memo, ReadSignal, RwSignal,
    },
    style::{CursorColor, CursorStyle, Style, TextColor},
    taffy::prelude::NodeId,
    views::{
        clip, container, dyn_stack,
        editor::{
            text::WrapMethod,
            view::{
                cursor_caret, DiffSectionKind, EditorView as FloemEditorView,
                EditorViewClass, LineRegion, ScreenLines,
            },
            visual_line::{RVLine, VLine},
            CurrentLineColor, CursorSurroundingLines, Editor, EditorStyle,
            IndentGuideColor, IndentStyleProp, Modal, ModalRelativeLine,
            PhantomColor, PlaceholderColor, PreeditUnderlineColor,
            RenderWhitespaceProp, ScrollBeyondLastLine, SelectionColor,
            ShowIndentGuide, SmartTab, VisibleWhitespaceColor, WrapProp,
        },
        empty, label,
        scroll::{scroll, HideBar, PropagatePointerWheel},
        stack, svg, Decorators,
    },
    Renderer, View, ViewId,
};
use itertools::Itertools;
use lapce_core::{
    buffer::{diff::DiffLines, rope_text::RopeText, Buffer},
    cursor::{CursorAffinity, CursorMode},
};
use lapce_rpc::dap_types::{DapId, SourceBreakpoint};
use lapce_xi_rope::find::CaseMatching;

use super::{gutter::editor_gutter_view, DocSignal, EditorData};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, editor::WrapStyle, icon::LapceIcons, LapceConfig},
    debug::LapceBreakpoint,
    doc::DocContent,
    text_input::TextInputBuilder,
    window_tab::{Focus, WindowTabData},
    workspace::LapceWorkspace,
};

struct StickyHeaderInfo {
    sticky_lines: Vec<usize>,
    last_sticky_should_scroll: bool,
    y_diff: f64,
}

fn editor_wrap(config: &LapceConfig) -> WrapMethod {
    /// Minimum width that we'll allow the view to be wrapped at.
    const MIN_WRAPPED_WIDTH: f32 = 100.0;

    match config.editor.wrap_style {
        WrapStyle::None => WrapMethod::None,
        WrapStyle::EditorWidth => WrapMethod::EditorWidth,
        WrapStyle::WrapWidth => WrapMethod::WrapWidth {
            width: (config.editor.wrap_width as f32).max(MIN_WRAPPED_WIDTH),
        },
    }
}

pub fn editor_style(
    config: ReadSignal<Arc<LapceConfig>>,
    doc: DocSignal,
    s: Style,
) -> Style {
    let config = config.get();
    let doc = doc.get();

    s.set(
        IndentStyleProp,
        doc.buffer.with_untracked(Buffer::indent_style),
    )
    .set(CursorColor, config.color(LapceColor::EDITOR_CARET))
    .set(SelectionColor, config.color(LapceColor::EDITOR_SELECTION))
    .set(
        CurrentLineColor,
        config.color(LapceColor::EDITOR_CURRENT_LINE),
    )
    .set(
        VisibleWhitespaceColor,
        config.color(LapceColor::EDITOR_VISIBLE_WHITESPACE),
    )
    .set(
        IndentGuideColor,
        config.color(LapceColor::EDITOR_INDENT_GUIDE),
    )
    .set(ScrollBeyondLastLine, config.editor.scroll_beyond_last_line)
    .color(config.color(LapceColor::EDITOR_FOREGROUND))
    .set(TextColor, config.color(LapceColor::EDITOR_FOREGROUND))
    .set(PhantomColor, config.color(LapceColor::EDITOR_DIM))
    .set(PlaceholderColor, config.color(LapceColor::EDITOR_DIM))
    .set(
        PreeditUnderlineColor,
        config.color(LapceColor::EDITOR_FOREGROUND),
    )
    .set(ShowIndentGuide, config.editor.show_indent_guide)
    .set(Modal, config.core.modal)
    .set(
        ModalRelativeLine,
        config.editor.modal_mode_relative_line_numbers,
    )
    .set(SmartTab, config.editor.smart_tab)
    .set(WrapProp, editor_wrap(&config))
    .set(
        CursorSurroundingLines,
        config.editor.cursor_surrounding_lines,
    )
    .set(RenderWhitespaceProp, config.editor.render_whitespace)
}

pub struct EditorView {
    id: ViewId,
    editor: EditorData,
    is_active: Memo<bool>,
    inner_node: Option<NodeId>,
    viewport: RwSignal<Rect>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    sticky_header_info: StickyHeaderInfo,
}

pub fn editor_view(
    e_data: EditorData,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> EditorView {
    let id = ViewId::new();
    let is_active = create_memo(move |_| is_active(true));

    let viewport = e_data.viewport();

    let doc = e_data.doc_signal();
    let view_kind = e_data.kind;
    let screen_lines = e_data.screen_lines();
    create_effect(move |_| {
        doc.track();
        view_kind.track();
        id.request_layout();
    });

    let hide_cursor = e_data.common.window_common.hide_cursor;
    create_effect(move |_| {
        hide_cursor.track();
        let occurrences = doc.with(|doc| doc.find_result.occurrences);
        occurrences.track();
        id.request_paint();
    });

    create_effect(move |last_rev| {
        let buffer = doc.with(|doc| doc.buffer);
        let rev = buffer.with(|buffer| buffer.rev());
        if last_rev == Some(rev) {
            return rev;
        }
        id.request_layout();
        rev
    });

    let config = e_data.common.config;
    let sticky_header_height_signal = e_data.sticky_header_height;
    let editor2 = e_data.clone();
    create_effect(move |last_rev| {
        let config = config.get();
        if !config.editor.sticky_header {
            return (DocContent::Local, 0, 0, Rect::ZERO, 0, None);
        }

        let doc = doc.get();
        let rect = viewport.get();
        let (screen_lines_len, screen_lines_first) = screen_lines
            .with(|lines| (lines.lines.len(), lines.lines.first().copied()));
        let rev = (
            doc.content.get(),
            doc.buffer.with(|b| b.rev()),
            doc.cache_rev.get(),
            rect,
            screen_lines_len,
            screen_lines_first,
        );
        if last_rev.as_ref() == Some(&rev) {
            return rev;
        }

        let sticky_header_info = get_sticky_header_info(
            &editor2,
            viewport,
            sticky_header_height_signal,
            &config,
        );

        id.update_state(sticky_header_info);

        rev
    });

    let ed1 = e_data.editor.clone();
    let ed2 = ed1.clone();
    let ed3 = ed1.clone();

    let editor_window_origin = e_data.window_origin();
    let cursor = e_data.cursor();
    let find_focus = e_data.find_focus;
    let ime_allowed = e_data.common.window_common.ime_allowed;
    let editor_viewport = e_data.viewport();
    let editor_cursor = e_data.cursor();
    create_effect(move |_| {
        let active = is_active.get();
        if active && !find_focus.get() {
            if !cursor.with(|c| c.is_insert()) {
                if ime_allowed.get_untracked() {
                    ime_allowed.set(false);
                    set_ime_allowed(false);
                }
            } else {
                if !ime_allowed.get_untracked() {
                    ime_allowed.set(true);
                    set_ime_allowed(true);
                }
                let (offset, affinity) = cursor.with(|c| (c.offset(), c.affinity));
                let (_, point_below) = ed1.points_of_offset(offset, affinity);
                let window_origin = editor_window_origin.get();
                let viewport = editor_viewport.get();
                let pos = window_origin
                    + (point_below.x - viewport.x0, point_below.y - viewport.y0);
                set_ime_cursor_area(pos, Size::new(800.0, 600.0));
            }
        }
    });

    let doc = e_data.doc_signal();
    EditorView {
        id,
        editor: e_data,
        is_active,
        inner_node: None,
        viewport,
        debug_breakline,
        sticky_header_info: StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        },
    }
    .on_event(EventListener::ImePreedit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImePreedit { text, cursor } = event {
            if text.is_empty() {
                ed2.clear_preedit();
            } else {
                let offset = editor_cursor.with_untracked(|c| c.offset());
                ed2.set_preedit(text.clone(), *cursor, offset);
            }
        }
        EventPropagation::Stop
    })
    .on_event(EventListener::ImeCommit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImeCommit(text) = event {
            ed3.clear_preedit();
            ed3.receive_char(text);
        }
        EventPropagation::Stop
    })
    .class(EditorViewClass)
    .style(move |s| editor_style(config, doc, s))
}

impl EditorView {
    fn paint_diff_sections(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        config: &LapceConfig,
    ) {
        let Some(diff_sections) = &screen_lines.diff_sections else {
            return;
        };
        for section in diff_sections.iter() {
            match section.kind {
                DiffSectionKind::NoCode => self.paint_diff_no_code(
                    cx,
                    viewport,
                    section.y_idx,
                    section.height,
                    config,
                ),
                DiffSectionKind::Added => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(
                                viewport.width(),
                                (config.editor.line_height() * section.height)
                                    as f64,
                            ))
                            .with_origin(Point::new(
                                viewport.x0,
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(LapceColor::SOURCE_CONTROL_ADDED)
                            .with_alpha_factor(0.2),
                        0.0,
                    );
                }
                DiffSectionKind::Removed => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(
                                viewport.width(),
                                (config.editor.line_height() * section.height)
                                    as f64,
                            ))
                            .with_origin(Point::new(
                                viewport.x0,
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(LapceColor::SOURCE_CONTROL_REMOVED)
                            .with_alpha_factor(0.2),
                        0.0,
                    );
                }
            }
        }
    }

    fn paint_diff_no_code(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        start_line: usize,
        height: usize,
        config: &LapceConfig,
    ) {
        let line_height = config.editor.line_height();
        let height = (height * line_height) as f64;
        let y = (start_line * line_height) as f64;
        let y_end = y + height;

        if y_end < viewport.y0 || y > viewport.y1 {
            return;
        }

        let y = y.max(viewport.y0 - 10.0);
        let y_end = y_end.min(viewport.y1 + 10.0);
        let height = y_end - y;

        let start_x = viewport.x0.floor() as usize;
        let start_x = start_x - start_x % 8;

        for x in (start_x..viewport.x1.ceil() as usize + 1 + height.ceil() as usize)
            .step_by(8)
        {
            let p0 = if x as f64 > viewport.x1.ceil() {
                Point::new(viewport.x1.ceil(), y + (x as f64 - viewport.x1.ceil()))
            } else {
                Point::new(x as f64, y)
            };

            let height = if x as f64 - height < viewport.x0.floor() {
                x as f64 - viewport.x0.floor()
            } else {
                height
            };
            if height > 0.0 {
                let p1 = Point::new(x as f64 - height, y + height);
                cx.stroke(
                    &Line::new(p0, p1),
                    config.color(LapceColor::EDITOR_DIM),
                    1.0,
                );
            }
        }
    }

    fn paint_cursor(
        &self,
        cx: &mut PaintCx,
        is_local: bool,
        screen_lines: &ScreenLines,
    ) {
        let e_data = self.editor.clone();
        let ed = e_data.editor.clone();
        let cursor = self.editor.cursor();
        let find_focus = self.editor.find_focus;
        let config = self.editor.common.config;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let viewport = self.viewport.get_untracked();
        let is_active =
            self.is_active.get_untracked() && !find_focus.get_untracked();

        let current_line_color = ed.es.with_untracked(EditorStyle::current_line);

        let breakline = self.debug_breakline.get_untracked().and_then(
            |(breakline, breakline_path)| {
                if e_data
                    .doc()
                    .content
                    .with_untracked(|c| c.path() == Some(&breakline_path))
                {
                    Some(breakline)
                } else {
                    None
                }
            },
        );

        // TODO: check if this is correct
        if let Some(breakline) = breakline {
            if let Some(info) = screen_lines.info_for_line(breakline) {
                let rect = Rect::from_origin_size(
                    (viewport.x0, info.vline_y),
                    (viewport.width(), line_height),
                );

                cx.fill(
                    &rect,
                    config.color(LapceColor::EDITOR_DEBUG_BREAK_LINE),
                    0.0,
                );
            }
        }

        cursor.with_untracked(|cursor| {
            let highlight_current_line = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Insert(_) => true,
                CursorMode::Visual { .. } => false,
            };

            if let Some(current_line_color) = current_line_color {
                // Highlight the current line
                if !is_local && highlight_current_line {
                    for (_, end) in cursor.regions_iter() {
                        // TODO: unsure if this is correct for wrapping lines
                        let rvline = ed.rvline_of_offset(end, cursor.affinity);

                        if let Some(info) = screen_lines
                            .info(rvline)
                            .filter(|_| Some(rvline.line) != breakline)
                        {
                            let rect = Rect::from_origin_size(
                                (viewport.x0, info.vline_y),
                                (viewport.width(), line_height),
                            );

                            cx.fill(&rect, current_line_color, 0.0);
                        }
                    }
                }
            }

            FloemEditorView::paint_selection(cx, &ed, screen_lines);

            FloemEditorView::paint_cursor_caret(cx, &ed, is_active, screen_lines);
        });
    }

    fn paint_find(&self, cx: &mut PaintCx, screen_lines: &ScreenLines) {
        let visual = self.editor.common.find.visual;
        if !visual.get_untracked() {
            return;
        }
        if screen_lines.lines.is_empty() {
            return;
        }

        let min_vline = *screen_lines.lines.first().unwrap();
        let max_vline = *screen_lines.lines.last().unwrap();
        let min_line = screen_lines.info(min_vline).unwrap().vline_info.rvline.line;
        let max_line = screen_lines.info(max_vline).unwrap().vline_info.rvline.line;

        let e_data = &self.editor;
        let ed = &e_data.editor;
        let doc = e_data.doc();

        let config = self.editor.common.config;
        let occurrences = doc.find_result.occurrences;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        doc.update_find();
        let start = ed.offset_of_line(min_line);
        let end = ed.offset_of_line(max_line + 1);

        // TODO: The selection rect creation logic for find is quite similar to the version
        // within insert cursor. It would be good to deduplicate it.
        let mut rects = Vec::new();
        for region in occurrences.with_untracked(|selection| {
            selection.regions_in_range(start, end).to_vec()
        }) {
            let start = region.min();
            let end = region.max();

            // TODO(minor): the proper affinity here should probably be tracked by selregion
            let (start_rvline, start_col) =
                ed.rvline_col_of_offset(start, CursorAffinity::Forward);
            let (end_rvline, end_col) =
                ed.rvline_col_of_offset(end, CursorAffinity::Backward);

            for line_info in screen_lines.iter_line_info() {
                let rvline_info = line_info.vline_info;
                let rvline = rvline_info.rvline;
                let line = rvline.line;

                if rvline < start_rvline {
                    continue;
                }

                if rvline > end_rvline {
                    break;
                }

                let left_col = if rvline == start_rvline { start_col } else { 0 };
                let (right_col, _vline_end) = if rvline == end_rvline {
                    let max_col = ed.last_col(rvline_info, true);
                    (end_col.min(max_col), false)
                } else {
                    (ed.last_col(rvline_info, true), true)
                };

                // TODO(minor): sel region should have the affinity of the start/end
                let x0 = ed
                    .line_point_of_line_col(
                        line,
                        left_col,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let x1 = ed
                    .line_point_of_line_col(
                        line,
                        right_col,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                if !rvline_info.is_empty() && start != end && left_col != right_col {
                    rects.push(
                        Size::new(x1 - x0, line_height)
                            .to_rect()
                            .with_origin(Point::new(x0, line_info.vline_y)),
                    );
                }
            }
        }

        let color = config.color(LapceColor::EDITOR_FOREGROUND);
        for rect in rects {
            cx.stroke(&rect, color, 1.0);
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }
        if !self.editor.kind.get_untracked().is_normal() {
            return;
        }

        let line_height = config.editor.line_height();
        let Some(start_vline) = screen_lines.lines.first() else {
            return;
        };
        let start_info = screen_lines.vline_info(*start_vline).unwrap();
        let start_line = start_info.rvline.line;

        let total_sticky_lines = self.sticky_header_info.sticky_lines.len();

        let paint_last_line = total_sticky_lines > 0
            && (self.sticky_header_info.last_sticky_should_scroll
                || self.sticky_header_info.y_diff != 0.0
                || start_line + total_sticky_lines - 1
                    != *self.sticky_header_info.sticky_lines.last().unwrap());

        let total_sticky_lines = if paint_last_line {
            total_sticky_lines
        } else {
            total_sticky_lines.saturating_sub(1)
        };

        if total_sticky_lines == 0 {
            return;
        }

        let scroll_offset = if self.sticky_header_info.last_sticky_should_scroll {
            self.sticky_header_info.y_diff
        } else {
            0.0
        };

        // Clear background

        let area_height = self
            .sticky_header_info
            .sticky_lines
            .iter()
            .copied()
            .map(|line| {
                let layout = self.editor.editor.text_layout(line);
                layout.line_count() * line_height
            })
            .sum::<usize>() as f64
            - scroll_offset;

        let sticky_area_rect = Size::new(viewport.x1, area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0))
            .inflate(10.0, 0.0);

        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
            3.0,
        );
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
            0.0,
        );

        // Paint lines
        let mut y_accum = 0.0;
        for (i, line) in self
            .sticky_header_info
            .sticky_lines
            .iter()
            .copied()
            .enumerate()
        {
            let y_diff = if i == total_sticky_lines - 1 {
                scroll_offset
            } else {
                0.0
            };

            let text_layout = self.editor.editor.text_layout(line);

            let text_height = (text_layout.line_count() * line_height) as f64;
            let height = text_height - y_diff;

            cx.save();

            let line_area_rect = Size::new(viewport.width(), height)
                .to_rect()
                .with_origin(Point::new(viewport.x0, viewport.y0 + y_accum));

            cx.clip(&line_area_rect);

            let y = viewport.y0 - y_diff + y_accum;
            cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

            y_accum += text_height;

            cx.restore();
        }
    }

    fn paint_scroll_bar(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        is_local: bool,
        config: Arc<LapceConfig>,
    ) {
        const BAR_WIDTH: f64 = 10.0;

        if is_local {
            return;
        }

        cx.fill(
            &Rect::ZERO
                .with_size(Size::new(1.0, viewport.height()))
                .with_origin(Point::new(
                    viewport.x0 + viewport.width() - BAR_WIDTH,
                    viewport.y0,
                ))
                .inflate(0.0, 10.0),
            config.color(LapceColor::LAPCE_SCROLL_BAR),
            0.0,
        );

        if !self.editor.kind.get_untracked().is_normal() {
            return;
        }

        let doc = self.editor.doc();
        let total_len = doc.buffer.with_untracked(|buffer| buffer.last_line());
        let changes = doc.head_changes().get_untracked();
        let total_height = viewport.height();
        let total_width = viewport.width();
        let line_height = config.editor.line_height();
        let content_height = if config.editor.scroll_beyond_last_line {
            (total_len * line_height) as f64 + total_height - line_height as f64
        } else {
            (total_len * line_height) as f64
        };

        let colors = changes_colors_all(&config, &self.editor.editor, changes);
        for (y, height, _, color) in colors {
            let y = y / content_height * total_height;
            let height = ((height * line_height) as f64 / content_height
                * total_height)
                .max(3.0);
            let rect = Rect::ZERO.with_size(Size::new(3.0, height)).with_origin(
                Point::new(
                    viewport.x0 + total_width - BAR_WIDTH + 1.0,
                    y + viewport.y0,
                ),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    /// Paint a highlight around the characters at the given positions.
    fn paint_char_highlights(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
        highlight_line_cols: impl Iterator<Item = (RVLine, usize)>,
    ) {
        let editor = &self.editor.editor;
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        for (rvline, col) in highlight_line_cols {
            // Is the given line on screen?
            if let Some(line_info) = screen_lines.info(rvline) {
                let x0 = editor
                    .line_point_of_line_col(
                        rvline.line,
                        col,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let x1 = editor
                    .line_point_of_line_col(
                        rvline.line,
                        col + 1,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                let y0 = line_info.vline_y;
                let y1 = y0 + line_height;

                let rect = Rect::new(x0, y0, x1, y1);

                cx.stroke(&rect, config.color(LapceColor::EDITOR_FOREGROUND), 1.0);
            }
        }
    }

    /// Paint scope lines between `(start_rvline, start_line, start_col)` and
    /// `(end_rvline, end_line end_col)`.
    fn paint_scope_lines(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        (start, start_col): (RVLine, usize),
        (end, end_col): (RVLine, usize),
    ) {
        let editor = &self.editor.editor;
        let doc = self.editor.doc();
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let brush = config.color(LapceColor::EDITOR_FOREGROUND);

        if start == end {
            if let Some(line_info) = screen_lines.info(start) {
                // TODO: Due to line wrapping the y positions of these two spots could be different, do we need to change it?
                let x0 = editor
                    .line_point_of_line_col(
                        start.line,
                        start_col + 1,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let x1 = editor
                    .line_point_of_line_col(
                        end.line,
                        end_col,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                if x0 < x1 {
                    let y = line_info.vline_y + line_height;

                    let p0 = Point::new(x0, y);
                    let p1 = Point::new(x1, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, 1.0);
                }
            }
        } else {
            // Are start_line and end_line on screen?
            let start_line_y = screen_lines
                .info(start)
                .map(|line_info| line_info.vline_y + line_height);
            let end_line_y = screen_lines
                .info(end)
                .map(|line_info| line_info.vline_y + line_height);

            // We only need to draw anything if start_line is on or before the visible section and
            // end_line is on or after the visible section.
            let y0 = start_line_y.or_else(|| {
                screen_lines
                    .lines
                    .first()
                    .is_some_and(|&first_vline| first_vline > start)
                    .then(|| viewport.min_y())
            });
            let y1 = end_line_y.or_else(|| {
                screen_lines
                    .lines
                    .last()
                    .is_some_and(|&last_line| last_line < end)
                    .then(|| viewport.max_y())
            });

            if let [Some(y0), Some(y1)] = [y0, y1] {
                let start_x = editor
                    .line_point_of_line_col(
                        start.line,
                        start_col + 1,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let end_x = editor
                    .line_point_of_line_col(
                        end.line,
                        end_col,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                // TODO(minor): is this correct with line wrapping?
                // The vertical line should be drawn to the left of any non-whitespace characters
                // in the enclosed section.
                let min_text_x = doc.buffer.with_untracked(|buffer| {
                    ((start.line + 1)..=end.line)
                        .filter(|&line| !buffer.is_line_whitespace(line))
                        .map(|line| {
                            let non_blank_offset =
                                buffer.first_non_blank_character_on_line(line);
                            let (_, col) =
                                editor.offset_to_line_col(non_blank_offset);

                            editor
                                .line_point_of_line_col(
                                    line,
                                    col,
                                    CursorAffinity::Backward,
                                    true,
                                )
                                .x
                        })
                        .min_by(f64::total_cmp)
                });

                let min_x = min_text_x.map_or(start_x, |min_text_x| {
                    cmp::min_by(min_text_x, start_x, f64::total_cmp)
                });

                // Is start_line on screen, and is the vertical line to the left of the opening
                // bracket?
                if let Some(y) = start_line_y.filter(|_| start_x > min_x) {
                    let p0 = Point::new(min_x, y);
                    let p1 = Point::new(start_x, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, 1.0);
                }

                // Is end_line on screen, and is the vertical line to the left of the closing
                // bracket?
                if let Some(y) = end_line_y.filter(|_| end_x > min_x) {
                    let p0 = Point::new(min_x, y);
                    let p1 = Point::new(end_x, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, 1.0);
                }

                let p0 = Point::new(min_x, y0);
                let p1 = Point::new(min_x, y1);
                let line = Line::new(p0, p1);

                cx.stroke(&line, brush, 1.0);
            }
        }
    }

    /// Paint enclosing bracket highlights and scope lines if the corresponding settings are
    /// enabled.
    fn paint_bracket_highlights_scope_lines(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();

        if config.editor.highlight_matching_brackets
            || config.editor.highlight_scope_lines
        {
            let e_data = &self.editor;
            let ed = &e_data.editor;
            let offset = ed.cursor.with_untracked(|cursor| cursor.mode.offset());

            let bracket_offsets = e_data
                .doc_signal()
                .with_untracked(|doc| doc.find_enclosing_brackets(offset))
                .map(|(start, end)| [start, end]);

            let bracket_line_cols = bracket_offsets.map(|bracket_offsets| {
                bracket_offsets.map(|offset| {
                    let (rvline, col) =
                        ed.rvline_col_of_offset(offset, CursorAffinity::Forward);
                    (rvline, col)
                })
            });

            if config.editor.highlight_matching_brackets {
                self.paint_char_highlights(
                    cx,
                    screen_lines,
                    bracket_line_cols.into_iter().flatten(),
                );
            }

            if config.editor.highlight_scope_lines {
                if let Some([start_line_col, end_line_col]) = bracket_line_cols {
                    self.paint_scope_lines(
                        cx,
                        viewport,
                        screen_lines,
                        start_line_col,
                        end_line_col,
                    );
                }
            }
        }
    }
}

impl View for EditorView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn style_pass(&mut self, cx: &mut StyleCx<'_>) {
        let editor = &self.editor.editor;
        if editor.es.try_update(|s| s.read(cx)).unwrap() {
            editor.floem_style_id.update(|val| *val += 1);
            cx.app_state_mut().request_paint(self.id());
        }
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Editor View".into()
    }

    fn update(
        &mut self,
        _cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast() {
            self.sticky_header_info = *state;
            self.id.request_layout();
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::NodeId {
        cx.layout_node(self.id, true, |_cx| {
            if self.inner_node.is_none() {
                self.inner_node = Some(self.id.new_taffy_node());
            }

            let e_data = &self.editor;
            let editor = &e_data.editor;

            let viewport_size = self.viewport.get_untracked().size();

            let screen_lines = e_data.screen_lines().get_untracked();
            for (line, _) in screen_lines.iter_lines_y() {
                // fill in text layout cache so that max width is correct.
                editor.text_layout(line);
            }

            let inner_node = self.inner_node.unwrap();

            let config = self.editor.common.config.get_untracked();
            let line_height = config.editor.line_height() as f64;

            let is_local = e_data.doc().content.with_untracked(|c| c.is_local());

            let width = editor.max_line_width() + 10.0;
            let width = if !is_local {
                width.max(viewport_size.width)
            } else {
                width
            };
            let last_vline = editor.last_vline().get();
            let last_vline = e_data.visual_line(last_vline);
            let last_line_height = line_height * (last_vline + 1) as f64;
            let height = last_line_height.max(line_height);
            let height = if !is_local {
                height.max(viewport_size.height)
            } else {
                height
            };

            let margin_bottom = if !is_local
                && editor
                    .es
                    .with_untracked(EditorStyle::scroll_beyond_last_line)
            {
                viewport_size.height.min(last_line_height) - line_height
            } else {
                0.0
            };

            let style = Style::new()
                .width(width)
                .height(height)
                .margin_bottom(margin_bottom)
                .to_taffy_style();
            self.id.set_taffy_style(inner_node, style);

            vec![inner_node]
        })
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        let viewport = cx.current_viewport();
        if self.viewport.with_untracked(|v| v != &viewport) {
            self.viewport.set(viewport);
        }
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let viewport = self.viewport.get_untracked();
        let e_data = &self.editor;
        let ed = &e_data.editor;
        let config = e_data.common.config.get_untracked();
        let doc = e_data.doc();
        let is_local = doc.content.with_untracked(|content| content.is_local());

        // We repeatedly get the screen lines because we don't currently carefully manage the
        // paint functions to avoid potentially needing to recompute them, which could *maybe*
        // make them invalid.
        // TODO: One way to get around the above issue would be to more careful, since we
        // technically don't need to stop it from *recomputing* just stop any possible changes, but
        // avoiding recomputation seems easiest/clearest.
        // I expect that most/all of the paint functions could restrict themselves to only what is
        // within the active screen lines without issue.
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_cursor(cx, is_local, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_diff_sections(cx, viewport, &screen_lines, &config);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_find(cx, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_bracket_highlights_scope_lines(cx, viewport, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        FloemEditorView::paint_text(cx, ed, viewport, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_sticky_headers(cx, viewport, &screen_lines);
        self.paint_scroll_bar(cx, viewport, is_local, config);
    }
}

fn get_sticky_header_info(
    editor_data: &EditorData,
    viewport: RwSignal<Rect>,
    sticky_header_height_signal: RwSignal<f64>,
    config: &LapceConfig,
) -> StickyHeaderInfo {
    let editor = &editor_data.editor;
    let doc = editor_data.doc();

    let viewport = viewport.get();
    // TODO(minor): should this be a `get`
    let screen_lines = editor.screen_lines.get();
    let line_height = config.editor.line_height() as f64;
    // let start_line = (viewport.y0 / line_height).floor() as usize;
    let Some(start) = screen_lines.lines.first() else {
        return StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        };
    };
    let start_info = screen_lines.info(*start).unwrap();
    let start_line = start_info.vline_info.rvline.line;

    let y_diff = viewport.y0 - start_info.vline_y;

    let mut last_sticky_should_scroll = false;
    let mut sticky_lines = Vec::new();
    if let Some(lines) = doc.sticky_headers(start_line) {
        let total_lines = lines.len();
        if total_lines > 0 {
            let line = start_line + total_lines;
            if let Some(new_lines) = doc.sticky_headers(line) {
                if new_lines.len() > total_lines {
                    sticky_lines = new_lines;
                } else {
                    sticky_lines = lines;
                    last_sticky_should_scroll = new_lines.len() < total_lines;
                    if new_lines.len() < total_lines {
                        if let Some(new_new_lines) =
                            doc.sticky_headers(start_line + total_lines - 1)
                        {
                            if new_new_lines.len() < total_lines {
                                sticky_lines.pop();
                                last_sticky_should_scroll = false;
                            }
                        } else {
                            sticky_lines.pop();
                            last_sticky_should_scroll = false;
                        }
                    }
                }
            } else {
                sticky_lines = lines;
                last_sticky_should_scroll = true;
            }
        }
    }

    let total_sticky_lines = sticky_lines.len();

    let paint_last_line = total_sticky_lines > 0
        && (last_sticky_should_scroll
            || y_diff != 0.0
            || start_line + total_sticky_lines - 1 != *sticky_lines.last().unwrap());

    // Fix up the line count in case we don't need to paint the last one.
    let total_sticky_lines = if paint_last_line {
        total_sticky_lines
    } else {
        total_sticky_lines.saturating_sub(1)
    };

    if total_sticky_lines == 0 {
        sticky_header_height_signal.set(0.0);
        return StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        };
    }

    let scroll_offset = if last_sticky_should_scroll {
        y_diff
    } else {
        0.0
    };

    let sticky_header_height = sticky_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // TODO(question): won't y_diff always be scroll_offset here? so we should just sub on
            // the outside
            let y_diff = if i == total_sticky_lines - 1 {
                scroll_offset
            } else {
                0.0
            };

            let layout = editor.text_layout(*line);
            layout.line_count() as f64 * line_height - y_diff
        })
        .sum();

    sticky_header_height_signal.set(sticky_header_height);
    StickyHeaderInfo {
        sticky_lines,
        last_sticky_should_scroll,
        y_diff,
    }
}

pub fn editor_container_view(
    window_tab_data: Rc<WindowTabData>,
    workspace: Arc<LapceWorkspace>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (editor_id, find_focus, sticky_header_height, editor_view, config) = editor
        .with_untracked(|editor| {
            (
                editor.id(),
                editor.find_focus,
                editor.sticky_header_height,
                editor.kind,
                editor.common.config,
            )
        });

    let main_split = window_tab_data.main_split.clone();
    let editors = main_split.editors;
    let scratch_docs = main_split.scratch_docs;
    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;
    let debug_breakline = window_tab_data.terminal.breakline;

    stack((
        editor_breadcrumbs(workspace, editor.get_untracked(), config),
        container(
            stack((
                editor_gutter(window_tab_data.clone(), editor, is_active),
                container(editor_content(editor, debug_breakline, is_active))
                    .style(move |s| s.size_pct(100.0, 100.0)),
                empty().style(move |s| {
                    let config = config.get();
                    s.absolute()
                        .width_pct(100.0)
                        .height(sticky_header_height.get() as f32)
                        // .box_shadow_blur(5.0)
                        // .border_bottom(1.0)
                        // .border_color(
                        //     config.get_color(LapceColor::LAPCE_BORDER),
                        // )
                        .apply_if(
                            !config.editor.sticky_header
                                || sticky_header_height.get() == 0.0
                                || !editor_view.get().is_normal(),
                            |s| s.hide(),
                        )
                }),
                find_view(
                    editor,
                    find_editor,
                    find_focus,
                    replace_editor,
                    replace_active,
                    replace_focus,
                    is_active,
                ),
            ))
            .style(|s| s.absolute().size_full()),
        )
        .style(|s| s.width_full().flex_basis(0).flex_grow(1.0)),
    ))
    .on_cleanup(move || {
        let editor = editor.get_untracked();
        editor.cancel_completion();
        editor.cancel_inline_completion();
        if editors.contains_untracked(editor_id) {
            // editor still exist, so it might be moved to a different editor tab
            return;
        }
        let doc = editor.doc();
        editor.scope.dispose();

        let scratch_doc_name =
            if let DocContent::Scratch { name, .. } = doc.content.get_untracked() {
                Some(name.to_string())
            } else {
                None
            };
        if let Some(name) = scratch_doc_name {
            if !scratch_docs
                .with_untracked(|scratch_docs| scratch_docs.contains_key(&name))
            {
                doc.scope.dispose();
            }
        }
    })
    .style(|s| s.flex_col().absolute().size_pct(100.0, 100.0))
}

fn editor_gutter(
    window_tab_data: Rc<WindowTabData>,
    e_data: RwSignal<EditorData>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let breakpoints = window_tab_data.terminal.debug.breakpoints;
    let daps = window_tab_data.terminal.debug.daps;

    let padding_left = 25.0;
    let padding_right = 30.0;

    let (ed, doc, config) = e_data
        .with_untracked(|e| (e.editor.clone(), e.doc_signal(), e.common.config));
    let cursor = ed.cursor;
    let viewport = ed.viewport;
    let scroll_delta = ed.scroll_delta;
    let screen_lines = ed.screen_lines;

    let num_display_lines = create_memo(move |_| {
        let screen_lines = screen_lines.get();
        screen_lines.lines.len()
        // let viewport = viewport.get();
        // let line_height = config.get().editor.line_height() as f64;
        // (viewport.height() / line_height).ceil() as usize + 1
    });

    let code_action_vline = create_memo(move |_| {
        if is_active(true) {
            let doc = doc.get();
            let (offset, affinity) =
                cursor.with(|cursor| (cursor.offset(), cursor.affinity));
            let has_code_actions = doc
                .code_actions()
                .with(|c| c.get(&offset).map(|c| !c.1.is_empty()).unwrap_or(false));
            if has_code_actions {
                let vline = ed.vline_of_offset(offset, affinity);
                Some(vline)
            } else {
                None
            }
        } else {
            None
        }
    });

    let gutter_rect = create_rw_signal(Rect::ZERO);
    let gutter_width = create_memo(move |_| gutter_rect.get().width());

    let breakpoints_view = move |i: usize| {
        let hovered = create_rw_signal(false);
        container(
            svg(move || config.get().ui_svg(LapceIcons::DEBUG_BREAKPOINT)).style(
                move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32 + 2.0;
                    s.size(size, size)
                        .color(config.color(LapceColor::DEBUG_BREAKPOINT_HOVER))
                        .apply_if(!hovered.get(), |s| s.hide())
                },
            ),
        )
        .on_click_stop(move |_| {
            let screen_lines = screen_lines.get_untracked();
            let line = screen_lines.lines.get(i).map(|r| r.line).unwrap_or(0);
            // let line = (viewport.get_untracked().y0
            //     / config.get_untracked().editor.line_height() as f64)
            //     .floor() as usize
            //     + i;
            let e_data = e_data.get_untracked();
            let doc = e_data.doc();
            let offset = doc.buffer.with_untracked(|b| b.offset_of_line(line));
            if let Some(path) = doc.content.get_untracked().path() {
                let path_breakpoints = breakpoints
                    .try_update(|breakpoints| {
                        let breakpoints =
                            breakpoints.entry(path.clone()).or_default();
                        if let std::collections::btree_map::Entry::Vacant(e) =
                            breakpoints.entry(line)
                        {
                            e.insert(LapceBreakpoint {
                                id: None,
                                verified: false,
                                message: None,
                                line,
                                offset,
                                dap_line: None,
                                active: true,
                            });
                        } else {
                            let mut toggle_active = false;
                            if let Some(breakpint) = breakpoints.get_mut(&line) {
                                if !breakpint.active {
                                    breakpint.active = true;
                                    toggle_active = true;
                                }
                            }
                            if !toggle_active {
                                breakpoints.remove(&line);
                            }
                        }
                        breakpoints.clone()
                    })
                    .unwrap();
                let source_breakpoints: Vec<SourceBreakpoint> = path_breakpoints
                    .iter()
                    .filter_map(|(_, b)| {
                        if b.active {
                            Some(SourceBreakpoint {
                                line: b.line + 1,
                                column: None,
                                condition: None,
                                hit_condition: None,
                                log_message: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let daps: Vec<DapId> =
                    daps.with_untracked(|daps| daps.keys().cloned().collect());
                for dap_id in daps {
                    e_data.common.proxy.dap_set_breakpoints(
                        dap_id,
                        path.to_path_buf(),
                        source_breakpoints.clone(),
                    );
                }
            }
        })
        .on_event_stop(EventListener::PointerEnter, move |_| {
            hovered.set(true);
        })
        .on_event_stop(EventListener::PointerLeave, move |_| {
            hovered.set(false);
        })
        .style(move |s| {
            s.width(padding_left)
                .height(config.get().editor.line_height() as f32)
                .justify_center()
                .items_center()
                .cursor(CursorStyle::Pointer)
        })
    };

    stack((
        stack((
            empty().style(move |s| s.width(padding_left)),
            label(move || {
                let doc = doc.get();
                doc.buffer.with(|b| b.last_line() + 1).to_string()
            }),
            empty().style(move |s| s.width(padding_right)),
        ))
        .style(|s| s.height_pct(100.0)),
        clip(
            stack((
                dyn_stack(
                    move || {
                        let num = num_display_lines.get();
                        0..num
                    },
                    move |i| *i,
                    breakpoints_view,
                )
                .style(move |s| {
                    s.absolute().flex_col().margin_top(
                        -(viewport.get().y0
                            % config.get().editor.line_height() as f64)
                            as f32,
                    )
                }),
                dyn_stack(
                    move || {
                        let e_data = e_data.get();
                        let doc = e_data.doc_signal().get();
                        let content = doc.content.get();
                        let breakpoints = if let Some(path) = content.path() {
                            breakpoints
                                .with(|b| b.get(path).cloned())
                                .unwrap_or_default()
                        } else {
                            Default::default()
                        };
                        breakpoints.into_iter()
                    },
                    move |(line, b)| (*line, b.active),
                    move |(line, breakpoint)| {
                        let active = breakpoint.active;
                        let line_y = screen_lines
                            .with_untracked(|s| s.info_for_line(line))
                            .map(|l| l.y)
                            .unwrap_or_default();
                        container(
                            svg(move || {
                                config.get().ui_svg(LapceIcons::DEBUG_BREAKPOINT)
                            })
                            .style(move |s| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32 + 2.0;
                                let color = if active {
                                    LapceColor::DEBUG_BREAKPOINT
                                } else {
                                    LapceColor::EDITOR_DIM
                                };
                                let color = config.color(color);
                                s.size(size, size).color(color)
                            }),
                        )
                        .style(move |s| {
                            let config = config.get();
                            s.absolute()
                                .width(padding_left)
                                .height(config.editor.line_height() as f32)
                                .justify_center()
                                .items_center()
                                .margin_top(line_y as f32 - viewport.get().y0 as f32)
                        })
                    },
                )
                .style(|s| s.absolute().size_pct(100.0, 100.0)),
            ))
            .style(|s| s.size_pct(100.0, 100.0)),
        )
        .style(move |s| {
            s.absolute()
                .size_pct(100.0, 100.0)
                .background(config.get().color(LapceColor::EDITOR_BACKGROUND))
        }),
        clip(
            stack((
                editor_gutter_view(e_data.get_untracked())
                    .on_resize(move |rect| {
                        gutter_rect.set(rect);
                    })
                    .on_event_stop(EventListener::PointerWheel, move |event| {
                        if let Event::PointerWheel(pointer_event) = event {
                            scroll_delta.set(pointer_event.delta);
                        }
                    })
                    .style(|s| s.size_pct(100.0, 100.0)),
                container(
                    svg(move || config.get().ui_svg(LapceIcons::LIGHTBULB)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .color(config.color(LapceColor::LAPCE_WARN))
                        },
                    ),
                )
                .on_click_stop(move |_| {
                    e_data.get_untracked().show_code_actions(true);
                })
                .style(move |s| {
                    let config = config.get();
                    let viewport = viewport.get();
                    let gutter_width = gutter_width.get();
                    let code_action_vline = code_action_vline.get();
                    let size = config.ui.icon_size() as f32;
                    let margin_left =
                        gutter_width as f32 + (padding_right - size) / 2.0 - 4.0;
                    let line_height = config.editor.line_height();
                    let margin_top = if let Some(vline) = code_action_vline {
                        (vline.get() * line_height) as f32 - viewport.y0 as f32
                            + (line_height as f32 - size) / 2.0
                            - 4.0
                    } else {
                        0.0
                    };
                    s.absolute()
                        .padding(4.0)
                        .border_radius(6.0)
                        .margin_left(margin_left)
                        .margin_top(margin_top)
                        .apply_if(code_action_vline.is_none(), |s| s.hide())
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .active(|s| {
                            s.background(
                                config.color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ),
                            )
                        })
                }),
            ))
            .style(|s| s.size_pct(100.0, 100.0)),
        )
        .style(move |s| {
            s.absolute()
                .size_pct(100.0, 100.0)
                .padding_left(padding_left)
                .padding_right(padding_right)
        }),
    ))
    .style(|s| s.height_pct(100.0))
}

fn editor_breadcrumbs(
    workspace: Arc<LapceWorkspace>,
    e_data: EditorData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let doc = e_data.doc_signal();
    let doc_path = create_memo(move |_| {
        let doc = doc.get();
        let content = doc.content.get();
        if let DocContent::History(history) = &content {
            Some(history.path.clone())
        } else {
            content.path().cloned()
        }
    });
    container(
        scroll(
            stack((
                {
                    let workspace = workspace.clone();
                    dyn_stack(
                        move || {
                            let full_path = doc_path.get().unwrap_or_default();
                            let mut path = full_path;
                            if let Some(workspace_path) =
                                workspace.clone().path.as_ref()
                            {
                                path = path
                                    .strip_prefix(workspace_path)
                                    .unwrap_or(&path)
                                    .to_path_buf();
                            }
                            path.ancestors()
                                .filter_map(|path| {
                                    Some(
                                        path.file_name()?
                                            .to_string_lossy()
                                            .into_owned(),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .enumerate()
                        },
                        |(i, section)| (*i, section.to_string()),
                        move |(i, section)| {
                            stack((
                                svg(move || {
                                    config
                                        .get()
                                        .ui_svg(LapceIcons::BREADCRUMB_SEPARATOR)
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    s.apply_if(i == 0, |s| s.hide())
                                        .size(size, size)
                                        .color(
                                            config.color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                }),
                                label(move || section.clone()),
                            ))
                            .style(|s| s.items_center())
                        },
                    )
                    .style(|s| s.padding_horiz(10.0))
                },
                label(move || {
                    let doc = doc.get();
                    if let DocContent::History(history) = doc.content.get() {
                        format!("({})", history.version)
                    } else {
                        "".to_string()
                    }
                })
                .style(move |s| {
                    let doc = doc.get();
                    let is_history = doc.content.with_untracked(|content| {
                        matches!(content, DocContent::History(_))
                    });

                    s.padding_right(10.0).apply_if(!is_history, |s| s.hide())
                }),
            ))
            .style(|s| s.items_center()),
        )
        .scroll_to(move || {
            doc.track();
            Some(Point::new(3000.0, 0.0))
        })
        .style(move |s| {
            s.set(HideBar, true)
                .absolute()
                .size_pct(100.0, 100.0)
                .border_bottom(1.0)
                .border_color(config.get().color(LapceColor::LAPCE_BORDER))
                .items_center()
        }),
    )
    .style(move |s| {
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        s.items_center()
            .width_pct(100.0)
            .height(line_height as f32)
            .apply_if(doc_path.get().is_none(), |s| s.hide())
    })
}

fn editor_content(
    e_data: RwSignal<EditorData>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let (
        cursor,
        scroll_delta,
        scroll_to,
        window_origin,
        viewport,
        sticky_header_height,
        config,
        editor,
    ) = e_data.with_untracked(|editor| {
        (
            editor.cursor().read_only(),
            editor.scroll_delta().read_only(),
            editor.scroll_to(),
            editor.window_origin(),
            editor.viewport(),
            editor.sticky_header_height,
            editor.common.config,
            editor.editor.clone(),
        )
    });

    {
        create_effect(move |_| {
            is_active(true);
            let e_data = e_data.get_untracked();
            e_data.cancel_completion();
            e_data.cancel_inline_completion();
        });
    }

    scroll({
        let editor_content_view =
            editor_view(e_data.get_untracked(), debug_breakline, is_active).style(
                move |s| s.absolute().min_size_full().cursor(CursorStyle::Text),
            );

        let id = editor_content_view.id();
        editor.editor_view_id.set(Some(id));

        let editor2 = editor.clone();
        editor_content_view
            .on_event_cont(EventListener::FocusGained, move |_| {
                editor.editor_view_focused.notify();
            })
            .on_event_cont(EventListener::FocusLost, move |_| {
                editor2.editor_view_focus_lost.notify();
            })
            .on_event_cont(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    e_data.get_untracked().pointer_down(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    e_data.get_untracked().pointer_move(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerUp, move |event| {
                if let Event::PointerUp(pointer_event) = event {
                    e_data.get_untracked().pointer_up(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerLeave, move |event| {
                if let Event::PointerLeave = event {
                    e_data.get_untracked().pointer_leave();
                }
            })
    })
    .on_move(move |point| {
        window_origin.set(point);
    })
    .on_scroll(move |_| {
        let e_data = e_data.get_untracked();
        e_data.cancel_completion();
        e_data.cancel_inline_completion();
    })
    .scroll_to(move || scroll_to.get().map(|s| s.to_point()))
    .scroll_delta(move || scroll_delta.get())
    .ensure_visible(move || {
        let e_data = e_data.get_untracked();
        let cursor = cursor.get();
        let offset = cursor.offset();
        e_data.doc_signal().track();
        e_data.kind.track();

        let LineRegion { x, width, rvline } = cursor_caret(
            &e_data.editor,
            offset,
            !cursor.is_insert(),
            cursor.affinity,
        );
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        // TODO: is there a good way to avoid the calculation of the vline here?
        let vline = e_data.editor.vline_of_rvline(rvline);
        let vline = e_data.visual_line(vline.get());
        let rect = Rect::from_origin_size(
            (x, (vline * line_height) as f64),
            (width, line_height as f64),
        )
        .inflate(10.0, 0.0);

        let viewport = viewport.get_untracked();
        let smallest_distance = (viewport.y0 - rect.y0)
            .abs()
            .min((viewport.y1 - rect.y0).abs())
            .min((viewport.y0 - rect.y1).abs())
            .min((viewport.y1 - rect.y1).abs());
        let biggest_distance = (viewport.y0 - rect.y0)
            .abs()
            .max((viewport.y1 - rect.y0).abs())
            .max((viewport.y0 - rect.y1).abs())
            .max((viewport.y1 - rect.y1).abs());
        let jump_to_middle = biggest_distance > viewport.height()
            && smallest_distance > viewport.height() / 2.0;

        if jump_to_middle {
            rect.inflate(0.0, viewport.height() / 2.0)
        } else {
            let cursor_surrounding_lines =
                e_data.editor.es.with(|s| s.cursor_surrounding_lines());
            let mut rect = rect;
            rect.y0 -= (cursor_surrounding_lines * line_height) as f64
                + sticky_header_height.get_untracked();
            rect.y1 += (cursor_surrounding_lines * line_height) as f64;
            rect
        }
    })
    .style(|s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .set(PropagatePointerWheel, false)
    })
}

fn search_editor_view(
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    replace_focus: RwSignal<bool>,
) -> impl View {
    let config = find_editor.common.config;

    let case_matching = find_editor.common.find.case_matching;
    let whole_word = find_editor.common.find.whole_words;
    let is_regex = find_editor.common.find.is_regex;
    let visual = find_editor.common.find.visual;

    stack((
        TextInputBuilder::new()
            .is_focused(move || {
                is_active(true)
                    && visual.get()
                    && find_focus.get()
                    && !replace_focus.get()
            })
            .build_editor(find_editor)
            .on_event_cont(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(false);
            })
            .style(|s| s.width_pct(100.0)),
        clickable_icon(
            || LapceIcons::SEARCH_CASE_SENSITIVE,
            move || {
                let new = match case_matching.get_untracked() {
                    CaseMatching::Exact => CaseMatching::CaseInsensitive,
                    CaseMatching::CaseInsensitive => CaseMatching::Exact,
                };
                case_matching.set(new);
            },
            move || case_matching.get() == CaseMatching::Exact,
            || false,
            || "Case Sensitive",
            config,
        )
        .style(|s| s.padding_vert(4.0)),
        clickable_icon(
            || LapceIcons::SEARCH_WHOLE_WORD,
            move || {
                whole_word.update(|whole_word| {
                    *whole_word = !*whole_word;
                });
            },
            move || whole_word.get(),
            || false,
            || "Whole Word",
            config,
        )
        .style(|s| s.padding_left(6.0)),
        clickable_icon(
            || LapceIcons::SEARCH_REGEX,
            move || {
                is_regex.update(|is_regex| {
                    *is_regex = !*is_regex;
                });
            },
            move || is_regex.get(),
            || false,
            || "Use Regex",
            config,
        )
        .style(|s| s.padding_horiz(6.0)),
    ))
    .style(move |s| {
        let config = config.get();
        s.width(200.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn replace_editor_view(
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    find_focus: RwSignal<bool>,
) -> impl View {
    let config = replace_editor.common.config;
    let visual = replace_editor.common.find.visual;

    stack((
        TextInputBuilder::new()
            .is_focused(move || {
                is_active(true)
                    && visual.get()
                    && find_focus.get()
                    && replace_active.get()
                    && replace_focus.get()
            })
            .build_editor(replace_editor)
            .on_event_cont(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(true);
            })
            .style(|s| s.width_pct(100.0)),
        empty().style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32 + 10.0;
            s.size(0.0, size).padding_vert(4.0)
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.width(200.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn find_view(
    editor: RwSignal<EditorData>,
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let common = find_editor.common.clone();
    let config = common.config;
    let find_visual = common.find.visual;
    let replace_doc = replace_editor.doc_signal();
    let focus = common.focus;

    let find_pos = create_memo(move |_| {
        let visual = find_visual.get();
        if !visual {
            return (0, 0);
        }
        let editor = editor.get_untracked();
        let cursor = editor.cursor();
        let offset = cursor.with(|cursor| cursor.offset());
        let occurrences = editor.doc_signal().get().find_result.occurrences;
        occurrences.with(|occurrences| {
            for (i, region) in occurrences.regions().iter().enumerate() {
                if offset <= region.max() {
                    return (i + 1, occurrences.regions().len());
                }
            }
            (occurrences.regions().len(), occurrences.regions().len())
        })
    });

    container(
        stack((
            stack((
                clickable_icon(
                    move || {
                        if replace_active.get() {
                            LapceIcons::ITEM_OPENED
                        } else {
                            LapceIcons::ITEM_CLOSED
                        }
                    },
                    move || {
                        replace_active.update(|active| *active = !*active);
                    },
                    move || false,
                    || false,
                    || "Toggle Replace",
                    config,
                )
                .style(|s| s.padding_horiz(6.0)),
                search_editor_view(
                    find_editor,
                    find_focus,
                    is_active,
                    replace_focus,
                ),
                label(move || {
                    let (current, all) = find_pos.get();
                    if all == 0 {
                        "No Results".to_string()
                    } else {
                        format!("{current} of {all}")
                    }
                })
                .style(|s| s.margin_left(6.0).min_width(70.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_BACKWARD,
                    move || {
                        editor.get_untracked().search_backward(Modifiers::empty());
                    },
                    move || false,
                    || false,
                    || "Previous Match",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_FORWARD,
                    move || {
                        editor.get_untracked().search_forward(Modifiers::empty());
                    },
                    move || false,
                    || false,
                    || "Next Match",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        editor.get_untracked().clear_search();
                    },
                    move || false,
                    || false,
                    || "Close",
                    config,
                )
                .style(|s| s.padding_horiz(6.0)),
            ))
            .style(|s| s.items_center()),
            stack((
                empty().style(move |s| {
                    let config = config.get();
                    let width = config.ui.icon_size() as f32 + 10.0 + 6.0 * 2.0;
                    s.width(width)
                }),
                replace_editor_view(
                    replace_editor,
                    replace_active,
                    replace_focus,
                    is_active,
                    find_focus,
                ),
                clickable_icon(
                    || LapceIcons::SEARCH_REPLACE,
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_next(&text);
                    },
                    move || false,
                    || false,
                    || "Replace Next",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_REPLACE_ALL,
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_all(&text);
                    },
                    move || false,
                    || false,
                    || "Replace All",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
            ))
            .style(move |s| {
                s.items_center()
                    .margin_top(4.0)
                    .apply_if(!replace_active.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.margin_right(50.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border_radius(6.0)
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .padding_vert(4.0)
                .cursor(CursorStyle::Default)
                .flex_col()
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            // Shift the editor tab focus to the editor the find search is attached to
            // So that if you have two tabs open side-by-side (and thus two find views),
            // clicking on one will shift the focus to the editor it's attached to
            let editor = editor.get_untracked();
            if let Some(editor_tab_id) = editor.editor_tab_id.get_untracked() {
                editor
                    .common
                    .internal_command
                    .send(InternalCommand::FocusEditorTab { editor_tab_id });
            }
            focus.set(Focus::Workbench);
            // Request focus on the app view, as our current method of dispatching pointer events
            // is from the app_view to the actual editor. That's also why this stops the pointer
            // event is stopped here, as otherwise our default handling would make it go through to
            // the editor.
            common
                .window_common
                .app_view_id
                .get_untracked()
                .request_focus();
        }),
    )
    .style(move |s| {
        s.absolute()
            .margin_top(-1.0)
            .width_pct(100.0)
            .justify_end()
            .apply_if(!find_visual.get(), |s| s.hide())
    })
}

/// Iterator over (len, color, modified) for each change in the diff
fn changes_color_iter<'a>(
    changes: &'a im::Vector<DiffLines>,
    config: &'a LapceConfig,
) -> impl Iterator<Item = (usize, Option<Color>, bool)> + 'a {
    let mut last_change = None;
    changes.iter().map(move |change| {
        let len = match change {
            DiffLines::Left(_range) => 0,
            DiffLines::Both(info) => info.right.len(),
            DiffLines::Right(range) => range.len(),
        };
        let mut modified = false;
        let color = match change {
            DiffLines::Left(_range) => {
                Some(config.color(LapceColor::SOURCE_CONTROL_REMOVED))
            }
            DiffLines::Right(_range) => {
                if let Some(DiffLines::Left(_)) = last_change.as_ref() {
                    modified = true;
                }
                if modified {
                    Some(config.color(LapceColor::SOURCE_CONTROL_MODIFIED))
                } else {
                    Some(config.color(LapceColor::SOURCE_CONTROL_ADDED))
                }
            }
            _ => None,
        };

        last_change = Some(change.clone());

        (len, color, modified)
    })
}

// TODO: both of the changes color functions could easily return iterators

/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_screen(
    config: &LapceConfig,
    editor: &Editor,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let screen_lines = editor.screen_lines.get_untracked();

    let Some((min, max)) = screen_lines.rvline_range() else {
        return Vec::new();
    };

    let line_height = config.editor.line_height();
    let mut line = 0;
    let mut colors = Vec::new();

    for (len, color, modified) in changes_color_iter(&changes, config) {
        let pre_line = line;

        line += len;
        if line < min.line {
            continue;
        }

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            let rvline = editor.rvline_of_line(pre_line);
            let vline = editor.vline_of_line(pre_line);
            let y = (vline.0 * line_height) as f64;
            let height = {
                // Accumulate the number of line indices each potentially wrapped line spans
                let end_line = rvline.line + len;

                editor.iter_rvlines_over(false, rvline, end_line).count()
            };
            let removed = len == 0;

            colors.push((y, height, removed, color));
        }

        if line > max.line {
            break;
        }
    }

    colors
}

// TODO: limit the visual line that changes are considered past to some reasonable number
// TODO(minor): This could be a `changes_colors_range` with some minor changes, but it isn't needed
/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_all(
    config: &LapceConfig,
    ed: &Editor,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let line_height = config.editor.line_height();

    let mut line = 0;
    let mut colors = Vec::new();

    let mut vline_iter = ed.iter_vlines(false, VLine(0)).peekable();

    for (len, color, modified) in changes_color_iter(&changes, config) {
        let pre_line = line;

        line += len;

        // Skip over all vlines that are before the current line
        vline_iter
            .by_ref()
            .peeking_take_while(|info| info.rvline.line < pre_line)
            .count();

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            // Find the info with a line == pre_line
            let Some(info) = vline_iter.peek() else {
                continue;
            };

            let y = info.vline.get() * line_height;
            let end_line = info.rvline.line + len;
            let height = vline_iter
                .by_ref()
                .peeking_take_while(|info| info.rvline.line < end_line)
                .count();
            let removed = len == 0;

            colors.push((y as f64, height, removed, color));
        }

        if vline_iter.peek().is_none() {
            break;
        }
    }

    colors
}
