use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use floem::{
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    event::{Event, EventListener},
    ext_event::create_ext_action,
    glazier::{Modifiers, PointerType},
    id::Id,
    peniko::{
        kurbo::{BezPath, Line, Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, on_cleanup, ReadSignal,
        RwSignal, SignalGet, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWith, SignalWithUntracked,
    },
    style::{ComputedStyle, CursorStyle, Style},
    taffy::prelude::Node,
    view::{ChangeFlags, View},
    views::{
        container, empty, label, list, scroll, stack, svg, virtual_list, Decorators,
        VirtualListDirection, VirtualListItemSize, VirtualListVector,
    },
    Renderer, ViewContext,
};
use lapce_core::{
    buffer::{
        rope_text::{RopeText, RopeTextVal},
        DiffLines,
    },
    char_buffer::CharBuffer,
    cursor::{ColPosition, CursorMode},
    mode::{Mode, VisualMode},
    selection::Selection,
    soft_tab::{snap_to_soft_tab_line_col, SnapDirection},
    word::WordCursor,
};
use lapce_rpc::style::LineStyle;
use lapce_xi_rope::{find::CaseMatching, Rope};
use lsp_types::DiagnosticSeverity;

use super::EditorData;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::{
        phantom_text::PhantomTextLine, DocContent, Document, EditorDiagnostic,
        TextCacheListener,
    },
    find::{Find, FindResult},
    main_split::MainSplitData,
    text_input::text_input,
    workspace::LapceWorkspace,
};

struct StickyHeaderInfo {
    sticky_lines: Vec<usize>,
    last_sticky_should_scroll: bool,
    y_diff: f64,
}

pub struct EditorView {
    id: Id,
    editor: RwSignal<EditorData>,
    is_active: Box<dyn Fn() -> bool + 'static>,
    inner_node: Option<Node>,
    viewport: RwSignal<Rect>,
    sticky_header_info: StickyHeaderInfo,
}

pub fn editor_view(
    editor: RwSignal<EditorData>,
    is_active: impl Fn() -> bool + 'static + Copy,
) -> EditorView {
    let cx = ViewContext::get_current();
    let id = cx.new_id();

    let viewport = create_rw_signal(cx.scope, Rect::ZERO);

    create_effect(cx.scope, move |_| {
        editor.with(|_| ());
        id.request_layout();
    });

    create_effect(cx.scope, move |last_rev| {
        let doc = editor.with(|editor| editor.doc);
        let rev = doc.with(|doc| doc.rev());
        if last_rev == Some(rev) {
            return rev;
        }
        id.request_layout();
        rev
    });

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
            (
                doc.content.clone(),
                doc.buffer().rev(),
                doc.style_rev(),
                rect,
            )
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

    EditorView {
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

impl EditorView {
    fn paint_cursor(
        &self,
        cx: &mut PaintCx,
        min_line: usize,
        max_line: usize,
        is_local: bool,
    ) {
        let (view, cursor, find_focus, config) =
            self.editor.with_untracked(|editor| {
                (
                    editor.view.clone(),
                    editor.cursor,
                    editor.find_focus,
                    editor.common.config,
                )
            });

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let viewport = self.viewport.get_untracked();
        let is_active = (*self.is_active)() && !find_focus.get_untracked();

        view.track_doc();
        let renders = cursor.with_untracked(|cursor| match &cursor.mode {
            CursorMode::Normal(offset) => {
                let line = view.line_of_offset(*offset);
                let mut renders = vec![CursorRender::CurrentLine { line }];
                if is_active {
                    let caret = cursor_caret(&view, *offset, true);
                    renders.push(caret);
                }
                renders
            }
            CursorMode::Visual { start, end, mode } => visual_cursor(
                &view,
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
                insert_cursor(&view, selection, min_line, max_line, 7.5, is_active)
            }
        });

        for render in renders {
            match render {
                CursorRender::CurrentLine { line } => {
                    if !is_local {
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
                        Point::new(
                            style.x
                                + if style.width.is_none() {
                                    viewport.x0
                                } else {
                                    0.0
                                },
                            y + (line_height - height) / 2.0,
                        ),
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
        let (view, config) = self
            .editor
            .with_untracked(|editor| (editor.view.clone(), editor.common.config));

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let font_size = config.editor.font_size();

        let indent_unit = view.indent_unit();
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .font_size(config.editor.font_size() as f32);
        let attrs_list = AttrsList::new(attrs);

        let mut indent_text = TextLayout::new();
        indent_text.set_text(&format!("{indent_unit}a"), attrs_list);
        let indent_text_width = indent_text.hit_position(indent_unit.len()).point.x;

        let last_line = view.last_line();

        for line in min_line..max_line + 1 {
            if line > last_line {
                break;
            }

            let text_layout = view.get_text_layout(line, font_size);
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

            if config.editor.show_indent_guide {
                let mut x = 0.0;
                while x + 1.0 < text_layout.indent {
                    cx.stroke(
                        &Line::new(Point::new(x, y), Point::new(x, y + line_height)),
                        config.get_color(LapceColor::EDITOR_INDENT_GUIDE),
                        1.0,
                    );
                    x += indent_text_width;
                }
            }

            cx.draw_text(
                &text_layout.text,
                Point::new(0.0, y + (line_height - height) / 2.0),
            );
        }
    }

    fn paint_find(&self, cx: &mut PaintCx, min_line: usize, max_line: usize) {
        let visual = self.editor.with_untracked(|e| e.common.find.visual);
        if !visual.get_untracked() {
            return;
        }

        let (view, config) = self
            .editor
            .with_untracked(|editor| (editor.view.clone(), editor.common.config));
        let occurrences = view.find_result().occurrences;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        view.update_find(min_line, max_line);
        let start = view.offset_of_line(min_line);
        let end = view.offset_of_line(max_line + 1);

        let mut rects = Vec::new();
        for region in occurrences
            .with(|selection| selection.regions_in_range(start, end).to_vec())
        {
            let start = region.min();
            let end = region.max();
            let (start_line, start_col) = view.offset_to_line_col(start);
            let (end_line, end_col) = view.offset_to_line_col(end);
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
                        let max_col = view.line_end_col(line, true);
                        (end_col.min(max_col), false)
                    }
                    _ => (view.line_end_col(line, true), true),
                };

                // Shift it by the inlay hints
                let phantom_text = view.line_phantom_text(line);
                let left_col = phantom_text.col_after(left_col, false);
                let right_col = phantom_text.col_after(right_col, false);

                let x0 = view.line_point_of_line_col(line, left_col, 12).x;
                let x1 = view.line_point_of_line_col(line, right_col, 12).x;

                if start != end {
                    rects.push(
                        Size::new(x1 - x0, line_height)
                            .to_rect()
                            .with_origin(Point::new(x0, line_height * line as f64)),
                    );
                }
            }
        }

        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        for rect in rects {
            cx.stroke(&rect, color, 1.0);
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        start_line: usize,
        viewport: Rect,
    ) {
        let (view, config) = self
            .editor
            .with_untracked(|editor| (editor.view.clone(), editor.common.config));
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
        let sticky_area_rect = Size::new(viewport.x1, area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0));

        cx.fill(
            &sticky_area_rect,
            config.get_color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
        );

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

            let line_area_rect = Size::new(viewport.width(), line_height - y_diff)
                .to_rect()
                .with_origin(Point::new(
                    viewport.x0,
                    viewport.y0 + line_height * i as f64,
                ));

            cx.clip(&line_area_rect);

            let text_layout = view.get_text_layout(line, config.editor.font_size());
            let y = viewport.y0
                + line_height * i as f64
                + (line_height - text_layout.text.size().height) / 2.0
                - y_diff;
            cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

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
        if is_local {
            return;
        }
        const BAR_WIDTH: f64 = 10.0;
        cx.fill(
            &Rect::ZERO
                .with_size(Size::new(1.0, viewport.height()))
                .with_origin(Point::new(
                    viewport.x0 + viewport.width() - BAR_WIDTH,
                    viewport.y0,
                ))
                .inflate(0.0, 10.0),
            config.get_color(LapceColor::LAPCE_SCROLL_BAR),
        );

        let doc = self.editor.with_untracked(|e| e.doc);
        let total_len = doc.with_untracked(|doc| doc.buffer().last_line());
        let changes = doc.with_untracked(|doc| doc.head_changes);
        let changes = changes.get_untracked();
        let total_height = viewport.height();
        let total_width = viewport.width();
        let line_height = config.editor.line_height();
        let content_height = if config.editor.scroll_beyond_last_line {
            (total_len * line_height) as f64 + total_height - line_height as f64
        } else {
            (total_len * line_height) as f64
        };

        let colors = changes_colors(changes, 0, total_len, &config);
        for (y, height, _, color) in colors {
            let y = (y * line_height) as f64 / content_height * total_height;
            let height = ((height * line_height) as f64 / content_height
                * total_height)
                .max(3.0);
            let rect = Rect::ZERO.with_size(Size::new(3.0, height)).with_origin(
                Point::new(
                    viewport.x0 + total_width - BAR_WIDTH + 1.0,
                    y + viewport.y0,
                ),
            );
            cx.fill(&rect, color);
        }
    }
}

impl View for EditorView {
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

            let (doc, view, viewport, config) =
                self.editor.with_untracked(|editor| {
                    (
                        editor.doc,
                        editor.view.clone(),
                        editor.viewport,
                        editor.common.config,
                    )
                });
            let config = config.get_untracked();
            let line_height = config.editor.line_height() as f64;
            let font_size = config.editor.font_size();
            let viewport = viewport.get_untracked();
            let min_line = (viewport.y0 / line_height).floor() as usize;
            let max_line = (viewport.y1 / line_height).ceil() as usize;
            for line in min_line..max_line + 1 {
                view.get_text_layout(line, font_size);
            }
            let (width, height) = doc.with_untracked(|doc| {
                let width = view.text_layouts.borrow().max_width + 20.0;
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

    fn compute_layout(&mut self, cx: &mut floem::context::LayoutCx) -> Option<Rect> {
        let viewport = cx.current_viewport().unwrap_or_default();
        if self.viewport.with_untracked(|v| v != &viewport) {
            self.viewport.set(viewport);
        }
        None
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

        let doc = self.editor.with_untracked(|e| e.doc);
        let is_local = doc.with_untracked(|doc| doc.content.is_local());

        self.paint_cursor(cx, min_line, max_line, is_local);
        self.paint_find(cx, min_line, max_line);
        self.paint_text(cx, min_line, max_line, viewport);
        self.paint_sticky_headers(cx, min_line, viewport);
        self.paint_scroll_bar(cx, viewport, is_local, config);
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
pub enum CursorRender {
    CurrentLine { line: usize },
    Selection { x: f64, width: f64, line: usize },
    Caret { x: f64, width: f64, line: usize },
}

pub fn cursor_caret(
    view: &EditorViewData,
    offset: usize,
    block: bool,
) -> CursorRender {
    let (line, col) = view.offset_to_line_col(offset);
    let phantom_text = view.line_phantom_text(line);
    let col = phantom_text.col_after(col, block);
    let x0 = view.line_point_of_line_col(line, col, 12).x;
    if block {
        let right_offset = view.move_right(offset, Mode::Insert, 1);
        let (_, right_col) = view.offset_to_line_col(right_offset);
        let x1 = view.line_point_of_line_col(line, right_col, 12).x;

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
    view: &EditorViewData,
    start: usize,
    end: usize,
    mode: &VisualMode,
    horiz: Option<&ColPosition>,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let (start_line, start_col) = view.offset_to_line_col(start.min(end));
    let (end_line, end_col) = view.offset_to_line_col(start.max(end));
    let (cursor_line, _) = view.offset_to_line_col(end);

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
                let max_col = view.line_end_col(line, false);
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
                    let max_col = view.line_end_col(line, true);

                    let end_offset =
                        view.move_right(start.max(end), Mode::Visual, 1);
                    let (_, end_col) = view.offset_to_line_col(end_offset);

                    (end_col.min(max_col), false)
                } else {
                    (view.line_end_col(line, true), true)
                }
            }
            VisualMode::Linewise => (view.line_end_col(line, true), true),
            VisualMode::Blockwise => {
                let max_col = view.line_end_col(line, true);
                let right = match horiz.as_ref() {
                    Some(&ColPosition::End) => max_col,
                    _ => {
                        let end_offset =
                            view.move_right(start.max(end), Mode::Visual, 1);
                        let (_, end_col) = view.offset_to_line_col(end_offset);
                        end_col.max(start_col).min(max_col)
                    }
                };
                (right, false)
            }
        };

        let phantom_text = view.line_phantom_text(line);
        let left_col = phantom_text.col_after(left_col, false);
        let right_col = phantom_text.col_after(right_col, false);
        let x0 = view.line_point_of_line_col(line, left_col, 12).x;
        let mut x1 = view.line_point_of_line_col(line, right_col, 12).x;
        if line_end {
            x1 += char_width;
        }

        renders.push(CursorRender::Selection {
            x: x0,
            width: x1 - x0,
            line,
        });

        if is_active && line == cursor_line {
            let caret = cursor_caret(view, end, true);
            renders.push(caret);
        }
    }

    renders
}

fn insert_cursor(
    view: &EditorViewData,
    selection: &Selection,
    min_line: usize,
    max_line: usize,
    char_width: f64,
    is_active: bool,
) -> Vec<CursorRender> {
    let start = view.offset_of_line(min_line);
    let end = view.offset_of_line(max_line + 1);
    let regions = selection.regions_in_range(start, end);

    let mut renders = Vec::new();

    for region in regions {
        let cursor_offset = region.end;
        let (cursor_line, _) = view.offset_to_line_col(cursor_offset);
        let start = region.start;
        let end = region.end;
        let (start_line, start_col) = view.offset_to_line_col(start.min(end));
        let (end_line, end_col) = view.offset_to_line_col(start.max(end));
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
                    let max_col = view.line_end_col(line, true);
                    (end_col.min(max_col), false)
                }
                _ => (view.line_end_col(line, true), true),
            };

            // Shift it by the inlay hints
            let phantom_text = view.line_phantom_text(line);
            let left_col = phantom_text.col_after(left_col, false);
            let right_col = phantom_text.col_after(right_col, false);

            let x0 = view.line_point_of_line_col(line, left_col, 12).x;
            let mut x1 = view.line_point_of_line_col(line, right_col, 12).x;
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
                let caret = cursor_caret(view, cursor_offset, false);
                renders.push(caret);
            }
        }
    }
    renders
}

pub fn editor_container_view(
    main_split: MainSplitData,
    workspace: Arc<LapceWorkspace>,
    is_active: impl Fn() -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (find_focus, sticky_header_height, config, editor_id, editor_scope) = editor
        .with_untracked(|editor| {
            (
                editor.find_focus,
                editor.sticky_header_height,
                editor.common.config,
                editor.editor_id,
                editor.scope,
            )
        });
    let editors = main_split.editors;
    on_cleanup(ViewContext::get_current().scope, move || {
        let exits =
            editors.with_untracked(|editors| editors.contains_key(&editor_id));
        if !exits {
            let send = create_ext_action(editor_scope, move |_| {
                editor_scope.dispose();
            });
            std::thread::spawn(move || {
                send(());
            });
        }
    });

    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;

    let cx = ViewContext::get_current();
    let editor_rect = create_rw_signal(cx.scope, Rect::ZERO);
    let gutter_rect = create_rw_signal(cx.scope, Rect::ZERO);

    stack(move || {
        (
            editor_breadcrumbs(workspace, editor, config),
            container(|| {
                stack(|| {
                    (
                        editor_gutter(editor, is_active, gutter_rect),
                        container(|| editor_content(editor, is_active))
                            .style(move || Style::BASE.size_pct(100.0, 100.0)),
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

    let cx = ViewContext::get_current();
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

    let gutter_width = create_memo(cx.scope, move |_| gutter_rect.get().width());

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

    let head_changes = move || {
        let viewport = viewport.get();
        let doc = editor.with(|editor| editor.doc);
        let changes = doc.with_untracked(|doc| doc.head_changes);
        let changes = changes.get();
        let config = config.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        changes_colors(changes, min_line, max_line, &config)
    };

    let gutter_view_fn = move |line: DocLine| {
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
                empty().style(move || Style::BASE.width_px(padding_left)),
                container(|| {
                    label(move || line_number.to_string()).style(move || {
                        let config = config.get();
                        let (current_line, _) = current_line.get_untracked();
                        Style::BASE.apply_if(current_line != line.line, move |s| {
                            s.color(*config.get_color(LapceColor::EDITOR_DIM))
                        })
                    })
                })
                .style(move || {
                    Style::BASE
                        .width_px(
                            gutter_width.get() as f32 - padding_left - padding_right,
                        )
                        .justify_end()
                }),
                container(|| {
                    container(|| {
                        container(|| {
                            svg(move || config.get().ui_svg(LapceIcons::LIGHTBULB))
                                .style(move || {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    Style::BASE.size_px(size, size).color(
                                        *config.get_color(LapceColor::LAPCE_WARN),
                                    )
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
                                code_action_line.get() != Some(line.line),
                                |s| s.hide(),
                            )
                        })
                    })
                    .style(move || {
                        Style::BASE
                            .justify_center()
                            .items_center()
                            .width_px(padding_right - padding_left)
                    })
                })
                .style(move || Style::BASE.justify_end().width_px(padding_right)),
            )
        })
        .style(move || {
            let config = config.get_untracked();
            let line_height = config.editor.line_height();
            Style::BASE.items_center().height_px(line_height as f32)
        })
    };

    stack(|| {
        (
            stack(|| {
                (
                    empty().style(move || Style::BASE.width_px(padding_left)),
                    label(move || {
                        editor
                            .get()
                            .doc
                            .with(|doc| (doc.buffer().last_line() + 1).to_string())
                    }),
                    empty().style(move || Style::BASE.width_px(padding_right)),
                )
            })
            .style(|| Style::BASE.height_pct(100.0)),
            scroll(|| {
                stack(|| {
                    (
                        virtual_list(
                            VirtualListDirection::Vertical,
                            VirtualListItemSize::Fixed(Box::new(move || {
                                config.get_untracked().editor.line_height() as f64
                            })),
                            move || {
                                let editor = editor.get();
                                current_line.get();
                                editor.view.track_doc();
                                editor.view
                            },
                            move |line: &DocLine| {
                                (line.line, current_line.get_untracked())
                            },
                            gutter_view_fn,
                        )
                        .style(move || {
                            let config = config.get();
                            let padding_bottom =
                                if config.editor.scroll_beyond_last_line {
                                    viewport.get().height() as f32
                                        - config.editor.line_height() as f32
                                } else {
                                    0.0
                                };
                            Style::BASE
                                .flex_col()
                                .width_pct(100.0)
                                .padding_bottom_px(padding_bottom)
                        }),
                        list(
                            head_changes,
                            move |(y, height, removed, color)| {
                                (*y, *height, *removed, *color)
                            },
                            move |(y, height, removed, color)| {
                                empty().style(move || {
                                    let line_height =
                                        config.get().editor.line_height();
                                    Style::BASE
                                        .absolute()
                                        .width_px(3.0)
                                        .height_px((height * line_height) as f32)
                                        .apply_if(removed, |s| s.height_px(10.0))
                                        .margin_left_px(
                                            gutter_width.get() as f32
                                                - padding_right
                                                + padding_left
                                                - 3.0,
                                        )
                                        .margin_top_px((y * line_height) as f32)
                                        .apply_if(removed, |s| {
                                            s.margin_top_px(
                                                (y * line_height) as f32 - 5.0,
                                            )
                                        })
                                        .background(color)
                                })
                            },
                        )
                        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0)),
                    )
                })
            })
            .hide_bar(|| true)
            .on_event(EventListener::PointerWheel, move |event| {
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
                        let mut path = full_path;
                        if let Some(workspace_path) = workspace.clone().path.as_ref()
                        {
                            path = path
                                .strip_prefix(workspace_path)
                                .unwrap_or(&path)
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
    let (
        cursor,
        scroll_delta,
        scroll_to,
        window_origin,
        viewport,
        sticky_header_height,
        config,
    ) = editor.with_untracked(|editor| {
        (
            editor.cursor.read_only(),
            editor.scroll_delta.read_only(),
            editor.scroll_to,
            editor.window_origin,
            editor.viewport,
            editor.sticky_header_height,
            editor.common.config,
        )
    });

    scroll(|| {
        let editor_content_view = editor_view(editor, is_active).style(move || {
            let config = config.get();
            let padding_bottom = if config.editor.scroll_beyond_last_line {
                viewport.get().height() as f32 - config.editor.line_height() as f32
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
            .on_event(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    let editor = editor.get_untracked();
                    editor.pointer_down(pointer_event);
                }
                true
            })
            .on_event(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    let editor = editor.get_untracked();
                    editor.pointer_move(pointer_event);
                }
                true
            })
            .on_event(EventListener::PointerUp, move |event| {
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
        let view = editor.with(|e| e.view.clone());
        let caret = cursor_caret(&view, offset, !cursor.is_insert());
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        if let CursorRender::Caret { x, width, line } = caret {
            let rect = Size::new(width, line_height as f64)
                .to_rect()
                .with_origin(Point::new(x, (line * line_height) as f64))
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
                let mut rect = rect;
                rect.y0 -= (config.editor.cursor_surrounding_lines * line_height)
                    as f64
                    + sticky_header_height.get_untracked();
                rect.y1 +=
                    (config.editor.cursor_surrounding_lines * line_height) as f64;
                rect
            }
        } else {
            Rect::ZERO
        }
    })
    .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
}

fn search_editor_view(
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    is_active: impl Fn() -> bool + 'static + Copy,
    replace_focus: RwSignal<bool>,
) -> impl View {
    let config = find_editor.common.config;

    let case_matching = find_editor.common.find.case_matching;
    let whole_word = find_editor.common.find.whole_words;
    let is_regex = find_editor.common.find.is_regex;
    let visual = find_editor.common.find.visual;

    stack(|| {
        (
            text_input(find_editor, move || {
                is_active()
                    && visual.get()
                    && find_focus.get()
                    && !replace_focus.get()
            })
            .on_event(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(false);
                false
            })
            .style(|| Style::BASE.width_pct(100.0)),
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
            .style(|| Style::BASE.padding_vert_px(4.0)),
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
            .style(|| Style::BASE.padding_horiz_px(6.0)),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .width_px(200.0)
            .items_center()
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
    let config = replace_editor.common.config;
    let visual = replace_editor.common.find.visual;

    stack(|| {
        (
            text_input(replace_editor, move || {
                is_active()
                    && visual.get()
                    && find_focus.get()
                    && replace_active.get()
                    && replace_focus.get()
            })
            .on_event(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(true);
                false
            })
            .style(|| Style::BASE.width_pct(100.0)),
            empty().style(move || {
                let config = config.get();
                let size = config.ui.icon_size() as f32 + 10.0;
                Style::BASE.size_px(0.0, size).padding_vert_px(4.0)
            }),
        )
    })
    .style(move || {
        let config = config.get();
        Style::BASE
            .width_px(200.0)
            .items_center()
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
        .on_event(EventListener::PointerDown, move |_| {
            let editor = editor.get_untracked();
            if let Some(editor_tab_id) = editor.editor_tab_id {
                editor
                    .common
                    .internal_command
                    .send(InternalCommand::FocusEditorTab { editor_tab_id });
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

#[derive(Clone)]
pub struct LineExtraStyle {
    pub x: f64,
    pub width: Option<f64>,
    pub bg_color: Option<Color>,
    pub under_line: Option<Color>,
    pub wave_line: Option<Color>,
}

#[derive(Clone)]
pub struct TextLayoutLine {
    /// Extra styling that should be applied to the text
    /// (x0, x1 or line display end, style)
    pub extra_style: Vec<LineExtraStyle>,
    pub text: TextLayout,
    pub whitespaces: Option<Vec<(char, (f64, f64))>>,
    pub indent: f64,
}

/// Keeps track of the text layouts so that we can efficiently reuse them.
#[derive(Clone, Default)]
pub struct TextLayoutCache {
    /// The id of the last config, which lets us know when the config changes so we can update
    /// the cache.
    config_id: u64,
    /// (Font Size -> (Line Number -> Text Layout))  
    /// Different font-sizes are cached separately, which is useful for features like code lens
    /// where the text becomes small but you may wish to revert quickly.
    pub layouts: HashMap<usize, HashMap<usize, Arc<TextLayoutLine>>>,
    pub max_width: f64,
}

impl TextLayoutCache {
    pub fn new() -> Self {
        Self {
            config_id: 0,
            layouts: HashMap::new(),
            max_width: 0.0,
        }
    }

    fn clear(&mut self) {
        self.layouts.clear();
        self.max_width = 0.0;
    }

    pub fn check_attributes(&mut self, config_id: u64) {
        if self.config_id != config_id {
            self.clear();
            self.config_id = config_id;
        }
    }
}

pub struct DocLine {
    pub rev: u64,
    pub style_rev: u64,
    pub line: usize,
    pub text: Arc<TextLayoutLine>,
}

impl VirtualListVector<DocLine> for EditorViewData {
    type ItemIterator = std::vec::IntoIter<DocLine>;

    fn total_len(&self) -> usize {
        self.num_lines()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> Self::ItemIterator {
        let lines = range
            .into_iter()
            .map(|line| DocLine {
                rev: self.rev(),
                style_rev: self.style_rev(),
                line,
                text: self.get_text_layout(line, 12),
            })
            .collect::<Vec<_>>();
        lines.into_iter()
    }
}

// TODO(minor): Should this go in another file? It doesn't really need to be with the drawing code for editor views
/// Data specific to the rendering of a view.  
/// This has various helper methods that may dispatch to the held [`Document`] signal, but are
/// untracked by default. If you need your signal to depend on the document,
/// then you should call [`EditorViewData::track_doc`].  
/// Note: This should be cheap to clone.
#[derive(Clone)]
pub struct EditorViewData {
    /// Equivalent to the `EditorData::doc` that contains this view.
    pub doc: RwSignal<Document>,
    /// The text layouts for the document. This may be shared with other views.
    text_layouts: Rc<RefCell<TextLayoutCache>>,

    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl EditorViewData {
    pub fn new(
        doc: RwSignal<Document>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> EditorViewData {
        let view = EditorViewData {
            doc,
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            config,
        };

        let listener = view.text_layouts.clone();
        doc.with_untracked(|doc| {
            doc.add_text_cache_listener(listener);
        });

        view
    }

    // Note: There is no document / buffer function as that would require cloning it
    // since we can't get a reference out of the RwSignal document.

    /// Subscribe to the doc signal, since the getter functions use `with_untracked` by default.
    pub fn track_doc(&self) {
        self.doc.track();
    }

    /// Return the underlying `Rope` of the document.
    pub fn text(&self) -> Rope {
        self.doc.with_untracked(|doc| doc.buffer().text().clone())
    }

    /// Return a [`RopeTextVal`] wrapper.  
    /// Unfortunately, we can't implement [`RopeText`] directly on [`EditorViewData`] due to
    /// it not having a reference to the rope.
    pub fn rope_text(&self) -> RopeTextVal {
        RopeTextVal::new(self.text())
    }

    /// Return the [`Document`]'s [`Find`] instance. Find uses signals, and so can be updated.
    pub fn find(&self) -> Find {
        self.doc.with_untracked(|doc| doc.find().clone())
    }

    pub fn find_result(&self) -> FindResult {
        self.doc.with_untracked(|doc| doc.find_result.clone())
    }

    pub fn update_find(&self, min_line: usize, max_line: usize) {
        self.doc
            .with_untracked(|doc| doc.update_find(min_line, max_line));
    }

    /// The current revision of the underlying buffer. This is used to track when the buffer has
    /// changed.
    pub fn rev(&self) -> u64 {
        self.doc.with_untracked(|doc| doc.rev())
    }

    // TODO: Should the editor view handle the style information?
    // It does not need to, since we can just get that information from the document,
    // but it might be nicer? It would let us make the document more strictly about the
    // data and the view more about the rendering.
    // but you could also consider it as the document managing the syntax it should apply
    // Though. There is the question of whether we'd want to allow different syntax highlighting in
    // different views. However, we probably wouldn't.

    pub fn style_rev(&self) -> u64 {
        self.doc.with_untracked(|doc| doc.style_rev())
    }

    /// The document for the given view was swapped out.
    pub fn update_doc(&mut self, doc: RwSignal<Document>) {
        let old_doc = self.doc;

        // Just recreate the view. This is simpler than trying to reuse the text layout cache in the cases where it is possible.
        *self = Self::new(doc, self.config);

        old_doc.with_untracked(|doc| {
            doc.clean_text_cache_listeners();
        });
    }

    /// Duplicate as a new view which refers to the same document.
    pub fn duplicate(&self) -> Self {
        // TODO: This is correct right now, as it has the views share the same text layout cache.
        // However, once we have line wrapping or other view-specific rendering changes, this should check for whether they're different.
        // This will likely require more information to be passed into duplicate,
        // like whether the wrap width will be the editor's width, and so it is unlikely to be exactly the same as the current view.
        self.clone()
    }

    fn line_phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc.with_untracked(|doc| doc.line_phantom_text(line))
    }

    fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        self.doc.with_untracked(|doc| doc.line_style(line))
    }

    fn diagnostics(&self) -> im::Vector<EditorDiagnostic> {
        self.doc
            .with_untracked(|doc| doc.diagnostics.diagnostics.get_untracked())
    }

    /// Create a new text layout for the given line.  
    /// Typically you should use [`EditorViewData::get_text_layout`] instead.
    fn new_text_layout(&self, line: usize, _font_size: usize) -> TextLayoutLine {
        let config = self.config.get_untracked();
        // let buffer = self.doc.with_untracked(|doc| doc.buffer().clone());
        // let line_content_original = buffer.line_content(line);
        // let line_content_original = doc.buffer().line_content(line);
        let line_content = self.doc.with_untracked(|doc| {
            let line_content_original = doc.buffer().line_content(line);

            // Get the line content with newline characters replaced with spaces
            // and the content without the newline characters
            let (line_content, _line_content_original) =
                if let Some(s) = line_content_original.strip_suffix("\r\n") {
                    (
                        format!("{s}  "),
                        &line_content_original[..line_content_original.len() - 2],
                    )
                } else if let Some(s) = line_content_original.strip_suffix('\n') {
                    (
                        format!("{s} ",),
                        &line_content_original[..line_content_original.len() - 1],
                    )
                } else {
                    (
                        line_content_original.to_string(),
                        &line_content_original[..],
                    )
                };

            line_content
        });

        // Combine the phantom text with the line content
        let phantom_text = self.line_phantom_text(line);
        let line_content = phantom_text.combine_with_text(line_content);

        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .color(*color)
            .family(&family)
            .font_size(config.editor.font_size() as f32);
        let mut attrs_list = AttrsList::new(attrs);

        // Apply various styles to the line's text based on our semantic/syntax highlighting
        let styles = self.line_style(line);
        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.get_style_color(fg_color) {
                    let start = phantom_text.col_at(line_style.start);
                    let end = phantom_text.col_at(line_style.end);
                    attrs_list.add_span(start..end, attrs.color(*fg_color));
                }
            }
        }

        let font_size = config.editor.font_size();

        // Apply phantom text specific styling
        for (offset, size, col, phantom) in phantom_text.offset_size_iter() {
            let start = col + offset;
            let end = start + size;

            let mut attrs = attrs;
            if let Some(fg) = phantom.fg {
                attrs = attrs.color(fg);
            }
            if let Some(phantom_font_size) = phantom.font_size {
                attrs = attrs.font_size(phantom_font_size.min(font_size) as f32);
            }
            attrs_list.add_span(start..end, attrs);
            // if let Some(font_family) = phantom.font_family.clone() {
            //     layout_builder = layout_builder.range_attribute(
            //         start..end,
            //         TextAttribute::FontFamily(font_family),
            //     );
            // }
        }

        let mut text_layout = TextLayout::new();
        text_layout.set_text(&line_content, attrs_list);

        // Keep track of background styling from phantom text, which is done separately
        // from the text layout attributes
        let mut extra_style = Vec::new();
        for (offset, size, col, phantom) in phantom_text.offset_size_iter() {
            if phantom.bg.is_some() || phantom.under_line.is_some() {
                let start = col + offset;
                let end = start + size;
                let x0 = text_layout.hit_position(start).point.x;
                let x1 = text_layout.hit_position(end).point.x;
                extra_style.push(LineExtraStyle {
                    x: x0,
                    width: Some(x1 - x0),
                    bg_color: phantom.bg,
                    under_line: phantom.under_line,
                    wave_line: None,
                });
            }
        }

        // Add the styling for the diagnostic severity, if applicable
        if let Some(max_severity) = phantom_text.max_severity {
            let theme_prop = if max_severity == DiagnosticSeverity::ERROR {
                LapceColor::ERROR_LENS_ERROR_BACKGROUND
            } else if max_severity == DiagnosticSeverity::WARNING {
                LapceColor::ERROR_LENS_WARNING_BACKGROUND
            } else {
                LapceColor::ERROR_LENS_OTHER_BACKGROUND
            };

            let x1 = (!config.editor.error_lens_end_of_line)
                .then(|| text_layout.hit_position(line_content.len()).point.x);

            extra_style.push(LineExtraStyle {
                x: 0.0,
                width: x1,
                bg_color: Some(*config.get_color(theme_prop)),
                under_line: None,
                wave_line: None,
            });
        }

        for diag in self.diagnostics().iter() {
            if diag.diagnostic.range.start.line as usize <= line
                && line <= diag.diagnostic.range.end.line as usize
            {
                let start = if diag.diagnostic.range.start.line as usize == line {
                    let (_, col) = self.offset_to_line_col(diag.range.0);
                    col
                } else {
                    let offset = self.first_non_blank_character_on_line(line);
                    let (_, col) = self.offset_to_line_col(offset);
                    col
                };
                let start = phantom_text.col_after(start, true);

                let end = if diag.diagnostic.range.end.line as usize == line {
                    let (_, col) = self.offset_to_line_col(diag.range.1);
                    col
                } else {
                    self.line_end_col(line, true)
                };
                let end = phantom_text.col_after(end, false);

                let x0 = text_layout.hit_position(start).point.x;
                let x1 = text_layout.hit_position(end).point.x;
                let color_name = match diag.diagnostic.severity {
                    Some(DiagnosticSeverity::ERROR) => LapceColor::LAPCE_ERROR,
                    _ => LapceColor::LAPCE_WARN,
                };
                let color = *config.get_color(color_name);
                extra_style.push(LineExtraStyle {
                    x: x0,
                    width: Some(x1 - x0),
                    bg_color: None,
                    under_line: None,
                    wave_line: Some(color),
                });
            }
        }

        TextLayoutLine {
            text: text_layout,
            extra_style,
            whitespaces: None,
            indent: 0.0,
        }
    }

    /// Get the text layout for the given line.  
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout(
        &self,
        line: usize,
        font_size: usize,
    ) -> Arc<TextLayoutLine> {
        // TODO: Should we just move the config cache check into `check_cache`?
        let config = self.config.get_untracked();
        // Check if the text layout needs to update due to the config being changed
        self.text_layouts.borrow_mut().check_attributes(config.id);
        // If we don't have a second layer of the hashmap initialized for this specific font size,
        // do it now
        if self.text_layouts.borrow().layouts.get(&font_size).is_none() {
            let mut cache = self.text_layouts.borrow_mut();
            cache.layouts.insert(font_size, HashMap::new());
        }

        // Get whether there's an entry for this specific font size and line
        let cache_exists = self
            .text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .is_some();
        // If there isn't an entry then we actually have to create it
        if !cache_exists {
            let text_layout = Arc::new(self.new_text_layout(line, font_size));
            let mut cache = self.text_layouts.borrow_mut();
            let width = text_layout.text.size().width;
            if width > cache.max_width {
                cache.max_width = width;
            }
            cache
                .layouts
                .get_mut(&font_size)
                .unwrap()
                .insert(line, text_layout);
        }

        // Just get the entry, assuming it has been created because we initialize it above.
        self.text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .cloned()
            .unwrap()
    }

    pub fn indent_unit(&self) -> &'static str {
        self.doc.with_untracked(|doc| doc.buffer().indent_unit())
    }

    // ==== Position Information ====

    /// The number of visual lines in the document.
    pub fn num_lines(&self) -> usize {
        self.doc.with_untracked(|doc| doc.buffer().num_lines())
    }

    /// The last allowed line in the document.
    pub fn last_line(&self) -> usize {
        self.doc.with_untracked(|doc| doc.buffer().last_line())
    }

    // ==== Line/Column Positioning ====

    /// Convert an offset into the buffer into a line and column.
    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        self.doc
            .with_untracked(|doc| doc.buffer().offset_to_line_col(offset))
    }

    pub fn offset_of_line(&self, offset: usize) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().offset_of_line(offset))
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().offset_of_line_col(line, col))
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().line_of_offset(offset))
    }

    /// Returns the offset into the buffer of the first non blank character on the given line.
    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer().first_non_blank_character_on_line(line)
        })
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().line_end_col(line, caret))
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        self.doc
            .with_untracked(|doc| doc.buffer().select_word(offset))
    }

    // ==== Points of locations ====

    /// Returns the point into the text layout of the line at the given offset.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_offset(&self, offset: usize, font_size: usize) -> Point {
        let (line, col) = self.offset_to_line_col(offset);
        self.line_point_of_line_col(line, col, font_size)
    }

    /// Returns the point into the text layout of the line at the given line and column.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_line_col(
        &self,
        line: usize,
        col: usize,
        font_size: usize,
    ) -> Point {
        let text_layout = self.get_text_layout(line, font_size);
        text_layout.text.hit_position(col).point
    }

    /// Get the (point above, point below) of a particular offset within the editor.
    pub fn points_of_offset(&self, offset: usize) -> (Point, Point) {
        let (line, col) = self.offset_to_line_col(offset);
        self.points_of_line_col(line, col)
    }

    /// Get the (point above, point below) of a particular (line, col) within the editor.
    pub fn points_of_line_col(&self, line: usize, col: usize) -> (Point, Point) {
        let config = self.config.get_untracked();
        let (y, line_height, font_size) = (
            config.editor.line_height() * line,
            config.editor.line_height(),
            config.editor.font_size(),
        );

        let line = line.min(self.last_line());

        let phantom_text = self.line_phantom_text(line);
        let col = phantom_text.col_after(col, false);

        let mut x_shift = 0.0;
        if font_size < config.editor.font_size() {
            let mut col = 0usize;
            self.doc.with_untracked(|doc| {
                let line_content = doc.buffer().line_content(line);
                for ch in line_content.chars() {
                    if ch == ' ' || ch == '\t' {
                        col += 1;
                    } else {
                        break;
                    }
                }
            });

            if col > 0 {
                let normal_text_layout =
                    self.get_text_layout(line, config.editor.font_size());
                let small_text_layout = self.get_text_layout(line, font_size);
                x_shift = normal_text_layout.text.hit_position(col).point.x
                    - small_text_layout.text.hit_position(col).point.x;
            }
        }

        let x = self.line_point_of_line_col(line, col, font_size).x + x_shift;
        (
            Point::new(x, y as f64),
            Point::new(x, (y + line_height) as f64),
        )
    }

    /// Get the offset of a particular point within the editor.  
    /// The boolean indicates whether the point is inside the text or not
    /// Points outside of vertical bounds will return the last line.
    /// Points outside of horizontal bounds will return the last column on the line.
    pub fn offset_of_point(&self, mode: Mode, point: Point) -> (usize, bool) {
        let ((line, col), is_inside) = self.line_col_of_point(mode, point);
        (self.offset_of_line_col(line, col), is_inside)
    }

    /// Get the (line, col) of a particular point within the editor.
    /// The boolean indicates whether the point is within the text bounds.  
    /// Points outside of vertical bounds will return the last line.
    /// Points outside of horizontal bounds will return the last column on the line.
    pub fn line_col_of_point(
        &self,
        mode: Mode,
        point: Point,
    ) -> ((usize, usize), bool) {
        let config = self.config.get_untracked();

        let line = (point.y / config.editor.line_height() as f64).floor() as usize;
        let line = line.min(self.last_line());
        let font_size = config.editor.font_size();
        let text_layout = self.get_text_layout(line, font_size);
        let hit_point = text_layout.text.hit_point(Point::new(point.x, 0.0));
        // We have to unapply the phantom text shifting in order to get back to the column in
        // the actual buffer
        let phantom_text = self.line_phantom_text(line);
        let col = phantom_text.before_col(hit_point.index);
        // Ensure that the column doesn't end up out of bounds, so things like clicking on the far
        // right end will just go to the end of the line.
        let max_col = self.line_end_col(line, mode != Mode::Normal);
        let mut col = col.min(max_col);

        if config.editor.atomic_soft_tabs && config.editor.tab_width > 1 {
            col = snap_to_soft_tab_line_col(
                &self.text(),
                line,
                col,
                SnapDirection::Nearest,
                config.editor.tab_width,
            );
        }

        ((line, col), hit_point.is_inside)
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        font_size: usize,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let text_layout = self.get_text_layout(line, font_size);
                let hit_point = text_layout.text.hit_point(Point::new(x, 0.0));
                let n = hit_point.index;

                n.min(self.line_end_col(line, caret))
            }
            ColPosition::End => self.line_end_col(line, caret),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.first_non_blank_character_on_line(line)
            }
        }
    }

    /// Advance to the right in the manner of the given mode.  
    /// This is not the same as the [`Movement::Right`] command.
    pub fn move_right(&self, offset: usize, mode: Mode, count: usize) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().move_right(offset, mode, count))
    }

    /// Advance to the left in the manner of the given mode.
    /// This is not the same as the [`Movement::Left`] command.
    pub fn move_left(&self, offset: usize, mode: Mode, count: usize) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer().move_left(offset, mode, count))
    }

    /// Find the next/previous offset of the match of the given character.
    /// This is intended for use by the [`Movement::NextUnmatched`] and
    /// [`Movement::PreviousUnmatched`] commands.
    pub fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        // This needs the doc's syntax, but it isn't cheap to clone
        // so this has to be a method on view for now.
        self.doc.with_untracked(|doc| {
            if let Some(syntax) = doc.syntax() {
                syntax
                    .find_tag(offset, previous, &CharBuffer::from(ch))
                    .unwrap_or(offset)
            } else {
                let text = self.text();
                let mut cursor = WordCursor::new(&text, offset);
                let new_offset = if previous {
                    cursor.previous_unmatched(ch)
                } else {
                    cursor.next_unmatched(ch)
                };

                new_offset.unwrap_or(offset)
            }
        })
    }

    /// Find the offset of the matching pair character.  
    /// This is intended for use by the [`Movement::MatchPairs`] command.
    pub fn find_matching_pair(&self, offset: usize) -> usize {
        // This needs the doc's syntax, but it isn't cheap to clone
        // so this has to be a method on view for now.
        self.doc.with_untracked(|doc| {
            if let Some(syntax) = doc.syntax() {
                syntax.find_matching_pair(offset).unwrap_or(offset)
            } else {
                WordCursor::new(&self.text(), offset)
                    .match_pairs()
                    .unwrap_or(offset)
            }
        })
    }
}

impl TextCacheListener for RefCell<TextLayoutCache> {
    fn clear(&self) {
        self.borrow_mut().clear();
    }
}

fn changes_colors(
    changes: im::Vector<DiffLines>,
    min_line: usize,
    max_line: usize,
    config: &LapceConfig,
) -> Vec<(usize, usize, bool, Color)> {
    let mut line = 0;
    let mut last_change = None;
    let mut colors = Vec::new();
    for change in changes.iter() {
        let len = match change {
            DiffLines::Left(_range) => 0,
            DiffLines::Skip(_left, right) => right.len(),
            DiffLines::Both(_left, right) => right.len(),
            DiffLines::Right(range) => range.len(),
        };
        line += len;
        if line < min_line {
            last_change = Some(change);
            continue;
        }

        let mut modified = false;
        let color = match change {
            DiffLines::Left(_range) => {
                Some(config.get_color(LapceColor::SOURCE_CONTROL_REMOVED))
            }
            DiffLines::Right(_range) => {
                if let Some(DiffLines::Left(_)) = last_change.as_ref() {
                    modified = true;
                }
                if modified {
                    Some(config.get_color(LapceColor::SOURCE_CONTROL_MODIFIED))
                } else {
                    Some(config.get_color(LapceColor::SOURCE_CONTROL_ADDED))
                }
            }
            _ => None,
        };

        if let Some(color) = color.cloned() {
            let y = line - len;
            let height = len;
            let removed = len == 0;

            if modified {
                colors.pop();
            }

            colors.push((y, height, removed, color));
        }

        if line > max_line {
            break;
        }
        last_change = Some(change);
    }
    colors
}
