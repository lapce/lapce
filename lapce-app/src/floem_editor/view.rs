use std::{collections::HashMap, ops::RangeInclusive, rc::Rc, sync::Arc};

use floem::{
    context::PaintCx,
    id::Id,
    kurbo::{Point, Rect},
    reactive::{RwSignal, Scope},
    view::{View, ViewData},
    views::{empty, stack},
    Renderer,
};

use crate::editor::visual_line::{Lines, RVLine, VLineInfo};

use super::{editor::Editor, text::Document};

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

fn editor_gutter() -> impl View {
    let padding_left = 0.0;
    stack((empty(),))
}
