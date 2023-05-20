use std::sync::{atomic::AtomicU64, Arc};

use floem::{
    context::PaintCx,
    event::{Event, EventListner},
    glazier::{Modifiers, PointerType},
    id::Id,
    peniko::{
        kurbo::{BezPath, Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, ReadSignal, RwSignal,
        SignalGet, SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
        SignalWithUntracked,
    },
    style::{ComputedStyle, CursorStyle, Dimension, Style},
    taffy::prelude::Node,
    view::{ChangeFlags, View},
    views::{
        clip, container, empty, label, list, rich_text, scroll, stack, svg,
        virtual_list, Decorators, VirtualListDirection, VirtualListItemSize,
    },
    AppContext, Renderer,
};
use lapce_core::{
    cursor::{ColPosition, Cursor, CursorMode},
    mode::{Mode, VisualMode},
    selection::Selection,
};
use lapce_xi_rope::find::CaseMatching;

use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::{DocContent, DocLine, Document, LineExtraStyle},
    main_split::MainSplitData,
    text_input::text_input,
    wave::wave_line,
    workspace::LapceWorkspace,
};

use super::EditorData;

struct StickyHeaderInfo {
    sticky_lines: Vec<usize>,
    last_sticky_should_scroll: bool,
    y_diff: f64,
}

pub struct EditorContentView {
    id: Id,
    editor: RwSignal<EditorData>,
    is_active: Box<dyn Fn() -> bool + 'static>,
    inner_node: Option<Node>,
    viewport: RwSignal<Rect>,
    sticky_header_info: StickyHeaderInfo,
}

pub fn editor_content_view(
    editor: RwSignal<EditorData>,
    is_active: impl Fn() -> bool + 'static + Copy,
) -> EditorContentView {
    let cx = AppContext::get_current();
    let id = cx.new_id();

    let viewport = create_rw_signal(cx.scope, Rect::ZERO);

    create_effect(cx.scope, move |last_rev| {
        let (doc, sticky_header_height_signal, config) =
            editor.with_untracked(|editor| {
                (
                    editor.doc,
                    editor.sticky_header_height,
                    editor.common.config,
                )
            });
        let config = config.get();
        if !config.editor.sticky_header {
            return (DocContent::Local, 0, 0, Rect::ZERO);
        }

        let rect = viewport.get();
        let rev = doc.with(|doc| {
            (doc.content.clone(), doc.buffer().rev(), doc.style_rev, rect)
        });
        if last_rev.as_ref() == Some(&rev) {
            return rev;
        }

        let sticky_header_info = get_sticky_header_info(
            doc,
            viewport,
            sticky_header_height_signal,
            &config,
        );

        id.update_state(sticky_header_info, false);

        rev
    });

    EditorContentView {
        id,
        editor,
        is_active: Box::new(is_active),
        inner_node: None,
        viewport,
        sticky_header_info: StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        },
    }
}

impl EditorContentView {
    fn paint_cursor(&self, cx: &mut PaintCx, min_line: usize, max_line: usize) {
        let (doc, cursor, find_focus, config) =
            self.editor.with_untracked(|editor| {
                (
                    editor.doc,
                    editor.cursor,
                    editor.find_focus,
                    editor.common.config,
                )
            });

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let viewport = self.viewport.get_untracked();
        let is_active = (*self.is_active)() && !find_focus.get_untracked();

        let renders = doc.with(|doc| {
            cursor.with(|cursor| match &cursor.mode {
                CursorMode::Normal(offset) => {
                    let line = doc.buffer().line_of_offset(*offset);
                    let mut renders = vec![CursorRender::CurrentLine { line }];
                    if is_active {
                        let caret = cursor_caret(doc, *offset, true);
                        renders.push(caret);
                    }
                    renders
                }
                CursorMode::Visual { start, end, mode } => visual_cursor(
                    doc,
                    *start,
                    *end,
                    mode,
                    cursor.horiz.as_ref(),
                    min_line,
                    max_line,
                    7.5,
                    is_active,
                ),
                CursorMode::Insert(selection) => {
                    insert_cursor(doc, selection, min_line, max_line, 7.5, is_active)
                }
            })
        });

        for render in renders {
            match render {
                CursorRender::CurrentLine { line } => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(viewport.width(), line_height))
                            .with_origin(Point::new(
                                viewport.x0,
                                line_height * line as f64,
                            )),
                        config.get_color(LapceColor::EDITOR_CURRENT_LINE),
                    );
                }
                CursorRender::Selection { x, width, line } => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(width, line_height))
                            .with_origin(Point::new(x, line_height * line as f64)),
                        config.get_color(LapceColor::EDITOR_SELECTION),
                    );
                }
                CursorRender::Caret { x, width, line } => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(width, line_height))
                            .with_origin(Point::new(x, line_height * line as f64)),
                        config.get_color(LapceColor::EDITOR_CARET),
                    );
                }
            }
        }
    }

    fn paint_wave_line(
        &self,
        cx: &mut PaintCx,
        width: f64,
        point: Point,
        color: Color,
    ) {
        let radius = 2.0;
        let origin = Point::new(point.x, point.y + radius);
        let mut path = BezPath::new();
        path.move_to(origin);

        let mut x = 0.0;
        let mut direction = -1.0;
        while x < width {
            let point = origin + (x, 0.0);
            let p1 = point + (radius, -radius * direction);
            let p2 = point + (radius * 2.0, 0.0);
            path.quad_to(p1, p2);
            x += radius * 2.0;
            direction *= -1.0;
        }

        cx.stroke(&path, color, 1.0);
    }

    fn paint_extra_style(
        &self,
        cx: &mut PaintCx,
        extra_styles: &[LineExtraStyle],
        y: f64,
        height: f64,
        line_height: f64,
        viewport: Rect,
    ) {
        for style in extra_styles {
            if let Some(bg) = style.bg_color {
                let width = style.width.unwrap_or_else(|| viewport.width());
                cx.fill(
                    &Rect::ZERO.with_size(Size::new(width, height)).with_origin(
                        Point::new(style.x, y + (line_height - height) / 2.0),
                    ),
                    bg,
                );
            }

            if let Some(color) = style.wave_line {
                let width = style.width.unwrap_or_else(|| viewport.width());
                self.paint_wave_line(
                    cx,
                    width,
                    Point::new(style.x, y + (line_height - height) / 2.0 + height),
                    color,
                );
            }
        }
    }

    fn paint_text(
        &self,
        cx: &mut PaintCx,
        min_line: usize,
        max_line: usize,
        viewport: Rect,
    ) {
        let (doc, config) = self
            .editor
            .with_untracked(|editor| (editor.doc, editor.common.config));

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let font_size = config.editor.font_size();

        doc.with_untracked(|doc| {
            let last_line = doc.buffer().last_line();

            for line in min_line..max_line + 1 {
                if line > last_line {
                    break;
                }

                let text_layout = doc.get_text_layout(line, font_size);
                let height = text_layout.text.size().height;
                let y = line as f64 * line_height;

                self.paint_extra_style(
                    cx,
                    &text_layout.extra_style,
                    y,
                    height,
                    line_height,
                    viewport,
                );

                cx.draw_text(
                    &text_layout.text,
                    Point::new(0.0, y + (line_height - height) / 2.0),
                );
            }
        });
    }

    fn paint_find(&self, cx: &mut PaintCx, min_line: usize, max_line: usize) {
        let visual = self.editor.with_untracked(|e| e.common.find.visual);
        if !visual.get_untracked() {
            return;
        }

        let (doc, config) = self
            .editor
            .with_untracked(|editor| (editor.doc, editor.common.config));
        let occurrences = doc.with_untracked(|doc| doc.find_result.occurrences);

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        let (start, end) = doc.with_untracked(|doc| {
            doc.update_find(min_line, max_line);
            let start = doc.buffer().offset_of_line(min_line);
            let end = doc.buffer().offset_of_line(max_line + 1);
            (start, end)
        });

        let mut rects = Vec::new();
        for region in occurrences
            .with(|selection| selection.regions_in_range(start, end).to_vec())
        {
            doc.with_untracked(|doc| {
                let start = region.min();
                let end = region.max();
                let (start_line, start_col) = doc.buffer().offset_to_line_col(start);
                let (end_line, end_col) = doc.buffer().offset_to_line_col(end);
                for line in min_line..max_line + 1 {
                    if line < start_line {
                        continue;
                    }

                    if line > end_line {
                        break;
                    }

                    let left_col = match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    };
                    let (right_col, _line_end) = match line {
                        _ if line == end_line => {
                            let max_col = doc.buffer().line_end_col(line, true);
                            (end_col.min(max_col), false)
                        }
                        _ => (doc.buffer().line_end_col(line, true), true),
                    };

                    // Shift it by the inlay hints
                    let phantom_text = doc.line_phantom_text(line);
                    let left_col = phantom_text.col_after(left_col, false);
                    let right_col = phantom_text.col_after(right_col, false);

                    let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
                    let x1 = doc.line_point_of_line_col(line, right_col, 12).x;

                    if start != end {
                        rects.push(
                            Size::new(x1 - x0, line_height).to_rect().with_origin(
                                Point::new(x0, line_height * line as f64),
                            ),
                        );
                    }
                }
            })
        }

        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        for rect in rects {
            cx.stroke(&rect, color, 1.0);
        }
    }

    fn new_paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        start_line: usize,
        viewport: Rect,
    ) {
        let (doc, config) = self
            .editor
            .with_untracked(|editor| (editor.doc, editor.common.config));
        let config = config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }

        let line_height = config.editor.line_height() as f64;

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
        let area_height =
            total_sticky_lines as f64 * line_height - scroll_offset + 1.0;
        let sticky_area_rect = Size::new(viewport.width(), area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0));

        cx.fill(
            &sticky_area_rect,
            config.get_color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
        );

        doc.with_untracked(|doc| {
            // Paint lines
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

                cx.save();

                let line_area_rect =
                    Size::new(viewport.width(), line_height - y_diff)
                        .to_rect()
                        .with_origin(Point::new(
                            viewport.x0,
                            viewport.y0 + line_height * i as f64,
                        ));

                cx.clip(&line_area_rect);

                let text_layout =
                    doc.get_text_layout(line, config.editor.font_size());
                let y = viewport.y0
                    + line_height * i as f64
                    + (line_height - text_layout.text.size().height) / 2.0
                    - y_diff;
                cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

                cx.restore();
            }
        });
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        start_line: usize,
        viewport: Rect,
    ) {
        let (doc, sticky_header_height_signal, config) =
            self.editor.with_untracked(|editor| {
                (
                    editor.doc,
                    editor.sticky_header_height,
                    editor.common.config,
                )
            });
        let config = config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }

        let line_height = config.editor.line_height() as f64;

        let y_diff = viewport.y0 - start_line as f64 * line_height;

        let mut last_sticky_should_scroll = false;
        let mut sticky_lines = Vec::new();
        doc.with_untracked(|doc| {
            if let Some(lines) = doc.sticky_headers(start_line) {
                let total_lines = lines.len();
                if total_lines > 0 {
                    let line = start_line + total_lines;
                    if let Some(new_lines) = doc.sticky_headers(line) {
                        if new_lines.len() > total_lines {
                            sticky_lines = new_lines;
                        } else {
                            sticky_lines = lines;
                            last_sticky_should_scroll =
                                new_lines.len() < total_lines;
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
        });

        let total_sticky_lines = sticky_lines.len();

        let paint_last_line = total_sticky_lines > 0
            && (last_sticky_should_scroll
                || y_diff != 0.0
                || start_line + total_sticky_lines - 1
                    != *sticky_lines.last().unwrap());

        // Fix up the line count in case we don't need to paint the last one.
        let total_sticky_lines = if paint_last_line {
            total_sticky_lines
        } else {
            total_sticky_lines.saturating_sub(1)
        };

        if total_sticky_lines == 0 {
            sticky_header_height_signal.set(0.0);
            return;
        }

        let scroll_offset = if last_sticky_should_scroll {
            y_diff
        } else {
            0.0
        };

        // Clear background
        let area_height =
            total_sticky_lines as f64 * line_height - scroll_offset + 1.0;
        let sticky_area_rect = Size::new(viewport.width(), area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0));

        cx.fill(
            &sticky_area_rect,
            config.get_color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
        );

        let mut sticky_header_height = 0.0;
        doc.with_untracked(|doc| {
            // Paint lines
            for (i, line) in sticky_lines.iter().copied().enumerate() {
                let y_diff = if i == total_sticky_lines - 1 {
                    scroll_offset
                } else {
                    0.0
                };

                sticky_header_height += line_height - y_diff;

                cx.save();

                let line_area_rect =
                    Size::new(viewport.width(), line_height - y_diff)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            viewport.y0 + line_height * i as f64,
                        ));

                cx.clip(&line_area_rect);

                let text_layout =
                    doc.get_text_layout(line, config.editor.font_size());
                let y = viewport.y0
                    + line_height * i as f64
                    + (line_height - text_layout.text.size().height) / 2.0
                    - y_diff;
                cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

                cx.restore();
            }
        });
        sticky_header_height_signal.set(sticky_header_height);
    }
}

impl View for EditorContentView {
    fn id(&self) -> Id {
        self.id
    }

    fn child(&mut self, _id: floem::id::Id) -> Option<&mut dyn View> {
        None
    }

    fn children(&mut self) -> Vec<&mut dyn View> {
        Vec::new()
    }

    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) -> floem::view::ChangeFlags {
        if let Ok(state) = state.downcast() {
            self.sticky_header_info = *state;
            cx.request_layout(self.id);
            ChangeFlags::LAYOUT
        } else {
            ChangeFlags::default()
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            if self.inner_node.is_none() {
                self.inner_node = Some(cx.new_node());
            }
            let inner_node = self.inner_node.unwrap();

            let (doc, config) = self
                .editor
                .with_untracked(|editor| (editor.doc, editor.common.config));
            let config = config.get_untracked();
            let line_height = config.editor.line_height() as f64;
            let (width, height) = doc.with_untracked(|doc| {
                let width = doc.max_width.get_untracked();
                let height = line_height * doc.buffer().num_lines() as f64;
                (width as f32, height as f32)
            });

            let style = Style::BASE
                .width_px(width)
                .height_px(height)
                .compute(&ComputedStyle::default())
                .to_taffy_style();
            cx.set_style(inner_node, style);

            vec![inner_node]
        })
    }

    fn compute_layout(&mut self, cx: &mut floem::context::LayoutCx) {
        let viewport = cx.current_viewport().unwrap_or_default();
        if self.viewport.with_untracked(|v| v != &viewport) {
            self.viewport.set(viewport);
        }
    }

    fn event(
        &mut self,
        _cx: &mut floem::context::EventCx,
        _id_path: Option<&[floem::id::Id]>,
        _event: Event,
    ) -> bool {
        false
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let viewport = self.viewport.get_untracked();
        let config = self.editor.with_untracked(|e| e.common.config);
        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        self.paint_cursor(cx, min_line, max_line);
        self.paint_find(cx, min_line, max_line);
        self.paint_text(cx, min_line, max_line, viewport);
        self.new_paint_sticky_headers(cx, min_line, viewport);
    }
}

fn get_sticky_header_info(
    doc: RwSignal<Document>,
    viewport: RwSignal<Rect>,
    sticky_header_height_signal: RwSignal<f64>,
    config: &LapceConfig,
) -> StickyHeaderInfo {
    let viewport = viewport.get();
    let line_height = config.editor.line_height() as f64;
    let start_line = (viewport.y0 / line_height).floor() as usize;

    let y_diff = viewport.y0 - start_line as f64 * line_height;

    let mut last_sticky_should_scroll = false;
    let mut sticky_lines = Vec::new();
    doc.with_untracked(|doc| {
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
    });

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

    let mut sticky_header_height = 0.0;
    for (i, _line) in sticky_lines.iter().enumerate() {
        let y_diff = if i == total_sticky_lines - 1 {
            scroll_offset
        } else {
            0.0
        };

        sticky_header_height += line_height - y_diff;
    }

    sticky_header_height_signal.set(sticky_header_height);
    StickyHeaderInfo {
        sticky_lines,
        last_sticky_should_scroll,
        y_diff,
    }
}

#[derive(Clone, Debug)]
enum CursorRender {
    CurrentLine { line: usize },
    Selection { x: f64, width: f64, line: usize },
    Caret { x: f64, width: f64, line: usize },
}

fn cursor_caret(doc: &Document, offset: usize, block: bool) -> CursorRender {
    let (line, col) = doc.buffer().offset_to_line_col(offset);
    let phantom_text = doc.line_phantom_text(line);
    let col = phantom_text.col_after(col, block);
    let x0 = doc.line_point_of_line_col(line, col, 12).x;
    if block {
        let right_offset = doc.buffer().move_right(offset, Mode::Insert, 1);
        let (_, right_col) = doc.buffer().offset_to_line_col(right_offset);
        let x1 = doc.line_point_of_line_col(line, right_col, 12).x;

        let width = if x1 > x0 { x1 - x0 } else { 7.0 };
        CursorRender::Caret { x: x0, width, line }
    } else {
        CursorRender::Caret {
            x: x0 - 1.0,
            width: 2.0,
            line,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn visual_cursor(
    doc: &Document,
    start: usize,
    end: usize,
    mode: &VisualMode,
    horiz: Option<&ColPosition>,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let (start_line, start_col) = doc.buffer().offset_to_line_col(start.min(end));
    let (end_line, end_col) = doc.buffer().offset_to_line_col(start.max(end));
    let (cursor_line, _) = doc.buffer().offset_to_line_col(end);

    let mut renders = Vec::new();

    for line in min_line..max_line + 1 {
        if line < start_line {
            continue;
        }

        if line > end_line {
            break;
        }

        let left_col = match mode {
            VisualMode::Normal => {
                if start_line == line {
                    start_col
                } else {
                    0
                }
            }
            VisualMode::Linewise => 0,
            VisualMode::Blockwise => {
                let max_col = doc.buffer().line_end_col(line, false);
                let left = start_col.min(end_col);
                if left > max_col {
                    continue;
                }
                left
            }
        };

        let (right_col, line_end) = match mode {
            VisualMode::Normal => {
                if line == end_line {
                    let max_col = doc.buffer().line_end_col(line, true);

                    let end_offset =
                        doc.buffer().move_right(start.max(end), Mode::Visual, 1);
                    let (_, end_col) = doc.buffer().offset_to_line_col(end_offset);

                    (end_col.min(max_col), false)
                } else {
                    (doc.buffer().line_end_col(line, true), true)
                }
            }
            VisualMode::Linewise => (doc.buffer().line_end_col(line, true), true),
            VisualMode::Blockwise => {
                let max_col = doc.buffer().line_end_col(line, true);
                let right = match horiz.as_ref() {
                    Some(&ColPosition::End) => max_col,
                    _ => {
                        let end_offset =
                            doc.buffer().move_right(start.max(end), Mode::Visual, 1);
                        let (_, end_col) =
                            doc.buffer().offset_to_line_col(end_offset);
                        end_col.max(start_col).min(max_col)
                    }
                };
                (right, false)
            }
        };

        let phantom_text = doc.line_phantom_text(line);
        let left_col = phantom_text.col_after(left_col, false);
        let right_col = phantom_text.col_after(right_col, false);
        let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
        let mut x1 = doc.line_point_of_line_col(line, right_col, 12).x;
        if line_end {
            x1 += char_width;
        }

        renders.push(CursorRender::Selection {
            x: x0,
            width: x1 - x0,
            line,
        });

        if is_active && line == cursor_line {
            let caret = cursor_caret(doc, end, true);
            renders.push(caret);
        }
    }

    renders
}

fn insert_cursor(
    doc: &Document,
    selection: &Selection,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let start = doc.buffer().offset_of_line(min_line);
    let end = doc.buffer().offset_of_line(max_line + 1);
    let regions = selection.regions_in_range(start, end);

    let mut renders = Vec::new();

    for region in regions {
        let cursor_offset = region.end;
        let (cursor_line, _) = doc.buffer().offset_to_line_col(cursor_offset);
        let start = region.start;
        let end = region.end;
        let (start_line, start_col) =
            doc.buffer().offset_to_line_col(start.min(end));
        let (end_line, end_col) = doc.buffer().offset_to_line_col(start.max(end));
        for line in min_line..max_line + 1 {
            if line < start_line {
                continue;
            }

            if line > end_line {
                break;
            }

            let left_col = match line {
                _ if line == start_line => start_col,
                _ => 0,
            };
            let (right_col, line_end) = match line {
                _ if line == end_line => {
                    let max_col = doc.buffer().line_end_col(line, true);
                    (end_col.min(max_col), false)
                }
                _ => (doc.buffer().line_end_col(line, true), true),
            };

            // Shift it by the inlay hints
            let phantom_text = doc.line_phantom_text(line);
            let left_col = phantom_text.col_after(left_col, false);
            let right_col = phantom_text.col_after(right_col, false);

            let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
            let mut x1 = doc.line_point_of_line_col(line, right_col, 12).x;
            if line_end {
                x1 += char_width;
            }

            if line == cursor_line {
                renders.push(CursorRender::CurrentLine { line });
            }

            if start != end {
                renders.push(CursorRender::Selection {
                    x: x0,
                    width: x1 - x0,
                    line,
                });
            }

            if is_active && line == cursor_line {
                let caret = cursor_caret(doc, cursor_offset, false);
                renders.push(caret);
            }
        }
    }
    renders
}

pub fn editor_view(
    main_split: MainSplitData,
    workspace: Arc<LapceWorkspace>,
    is_active: impl Fn() -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (cursor, viewport, find_focus, sticky_header_height, config) = editor
        .with_untracked(|editor| {
            (
                editor.cursor.read_only(),
                editor.viewport,
                editor.find_focus,
                editor.sticky_header_height,
                editor.common.config,
            )
        });

    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;

    let cx = AppContext::get_current();
    let editor_rect = create_rw_signal(cx.scope, Rect::ZERO);
    let gutter_rect = create_rw_signal(cx.scope, Rect::ZERO);

    stack(move || {
        (
            editor_breadcrumbs(workspace, editor, config),
            container(|| {
                stack(|| {
                    (
                        editor_gutter(editor, is_active, gutter_rect),
                        editor_content(editor, is_active).style(move || {
                            let width = editor_rect.get().width()
                                - gutter_rect.get().width();
                            Style::BASE.height_pct(100.0).width_px(width as f32)
                        }),
                        empty().style(move || {
                            let config = config.get();
                            Style::BASE
                                .absolute()
                                .width_pct(100.0)
                                .height_px(sticky_header_height.get() as f32)
                                .border_bottom(1.0)
                                .border_color(
                                    *config.get_color(LapceColor::LAPCE_BORDER),
                                )
                                .apply_if(
                                    !config.editor.sticky_header
                                        || sticky_header_height.get() == 0.0,
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
                    )
                })
                .on_resize(move |_, rect| {
                    editor_rect.set(rect);
                })
                .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0)),
        )
    })
    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
}

fn editor_gutter(
    editor: RwSignal<EditorData>,
    is_active: impl Fn() -> bool + 'static + Copy,
    gutter_rect: RwSignal<Rect>,
) -> impl View {
    let (cursor, viewport, scroll_delta, config) = editor.with(|editor| {
        (
            editor.cursor,
            editor.viewport,
            editor.scroll_delta,
            editor.common.config,
        )
    });

    let padding_left = 10.0;
    let padding_right = 30.0;

    let cx = AppContext::get_current();
    let code_action_line = create_memo(cx.scope, move |_| {
        if is_active() {
            let doc = editor.with(|editor| editor.doc);
            let offset = cursor.with(|cursor| cursor.offset());
            doc.with(|doc| {
                let line = doc.buffer().line_of_offset(offset);
                let has_code_actions = doc
                    .code_actions
                    .get(&offset)
                    .map(|c| !c.1.is_empty())
                    .unwrap_or(false);
                if has_code_actions {
                    Some(line)
                } else {
                    None
                }
            })
        } else {
            None
        }
    });

    let current_line = create_memo(cx.scope, move |_| {
        let doc = editor.with(|editor| editor.doc);
        let (offset, mode) =
            cursor.with(|cursor| (cursor.offset(), cursor.get_mode()));
        let line = doc.with(|doc| {
            let line = doc.buffer().line_of_offset(offset);
            line
        });
        (line, mode)
    });

    stack(|| {
        (
            stack(|| {
                (
                    label(|| "".to_string())
                        .style(move || Style::BASE.width_px(padding_left)),
                    label(move || {
                        editor
                            .get()
                            .doc
                            .with(|doc| (doc.buffer().last_line() + 1).to_string())
                    }),
                    label(|| "".to_string())
                        .style(move || Style::BASE.width_px(padding_right)),
                )
            })
            .style(|| Style::BASE.height_pct(100.0)),
            scroll(|| {
                virtual_list(
                    VirtualListDirection::Vertical,
                    VirtualListItemSize::Fixed(Box::new(move || {
                        config.get_untracked().editor.line_height() as f64
                    })),
                    move || {
                        let editor = editor.get();
                        current_line.get();
                        editor.doc.get()
                    },
                    move |line: &DocLine| (line.line, current_line.get_untracked()),
                    move |line: DocLine| {
                        let line_number = {
                            let config = config.get_untracked();
                            let (current_line, mode) = current_line.get_untracked();
                            if config.core.modal
                                && config.editor.modal_mode_relative_line_numbers
                                && mode != Mode::Insert
                            {
                                if line.line == current_line {
                                    line.line + 1
                                } else {
                                    line.line.abs_diff(current_line)
                                }
                            } else {
                                line.line + 1
                            }
                        };

                        stack(move || {
                            (
                                label(|| "".to_string()).style(move || {
                                    Style::BASE.width_px(padding_left)
                                }),
                                label(move || line_number.to_string()).style(
                                    move || {
                                        let config = config.get();
                                        let (current_line, _) =
                                            current_line.get_untracked();
                                        Style::BASE
                                            .flex_grow(1.0)
                                            .apply_if(
                                                current_line != line.line,
                                                move |s| {
                                                    s.color(*config.get_color(
                                                        LapceColor::EDITOR_DIM,
                                                    ))
                                                },
                                            )
                                            .justify_end()
                                    },
                                ),
                                container(|| {
                                    container(|| {
                                        container(|| {
                                            svg(move || {
                                                config
                                                    .get()
                                                    .ui_svg(LapceIcons::LIGHTBULB)
                                            })
                                            .style(move || {
                                                let config = config.get();
                                                let size =
                                                    config.ui.icon_size() as f32;
                                                Style::BASE
                                                    .size_px(size, size)
                                                    .color(*config.get_color(
                                                        LapceColor::LAPCE_WARN,
                                                    ))
                                            })
                                        })
                                        .on_click(move |_| {
                                            editor.with_untracked(|editor| {
                                                editor.show_code_actions(true);
                                            });
                                            true
                                        })
                                        .style(move || {
                                            Style::BASE.apply_if(
                                                code_action_line.get()
                                                    != Some(line.line),
                                                |s| s.hide(),
                                            )
                                        })
                                    })
                                    .style(
                                        move || {
                                            Style::BASE
                                                .justify_center()
                                                .items_center()
                                                .width_px(
                                                    padding_right - padding_left,
                                                )
                                        },
                                    )
                                })
                                .style(move || {
                                    Style::BASE.justify_end().width_px(padding_right)
                                }),
                            )
                        })
                        .style(move || {
                            let config = config.get_untracked();
                            let line_height = config.editor.line_height();
                            Style::BASE.items_center().height_px(line_height as f32)
                        })
                    },
                )
                .style(move || {
                    let config = config.get();
                    let padding_bottom = if config.editor.scroll_beyond_last_line {
                        viewport.get().height() as f32
                            - config.editor.line_height() as f32
                    } else {
                        0.0
                    };
                    Style::BASE
                        .flex_col()
                        .width_pct(100.0)
                        .padding_bottom_px(padding_bottom)
                })
            })
            .hide_bar(|| true)
            .on_event(EventListner::PointerWheel, move |event| {
                if let Event::PointerWheel(pointer_event) = event {
                    if let PointerType::Mouse(info) = &pointer_event.pointer_type {
                        scroll_delta.set(info.wheel_delta);
                    }
                }
                true
            })
            .on_scroll_to(move || {
                let viewport = viewport.get();
                Some(viewport.origin())
            })
            .style(move || {
                Style::BASE
                    .absolute()
                    .background(
                        *config.get().get_color(LapceColor::EDITOR_BACKGROUND),
                    )
                    .size_pct(100.0, 100.0)
            }),
        )
    })
    .on_resize(move |_, rect| {
        gutter_rect.set(rect);
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .font_family(config.editor.font_family.clone())
            .font_size(config.editor.font_size() as f32)
    })
}

fn editor_cursor(
    editor: RwSignal<EditorData>,
    cursor: ReadSignal<Cursor>,
    viewport: ReadSignal<Rect>,
    is_active: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    {
        let cx = AppContext::get_current();
        create_effect(cx.scope, move |_| {
            let (doc, find) = editor.with(|e| (e.doc, e.common.find.clone()));
            if !find.visual.get() {
                return;
            }
            find.search_string.with(|_| ());
            find.case_matching.with(|_| ());
            find.whole_words.with(|_| ());
            let viewport = viewport.get();
            let config = config.get();
            let line_height = config.editor.line_height() as f64;
            let min_line = (viewport.y0 / line_height).floor() as usize;
            let max_line = (viewport.y1 / line_height).ceil() as usize;
            doc.with(|doc| {
                doc.update_find(min_line, max_line);
            });
        });
    }

    let search_rects = move || {
        let visual = editor.with_untracked(|e| e.common.find.visual);
        if !visual.get() {
            return Vec::new();
        }

        let doc = editor.with(|e| e.doc);
        let occurrences = doc.with_untracked(|doc| doc.find_result.occurrences);

        let viewport = viewport.get();
        let config = config.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;
        let (start, end) = doc.with_untracked(|doc| {
            let start = doc.buffer().offset_of_line(min_line);
            let end = doc.buffer().offset_of_line(max_line + 1);
            (start, end)
        });
        let mut rects = Vec::new();
        for region in occurrences
            .with(|selection| selection.regions_in_range(start, end).to_vec())
        {
            doc.with_untracked(|doc| {
                let start = region.min();
                let end = region.max();
                let (start_line, start_col) = doc.buffer().offset_to_line_col(start);
                let (end_line, end_col) = doc.buffer().offset_to_line_col(end);
                for line in min_line..max_line + 1 {
                    if line < start_line {
                        continue;
                    }

                    if line > end_line {
                        break;
                    }

                    let left_col = match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    };
                    let (right_col, _line_end) = match line {
                        _ if line == end_line => {
                            let max_col = doc.buffer().line_end_col(line, true);
                            (end_col.min(max_col), false)
                        }
                        _ => (doc.buffer().line_end_col(line, true), true),
                    };

                    // Shift it by the inlay hints
                    let phantom_text = doc.line_phantom_text(line);
                    let left_col = phantom_text.col_after(left_col, false);
                    let right_col = phantom_text.col_after(right_col, false);

                    let x0 = doc.line_point_of_line_col(line, left_col, 12).x;
                    let x1 = doc.line_point_of_line_col(line, right_col, 12).x;

                    if start != end {
                        rects.push(
                            Size::new(x1 - x0, line_height).to_rect().with_origin(
                                Point::new(
                                    x0 - viewport.x0,
                                    line_height * line as f64 - viewport.y0,
                                ),
                            ),
                        );
                    }
                }
            })
        }
        rects
    };

    let cursor = move || {
        let viewport = viewport.get();
        let config = config.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        let editor = editor.get();
        let is_active = is_active() && !editor.find_focus.get();
        let doc = editor.doc;
        doc.with(|doc| {
            cursor.with(|cursor| match &cursor.mode {
                CursorMode::Normal(offset) => {
                    let line = doc.buffer().line_of_offset(*offset);
                    let mut renders =
                        vec![(viewport, CursorRender::CurrentLine { line })];
                    if is_active {
                        let caret = cursor_caret(doc, *offset, true);
                        renders.push((viewport, caret));
                    }
                    renders
                }
                CursorMode::Visual { start, end, mode } => visual_cursor(
                    doc, *start, *end, mode, None, min_line, max_line, 7.5,
                    is_active,
                )
                .into_iter()
                .map(|render| (viewport, render))
                .collect(),
                CursorMode::Insert(selection) => {
                    insert_cursor(doc, selection, min_line, max_line, 7.5, is_active)
                        .into_iter()
                        .map(|render| (viewport, render))
                        .collect()
                }
            })
        })
    };

    let id = AtomicU64::new(0);
    clip(|| {
        stack(|| {
            (
                list(
                    cursor,
                    move |(_viewport, _cursor)| {
                        id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    },
                    move |(viewport, cursor)| {
                        label(|| "".to_string()).style(move || {
                            let config = config.get_untracked();
                            let line_height = config.editor.line_height();

                            let (width, margin_left, line, background) =
                                match &cursor {
                                    CursorRender::CurrentLine { line } => (
                                        Dimension::Percent(1.0),
                                        0.0,
                                        *line,
                                        *config.get_color(
                                            LapceColor::EDITOR_CURRENT_LINE,
                                        ),
                                    ),
                                    CursorRender::Selection { x, width, line } => (
                                        Dimension::Points(*width as f32),
                                        (*x - viewport.x0) as f32,
                                        *line,
                                        *config
                                            .get_color(LapceColor::EDITOR_SELECTION),
                                    ),
                                    CursorRender::Caret { x, width, line } => (
                                        Dimension::Points(*width as f32),
                                        (*x - viewport.x0) as f32,
                                        *line,
                                        *config.get_color(LapceColor::EDITOR_CARET),
                                    ),
                                };

                            Style::BASE
                                .absolute()
                                .width(width)
                                .height_px(line_height as f32)
                                .margin_left_px(margin_left)
                                .margin_top_px(
                                    (line * line_height) as f32 - viewport.y0 as f32,
                                )
                                .background(background)
                        })
                    },
                )
                .style(move || Style::BASE.absolute().size_pct(100.0, 100.0)),
                {
                    let id = AtomicU64::new(0);
                    list(
                        search_rects,
                        move |_| {
                            id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        },
                        move |rect| {
                            label(|| "".to_string()).style(move || {
                                Style::BASE
                                    .absolute()
                                    .width_px(rect.width() as f32)
                                    .height_px(rect.height() as f32)
                                    .margin_left_px(rect.x0 as f32)
                                    .margin_top_px(rect.y0 as f32)
                                    .border_color(
                                        *config.get().get_color(
                                            LapceColor::EDITOR_FOREGROUND,
                                        ),
                                    )
                                    .border(1.0)
                            })
                        },
                    )
                    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
                },
            )
        })
        .style(move || Style::BASE.absolute().size_pct(100.0, 100.0))
    })
    .style(move || Style::BASE.absolute().size_pct(100.0, 100.0))
}

fn editor_breadcrumbs(
    workspace: Arc<LapceWorkspace>,
    editor: RwSignal<EditorData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack(move || {
        (
            label(|| " ".to_string()).style(|| Style::BASE.margin_vert_px(5.0)),
            scroll(move || {
                let workspace = workspace.clone();
                list(
                    move || {
                        let doc = editor.with(|editor| editor.doc);
                        let full_path = doc
                            .with_untracked(|doc| doc.content.path().cloned())
                            .unwrap_or_default();
                        let mut path = full_path.clone();
                        if let Some(workspace_path) = workspace.clone().path.as_ref()
                        {
                            path = path
                                .strip_prefix(workspace_path)
                                .unwrap_or(&full_path)
                                .to_path_buf();
                        }
                        path.ancestors()
                            .collect::<Vec<_>>()
                            .iter()
                            .rev()
                            .filter_map(|path| {
                                Some(path.file_name()?.to_str()?.to_string())
                            })
                            .collect::<Vec<_>>()
                            .into_iter()
                            .enumerate()
                    },
                    |(i, section)| (*i, section.to_string()),
                    move |(i, section)| {
                        stack(move || {
                            (
                                svg(move || {
                                    config
                                        .get()
                                        .ui_svg(LapceIcons::BREADCRUMB_SEPARATOR)
                                })
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    Style::BASE
                                        .apply_if(i == 0, |s| s.hide())
                                        .size_px(size, size)
                                        .color(*config.get_color(
                                            LapceColor::LAPCE_ICON_ACTIVE,
                                        ))
                                }),
                                label(move || section.clone()),
                            )
                        })
                        .style(|| Style::BASE.items_center())
                    },
                )
                .style(|| Style::BASE.padding_horiz_px(10.0))
            })
            .on_scroll_to(move || {
                editor.with(|_editor| ());
                Some(Point::new(3000.0, 0.0))
            })
            .hide_bar(|| true)
            .style(move || {
                Style::BASE
                    .absolute()
                    .size_pct(100.0, 100.0)
                    .border_bottom(1.0)
                    .border_color(*config.get().get_color(LapceColor::LAPCE_BORDER))
                    .items_center()
            }),
        )
    })
    .style(move || {
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        Style::BASE.items_center().height_px(line_height as f32)
    })
}

fn editor_content(
    editor: RwSignal<EditorData>,
    is_active: impl Fn() -> bool + 'static + Copy,
) -> impl View {
    let (cursor, scroll_delta, scroll_to, window_origin, viewport, config) = editor
        .with_untracked(|editor| {
            (
                editor.cursor.read_only(),
                editor.scroll_delta.read_only(),
                editor.scroll_to,
                editor.window_origin,
                editor.viewport,
                editor.common.config,
            )
        });

    scroll(|| {
        let editor_content_view =
            editor_content_view(editor, is_active).style(move || {
                let config = config.get();
                let padding_bottom = if config.editor.scroll_beyond_last_line {
                    viewport.get().height() as f32
                        - config.editor.line_height() as f32
                } else {
                    0.0
                };
                Style::BASE
                    .padding_bottom_px(padding_bottom)
                    .cursor(CursorStyle::Text)
                    .min_size_pct(100.0, 100.0)
            });
        let id = editor_content_view.id();
        editor_content_view
            .on_event(EventListner::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    let editor = editor.get_untracked();
                    editor.pointer_down(pointer_event);
                }
                true
            })
            .on_event(EventListner::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    let editor = editor.get_untracked();
                    editor.pointer_move(pointer_event);
                }
                true
            })
            .on_event(EventListner::PointerUp, move |event| {
                if let Event::PointerUp(pointer_event) = event {
                    let editor = editor.get_untracked();
                    editor.pointer_up(pointer_event);
                }
                true
            })
    })
    .scroll_bar_color(move || *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR))
    .on_resize(move |point, _rect| {
        window_origin.set(point);
    })
    .on_scroll(move |rect| {
        viewport.set(rect);
    })
    .on_scroll_to(move || scroll_to.get().map(|s| s.to_point()))
    .on_scroll_delta(move || scroll_delta.get())
    .on_ensure_visible(move || {
        let cursor = cursor.get();
        let offset = cursor.offset();
        let editor = editor.get_untracked();
        let doc = editor.doc;
        let caret =
            doc.with_untracked(|doc| cursor_caret(doc, offset, !cursor.is_insert()));
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        if let CursorRender::Caret { x, width, line } = caret {
            let rect = Size::new(width, line_height as f64)
                .to_rect()
                .with_origin(Point::new(x, (line * line_height) as f64));

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
                rect.inflate(
                    0.0,
                    (config.editor.cursor_surrounding_lines * line_height) as f64,
                )
            }
        } else {
            Rect::ZERO
        }
    })
    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
}

fn editor_extra_style(
    extra_styles: Vec<LineExtraStyle>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    list(
        move || extra_styles.clone(),
        |_| 0,
        move |extra| {
            container(|| {
                stack(|| {
                    (
                        label(|| " ".to_string()).style(move || {
                            let config = config.get();
                            Style::BASE
                                .font_family(config.editor.font_family.clone())
                                .font_size(config.editor.font_size() as f32)
                                .width_pct(100.0)
                                .apply_opt(extra.bg_color, Style::background)
                        }),
                        wave_line().style(move || {
                            Style::BASE
                                .absolute()
                                .size_pct(100.0, 100.0)
                                .apply_opt(extra.wave_line, Style::color)
                        }),
                    )
                })
                .style(|| Style::BASE.width_pct(100.0))
            })
            .style(move || {
                let line_height = config.get().editor.line_height();
                Style::BASE
                    .absolute()
                    .height_px(line_height as f32)
                    .width(match extra.width {
                        Some(width) => Dimension::Points(width as f32),
                        None => Dimension::Percent(1.0),
                    })
                    .apply_if(extra.width.is_some(), |s| {
                        s.margin_left_px(extra.x as f32)
                    })
                    .items_center()
            })
        },
    )
    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
}

fn search_editor_view(
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    is_active: impl Fn() -> bool + 'static + Copy,
    replace_focus: RwSignal<bool>,
) -> impl View {
    let cx = AppContext::get_current();
    let cursor_x = create_rw_signal(cx.scope, 0.0);

    let doc = find_editor.doc;
    let cursor = find_editor.cursor;
    let config = find_editor.common.config;

    let case_matching = find_editor.common.find.case_matching;
    let whole_word = find_editor.common.find.whole_words;
    let is_regex = find_editor.common.find.is_regex;
    let visual = find_editor.common.find.visual;

    stack(|| {
        (
            container(|| {
                scroll(|| {
                    text_input(
                        doc,
                        cursor,
                        move || {
                            is_active()
                                && visual.get()
                                && find_focus.get()
                                && !replace_focus.get()
                        },
                        config,
                    )
                    .on_cursor_pos(move |point| {
                        cursor_x.set(point.x);
                    })
                    .style(|| Style::BASE.padding_horiz_px(1.0))
                })
                .on_event(EventListner::PointerDown, move |_| {
                    find_focus.set(true);
                    replace_focus.set(false);
                    false
                })
                .hide_bar(|| true)
                .on_ensure_visible(move || {
                    Size::new(20.0, 0.0)
                        .to_rect()
                        .with_origin(Point::new(cursor_x.get() - 10.0, 0.0))
                })
                .style(|| {
                    Style::BASE
                        .absolute()
                        .cursor(CursorStyle::Text)
                        .size_pct(100.0, 100.0)
                        .items_center()
                })
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0)),
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
                config,
            )
            .style(|| Style::BASE.padding_left_px(6.0)),
            clickable_icon(
                || LapceIcons::SEARCH_WHOLE_WORD,
                move || {
                    whole_word.update(|whole_word| {
                        *whole_word = !*whole_word;
                    });
                },
                move || whole_word.get(),
                || false,
                config,
            )
            .style(|| Style::BASE.padding_left_px(6.0)),
            clickable_icon(
                || LapceIcons::SEARCH_REGEX,
                move || {
                    is_regex.update(|is_regex| {
                        *is_regex = !*is_regex;
                    });
                },
                move || is_regex.get(),
                || false,
                config,
            )
            .style(|| Style::BASE.padding_left_px(6.0)),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .width_px(200.0)
            .padding_horiz_px(6.0)
            .padding_vert_px(4.0)
            .border(1.0)
            .border_radius(6.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn replace_editor_view(
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn() -> bool + 'static + Copy,
    find_focus: RwSignal<bool>,
) -> impl View {
    let cx = AppContext::get_current();
    let cursor_x = create_rw_signal(cx.scope, 0.0);

    let doc = replace_editor.doc;
    let cursor = replace_editor.cursor;
    let config = replace_editor.common.config;
    let visual = replace_editor.common.find.visual;

    stack(|| {
        (
            container(|| {
                scroll(|| {
                    text_input(
                        doc,
                        cursor,
                        move || {
                            is_active()
                                && visual.get()
                                && find_focus.get()
                                && replace_active.get()
                                && replace_focus.get()
                        },
                        config,
                    )
                    .on_cursor_pos(move |point| {
                        cursor_x.set(point.x);
                    })
                    .style(|| Style::BASE.padding_horiz_px(1.0))
                })
                .on_event(EventListner::PointerDown, move |_| {
                    find_focus.set(true);
                    replace_focus.set(true);
                    false
                })
                .hide_bar(|| true)
                .on_ensure_visible(move || {
                    Size::new(20.0, 0.0)
                        .to_rect()
                        .with_origin(Point::new(cursor_x.get() - 10.0, 0.0))
                })
                .style(|| {
                    Style::BASE
                        .absolute()
                        .cursor(CursorStyle::Text)
                        .size_pct(100.0, 100.0)
                        .items_center()
                })
            })
            .style(|| Style::BASE.size_pct(100.0, 100.0)),
            empty().style(move || {
                let config = config.get();
                let size = config.ui.icon_size() as f32 + 10.0;
                Style::BASE.size_px(0.0, size)
            }),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .width_px(200.0)
            .padding_horiz_px(6.0)
            .padding_vert_px(4.0)
            .border(1.0)
            .border_radius(6.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn find_view(
    editor: RwSignal<EditorData>,
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn() -> bool + 'static + Copy,
) -> impl View {
    let config = find_editor.common.config;
    let find_visual = find_editor.common.find.visual;

    container(|| {
        stack(|| {
            (
                stack(|| {
                    (
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
                            config,
                        )
                        .style(|| Style::BASE.padding_horiz_px(6.0)),
                        search_editor_view(
                            find_editor,
                            find_focus,
                            is_active,
                            replace_focus,
                        ),
                        clickable_icon(
                            || LapceIcons::SEARCH_BACKWARD,
                            move || {
                                editor
                                    .get_untracked()
                                    .search_backward(Modifiers::empty());
                            },
                            move || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.padding_left_px(6.0)),
                        clickable_icon(
                            || LapceIcons::SEARCH_FORWARD,
                            move || {
                                editor
                                    .get_untracked()
                                    .search_forward(Modifiers::empty());
                            },
                            move || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.padding_left_px(6.0)),
                        clickable_icon(
                            || LapceIcons::CLOSE,
                            move || {
                                editor.get_untracked().clear_search();
                            },
                            move || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.padding_horiz_px(6.0)),
                    )
                })
                .style(|| Style::BASE.items_center()),
                stack(|| {
                    (
                        empty().style(move || {
                            let config = config.get();
                            let width =
                                config.ui.icon_size() as f32 + 10.0 + 6.0 * 2.0;
                            Style::BASE.width_px(width)
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
                            move || {},
                            move || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.padding_left_px(6.0)),
                        clickable_icon(
                            || LapceIcons::SEARCH_REPLACE_ALL,
                            move || {},
                            move || false,
                            || false,
                            config,
                        )
                        .style(|| Style::BASE.padding_left_px(6.0)),
                    )
                })
                .style(move || {
                    Style::BASE
                        .items_center()
                        .margin_top_px(4.0)
                        .apply_if(!replace_active.get(), |s| s.hide())
                }),
            )
        })
        .style(move || {
            let config = config.get();
            Style::BASE
                .margin_right_px(50.0)
                .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
                .border_radius(6.0)
                .border(1.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .padding_vert_px(4.0)
                .cursor(CursorStyle::Default)
                .flex_col()
        })
        .on_event(EventListner::PointerDown, move |_| {
            let editor = editor.get_untracked();
            if let Some(editor_tab_id) = editor.editor_tab_id {
                editor
                    .common
                    .internal_command
                    .set(Some(InternalCommand::FocusEditorTab { editor_tab_id }));
            }
            true
        })
    })
    .style(move || {
        Style::BASE
            .absolute()
            .margin_top_px(-1.0)
            .width_pct(100.0)
            .justify_end()
            .apply_if(!find_visual.get(), |s| s.hide())
    })
}
