use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use floem::{
    cosmic_text::TextLayout,
    peniko::{kurbo::Point, Color},
    reactive::{ReadSignal, RwSignal, Scope},
    views::VirtualListVector,
};
use lapce_core::{
    buffer::{
        diff::DiffLines,
        rope_text::{RopeText, RopeTextVal},
    },
    char_buffer::CharBuffer,
    cursor::ColPosition,
    mode::Mode,
    soft_tab::{snap_to_soft_tab_line_col, SnapDirection},
    word::WordCursor,
};
use lapce_xi_rope::Rope;

use crate::{
    config::LapceConfig,
    doc::{phantom_text::PhantomTextLine, Document},
    find::{Find, FindResult},
};

use super::{diff::DiffInfo, FONT_SIZE};

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
    cache_rev: u64,
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
            cache_rev: 0,
            layouts: HashMap::new(),
            max_width: 0.0,
        }
    }

    pub fn clear(&mut self, cache_rev: u64) {
        self.layouts.clear();
        self.cache_rev = cache_rev;
        self.max_width = 0.0;
    }

    pub fn check_attributes(&mut self, config_id: u64) {
        if self.config_id != config_id {
            self.clear(self.cache_rev + 1);
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

// TODO(minor): Should this go in another file? It doesn't really need to be with the drawing code for editor views
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
    /// The text layouts for the document. This may be shared with other views.
    pub text_layouts: Rc<RefCell<TextLayoutCache>>,

    pub config: ReadSignal<Arc<LapceConfig>>,
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
                style_rev: self.cache_rev(),
                line,
                text: self.get_text_layout(line, FONT_SIZE),
            })
            .collect::<Vec<_>>();
        lines.into_iter()
    }
}

impl EditorViewData {
    pub fn new(
        cx: Scope,
        doc: Rc<Document>,
        kind: EditorViewKind,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> EditorViewData {
        EditorViewData {
            doc: cx.create_rw_signal(doc),
            kind: cx.create_rw_signal(kind),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            config,
        }
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

    /// Return the [`Document`]'s [`Find`] instance. Find uses signals, and so can be updated.
    pub fn find(&self) -> Find {
        self.doc.with_untracked(|doc| doc.find().clone())
    }

    pub fn find_result(&self) -> FindResult {
        self.doc.get_untracked().find_result.clone()
    }

    pub fn update_find(&self) {
        self.doc.with_untracked(|doc| doc.update_find());
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

    pub fn cache_rev(&self) -> u64 {
        self.doc.get_untracked().cache_rev.get_untracked()
    }

    /// The document for the given view was swapped out.
    pub fn update_doc(&self, doc: Rc<Document>) {
        self.doc.set(doc);
        self.text_layouts.borrow_mut().clear(0);
    }

    /// Duplicate as a new view which refers to the same document.
    pub fn duplicate(&self, cx: Scope) -> Self {
        // TODO: This is correct right now, as it has the views share the same text layout cache.
        // However, once we have line wrapping or other view-specific rendering changes, this should check for whether they're different.
        // This will likely require more information to be passed into duplicate,
        // like whether the wrap width will be the editor's width, and so it is unlikely to be exactly the same as the current view.
        EditorViewData {
            doc: cx.create_rw_signal(self.doc.get_untracked()),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            kind: cx.create_rw_signal(self.kind.get_untracked()),
            config: self.config,
        }
    }

    pub fn line_phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc.with_untracked(|doc| doc.line_phantom_text(line))
    }

    /// Get the text layout for the given line.
    /// If the text layout is not cached, it will be created and cached.
    pub fn get_text_layout(
        &self,
        line: usize,
        font_size: usize,
    ) -> Arc<TextLayoutLine> {
        {
            let mut text_layouts = self.text_layouts.borrow_mut();
            let cache_rev = self.doc.get_untracked().cache_rev.get_untracked();
            if cache_rev != text_layouts.cache_rev {
                text_layouts.clear(cache_rev);
            }
        }

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
            let text_layout = self
                .doc
                .with_untracked(|doc| doc.get_text_layout(line, font_size));
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
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.indent_unit()))
    }

    // ==== Position Information ====

    /// The number of visual lines in the document.
    pub fn num_lines(&self) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.num_lines()))
    }

    /// The last allowed line in the document.
    pub fn last_line(&self) -> usize {
        self.doc
            .with_untracked(|doc| doc.buffer.with_untracked(|b| b.last_line()))
    }

    // ==== Line/Column Positioning ====

    /// Convert an offset into the buffer into a line and column.
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
        let (line_height, font_size) =
            (config.editor.line_height(), config.editor.font_size());

        let y = self.visual_line(line) * line_height;

        let line = line.min(self.last_line());

        let phantom_text = self.line_phantom_text(line);
        let col = phantom_text.col_after(col, false);

        let mut x_shift = 0.0;
        if font_size < config.editor.font_size() {
            let mut col = 0usize;
            self.doc.get_untracked().buffer.with_untracked(|buffer| {
                let line_content = buffer.line_content(line);
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

    pub fn actual_line(&self, visual_line: usize, bottom_affinity: bool) -> usize {
        self.kind.with_untracked(|kind| match kind {
            EditorViewKind::Normal => visual_line,
            EditorViewKind::Diff(diff) => {
                let is_right = diff.is_right;
                let mut actual_line: usize = 0;
                let mut current_visual_line = 0;
                let mut last_change: Option<&DiffLines> = None;
                let mut changes = diff.changes.iter().peekable();
                while let Some(change) = changes.next() {
                    match (is_right, change) {
                        (true, DiffLines::Left(range)) => {
                            if let Some(DiffLines::Right(_)) = changes.peek() {
                            } else {
                                current_visual_line += range.len();
                                if current_visual_line >= visual_line {
                                    return if bottom_affinity {
                                        actual_line
                                    } else {
                                        actual_line.saturating_sub(1)
                                    };
                                }
                            }
                        }
                        (false, DiffLines::Right(range)) => {
                            let len = if let Some(DiffLines::Left(r)) = last_change {
                                range.len() - r.len().min(range.len())
                            } else {
                                range.len()
                            };
                            if len > 0 {
                                current_visual_line += len;
                                if current_visual_line >= visual_line {
                                    return actual_line;
                                }
                            }
                        }
                        (true, DiffLines::Right(range))
                        | (false, DiffLines::Left(range)) => {
                            let len = range.len();
                            if current_visual_line + len > visual_line {
                                return range.start
                                    + (visual_line - current_visual_line);
                            }
                            current_visual_line += len;
                            actual_line += len;
                            if is_right {
                                if let Some(DiffLines::Left(r)) = last_change {
                                    let len = r.len() - r.len().min(range.len());
                                    if len > 0 {
                                        current_visual_line += len;
                                        if current_visual_line > visual_line {
                                            return if bottom_affinity {
                                                actual_line
                                            } else {
                                                actual_line - range.len()
                                            };
                                        }
                                    }
                                }
                            }
                        }
                        (_, DiffLines::Both(info)) => {
                            let len = info.right.len();
                            let start = if is_right {
                                info.right.start
                            } else {
                                info.left.start
                            };

                            if let Some(skip) = info.skip.as_ref() {
                                if current_visual_line + skip.start == visual_line {
                                    return if bottom_affinity {
                                        actual_line + skip.end
                                    } else {
                                        (actual_line + skip.start).saturating_sub(1)
                                    };
                                } else if current_visual_line + skip.start + 1
                                    > visual_line
                                {
                                    return actual_line + visual_line
                                        - current_visual_line;
                                } else if current_visual_line + len - skip.len() + 1
                                    >= visual_line
                                {
                                    return actual_line
                                        + skip.end
                                        + (visual_line
                                            - current_visual_line
                                            - skip.start
                                            - 1);
                                }
                                actual_line += len;
                                current_visual_line += len - skip.len() + 1;
                            } else {
                                if current_visual_line + len > visual_line {
                                    return start
                                        + (visual_line - current_visual_line);
                                }
                                current_visual_line += len;
                                actual_line += len;
                            }
                        }
                    }
                    last_change = Some(change);
                }
                actual_line
            }
        })
    }

    pub fn visual_line(&self, line: usize) -> usize {
        self.kind.with_untracked(|kind| match kind {
            EditorViewKind::Normal => line,
            EditorViewKind::Diff(diff) => {
                let is_right = diff.is_right;
                let mut last_change: Option<&DiffLines> = None;
                let mut visual_line = 0;
                let mut changes = diff.changes.iter().peekable();
                while let Some(change) = changes.next() {
                    match (is_right, change) {
                        (true, DiffLines::Left(range)) => {
                            if let Some(DiffLines::Right(_)) = changes.peek() {
                            } else {
                                visual_line += range.len();
                            }
                        }
                        (false, DiffLines::Right(range)) => {
                            let len = if let Some(DiffLines::Left(r)) = last_change {
                                range.len() - r.len().min(range.len())
                            } else {
                                range.len()
                            };
                            if len > 0 {
                                visual_line += len;
                            }
                        }
                        (true, DiffLines::Right(range))
                        | (false, DiffLines::Left(range)) => {
                            if line < range.end {
                                return visual_line + line - range.start;
                            }
                            visual_line += range.len();
                            if is_right {
                                if let Some(DiffLines::Left(r)) = last_change {
                                    let len = r.len() - r.len().min(range.len());
                                    if len > 0 {
                                        visual_line += len;
                                    }
                                }
                            }
                        }
                        (_, DiffLines::Both(info)) => {
                            let end = if is_right {
                                info.right.end
                            } else {
                                info.left.end
                            };
                            if line >= end {
                                visual_line += info.right.len()
                                    - info
                                        .skip
                                        .as_ref()
                                        .map(|skip| skip.len().saturating_sub(1))
                                        .unwrap_or(0);
                                last_change = Some(change);
                                continue;
                            }

                            let start = if is_right {
                                info.right.start
                            } else {
                                info.left.start
                            };
                            if let Some(skip) = info.skip.as_ref() {
                                if start + skip.start > line {
                                    return visual_line + line - start;
                                } else if start + skip.end > line {
                                    return visual_line + skip.start;
                                } else {
                                    return visual_line
                                        + (line - start - skip.len() + 1);
                                }
                            } else {
                                return visual_line + line - start;
                            }
                        }
                    }
                    last_change = Some(change);
                }
                visual_line
            }
        })
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

        let visual_line =
            (point.y / config.editor.line_height() as f64).floor() as usize;
        let line = self.actual_line(visual_line, true);
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
            doc.syntax.with_untracked(|syntax| {
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
            doc.syntax.with_untracked(|syntax| {
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
