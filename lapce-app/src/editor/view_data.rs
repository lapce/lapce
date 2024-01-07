use std::{
    cell::{Cell, RefCell},
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

use floem::{
    cosmic_text::{LayoutLine, TextLayout, Wrap},
    peniko::{
        kurbo::{Point, Rect},
        Color,
    },
    reactive::{batch, untrack, ReadSignal, RwSignal, Scope},
};
use lapce_core::{
    buffer::rope_text::{RopeText, RopeTextVal},
    char_buffer::CharBuffer,
    cursor::{ColPosition, CursorAffinity},
    mode::Mode,
    soft_tab::{snap_to_soft_tab_line_col, SnapDirection},
    word::WordCursor,
};
use lapce_xi_rope::Rope;

use crate::{
    config::{editor::WrapStyle, LapceConfig},
    doc::{phantom_text::PhantomTextLine, Document, DocumentExt},
    find::{Find, FindResult},
};

use super::{
    compute_screen_lines,
    diff::DiffInfo,
    view::ScreenLines,
    visual_line::{
        hit_position_aff, FontSizeCacheId, LayoutEvent, LineFontSizeProvider, Lines,
        RVLine, ResolvedWrap, TextLayoutProvider, VLine, VLineInfo,
    },
};

#[derive(Clone, Debug)]
pub struct LineExtraStyle {
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: f64,
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
impl TextLayoutLine {
    /// The number of line breaks in the text layout. Always at least `1`.
    pub fn line_count(&self) -> usize {
        self.relevant_layouts().count().max(1)
    }

    /// Iterate over all the layouts that are nonempty.  
    /// Note that this may be empty if the line is completely empty, like the last line
    fn relevant_layouts(&self) -> impl Iterator<Item = &'_ LayoutLine> + '_ {
        // Even though we only have one hard line (and thus only one `lines` entry) typically, for
        // normal buffer lines, we can have more than one due to multiline phantom text. So we have
        // to sum over all of the entries line counts.
        self.text
            .lines
            .iter()
            .flat_map(|l| l.layout_opt().as_deref())
            .flat_map(|ls| ls.iter())
            .filter(|l| !l.glyphs.is_empty())
    }

    /// Iterator over the (start, end) columns of the relevant layouts.  
    pub fn layout_cols<'a>(
        &'a self,
        text_prov: impl TextLayoutProvider + 'a,
        line: usize,
    ) -> impl Iterator<Item = (usize, usize)> + 'a {
        let mut prefix = None;
        // Include an entry if there is nothing
        if self.text.lines.len() == 1 {
            let line_start = self.text.lines[0].start_index();
            if let Some(layouts) = self.text.lines[0].layout_opt().as_deref() {
                // Do we need to require !layouts.is_empty()?
                if !layouts.is_empty() && layouts.iter().all(|l| l.glyphs.is_empty())
                {
                    // We assume the implicit glyph start is zero
                    prefix = Some((line_start, line_start));
                }
            }
        }

        let line_v = line;
        let iter = self
            .text
            .lines
            .iter()
            .filter_map(|line| line.layout_opt().as_deref().map(|ls| (line, ls)))
            .flat_map(|(line, ls)| ls.iter().map(move |l| (line, l)))
            .filter(|(_, l)| !l.glyphs.is_empty())
            .map(move |(tl_line, l)| {
                let line_start = tl_line.start_index();

                let start = line_start + l.glyphs[0].start;
                let end = line_start + l.glyphs.last().unwrap().end;

                let text = text_prov.rope_text();
                // We can't just use the original end, because the *true* last glyph on the line
                // may be a space, but it isn't included in the layout! Though this only happens
                // for single spaces, for some reason.
                let pre_end = text_prov.before_phantom_col(line_v, end);
                let line_offset = text.offset_of_line(line);

                // TODO(minor): We don't really need the entire line, just the two characters after
                let line_end = text.line_end_col(line, true);

                let end = if pre_end <= line_end {
                    let after = text
                        .slice_to_cow(line_offset + pre_end..line_offset + line_end);
                    if after.starts_with(' ') && !after.starts_with("  ") {
                        end + 1
                    } else {
                        end
                    }
                } else {
                    end
                };

                (start, end)
            });

        prefix.into_iter().chain(iter)
    }

    /// Iterator over the start columns of the relevant layouts
    pub fn start_layout_cols<'a>(
        &'a self,
        text_prov: impl TextLayoutProvider + 'a,
        line: usize,
    ) -> impl Iterator<Item = usize> + 'a {
        self.layout_cols(text_prov, line).map(|(start, _)| start)
    }

    /// Get the top y position of the given line index
    pub fn get_layout_y(&self, nth: usize) -> Option<f64> {
        if nth == 0 {
            return Some(0.0);
        }

        let mut line_y = 0.0;
        for (i, layout) in self.relevant_layouts().enumerate() {
            // This logic matches how layout run iter computes the line_y
            let line_height = layout.line_ascent + layout.line_descent;
            if i == nth {
                let offset = (line_height
                    - (layout.glyph_ascent + layout.glyph_descent))
                    / 2.0;

                return Some((line_y - offset - layout.glyph_descent) as f64);
            }

            line_y += line_height;
        }

        None
    }

    /// Get the (start x, end x) positions of the given line index
    pub fn get_layout_x(&self, nth: usize) -> Option<(f32, f32)> {
        let layout = self.relevant_layouts().nth(nth)?;

        let start = layout.glyphs.first().map(|g| g.x).unwrap_or(0.0);
        let end = layout.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0);

        Some((start, end))
    }
}

#[derive(Clone)]
pub enum EditorViewKind {
    Normal,
    Diff(DiffInfo),
}

impl EditorViewKind {
    pub fn is_normal(&self) -> bool {
        matches!(self, EditorViewKind::Normal)
    }
}

/// Data specific to the rendering of a view.
/// This has various helper methods that may dispatch to the held [`Document`] signal, but are
/// untracked by default. If you need your signal to depend on the document,
/// then you should call [`EditorViewData::track_doc`].
/// Note: This should be cheap to clone.
#[derive(Clone)]
pub struct EditorViewData {
    /// Equivalent to the `EditorData::doc` that contains this view.
    pub doc: RwSignal<Rc<Document>>,
    pub kind: RwSignal<EditorViewKind>,
    /// Equivalent to the `EditorData::viewport that contains this view data.
    pub viewport: RwSignal<Rect>,
    /// Lines holds various `RefCell`/`Cell`s so it can be accessed immutably, while still in
    /// reality modifying it.
    lines: Rc<Lines>,

    cx: Cell<Scope>,
    effects_cx: Cell<Scope>,

    pub config: ReadSignal<Arc<LapceConfig>>,

    pub screen_lines: RwSignal<ScreenLines>,
}

impl EditorViewData {
    pub fn new(
        cx: Scope,
        doc: Rc<Document>,
        kind: EditorViewKind,
        viewport: RwSignal<Rect>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> EditorViewData {
        let font_sizes = RefCell::new(Arc::new(ViewDataFontSizes {
            config,
            loaded: doc.loaded.read_only(),
        }));
        let lines = Rc::new(Lines::new(cx, font_sizes));
        let doc = cx.create_rw_signal(doc);
        let kind = cx.create_rw_signal(kind);

        let screen_lines =
            cx.create_rw_signal(ScreenLines::new(cx, viewport.get_untracked()));

        let view = EditorViewData {
            doc,
            kind,
            viewport,
            lines,
            cx: Cell::new(cx),
            effects_cx: Cell::new(cx.create_child()),
            config,
            screen_lines,
        };

        // Watch the relevant parts for recalculating the screen lines
        create_view_effects(view.effects_cx.get(), &view);

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
        let doc = self.doc.get_untracked();
        doc.buffer.with_untracked(|b| b.text().clone())
    }

    /// Return a [`RopeTextVal`] wrapper.
    /// Unfortunately, we can't implement [`RopeText`] directly on [`EditorViewData`] due to
    /// it not having a reference to the rope.
    pub fn rope_text(&self) -> RopeTextVal {
        RopeTextVal::new(self.text())
    }

    /// Get the text layout provider of this view.  
    /// Typically should not be needed outside of view's code.
    pub(crate) fn text_prov(&self) -> ViewDataTextLayoutProv {
        ViewDataTextLayoutProv {
            text: self.text(),
            doc: self.doc,
        }
    }

    /// Return the [`Document`]'s [`Find`] instance. Find uses signals, and so can be updated.
    pub fn find(&self) -> Find {
        self.doc.with_untracked(|doc| doc.find().clone())
    }

    pub fn find_result(&self) -> FindResult {
        self.doc.get_untracked().backend.find_result.clone()
    }

    pub fn update_find(&self) {
        self.doc.with_untracked(|doc| doc.update_find());
    }

    /// The current revision of the underlying buffer. This is used to track when the buffer has
    /// changed.
    pub fn rev(&self) -> u64 {
        self.doc.with_untracked(|doc| doc.rev())
    }

    pub fn cache_rev(&self) -> u64 {
        self.doc.get_untracked().cache_rev.get_untracked()
    }

    /// The document for the given view was swapped out.
    pub fn update_doc(&self, doc: Rc<Document>) {
        batch(|| {
            // Get rid of all the effects
            self.effects_cx.get().dispose();

            *self.lines.font_sizes.borrow_mut() = Arc::new(ViewDataFontSizes {
                config: self.config,
                loaded: doc.loaded.read_only(),
            });
            self.lines.clear(0, None);
            self.doc.set(doc);
            self.screen_lines.update(|screen_lines| {
                screen_lines.clear(self.viewport.get_untracked())
            });
            // Recreate the effects
            self.effects_cx.set(self.cx.get().create_child());
            create_view_effects(self.effects_cx.get(), self);
        });
    }

    /// Duplicate as a new view which refers to the same document.
    pub fn duplicate(&self, cx: Scope, viewport: RwSignal<Rect>) -> Self {
        let kind = self.kind.get_untracked();

        EditorViewData::new(
            cx,
            self.doc.get_untracked(),
            kind,
            viewport,
            self.config,
        )
    }

    pub fn line_phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc.with_untracked(|doc| doc.line_phantom_text(line))
    }

    /// Get the text layout for the given line.
    /// If the text layout is not cached, it will be created and cached.
    /// This triggers a layout event.
    pub fn get_text_layout(&self, line: usize) -> Arc<TextLayoutLine> {
        self.get_text_layout_trigger(line, true)
    }

    /// Get the text layout for the given line, deciding whether or not it should trigger a layout
    /// event.  
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout_trigger(
        &self,
        line: usize,
        trigger: bool,
    ) -> Arc<TextLayoutLine> {
        let config = self.config.get_untracked();
        let config_id: u64 = config.id;

        let text_prov = self.text_prov();

        self.lines
            .get_init_text_layout(config_id, &text_prov, line, trigger)
    }

    /// Try to get a text layout for the given line, without creating it if it doesn't exist.  
    /// Note that it may still clear the cache if the config id has changed.
    pub fn try_get_text_layout(&self, line: usize) -> Option<Arc<TextLayoutLine>> {
        let config = self.config.get_untracked();
        let config_id = config.id;

        self.lines.try_get_text_layout(config_id, line)
    }

    pub fn indent_unit(&self) -> &'static str {
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.indent_unit()))
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
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.num_lines()))
    }

    /// The last allowed buffer line in the document.
    pub fn last_line(&self) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.last_line()))
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
        self.doc.with_untracked(|doc| {
            doc.buffer.with_untracked(|b| b.offset_to_line_col(offset))
        })
    }

    pub fn offset_of_line(&self, offset: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer.with_untracked(|b| b.offset_of_line(offset))
        })
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer
                .with_untracked(|b| b.offset_of_line_col(line, col))
        })
    }

    /// Get the buffer line of an offset
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer.with_untracked(|b| b.line_of_offset(offset))
        })
    }

    /// Returns the offset into the buffer of the first non blank character on the given line.
    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer
                .with_untracked(|b| b.first_non_blank_character_on_line(line))
        })
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer.with_untracked(|b| b.line_end_col(line, caret))
        })
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        self.doc.with_untracked(|doc| {
            doc.buffer.with_untracked(|b| b.select_word(offset))
        })
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
        let text_layout = self.get_text_layout(line);
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
        let config = self.config.get_untracked();
        let line_height = config.editor.line_height() as f64;

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
        let config = self.config.get_untracked();

        let line_height = config.editor.line_height() as f64;
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
        let text_layout = self.get_text_layout(line);

        let y = text_layout.get_layout_y(rvline.line_index).unwrap_or(0.0);

        let hit_point = text_layout.text.hit_point(Point::new(point.x, y));
        // We have to unapply the phantom text shifting in order to get back to the column in
        // the actual buffer
        let phantom_text = self.line_phantom_text(line);
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
                let text_layout = self.get_text_layout(line);
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
                let text_layout = self.get_text_layout(line);
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
        self.doc.with_untracked(|doc| {
            doc.buffer
                .with_untracked(|b| b.move_right(offset, mode, count))
        })
    }

    /// Advance to the left in the manner of the given mode.
    /// This is not the same as the [`Movement::Left`] command.
    pub fn move_left(&self, offset: usize, mode: Mode, count: usize) -> usize {
        self.doc.with_untracked(|doc| {
            doc.buffer
                .with_untracked(|b| b.move_left(offset, mode, count))
        })
    }

    /// Find the next/previous offset of the match of the given character.
    /// This is intended for use by the [`Movement::NextUnmatched`] and
    /// [`Movement::PreviousUnmatched`] commands.
    pub fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        // This needs the doc's syntax, but it isn't cheap to clone
        // so this has to be a method on view for now.
        self.doc.with_untracked(|doc| {
            doc.syntax().with_untracked(|syntax| {
                if syntax.layers.is_some() {
                    syntax
                        .find_tag(offset, previous, &CharBuffer::from(ch))
                        .unwrap_or(offset)
                } else {
                    let text = doc.buffer.with_untracked(|b| b.text().clone());
                    let mut cursor = WordCursor::new(&text, offset);
                    let new_offset = if previous {
                        cursor.previous_unmatched(ch)
                    } else {
                        cursor.next_unmatched(ch)
                    };

                    new_offset.unwrap_or(offset)
                }
            })
        })
    }

    /// Find the offset of the matching pair character.
    /// This is intended for use by the [`Movement::MatchPairs`] command.
    pub fn find_matching_pair(&self, offset: usize) -> usize {
        // This needs the doc's syntax, but it isn't cheap to clone
        // so this has to be a method on view for now.
        self.doc.with_untracked(|doc| {
            doc.syntax().with_untracked(|syntax| {
                if syntax.layers.is_some() {
                    syntax.find_matching_pair(offset).unwrap_or(offset)
                } else {
                    let text = doc.buffer.with_untracked(|b| b.text().clone());
                    WordCursor::new(&text, offset)
                        .match_pairs()
                        .unwrap_or(offset)
                }
            })
        })
    }
}

#[derive(Clone)]
pub(crate) struct ViewDataTextLayoutProv {
    text: Rope,
    doc: RwSignal<Rc<Document>>,
}
impl TextLayoutProvider for ViewDataTextLayoutProv {
    fn text(&self) -> &Rope {
        &self.text
    }

    fn new_text_layout(
        &self,
        line: usize,
        font_size: usize,
        wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine> {
        let mut text_layout = self
            .doc
            .with_untracked(|doc| doc.get_text_layout(line, font_size));
        // TODO: This will potentially duplicate the text layout even without wrapping, but without wrapping we can just use the same styles.
        // TODO: It does also pretty similar for other wrapping modes. So it would be good to have smarter deduplication, like if both of your splits for the same doc have a column wrap of 100 then they can share all of the text layout lines.
        let text_layout_r = Arc::make_mut(&mut text_layout);
        match wrap {
            ResolvedWrap::None => {
                // We do not have to set the wrap mode if we do not set the width
            }
            ResolvedWrap::Column(_col) => todo!(),
            ResolvedWrap::Width(px) => {
                // TODO: we could do some more invasive text layout structure to avoid multiple arcs since this definitely clones..
                text_layout_r.text.set_wrap(Wrap::Word);
                text_layout_r.text.set_size(px, f32::MAX);
            }
        }

        self.doc.with_untracked(|doc| {
            doc.apply_styles(line, text_layout_r);
        });

        text_layout
    }

    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        // TODO: this only needs to shift the col so it does not need the full string allocated
        // phantom text!
        self.doc
            .with_untracked(|doc| doc.line_phantom_text(line))
            .before_col(col)
    }

    fn has_multiline_phantom(&self) -> bool {
        // TODO: We could be more specific here. Use the config for error lens &etc to determine
        // whether we can ever have multiline phantom text in this view.
        // Perhaps should track whether any multiline phantom text gets added, to get the fast path
        // more often.
        true
    }
}

struct ViewDataFontSizes {
    config: ReadSignal<Arc<LapceConfig>>,
    loaded: ReadSignal<bool>,
}
impl LineFontSizeProvider for ViewDataFontSizes {
    fn font_size(&self, _line: usize) -> usize {
        // TODO: code lens
        self.config
            .with_untracked(|config| config.editor.font_size())
    }

    fn cache_id(&self) -> FontSizeCacheId {
        let mut hasher = DefaultHasher::new();

        // TODO: is this actually good enough for comparing cache state?
        // Potentially we should just have it return an arbitrary type that impl's Eq
        self.config.with_untracked(|config| {
            // TODO: we could do only pieces relevant to the lines
            config.id.hash(&mut hasher);
        });

        self.loaded.with_untracked(|loaded| {
            loaded.hash(&mut hasher);
        });

        hasher.finish()
    }
}

/// Minimum width that we'll allow the view to be wrapped at.
const MIN_WRAPPED_WIDTH: f32 = 100.0;

/// Create various reactive effects to update the screen lines whenever relevant parts of the view,
/// doc, text layouts, viewport, etc. change.
/// This tries to be smart to a degree.
fn create_view_effects(cx: Scope, view: &EditorViewData) {
    // Cloning is fun.
    let view = view.clone();
    let view2 = view.clone();
    let view3 = view.clone();
    let view4 = view.clone();

    let update_screen_lines = |view: &EditorViewData| {
        // This function should not depend on the viewport signal directly.

        // This is wrapped in an update to make any updates-while-updating very obvious
        // which they wouldn't be if we computed and then `set`.
        view.screen_lines.update(|screen_lines| {
            let new_screen_lines = compute_screen_lines(
                view.config,
                screen_lines.base,
                view.kind.read_only(),
                view.doc.read_only(),
                &view.lines,
                view.text_prov(),
            );

            *screen_lines = new_screen_lines;
        });
    };

    // Listen for cache revision changes (essentially edits to the file or requiring
    // changes on text layouts, like if diagnostics load in)
    cx.create_effect(move |_| {
        // We can't put this with the other effects because we only want to update screen lines if
        // the cache rev actually changed
        let cache_rev = view.doc.with(|doc| doc.cache_rev).get();
        view.lines.check_cache_rev(cache_rev);
    });

    // Listen for layout events, currently only when a layout is created, and update screen
    // lines based on that
    view3.lines.layout_event.listen_with(cx, move |val| {
        let view = &view2;
        // TODO: Move this logic onto screen lines somehow, perhaps just an auxilary
        // function, to avoid getting confused about what is relevant where.

        match val {
            LayoutEvent::CreatedLayout { line, .. } => {
                let sl = view.screen_lines.get_untracked();

                // Intelligently update screen lines, avoiding recalculation if possible
                let should_update = sl.on_created_layout(view, line);

                if should_update {
                    untrack(|| {
                        update_screen_lines(view);
                    });
                }
            }
        }
    });

    // TODO: should we have some debouncing for editor width? Ideally we'll be fast enough to not
    // even need it, though we might not want to use a bunch of cpu whilst resizing anyway.

    let viewport_changed_trigger = cx.create_trigger();

    // Watch for changes to the viewport so that we can alter the wrapping
    // As well as updating the screen lines base
    cx.create_effect(move |_| {
        let view = &view3;

        let viewport = view.viewport.get();
        let config = view.config.get();
        let wrap_style = if let EditorViewKind::Diff(_) = view.kind.get() {
            // TODO: let diff have wrapped text
            WrapStyle::None
        } else {
            config.editor.wrap_style
        };
        let res_wrap = match wrap_style {
            WrapStyle::None => ResolvedWrap::None,
            // TODO: ensure that these values have some minimums that is not too small
            WrapStyle::EditorWidth => {
                ResolvedWrap::Width((viewport.width() as f32).max(MIN_WRAPPED_WIDTH))
            }
            // WrapStyle::WrapColumn => ResolvedWrap::Column(config.editor.wrap_column),
            WrapStyle::WrapWidth => ResolvedWrap::Width(
                (config.editor.wrap_width as f32).max(MIN_WRAPPED_WIDTH),
            ),
        };

        view.lines.set_wrap(res_wrap);

        // Update the base
        let base = view.screen_lines.with_untracked(|sl| sl.base);

        // TODO: should this be a with or with_untracked?
        if viewport != base.with_untracked(|base| base.active_viewport) {
            batch(|| {
                base.update(|base| {
                    base.active_viewport = viewport;
                });
                // TODO: Can I get rid of this and just call update screen lines with an
                // untrack around it?
                viewport_changed_trigger.notify();
            });
        }
    });
    // Watch for when the viewport as changed in a relevant manner
    // and for anything that `update_screen_lines` tracks.
    cx.create_effect(move |_| {
        viewport_changed_trigger.track();

        update_screen_lines(&view4);
    });
}
