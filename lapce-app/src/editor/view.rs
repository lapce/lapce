use std::{collections::HashMap, sync::Arc};

use floem::{
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    event::{Event, EventListener},
    glazier::{Modifiers, PointerType},
    id::Id,
    peniko::{
        kurbo::{BezPath, Line, Point, Rect, Size},
        Color,
    },
    reactive::{create_effect, create_memo, create_rw_signal, ReadSignal, RwSignal},
    style::{ComputedStyle, CursorStyle, Style},
    taffy::prelude::Node,
    view::{ChangeFlags, View},
    views::{clip, container, empty, label, list, scroll, stack, svg, Decorators},
    Renderer, ViewContext,
};
use lapce_core::{
    buffer::{diff::DiffLines, rope_text::RopeText},
    cursor::{ColPosition, CursorMode},
    mode::{Mode, VisualMode},
    selection::Selection,
};
use lapce_xi_rope::find::CaseMatching;

use super::{
    gutter::editor_gutter_view,
    view_data::{EditorViewData, LineExtraStyle},
    EditorData,
};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::{DocContent, Document},
    main_split::MainSplitData,
    text_input::text_input,
    workspace::LapceWorkspace,
};

pub enum DiffSectionKind {
    NoCode,
    Added,
    Removed,
}

pub struct DiffSection {
    pub start_line: usize,
    pub height: usize,
    pub kind: DiffSectionKind,
}

pub struct ScreenLines {
    pub lines: Vec<usize>,
    pub info: HashMap<usize, LineInfo>,
    pub diff_sections: Vec<DiffSection>,
}

pub struct LineInfo {
    // font_size: usize,
    // line_height: f64,
    // x: f64,
    pub y: usize,
}

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

    let viewport = create_rw_signal(Rect::ZERO);

    create_effect(move |_| {
        let kind = editor.with(|editor| editor.view.kind);
        kind.track();
        id.request_layout();
    });

    create_effect(move |last_rev| {
        let doc = editor.with(|editor| editor.view.doc);
        let rev = doc.with(|doc| doc.rev());
        if last_rev == Some(rev) {
            return rev;
        }
        id.request_layout();
        rev
    });

    create_effect(move |last_rev| {
        let (doc, sticky_header_height_signal, config) = editor.with(|editor| {
            (
                editor.view.doc,
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
                doc.cache_rev(),
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
    fn paint_diff_sections(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        config: &LapceConfig,
    ) {
        for section in &screen_lines.diff_sections {
            match section.kind {
                DiffSectionKind::NoCode => self.paint_diff_no_code(
                    cx,
                    viewport,
                    section.start_line,
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
                                (section.start_line * config.editor.line_height())
                                    as f64,
                            )),
                        config
                            .get_color(LapceColor::SOURCE_CONTROL_ADDED)
                            .with_alpha_factor(0.2),
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
                                (section.start_line * config.editor.line_height())
                                    as f64,
                            )),
                        config
                            .get_color(LapceColor::SOURCE_CONTROL_REMOVED)
                            .with_alpha_factor(0.2),
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
                    *config.get_color(LapceColor::EDITOR_DIM),
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
                7.5,
                is_active,
                screen_lines,
            ),
            CursorMode::Insert(selection) => {
                insert_cursor(&view, selection, 7.5, is_active, screen_lines)
            }
        });

        for render in renders {
            match render {
                CursorRender::CurrentLine { line } => {
                    if !is_local {
                        if let Some(info) = screen_lines.info.get(&line) {
                            cx.fill(
                                &Rect::ZERO
                                    .with_size(Size::new(
                                        viewport.width(),
                                        line_height,
                                    ))
                                    .with_origin(Point::new(
                                        viewport.x0,
                                        info.y as f64,
                                    )),
                                config.get_color(LapceColor::EDITOR_CURRENT_LINE),
                            );
                        }
                    }
                }
                CursorRender::Selection { x, width, line } => {
                    if let Some(info) = screen_lines.info.get(&line) {
                        cx.fill(
                            &Rect::ZERO
                                .with_size(Size::new(width, line_height))
                                .with_origin(Point::new(x, info.y as f64)),
                            config.get_color(LapceColor::EDITOR_SELECTION),
                        );
                    }
                }
                CursorRender::Caret { x, width, line } => {
                    if let Some(info) = screen_lines.info.get(&line) {
                        cx.fill(
                            &Rect::ZERO
                                .with_size(Size::new(width, line_height))
                                .with_origin(Point::new(x, info.y as f64)),
                            config.get_color(LapceColor::EDITOR_CARET),
                        );
                    }
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
        viewport: Rect,
        screen_lines: &ScreenLines,
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

        for line in &screen_lines.lines {
            let line = *line;
            if line > last_line {
                break;
            }

            let info = screen_lines.info.get(&line).unwrap();
            let text_layout = view.get_text_layout(line, font_size);
            let height = text_layout.text.size().height;
            let y = info.y;

            self.paint_extra_style(
                cx,
                &text_layout.extra_style,
                y as f64,
                height,
                line_height,
                viewport,
            );

            if let Some(whitespaces) = &text_layout.whitespaces {
                let family: Vec<FamilyOwned> =
                    FamilyOwned::parse_list(&config.editor.font_family).collect();
                let attrs = Attrs::new()
                    .color(*config.get_color(LapceColor::EDITOR_VISIBLE_WHITESPACE))
                    .family(&family)
                    .font_size(config.editor.font_size() as f32);
                let attrs_list = AttrsList::new(attrs);
                let mut space_text = TextLayout::new();
                space_text.set_text("·", attrs_list.clone());
                let mut tab_text = TextLayout::new();
                tab_text.set_text("→", attrs_list);

                for (c, (x0, _x1)) in whitespaces.iter() {
                    match *c {
                        '\t' => {
                            cx.draw_text(
                                &tab_text,
                                Point::new(
                                    *x0,
                                    y as f64 + (line_height - height) / 2.0,
                                ),
                            );
                        }
                        ' ' => {
                            cx.draw_text(
                                &space_text,
                                Point::new(
                                    *x0,
                                    y as f64 + (line_height - height) / 2.0,
                                ),
                            );
                        }
                        _ => {}
                    }
                }
            }

            if config.editor.show_indent_guide {
                let mut x = 0.0;
                while x + 1.0 < text_layout.indent {
                    cx.stroke(
                        &Line::new(
                            Point::new(x, y as f64),
                            Point::new(x, y as f64 + line_height),
                        ),
                        config.get_color(LapceColor::EDITOR_INDENT_GUIDE),
                        1.0,
                    );
                    x += indent_text_width;
                }
            }

            cx.draw_text(
                &text_layout.text,
                Point::new(0.0, y as f64 + (line_height - height) / 2.0),
            );
        }
    }

    fn paint_find(&self, cx: &mut PaintCx, screen_lines: &ScreenLines) {
        let visual = self.editor.with_untracked(|e| e.common.find.visual);
        if !visual.get_untracked() {
            return;
        }
        if screen_lines.lines.is_empty() {
            return;
        }

        let min_line = *screen_lines.lines.first().unwrap();
        let max_line = *screen_lines.lines.last().unwrap();

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
        for region in occurrences.with_untracked(|selection| {
            selection.regions_in_range(start, end).to_vec()
        }) {
            let start = region.min();
            let end = region.max();
            let (start_line, start_col) = view.offset_to_line_col(start);
            let (end_line, end_col) = view.offset_to_line_col(end);
            for line in &screen_lines.lines {
                let line = *line;
                if line < start_line {
                    continue;
                }

                if line > end_line {
                    break;
                }

                let info = screen_lines.info.get(&line).unwrap();

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
                            .with_origin(Point::new(x0, info.y as f64)),
                    );
                }
            }
        }

        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        for rect in rects {
            cx.stroke(&rect, color, 1.0);
        }
    }

    fn paint_sticky_headers(&self, cx: &mut PaintCx, viewport: Rect) {
        let (view, editor_view, config) = self.editor.with_untracked(|editor| {
            (editor.view.clone(), editor.view.kind, editor.common.config)
        });
        let config = config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }
        if !editor_view.get_untracked().is_normal() {
            return;
        }

        let line_height = config.editor.line_height() as f64;
        let start_line = (viewport.y0 / line_height).floor() as usize;

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

        let editor_view = self.editor.with_untracked(|editor| editor.view.kind);
        if !editor_view.get_untracked().is_normal() {
            return;
        }

        let doc = self.editor.with_untracked(|e| e.view.doc);
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

    fn child(&self, _id: floem::id::Id) -> Option<&dyn View> {
        None
    }

    fn child_mut(&mut self, _id: floem::id::Id) -> Option<&mut dyn View> {
        None
    }

    fn children(&self) -> Vec<&dyn View> {
        Vec::new()
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
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

            let (doc, view, config) = self.editor.with_untracked(|editor| {
                (editor.view.doc, editor.view.clone(), editor.common.config)
            });
            let config = config.get_untracked();
            let line_height = config.editor.line_height() as f64;
            let font_size = config.editor.font_size();

            let screen_lines =
                self.editor.with_untracked(|editor| editor.screen_lines());
            for line in screen_lines.lines {
                view.get_text_layout(line, font_size);
            }

            let (width, height) = doc.with_untracked(|doc| {
                let width = view.text_layouts.borrow().max_width + 20.0;
                let height = line_height
                    * (view.visual_line(doc.buffer().last_line()) + 1) as f64;
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
        let (config, screen_lines) = self
            .editor
            .with_untracked(|e| (e.common.config, e.screen_lines()));
        let config = config.get_untracked();

        let doc = self.editor.with_untracked(|e| e.view.doc);
        let is_local = doc.with_untracked(|doc| doc.content.is_local());

        self.paint_cursor(cx, is_local, &screen_lines);
        self.paint_diff_sections(cx, viewport, &screen_lines, &config);
        self.paint_find(cx, &screen_lines);
        self.paint_text(cx, viewport, &screen_lines);
        self.paint_sticky_headers(cx, viewport);
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
    char_width: f64,
    is_active: bool,
    screen_lines: &ScreenLines,
) -> Vec<CursorRender> {
    let (start_line, start_col) = view.offset_to_line_col(start.min(end));
    let (end_line, end_col) = view.offset_to_line_col(start.max(end));
    let (cursor_line, _) = view.offset_to_line_col(end);

    let mut renders = Vec::new();

    for line in &screen_lines.lines {
        let line = *line;
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
    char_width: f64,
    is_active: bool,
    screen_lines: &ScreenLines,
) -> Vec<CursorRender> {
    if screen_lines.lines.is_empty() {
        return Vec::new();
    }
    let start_line = *screen_lines.lines.first().unwrap();
    let end_line = *screen_lines.lines.last().unwrap();
    let start = view.offset_of_line(start_line);
    let end = view.offset_of_line(end_line + 1);
    let regions = selection.regions_in_range(start, end);

    let mut renders = Vec::new();

    for region in regions {
        let cursor_offset = region.end;
        let (cursor_line, _) = view.offset_to_line_col(cursor_offset);
        let start = region.start;
        let end = region.end;
        let (start_line, start_col) = view.offset_to_line_col(start.min(end));
        let (end_line, end_col) = view.offset_to_line_col(start.max(end));
        for line in &screen_lines.lines {
            let line = *line;
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
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (find_focus, sticky_header_height, editor_view, config) = editor
        .with_untracked(|editor| {
            (
                editor.find_focus,
                editor.sticky_header_height,
                editor.view.kind,
                editor.common.config,
            )
        });

    let scratch_docs = main_split.scratch_docs;
    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;

    let editor_rect = create_rw_signal(Rect::ZERO);

    stack(move || {
        (
            editor_breadcrumbs(workspace, editor, config),
            container(|| {
                stack(|| {
                    (
                        editor_gutter(editor, is_active),
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
    .on_cleanup(move || {
        let (editor_cx, doc) =
            editor.with_untracked(|editor| (editor.scope, editor.view.doc));
        editor_cx.dispose();

        let scratch_doc_name = doc.with_untracked(|doc| {
            if let DocContent::Scratch { name, .. } = &doc.content {
                Some(name.to_string())
            } else {
                None
            }
        });
        if let Some(name) = scratch_doc_name {
            if !scratch_docs
                .with_untracked(|scratch_docs| scratch_docs.contains_key(&name))
            {
                doc.with_untracked(|doc| doc.scope).dispose();
            }
        }
    })
    .style(|| Style::BASE.flex_col().size_pct(100.0, 100.0))
}

fn editor_gutter(
    editor: RwSignal<EditorData>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let padding_left = 10.0;
    let padding_right = 30.0;

    let (cursor, viewport, scroll_delta, config) = editor
        .with_untracked(|e| (e.cursor, e.viewport, e.scroll_delta, e.common.config));

    let code_action_line = create_memo(move |_| {
        if is_active(true) {
            let doc = editor.with(|editor| editor.view.doc);
            let offset = cursor.with(|cursor| cursor.offset());
            doc.with(|doc| {
                let has_code_actions = doc
                    .code_actions
                    .get(&offset)
                    .map(|c| !c.1.is_empty())
                    .unwrap_or(false);
                if has_code_actions {
                    let line = doc.buffer().line_of_offset(offset);
                    Some(line)
                } else {
                    None
                }
            })
        } else {
            None
        }
    });

    let gutter_rect = create_rw_signal(Rect::ZERO);
    let gutter_width = create_memo(move |_| gutter_rect.get().width());

    stack(move || {
        (
            stack(|| {
                (
                    empty().style(move || Style::BASE.width_px(padding_left)),
                    label(move || {
                        let doc = editor.with(|e| e.view.doc);
                        doc.with(|doc| (doc.buffer().last_line() + 1).to_string())
                    }),
                    empty().style(move || Style::BASE.width_px(padding_right)),
                )
            })
            .style(|| Style::BASE.height_pct(100.0)),
            clip(|| {
                stack(|| {
                    (
                        editor_gutter_view(editor)
                            .on_resize(move |_, rect| {
                                gutter_rect.set(rect);
                            })
                            .on_event(EventListener::PointerWheel, move |event| {
                                if let Event::PointerWheel(pointer_event) = event {
                                    if let PointerType::Mouse(info) =
                                        &pointer_event.pointer_type
                                    {
                                        scroll_delta.set(info.wheel_delta);
                                    }
                                }
                                true
                            })
                            .style(|| Style::BASE.size_pct(100.0, 100.0)),
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
                            let config = config.get();
                            let viewport = viewport.get();
                            let gutter_width = gutter_width.get();
                            let code_action_line = code_action_line.get();
                            let size = config.ui.icon_size() as f32;
                            let margin_left = gutter_width as f32
                                + (padding_right - size) / 2.0
                                - 4.0;
                            let line_height = config.editor.line_height();
                            let margin_top = if let Some(line) = code_action_line {
                                (line * line_height) as f32 - viewport.y0 as f32
                                    + (line_height as f32 - size) / 2.0
                                    - 4.0
                            } else {
                                0.0
                            };
                            Style::BASE
                                .absolute()
                                .padding_px(4.0)
                                .border_radius(6.0)
                                .margin_left_px(margin_left)
                                .margin_top_px(margin_top)
                                .apply_if(code_action_line.is_none(), |s| s.hide())
                        })
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .active_style(move || {
                            Style::BASE.background(*config.get().get_color(
                                LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                            ))
                        }),
                    )
                })
                .style(|| Style::BASE.size_pct(100.0, 100.0))
            })
            .style(move || {
                Style::BASE
                    .absolute()
                    .size_pct(100.0, 100.0)
                    .background(
                        *config.get().get_color(LapceColor::EDITOR_BACKGROUND),
                    )
                    .padding_left_px(padding_left)
                    .padding_right_px(padding_right)
            }),
        )
    })
    .style(|| Style::BASE.height_pct(100.0))
}

fn editor_breadcrumbs(
    workspace: Arc<LapceWorkspace>,
    editor: RwSignal<EditorData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let doc_path = create_memo(move |_| {
        let doc = editor.with(|editor| editor.view.doc);
        doc.with(|doc| {
            if let DocContent::History(history) = &doc.content {
                Some(history.path.clone())
            } else {
                doc.content.path().cloned()
            }
        })
    });
    container(move || {
        scroll(move || {
            stack(|| {
                (
                    {
                        let workspace = workspace.clone();
                        list(
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
                                            config.get().ui_svg(
                                                LapceIcons::BREADCRUMB_SEPARATOR,
                                            )
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
                    },
                    label(move || {
                        let doc = editor.with(|editor| editor.view.doc);
                        doc.with_untracked(|doc| {
                            if let DocContent::History(history) = &doc.content {
                                format!("({})", history.version)
                            } else {
                                "".to_string()
                            }
                        })
                    })
                    .style(move || {
                        let doc = editor.with(|editor| editor.view.doc);
                        let is_history = doc.with_untracked(|doc| {
                            matches!(&doc.content, DocContent::History(_))
                        });

                        Style::BASE
                            .padding_right_px(10.0)
                            .apply_if(!is_history, |s| s.hide())
                    }),
                )
            })
            .style(|| Style::BASE.items_center())
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
        })
    })
    .style(move || {
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        Style::BASE
            .items_center()
            .width_pct(100.0)
            .height_px(line_height as f32)
            .apply_if(doc_path.get().is_none(), |s| s.hide())
    })
}

fn editor_content(
    editor: RwSignal<EditorData>,
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
        let editor_content_view = editor_view(editor, move || is_active(false))
            .style(move || {
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
            .on_event(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    let editor = editor.get_untracked();
                    editor.pointer_down(pointer_event);
                }
                false
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
                .with_origin(Point::new(
                    x,
                    (view.visual_line(line) * line_height) as f64,
                ))
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
    is_active: impl Fn(bool) -> bool + 'static + Copy,
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
                is_active(true)
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
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    find_focus: RwSignal<bool>,
) -> impl View {
    let config = replace_editor.common.config;
    let visual = replace_editor.common.find.visual;

    stack(|| {
        (
            text_input(replace_editor, move || {
                is_active(true)
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
    is_active: impl Fn(bool) -> bool + 'static + Copy,
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

pub fn changes_colors(
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
            DiffLines::Both(info) => info.right.len(),
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
