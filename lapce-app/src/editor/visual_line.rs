//! Visual Line implementation  
//!   
//! Files are easily broken up into buffer lines by just spliiting on `\n` or `\r\n`.  
//! However, editors require features like wrapping and multiline phantom text. These break the
//! nice one-to-one correspondence between buffer lines and visual lines.  
//!   
//! When rendering with those, we have to display based on visual lines rather than the
//! underlying buffer lines. As well, it is expected for interaction - like movement and clicking -
//! to work in a similar intuitive manner as it would be if there was no wrapping or phantom text.  
//! Ex: Moving down a line should move to the next visual line, not the next buffer line by
//! default.  
//! (Sometimes! Some vim defaults are to move to the next buffer line, or there might be other
//! differences)  
//!  
//! There's two types of ways of talking about Visual Lines:  
//! - [`VLine`]: Variables are often written with `vline` in the name  
//! - [`RVLine`]: Variables are often written with `rvline` in the name  
//!   
//! [`VLine`] is an absolute visual line within the file. This is useful for some positioning tasks
//! but is more expensive to calculate due to the nontriviality of the `buffer line <-> visual line`
//! conversion when the file has any wrapping or multiline phantom text.  
//!  
//! Typically, code should prefer to use [`RVLine`]. This simply stores the underlying
//! buffer line, and a line index. This is not enough for absolute positioning within the display,
//! but it is enough for most other things (like movement). This is easier to calculate since it
//! only needs to find the right (potentially wrapped or multiline) layout for the easy-to-work
//! with buffer line.
//!   
//! [`VLine`] is a single `usize` internally which can be multiplied by the line-height to get the
//! absolute position. This means that it is not stable across text layouts being changed.    
//! An [`RVLine`] holds the buffer line and the 'line index' within the layout. The line index
//! would be `0` for the first line, `1` if it is on the second wrapped line, etc. This is more
//! stable across text layouts being changed, as it is only relative to a specific line.  
//!   
//! -----
//!   
//! [`Lines`] is the main structure. It is responsible for holding the text layouts, as well as
//! providing the functions to convert between (r)vlines and buffer lines.
//!   
//! ----
//!
//! Many of [`Lines`] functions are passed a [`TextLayoutProvider`].  
//! This serves the dual-purpose of giving us the text of the underlying file, as well as
//! for constructing the text layouts that we use for rendering.  
//! Having a trait that is passed in simplifies the logic, since the caller is the one who tracks
//! the text in whatever manner they chose.  

// TODO: This file is getting long. Possibly it should be broken out into multiple files.
// Especially as it will only grow with more utility functions.

// TODO(minor): We use a lot of `impl TextLayoutProvider`.
// This has the desired benefit of inlining the functions, so that the compiler can optimize the
// logic better than a naive for-loop or whatnot.
// However it does have the issue that it overuses generics, and we sometimes end up instantiating
// multiple versions of the same function. `T: TextLayoutProvider`, `&T`...
// - It would be better to standardize on one way of doing that, probably `&impl TextLayoutProvider`

use std::{
    cell::{Cell, RefCell},
    cmp::Ordering,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use floem::{cosmic_text::LayoutGlyph, reactive::Scope};
use lapce_core::{
    buffer::rope_text::{RopeText, RopeTextRef},
    cursor::CursorAffinity,
    word::WordCursor,
};
use lapce_xi_rope::{Interval, Rope};

use crate::listener::Listener;

use super::view_data::TextLayoutLine;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedWrap {
    None,
    Column(usize),
    Width(f32),
}
impl ResolvedWrap {
    pub fn is_different_kind(self, other: ResolvedWrap) -> bool {
        !matches!(
            (self, other),
            (ResolvedWrap::None, ResolvedWrap::None)
                | (ResolvedWrap::Column(_), ResolvedWrap::Column(_))
                | (ResolvedWrap::Width(_), ResolvedWrap::Width(_))
        )
    }
}

/// A line within the editor view.  
/// This gives the absolute position of the visual line.  
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VLine(pub usize);
impl VLine {
    pub fn get(&self) -> usize {
        self.0
    }
}

/// A visual line relative to some other line within the editor view.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RVLine {
    /// The buffer line this is for
    pub line: usize,
    /// The index of the actual visual line's layout
    pub line_index: usize,
}
impl RVLine {
    pub fn new(line: usize, line_index: usize) -> RVLine {
        RVLine { line, line_index }
    }

    /// Is this the first visual line for the buffer line?
    pub fn is_first(&self) -> bool {
        self.line_index == 0
    }
}

/// (Font Size -> (Buffer Line Number -> Text Layout))  
pub type Layouts = HashMap<usize, HashMap<usize, Arc<TextLayoutLine>>>;

#[derive(Default)]
pub struct TextLayoutCache {
    /// The id of the last config so that we can clear when the config changes
    config_id: u64,
    /// The most recent cache revision of the document.
    cache_rev: u64,
    /// (Font Size -> (Buffer Line Number -> Text Layout))  
    /// Different font-sizes are cached separately, which is useful for features like code lens
    /// where the font-size can rapidly change.  
    /// It would also be useful for a prospective minimap feature.  
    pub layouts: Layouts,
    /// The maximum width seen so far, used to determine if we need to show horizontal scrollbar
    pub max_width: f64,
}
impl TextLayoutCache {
    pub fn clear(&mut self, cache_rev: u64, config_id: Option<u64>) {
        self.layouts.clear();
        if let Some(config_id) = config_id {
            self.config_id = config_id;
        }
        self.cache_rev = cache_rev;
        self.max_width = 0.0;
    }

    /// Clear the layouts without changing the document cache revision.  
    /// Ex: Wrapping width changed, which does not change what the document holds.
    pub fn clear_unchanged(&mut self) {
        self.layouts.clear();
        self.max_width = 0.0;
    }

    pub fn get(
        &self,
        font_size: usize,
        line: usize,
    ) -> Option<&Arc<TextLayoutLine>> {
        self.layouts.get(&font_size).and_then(|c| c.get(&line))
    }

    pub fn get_mut(
        &mut self,
        font_size: usize,
        line: usize,
    ) -> Option<&mut Arc<TextLayoutLine>> {
        self.layouts
            .get_mut(&font_size)
            .and_then(|c| c.get_mut(&line))
    }

    /// Get the (start, end) columns of the (line, line_index)
    pub fn get_layout_col(
        &self,
        text_prov: &impl TextLayoutProvider,
        font_size: usize,
        line: usize,
        line_index: usize,
    ) -> Option<(usize, usize)> {
        self.get(font_size, line)
            .and_then(|l| l.layout_cols(text_prov, line).nth(line_index))
    }

    /// Check whether the config id has changed, clearing the cache if it has.
    pub fn check_attributes(&mut self, config_id: u64) {
        if self.config_id != config_id {
            self.clear(self.cache_rev + 1, Some(config_id));
        }
    }
}

// TODO(minor): Should we rename this? It does more than just providing the text layout. It provides the text, text layouts, phantom text, and whether it has multiline phantom text. It is more of an outside state.
/// The [`TextLayoutProvider`] serves two primary roles:
/// - Providing the [`Rope`] text of the underlying file
/// - Constructing the text layout for a given line
///   
/// Note: `text` does not necessarily include every piece of text. The obvious example is phantom
/// text, which is not in the underlying buffer.  
///  
/// Using this trait rather than passing around something like [`Document`] allows the backend to
/// be swapped out if needed. This would be useful if we ever wanted to reuse it across different
/// views that did not naturally fit into our 'document' model. As well as when we want to extract
/// the editor view code int a separate crate for Floem.
pub trait TextLayoutProvider {
    fn text(&self) -> &Rope;

    /// Shorthand for getting a rope text version of `text`.  
    /// This MUST hold the same rope that `text` would return.
    fn rope_text(&self) -> RopeTextRef {
        RopeTextRef::new(self.text())
    }

    // TODO(minor): Do we really need to pass font size to this? The outer-api is providing line
    // font size provider already, so it should be able to just use that.
    fn new_text_layout(
        &self,
        line: usize,
        font_size: usize,
        wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine>;

    /// Translate a column position into the postiion it would be before combining with the phantom
    /// text
    fn before_phantom_col(&self, line: usize, col: usize) -> usize;

    /// Whether the text has *any* multiline phantom text.  
    /// This is used to determine whether we can use the fast route where the lines are linear,
    /// which also requires no wrapping.  
    /// This should be a conservative estimate, so if you aren't bothering to check all of your
    /// phantom text then just return true.
    fn has_multiline_phantom(&self) -> bool;
}
impl<T: TextLayoutProvider> TextLayoutProvider for &T {
    fn text(&self) -> &Rope {
        (**self).text()
    }

    fn new_text_layout(
        &self,
        line: usize,
        font_size: usize,
        wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine> {
        (**self).new_text_layout(line, font_size, wrap)
    }

    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        (**self).before_phantom_col(line, col)
    }

    fn has_multiline_phantom(&self) -> bool {
        (**self).has_multiline_phantom()
    }
}

pub type FontSizeCacheId = u64;
pub trait LineFontSizeProvider {
    /// Get the 'general' font size for a specific buffer line.  
    /// This is typically the editor font size.  
    /// There might be alternate font-sizes within the line, like for phantom text, but those are
    /// not considered here.
    fn font_size(&self, line: usize) -> usize;

    /// An identifier used to mark when the font size info has changed.  
    /// This lets us update information.
    fn cache_id(&self) -> FontSizeCacheId;
}

/// Layout events. This is primarily needed for logic which tracks visual lines intelligently, like
/// `ScreenLines` in Lapce.  
/// This is currently limited to only a `CreatedLayout` event, as changed to the cache rev would
/// capture the idea of all the layouts being cleared. In the future it could be expanded to more
/// events, especially if cache rev gets more specific than clearing everything.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutEvent {
    CreatedLayout { font_size: usize, line: usize },
}

/// The main structure for tracking visual line information.  
pub struct Lines {
    /// This is inside out from the usual way of writing Arc-RefCells due to sometimes wanting to
    /// swap out font sizes, while also grabbing an `Arc` to hold.  
    /// An `Arc<RefCell<_>>` has the issue that with a `dyn` it can't know they're the same size
    /// if you were to assign. So this allows us to swap out the `Arc`, though it does mean that
    /// the other holders of the `Arc` don't get the new version. That is fine currently.
    pub font_sizes: RefCell<Arc<dyn LineFontSizeProvider>>,
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    wrap: Cell<ResolvedWrap>,
    font_size_cache_id: Cell<FontSizeCacheId>,
    last_vline: Rc<Cell<Option<VLine>>>,
    pub layout_event: Listener<LayoutEvent>,
}
impl Lines {
    pub fn new(
        cx: Scope,
        font_sizes: RefCell<Arc<dyn LineFontSizeProvider>>,
    ) -> Lines {
        let id = font_sizes.borrow().cache_id();
        Lines {
            font_sizes,
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::default())),
            wrap: Cell::new(ResolvedWrap::None),
            font_size_cache_id: Cell::new(id),
            last_vline: Rc::new(Cell::new(None)),
            layout_event: Listener::new_empty(cx),
        }
    }

    /// The current wrapping style
    pub fn wrap(&self) -> ResolvedWrap {
        self.wrap.get()
    }

    /// Set the wrapping style  
    /// Does nothing if the wrapping style is the same as the current one.  
    /// Will trigger a clear of the text layouts if the wrapping style is different.
    pub fn set_wrap(&self, wrap: ResolvedWrap) {
        if wrap == self.wrap.get() {
            return;
        }

        // TODO(perf): We could improve this by only clearing the lines that would actually change
        // Ex: Single vline lines don't need to be cleared if the wrapping changes from
        // some width to None, or from some width to some larger width.
        self.clear_unchanged();

        self.wrap.set(wrap);
    }

    /// The max width of the text layouts displayed
    pub fn max_width(&self) -> f64 {
        self.text_layouts.borrow().max_width
    }

    /// Check if the lines can be modelled as a purely linear file.  
    /// If `true` this makes various operations simpler because there is a one-to-one
    /// correspondence between visual lines and buffer lines.  
    /// However, if there is wrapping or any multiline phantom text, then we can't rely on that.  
    ///   
    /// TODO:?
    /// We could be smarter about various pieces.  
    /// - If there was no lines that exceeded the wrap width then we could do the fast path
    ///    - Would require tracking that but might not be too hard to do it whenever we create a
    ///      text layout
    /// - `is_linear` could be up to some line, which allows us to make at least the earliest parts
    ///    before any wrapping were faster. However, early lines are faster to calculate anyways.
    pub fn is_linear(&self, text_prov: impl TextLayoutProvider) -> bool {
        self.wrap.get() == ResolvedWrap::None && !text_prov.has_multiline_phantom()
    }

    /// Get the font size that [`Self::font_sizes`] provides
    pub fn font_size(&self, line: usize) -> usize {
        self.font_sizes.borrow().font_size(line)
    }

    /// Get the last visual line of the file.  
    /// Cached.
    pub fn last_vline(&self, text_prov: impl TextLayoutProvider) -> VLine {
        let current_id = self.font_sizes.borrow().cache_id();
        if current_id != self.font_size_cache_id.get() {
            self.last_vline.set(None);
            self.font_size_cache_id.set(current_id);
        }

        if let Some(last_vline) = self.last_vline.get() {
            last_vline
        } else {
            // For most files this should easily be fast enough.
            // Though it could still be improved.
            let rope_text = text_prov.rope_text();
            let hard_line_count = rope_text.num_lines();

            let line_count = if self.is_linear(text_prov) {
                hard_line_count
            } else {
                let mut soft_line_count = 0;

                let layouts = self.text_layouts.borrow();
                for i in 0..hard_line_count {
                    let font_size = self.font_size(i);
                    if let Some(text_layout) = layouts.get(font_size, i) {
                        let line_count = text_layout.line_count();
                        soft_line_count += line_count;
                    } else {
                        soft_line_count += 1;
                    }
                }

                soft_line_count
            };

            let last_vline = line_count.saturating_sub(1);
            self.last_vline.set(Some(VLine(last_vline)));
            VLine(last_vline)
        }
    }

    /// Clear the cache for the last vline
    pub fn clear_last_vline(&self) {
        self.last_vline.set(None);
    }

    /// The last relative visual line.  
    /// Cheap, so not cached
    pub fn last_rvline(&self, text_prov: impl TextLayoutProvider) -> RVLine {
        let rope_text = text_prov.rope_text();
        let last_line = rope_text.last_line();
        let layouts = self.text_layouts.borrow();
        let font_size = self.font_size(last_line);

        if let Some(layout) = layouts.get(font_size, last_line) {
            let line_count = layout.line_count();

            RVLine::new(last_line, line_count - 1)
        } else {
            RVLine::new(last_line, 0)
        }
    }

    /// 'len' version of [`Lines::last_vline`]  
    /// Cached.
    pub fn num_vlines(&self, text_prov: impl TextLayoutProvider) -> usize {
        self.last_vline(text_prov).get() + 1
    }

    /// Get the text layout for the given buffer line number.
    /// This will create the text layout if it doesn't exist.  
    ///   
    /// `trigger` (default to true) decides whether the creation of the text layout should trigger
    /// the [`LayoutEvent::CreatedLayout`] event.  
    ///   
    /// This will check the `config_id`, which decides whether it should clear out the text layout
    /// cache.
    pub fn get_init_text_layout(
        &self,
        config_id: u64,
        text_prov: impl TextLayoutProvider,
        line: usize,
        trigger: bool,
    ) -> Arc<TextLayoutLine> {
        self.check_config_id(config_id);

        let font_size = self.font_size(line);
        get_init_text_layout(
            &self.text_layouts,
            trigger.then_some(self.layout_event),
            text_prov,
            line,
            font_size,
            self.wrap.get(),
            &self.last_vline,
        )
    }

    /// Try to get the text layout for the given line number.  
    ///   
    /// This will check the `config_id`, which decides whether it should clear out the text layout
    /// cache.
    pub fn try_get_text_layout(
        &self,
        config_id: u64,
        line: usize,
    ) -> Option<Arc<TextLayoutLine>> {
        self.check_config_id(config_id);

        let font_size = self.font_size(line);

        self.text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .and_then(|f| f.get(&line))
            .cloned()
    }

    /// Initialize the text layout of every line in the real line interval.  
    ///   
    /// `trigger` (default to true) decides whether the creation of the text layout should trigger
    /// the [`LayoutEvent::CreatedLayout`] event.
    pub fn init_line_interval(
        &self,
        config_id: u64,
        text_prov: &impl TextLayoutProvider,
        lines: impl Iterator<Item = usize>,
        trigger: bool,
    ) {
        for line in lines {
            self.get_init_text_layout(config_id, text_prov, line, trigger);
        }
    }

    /// Initialize the text layout of every line in the file.  
    /// This should typically not be used.  
    ///   
    /// `trigger` (default to true) decides whether the creation of the text layout should trigger
    /// the [`LayoutEvent::CreatedLayout`] event.
    pub fn init_all(
        &self,
        config_id: u64,
        text_prov: &impl TextLayoutProvider,
        trigger: bool,
    ) {
        let text = text_prov.text();
        let last_line = text.line_of_offset(text.len());
        self.init_line_interval(config_id, text_prov, 0..=last_line, trigger);
    }

    /// Iterator over [`VLineInfo`]s, starting at `start_line`.  
    pub fn iter_vlines(
        &self,
        text_prov: impl TextLayoutProvider,
        backwards: bool,
        start: VLine,
    ) -> impl Iterator<Item = VLineInfo> {
        VisualLines::new(self, text_prov, backwards, start)
    }

    /// Iterator over [`VLineInfo`]s, starting at `start_line` and ending at `end_line`.  
    /// `start_line..end_line`
    pub fn iter_vlines_over(
        &self,
        text_prov: impl TextLayoutProvider,
        backwards: bool,
        start: VLine,
        end: VLine,
    ) -> impl Iterator<Item = VLineInfo> {
        self.iter_vlines(text_prov, backwards, start)
            .take_while(move |info| info.vline < end)
    }

    /// Iterator over *relative* [`VLineInfo`]s, starting at the rvline, `start_line`.  
    /// This is preferable over `iter_vlines` if you do not need to absolute visual line value and
    /// can provide the buffer line.
    pub fn iter_rvlines(
        &self,
        text_prov: impl TextLayoutProvider,
        backwards: bool,
        start: RVLine,
    ) -> impl Iterator<Item = VLineInfo<()>> {
        VisualLinesRelative::new(self, text_prov, backwards, start)
    }

    /// Iterator over *relative* [`VLineInfo`]s, starting at the rvline `start_line` and
    /// ending at the buffer line `end_line`.  
    /// `start_line..end_line`  
    /// This is preferable over `iter_vlines` if you do not need the absolute visual line value and
    /// you can provide the buffer line.  
    pub fn iter_rvlines_over(
        &self,
        text_prov: impl TextLayoutProvider,
        backwards: bool,
        start: RVLine,
        end_line: usize,
    ) -> impl Iterator<Item = VLineInfo<()>> {
        self.iter_rvlines(text_prov, backwards, start)
            .take_while(move |info| info.rvline.line < end_line)
    }

    // TODO(minor): Get rid of the clone bound.
    /// Initialize the text layouts as you iterate over them.  
    pub fn iter_vlines_init(
        &self,
        text_prov: impl TextLayoutProvider + Clone,
        config_id: u64,
        start: VLine,
        trigger: bool,
    ) -> impl Iterator<Item = VLineInfo> {
        self.check_config_id(config_id);

        if start <= self.last_vline(&text_prov) {
            // We initialize the text layout for the line that start line is for
            let (_, rvline) = find_vline_init_info(self, &text_prov, start).unwrap();
            self.get_init_text_layout(config_id, &text_prov, rvline.line, trigger);
            // If the start line was past the last vline then we don't need to initialize anything
            // since it won't get anything.
        }

        let text_layouts = self.text_layouts.clone();
        let font_sizes = self.font_sizes.clone();
        let wrap = self.wrap.get();
        let last_vline = self.last_vline.clone();
        let layout_event = trigger.then_some(self.layout_event);
        self.iter_vlines(text_prov.clone(), false, start)
            .map(move |v| {
                if v.is_first() {
                    // For every (first) vline we initialize the next buffer line's text layout
                    // This ensures it is ready for when re reach it.
                    let next_line = v.rvline.line + 1;
                    let font_size = font_sizes.borrow().font_size(next_line);
                    // `init_iter_vlines` is the reason `get_init_text_layout` is split out.
                    // Being split out lets us avoid attaching lifetimes to the iterator, since it
                    // only uses Rc/Arcs it is given.
                    // This is useful since `Lines` would be in a
                    // `Rc<RefCell<_>>` which would make iterators with lifetimes referring to
                    // `Lines` a pain.
                    get_init_text_layout(
                        &text_layouts,
                        layout_event,
                        &text_prov,
                        next_line,
                        font_size,
                        wrap,
                        &last_vline,
                    );
                }
                v
            })
    }

    /// Iterator over [`VLineInfo`]s, starting at `start_line` and ending at `end_line`.
    /// `start_line..end_line`  
    /// Initializes the text layouts as you iterate over them.
    ///
    /// `trigger` (default to true) decides whether the creation of the text layout should trigger
    /// the [`LayoutEvent::CreatedLayout`] event.
    pub fn iter_vlines_init_over(
        &self,
        text_prov: impl TextLayoutProvider + Clone,
        config_id: u64,
        start: VLine,
        end: VLine,
        trigger: bool,
    ) -> impl Iterator<Item = VLineInfo> {
        self.iter_vlines_init(text_prov, config_id, start, trigger)
            .take_while(move |info| info.vline < end)
    }

    /// Iterator over *relative* [`VLineInfo`]s, starting at the rvline, `start_line` and
    /// ending at the buffer line `end_line`.
    /// `start_line..end_line`
    ///
    /// `trigger` (default to true) decides whether the creation of the text layout should trigger
    /// the [`LayoutEvent::CreatedLayout`] event.
    pub fn iter_rvlines_init(
        &self,
        text_prov: impl TextLayoutProvider + Clone,
        config_id: u64,
        start: RVLine,
        trigger: bool,
    ) -> impl Iterator<Item = VLineInfo<()>> {
        self.check_config_id(config_id);

        if start.line <= text_prov.rope_text().last_line() {
            // Initialize the text layout for the line that start line is for
            self.get_init_text_layout(config_id, &text_prov, start.line, trigger);
        }

        let text_layouts = self.text_layouts.clone();
        let font_sizes = self.font_sizes.clone();
        let wrap = self.wrap.get();
        let last_vline = self.last_vline.clone();
        let layout_event = trigger.then_some(self.layout_event);
        self.iter_rvlines(text_prov.clone(), false, start)
            .map(move |v| {
                if v.is_first() {
                    // For every (first) vline we initialize the next buffer line's text layout
                    // This ensures it is ready for when re reach it.
                    let next_line = v.rvline.line + 1;
                    let font_size = font_sizes.borrow().font_size(next_line);
                    // `init_iter_lines` is the reason `get_init_text_layout` is split out.
                    // Being split out lets us avoid attaching lifetimes to the iterator, since it
                    // only uses Rc/Arcs that it. This is useful since `Lines` would be in a
                    // `Rc<RefCell<_>>` which would make iterators with lifetimes referring to
                    // `Lines` a pain.
                    get_init_text_layout(
                        &text_layouts,
                        layout_event,
                        &text_prov,
                        next_line,
                        font_size,
                        wrap,
                        &last_vline,
                    );
                }
                v
            })
    }

    /// Get the visual line of the offset.  
    ///   
    /// `affinity` decides whether an offset at a soft line break is considered to be on the
    /// previous line or the next line.  
    /// If `affinity` is `CursorAffinity::Forward` and is at the very end of the wrapped line, then
    /// the offset is considered to be on the next vline.
    pub fn vline_of_offset(
        &self,
        text_prov: &impl TextLayoutProvider,
        offset: usize,
        affinity: CursorAffinity,
    ) -> VLine {
        let text = text_prov.text();

        let offset = offset.min(text.len());

        if self.is_linear(text_prov) {
            let buffer_line = text.line_of_offset(offset);
            return VLine(buffer_line);
        }

        let Some((vline, _line_index)) =
            find_vline_of_offset(self, text_prov, offset, affinity)
        else {
            // We assume it is out of bounds
            return self.last_vline(text_prov);
        };

        vline
    }

    /// Get the visual line and column of the given offset.  
    ///   
    /// The column is before phantom text is applied and is into the overall line, not the
    /// individual visual line.
    pub fn vline_col_of_offset(
        &self,
        text_prov: &impl TextLayoutProvider,
        offset: usize,
        affinity: CursorAffinity,
    ) -> (VLine, usize) {
        let vline = self.vline_of_offset(text_prov, offset, affinity);
        let last_col = self
            .iter_vlines(text_prov, false, vline)
            .next()
            .map(|info| info.last_col(text_prov, true))
            .unwrap_or(0);

        let line = text_prov.text().line_of_offset(offset);
        let line_offset = text_prov.text().offset_of_line(line);

        let col = offset - line_offset;
        let col = col.min(last_col);

        (vline, col)
    }

    /// Get the nearest offset to the start of the visual line
    pub fn offset_of_vline(
        &self,
        text_prov: &impl TextLayoutProvider,
        vline: VLine,
    ) -> usize {
        find_vline_init_info(self, text_prov, vline)
            .map(|x| x.0)
            .unwrap_or_else(|| text_prov.text().len())
    }

    /// Get the first visual line of the buffer line.
    pub fn vline_of_line(
        &self,
        text_prov: &impl TextLayoutProvider,
        line: usize,
    ) -> VLine {
        if self.is_linear(text_prov) {
            return VLine(line);
        }

        find_vline_of_line(self, text_prov, line)
            .unwrap_or_else(|| self.last_vline(text_prov))
    }

    /// Find the matching visual line for the given relative visual line.
    pub fn vline_of_rvline(
        &self,
        text_prov: &impl TextLayoutProvider,
        rvline: RVLine,
    ) -> VLine {
        if self.is_linear(text_prov) {
            debug_assert_eq!(rvline.line_index, 0, "Got a nonzero line index despite being linear, old RVLine was used.");
            return VLine(rvline.line);
        }

        let vline = self.vline_of_line(text_prov, rvline.line);

        // TODO(minor): There may be edge cases with this, like when you have a bunch of multiline
        // phantom text at the same offset
        VLine(vline.get() + rvline.line_index)
    }

    /// Get the relative visual line of the offset.
    ///  
    /// `affinity` decides whether an offset at a soft line break is considered to be on the
    /// previous line or the next line.
    /// If `affinity` is `CursorAffinity::Forward` and is at the very end of the wrapped line, then
    /// the offset is considered to be on the next rvline.
    pub fn rvline_of_offset(
        &self,
        text_prov: &impl TextLayoutProvider,
        offset: usize,
        affinity: CursorAffinity,
    ) -> RVLine {
        let text = text_prov.text();

        let offset = offset.min(text.len());

        if self.is_linear(text_prov) {
            let buffer_line = text.line_of_offset(offset);
            return RVLine::new(buffer_line, 0);
        }

        find_rvline_of_offset(self, text_prov, offset, affinity)
            .unwrap_or_else(|| self.last_rvline(text_prov))
    }

    /// Get the relative visual line and column of the given offset  
    ///   
    /// The column is before phantom text is applied and is into the overall line, not the
    /// individual visual line.
    pub fn rvline_col_of_offset(
        &self,
        text_prov: &impl TextLayoutProvider,
        offset: usize,
        affinity: CursorAffinity,
    ) -> (RVLine, usize) {
        let rvline = self.rvline_of_offset(text_prov, offset, affinity);
        let info = self.iter_rvlines(text_prov, false, rvline).next().unwrap();
        let line_offset = text_prov.text().offset_of_line(rvline.line);

        let col = offset - line_offset;
        let col = col.min(info.last_col(text_prov, true));

        (rvline, col)
    }

    /// Get the offset of a relative visual line
    pub fn offset_of_rvline(
        &self,
        text_prov: &impl TextLayoutProvider,
        RVLine { line, line_index }: RVLine,
    ) -> usize {
        let rope_text = text_prov.rope_text();
        let font_size = self.font_size(line);
        let layouts = self.text_layouts.borrow();

        let base_offset = rope_text.offset_of_line(line);

        // We could remove the debug asserts and allow invalid line indices. However I think it is
        // desirable to avoid those since they are probably indicative of bugs.
        if let Some(text_layout) = layouts.get(font_size, line) {
            debug_assert!(
                line_index < text_layout.line_count(),
                "Line index was out of bounds. This likely indicates keeping an rvline past when it was valid."
            );

            let line_index = line_index.min(text_layout.line_count() - 1);

            let col = text_layout
                .start_layout_cols(text_prov, line)
                .nth(line_index)
                .unwrap_or(0);
            let col = text_prov.before_phantom_col(line, col);

            base_offset + col
        } else {
            // There was no text layout for this line, so we treat it like if line index is zero
            // even if it is not.

            debug_assert_eq!(line_index, 0, "Line index was zero. This likely indicates keeping an rvline past when it was valid.");

            base_offset
        }
    }

    /// Get the relative visual line of the buffer line
    pub fn rvline_of_line(
        &self,
        text_prov: &impl TextLayoutProvider,
        line: usize,
    ) -> RVLine {
        if self.is_linear(text_prov) {
            return RVLine::new(line, 0);
        }

        let offset = text_prov.rope_text().offset_of_line(line);

        find_rvline_of_offset(self, text_prov, offset, CursorAffinity::Backward)
            .unwrap_or_else(|| self.last_rvline(text_prov))
    }

    /// Check whether the config id has changed, clearing the cache if it has.
    pub fn check_config_id(&self, config_id: u64) {
        // Check if the text layout needs to update due to the config being changed
        if config_id != self.text_layouts.borrow().config_id {
            let cache_rev = self.text_layouts.borrow().cache_rev + 1;
            self.clear(cache_rev, Some(config_id));
        }
    }

    /// Check whether the text layout cache revision is different.  
    /// Clears the layouts and updates the cache rev if it was different.
    pub fn check_cache_rev(&self, cache_rev: u64) {
        if cache_rev != self.text_layouts.borrow().cache_rev {
            self.clear(cache_rev, None);
        }
    }

    /// Clear the text layouts with a given cache revision
    pub fn clear(&self, cache_rev: u64, config_id: Option<u64>) {
        self.text_layouts.borrow_mut().clear(cache_rev, config_id);
        self.last_vline.set(None);
    }

    /// Clear the layouts and vline without changing the cache rev or config id.
    pub fn clear_unchanged(&self) {
        self.text_layouts.borrow_mut().clear_unchanged();
        self.last_vline.set(None);
    }
}

/// This is a separate function as a hacky solution to lifetimes.  
/// While it being on `Lines` makes the most sense, it being separate lets us only have
/// `text_layouts` and `wrap` from the original to then initialize a text layout. This simplifies
/// lifetime issues in some functions, since they can just have an `Arc`/`Rc`.  
///   
/// Note: This does not clear the cache or check via config id. That should be done outside this
/// as `Lines` does require knowing when the cache is invalidated.
fn get_init_text_layout(
    text_layouts: &RefCell<TextLayoutCache>,
    layout_event: Option<Listener<LayoutEvent>>,
    text_prov: impl TextLayoutProvider,
    line: usize,
    font_size: usize,
    wrap: ResolvedWrap,
    last_vline: &Cell<Option<VLine>>,
) -> Arc<TextLayoutLine> {
    // If we don't have a second layer of the hashmap initialized for this specific font size,
    // do it now
    if text_layouts.borrow().layouts.get(&font_size).is_none() {
        let mut cache = text_layouts.borrow_mut();
        cache.layouts.insert(font_size, HashMap::new());
    }

    // Get whether there's an entry for this specific font size and line
    let cache_exists = text_layouts
        .borrow()
        .layouts
        .get(&font_size)
        .unwrap()
        .get(&line)
        .is_some();
    // If there isn't an entry then we actually have to create it
    if !cache_exists {
        let text_layout = text_prov.new_text_layout(line, font_size, wrap);

        // Update last vline
        if let Some(vline) = last_vline.get() {
            let last_line = text_prov.rope_text().last_line();
            if line <= last_line {
                // We can get rid of the old line count and add our new count.
                // This lets us typically avoid having to calculate the last visual line.
                let vline = vline.get();
                let new_vline = vline + (text_layout.line_count() - 1);

                last_vline.set(Some(VLine(new_vline)));
            }
            // If the line is past the end of the file, then we don't need to update the last
            // visual line. It is garbage.
        }
        // Otherwise last vline was already None.

        {
            // Add the text layout to the cache.
            let mut cache = text_layouts.borrow_mut();
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

        if let Some(layout_event) = layout_event {
            layout_event.send(LayoutEvent::CreatedLayout { font_size, line });
        }
    }

    // Just get the entry, assuming it has been created because we initialize it above.
    text_layouts
        .borrow()
        .layouts
        .get(&font_size)
        .unwrap()
        .get(&line)
        .cloned()
        .unwrap()
}

/// Returns (visual line, line_index)  
fn find_vline_of_offset(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    offset: usize,
    affinity: CursorAffinity,
) -> Option<(VLine, usize)> {
    let layouts = lines.text_layouts.borrow();

    let rope_text = text_prov.rope_text();

    let buffer_line = rope_text.line_of_offset(offset);
    let line_start_offset = rope_text.offset_of_line(buffer_line);
    let vline = find_vline_of_line(lines, text_prov, buffer_line)?;

    let font_size = lines.font_size(buffer_line);
    let Some(text_layout) = layouts.get(font_size, buffer_line) else {
        // No text layout for this line, so the vline we found is definitely correct.
        // As well, there is no previous soft line to consider
        return Some((vline, 0));
    };

    let col = offset - line_start_offset;

    let (vline, line_index) =
        find_start_line_index(text_prov, text_layout, buffer_line, col)
            .map(|line_index| (VLine(vline.get() + line_index), line_index))?;

    // If the most recent line break was due to a soft line break,
    if line_index > 0 {
        if let CursorAffinity::Backward = affinity {
            // TODO: This can definitely be smarter. We're doing a vline search, and then this is
            // practically doing another!
            let line_end = lines.offset_of_vline(text_prov, vline);
            // then if we're right at that soft line break, a backwards affinity
            // means that we are on the previous visual line.
            if line_end == offset && vline.get() != 0 {
                return Some((VLine(vline.get() - 1), line_index - 1));
            }
        }
    }

    Some((vline, line_index))
}

fn find_rvline_of_offset(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    offset: usize,
    affinity: CursorAffinity,
) -> Option<RVLine> {
    let layouts = lines.text_layouts.borrow();

    let rope_text = text_prov.rope_text();

    let buffer_line = rope_text.line_of_offset(offset);
    let line_start_offset = rope_text.offset_of_line(buffer_line);

    let font_size = lines.font_size(buffer_line);
    let Some(text_layout) = layouts.get(font_size, buffer_line) else {
        // There is no text layout for this line so the line index is always zero.
        return Some(RVLine::new(buffer_line, 0));
    };

    let col = offset - line_start_offset;

    let rv = find_start_line_index(text_prov, text_layout, buffer_line, col)
        .map(|line_index| RVLine::new(buffer_line, line_index))?;

    // If the most recent line break was due to a soft line break,
    if rv.line_index > 0 {
        if let CursorAffinity::Backward = affinity {
            let line_end = lines.offset_of_rvline(text_prov, rv);
            // then if we're right at that soft line break, a backwards affinity
            // means that we are on the previous visual line.
            if line_end == offset {
                if rv.line_index > 0 {
                    return Some(RVLine::new(rv.line, rv.line_index - 1));
                } else if rv.line == 0 {
                    // There is no previous line, we do nothing.
                } else {
                    // We have to get rvline info for that rvline, so we can get the last line index
                    // This should aways have at least one rvline in it.
                    let font_sizes = lines.font_sizes.borrow();
                    let (prev, _) =
                        prev_rvline(&layouts, text_prov, &**font_sizes, rv)?;
                    return Some(prev);
                }
            }
        }
    }

    Some(rv)
}

// TODO: a lot of these just take lines, so should possibly just be put on it.

/// Find the line index which contains the column.
fn find_start_line_index(
    text_prov: &impl TextLayoutProvider,
    text_layout: &TextLayoutLine,
    line: usize,
    col: usize,
) -> Option<usize> {
    let mut starts = text_layout
        .layout_cols(text_prov, line)
        .enumerate()
        .peekable();

    while let Some((i, (layout_start, _))) = starts.next() {
        // TODO: we should just apply after_col to col to do this transformation once
        let layout_start = text_prov.before_phantom_col(line, layout_start);
        if layout_start >= col {
            return Some(i);
        }

        let next_start = starts.peek().map(|(_, (next_start, _))| {
            text_prov.before_phantom_col(line, *next_start)
        });

        if let Some(next_start) = next_start {
            if next_start > col {
                // The next layout starts *past* our column, so we're on the previous line.
                return Some(i);
            }
        } else {
            // There was no next glyph, which implies that we are either on this line or not at all
            return Some(i);
        }
    }

    None
}

/// Get the first visual line of a buffer line.
fn find_vline_of_line(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    line: usize,
) -> Option<VLine> {
    let rope = text_prov.rope_text();

    let last_line = rope.last_line();

    if line > last_line / 2 {
        // Often the last vline will already be cached, which lets us half the search time.
        // The compiler may or may not be smart enough to combine the last vline calculation with
        // our calculation of the vline of the line we're looking for, but it might not.
        // If it doesn't, we could write a custom version easily.
        let last_vline = lines.last_vline(text_prov);
        let last_rvline = lines.last_rvline(text_prov);
        let last_start_vline = VLine(last_vline.get() - last_rvline.line_index);
        find_vline_of_line_backwards(lines, (last_start_vline, last_line), line)
    } else {
        find_vline_of_line_forwards(lines, (VLine(0), 0), line)
    }
}

/// Get the first visual line of a buffer line.  
/// This searches backwards from `pivot`, so it should be *after* the given line.  
/// This requires that the `pivot` is the first line index of the line it is for.
fn find_vline_of_line_backwards(
    lines: &Lines,
    (start, s_line): (VLine, usize),
    line: usize,
) -> Option<VLine> {
    if line > s_line {
        return None;
    } else if line == s_line {
        return Some(start);
    } else if line == 0 {
        return Some(VLine(0));
    }

    let layouts = lines.text_layouts.borrow();

    let mut cur_vline = start.get();

    for cur_line in line..s_line {
        let font_size = lines.font_size(cur_line);

        let Some(text_layout) = layouts.get(font_size, cur_line) else {
            // no text layout, so its just a normal line
            cur_vline -= 1;
            continue;
        };

        let line_count = text_layout.line_count();

        cur_vline -= line_count;
    }

    Some(VLine(cur_vline))
}

fn find_vline_of_line_forwards(
    lines: &Lines,
    (start, s_line): (VLine, usize),
    line: usize,
) -> Option<VLine> {
    match line.cmp(&s_line) {
        Ordering::Equal => return Some(start),
        Ordering::Less => return None,
        Ordering::Greater => (),
    }

    let layouts = lines.text_layouts.borrow();

    let mut cur_vline = start.get();

    for cur_line in s_line..line {
        let font_size = lines.font_size(cur_line);

        let Some(text_layout) = layouts.get(font_size, cur_line) else {
            // no text layout, so its just a normal line
            cur_vline += 1;
            continue;
        };

        let line_count = text_layout.line_count();
        cur_vline += line_count;
    }

    Some(VLine(cur_vline))
}

/// Find the (start offset, buffer line, layout line index) of a given visual line.  
///   
/// start offset is into the file, rather than the text layouts string, so it does not include
/// phantom text.
///
/// Returns `None` if the visual line is out of bounds.
fn find_vline_init_info(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    vline: VLine,
) -> Option<(usize, RVLine)> {
    let rope_text = text_prov.rope_text();

    if vline.get() == 0 {
        return Some((0, RVLine::new(0, 0)));
    }

    if lines.is_linear(text_prov) {
        // If lines is linear then we can trivially convert the visual line to a buffer line
        let line = vline.get();
        if line > rope_text.last_line() {
            return None;
        }

        return Some((rope_text.offset_of_line(line), RVLine::new(line, 0)));
    }

    let last_vline = lines.last_vline(text_prov);

    if vline > last_vline {
        return None;
    }

    if vline.get() < last_vline.get() / 2 {
        let last_rvline = lines.last_rvline(text_prov);
        find_vline_init_info_rv_backward(
            lines,
            text_prov,
            (last_vline, last_rvline),
            vline,
        )
    } else {
        find_vline_init_info_forward(lines, text_prov, (VLine(0), 0), vline)
    }
}

// TODO(minor): should we package (VLine, buffer line) into a struct since we use it for these
// pseudo relative calculations often?
/// Find the `(start offset, rvline)` of a given [`VLine`]  
///   
/// start offset is into the file, rather than text layout's string, so it does not include
/// phantom text.  
///
/// Returns `None` if the visual line is out of bounds, or if the start is past our target.
fn find_vline_init_info_forward(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    (start, start_line): (VLine, usize),
    vline: VLine,
) -> Option<(usize, RVLine)> {
    if start > vline {
        return None;
    }

    let rope_text = text_prov.rope_text();

    let mut cur_line = start_line;
    let mut cur_vline = start.get();

    let layouts = lines.text_layouts.borrow();
    while cur_vline < vline.get() {
        let font_size = lines.font_size(cur_line);
        let line_count = if let Some(text_layout) = layouts.get(font_size, cur_line)
        {
            let line_count = text_layout.line_count();

            // We can then check if the visual line is in this intervening range.
            if cur_vline + line_count > vline.get() {
                // We found the line that contains the visual line.
                // We can now find the offset of the visual line within the line.
                let line_index = vline.get() - cur_vline;
                // TODO: is it fine to unwrap here?
                let col = text_layout
                    .start_layout_cols(text_prov, cur_line)
                    .nth(line_index)
                    .unwrap_or(0);
                let col = text_prov.before_phantom_col(cur_line, col);

                let base_offset = rope_text.offset_of_line(cur_line);
                return Some((base_offset + col, RVLine::new(cur_line, line_index)));
            }

            // The visual line is not in this line, so we have to keep looking.
            line_count
        } else {
            // There was no text layout so we only have to consider the line breaks in this line.
            // Which, since we don't handle phantom text, is just one.

            1
        };

        cur_line += 1;
        cur_vline += line_count;
    }

    // We've reached the visual line we're looking for, we can return the offset.
    // This also handles the case where the vline is past the end of the text.
    if cur_vline == vline.get() {
        if cur_line > rope_text.last_line() {
            return None;
        }

        // We use cur_line because if our target vline is out of bounds
        // then the result should be len
        Some((rope_text.offset_of_line(cur_line), RVLine::new(cur_line, 0)))
    } else {
        // We've gone past the visual line we're looking for, so it is out of bounds.
        None
    }
}

/// Find the `(start offset, rvline)` of a given [`VLine`]
///
/// `start offset` is into the file, rather than the text layout's content, so it does not
/// include phantom text.  
///
/// Returns `None` if the visual line is out of bounds or if the start is before our target.  
/// This iterates backwards.
fn find_vline_init_info_rv_backward(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    (start, start_rvline): (VLine, RVLine),
    vline: VLine,
) -> Option<(usize, RVLine)> {
    if start < vline {
        // The start was before the target.
        return None;
    }

    // This would the vline at the very start of the buffer line
    let shifted_start = VLine(start.get() - start_rvline.line_index);
    match shifted_start.cmp(&vline) {
        // The shifted start was equivalent to the vline, which makes it easy to compute
        Ordering::Equal => {
            let offset = text_prov.rope_text().offset_of_line(start_rvline.line);
            Some((offset, RVLine::new(start_rvline.line, 0)))
        }
        // The new start is before the vline, that means the vline is on the same line.
        Ordering::Less => {
            let line_index = vline.get() - shifted_start.get();
            let layouts = lines.text_layouts.borrow();
            let font_size = lines.font_size(start_rvline.line);
            if let Some(text_layout) = layouts.get(font_size, start_rvline.line) {
                vline_init_info_b(
                    text_prov,
                    text_layout,
                    RVLine::new(start_rvline.line, line_index),
                )
            } else {
                // There was no text layout so we only have to consider the line breaks in this line.

                let base_offset =
                    text_prov.rope_text().offset_of_line(start_rvline.line);
                Some((base_offset, RVLine::new(start_rvline.line, 0)))
            }
        }
        Ordering::Greater => find_vline_init_info_backward(
            lines,
            text_prov,
            (shifted_start, start_rvline.line),
            vline,
        ),
    }
}

fn find_vline_init_info_backward(
    lines: &Lines,
    text_prov: &impl TextLayoutProvider,
    (mut start, mut start_line): (VLine, usize),
    vline: VLine,
) -> Option<(usize, RVLine)> {
    loop {
        let (prev_vline, prev_line) = prev_line_start(lines, start, start_line)?;

        match prev_vline.cmp(&vline) {
            // We found the target, and it was at the start
            Ordering::Equal => {
                let offset = text_prov.rope_text().offset_of_line(prev_line);
                return Some((offset, RVLine::new(prev_line, 0)));
            }
            // The target is on this line, so we can just search for it
            Ordering::Less => {
                let font_size = lines.font_size(prev_line);
                let layouts = lines.text_layouts.borrow();
                if let Some(text_layout) = layouts.get(font_size, prev_line) {
                    return vline_init_info_b(
                        text_prov,
                        text_layout,
                        RVLine::new(prev_line, vline.get() - prev_vline.get()),
                    );
                } else {
                    // There was no text layout so we only have to consider the line breaks in this line.
                    // Which, since we don't handle phantom text, is just one.

                    let base_offset =
                        text_prov.rope_text().offset_of_line(prev_line);
                    return Some((base_offset, RVLine::new(prev_line, 0)));
                }
            }
            // The target is before this line, so we have to keep searching
            Ordering::Greater => {
                start = prev_vline;
                start_line = prev_line;
            }
        }
    }
}

/// Get the previous (line, start visual line) from a (line, start visual line).
fn prev_line_start(
    lines: &Lines,
    vline: VLine,
    line: usize,
) -> Option<(VLine, usize)> {
    if line == 0 {
        return None;
    }

    let layouts = lines.text_layouts.borrow();

    let prev_line = line - 1;
    let font_size = lines.font_size(line);
    if let Some(layout) = layouts.get(font_size, prev_line) {
        let line_count = layout.line_count();
        let prev_vline = vline.get() - line_count;
        Some((VLine(prev_vline), prev_line))
    } else {
        // There's no layout for the previous line which makes this easy
        Some((VLine(vline.get() - 1), prev_line))
    }
}

fn vline_init_info_b(
    text_prov: &impl TextLayoutProvider,
    text_layout: &TextLayoutLine,
    rv: RVLine,
) -> Option<(usize, RVLine)> {
    let rope_text = text_prov.rope_text();
    let col = text_layout
        .start_layout_cols(text_prov, rv.line)
        .nth(rv.line_index)
        .unwrap_or(0);
    let col = text_prov.before_phantom_col(rv.line, col);

    let base_offset = rope_text.offset_of_line(rv.line);

    Some((base_offset + col, rv))
}

/// Information about the visual line and how it relates to the underlying buffer line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct VLineInfo<L = VLine> {
    /// Start offset to end offset in the buffer that this visual line covers.  
    /// Note that this is obviously not including phantom text.  
    pub interval: Interval,
    /// The total number of lines in this buffer line. Always at least 1.
    pub line_count: usize,
    pub rvline: RVLine,
    /// The actual visual line this is for.  
    /// For relative visual line iteration, this is empty.  
    pub vline: L,
}
impl<L: std::fmt::Debug> VLineInfo<L> {
    fn new<I: Into<Interval>>(
        iv: I,
        rvline: RVLine,
        line_count: usize,
        vline: L,
    ) -> Self {
        Self {
            interval: iv.into(),
            line_count,
            rvline,
            vline,
        }
    }

    pub fn to_blank(&self) -> VLineInfo<()> {
        VLineInfo::new(self.interval, self.rvline, self.line_count, ())
    }

    /// Check whether the interval is empty.  
    /// Note that there could still be phantom text on this line.
    pub fn is_empty(&self) -> bool {
        self.interval.is_empty()
    }

    pub fn is_first(&self) -> bool {
        self.rvline.is_first()
    }

    // TODO: is this correct for phantom lines?
    // TODO: can't we just use the line count field now?
    /// Is this the last visual line for the relevant buffer line?
    pub fn is_last(&self, text_prov: &impl TextLayoutProvider) -> bool {
        let rope_text = text_prov.rope_text();
        let line_end = rope_text.line_end_offset(self.rvline.line, false);
        let vline_end = self.line_end_offset(text_prov, false);

        line_end == vline_end
    }

    /// Get the first column of the overall line of the visual line
    pub fn first_col(&self, text_prov: &impl TextLayoutProvider) -> usize {
        let line_start = self.interval.start;
        let start_offset = text_prov.text().offset_of_line(self.rvline.line);
        line_start - start_offset
    }

    /// Get the last column in the overall line of this visual line  
    /// The caret decides whether it is after the last character, or before it.  
    /// ```rust,ignore
    /// // line content = "conf = Config::default();\n"
    /// // wrapped breakup = ["conf = ", "Config::default();\n"]
    ///
    /// // when vline_info is for "conf = "
    /// assert_eq!(vline_info.last_col(text_prov, false), 6) // "conf =| "
    /// assert_eq!(vline_info.last_col(text_prov, true), 7) // "conf = |"
    /// // when vline_info is for "Config::default();\n"
    /// // Notice that the column is in the overall line, not the wrapped line.
    /// assert_eq!(vline_info.last_col(text_prov, false), 24) // "Config::default()|;"
    /// assert_eq!(vline_info.last_col(text_prov, true), 25) // "Config::default();|"
    /// ```
    pub fn last_col(
        &self,
        text_prov: &impl TextLayoutProvider,
        caret: bool,
    ) -> usize {
        let vline_end = self.interval.end;
        let start_offset = text_prov.text().offset_of_line(self.rvline.line);
        // If these subtractions crash, then it is likely due to a bad vline being kept around
        // somewhere
        if !caret && !self.interval.is_empty() {
            let vline_pre_end =
                text_prov.rope_text().prev_grapheme_offset(vline_end, 1, 0);
            vline_pre_end - start_offset
        } else {
            vline_end - start_offset
        }
    }

    // TODO: we could generalize `RopeText::line_end_offset` to any interval, and then just use it here instead of basically reimplementing it.
    pub fn line_end_offset(
        &self,
        text_prov: &impl TextLayoutProvider,
        caret: bool,
    ) -> usize {
        let text = text_prov.text();
        let rope_text = RopeTextRef::new(text);

        let mut offset = self.interval.end;
        let mut line_content: &str = &text.slice_to_cow(self.interval);
        if line_content.ends_with("\r\n") {
            offset -= 2;
            line_content = &line_content[..line_content.len() - 2];
        } else if line_content.ends_with('\n') {
            offset -= 1;
            line_content = &line_content[..line_content.len() - 1];
        }
        if !caret && !line_content.is_empty() {
            offset = rope_text.prev_grapheme_offset(offset, 1, 0);
        }
        offset
    }

    /// Returns the offset of the first non-blank character in the line.
    pub fn first_non_blank_character(
        &self,
        text_prov: &impl TextLayoutProvider,
    ) -> usize {
        WordCursor::new(text_prov.text(), self.interval.start).next_non_blank_char()
    }
}

/// Iterator of the visual lines in a [`Lines`].  
/// This only considers wrapped and phantom text lines that have been rendered into a text layout.  
///   
/// In principle, we could consider the newlines in phantom text for lines that have not been
/// rendered. However, that is more expensive to compute and is probably not actually *useful*.
struct VisualLines<T: TextLayoutProvider> {
    v: VisualLinesRelative<T>,
    vline: VLine,
}
impl<T: TextLayoutProvider> VisualLines<T> {
    pub fn new(
        lines: &Lines,
        text_prov: T,
        backwards: bool,
        start: VLine,
    ) -> VisualLines<T> {
        // TODO(minor): If we aren't using offset here then don't calculate it.
        let Some((_offset, rvline)) = find_vline_init_info(lines, &text_prov, start)
        else {
            return VisualLines::empty(lines, text_prov, backwards);
        };

        VisualLines {
            v: VisualLinesRelative::new(lines, text_prov, backwards, rvline),
            vline: start,
        }
    }

    pub fn empty(lines: &Lines, text_prov: T, backwards: bool) -> VisualLines<T> {
        VisualLines {
            v: VisualLinesRelative::empty(lines, text_prov, backwards),
            vline: VLine(0),
        }
    }
}
impl<T: TextLayoutProvider> Iterator for VisualLines<T> {
    type Item = VLineInfo;

    fn next(&mut self) -> Option<VLineInfo> {
        let was_first_iter = self.v.is_first_iter;
        let info = self.v.next()?;

        if !was_first_iter {
            if self.v.backwards {
                // This saturation isn't really needed, but just in case.
                debug_assert!(
                    self.vline.get() != 0,
                    "Expected VLine to always be nonzero if we were going backwards"
                );
                self.vline = VLine(self.vline.get().saturating_sub(1));
            } else {
                self.vline = VLine(self.vline.get() + 1);
            }
        }

        Some(VLineInfo {
            interval: info.interval,
            line_count: info.line_count,
            rvline: info.rvline,
            vline: self.vline,
        })
    }
}

/// Iterator of the visual lines in a [`Lines`] relative to some starting buffer line.  
/// This only considers wrapped and phantom text lines that have been rendered into a text layout.
struct VisualLinesRelative<T: TextLayoutProvider> {
    font_sizes: Arc<dyn LineFontSizeProvider>,
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    text_prov: T,

    is_done: bool,

    rvline: RVLine,
    /// Our current offset into the rope.
    offset: usize,

    /// Which direction we should move in.
    backwards: bool,
    /// Whether there is a one-to-one mapping between buffer lines and visual lines.
    linear: bool,

    is_first_iter: bool,
}
impl<T: TextLayoutProvider> VisualLinesRelative<T> {
    pub fn new(
        lines: &Lines,
        text_prov: T,
        backwards: bool,
        start: RVLine,
    ) -> VisualLinesRelative<T> {
        // Empty iterator if we're past the end of the possible lines
        if start > lines.last_rvline(&text_prov) {
            return VisualLinesRelative::empty(lines, text_prov, backwards);
        }

        let layouts = lines.text_layouts.borrow();
        let font_size = lines.font_size(start.line);
        let offset = rvline_offset(&layouts, &text_prov, font_size, start);

        let linear = lines.is_linear(&text_prov);

        VisualLinesRelative {
            font_sizes: lines.font_sizes.borrow().clone(),
            text_layouts: lines.text_layouts.clone(),
            text_prov,
            is_done: false,
            rvline: start,
            offset,
            backwards,
            linear,
            is_first_iter: true,
        }
    }

    pub fn empty(
        lines: &Lines,
        text_prov: T,
        backwards: bool,
    ) -> VisualLinesRelative<T> {
        VisualLinesRelative {
            font_sizes: lines.font_sizes.borrow().clone(),
            text_layouts: lines.text_layouts.clone(),
            text_prov,
            is_done: true,
            rvline: RVLine::new(0, 0),
            offset: 0,
            backwards,
            linear: true,
            is_first_iter: true,
        }
    }
}
impl<T: TextLayoutProvider> Iterator for VisualLinesRelative<T> {
    type Item = VLineInfo<()>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        let layouts = self.text_layouts.borrow();
        if self.is_first_iter {
            // This skips the next line call on the first line.
            self.is_first_iter = false;
        } else {
            let v = shift_rvline(
                &layouts,
                &self.text_prov,
                &*self.font_sizes,
                self.rvline,
                self.backwards,
                self.linear,
            );
            let Some((new_rel_vline, offset)) = v else {
                self.is_done = true;
                return None;
            };

            self.rvline = new_rel_vline;
            self.offset = offset;

            if self.rvline.line > self.text_prov.rope_text().last_line() {
                self.is_done = true;
                return None;
            }
        }

        let line = self.rvline.line;
        let line_index = self.rvline.line_index;
        let vline = self.rvline;

        let start = self.offset;

        let font_size = self.font_sizes.font_size(line);
        let end = end_of_rvline(&layouts, &self.text_prov, font_size, self.rvline);

        let line_count = if let Some(text_layout) = layouts.get(font_size, line) {
            text_layout.line_count()
        } else {
            1
        };
        debug_assert!(start <= end, "line: {line}, line_index: {line_index}, line_count: {line_count}, vline: {vline:?}, start: {start}, end: {end}, backwards: {} text_len: {}", self.backwards, self.text_prov.text().len());
        let info = VLineInfo::new(start..end, self.rvline, line_count, ());

        Some(info)
    }
}

// TODO: This might skip spaces at the end of lines, which we probably don't want?
/// Get the end offset of the visual line from the file's line and the line index.  
fn end_of_rvline(
    layouts: &TextLayoutCache,
    text_prov: &impl TextLayoutProvider,
    font_size: usize,
    RVLine { line, line_index }: RVLine,
) -> usize {
    if line > text_prov.rope_text().last_line() {
        return text_prov.text().len();
    }

    if let Some((_, end_col)) =
        layouts.get_layout_col(text_prov, font_size, line, line_index)
    {
        let end_col = text_prov.before_phantom_col(line, end_col);
        let base_offset = text_prov.text().offset_of_line(line);

        base_offset + end_col
    } else {
        let rope_text = text_prov.rope_text();

        rope_text.line_end_offset(line, true)
    }
}

/// Shift a relative visual line forward or backwards based on the `backwards` parameter.
fn shift_rvline(
    layouts: &TextLayoutCache,
    text_prov: &impl TextLayoutProvider,
    font_sizes: &dyn LineFontSizeProvider,
    vline: RVLine,
    backwards: bool,
    linear: bool,
) -> Option<(RVLine, usize)> {
    if linear {
        let rope_text = text_prov.rope_text();
        debug_assert_eq!(
            vline.line_index, 0,
            "Line index should be zero if we're linearly working with lines"
        );
        if backwards {
            if vline.line == 0 {
                return None;
            }

            let prev_line = vline.line - 1;
            let offset = rope_text.offset_of_line(prev_line);
            Some((RVLine::new(prev_line, 0), offset))
        } else {
            let next_line = vline.line + 1;

            if next_line > rope_text.last_line() {
                return None;
            }

            let offset = rope_text.offset_of_line(next_line);
            Some((RVLine::new(next_line, 0), offset))
        }
    } else if backwards {
        prev_rvline(layouts, text_prov, font_sizes, vline)
    } else {
        let font_size = font_sizes.font_size(vline.line);
        Some(next_rvline(layouts, text_prov, font_size, vline))
    }
}

fn rvline_offset(
    layouts: &TextLayoutCache,
    text_prov: &impl TextLayoutProvider,
    font_size: usize,
    RVLine { line, line_index }: RVLine,
) -> usize {
    let rope_text = text_prov.rope_text();
    if let Some((line_col, _)) =
        layouts.get_layout_col(text_prov, font_size, line, line_index)
    {
        let line_offset = rope_text.offset_of_line(line);
        let line_col = text_prov.before_phantom_col(line, line_col);

        line_offset + line_col
    } else {
        // There was no text layout line so this is a normal line.
        debug_assert_eq!(line_index, 0);

        rope_text.offset_of_line(line)
    }
}

/// Move to the next visual line, giving the new information.  
/// Returns `(new rel vline, offset)`
fn next_rvline(
    layouts: &TextLayoutCache,
    text_prov: &impl TextLayoutProvider,
    font_size: usize,
    RVLine { line, line_index }: RVLine,
) -> (RVLine, usize) {
    let rope_text = text_prov.rope_text();
    if let Some(layout_line) = layouts.get(font_size, line) {
        if let Some((line_col, _)) =
            layout_line.layout_cols(text_prov, line).nth(line_index + 1)
        {
            let line_offset = rope_text.offset_of_line(line);
            let line_col = text_prov.before_phantom_col(line, line_col);
            let offset = line_offset + line_col;

            (RVLine::new(line, line_index + 1), offset)
        } else {
            // There was no next layout/vline on this buffer line.
            // So we can simply move to the start of the next buffer line.

            (RVLine::new(line + 1, 0), rope_text.offset_of_line(line + 1))
        }
    } else {
        // There was no text layout line, so this is a normal line.
        debug_assert_eq!(line_index, 0);

        (RVLine::new(line + 1, 0), rope_text.offset_of_line(line + 1))
    }
}

/// Move to the previous visual line, giving the new information.  
/// Returns `(new line, new line_index, offset)`  
/// Returns `None` if the line and line index are zero and thus there is no previous visual line.
fn prev_rvline(
    layouts: &TextLayoutCache,
    text_prov: &impl TextLayoutProvider,
    font_sizes: &dyn LineFontSizeProvider,
    RVLine { line, line_index }: RVLine,
) -> Option<(RVLine, usize)> {
    let rope_text = text_prov.rope_text();
    if line_index == 0 {
        // Line index was zero so we must be moving back a buffer line
        if line == 0 {
            return None;
        }

        let prev_line = line - 1;
        let font_size = font_sizes.font_size(prev_line);
        if let Some(layout_line) = layouts.get(font_size, prev_line) {
            let line_offset = rope_text.offset_of_line(prev_line);
            let (i, line_col) = layout_line
                .start_layout_cols(text_prov, prev_line)
                .enumerate()
                .last()
                .unwrap_or((0, 0));
            let line_col = text_prov.before_phantom_col(prev_line, line_col);
            let offset = line_offset + line_col;

            Some((RVLine::new(prev_line, i), offset))
        } else {
            // There was no text layout line, so the previous line is a normal line.
            let prev_line_offset = rope_text.offset_of_line(prev_line);
            Some((RVLine::new(prev_line, 0), prev_line_offset))
        }
    } else {
        // We're still on the same buffer line, so we can just move to the previous layout/vline.

        let prev_line_index = line_index - 1;
        let font_size = font_sizes.font_size(line);
        if let Some(layout_line) = layouts.get(font_size, line) {
            if let Some((line_col, _)) = layout_line
                .layout_cols(text_prov, line)
                .nth(prev_line_index)
            {
                let line_offset = rope_text.offset_of_line(line);
                let line_col = text_prov.before_phantom_col(line, line_col);
                let offset = line_offset + line_col;

                Some((RVLine::new(line, prev_line_index), offset))
            } else {
                // There was no previous layout/vline on this buffer line.
                // So we can simply move to the end of the previous buffer line.

                let prev_line_offset = rope_text.offset_of_line(line - 1);
                Some((RVLine::new(line - 1, 0), prev_line_offset))
            }
        } else {
            debug_assert!(
                false,
                "line_index was nonzero but there was no text layout line"
            );
            // Despite that this shouldn't happen we default to just giving the start of this
            // normal line
            let line_offset = rope_text.offset_of_line(line);
            Some((RVLine::new(line, 0), line_offset))
        }
    }
}

// FIXME: Put this in our cosmic-text fork.

/// Hit position but decides wether it should go to the next line based on the `before` bool.
/// (Hit position should be equivalent to `before=false`).  
/// This is needed when we have an idx at the end of, for example, a wrapped line which could be on
/// the first or second line.
pub fn hit_position_aff(
    this: &floem::cosmic_text::TextLayout,
    idx: usize,
    before: bool,
) -> floem::cosmic_text::HitPosition {
    use floem::{cosmic_text::HitPosition, kurbo::Point};
    let mut last_line = 0;
    let mut last_end: usize = 0;
    let mut offset = 0;
    let mut last_glyph: Option<&LayoutGlyph> = None;
    let mut last_line_width = 0.0;
    let mut last_glyph_width = 0.0;
    let mut last_position = HitPosition {
        line: 0,
        point: Point::ZERO,
        glyph_ascent: 0.0,
        glyph_descent: 0.0,
    };
    for (line, run) in this.layout_runs().enumerate() {
        if run.line_i > last_line {
            last_line = run.line_i;
            offset += last_end + 1;
        }

        // Handles wrapped lines, like:
        // ```rust
        // let config_path = |
        // dirs::config_dir();
        // ```
        // The glyphs won't contain the space at the end of the first part, and the position right
        // after the space is the same column as at `|dirs`, which is what before is letting us
        // distinguish.
        // So essentially, if the next run has a glyph that is at the same idx as the end of the
        // previous run, *and* it is at `idx` itself, then we know to position it on the previous.
        if let Some(last_glyph) = last_glyph {
            if let Some(first_glyph) = run.glyphs.first() {
                let end = last_glyph.end + offset + 1;
                if before && end == idx && end == first_glyph.start + offset {
                    last_position.point.x = (last_line_width + last_glyph.w) as f64;
                    return last_position;
                }
            }
        }

        for glyph in run.glyphs {
            if glyph.start + offset > idx {
                last_position.point.x += last_glyph_width as f64;
                return last_position;
            }
            last_end = glyph.end;
            last_glyph_width = glyph.w;
            last_position = HitPosition {
                line,
                point: Point::new(glyph.x as f64, run.line_y as f64),
                glyph_ascent: run.glyph_ascent as f64,
                glyph_descent: run.glyph_descent as f64,
            };
            if (glyph.start + offset..glyph.end + offset).contains(&idx) {
                return last_position;
            }
        }

        last_glyph = run.glyphs.last();
        last_line_width = run.line_w;
    }

    if idx > 0 {
        last_position.point.x += last_glyph_width as f64;
        return last_position;
    }

    HitPosition {
        line: 0,
        point: Point::ZERO,
        glyph_ascent: 0.0,
        glyph_descent: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, cell::RefCell, sync::Arc};

    use floem::{
        cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout, Wrap},
        reactive::Scope,
    };
    use im::HashMap;
    use lapce_core::{
        buffer::rope_text::{RopeText, RopeTextRef},
        cursor::CursorAffinity,
    };
    use lapce_xi_rope::Rope;
    use smallvec::smallvec;

    use crate::{
        doc::phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine},
        editor::{
            view_data::TextLayoutLine,
            visual_line::{
                find_vline_of_line_backwards, find_vline_of_line_forwards, RVLine,
            },
        },
    };

    use super::{
        find_vline_init_info_forward, find_vline_init_info_rv_backward,
        FontSizeCacheId, LineFontSizeProvider, Lines, ResolvedWrap,
        TextLayoutProvider, VLine,
    };

    /// For most of the logic we standardize on a specific font size.
    const FONT_SIZE: usize = 12;

    struct TestTextLayoutProvider<'a> {
        text: &'a Rope,
        phantom: HashMap<usize, PhantomTextLine>,
        font_family: Vec<FamilyOwned>,
        #[allow(dead_code)]
        wrap: Wrap,
    }
    impl<'a> TestTextLayoutProvider<'a> {
        fn new(
            text: &'a Rope,
            ph: HashMap<usize, PhantomTextLine>,
            wrap: Wrap,
        ) -> Self {
            Self {
                text,
                phantom: ph,
                // we use a specific font to make width calculations consistent between platforms.
                // TODO(minor): Is there a more common font that we can use?
                font_family: FamilyOwned::parse_list("Cascadia Code").collect(),
                wrap,
            }
        }
    }
    impl<'a> TextLayoutProvider for TestTextLayoutProvider<'a> {
        fn text(&self) -> &Rope {
            self.text
        }

        // An implementation relatively close to the actual new text layout impl but simplified.
        // TODO(minor): It would be nice to just use the same impl as view's
        fn new_text_layout(
            &self,
            line: usize,
            font_size: usize,
            wrap: ResolvedWrap,
        ) -> Arc<TextLayoutLine> {
            let rope_text = RopeTextRef::new(self.text);
            let line_content_original = rope_text.line_content(line);

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

            let phantom_text = self.phantom.get(&line).cloned().unwrap_or_default();
            let line_content = phantom_text.combine_with_text(line_content);

            // let color

            let attrs = Attrs::new()
                .family(&self.font_family)
                .font_size(font_size as f32);
            let mut attrs_list = AttrsList::new(attrs);

            // We don't do line styles, since they aren't relevant

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
            text_layout.set_wrap(Wrap::Word);
            match wrap {
                // We do not have to set the wrap mode if we do not set the width
                ResolvedWrap::None => {}
                ResolvedWrap::Column(_col) => todo!(),
                ResolvedWrap::Width(px) => {
                    text_layout.set_size(px, f32::MAX);
                }
            }
            text_layout.set_text(&line_content, attrs_list);

            // skip phantom text background styling because it doesn't shift positions
            // skip severity styling
            // skip diagnostic background styling

            Arc::new(TextLayoutLine {
                extra_style: Vec::new(),
                text: text_layout,
                whitespaces: None,
                indent: 0.0,
            })
        }

        fn before_phantom_col(&self, line: usize, col: usize) -> usize {
            self.phantom
                .get(&line)
                .map(|x| x.before_col(col))
                .unwrap_or(col)
        }

        fn has_multiline_phantom(&self) -> bool {
            // Conservatively, yes.
            true
        }
    }

    struct TestFontSize {
        font_size: usize,
    }
    impl LineFontSizeProvider for TestFontSize {
        fn font_size(&self, _line: usize) -> usize {
            self.font_size
        }

        fn cache_id(&self) -> FontSizeCacheId {
            0
        }
    }

    fn make_lines(
        text: &Rope,
        width: f32,
        init: bool,
    ) -> (TestTextLayoutProvider<'_>, Lines) {
        make_lines_ph(text, width, init, HashMap::new())
    }

    fn make_lines_ph(
        text: &Rope,
        width: f32,
        init: bool,
        ph: HashMap<usize, PhantomTextLine>,
    ) -> (TestTextLayoutProvider<'_>, Lines) {
        let wrap = Wrap::Word;
        let r_wrap = ResolvedWrap::Width(width);
        let font_sizes = TestFontSize {
            font_size: FONT_SIZE,
        };
        let text = TestTextLayoutProvider::new(text, ph, wrap);
        let cx = Scope::new();
        let lines = Lines::new(cx, RefCell::new(Arc::new(font_sizes)));
        lines.set_wrap(r_wrap);

        if init {
            let config_id = 0;
            lines.init_all(config_id, &text, true);
        }

        (text, lines)
    }

    fn render_breaks<'a>(
        text: &'a Rope,
        lines: &mut Lines,
        font_size: usize,
    ) -> Vec<Cow<'a, str>> {
        // TODO: line_content on ropetextref would have the lifetime reference rope_text
        // rather than the held &'a Rope.
        // I think this would require an alternate trait for those functions to avoid incorrect lifetimes. Annoying but workable.
        let rope_text = RopeTextRef::new(text);
        let mut result = Vec::new();
        let layouts = lines.text_layouts.borrow();

        for line in 0..rope_text.num_lines() {
            if let Some(text_layout) = layouts.get(font_size, line) {
                let lines = &text_layout.text.lines;
                for line in lines {
                    let layouts = line.layout_opt().as_deref().unwrap();
                    for layout in layouts {
                        // Spacing
                        if layout.glyphs.is_empty() {
                            continue;
                        }
                        let start_idx = layout.glyphs[0].start;
                        let end_idx = layout.glyphs.last().unwrap().end;
                        // Hacky solution to include the ending space/newline since those get trimmed off
                        let line_content = line
                            .text()
                            .get(start_idx..=end_idx)
                            .unwrap_or(&line.text()[start_idx..end_idx]);
                        result.push(Cow::Owned(line_content.to_string()));
                    }
                }
            } else {
                let line_content = rope_text.line_content(line);

                let line_content = match line_content {
                    Cow::Borrowed(x) => {
                        if let Some(x) = x.strip_suffix('\n') {
                            // Cow::Borrowed(x)
                            Cow::Owned(x.to_string())
                        } else {
                            // Cow::Borrowed(x)
                            Cow::Owned(x.to_string())
                        }
                    }
                    Cow::Owned(x) => {
                        if let Some(x) = x.strip_suffix('\n') {
                            Cow::Owned(x.to_string())
                        } else {
                            Cow::Owned(x)
                        }
                    }
                };
                result.push(line_content);
            }
        }
        result
    }

    /// Utility fn to quickly create simple phantom text
    fn mph(kind: PhantomTextKind, col: usize, text: &str) -> PhantomText {
        PhantomText {
            kind,
            col,
            text: text.to_string(),
            font_size: None,
            fg: None,
            bg: None,
            under_line: None,
        }
    }

    fn ffvline_info(
        lines: &Lines,
        text_prov: impl TextLayoutProvider,
        vline: VLine,
    ) -> Option<(usize, RVLine)> {
        find_vline_init_info_forward(lines, &text_prov, (VLine(0), 0), vline)
    }

    fn fbvline_info(
        lines: &Lines,
        text_prov: impl TextLayoutProvider,
        vline: VLine,
    ) -> Option<(usize, RVLine)> {
        let last_vline = lines.last_vline(&text_prov);
        let last_rvline = lines.last_rvline(&text_prov);
        find_vline_init_info_rv_backward(
            lines,
            &text_prov,
            (last_vline, last_rvline),
            vline,
        )
    }

    #[test]
    fn find_vline_init_info_empty() {
        // Test empty buffer
        let text = Rope::from("");
        let (text_prov, lines) = make_lines(&text, 50.0, false);

        assert_eq!(
            ffvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(
            fbvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(ffvline_info(&lines, &text_prov, VLine(1)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(1)), None);

        // Test empty buffer with phantom text and no wrapping
        let text = Rope::from("");
        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(
                    PhantomTextKind::Completion,
                    0,
                    "hello world abc"
                )],
            },
        );
        let (text_prov, lines) = make_lines_ph(&text, 20.0, false, ph);

        assert_eq!(
            ffvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(
            fbvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(ffvline_info(&lines, &text_prov, VLine(1)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(1)), None);

        // Test empty buffer with phantom text and wrapping
        lines.init_all(0, &text_prov, true);

        assert_eq!(
            ffvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(
            fbvline_info(&lines, &text_prov, VLine(0)),
            Some((0, RVLine::new(0, 0)))
        );
        assert_eq!(
            ffvline_info(&lines, &text_prov, VLine(1)),
            Some((0, RVLine::new(0, 1)))
        );
        assert_eq!(
            fbvline_info(&lines, &text_prov, VLine(1)),
            Some((0, RVLine::new(0, 1)))
        );
        assert_eq!(
            ffvline_info(&lines, &text_prov, VLine(2)),
            Some((0, RVLine::new(0, 2)))
        );
        assert_eq!(
            fbvline_info(&lines, &text_prov, VLine(2)),
            Some((0, RVLine::new(0, 2)))
        );
        // Going outside bounds only ends up with None
        assert_eq!(ffvline_info(&lines, &text_prov, VLine(3)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(3)), None);
        // The affinity would shift from the front/end of the phantom line
        // TODO: test affinity of logic behind clicking past the last vline?
    }

    #[test]
    fn find_vline_init_info_unwrapping() {
        // Multiple lines with too large width for there to be any wrapping.
        let text = Rope::from("hello\nworld toast and jam\nthe end\nhi");
        let rope_text = RopeTextRef::new(&text);
        let (text_prov, mut lines) = make_lines(&text, 500.0, false);

        // Assert that with no text layouts (aka no wrapping and no phantom text) the function
        // works
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["hello", "world toast and jam", "the end", "hi"]
        );

        lines.init_all(0, &text_prov, true);

        // Assert that even with text layouts, if it has no wrapping applied (because the width is large in this case) and no phantom text then it produces the same offsets as before.
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(20)), None);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["hello ", "world toast and jam ", "the end ", "hi"]
        );
    }

    #[test]
    fn find_vline_init_info_phantom_unwrapping() {
        let text = Rope::from("hello\nworld toast and jam\nthe end\nhi");
        let rope_text = RopeTextRef::new(&text);

        // Multiple lines with too large width for there to be any wrapping and phantom text
        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(PhantomTextKind::Completion, 0, "greet world")],
            },
        );

        let (text_prov, lines) = make_lines_ph(&text, 500.0, false, ph);

        // With no text layouts, phantom text isn't initialized so it has no affect.
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        lines.init_all(0, &text_prov, true);

        // With text layouts, the phantom text is applied.
        // But with a single line of phantom text, it doesn't affect the offsets.
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        // Multiple lines with too large width and a phantom text that takes up multiple lines.
        let mut ph = HashMap::new();
        ph.insert(
             0,
             PhantomTextLine {
                 text: smallvec![
                     mph(PhantomTextKind::Completion, 0, "greet\nworld"),
                 ],
             },
         );

        let (text_prov, mut lines) = make_lines_ph(&text, 500.0, false, ph);

        // With no text layouts, phantom text isn't initialized so it has no affect.
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        lines.init_all(0, &text_prov, true);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            [
                "greet",
                "worldhello ",
                "world toast and jam ",
                "the end ",
                "hi"
            ]
        );

        // With text layouts, the phantom text is applied.
        // With a phantom text that takes up multiple lines, it does not affect the offsets
        // but it does affect the valid visual lines.
        let info = ffvline_info(&lines, &text_prov, VLine(0));
        assert_eq!(info, Some((0, RVLine::new(0, 0))));
        let info = fbvline_info(&lines, &text_prov, VLine(0));
        assert_eq!(info, Some((0, RVLine::new(0, 0))));
        let info = ffvline_info(&lines, &text_prov, VLine(1));
        assert_eq!(info, Some((0, RVLine::new(0, 1))));
        let info = fbvline_info(&lines, &text_prov, VLine(1));
        assert_eq!(info, Some((0, RVLine::new(0, 1))));

        for line in 2..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line - 1);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                info,
                (line_offset, RVLine::new(line - 1, 0)),
                "vline {}",
                line
            );
            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                info,
                (line_offset, RVLine::new(line - 1, 0)),
                "vline {}",
                line
            );
        }

        // Then there's one extra vline due to the phantom text wrapping
        let line_offset = rope_text.offset_of_line(rope_text.last_line());

        let info =
            ffvline_info(&lines, &text_prov, VLine(rope_text.last_line() + 1));
        assert_eq!(
            info,
            Some((line_offset, RVLine::new(rope_text.last_line(), 0))),
            "line {}",
            rope_text.last_line() + 1,
        );
        let info =
            fbvline_info(&lines, &text_prov, VLine(rope_text.last_line() + 1));
        assert_eq!(
            info,
            Some((line_offset, RVLine::new(rope_text.last_line(), 0))),
            "line {}",
            rope_text.last_line() + 1,
        );

        // Multiple lines with too large width and a phantom text that takes up multiple lines.
        // But the phantom text is not at the start of the first line.
        let mut ph = HashMap::new();
        ph.insert(
             2, // "the end"
             PhantomTextLine {
                 text: smallvec![
                     mph(PhantomTextKind::Completion, 3, "greet\nworld"),
                 ],
             },
         );

        let (text_prov, mut lines) = make_lines_ph(&text, 500.0, false, ph);

        // With no text layouts, phantom text isn't initialized so it has no affect.
        for line in 0..rope_text.num_lines() {
            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();

            let line_offset = rope_text.offset_of_line(line);

            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        lines.init_all(0, &text_prov, true);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            [
                "hello ",
                "world toast and jam ",
                "thegreet",
                "world end ",
                "hi"
            ]
        );

        // With text layouts, the phantom text is applied.
        // With a phantom text that takes up multiple lines, it does not affect the offsets
        // but it does affect the valid visual lines.
        for line in 0..3 {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "vline {}", line);
        }

        // ' end'
        let info = ffvline_info(&lines, &text_prov, VLine(3));
        assert_eq!(info, Some((29, RVLine::new(2, 1))));
        let info = fbvline_info(&lines, &text_prov, VLine(3));
        assert_eq!(info, Some((29, RVLine::new(2, 1))));

        let info = ffvline_info(&lines, &text_prov, VLine(4));
        assert_eq!(info, Some((34, RVLine::new(3, 0))));
        let info = fbvline_info(&lines, &text_prov, VLine(4));
        assert_eq!(info, Some((34, RVLine::new(3, 0))));
    }

    #[test]
    fn find_vline_init_info_basic_wrapping() {
        // Tests with more mixes of text layout lines and uninitialized lines

        // Multiple lines with a small enough width for there to be a bunch of wrapping
        let text = Rope::from("hello\nworld toast and jam\nthe end\nhi");
        let rope_text = RopeTextRef::new(&text);
        let (text_prov, mut lines) = make_lines(&text, 30.0, false);

        // Assert that with no text layouts (aka no wrapping and no phantom text) the function
        // works
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "line {}", line);

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(info, (line_offset, RVLine::new(line, 0)), "line {}", line);
        }

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(20)), None);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["hello", "world toast and jam", "the end", "hi"]
        );

        lines.init_all(0, &text_prov, true);

        {
            let layouts = lines.text_layouts.borrow();

            assert!(layouts.get(FONT_SIZE, 0).is_some());
            assert!(layouts.get(FONT_SIZE, 1).is_some());
            assert!(layouts.get(FONT_SIZE, 2).is_some());
            assert!(layouts.get(FONT_SIZE, 3).is_some());
            assert!(layouts.get(FONT_SIZE, 4).is_none());
        }

        // start offset, start buffer line, layout line index)
        let line_data = [
            (0, 0, 0),
            (6, 1, 0),
            (12, 1, 1),
            (18, 1, 2),
            (22, 1, 3),
            (26, 2, 0),
            (30, 2, 1),
            (34, 3, 0),
        ];
        assert_eq!(lines.last_vline(&text_prov), VLine(7));
        assert_eq!(lines.last_rvline(&text_prov), RVLine::new(3, 0));
        #[allow(clippy::needless_range_loop)]
        for line in 0..8 {
            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[line],
                "vline {}",
                line
            );
            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[line],
                "vline {}",
                line
            );
        }

        // Directly out of bounds
        assert_eq!(ffvline_info(&lines, &text_prov, VLine(9)), None,);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(9)), None,);

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(20)), None);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["hello ", "world ", "toast ", "and ", "jam ", "the ", "end ", "hi"]
        );

        let vline_line_data = [0, 1, 5, 7];

        let rope = text_prov.rope_text();
        let last_start_vline = VLine(
            lines.last_vline(&text_prov).get()
                - lines.last_rvline(&text_prov).line_index,
        );
        #[allow(clippy::needless_range_loop)]
        for line in 0..4 {
            let vline = VLine(vline_line_data[line]);
            assert_eq!(
                find_vline_of_line_forwards(&lines, Default::default(), line),
                Some(vline)
            );
            assert_eq!(
                find_vline_of_line_backwards(
                    &lines,
                    (last_start_vline, rope.last_line()),
                    line
                ),
                Some(vline),
                "line: {line}"
            );
        }

        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let (text_prov, mut lines) = make_lines(&text, 2., true);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            [
                "aaaa ", "bb ", "bb ", "cc ", "cc ", "dddd ", "eeee ", "ff ", "ff ",
                "gggg"
            ]
        );

        // (start offset, start buffer line, layout line index)
        let line_data = [
            (0, 0, 0),
            (5, 1, 0),
            (8, 1, 1),
            (11, 1, 2),
            (14, 2, 0),
            (17, 2, 1),
            (22, 2, 2),
            (27, 2, 3),
            (30, 3, 0),
            (33, 3, 1),
        ];
        #[allow(clippy::needless_range_loop)]
        for vline in 0..10 {
            let info = ffvline_info(&lines, &text_prov, VLine(vline)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[vline],
                "vline {}",
                vline
            );
            let info = fbvline_info(&lines, &text_prov, VLine(vline)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[vline],
                "vline {}",
                vline
            );
        }

        let vline_line_data = [0, 1, 4, 8];

        let rope = text_prov.rope_text();
        let last_start_vline = VLine(
            lines.last_vline(&text_prov).get()
                - lines.last_rvline(&text_prov).line_index,
        );
        #[allow(clippy::needless_range_loop)]
        for line in 0..4 {
            let vline = VLine(vline_line_data[line]);
            assert_eq!(
                find_vline_of_line_forwards(&lines, Default::default(), line),
                Some(vline)
            );
            assert_eq!(
                find_vline_of_line_backwards(
                    &lines,
                    (last_start_vline, rope.last_line()),
                    line
                ),
                Some(vline),
                "line: {line}"
            );
        }

        // TODO: tests that have less line wrapping
    }

    #[test]
    fn find_vline_init_info_basic_wrapping_phantom() {
        // Single line Phantom text at the very start
        let text = Rope::from("hello\nworld toast and jam\nthe end\nhi");
        let rope_text = RopeTextRef::new(&text);

        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(PhantomTextKind::Completion, 0, "greet world")],
            },
        );

        let (text_prov, mut lines) = make_lines_ph(&text, 30.0, false, ph);

        // Assert that with no text layouts there is no change in behavior from having no phantom
        // text
        for line in 0..rope_text.num_lines() {
            let line_offset = rope_text.offset_of_line(line);

            let info = ffvline_info(&lines, &text_prov, VLine(line));
            assert_eq!(
                info,
                Some((line_offset, RVLine::new(line, 0))),
                "line {}",
                line
            );

            let info = fbvline_info(&lines, &text_prov, VLine(line));
            assert_eq!(
                info,
                Some((line_offset, RVLine::new(line, 0))),
                "line {}",
                line
            );
        }

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(20)), None);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["hello", "world toast and jam", "the end", "hi"]
        );

        lines.init_all(0, &text_prov, true);

        {
            let layouts = lines.text_layouts.borrow();

            assert!(layouts.get(FONT_SIZE, 0).is_some());
            assert!(layouts.get(FONT_SIZE, 1).is_some());
            assert!(layouts.get(FONT_SIZE, 2).is_some());
            assert!(layouts.get(FONT_SIZE, 3).is_some());
            assert!(layouts.get(FONT_SIZE, 4).is_none());
        }

        // start offset, start buffer line, layout line index)
        let line_data = [
            (0, 0, 0),
            (0, 0, 1),
            (6, 1, 0),
            (12, 1, 1),
            (18, 1, 2),
            (22, 1, 3),
            (26, 2, 0),
            (30, 2, 1),
            (34, 3, 0),
        ];

        #[allow(clippy::needless_range_loop)]
        for line in 0..9 {
            let info = ffvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[line],
                "vline {}",
                line
            );

            let info = fbvline_info(&lines, &text_prov, VLine(line)).unwrap();
            assert_eq!(
                (info.0, info.1.line, info.1.line_index),
                line_data[line],
                "vline {}",
                line
            );
        }

        // Directly out of bounds
        assert_eq!(ffvline_info(&lines, &text_prov, VLine(9)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(9)), None);

        assert_eq!(ffvline_info(&lines, &text_prov, VLine(20)), None);
        assert_eq!(fbvline_info(&lines, &text_prov, VLine(20)), None);

        // TODO: Currently the way we join phantom text and how cosmic wraps lines,
        // the phantom text will be joined with whatever the word next to it is - if there is no
        // spaces. It might be desirable to always separate them to let it wrap independently.
        // An easy way to do this is to always include a space, and then manually cut the glyph
        // margin in the text layout.
        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            [
                "greet ",
                "worldhello ",
                "world ",
                "toast ",
                "and ",
                "jam ",
                "the ",
                "end ",
                "hi"
            ]
        );

        // TODO: multiline phantom text in the middle
        // TODO: test at the end
    }

    #[test]
    fn num_vlines() {
        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let (text_prov, lines) = make_lines(&text, 2., true);
        assert_eq!(lines.num_vlines(&text_prov), 10);

        // With phantom text
        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(PhantomTextKind::Completion, 0, "greet\nworld")],
            },
        );

        let (text_prov, lines) = make_lines_ph(&text, 2., true, ph);

        // Only one increase because the second line of the phantom text is directly attached to
        // the word at the start of the next line.
        assert_eq!(lines.num_vlines(&text_prov), 11);
    }

    #[test]
    fn offset_to_line() {
        let text = "a b c d ".into();
        let (text_prov, lines) = make_lines(&text, 1., true);
        assert_eq!(lines.num_vlines(&text_prov), 4);

        let vlines = [0, 0, 1, 1, 2, 2, 3, 3];
        for (i, v) in vlines.iter().enumerate() {
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Forward),
                VLine(*v),
                "offset: {i}"
            );
        }

        assert_eq!(lines.offset_of_vline(&text_prov, VLine(0)), 0);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(1)), 2);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(2)), 4);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(3)), 6);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(10)), 8);

        for offset in 0..text.len() {
            let line =
                lines.vline_of_offset(&text_prov, offset, CursorAffinity::Forward);
            let line_offset = lines.offset_of_vline(&text_prov, line);
            assert!(
                line_offset <= offset,
                "{} <= {} L{:?} O{}",
                line_offset,
                offset,
                line,
                offset
            );
        }

        let text = "blah\n\n\nhi\na b c d e".into();
        let (text_prov, lines) = make_lines(&text, 12.0 * 3.0, true);
        let vlines = [0, 0, 0, 0, 0];
        for (i, v) in vlines.iter().enumerate() {
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Forward),
                VLine(*v),
                "offset: {i}"
            );
        }
        assert_eq!(
            lines
                .vline_of_offset(&text_prov, 4, CursorAffinity::Backward)
                .get(),
            0
        );
        // Test that cursor affinity has no effect for hard line breaks
        assert_eq!(
            lines
                .vline_of_offset(&text_prov, 5, CursorAffinity::Forward)
                .get(),
            1
        );
        assert_eq!(
            lines
                .vline_of_offset(&text_prov, 5, CursorAffinity::Backward)
                .get(),
            1
        );
        // starts at 'd'. Tests that cursor affinity works for soft line breaks
        assert_eq!(
            lines
                .vline_of_offset(&text_prov, 16, CursorAffinity::Forward)
                .get(),
            5
        );
        assert_eq!(
            lines
                .vline_of_offset(&text_prov, 16, CursorAffinity::Backward)
                .get(),
            4
        );

        assert_eq!(
            lines.vline_of_offset(&text_prov, 20, CursorAffinity::Forward),
            lines.last_vline(&text_prov)
        );

        let text = "a\nb\nc\n".into();
        let (text_prov, lines) = make_lines(&text, 1., true);
        assert_eq!(lines.num_vlines(&text_prov), 4);

        // let vlines = [(0, 0), (0, 0), (1, 1), (1, 1), (2, 2), (2, 2), (3, 3)];
        let vlines = [0, 0, 1, 1, 2, 2, 3, 3];
        for (i, v) in vlines.iter().enumerate() {
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Forward),
                VLine(*v),
                "offset: {i}"
            );
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Backward),
                VLine(*v),
                "offset: {i}"
            );
        }

        let text = Rope::from(
            "asdf\nposition: Some(EditorPosition::Offset(self.offset))\nasdf\nasdf",
        );
        let (text_prov, mut lines) = make_lines(&text, 1., true);
        println!("Breaks: {:?}", render_breaks(&text, &mut lines, FONT_SIZE));

        let rvline = lines.rvline_of_offset(&text_prov, 3, CursorAffinity::Backward);
        assert_eq!(rvline, RVLine::new(0, 0));
        let rvline_info = lines
            .iter_rvlines(&text_prov, false, rvline)
            .next()
            .unwrap();
        assert_eq!(rvline_info.rvline, rvline);
        let offset = lines.offset_of_rvline(&text_prov, rvline);
        assert_eq!(offset, 0);
        assert_eq!(
            lines.vline_of_offset(&text_prov, offset, CursorAffinity::Backward),
            VLine(0)
        );
        assert_eq!(lines.vline_of_rvline(&text_prov, rvline), VLine(0));

        let rvline = lines.rvline_of_offset(&text_prov, 7, CursorAffinity::Backward);
        assert_eq!(rvline, RVLine::new(1, 0));
        let rvline_info = lines
            .iter_rvlines(&text_prov, false, rvline)
            .next()
            .unwrap();
        assert_eq!(rvline_info.rvline, rvline);
        let offset = lines.offset_of_rvline(&text_prov, rvline);
        assert_eq!(offset, 5);
        assert_eq!(
            lines.vline_of_offset(&text_prov, offset, CursorAffinity::Backward),
            VLine(1)
        );
        assert_eq!(lines.vline_of_rvline(&text_prov, rvline), VLine(1));

        let rvline =
            lines.rvline_of_offset(&text_prov, 17, CursorAffinity::Backward);
        assert_eq!(rvline, RVLine::new(1, 1));
        let rvline_info = lines
            .iter_rvlines(&text_prov, false, rvline)
            .next()
            .unwrap();
        assert_eq!(rvline_info.rvline, rvline);
        let offset = lines.offset_of_rvline(&text_prov, rvline);
        assert_eq!(offset, 15);
        assert_eq!(
            lines.vline_of_offset(&text_prov, offset, CursorAffinity::Backward),
            VLine(1)
        );
        assert_eq!(
            lines.vline_of_offset(&text_prov, offset, CursorAffinity::Forward),
            VLine(2)
        );
        assert_eq!(lines.vline_of_rvline(&text_prov, rvline), VLine(2));
    }

    #[test]
    fn offset_to_line_phantom() {
        let text = "a b c d ".into();
        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(PhantomTextKind::Completion, 1, "hi")],
            },
        );

        let (text_prov, mut lines) = make_lines_ph(&text, 1., true, ph);

        // The 'hi' is joined with the 'a' so it's not wrapped to a separate line
        assert_eq!(lines.num_vlines(&text_prov), 4);

        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["ahi ", "b ", "c ", "d "]
        );

        let vlines = [0, 0, 1, 1, 2, 2, 3, 3];
        // Unchanged. The phantom text has no effect in the position. It doesn't shift a line with
        // the affinity due to its position and it isn't multiline.
        for (i, v) in vlines.iter().enumerate() {
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Forward),
                VLine(*v),
                "offset: {i}"
            );
        }

        assert_eq!(lines.offset_of_vline(&text_prov, VLine(0)), 0);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(1)), 2);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(2)), 4);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(3)), 6);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(10)), 8);

        for offset in 0..text.len() {
            let line =
                lines.vline_of_offset(&text_prov, offset, CursorAffinity::Forward);
            let line_offset = lines.offset_of_vline(&text_prov, line);
            assert!(
                line_offset <= offset,
                "{} <= {} L{:?} O{}",
                line_offset,
                offset,
                line,
                offset
            );
        }

        // Same as above but with a slightly shifted to make the affinity change the resulting vline
        let mut ph = HashMap::new();
        ph.insert(
            0,
            PhantomTextLine {
                text: smallvec![mph(PhantomTextKind::Completion, 2, "hi")],
            },
        );

        let (text_prov, mut lines) = make_lines_ph(&text, 1., true, ph);

        // The 'hi' is joined with the 'a' so it's not wrapped to a separate line
        assert_eq!(lines.num_vlines(&text_prov), 4);

        // TODO: Should this really be forward rendered?
        assert_eq!(
            render_breaks(&text, &mut lines, FONT_SIZE),
            ["a ", "hib ", "c ", "d "]
        );

        for (i, v) in vlines.iter().enumerate() {
            assert_eq!(
                lines.vline_of_offset(&text_prov, i, CursorAffinity::Forward),
                VLine(*v),
                "offset: {i}"
            );
        }
        assert_eq!(
            lines.vline_of_offset(&text_prov, 2, CursorAffinity::Backward),
            VLine(0)
        );

        assert_eq!(lines.offset_of_vline(&text_prov, VLine(0)), 0);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(1)), 2);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(2)), 4);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(3)), 6);
        assert_eq!(lines.offset_of_vline(&text_prov, VLine(10)), 8);

        for offset in 0..text.len() {
            let line =
                lines.vline_of_offset(&text_prov, offset, CursorAffinity::Forward);
            let line_offset = lines.offset_of_vline(&text_prov, line);
            assert!(
                line_offset <= offset,
                "{} <= {} L{:?} O{}",
                line_offset,
                offset,
                line,
                offset
            );
        }
    }

    #[test]
    fn iter_lines() {
        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let (text_prov, lines) = make_lines(&text, 2., true);
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(0))
            .take(2)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["aaaa", "bb "]);

        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(1))
            .take(2)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["bb ", "bb "]);

        let v = lines.get_init_text_layout(0, &text_prov, 2, true);
        let v = v.layout_cols(&text_prov, 2).collect::<Vec<_>>();
        assert_eq!(v, [(0, 3), (3, 8), (8, 13), (13, 15)]);
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(3))
            .take(3)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["cc", "cc ", "dddd "]);

        let mut r: Vec<_> = lines.iter_vlines(&text_prov, false, VLine(0)).collect();
        r.reverse();
        let r1: Vec<_> = lines
            .iter_vlines(&text_prov, true, lines.last_vline(&text_prov))
            .collect();
        assert_eq!(r, r1);

        let rel1: Vec<_> = lines
            .iter_rvlines(&text_prov, false, RVLine::new(0, 0))
            .map(|i| i.rvline)
            .collect();
        r.reverse(); // revert back
        assert!(r.iter().map(|i| i.rvline).eq(rel1));

        // Empty initialized
        let text: Rope = "".into();
        let (text_prov, lines) = make_lines(&text, 2., true);
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(0))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec![""]);
        // Empty initialized - Out of bounds
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(1))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(2))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());

        let mut r: Vec<_> = lines.iter_vlines(&text_prov, false, VLine(0)).collect();
        r.reverse();
        let r1: Vec<_> = lines
            .iter_vlines(&text_prov, true, lines.last_vline(&text_prov))
            .collect();
        assert_eq!(r, r1);

        let rel1: Vec<_> = lines
            .iter_rvlines(&text_prov, false, RVLine::new(0, 0))
            .map(|i| i.rvline)
            .collect();
        r.reverse(); // revert back
        assert!(r.iter().map(|i| i.rvline).eq(rel1));

        // Empty uninitialized
        let text: Rope = "".into();
        let (text_prov, lines) = make_lines(&text, 2., false);
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(0))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec![""]);
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(1))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());
        let r: Vec<_> = lines
            .iter_vlines(&text_prov, false, VLine(2))
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());

        let mut r: Vec<_> = lines.iter_vlines(&text_prov, false, VLine(0)).collect();
        r.reverse();
        let r1: Vec<_> = lines
            .iter_vlines(&text_prov, true, lines.last_vline(&text_prov))
            .collect();
        assert_eq!(r, r1);

        let rel1: Vec<_> = lines
            .iter_rvlines(&text_prov, false, RVLine::new(0, 0))
            .map(|i| i.rvline)
            .collect();
        r.reverse(); // revert back
        assert!(r.iter().map(|i| i.rvline).eq(rel1));

        // TODO: clean up the above tests with some helper function. Very noisy at the moment.
        // TODO: phantom text iter lines tests?
    }

    // TODO(minor): Deduplicate the test code between this and iter_lines
    // We're just testing whether it has equivalent behavior to iter lines (when lines are
    // initialized)
    #[test]
    fn init_iter_vlines() {
        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let (text_prov, lines) = make_lines(&text, 2., false);
        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(0), true)
            .take(2)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["aaaa", "bb "]);

        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(1), true)
            .take(2)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["bb ", "bb "]);

        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(3), true)
            .take(3)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec!["cc", "cc ", "dddd "]);

        // Empty initialized
        let text: Rope = "".into();
        let (text_prov, lines) = make_lines(&text, 2., false);
        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(0), true)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, vec![""]);
        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(1), true)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());
        let r: Vec<_> = lines
            .iter_vlines_init(&text_prov, 0, VLine(2), true)
            .map(|l| text.slice_to_cow(l.interval))
            .collect();
        assert_eq!(r, Vec::<&str>::new());
    }

    #[test]
    fn line_numbers() {
        let text: Rope = "aaaa\nbb bb cc\ncc dddd eeee ff\nff gggg".into();
        let (text_prov, lines) = make_lines(&text, 12.0 * 2.0, true);
        let get_nums = |start_vline: usize| {
            lines
                .iter_vlines(&text_prov, false, VLine(start_vline))
                .map(|l| {
                    (
                        l.rvline.line,
                        l.vline.get(),
                        l.is_first(),
                        text.slice_to_cow(l.interval),
                    )
                })
                .collect::<Vec<_>>()
        };
        // (line, vline, is_first, text)
        let x = vec![
            (0, 0, true, "aaaa".into()),
            (1, 1, true, "bb ".into()),
            (1, 2, false, "bb ".into()),
            (1, 3, false, "cc\n".into()), // TODO: why does this have \n but the first line doesn't??
            (2, 4, true, "cc ".into()),
            (2, 5, false, "dddd ".into()),
            (2, 6, false, "eeee ".into()),
            (2, 7, false, "ff\n".into()),
            (3, 8, true, "ff ".into()),
            (3, 9, false, "gggg".into()),
        ];

        // This ensures that there's no inconsistencies between starting at a specific index
        // vs starting at zero and iterating to that index.
        for i in 0..x.len() {
            let nums = get_nums(i);
            println!("i: {i}, #nums: {}, #&x[i..]: {}", nums.len(), x[i..].len());
            assert_eq!(nums, &x[i..], "failed at #{i}");
        }

        // TODO: test this without any wrapping
    }

    #[test]
    fn last_col() {
        let text: Rope = Rope::from("conf = Config::default();");
        let (text_prov, lines) = make_lines(&text, 24.0 * 2.0, true);

        let mut iter = lines.iter_rvlines(&text_prov, false, RVLine::default());

        // "conf = "
        let v = iter.next().unwrap();
        assert_eq!(v.last_col(&text_prov, false), 6);
        assert_eq!(v.last_col(&text_prov, true), 7);

        // "Config::default();"
        let v = iter.next().unwrap();
        assert_eq!(v.last_col(&text_prov, false), 24);
        assert_eq!(v.last_col(&text_prov, true), 25);
    }
}
