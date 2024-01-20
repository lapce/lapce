use std::{collections::HashMap, ops::RangeInclusive, rc::Rc};

use floem::{
    action::{set_ime_allowed, set_ime_cursor_area},
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, TextLayout},
    event::{Event, EventListener},
    id::Id,
    keyboard::{Key, ModifiersState, NamedKey},
    kurbo::{BezPath, Line, Point, Rect, Size, Vec2},
    peniko::Color,
    reactive::{
        batch, create_effect, create_memo, create_rw_signal, Memo, RwSignal, Scope,
    },
    style::{CursorStyle, Style},
    taffy::node::Node,
    view::{View, ViewData},
    views::{clip, container, empty, label, scroll, stack, Decorators},
    EventPropagation, Renderer,
};
use floem_editor_core::{
    buffer::rope_text::RopeText,
    cursor::{ColPosition, CursorAffinity, CursorMode},
    mode::{Mode, VisualMode},
};

use crate::{
    command::CommandExecuted,
    gutter::editor_gutter_view,
    keypress::{key::KeyInput, press::KeyPress},
    layout::LineExtraStyle,
    phantom_text::PhantomTextKind,
    visual_line::{RVLine, VLineInfo},
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

// TODO(floem-editor): We have diff sections in screen lines because Lapce uses them, but
// we don't really have support for diffs in floem-editor!
// Possibly we should just move that out to a separate field on Lapce's editor.
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

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
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

    /// Ran on [`LayoutEvent::CreatedLayout`] to update  [`ScreenLinesBase`] &
    /// the viewport if necessary.
    ///
    /// Returns `true` if [`ScreenLines`] needs to be completely updated in response
    pub fn on_created_layout(&self, ed: &Editor, line: usize) -> bool {
        // The default creation is empty, force an update if we're ever like this since it should
        // not happen.
        if self.is_empty() {
            return true;
        }

        let base = self.base.get_untracked();
        let vp = ed.viewport.get_untracked();

        let is_before = self
            .iter_vline_info()
            .next()
            .map(|l| line < l.rvline.line)
            .unwrap_or(false);

        // If the line is created before the current screenlines, we can simply shift the
        // base and viewport forward by the number of extra wrapped lines,
        // without needing to recompute the screen lines.
        if is_before {
            // TODO: don't assume line height is constant
            let line_height = f64::from(ed.line_height(0));

            // We could use `try_text_layout` here, but I believe this guards against a rare
            // crash (though it is hard to verify) wherein the style id has changed and so the
            // layouts get cleared.
            // However, the original trigger of the layout event was when a layout was created
            // and it expects it to still exist. So we create it just in case, though we of course
            // don't trigger another layout event.
            let layout = ed.text_layout_trigger(line, false);

            // One line was already accounted for by treating it as an unwrapped line.
            let new_lines = layout.line_count() - 1;

            let new_y0 = base.active_viewport.y0 + new_lines as f64 * line_height;
            let new_y1 = new_y0 + vp.height();
            let new_viewport = Rect::new(vp.x0, new_y0, vp.x1, new_y1);

            batch(|| {
                self.base.set(ScreenLinesBase {
                    active_viewport: new_viewport,
                });
                ed.viewport.set(new_viewport);
            });

            // Ensure that it is created even after the base/viewport signals have been updated.
            // (We need the `text_layout` to still have the layout)
            // But we have to trigger an event still if it is created because it *would* alter the
            // screenlines.
            // TODO: this has some risk for infinite looping if we're unlucky.
            let _layout = ed.text_layout_trigger(line, true);

            return false;
        }

        let is_after = self
            .iter_vline_info()
            .last()
            .map(|l| line > l.rvline.line)
            .unwrap_or(false);

        // If the line created was after the current view, we don't need to update the screenlines
        // at all, since the new line is not visible and has no effect on y positions
        if is_after {
            return false;
        }

        // If the line is created within the current screenlines, we need to update the
        // screenlines to account for the new line.
        // That is handled by the caller.
        true
    }
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
    is_active: Memo<bool>,
    inner_node: Option<Node>,
}
impl EditorView {
    #[allow(clippy::too_many_arguments)]
    fn paint_normal_selection(
        cx: &mut PaintCx,
        ed: &Editor,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        is_block_cursor: bool,
    ) {
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

            let line_height = ed.line_height(line);
            let rect = Rect::from_origin_size(
                (x0, vline_y),
                (width, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_linewise_selection(
        cx: &mut PaintCx,
        ed: &Editor,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
    ) {
        let viewport = ed.viewport.get_untracked();

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

            let line_height = ed.line_height(line);
            let rect = Rect::from_origin_size(
                (viewport.x0, vline_y),
                (x1 - viewport.x0, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_blockwise_selection(
        cx: &mut PaintCx,
        ed: &Editor,
        color: Color,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        horiz: Option<ColPosition>,
    ) {
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

            let line_height = ed.line_height(line);
            let rect = Rect::from_origin_size(
                (x0, line_info.vline_y),
                (x1 - x0, f64::from(line_height)),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    fn paint_cursor(
        cx: &mut PaintCx,
        ed: &Editor,
        is_local: bool,
        is_active: bool,
        screen_lines: &ScreenLines,
    ) {
        let cursor = ed.cursor;

        let viewport = ed.viewport.get_untracked();

        let current_line_color = ed.color(EditorColor::CurrentLine);

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

                    if let Some(info) = screen_lines.info(rvline) {
                        let line_height =
                            ed.line_height(info.vline_info.rvline.line);
                        let rect = Rect::from_origin_size(
                            (viewport.x0, info.vline_y),
                            (viewport.width(), f64::from(line_height)),
                        );

                        cx.fill(&rect, current_line_color, 0.0);
                    }
                }
            }

            EditorView::paint_selection(cx, ed, screen_lines);

            EditorView::paint_cursor_caret(cx, ed, is_active, screen_lines);
        });
    }

    pub fn paint_selection(
        cx: &mut PaintCx,
        ed: &Editor,
        screen_lines: &ScreenLines,
    ) {
        let cursor = ed.cursor;

        let selection_color = ed.color(EditorColor::Selection);

        cursor.with_untracked(|cursor| match cursor.mode {
            CursorMode::Normal(_) => {}
            CursorMode::Visual {
                start,
                end,
                mode: VisualMode::Normal,
            } => {
                let start_offset = start.min(end);
                let end_offset = ed.move_right(start.max(end), Mode::Insert, 1);

                EditorView::paint_normal_selection(
                    cx,
                    ed,
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
                EditorView::paint_linewise_selection(
                    cx,
                    ed,
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
                EditorView::paint_blockwise_selection(
                    cx,
                    ed,
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
                    EditorView::paint_normal_selection(
                        cx,
                        ed,
                        selection_color,
                        screen_lines,
                        start.min(end),
                        start.max(end),
                        cursor.affinity,
                        false,
                    );
                }
            }
        });
    }

    pub fn paint_cursor_caret(
        cx: &mut PaintCx,
        ed: &Editor,
        is_active: bool,
        screen_lines: &ScreenLines,
    ) {
        let cursor = ed.cursor;
        let hide_cursor = ed.cursor_info.hidden;
        let caret_color = ed.color(EditorColor::Caret);

        if !(is_active && !hide_cursor.get_untracked()) {
            return;
        }

        cursor.with_untracked(|cursor| {
            let style = ed.style();
            for (_, end) in cursor.regions_iter() {
                let is_block = match cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => true,
                    CursorMode::Insert(_) => false,
                };
                let LineRegion { x, width, rvline } =
                    cursor_caret(ed, end, is_block, cursor.affinity);

                if let Some(info) = screen_lines.info(rvline) {
                    if !style.paint_caret(ed, rvline.line) {
                        continue;
                    }

                    let line_height = ed.line_height(info.vline_info.rvline.line);
                    let rect = Rect::from_origin_size(
                        (x, info.vline_y),
                        (width, f64::from(line_height)),
                    );
                    cx.fill(&rect, caret_color, 0.0);
                }
            }
        });
    }

    pub fn paint_wave_line(
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

    pub fn paint_extra_style(
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
                EditorView::paint_wave_line(
                    cx,
                    width,
                    Point::new(style.x, y),
                    color,
                );
            }
        }
    }

    pub fn paint_text(
        cx: &mut PaintCx,
        ed: &Editor,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let style = ed.style();

        // TODO: cache indent text layout width
        let indent_unit = style.indent_style().as_str();
        // TODO: don't assume font family is the same for all lines?
        let family = style.font_family(0);
        let attrs = Attrs::new()
            .family(&family)
            .font_size(style.font_size(0) as f32);
        let attrs_list = AttrsList::new(attrs);

        let mut indent_text = TextLayout::new();
        indent_text.set_text(&format!("{indent_unit}a"), attrs_list);
        let indent_text_width = indent_text.hit_position(indent_unit.len()).point.x;

        for (line, y) in screen_lines.iter_lines_y() {
            let text_layout = ed.text_layout(line);

            EditorView::paint_extra_style(cx, &text_layout.extra_style, y, viewport);

            if let Some(whitespaces) = &text_layout.whitespaces {
                let family = style.font_family(line);
                let font_size = style.font_size(line) as f32;
                let attrs = Attrs::new()
                    .color(style.color(EditorColor::VisibleWhitespace))
                    .family(&family)
                    .font_size(font_size);
                let attrs_list = AttrsList::new(attrs);
                let mut space_text = TextLayout::new();
                space_text.set_text("·", attrs_list.clone());
                let mut tab_text = TextLayout::new();
                tab_text.set_text("→", attrs_list);

                for (c, (x0, _x1)) in whitespaces.iter() {
                    match *c {
                        '\t' => {
                            cx.draw_text(&tab_text, Point::new(*x0, y));
                        }
                        ' ' => {
                            cx.draw_text(&space_text, Point::new(*x0, y));
                        }
                        _ => {}
                    }
                }
            }

            if ed.show_indent_guide.get_untracked() {
                let line_height = f64::from(ed.line_height(line));
                let mut x = 0.0;
                while x + 1.0 < text_layout.indent {
                    cx.stroke(
                        &Line::new(Point::new(x, y), Point::new(x, y + line_height)),
                        style.color(EditorColor::IndentGuide),
                        1.0,
                    );
                    x += indent_text_width;
                }
            }

            cx.draw_text(&text_layout.text, Point::new(0.0, y));
        }
    }

    pub fn paint_scroll_bar(
        cx: &mut PaintCx,
        ed: &Editor,
        viewport: Rect,
        is_local: bool,
    ) {
        if is_local {
            return;
        }

        // TODO: let this be customized
        const BAR_WIDTH: f64 = 10.0;
        cx.fill(
            &Rect::ZERO
                .with_size(Size::new(1.0, viewport.height()))
                .with_origin(Point::new(
                    viewport.x0 + viewport.width() - BAR_WIDTH,
                    viewport.y0,
                ))
                .inflate(0.0, 10.0),
            ed.color(EditorColor::Scrollbar),
            0.0,
        );
    }

    /// Calculate the `x` coordinate of the left edge of the given column on the given line.
    /// If `before_cursor` is `true`, the calculated position will be to the right of any inlay
    /// hints before and adjacent to the given column. Else, the calculated position will be to the
    /// left of any such inlay hints.
    fn calculate_col_x(
        ed: &Editor,
        line: usize,
        col: usize,
        affinity: CursorAffinity,
    ) -> f64 {
        let before_cursor = affinity == CursorAffinity::Backward;
        let phantom_text = ed.phantom_text(line);
        let col = phantom_text.col_after(col, before_cursor);
        ed.line_point_of_line_col(line, col, affinity).x
    }

    /// Paint a highlight around the characters at the given positions.
    fn paint_char_highlights(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
        highlight_line_cols: impl Iterator<Item = (RVLine, usize)>,
    ) {
        let ed = &self.editor;

        for (rvline, col) in highlight_line_cols {
            // Is the given line on screen?
            if let Some(line_info) = screen_lines.info(rvline) {
                let x0 = Self::calculate_col_x(
                    ed,
                    rvline.line,
                    col,
                    CursorAffinity::Backward,
                );
                let x1 = Self::calculate_col_x(
                    ed,
                    rvline.line,
                    col + 1,
                    CursorAffinity::Forward,
                );

                let line_height = f64::from(ed.line_height(rvline.line));

                let y0 = line_info.vline_y;
                let y1 = y0 + line_height;

                let rect = Rect::new(x0, y0, x1, y1);

                cx.stroke(&rect, ed.color(EditorColor::Foreground), 1.0);
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
        let ed = &self.editor;
        let brush = ed.color(EditorColor::Foreground);

        if start == end {
            if let Some(line_info) = screen_lines.info(start) {
                // TODO: Due to line wrapping the y positions of these two spots could be different, do we need to change it?
                let x0 = Self::calculate_col_x(
                    ed,
                    start.line,
                    start_col + 1,
                    CursorAffinity::Forward,
                );
                let x1 = Self::calculate_col_x(
                    ed,
                    end.line,
                    end_col,
                    CursorAffinity::Backward,
                );

                if x0 < x1 {
                    let line_height =
                        f64::from(ed.line_height(line_info.vline_info.rvline.line));
                    let y = line_info.vline_y + line_height;

                    let p0 = Point::new(x0, y);
                    let p1 = Point::new(x1, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, 1.0);
                }
            }
        } else {
            // Are start_line and end_line on screen?
            let start_line_y = screen_lines.info(start).map(|line_info| {
                let line_height =
                    f64::from(ed.line_height(line_info.vline_info.rvline.line));
                line_info.vline_y + line_height
            });
            let end_line_y = screen_lines.info(end).map(|line_info| {
                let line_height =
                    f64::from(ed.line_height(line_info.vline_info.rvline.line));
                line_info.vline_y + line_height
            });

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
                let start_x = Self::calculate_col_x(
                    ed,
                    start.line,
                    start_col + 1,
                    CursorAffinity::Forward,
                );
                let end_x = Self::calculate_col_x(
                    ed,
                    end.line,
                    end_col,
                    CursorAffinity::Backward,
                );

                // TODO(minor): is this correct with line wrapping?
                // The vertical line should be drawn to the left of any non-whitespace characters
                // in the enclosed section.
                let rope_text = ed.rope_text();
                let min_text_x = {
                    ((start.line + 1)..=end.line)
                        .filter(|&line| !rope_text.is_line_whitespace(line))
                        .map(|line| {
                            let non_blank_offset =
                                rope_text.first_non_blank_character_on_line(line);
                            let (_, col) = ed.offset_to_line_col(non_blank_offset);

                            Self::calculate_col_x(
                                ed,
                                line,
                                col,
                                CursorAffinity::Backward,
                            )
                        })
                        .min_by(f64::total_cmp)
                };

                let min_x = min_text_x.map_or(start_x, |min_text_x| {
                    std::cmp::min_by(min_text_x, start_x, f64::total_cmp)
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
        _cx: &mut floem::context::UpdateCx,
        _state: Box<dyn std::any::Any>,
    ) {
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            if self.inner_node.is_none() {
                self.inner_node = Some(cx.new_node());
            }

            let screen_lines = self.editor.screen_lines.get_untracked();
            for (line, _) in screen_lines.iter_lines_y() {
                // fill in text layout cache so that max width is correct.
                self.editor.text_layout(line);
            }

            let inner_node = self.inner_node.unwrap();

            // TODO: don't assume there's a constant line height
            let line_height = f64::from(self.editor.line_height(0));

            let width = self.editor.max_line_width() + 20.0;
            let height = line_height * self.editor.last_vline().get() as f64;

            let style = Style::new().width(width).height(height).to_taffy_style();
            cx.set_style(inner_node, style);

            vec![inner_node]
        })
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        let viewport = cx.current_viewport();
        if self.editor.viewport.with_untracked(|v| v != &viewport) {
            self.editor.viewport.set(viewport);
        }
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let ed = &self.editor;
        let viewport = ed.viewport.get_untracked();
        let is_local = false; // TODO(floem-editor)

        // We repeatedly get the screen lines because we don't currently carefully manage the
        // paint functions to avoid potentially needing to recompute them, which could *maybe*
        // make them invalid.
        // TODO: One way to get around the above issue would be to more careful, since we
        // technically don't need to stop it from *recomputing* just stop any possible changes, but
        // avoiding recomputation seems easiest/clearest.
        // I expect that most/all of the paint functions could restrict themselves to only what is
        // within the active screen lines without issue.
        let screen_lines = ed.screen_lines.get_untracked();
        EditorView::paint_cursor(
            cx,
            ed,
            is_local,
            self.is_active.get_untracked(),
            &screen_lines,
        );
        let screen_lines = ed.screen_lines.get_untracked();
        EditorView::paint_text(cx, ed, viewport, &screen_lines);
        EditorView::paint_scroll_bar(cx, ed, viewport, is_local);
    }
}

pub fn editor_view(
    editor: Rc<Editor>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> EditorView {
    let id = Id::next();
    let is_active = create_memo(move |_| is_active(true));

    let data = ViewData::new(id);

    // TODO: is_active

    let doc = editor.doc;
    let style = editor.style;
    create_effect(move |_| {
        doc.track();
        style.track();
        id.request_layout();
    });

    let hide_cursor = editor.cursor_info.hidden;
    create_effect(move |_| {
        hide_cursor.track();
        id.request_paint();
    });

    let editor_window_origin = editor.window_origin;
    let cursor = editor.cursor;
    let ime_allowed = editor.ime_allowed;
    let editor_viewport = editor.viewport;
    let ed = editor.clone();
    create_effect(move |_| {
        let active = is_active.get();
        if active {
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
                let (_, point_below) = ed.points_of_offset(offset, affinity);
                let window_origin = editor_window_origin.get();
                let viewport = editor_viewport.get();
                let pos = window_origin
                    + (point_below.x - viewport.x0, point_below.y - viewport.y0);
                set_ime_cursor_area(pos, Size::new(800.0, 600.0));
            }
        }
    });

    let ed = editor.clone();
    let ed2 = editor.clone();
    EditorView {
        id,
        data,
        editor,
        is_active,
        inner_node: None,
    }
    .on_event(EventListener::ImePreedit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImePreedit { text, cursor } = event {
            if text.is_empty() {
                ed.clear_preedit();
            } else {
                let offset = ed.cursor.with_untracked(|c| c.offset());
                ed.set_preedit(text.clone(), *cursor, offset);
            }
        }
        EventPropagation::Stop
    })
    .on_event(EventListener::ImeCommit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImeCommit(text) = event {
            ed2.clear_preedit();
            ed2.receive_char(text);
        }
        EventPropagation::Stop
    })
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
        let line_height = ed.line_height(info.rvline.line);

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

pub fn editor_container_view(
    editor: RwSignal<Rc<Editor>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    handle_key_event: impl Fn(&KeyPress, ModifiersState) -> CommandExecuted + 'static,
) -> impl View {
    let editor_rect = create_rw_signal(Rect::ZERO);

    stack((
        // editor_breadcrumbs(workspace, editor.get_untracked(), config),
        container(
            stack((
                editor_gutter(editor, is_active),
                container(editor_content(editor, is_active, handle_key_event))
                    .style(move |s| s.size_pct(100.0, 100.0)),
                empty().style(move |s| {
                    s.absolute().width_pct(100.0)
                    // TODO:
                    // .height(sticky_header_height.get() as f32)
                    // .apply_if(
                    //     !config.editor.sticky_header
                    //         || sticky_header_height.get() == 0.0
                    //         || !editor_view.get().is_normal(),
                    //     |s| s.hide(),
                    // )
                }),
                // find_view(
                //     editor,
                //     find_editor,
                //     find_focus,
                //     replace_editor,
                //     replace_active,
                //     replace_focus,
                //     is_active,
                // ),
            ))
            .on_resize(move |rect| {
                editor_rect.set(rect);
            })
            .style(|s| s.absolute().size_pct(100.0, 100.0)),
        )
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .on_cleanup(move || {
        // TODO(floem-editor): We should do cleanup of the scope at least, but we also need it
        // conditional such that it doesn't always run.
        // if editors.with_untracked(|editors| editors.contains_key(&editor_id)) {
        //     // editor still exist, so it might be moved to a different editor tab
        //     return;
        // }
        // let editor = editor.get_untracked();
        // let doc = editor.view.doc.get_untracked();
        // editor.scope.dispose();

        // let scratch_doc_name =
        //     if let DocContent::Scratch { name, .. } = doc.content.get_untracked() {
        //         Some(name.to_string())
        //     } else {
        //         None
        //     };
        // if let Some(name) = scratch_doc_name {
        //     if !scratch_docs
        //         .with_untracked(|scratch_docs| scratch_docs.contains_key(&name))
        //     {
        //         doc.scope.dispose();
        //     }
        // }
    })
    // TODO(minor): only depend on style
    .style(move |s| {
        s.flex_col()
            .size_pct(100.0, 100.0)
            .background(editor.get().color(EditorColor::Background))
    })
}

/// Default editor gutter
/// Simply shows line numbers
pub fn editor_gutter(
    editor: RwSignal<Rc<Editor>>,
    _is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    // TODO(floem-editor): these are probably tuned for lapce?
    let padding_left = 25.0;
    let padding_right = 30.0;

    let ed = editor.get_untracked();

    let scroll_delta = ed.scroll_delta;

    let gutter_rect = create_rw_signal(Rect::ZERO);

    stack((
        stack((
            empty().style(move |s| s.width(padding_left)),
            // TODO(minor): this could just track purely Doc
            label(move || (editor.get().last_line() + 1).to_string()),
            empty().style(move |s| s.width(padding_right)),
        ))
        .style(|s| s.height_pct(100.0)),
        clip(
            stack((editor_gutter_view(editor.get_untracked())
                .on_resize(move |rect| {
                    gutter_rect.set(rect);
                })
                .on_event_stop(EventListener::PointerWheel, move |event| {
                    if let Event::PointerWheel(pointer_event) = event {
                        scroll_delta.set(pointer_event.delta);
                    }
                })
                .style(|s| s.size_pct(100.0, 100.0)),))
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

fn editor_content(
    editor: RwSignal<Rc<Editor>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    handle_key_event: impl Fn(&KeyPress, ModifiersState) -> CommandExecuted + 'static,
) -> impl View {
    let ed = editor.get_untracked();
    let cursor = ed.cursor;
    let scroll_delta = ed.scroll_delta;
    let scroll_to = ed.scroll_to;
    let window_origin = ed.window_origin;
    let viewport = ed.viewport;
    let scroll_beyond_last_line = ed.scroll_beyond_last_line;

    scroll({
        let editor_content_view = editor_view(ed, is_active).style(move |s| {
            let padding_bottom = if scroll_beyond_last_line.get() {
                // TODO: don't assume line height is constant?
                // just use the last line's line height maybe, or just make
                // scroll beyond last line a f32
                // TODO: we shouldn't be using `get` on editor here, isn't this more of a 'has the
                // style cache changed'?
                let line_height = editor.get().line_height(0) as f32;
                viewport.get().height() as f32 - line_height
            } else {
                0.0
            };

            s.absolute()
                .padding_bottom(padding_bottom)
                .cursor(CursorStyle::Text)
                .min_size_pct(100.0, 100.0)
        });

        let id = editor_content_view.id();

        editor_content_view
            .on_event_cont(EventListener::PointerDown, move |event| {
                // TODO:
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    id.request_focus();
                    editor.get_untracked().pointer_down(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    editor.get_untracked().pointer_move(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerUp, move |event| {
                if let Event::PointerUp(pointer_event) = event {
                    editor.get_untracked().pointer_up(pointer_event);
                }
            })
            .on_event_stop(EventListener::KeyDown, move |event| {
                let Event::KeyDown(key_event) = event else {
                    return;
                };

                let Ok(keypress) = KeyPress::try_from(key_event) else {
                    return;
                };

                handle_key_event(&keypress, key_event.modifiers);

                let mut mods = key_event.modifiers;
                mods.set(ModifiersState::SHIFT, false);
                #[cfg(target_os = "macos")]
                mods.set(ModifiersState::ALT, false);

                if mods.is_empty() {
                    if let KeyInput::Keyboard(Key::Character(c), _) = keypress.key {
                        editor.get_untracked().receive_char(&c);
                    } else if let KeyInput::Keyboard(
                        Key::Named(NamedKey::Space),
                        _,
                    ) = keypress.key
                    {
                        editor.get_untracked().receive_char(" ");
                    }
                }
            })
    })
    .on_move(move |point| {
        window_origin.set(point);
    })
    .on_scroll_to(move || scroll_to.get().map(Vec2::to_point))
    .on_scroll_delta(move || scroll_delta.get())
    .on_ensure_visible(move || {
        let editor = editor.get_untracked();
        let cursor = cursor.get();
        let offset = cursor.offset();
        editor.doc.track();
        // TODO:?
        // editor.kind.track();

        let LineRegion { x, width, rvline } =
            cursor_caret(&editor, offset, !cursor.is_insert(), cursor.affinity);

        // TODO: don't assume line-height is constant
        let line_height = f64::from(editor.line_height(0));

        // TODO: is there a good way to avoid the calculation of the vline here?
        let vline = editor.vline_of_rvline(rvline);
        let rect = Rect::from_origin_size(
            (x, vline.get() as f64 * line_height),
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
            let mut rect = rect;
            let cursor_surrounding_lines =
                editor.cursor_surrounding_lines.get_untracked() as f64;
            rect.y0 -= cursor_surrounding_lines * line_height;
            // TODO:
            // + sticky_header_height.get_untracked();
            rect.y1 += cursor_surrounding_lines * line_height;
            rect
        }
    })
    .style(|s| s.absolute().size_pct(100.0, 100.0))
}
