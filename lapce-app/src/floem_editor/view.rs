use std::{collections::HashMap, ops::RangeInclusive, rc::Rc};

use floem::{
    context::PaintCx,
    id::Id,
    kurbo::{BezPath, Line, Point, Rect, Size},
    peniko::Color,
    reactive::{RwSignal, Scope},
    view::{View, ViewData},
    views::{empty, stack},
    Renderer,
};
use lapce_core::{
    cursor::{ColPosition, CursorAffinity, CursorMode},
    mode::{Mode, VisualMode},
};

use crate::{
    doc::phantom_text::PhantomTextKind,
    editor::{
        view_data::LineExtraStyle,
        visual_line::{RVLine, VLineInfo},
    },
};

use super::{
    color::EditorColor,
    editor::{Editor, CHAR_WIDTH},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffSectionKind {
    NoCode,
    Added,
    Removed,
}

#[derive(Clone, PartialEq)]
pub struct DiffSection {
    /// The y index that the diff section is at.  
    /// This is multiplied by the line height to get the y position.  
    /// So this can roughly be considered as the `VLine of the start of this diff section, but it
    /// isn't necessarily convertable to a `VLine` due to jumping over empty code sections.
    pub y_idx: usize,
    pub height: usize,
    pub kind: DiffSectionKind,
}

#[derive(Clone, PartialEq)]
pub struct ScreenLines {
    pub lines: Rc<Vec<RVLine>>,
    /// Guaranteed to have an entry for each `VLine` in `lines`  
    /// You should likely use accessor functions rather than this directly.
    pub info: Rc<HashMap<RVLine, LineInfo>>,
    pub diff_sections: Option<Rc<Vec<DiffSection>>>,
    /// The base y position that all the y positions inside `info` are relative to.  
    /// This exists so that if a text layout is created outside of the view, we don't have to
    /// completely recompute the screen lines (or do somewhat intricate things to update them)
    /// we simply have to update the `base_y`.
    pub base: RwSignal<ScreenLinesBase>,
}
impl ScreenLines {
    pub fn new(cx: Scope, viewport: Rect) -> ScreenLines {
        ScreenLines {
            lines: Default::default(),
            info: Default::default(),
            diff_sections: Default::default(),
            base: cx.create_rw_signal(ScreenLinesBase {
                active_viewport: viewport,
            }),
        }
    }

    pub fn clear(&mut self, viewport: Rect) {
        self.lines = Default::default();
        self.info = Default::default();
        self.diff_sections = Default::default();
        self.base.set(ScreenLinesBase {
            active_viewport: viewport,
        });
    }

    /// Get the line info for the given rvline.  
    pub fn info(&self, rvline: RVLine) -> Option<LineInfo> {
        let info = self.info.get(&rvline)?;
        let base = self.base.get();

        Some(info.clone().with_base(base))
    }

    pub fn vline_info(&self, rvline: RVLine) -> Option<VLineInfo<()>> {
        self.info.get(&rvline).map(|info| info.vline_info)
    }

    pub fn rvline_range(&self) -> Option<(RVLine, RVLine)> {
        self.lines.first().copied().zip(self.lines.last().copied())
    }

    /// Iterate over the line info, copying them with the full y positions.  
    pub fn iter_line_info(&self) -> impl Iterator<Item = LineInfo> + '_ {
        self.lines.iter().map(|rvline| self.info(*rvline).unwrap())
    }

    /// Iterate over the line info within the range, copying them with the full y positions.  
    /// If the values are out of range, it is clamped to the valid lines within.
    pub fn iter_line_info_r(
        &self,
        r: RangeInclusive<RVLine>,
    ) -> impl Iterator<Item = LineInfo> + '_ {
        // We search for the start/end indices due to not having a good way to iterate over
        // successive rvlines without the view.
        // This should be good enough due to lines being small.
        let start_idx = self.lines.binary_search(r.start()).ok().or_else(|| {
            if self.lines.first().map(|l| r.start() < l).unwrap_or(false) {
                Some(0)
            } else {
                // The start is past the start of our lines
                None
            }
        });

        let end_idx = self.lines.binary_search(r.end()).ok().or_else(|| {
            if self.lines.last().map(|l| r.end() > l).unwrap_or(false) {
                Some(self.lines.len())
            } else {
                // The end is before the end of our lines but not available
                None
            }
        });

        if let (Some(start_idx), Some(end_idx)) = (start_idx, end_idx) {
            self.lines.get(start_idx..=end_idx)
        } else {
            // Hacky method to get an empty iterator of the same type
            self.lines.get(0..0)
        }
        .into_iter()
        .flatten()
        .copied()
        .map(|rvline| self.info(rvline).unwrap())
    }

    pub fn iter_vline_info(&self) -> impl Iterator<Item = VLineInfo<()>> + '_ {
        self.lines
            .iter()
            .map(|vline| &self.info[vline].vline_info)
            .copied()
    }

    pub fn iter_vline_info_r(
        &self,
        r: RangeInclusive<RVLine>,
    ) -> impl Iterator<Item = VLineInfo<()>> + '_ {
        // TODO(minor): this should probably skip tracking?
        self.iter_line_info_r(r).map(|x| x.vline_info)
    }

    /// Iter the real lines underlying the visual lines on the screen
    pub fn iter_lines(&self) -> impl Iterator<Item = usize> + '_ {
        // We can just assume that the lines stored are contiguous and thus just get the first
        // buffer line and then the last buffer line.
        let start_vline = self.lines.first().copied().unwrap_or_default();
        let end_vline = self.lines.last().copied().unwrap_or_default();

        let start_line = self.info(start_vline).unwrap().vline_info.rvline.line;
        let end_line = self.info(end_vline).unwrap().vline_info.rvline.line;

        start_line..=end_line
    }

    /// Iterate over the real lines underlying the visual lines on the screen with the y position
    /// of their layout.  
    /// (line, y)  
    pub fn iter_lines_y(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        let mut last_line = None;
        self.lines.iter().filter_map(move |vline| {
            let info = self.info(*vline).unwrap();

            let line = info.vline_info.rvline.line;

            if last_line == Some(line) {
                // We've already considered this line.
                return None;
            }

            last_line = Some(line);

            Some((line, info.y))
        })
    }

    /// Get the earliest line info for a given line.
    pub fn info_for_line(&self, line: usize) -> Option<LineInfo> {
        self.info(self.first_rvline_for_line(line)?)
    }

    /// Get the earliest rvline for the given line
    pub fn first_rvline_for_line(&self, line: usize) -> Option<RVLine> {
        self.lines
            .iter()
            .find(|rvline| rvline.line == line)
            .copied()
    }

    /// Get the latest rvline for the given line
    pub fn last_rvline_for_line(&self, line: usize) -> Option<RVLine> {
        self.lines
            .iter()
            .rfind(|rvline| rvline.line == line)
            .copied()
    }

    // TODO: on_created_layout
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenLinesBase {
    /// The current/previous viewport.  
    /// Used for determining whether there were any changes, and the `y0` serves as the
    /// base for positioning the lines.
    pub active_viewport: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    // font_size: usize,
    // line_height: f64,
    // x: f64,
    /// The starting y position of the overall line that this vline
    /// is a part of.
    pub y: f64,
    /// The y position of the visual line
    pub vline_y: f64,
    pub vline_info: VLineInfo<()>,
}
impl LineInfo {
    pub fn with_base(mut self, base: ScreenLinesBase) -> Self {
        self.y += base.active_viewport.y0;
        self.vline_y += base.active_viewport.y0;
        self
    }
}

pub struct EditorView {
    id: Id,
    data: ViewData,
    editor: Rc<Editor>,
}
impl EditorView {
    #[allow(clippy::too_many_arguments)]
    fn paint_normal_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        is_block_cursor: bool,
    ) {
        let ed = &self.editor;

        // TODO: selections should have separate start/end affinity
        let (start_rvline, start_col) =
            ed.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, end_col) = ed.rvline_col_of_offset(end_offset, affinity);

        for LineInfo {
            vline_y,
            vline_info: info,
            ..
        } in screen_lines.iter_line_info_r(start_rvline..=end_rvline)
        {
            let rvline = info.rvline;
            let line = rvline.line;

            let phantom_text = ed.phantom_text(line);
            let left_col = if rvline == start_rvline {
                start_col
            } else {
                ed.first_col(info)
            };
            let right_col = if rvline == end_rvline {
                end_col
            } else {
                ed.last_col(info, true)
            };
            let left_col = phantom_text.col_after(left_col, is_block_cursor);
            let right_col = phantom_text.col_after(right_col, false);

            // Skip over empty selections
            if !info.is_empty() && left_col == right_col {
                continue;
            }

            // TODO: What affinity should these use?
            let x0 = ed
                .line_point_of_line_col(line, left_col, CursorAffinity::Forward)
                .x;
            let x1 = ed
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x;
            // TODO(minor): Should this be line != end_line?
            let x1 = if rvline != end_rvline {
                x1 + CHAR_WIDTH
            } else {
                x1
            };

            let (x0, width) = if info.is_empty() {
                let text_layout = ed.text_layout(line);
                let width = text_layout
                    .get_layout_x(rvline.line_index)
                    .map(|(_, x1)| x1)
                    .unwrap_or(0.0)
                    .into();
                (0.0, width)
            } else {
                (x0, x1 - x0)
            };

            let line_height = ed.style().line_height(line);
            let rect = Rect::from_origin_size(
                (x0, vline_y),
                (width, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_linewise_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
    ) {
        let ed = &self.editor;
        let viewport = self.editor.viewport.get_untracked();

        let (start_rvline, _) = ed.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, _) = ed.rvline_col_of_offset(end_offset, affinity);
        // Linewise selection is by *line* so we move to the start/end rvlines of the line
        let start_rvline = screen_lines
            .first_rvline_for_line(start_rvline.line)
            .unwrap_or(start_rvline);
        let end_rvline = screen_lines
            .last_rvline_for_line(end_rvline.line)
            .unwrap_or(end_rvline);

        for LineInfo {
            vline_info: info,
            vline_y,
            ..
        } in screen_lines.iter_line_info_r(start_rvline..=end_rvline)
        {
            let rvline = info.rvline;
            let line = rvline.line;

            // TODO: give ed a phantom_col_after
            let phantom_text = ed.phantom_text(line);

            // The left column is always 0 for linewise selections.
            let right_col = ed.last_col(info, true);
            let right_col = phantom_text.col_after(right_col, false);

            // TODO: what affinity to use?
            let x1 = ed
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x
                + CHAR_WIDTH;

            let line_height = ed.style().line_height(line);
            let rect = Rect::from_origin_size(
                (viewport.x0, vline_y),
                (x1 - viewport.x0, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_blockwise_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        horiz: Option<ColPosition>,
    ) {
        let ed = &self.editor;

        let (start_rvline, start_col) =
            ed.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, end_col) = ed.rvline_col_of_offset(end_offset, affinity);
        let left_col = start_col.min(end_col);
        let right_col = start_col.max(end_col) + 1;

        let lines = screen_lines
            .iter_line_info_r(start_rvline..=end_rvline)
            .filter_map(|line_info| {
                let max_col = ed.last_col(line_info.vline_info, true);
                (max_col > left_col).then_some((line_info, max_col))
            });

        for (line_info, max_col) in lines {
            let line = line_info.vline_info.rvline.line;
            let right_col = if let Some(ColPosition::End) = horiz {
                max_col
            } else {
                right_col.min(max_col)
            };
            let phantom_text = ed.phantom_text(line);
            let left_col = phantom_text.col_after(left_col, true);
            let right_col = phantom_text.col_after(right_col, false);

            // TODO: what affinity to use?
            let x0 = ed
                .line_point_of_line_col(line, left_col, CursorAffinity::Forward)
                .x;
            let x1 = ed
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x;

            let line_height = ed.style().line_height(line);
            let rect = Rect::from_origin_size(
                (x0, line_info.vline_y),
                (x1 - x0, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    fn paint_cursor(
        &self,
        cx: &mut PaintCx,
        is_local: bool,
        screen_lines: &ScreenLines,
    ) {
        let ed = &self.editor;
        let cursor = self.editor.cursor;
        // let find_focus = self.editor.find_focus;
        // let hide_cursor = self.editor.common.window_common.hide_cursor;

        let viewport = self.editor.viewport.get_untracked();
        // let is_active =
        // self.is_active.get_untracked() && !find_focus.get_untracked();

        let current_line_color = ed.color(EditorColor::CurrentLine);
        let selection_color = ed.color(EditorColor::Selection);
        let caret_color = ed.color(EditorColor::Caret);

        // TODO:
        // let breakline = self.debug_breakline.get_untracked().and_then(
        //     |(breakline, breakline_path)| {
        //         if view
        //             .doc
        //             .get_untracked()
        //             .content
        //             .with_untracked(|c| c.path() == Some(&breakline_path))
        //         {
        //             Some(breakline)
        //         } else {
        //             None
        //         }
        //     },
        // );

        // // TODO: check if this is correct
        // if let Some(breakline) = breakline {
        //     if let Some(info) = screen_lines.info_for_line(breakline) {
        //         let rect = Rect::from_origin_size(
        //             (viewport.x0, info.vline_y),
        //             (viewport.width(), line_height),
        //         );

        //         cx.fill(
        //             &rect,
        //             config.get_color(LapceColor::EDITOR_DEBUG_BREAK_LINE),
        //             0.0,
        //         );
        //     }
        // }

        cursor.with_untracked(|cursor| {
            let highlight_current_line = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Insert(_) => true,
                CursorMode::Visual { .. } => false,
            };

            // Highlight the current line
            if !is_local && highlight_current_line {
                for (_, end) in cursor.regions_iter() {
                    // TODO: unsure if this is correct for wrapping lines
                    let rvline = ed.rvline_of_offset(end, cursor.affinity);

                    if let Some(info) = screen_lines.info(rvline)
                    // TODO:
                    // .filter(|_| Some(rvline.line) != breakline)
                    {
                        let line_height =
                            ed.style().line_height(info.vline_info.rvline.line);
                        let rect = Rect::from_origin_size(
                            (viewport.x0, info.vline_y),
                            (viewport.width(), f64::from(line_height)),
                        );

                        cx.fill(&rect, current_line_color, 0.0);
                    }
                }
            }

            match cursor.mode {
                CursorMode::Normal(_) => {}
                CursorMode::Visual {
                    start,
                    end,
                    mode: VisualMode::Normal,
                } => {
                    let start_offset = start.min(end);
                    let end_offset = ed.move_right(start.max(end), Mode::Insert, 1);

                    self.paint_normal_selection(
                        cx,
                        selection_color,
                        screen_lines,
                        start_offset,
                        end_offset,
                        cursor.affinity,
                        true,
                    );
                }
                CursorMode::Visual {
                    start,
                    end,
                    mode: VisualMode::Linewise,
                } => {
                    self.paint_linewise_selection(
                        cx,
                        selection_color,
                        screen_lines,
                        start.min(end),
                        start.max(end),
                        cursor.affinity,
                    );
                }
                CursorMode::Visual {
                    start,
                    end,
                    mode: VisualMode::Blockwise,
                } => {
                    self.paint_blockwise_selection(
                        cx,
                        selection_color,
                        screen_lines,
                        start.min(end),
                        start.max(end),
                        cursor.affinity,
                        cursor.horiz,
                    );
                }
                CursorMode::Insert(_) => {
                    for (start, end) in
                        cursor.regions_iter().filter(|(start, end)| start != end)
                    {
                        self.paint_normal_selection(
                            cx,
                            selection_color,
                            screen_lines,
                            start.min(end),
                            start.max(end),
                            cursor.affinity,
                            false,
                        );
                    }
                }
            }

            // if is_active && !hide_cursor.get_untracked() {
            //     for (_, end) in cursor.regions_iter() {
            //         let is_block = match cursor.mode {
            //             CursorMode::Normal(_) | CursorMode::Visual { .. } => true,
            //             CursorMode::Insert(_) => false,
            //         };
            //         let LineRegion { x, width, rvline } =
            //             cursor_caret(ed, end, is_block, cursor.affinity);

            //         if let Some(info) = screen_lines.info(rvline) {
            //             let line_height =
            //                 ed.style().line_height(info.vline_info.rvline.line);
            //             let rect = Rect::from_origin_size(
            //                 (x, info.vline_y),
            //                 (width, f64::from(line_height)),
            //             );
            //             cx.fill(&rect, caret_color, 0.0);
            //         }
            //     }
            // }
        });
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
        viewport: Rect,
    ) {
        for style in extra_styles {
            let height = style.height;
            if let Some(bg) = style.bg_color {
                let width = style.width.unwrap_or_else(|| viewport.width());
                let base = if style.width.is_none() {
                    viewport.x0
                } else {
                    0.0
                };
                let x = style.x + base;
                let y = y + style.y;
                cx.fill(
                    &Rect::ZERO
                        .with_size(Size::new(width, height))
                        .with_origin(Point::new(x, y)),
                    bg,
                    0.0,
                );
            }

            if let Some(color) = style.under_line {
                let width = style.width.unwrap_or_else(|| viewport.width());
                let base = if style.width.is_none() {
                    viewport.x0
                } else {
                    0.0
                };
                let x = style.x + base;
                let y = y + style.y + height;
                cx.stroke(
                    &Line::new(Point::new(x, y), Point::new(x + width, y)),
                    color,
                    1.0,
                );
            }

            if let Some(color) = style.wave_line {
                let width = style.width.unwrap_or_else(|| viewport.width());
                let y = y + style.y + height;
                self.paint_wave_line(cx, width, Point::new(style.x, y), color);
            }
        }
    }

    fn paint_text(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        // TODO: indent text layout

        for (line, y) in screen_lines.iter_lines_y() {
            let text_layout = self.editor.text_layout(line);

            // TODO: paint extra style

            // TODO: whitespaces

            // TODO: indent guide

            cx.draw_text(&text_layout.text, Point::new(0.0, y));
        }
    }
}
impl View for EditorView {
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
        // if let Ok(state) = state.downcast() {
        //     self.sticky_header_info = *state;
        //     cx.request_layout(self.id);
        // }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            // if self.inner_node.is_none() {
            //     self.inner_node = Some(cx.new_node());
            // }
            // let inner_node = self.inner_node.unwrap();

            // let config = self.editor.common.config.get_untracked();
            // let line_height = config.editor.line_height() as f64;

            // let width = self.editor.view.max_line_width() + 20.0;
            // let height = line_height * self.editor.view.last_vline().get() as f64;

            // let style = Style::new().width(width).height(height).to_taffy_style();
            // cx.set_style(inner_node, style);

            // vec![inner_node]
            vec![]
        })
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        // let viewport = cx.current_viewport();
        // if self.viewport.with_untracked(|v| v != &viewport) {
        //     self.viewport.set(viewport);
        // }
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        // let viewport = self.viewport.get_untracked();
        // let config = self.editor.common.config.get_untracked();
        // let doc = self.editor.view.doc.get_untracked();
        // let is_local = doc.content.with_untracked(|content| content.is_local());

        // // We repeatedly get the screen lines because we don't currently carefully manage the
        // // paint functions to avoid potentially needing to recompute them, which could *maybe*
        // // make them invalid.
        // // TODO: One way to get around the above issue would be to more careful, since we
        // // technically don't need to stop it from *recomputing* just stop any possible changes, but
        // // avoiding recomputation seems easiest/clearest.
        // // I expect that most/all of the paint functions could restrict themselves to only what is
        // // within the active screen lines without issue.
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_cursor(cx, is_local, &screen_lines);
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_diff_sections(cx, viewport, &screen_lines, &config);
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_find(cx, &screen_lines);
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_bracket_highlights_scope_lines(cx, viewport, &screen_lines);
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_text(cx, viewport, &screen_lines);
        // let screen_lines = self.editor.screen_lines().get_untracked();
        // self.paint_sticky_headers(cx, viewport, &screen_lines);
        // self.paint_scroll_bar(cx, viewport, is_local, config);
    }
}

pub fn editor_view(editor: Rc<Editor>) -> EditorView {
    let id = Id::next();
    let data = ViewData::new(id);
    EditorView { id, data, editor }
}

#[derive(Clone, Debug)]
pub struct LineRegion {
    pub x: f64,
    pub width: f64,
    pub rvline: RVLine,
}

/// Get the render information for a caret cursor at the given `offset`.  
pub fn cursor_caret(
    ed: &Editor,
    offset: usize,
    block: bool,
    affinity: CursorAffinity,
) -> LineRegion {
    let info = ed.rvline_info_of_offset(offset, affinity);
    let (_, col) = ed.offset_to_line_col(offset);
    let after_last_char = col == ed.line_end_col(info.rvline.line, true);

    let doc = ed.doc();
    let preedit_start = doc
        .preedit()
        .preedit
        .with_untracked(|preedit| {
            preedit.as_ref().and_then(|preedit| {
                let preedit_line = ed.line_of_offset(preedit.offset);
                preedit.cursor.map(|x| (preedit_line, x))
            })
        })
        .filter(|(preedit_line, _)| *preedit_line == info.rvline.line)
        .map(|(_, (start, _))| start);

    let phantom_text = ed.phantom_text(info.rvline.line);

    let (_, col) = ed.offset_to_line_col(offset);
    let ime_kind = preedit_start.map(|_| PhantomTextKind::Ime);
    // The cursor should be after phantom text if the affinity is forward, or it is a block cursor.
    // - if we have a relevant preedit we skip over IMEs
    // - we skip over completion lens, as the text should be after the cursor
    let col = phantom_text.col_after_ignore(
        col,
        affinity == CursorAffinity::Forward || (block && !after_last_char),
        |p| p.kind == PhantomTextKind::Completion || Some(p.kind) == ime_kind,
    );
    // We shift forward by the IME's start. This is due to the cursor potentially being in the
    // middle of IME phantom text while editing it.
    let col = col + preedit_start.unwrap_or(0);

    let point = ed.line_point_of_line_col(info.rvline.line, col, affinity);

    let rvline = if preedit_start.is_some() {
        // If there's an IME edit, then we need to use the point's y to get the actual y position
        // that the IME cursor is at. Since it could be in the middle of the IME phantom text
        let y = point.y;

        // TODO: I don't think this is handling varying line heights properly
        let line_height = ed.style().line_height(info.rvline.line);

        let line_index = (y / f64::from(line_height)).floor() as usize;
        RVLine::new(info.rvline.line, line_index)
    } else {
        info.rvline
    };

    let x0 = point.x;
    if block {
        let width = if after_last_char {
            CHAR_WIDTH
        } else {
            let x1 = ed
                .line_point_of_line_col(info.rvline.line, col + 1, affinity)
                .x;
            x1 - x0
        };

        LineRegion {
            x: x0,
            width,
            rvline,
        }
    } else {
        LineRegion {
            x: x0 - 1.0,
            width: 2.0,
            rvline,
        }
    }
}

fn editor_gutter() -> impl View {
    let padding_left = 0.0;
    stack((empty(),))
}
