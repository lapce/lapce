use std::{cell::RefCell, rc::Rc, sync::Arc};

use floem::{
    cosmic_text::{Attrs, AttrsList, LineHeightValue, TextLayout, Wrap},
    kurbo::{Point, Rect},
    peniko::Color,
    reactive::{ReadSignal, RwSignal, Scope},
};
use lapce_core::{
    buffer::rope_text::{RopeText, RopeTextVal},
    cursor::{ColPosition, Cursor, CursorAffinity, CursorMode},
    mode::Mode,
    selection::Selection,
    soft_tab::{snap_to_soft_tab_line_col, SnapDirection},
};
use lapce_xi_rope::Rope;

use crate::{
    doc::phantom_text::PhantomTextLine,
    editor::{
        view_data::TextLayoutLine,
        visual_line::{
            hit_position_aff, FontSizeCacheId, LineFontSizeProvider, Lines, RVLine,
            ResolvedWrap, TextLayoutProvider, VLine, VLineInfo,
        },
    },
};

use super::{
    color::EditorColor,
    text::{Document, Styling, WrapMethod},
    view::ScreenLines,
};

pub(crate) const CHAR_WIDTH: f64 = 7.5;

/// The data for a specific editor view
#[derive(Clone)]
pub struct Editor {
    /// Whether you can edit within this editor.
    pub read_only: RwSignal<bool>,
    /// Whether you can scroll beyond the last line of the document.
    pub scroll_beyond_last_line: RwSignal<bool>,

    doc: RwSignal<Rc<dyn Document>>,
    style: RwSignal<Rc<dyn Styling>>,

    pub cursor: RwSignal<Cursor>,

    pub viewport: RwSignal<Rect>,
    /// Holds the cache of the lines and provides many utility functions for them.
    lines: Rc<Lines>,
    pub screen_lines: RwSignal<ScreenLines>,
}
impl Editor {
    // TODO: shouldn't this accept an `RwSignal<Rc<dyn Document>>` so that it can listen for
    // changes in other editors?
    pub fn new(cx: Scope, doc: Rc<dyn Document>, style: Rc<dyn Styling>) -> Editor {
        let cx = cx.create_child();

        let viewport = cx.create_rw_signal(Rect::ZERO);
        let modal = false; // TODO
        let cursor_mode = if modal
        /* && !is_local */
        {
            CursorMode::Normal(0)
        } else {
            CursorMode::Insert(Selection::caret(0))
        };
        let cursor = Cursor::new(cursor_mode, None, None);
        let cursor = cx.create_rw_signal(cursor);

        let doc = cx.create_rw_signal(doc);
        let style = cx.create_rw_signal(style);

        let font_sizes = RefCell::new(Arc::new(EditorFontSizes {
            style: style.read_only(),
        }));
        let lines = Rc::new(Lines::new(cx, font_sizes));
        let screen_lines =
            cx.create_rw_signal(ScreenLines::new(cx, viewport.get_untracked()));

        // TODO: reset blink cursor effect

        Editor {
            read_only: cx.create_rw_signal(false),
            scroll_beyond_last_line: cx.create_rw_signal(false),
            doc,
            style,
            cursor,
            viewport,
            lines,
            screen_lines,
        }
    }

    pub fn doc_signal(&self) -> RwSignal<Rc<dyn Document>> {
        self.doc
    }

    /// Get the document untracked
    pub fn doc(&self) -> Rc<dyn Document> {
        self.doc.get_untracked()
    }

    pub fn style_signal(&self) -> RwSignal<Rc<dyn Styling>> {
        self.style
    }

    /// Get the styling untracked
    pub fn style(&self) -> Rc<dyn Styling> {
        self.style.get_untracked()
    }

    /// Get the text of the document  
    /// You should typically prefer [`Self::rope_text`]
    pub fn text(&self) -> Rope {
        self.doc().text()
    }

    /// Get the [`RopeTextVal`] from `doc` untracked
    pub fn rope_text(&self) -> RopeTextVal {
        self.doc().rope_text()
    }

    // Get the text layout for a document line, creating it if needed.
    pub(crate) fn text_layout(&self, line: usize) -> Arc<TextLayoutLine> {
        self.text_layout_trigger(line, true)
    }

    fn text_prov(&self) -> EditorTextProv {
        let doc = self.doc.get_untracked();
        EditorTextProv {
            text: doc.text(),
            doc,
            style: self.style.get_untracked(),
            viewport: self.viewport.get_untracked(),
        }
    }

    pub(crate) fn text_layout_trigger(
        &self,
        line: usize,
        trigger: bool,
    ) -> Arc<TextLayoutLine> {
        // TODO: config id
        let config_id = 0;
        let text_prov = self.text_prov();
        self.lines
            .get_init_text_layout(config_id, &text_prov, line, trigger)
    }

    pub fn phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc().phantom_text(line)
    }

    pub fn line_height(&self, line: usize) -> f32 {
        self.style().line_height(line)
    }

    pub fn color(&self, color: EditorColor) -> Color {
        self.style().color(color)
    }

    /// Iterate over the visual lines in the view, starting at the given line.
    pub fn iter_vlines(
        &self,
        backwards: bool,
        start: VLine,
    ) -> impl Iterator<Item = VLineInfo> {
        self.lines.iter_vlines(self.text_prov(), backwards, start)
    }

    /// Iterate over the visual lines in the view, starting at the given line and ending at the
    /// given line. `start_line..end_line`
    pub fn iter_vlines_over(
        &self,
        backwards: bool,
        start: VLine,
        end: VLine,
    ) -> impl Iterator<Item = VLineInfo> {
        self.lines
            .iter_vlines_over(self.text_prov(), backwards, start, end)
    }

    /// Iterator over *relative* [`VLineInfo`]s, starting at the buffer line, `start_line`.  
    /// The `visual_line`s provided by this will start at 0 from your `start_line`.  
    /// This is preferable over `iter_lines` if you do not need to absolute visual line value.
    pub fn iter_rvlines(
        &self,
        backwards: bool,
        start: RVLine,
    ) -> impl Iterator<Item = VLineInfo<()>> {
        self.lines.iter_rvlines(self.text_prov(), backwards, start)
    }

    /// Iterator over *relative* [`VLineInfo`]s, starting at the buffer line, `start_line` and
    /// ending at `end_line`.  
    /// `start_line..end_line`  
    /// This is preferable over `iter_lines` if you do not need to absolute visual line value.
    pub fn iter_rvlines_over(
        &self,
        backwards: bool,
        start: RVLine,
        end_line: usize,
    ) -> impl Iterator<Item = VLineInfo<()>> {
        self.lines
            .iter_rvlines_over(self.text_prov(), backwards, start, end_line)
    }

    // ==== Position Information ====

    pub fn first_rvline_info(&self) -> VLineInfo<()> {
        self.rvline_info(RVLine::default())
    }

    /// The number of lines in the document.
    pub fn num_lines(&self) -> usize {
        self.rope_text().num_lines()
    }

    /// The last allowed buffer line in the document.
    pub fn last_line(&self) -> usize {
        self.rope_text().last_line()
    }

    pub fn last_vline(&self) -> VLine {
        self.lines.last_vline(self.text_prov())
    }

    pub fn last_rvline(&self) -> RVLine {
        self.lines.last_rvline(self.text_prov())
    }

    pub fn last_rvline_info(&self) -> VLineInfo<()> {
        self.rvline_info(self.last_rvline())
    }

    // ==== Line/Column Positioning ====

    /// Convert an offset into the buffer into a line and idx.  
    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        self.rope_text().offset_to_line_col(offset)
    }

    pub fn offset_of_line(&self, offset: usize) -> usize {
        self.rope_text().offset_of_line(offset)
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        self.rope_text().offset_of_line_col(line, col)
    }

    /// Get the buffer line of an offset
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope_text().line_of_offset(offset)
    }

    /// Returns the offset into the buffer of the first non blank character on the given line.
    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        self.rope_text().first_non_blank_character_on_line(line)
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        self.rope_text().line_end_col(line, caret)
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        self.rope_text().select_word(offset)
    }

    /// `affinity` decides whether an offset at a soft line break is considered to be on the
    /// previous line or the next line.  
    /// If `affinity` is `CursorAffinity::Forward` and is at the very end of the wrapped line, then
    /// the offset is considered to be on the next line.
    pub fn vline_of_offset(&self, offset: usize, affinity: CursorAffinity) -> VLine {
        self.lines
            .vline_of_offset(&self.text_prov(), offset, affinity)
    }

    pub fn vline_of_line(&self, line: usize) -> VLine {
        self.lines.vline_of_line(&self.text_prov(), line)
    }

    pub fn vline_of_rvline(&self, rvline: RVLine) -> VLine {
        self.lines.vline_of_rvline(&self.text_prov(), rvline)
    }

    /// Get the nearest offset to the start of the visual line.
    pub fn offset_of_vline(&self, vline: VLine) -> usize {
        self.lines.offset_of_vline(&self.text_prov(), vline)
    }

    /// Get the visual line and column of the given offset.  
    /// The column is before phantom text is applied.
    pub fn vline_col_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> (VLine, usize) {
        self.lines
            .vline_col_of_offset(&self.text_prov(), offset, affinity)
    }

    pub fn rvline_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> RVLine {
        self.lines
            .rvline_of_offset(&self.text_prov(), offset, affinity)
    }

    pub fn rvline_col_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> (RVLine, usize) {
        self.lines
            .rvline_col_of_offset(&self.text_prov(), offset, affinity)
    }

    pub fn offset_of_rvline(&self, rvline: RVLine) -> usize {
        self.lines.offset_of_rvline(&self.text_prov(), rvline)
    }

    pub fn vline_info(&self, vline: VLine) -> VLineInfo {
        let vline = vline.min(self.last_vline());
        self.iter_vlines(false, vline).next().unwrap()
    }

    pub fn screen_rvline_info_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> Option<VLineInfo<()>> {
        let rvline = self.rvline_of_offset(offset, affinity);
        self.screen_lines.with_untracked(|screen_lines| {
            screen_lines
                .iter_vline_info()
                .find(|vline_info| vline_info.rvline == rvline)
        })
    }

    pub fn rvline_info(&self, rvline: RVLine) -> VLineInfo<()> {
        let rvline = rvline.min(self.last_rvline());
        self.iter_rvlines(false, rvline).next().unwrap()
    }

    pub fn rvline_info_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> VLineInfo<()> {
        let rvline = self.rvline_of_offset(offset, affinity);
        self.rvline_info(rvline)
    }

    /// Get the first column of the overall line of the visual line
    pub fn first_col<T: std::fmt::Debug>(&self, info: VLineInfo<T>) -> usize {
        info.first_col(&self.text_prov())
    }

    /// Get the last column in the overall line of the visual line
    pub fn last_col<T: std::fmt::Debug>(
        &self,
        info: VLineInfo<T>,
        caret: bool,
    ) -> usize {
        info.last_col(&self.text_prov(), caret)
    }

    // ==== Points of locations ====

    pub fn max_line_width(&self) -> f64 {
        self.lines.max_width()
    }

    /// Returns the point into the text layout of the line at the given offset.
    /// `x` being the leading edge of the character, and `y` being the baseline.
    pub fn line_point_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> Point {
        let (line, col) = self.offset_to_line_col(offset);
        self.line_point_of_line_col(line, col, affinity)
    }

    /// Returns the point into the text layout of the line at the given line and col.
    /// `x` being the leading edge of the character, and `y` being the baseline.  
    pub fn line_point_of_line_col(
        &self,
        line: usize,
        col: usize,
        affinity: CursorAffinity,
    ) -> Point {
        let text_layout = self.text_layout(line);
        hit_position_aff(
            &text_layout.text,
            col,
            affinity == CursorAffinity::Backward,
        )
        .point
    }

    /// Get the (point above, point below) of a particular offset within the editor.
    pub fn points_of_offset(
        &self,
        offset: usize,
        affinity: CursorAffinity,
    ) -> (Point, Point) {
        let line = self.line_of_offset(offset);
        let line_height = f64::from(self.style().line_height(line));

        let info = self.screen_lines.with_untracked(|sl| {
            sl.iter_line_info()
                .find(|info| info.vline_info.interval.contains(offset))
        });
        let Some(info) = info else {
            // TODO: We could do a smarter method where we get the approximate y position
            // because, for example, this spot could be folded away, and so it would be better to
            // supply the *nearest* position on the screen.
            return (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
        };

        let y = info.vline_y;

        let x = self.line_point_of_offset(offset, affinity).x;

        (Point::new(x, y), Point::new(x, y + line_height))
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
        // TODO: this assumes that line height is constant!
        let line_height = f64::from(self.style().line_height(0));
        let info = if point.y <= 0.0 {
            Some(self.first_rvline_info())
        } else {
            self.screen_lines
                .with_untracked(|sl| {
                    sl.iter_line_info().find(|info| {
                        info.vline_y <= point.y
                            && info.vline_y + line_height >= point.y
                    })
                })
                .map(|info| info.vline_info)
        };
        let info = info.unwrap_or_else(|| {
            for (y_idx, info) in
                self.iter_rvlines(false, RVLine::default()).enumerate()
            {
                let vline_y = y_idx as f64 * line_height;
                if vline_y <= point.y && vline_y + line_height >= point.y {
                    return info;
                }
            }

            self.last_rvline_info()
        });

        let rvline = info.rvline;
        let line = rvline.line;
        let text_layout = self.text_layout(line);

        let y = text_layout.get_layout_y(rvline.line_index).unwrap_or(0.0);

        let hit_point = text_layout.text.hit_point(Point::new(point.x, y));
        // We have to unapply the phantom text shifting in order to get back to the column in
        // the actual buffer
        let phantom_text = self.doc().phantom_text(line);
        let col = phantom_text.before_col(hit_point.index);
        // Ensure that the column doesn't end up out of bounds, so things like clicking on the far
        // right end will just go to the end of the line.
        let max_col = self.line_end_col(line, mode != Mode::Normal);
        let mut col = col.min(max_col);

        // TODO: we need to handle affinity. Clicking at end of a wrapped line should give it a
        // backwards affinity, while being at the start of the next line should be a forwards aff

        // TODO: this is a hack to get around text layouts not including spaces at the end of
        // wrapped lines, but we want to be able to click on them
        if !hit_point.is_inside {
            // TODO(minor): this is probably wrong in some manners
            col = info.last_col(&self.text_prov(), true);
        }

        let tab_width = self.style().tab_width(line);
        if self.style().atomic_soft_tabs(line) && tab_width > 1 {
            col = snap_to_soft_tab_line_col(
                &self.text(),
                line,
                col,
                SnapDirection::Nearest,
                tab_width,
            );
        }

        ((line, col), hit_point.is_inside)
    }

    // TODO: colposition probably has issues with wrapping?
    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                // TODO: won't this be incorrect with phantom text? Shouldn't this just use
                // line_col_of_point and get the col from that?
                let text_layout = self.text_layout(line);
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
    /// Get the column from a horizontal at a specific line index (in a text layout)
    pub fn rvline_horiz_col(
        &self,
        RVLine { line, line_index }: RVLine,
        horiz: &ColPosition,
        caret: bool,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let text_layout = self.text_layout(line);
                // TODO: It would be better to have an alternate hit point function that takes a
                // line index..
                let y_pos = text_layout
                    .relevant_layouts()
                    .take(line_index)
                    .map(|l| (l.line_ascent + l.line_descent) as f64)
                    .sum();
                let hit_point = text_layout.text.hit_point(Point::new(x, y_pos));
                let n = hit_point.index;

                n.min(self.line_end_col(line, caret))
            }
            // Otherwise it is the same as the other function
            _ => self.line_horiz_col(line, horiz, caret),
        }
    }

    /// Advance to the right in the manner of the given mode.  
    /// This is not the same as the [`Movement::Right`] command.
    pub fn move_right(&self, offset: usize, mode: Mode, count: usize) -> usize {
        self.rope_text().move_right(offset, mode, count)
    }

    /// Advance to the left in the manner of the given mode.
    /// This is not the same as the [`Movement::Left`] command.
    pub fn move_left(&self, offset: usize, mode: Mode, count: usize) -> usize {
        self.rope_text().move_left(offset, mode, count)
    }
}

struct EditorTextProv {
    text: Rope,
    doc: Rc<dyn Document>,
    style: Rc<dyn Styling>,

    viewport: Rect,
}
impl TextLayoutProvider for EditorTextProv {
    // TODO: should this just return a `Rope`, or should `Document::text` return a `&Rope`?
    fn text(&self) -> &Rope {
        &self.text
    }

    fn new_text_layout(
        &self,
        line: usize,
        _font_size: usize,
        _wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine> {
        // TODO: we could share text layouts between different editor views given some knowledge of
        // their wrapping
        let text = self.rope_text();

        let line_content_original = text.line_content(line);

        let font_size = self.style.font_size(self.style.font_size(line));

        // Get the line content with newline characters replaced with spaces
        // and the content without the newline characters
        // TODO: cache or add some way that text layout is created to auto insert the spaces instead
        // though we immediately combine with phantom text so that's a thing.
        let line_content =
            if let Some(s) = line_content_original.strip_suffix("\r\n") {
                format!("{s}  ")
            } else if let Some(s) = line_content_original.strip_suffix('\n') {
                format!("{s} ",)
            } else {
                line_content_original.to_string()
            };
        // Combine the phantom text with the line content
        let phantom_text = self.doc.phantom_text(line);
        let line_content = phantom_text.combine_with_text(&line_content);

        let family = self.style.font_family(line);
        let attrs = Attrs::new()
            .color(self.style.color(EditorColor::Foreground))
            .family(&family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Px(self.style.line_height(line)));
        let mut attrs_list = AttrsList::new(attrs);

        self.style.apply_attr_styles(line, attrs, &mut attrs_list);

        let mut text_layout = TextLayout::new();
        // TODO: we could move tab width setting to be done by the document
        text_layout.set_tab_width(self.style.tab_width(line));
        text_layout.set_text(&line_content, attrs_list);

        match self.style.wrap(line) {
            WrapMethod::None => {}
            WrapMethod::EditorWidth => {
                text_layout.set_wrap(Wrap::Word);
                text_layout.set_size(self.viewport.width() as f32, f32::MAX);
            }
            WrapMethod::WrapWidth { width } => {
                text_layout.set_wrap(Wrap::Word);
                text_layout.set_size(width, f32::MAX);
            }
            // TODO:
            WrapMethod::WrapColumn { .. } => {}
        }

        // TODO(floem-editor):
        // let whitespaces = Self::new_whitespace_layout(
        //     line_content_original,
        //     &text_layout,
        //     &phantom_text,
        //     styling.render_whitespace(),
        // );

        // let indent_line = B::indent_line(self, line, line_content_original);

        // let indent = if indent_line != line {
        //     self.get_text_layout(indent_line, font_size).indent + 1.0
        // } else {
        //     let (_, col) = self.buffer.with_untracked(|buffer| {
        //         let offset = buffer.first_non_blank_character_on_line(indent_line);
        //         buffer.offset_to_line_col(offset)
        //     });
        //     text_layout.hit_position(col).point.x
        // };
        let whitespaces = None;
        let indent = 0.0;

        let mut layout_line = TextLayoutLine {
            text: text_layout,
            extra_style: Vec::new(),
            whitespaces,
            indent,
        };
        self.style.apply_layout_styles(line, &mut layout_line);

        Arc::new(layout_line)
    }

    // TODO: doc has these two functions, should we just make it a common subtrait for having
    // phantom text?
    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        self.doc.before_phantom_col(line, col)
    }

    fn has_multiline_phantom(&self) -> bool {
        self.doc.has_multiline_phantom()
    }
}

struct EditorFontSizes {
    style: ReadSignal<Rc<dyn Styling>>,
}
impl LineFontSizeProvider for EditorFontSizes {
    fn font_size(&self, line: usize) -> usize {
        self.style.with_untracked(|style| style.font_size(line))
    }

    fn cache_id(&self) -> FontSizeCacheId {
        // TODO: we could have a cache id on the styling
        0
    }
}
