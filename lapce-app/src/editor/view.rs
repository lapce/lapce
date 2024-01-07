use std::{
    cmp, collections::HashMap, ops::RangeInclusive, path::PathBuf, rc::Rc, sync::Arc,
};

use floem::{
    action::{set_ime_allowed, set_ime_cursor_area},
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    event::{Event, EventListener},
    id::Id,
    keyboard::ModifiersState,
    peniko::{
        kurbo::{BezPath, Line, Point, Rect, Size},
        Color,
    },
    reactive::{
        batch, create_effect, create_memo, create_rw_signal, Memo, ReadSignal,
        RwSignal, Scope,
    },
    style::{CursorStyle, Style},
    taffy::prelude::Node,
    view::{View, ViewData},
    views::{
        clip, container, dyn_stack, empty, label, scroll, stack, svg, Decorators,
    },
    EventPropagation, Renderer,
};
use itertools::Itertools;
use lapce_core::{
    buffer::{diff::DiffLines, rope_text::RopeText},
    cursor::{ColPosition, CursorAffinity, CursorMode},
    mode::{Mode, VisualMode},
};
use lapce_rpc::dap_types::{DapId, SourceBreakpoint};
use lapce_xi_rope::find::CaseMatching;

use super::{
    gutter::editor_gutter_view,
    view_data::{EditorViewData, LineExtraStyle},
    visual_line::{RVLine, VLine, VLineInfo},
    EditorData, CHAR_WIDTH,
};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    debug::LapceBreakpoint,
    doc::{phantom_text::PhantomTextKind, DocContent, Document, DocumentExt},
    keypress::KeyPressFocus,
    text_input::text_input,
    window_tab::{Focus, WindowTabData},
    workspace::LapceWorkspace,
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

    /// Ran on [`LayoutEvent::CreatedLayout`] to update  [`ScreenLinesBase`] &
    /// the viewport if necessary.
    ///
    /// Returns `true` if [`ScreenLines`] needs to be completely updated in response
    pub(crate) fn on_created_layout(
        &self,
        view: &EditorViewData,
        line: usize,
    ) -> bool {
        let config = view.config.get_untracked();
        let base = self.base.get_untracked();
        let vp = view.viewport.get_untracked();

        let is_before = self
            .iter_vline_info()
            .next()
            .map(|l| line < l.rvline.line)
            .unwrap_or(false);

        // If the line is created before the current screenlines, we can simply shift the
        // base and viewport forward by the number of extra wrapped lines,
        // without needing to recompute the screen lines.
        if is_before {
            let line_height = config.editor.line_height();

            // We could use `try_get_text_layout` here, but I believe this guards against a rare
            // crash (though it is hard to verify) wherein the config id has changed and so the
            // layouts get cleared.
            // However, the original trigger of the layout event was when a layout was created
            // and it expects it to still exist. So we create it just in case, though we of course
            // don't trigger another layout event.
            let layout = view.get_text_layout_trigger(line, false);

            // One line was already accounted for by treating it as an unwrapped line.
            let new_lines = layout.line_count() - 1;

            let new_y0 = base.active_viewport.y0 + (new_lines * line_height) as f64;
            let new_y1 = new_y0 + vp.height();
            let new_viewport = Rect::new(vp.x0, new_y0, vp.x1, new_y1);

            batch(|| {
                self.base.set(ScreenLinesBase {
                    active_viewport: new_viewport,
                });
                view.viewport.set(new_viewport);
            });

            // Ensure that it is created even after the base/viewport signals have been updated.
            // (We need the `get_text_layout` to still have the layout)
            // But we have to trigger an event still if it is created because it *would* alter the
            // screenlines.
            // TODO: this has some risk for infinite looping if we're unlucky.
            let _layout = view.get_text_layout_trigger(line, true);

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

struct StickyHeaderInfo {
    sticky_lines: Vec<usize>,
    last_sticky_should_scroll: bool,
    y_diff: f64,
}

pub struct EditorView {
    id: Id,
    data: ViewData,
    editor: Rc<EditorData>,
    is_active: Memo<bool>,
    inner_node: Option<Node>,
    viewport: RwSignal<Rect>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    sticky_header_info: StickyHeaderInfo,
}

pub fn editor_view(
    editor: Rc<EditorData>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> EditorView {
    let id = Id::next();
    let is_active = create_memo(move |_| is_active(true));

    let viewport = editor.viewport;

    let doc = editor.view.doc;
    let view_kind = editor.view.kind;
    create_effect(move |_| {
        doc.track();
        view_kind.track();
        id.request_layout();
    });

    let hide_cursor = editor.common.window_common.hide_cursor;
    create_effect(move |_| {
        hide_cursor.track();
        let occurrences = doc.with(|doc| doc.backend.find_result.occurrences);
        occurrences.track();
        id.request_paint();
    });

    create_effect(move |last_rev| {
        let buffer = doc.with(|doc| doc.buffer);
        let rev = buffer.with(|buffer| buffer.rev());
        if last_rev == Some(rev) {
            return rev;
        }
        id.request_layout();
        rev
    });

    let config = editor.common.config;
    let sticky_header_height_signal = editor.sticky_header_height;
    let editor2 = editor.clone();
    create_effect(move |last_rev| {
        let config = config.get();
        if !config.editor.sticky_header {
            return (DocContent::Local, 0, 0, Rect::ZERO);
        }

        let doc = doc.get();
        let rect = viewport.get();
        let rev = (
            doc.content.get(),
            doc.buffer.with(|b| b.rev()),
            doc.cache_rev.get(),
            rect,
        );
        if last_rev.as_ref() == Some(&rev) {
            return rev;
        }

        let sticky_header_info = get_sticky_header_info(
            &editor2.view,
            doc,
            viewport,
            sticky_header_height_signal,
            &config,
        );

        id.update_state(sticky_header_info, false);

        rev
    });

    let editor_window_origin = editor.window_origin;
    let cursor = editor.cursor;
    let find_focus = editor.find_focus;
    let ime_allowed = editor.common.window_common.ime_allowed;
    let editor_viewport = editor.viewport;
    let editor_view = editor.view.clone();
    let doc = editor.view.doc;
    let editor_cursor = editor.cursor;
    let local_editor = editor.clone();
    create_effect(move |_| {
        let active = is_active.get();
        if active && !find_focus.get() {
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
                let (_, point_below) =
                    editor_view.points_of_offset(offset, affinity);
                let window_origin = editor_window_origin.get();
                let viewport = editor_viewport.get();
                let pos = window_origin
                    + (point_below.x - viewport.x0, point_below.y - viewport.y0);
                set_ime_cursor_area(pos, Size::new(800.0, 600.0));
            }
        }
    });

    EditorView {
        id,
        data: ViewData::new(id),
        editor,
        is_active,
        inner_node: None,
        viewport,
        debug_breakline,
        sticky_header_info: StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        },
    }
    .on_event(EventListener::ImePreedit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImePreedit { text, cursor } = event {
            let doc = doc.get_untracked();
            if text.is_empty() {
                doc.clear_preedit();
            } else {
                let offset = editor_cursor.with_untracked(|c| c.offset());
                doc.set_preedit(text.clone(), *cursor, offset);
            }
        }
        EventPropagation::Stop
    })
    .on_event(EventListener::ImeCommit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImeCommit(text) = event {
            let doc = doc.get_untracked();
            doc.clear_preedit();
            local_editor.receive_char(text);
        }
        EventPropagation::Stop
    })
}

impl EditorView {
    fn paint_diff_sections(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        config: &LapceConfig,
    ) {
        let Some(diff_sections) = &screen_lines.diff_sections else {
            return;
        };
        for section in diff_sections.iter() {
            match section.kind {
                DiffSectionKind::NoCode => self.paint_diff_no_code(
                    cx,
                    viewport,
                    section.y_idx,
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
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(LapceColor::SOURCE_CONTROL_ADDED)
                            .with_alpha_factor(0.2),
                        0.0,
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
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(LapceColor::SOURCE_CONTROL_REMOVED)
                            .with_alpha_factor(0.2),
                        0.0,
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
                    config.color(LapceColor::EDITOR_DIM),
                    1.0,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_normal_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        line_height: f64,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        is_block_cursor: bool,
    ) {
        let view = &self.editor.view;

        // TODO: selections should have separate start/end affinity
        let (start_rvline, start_col) =
            view.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, end_col) = view.rvline_col_of_offset(end_offset, affinity);

        for LineInfo {
            vline_y,
            vline_info: info,
            ..
        } in screen_lines.iter_line_info_r(start_rvline..=end_rvline)
        {
            let rvline = info.rvline;
            let line = rvline.line;

            let phantom_text = view.line_phantom_text(line);
            let left_col = if rvline == start_rvline {
                start_col
            } else {
                view.first_col(info)
            };
            let right_col = if rvline == end_rvline {
                end_col
            } else {
                view.last_col(info, true)
            };
            let left_col = phantom_text.col_after(left_col, is_block_cursor);
            let right_col = phantom_text.col_after(right_col, false);

            // Skip over empty selections
            if !info.is_empty() && left_col == right_col {
                continue;
            }

            // TODO: What affinity should these use?
            let x0 = view
                .line_point_of_line_col(line, left_col, CursorAffinity::Forward)
                .x;
            let x1 = view
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x;
            // TODO(minor): Should this be line != end_line?
            let x1 = if rvline != end_rvline {
                x1 + CHAR_WIDTH
            } else {
                x1
            };

            let (x0, width) = if info.is_empty() {
                let text_layout = view.get_text_layout(line);
                let width = text_layout
                    .get_layout_x(rvline.line_index)
                    .map(|(_, x1)| x1)
                    .unwrap_or(0.0)
                    .into();
                (0.0, width)
            } else {
                (x0, x1 - x0)
            };

            let rect = Rect::from_origin_size((x0, vline_y), (width, line_height));
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_linewise_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        line_height: f64,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
    ) {
        let view = &self.editor.view;
        let viewport = self.viewport.get_untracked();

        let (start_rvline, _) = view.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, _) = view.rvline_col_of_offset(end_offset, affinity);
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

            let phantom_text = view.line_phantom_text(line);

            // The left column is always 0 for linewise selections.
            let right_col = view.last_col(info, true);
            let right_col = phantom_text.col_after(right_col, false);

            // TODO: what affinity to use?
            let x1 = view
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x
                + CHAR_WIDTH;

            let rect = Rect::from_origin_size(
                (viewport.x0, vline_y),
                (x1 - viewport.x0, line_height),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_blockwise_selection(
        &self,
        cx: &mut PaintCx,
        color: Color,
        line_height: f64,
        screen_lines: &ScreenLines,
        start_offset: usize,
        end_offset: usize,
        affinity: CursorAffinity,
        horiz: Option<ColPosition>,
    ) {
        let view = &self.editor.view;

        let (start_rvline, start_col) =
            view.rvline_col_of_offset(start_offset, affinity);
        let (end_rvline, end_col) = view.rvline_col_of_offset(end_offset, affinity);
        let left_col = start_col.min(end_col);
        let right_col = start_col.max(end_col) + 1;

        let lines = screen_lines
            .iter_line_info_r(start_rvline..=end_rvline)
            .filter_map(|line_info| {
                let max_col = view.last_col(line_info.vline_info, true);
                (max_col > left_col).then_some((line_info, max_col))
            });

        for (line_info, max_col) in lines {
            let line = line_info.vline_info.rvline.line;
            let right_col = if let Some(ColPosition::End) = horiz {
                max_col
            } else {
                right_col.min(max_col)
            };
            let phantom_text = view.line_phantom_text(line);
            let left_col = phantom_text.col_after(left_col, true);
            let right_col = phantom_text.col_after(right_col, false);

            // TODO: what affinity to use?
            let x0 = view
                .line_point_of_line_col(line, left_col, CursorAffinity::Forward)
                .x;
            let x1 = view
                .line_point_of_line_col(line, right_col, CursorAffinity::Backward)
                .x;

            let rect = Rect::from_origin_size(
                (x0, line_info.vline_y),
                (x1 - x0, line_height),
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
        let view = &self.editor.view;
        let cursor = self.editor.cursor;
        let find_focus = self.editor.find_focus;
        let hide_cursor = self.editor.common.window_common.hide_cursor;
        let config = self.editor.common.config;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let viewport = self.viewport.get_untracked();
        let is_active =
            self.is_active.get_untracked() && !find_focus.get_untracked();

        let current_line_color = config.color(LapceColor::EDITOR_CURRENT_LINE);
        let selection_color = config.color(LapceColor::EDITOR_SELECTION);
        let caret_color = config.color(LapceColor::EDITOR_CARET);

        let breakline = self.debug_breakline.get_untracked().and_then(
            |(breakline, breakline_path)| {
                if view
                    .doc
                    .get_untracked()
                    .content
                    .with_untracked(|c| c.path() == Some(&breakline_path))
                {
                    Some(breakline)
                } else {
                    None
                }
            },
        );

        // TODO: check if this is correct
        if let Some(breakline) = breakline {
            if let Some(info) = screen_lines.info_for_line(breakline) {
                let rect = Rect::from_origin_size(
                    (viewport.x0, info.vline_y),
                    (viewport.width(), line_height),
                );

                cx.fill(
                    &rect,
                    config.color(LapceColor::EDITOR_DEBUG_BREAK_LINE),
                    0.0,
                );
            }
        }

        cursor.with_untracked(|cursor| {
            let highlight_current_line = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Insert(_) => true,
                CursorMode::Visual { .. } => false,
            };

            // Highlight the current line
            if !is_local && highlight_current_line {
                for (_, end) in cursor.regions_iter() {
                    // TODO: unsure if this is correct for wrapping lines
                    let rvline = view.rvline_of_offset(end, cursor.affinity);

                    if let Some(info) = screen_lines
                        .info(rvline)
                        .filter(|_| Some(rvline.line) != breakline)
                    {
                        let rect = Rect::from_origin_size(
                            (viewport.x0, info.vline_y),
                            (viewport.width(), line_height),
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
                    let end_offset =
                        view.move_right(start.max(end), Mode::Insert, 1);

                    self.paint_normal_selection(
                        cx,
                        selection_color,
                        line_height,
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
                        line_height,
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
                        line_height,
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
                            line_height,
                            screen_lines,
                            start.min(end),
                            start.max(end),
                            cursor.affinity,
                            false,
                        );
                    }
                }
            }

            if is_active && !hide_cursor.get_untracked() {
                for (_, end) in cursor.regions_iter() {
                    let is_block = match cursor.mode {
                        CursorMode::Normal(_) | CursorMode::Visual { .. } => true,
                        CursorMode::Insert(_) => false,
                    };
                    let LineRegion { x, width, rvline } =
                        cursor_caret(view, end, is_block, cursor.affinity);

                    if let Some(info) = screen_lines.info(rvline) {
                        let rect = Rect::from_origin_size(
                            (x, info.vline_y),
                            (width, line_height),
                        );
                        cx.fill(&rect, caret_color, 0.0);
                    }
                }
            }
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
        let view = self.editor.view.clone();
        let config = self.editor.common.config;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        // TODO: cache indent text layout
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

        for (line, y) in screen_lines.iter_lines_y() {
            let text_layout = view.get_text_layout(line);

            self.paint_extra_style(cx, &text_layout.extra_style, y, viewport);

            if let Some(whitespaces) = &text_layout.whitespaces {
                let family: Vec<FamilyOwned> =
                    FamilyOwned::parse_list(&config.editor.font_family).collect();
                let attrs = Attrs::new()
                    .color(config.color(LapceColor::EDITOR_VISIBLE_WHITESPACE))
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
                            cx.draw_text(&tab_text, Point::new(*x0, y));
                        }
                        ' ' => {
                            cx.draw_text(&space_text, Point::new(*x0, y));
                        }
                        _ => {}
                    }
                }
            }

            if config.editor.show_indent_guide {
                let mut x = 0.0;
                while x + 1.0 < text_layout.indent {
                    cx.stroke(
                        &Line::new(Point::new(x, y), Point::new(x, y + line_height)),
                        config.color(LapceColor::EDITOR_INDENT_GUIDE),
                        1.0,
                    );
                    x += indent_text_width;
                }
            }

            cx.draw_text(&text_layout.text, Point::new(0.0, y));
        }
    }

    fn paint_find(&self, cx: &mut PaintCx, screen_lines: &ScreenLines) {
        let visual = self.editor.common.find.visual;
        if !visual.get_untracked() {
            return;
        }
        if screen_lines.lines.is_empty() {
            return;
        }

        let min_vline = *screen_lines.lines.first().unwrap();
        let max_vline = *screen_lines.lines.last().unwrap();
        let min_line = screen_lines.info(min_vline).unwrap().vline_info.rvline.line;
        let max_line = screen_lines.info(max_vline).unwrap().vline_info.rvline.line;

        let view = self.editor.view.clone();
        let config = self.editor.common.config;
        let occurrences = view.find_result().occurrences;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        view.update_find();
        let start = view.offset_of_line(min_line);
        let end = view.offset_of_line(max_line + 1);

        // TODO: The selection rect creation logic for find is quite similar to the version
        // within insert cursor. It would be good to deduplicate it.
        let mut rects = Vec::new();
        for region in occurrences.with_untracked(|selection| {
            selection.regions_in_range(start, end).to_vec()
        }) {
            let start = region.min();
            let end = region.max();

            // TODO(minor): the proper affinity here should probably be tracked by selregion
            let (start_rvline, start_col) =
                view.rvline_col_of_offset(start, CursorAffinity::Forward);
            let (end_rvline, end_col) =
                view.rvline_col_of_offset(end, CursorAffinity::Backward);

            for line_info in screen_lines.iter_line_info() {
                let rvline_info = line_info.vline_info;
                let rvline = rvline_info.rvline;
                let line = rvline.line;

                if rvline < start_rvline {
                    continue;
                }

                if rvline > end_rvline {
                    break;
                }

                let phantom_text = view.line_phantom_text(line);

                let left_col = if rvline == start_rvline { start_col } else { 0 };
                let (right_col, _vline_end) = if rvline == end_rvline {
                    let max_col = view.last_col(rvline_info, true);
                    (end_col.min(max_col), false)
                } else {
                    (view.last_col(rvline_info, true), true)
                };

                // Shift it by the phantom text
                let left_col = phantom_text.col_after(left_col, false);
                let right_col = phantom_text.col_after(right_col, false);

                // TODO(minor): sel region should have the affinity of the start/end
                let x0 = view
                    .line_point_of_line_col(line, left_col, CursorAffinity::Forward)
                    .x;
                let x1 = view
                    .line_point_of_line_col(
                        line,
                        right_col,
                        CursorAffinity::Backward,
                    )
                    .x;

                if !rvline_info.is_empty() && start != end && left_col != right_col {
                    rects.push(
                        Size::new(x1 - x0, line_height)
                            .to_rect()
                            .with_origin(Point::new(x0, line_info.vline_y)),
                    );
                }
            }
        }

        let color = config.color(LapceColor::EDITOR_FOREGROUND);
        for rect in rects {
            cx.stroke(&rect, color, 1.0);
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }
        if !self.editor.view.kind.get_untracked().is_normal() {
            return;
        }

        let line_height = config.editor.line_height();
        let Some(start_vline) = screen_lines.lines.first() else {
            return;
        };
        let start_info = screen_lines.vline_info(*start_vline).unwrap();
        let start_line = start_info.rvline.line;

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

        let area_height = self
            .sticky_header_info
            .sticky_lines
            .iter()
            .copied()
            .map(|line| {
                let layout = self.editor.view.get_text_layout(line);
                layout.line_count() * line_height
            })
            .sum::<usize>() as f64
            - scroll_offset;

        let sticky_area_rect = Size::new(viewport.x1, area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0))
            .inflate(10.0, 0.0);

        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
            3.0,
        );
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
            0.0,
        );

        // Paint lines
        let mut y_accum = 0.0;
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

            let text_layout = self.editor.view.get_text_layout(line);

            let text_height = (text_layout.line_count() * line_height) as f64;
            let height = text_height - y_diff;

            cx.save();

            let line_area_rect = Size::new(viewport.width(), height)
                .to_rect()
                .with_origin(Point::new(viewport.x0, viewport.y0 + y_accum));

            cx.clip(&line_area_rect);

            let y = viewport.y0 - y_diff + y_accum;
            cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

            y_accum += text_height;

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
            config.color(LapceColor::LAPCE_SCROLL_BAR),
            0.0,
        );

        if !self.editor.view.kind.get_untracked().is_normal() {
            return;
        }

        let doc = self.editor.view.doc.get_untracked();
        let total_len = doc.buffer.with_untracked(|buffer| buffer.last_line());
        let changes = doc.head_changes().get_untracked();
        let total_height = viewport.height();
        let total_width = viewport.width();
        let line_height = config.editor.line_height();
        let content_height = if config.editor.scroll_beyond_last_line {
            (total_len * line_height) as f64 + total_height - line_height as f64
        } else {
            (total_len * line_height) as f64
        };

        let colors = changes_colors_all(&self.editor.view, changes);
        for (y, height, _, color) in colors {
            let y = y / content_height * total_height;
            let height = ((height * line_height) as f64 / content_height
                * total_height)
                .max(3.0);
            let rect = Rect::ZERO.with_size(Size::new(3.0, height)).with_origin(
                Point::new(
                    viewport.x0 + total_width - BAR_WIDTH + 1.0,
                    y + viewport.y0,
                ),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    /// Calculate the `x` coordinate of the left edge of the given column on the given line.
    /// If `before_cursor` is `true`, the calculated position will be to the right of any inlay
    /// hints before and adjacent to the given column. Else, the calculated position will be to the
    /// left of any such inlay hints.
    fn calculate_col_x(
        view: &EditorViewData,
        line: usize,
        col: usize,
        affinity: CursorAffinity,
    ) -> f64 {
        let before_cursor = affinity == CursorAffinity::Backward;
        let phantom_text = view.line_phantom_text(line);
        let col = phantom_text.col_after(col, before_cursor);
        view.line_point_of_line_col(line, col, affinity).x
    }

    /// Paint a highlight around the characters at the given positions.
    fn paint_char_highlights(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
        highlight_line_cols: impl Iterator<Item = (RVLine, usize)>,
    ) {
        let view = &self.editor.view;
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        for (rvline, col) in highlight_line_cols {
            // Is the given line on screen?
            if let Some(line_info) = screen_lines.info(rvline) {
                let x0 = Self::calculate_col_x(
                    view,
                    rvline.line,
                    col,
                    CursorAffinity::Backward,
                );
                let x1 = Self::calculate_col_x(
                    view,
                    rvline.line,
                    col + 1,
                    CursorAffinity::Forward,
                );

                let y0 = line_info.vline_y;
                let y1 = y0 + line_height;

                let rect = Rect::new(x0, y0, x1, y1);

                cx.stroke(&rect, config.color(LapceColor::EDITOR_FOREGROUND), 1.0);
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
        let view = &self.editor.view;
        let doc = view.doc.get_untracked();
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let brush = config.color(LapceColor::EDITOR_FOREGROUND);

        if start == end {
            if let Some(line_info) = screen_lines.info(start) {
                // TODO: Due to line wrapping the y positions of these two spots could be different, do we need to change it?
                let x0 = Self::calculate_col_x(
                    view,
                    start.line,
                    start_col + 1,
                    CursorAffinity::Forward,
                );
                let x1 = Self::calculate_col_x(
                    view,
                    end.line,
                    end_col,
                    CursorAffinity::Backward,
                );

                if x0 < x1 {
                    let y = line_info.vline_y + line_height;

                    let p0 = Point::new(x0, y);
                    let p1 = Point::new(x1, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, 1.0);
                }
            }
        } else {
            // Are start_line and end_line on screen?
            let start_line_y = screen_lines
                .info(start)
                .map(|line_info| line_info.vline_y + line_height);
            let end_line_y = screen_lines
                .info(end)
                .map(|line_info| line_info.vline_y + line_height);

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
                    view,
                    start.line,
                    start_col + 1,
                    CursorAffinity::Forward,
                );
                let end_x = Self::calculate_col_x(
                    view,
                    end.line,
                    end_col,
                    CursorAffinity::Backward,
                );

                // TODO(minor): is this correct with line wrapping?
                // The vertical line should be drawn to the left of any non-whitespace characters
                // in the enclosed section.
                let min_text_x = doc.buffer.with_untracked(|buffer| {
                    ((start.line + 1)..=end.line)
                        .filter(|&line| !buffer.is_line_whitespace(line))
                        .map(|line| {
                            let non_blank_offset =
                                buffer.first_non_blank_character_on_line(line);
                            let (_, col) = view.offset_to_line_col(non_blank_offset);

                            Self::calculate_col_x(
                                view,
                                line,
                                col,
                                CursorAffinity::Backward,
                            )
                        })
                        .min_by(f64::total_cmp)
                });

                let min_x = min_text_x.map_or(start_x, |min_text_x| {
                    cmp::min_by(min_text_x, start_x, f64::total_cmp)
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

    /// Paint enclosing bracket highlights and scope lines if the corresponding settings are
    /// enabled.
    fn paint_bracket_highlights_scope_lines(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();

        if config.editor.highlight_matching_brackets
            || config.editor.highlight_scope_lines
        {
            let view = &self.editor.view;
            let offset = self
                .editor
                .cursor
                .with_untracked(|cursor| cursor.mode.offset());

            let bracket_offsets = view
                .doc
                .with_untracked(|doc| doc.find_enclosing_brackets(offset))
                .map(|(start, end)| [start, end]);

            let bracket_line_cols = bracket_offsets.map(|bracket_offsets| {
                bracket_offsets.map(|offset| {
                    let (rvline, col) =
                        view.rvline_col_of_offset(offset, CursorAffinity::Forward);
                    (rvline, col)
                })
            });

            if config.editor.highlight_matching_brackets {
                self.paint_char_highlights(
                    cx,
                    screen_lines,
                    bracket_line_cols.into_iter().flatten(),
                );
            }

            if config.editor.highlight_scope_lines {
                if let Some([start_line_col, end_line_col]) = bracket_line_cols {
                    self.paint_scope_lines(
                        cx,
                        viewport,
                        screen_lines,
                        start_line_col,
                        end_line_col,
                    );
                }
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
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast() {
            self.sticky_header_info = *state;
            cx.request_layout(self.id);
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

            let config = self.editor.common.config.get_untracked();
            let line_height = config.editor.line_height() as f64;

            let width = self.editor.view.max_line_width() + 20.0;
            let height = line_height * self.editor.view.last_vline().get() as f64;

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
        if self.viewport.with_untracked(|v| v != &viewport) {
            self.viewport.set(viewport);
        }
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let viewport = self.viewport.get_untracked();
        let config = self.editor.common.config.get_untracked();
        let doc = self.editor.view.doc.get_untracked();
        let is_local = doc.content.with_untracked(|content| content.is_local());

        // We repeatedly get the screen lines because we don't currently carefully manage the
        // paint functions to avoid potentially needing to recompute them, which could *maybe*
        // make them invalid.
        // TODO: One way to get around the above issue would be to more careful, since we
        // technically don't need to stop it from *recomputing* just stop any possible changes, but
        // avoiding recomputation seems easiest/clearest.
        // I expect that most/all of the paint functions could restrict themselves to only what is
        // within the active screen lines without issue.
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_cursor(cx, is_local, &screen_lines);
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_diff_sections(cx, viewport, &screen_lines, &config);
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_find(cx, &screen_lines);
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_bracket_highlights_scope_lines(cx, viewport, &screen_lines);
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_text(cx, viewport, &screen_lines);
        let screen_lines = self.editor.screen_lines().get_untracked();
        self.paint_sticky_headers(cx, viewport, &screen_lines);
        self.paint_scroll_bar(cx, viewport, is_local, config);
    }
}

fn get_sticky_header_info(
    view: &EditorViewData,
    doc: Rc<Document>,
    viewport: RwSignal<Rect>,
    sticky_header_height_signal: RwSignal<f64>,
    config: &LapceConfig,
) -> StickyHeaderInfo {
    let viewport = viewport.get();
    // TODO(minor): should this be a `get`
    let screen_lines = view.screen_lines.get();
    let line_height = config.editor.line_height() as f64;
    // let start_line = (viewport.y0 / line_height).floor() as usize;
    let Some(start) = screen_lines.lines.first() else {
        return StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        };
    };
    let start_info = screen_lines.info(*start).unwrap();
    let start_line = start_info.vline_info.rvline.line;

    let y_diff = viewport.y0 - start_info.vline_y;

    let mut last_sticky_should_scroll = false;
    let mut sticky_lines = Vec::new();
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

    let sticky_header_height = sticky_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // TODO(question): won't y_diff always be scroll_offset here? so we should just sub on
            // the outside
            let y_diff = if i == total_sticky_lines - 1 {
                scroll_offset
            } else {
                0.0
            };

            let layout = view.get_text_layout(*line);
            layout.line_count() as f64 * line_height - y_diff
        })
        .sum();

    sticky_header_height_signal.set(sticky_header_height);
    StickyHeaderInfo {
        sticky_lines,
        last_sticky_should_scroll,
        y_diff,
    }
}

#[derive(Clone, Debug)]
pub struct LineRegion {
    pub x: f64,
    pub width: f64,
    pub rvline: RVLine,
}

/// Get the render information for a caret cursor at the given `offset`.  
pub fn cursor_caret(
    view: &EditorViewData,
    offset: usize,
    block: bool,
    affinity: CursorAffinity,
) -> LineRegion {
    let info = view.rvline_info_of_offset(offset, affinity);
    let (_, col) = view.offset_to_line_col(offset);
    let after_last_char = col == view.line_end_col(info.rvline.line, true);

    let doc = view.doc.get_untracked();
    let preedit_start = doc
        .preedit
        .with_untracked(|preedit| {
            preedit.as_ref().and_then(|preedit| {
                let preedit_line = doc
                    .buffer
                    .with_untracked(|b| b.line_of_offset(preedit.offset));
                preedit.cursor.map(|x| (preedit_line, x))
            })
        })
        .filter(|(preedit_line, _)| *preedit_line == info.rvline.line)
        .map(|(_, (start, _))| start);

    let phantom_text = view.line_phantom_text(info.rvline.line);

    let (_, col) = view.offset_to_line_col(offset);
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

    let point = view.line_point_of_line_col(info.rvline.line, col, affinity);

    let rvline = if preedit_start.is_some() {
        // If there's an IME edit, then we need to use the point's y to get the actual y position
        // that the IME cursor is at. Since it could be in the middle of the IME phantom text
        // TODO(minor): is there a cleaner way of doing this? It also won't work nicely with
        // varying line heights.
        let y = point.y;
        let config = view.config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        let line_index = (y / line_height).floor() as usize;
        RVLine::new(info.rvline.line, line_index)
    } else {
        info.rvline
    };

    let x0 = point.x;
    if block {
        let width = if after_last_char {
            CHAR_WIDTH
        } else {
            let x1 = view
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
    window_tab_data: Rc<WindowTabData>,
    workspace: Arc<LapceWorkspace>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    editor: RwSignal<Rc<EditorData>>,
) -> impl View {
    let (editor_id, find_focus, sticky_header_height, editor_view, config) = editor
        .with_untracked(|editor| {
            (
                editor.editor_id,
                editor.find_focus,
                editor.sticky_header_height,
                editor.view.kind,
                editor.common.config,
            )
        });

    let main_split = window_tab_data.main_split.clone();
    let editors = main_split.editors;
    let scratch_docs = main_split.scratch_docs;
    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;
    let debug_breakline = window_tab_data.terminal.breakline;

    let editor_rect = create_rw_signal(Rect::ZERO);

    stack((
        editor_breadcrumbs(workspace, editor.get_untracked(), config),
        container(
            stack((
                editor_gutter(window_tab_data.clone(), editor, is_active),
                container(editor_content(editor, debug_breakline, is_active))
                    .style(move |s| s.size_pct(100.0, 100.0)),
                empty().style(move |s| {
                    let config = config.get();
                    s.absolute()
                        .width_pct(100.0)
                        .height(sticky_header_height.get() as f32)
                        // .box_shadow_blur(5.0)
                        // .border_bottom(1.0)
                        // .border_color(
                        //     config.get_color(LapceColor::LAPCE_BORDER),
                        // )
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
            ))
            .on_resize(move |rect| {
                editor_rect.set(rect);
            })
            .style(|s| s.absolute().size_pct(100.0, 100.0)),
        )
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .on_cleanup(move || {
        if editors.with_untracked(|editors| editors.contains_key(&editor_id)) {
            // editor still exist, so it might be moved to a different editor tab
            return;
        }
        let editor = editor.get_untracked();
        let doc = editor.view.doc.get_untracked();
        editor.scope.dispose();

        let scratch_doc_name =
            if let DocContent::Scratch { name, .. } = doc.content.get_untracked() {
                Some(name.to_string())
            } else {
                None
            };
        if let Some(name) = scratch_doc_name {
            if !scratch_docs
                .with_untracked(|scratch_docs| scratch_docs.contains_key(&name))
            {
                doc.scope.dispose();
            }
        }
    })
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

fn editor_gutter(
    window_tab_data: Rc<WindowTabData>,
    editor: RwSignal<Rc<EditorData>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let screen_lines = editor.with_untracked(|x| x.screen_lines());
    let breakpoints = window_tab_data.terminal.debug.breakpoints;
    let daps = window_tab_data.terminal.debug.daps;

    let padding_left = 25.0;
    let padding_right = 30.0;

    let (view, cursor, viewport, scroll_delta, config) =
        editor.with_untracked(|e| {
            (
                e.view.clone(),
                e.cursor,
                e.viewport,
                e.scroll_delta,
                e.common.config,
            )
        });
    let doc = view.doc;

    let num_display_lines = create_memo(move |_| {
        let screen_lines = screen_lines.get();
        screen_lines.lines.len()
        // let viewport = viewport.get();
        // let line_height = config.get().editor.line_height() as f64;
        // (viewport.height() / line_height).ceil() as usize + 1
    });

    let code_action_vline = create_memo(move |_| {
        if is_active(true) {
            let doc = doc.get();
            let (offset, affinity) =
                cursor.with(|cursor| (cursor.offset(), cursor.affinity));
            let has_code_actions = doc
                .code_actions()
                .with(|c| c.get(&offset).map(|c| !c.1.is_empty()).unwrap_or(false));
            if has_code_actions {
                let vline = view.vline_of_offset(offset, affinity);
                Some(vline)
            } else {
                None
            }
        } else {
            None
        }
    });

    let gutter_rect = create_rw_signal(Rect::ZERO);
    let gutter_width = create_memo(move |_| gutter_rect.get().width());

    let breakpoints_view = move |i: usize| {
        let hovered = create_rw_signal(false);
        container(
            svg(move || config.get().ui_svg(LapceIcons::DEBUG_BREAKPOINT)).style(
                move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32 + 2.0;
                    s.size(size, size)
                        .color(config.color(LapceColor::DEBUG_BREAKPOINT_HOVER))
                        .apply_if(!hovered.get(), |s| s.hide())
                },
            ),
        )
        .on_click_stop(move |_| {
            let screen_lines = screen_lines.get_untracked();
            let line = screen_lines.lines.get(i).map(|r| r.line).unwrap_or(0);
            // let line = (viewport.get_untracked().y0
            //     / config.get_untracked().editor.line_height() as f64)
            //     .floor() as usize
            //     + i;
            let editor = editor.get_untracked();
            let doc = editor.view.doc.get_untracked();
            let offset = doc.buffer.with_untracked(|b| b.offset_of_line(line));
            if let Some(path) = doc.content.get_untracked().path() {
                let path_breakpoints = breakpoints
                    .try_update(|breakpoints| {
                        let breakpoints =
                            breakpoints.entry(path.clone()).or_default();
                        if let std::collections::btree_map::Entry::Vacant(e) =
                            breakpoints.entry(line)
                        {
                            e.insert(LapceBreakpoint {
                                id: None,
                                verified: false,
                                message: None,
                                line,
                                offset,
                                dap_line: None,
                                active: true,
                            });
                        } else {
                            let mut toggle_active = false;
                            if let Some(breakpint) = breakpoints.get_mut(&line) {
                                if !breakpint.active {
                                    breakpint.active = true;
                                    toggle_active = true;
                                }
                            }
                            if !toggle_active {
                                breakpoints.remove(&line);
                            }
                        }
                        breakpoints.clone()
                    })
                    .unwrap();
                let source_breakpoints: Vec<SourceBreakpoint> = path_breakpoints
                    .iter()
                    .filter_map(|(_, b)| {
                        if b.active {
                            Some(SourceBreakpoint {
                                line: b.line + 1,
                                column: None,
                                condition: None,
                                hit_condition: None,
                                log_message: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let daps: Vec<DapId> =
                    daps.with_untracked(|daps| daps.keys().cloned().collect());
                for dap_id in daps {
                    editor.common.proxy.dap_set_breakpoints(
                        dap_id,
                        path.to_path_buf(),
                        source_breakpoints.clone(),
                    );
                }
            }
        })
        .on_event_stop(EventListener::PointerEnter, move |_| {
            hovered.set(true);
        })
        .on_event_stop(EventListener::PointerLeave, move |_| {
            hovered.set(false);
        })
        .style(move |s| {
            s.width(padding_left)
                .height(config.get().editor.line_height() as f32)
                .justify_center()
                .items_center()
                .cursor(CursorStyle::Pointer)
        })
    };

    stack((
        stack((
            empty().style(move |s| s.width(padding_left)),
            label(move || {
                let doc = doc.get();
                doc.buffer.with(|b| b.last_line() + 1).to_string()
            }),
            empty().style(move |s| s.width(padding_right)),
        ))
        .style(|s| s.height_pct(100.0)),
        clip(
            stack((
                dyn_stack(
                    move || {
                        let num = num_display_lines.get();
                        0..num
                    },
                    move |i| *i,
                    breakpoints_view,
                )
                .style(move |s| {
                    s.absolute().flex_col().margin_top(
                        -(viewport.get().y0
                            % config.get().editor.line_height() as f64)
                            as f32,
                    )
                }),
                dyn_stack(
                    move || {
                        let editor = editor.get();
                        let doc = editor.view.doc.get();
                        let content = doc.content.get();
                        let breakpoints = if let Some(path) = content.path() {
                            breakpoints
                                .with(|b| b.get(path).cloned())
                                .unwrap_or_default()
                        } else {
                            Default::default()
                        };
                        breakpoints.into_iter()
                    },
                    move |(line, b)| (*line, b.active),
                    move |(line, breakpoint)| {
                        let active = breakpoint.active;
                        let line_y = screen_lines
                            .with_untracked(|s| s.info_for_line(line))
                            .map(|l| l.y)
                            .unwrap_or_default();
                        container(
                            svg(move || {
                                config.get().ui_svg(LapceIcons::DEBUG_BREAKPOINT)
                            })
                            .style(move |s| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32 + 2.0;
                                let color = if active {
                                    LapceColor::DEBUG_BREAKPOINT
                                } else {
                                    LapceColor::EDITOR_DIM
                                };
                                let color = config.color(color);
                                s.size(size, size).color(color)
                            }),
                        )
                        .style(move |s| {
                            let config = config.get();
                            s.absolute()
                                .width(padding_left)
                                .height(config.editor.line_height() as f32)
                                .justify_center()
                                .items_center()
                                .margin_top(line_y as f32 - viewport.get().y0 as f32)
                        })
                    },
                )
                .style(|s| s.absolute().size_pct(100.0, 100.0)),
            ))
            .style(|s| s.size_pct(100.0, 100.0)),
        )
        .style(move |s| {
            s.absolute()
                .size_pct(100.0, 100.0)
                .background(config.get().color(LapceColor::EDITOR_BACKGROUND))
        }),
        clip(
            stack((
                editor_gutter_view(editor.get_untracked())
                    .on_resize(move |rect| {
                        gutter_rect.set(rect);
                    })
                    .on_event_stop(EventListener::PointerWheel, move |event| {
                        if let Event::PointerWheel(pointer_event) = event {
                            scroll_delta.set(pointer_event.delta);
                        }
                    })
                    .style(|s| s.size_pct(100.0, 100.0)),
                container(
                    svg(move || config.get().ui_svg(LapceIcons::LIGHTBULB)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .color(config.color(LapceColor::LAPCE_WARN))
                        },
                    ),
                )
                .on_click_stop(move |_| {
                    editor.get_untracked().show_code_actions(true);
                })
                .style(move |s| {
                    let config = config.get();
                    let viewport = viewport.get();
                    let gutter_width = gutter_width.get();
                    let code_action_vline = code_action_vline.get();
                    let size = config.ui.icon_size() as f32;
                    let margin_left =
                        gutter_width as f32 + (padding_right - size) / 2.0 - 4.0;
                    let line_height = config.editor.line_height();
                    let margin_top = if let Some(vline) = code_action_vline {
                        (vline.get() * line_height) as f32 - viewport.y0 as f32
                            + (line_height as f32 - size) / 2.0
                            - 4.0
                    } else {
                        0.0
                    };
                    s.absolute()
                        .padding(4.0)
                        .border_radius(6.0)
                        .margin_left(margin_left)
                        .margin_top(margin_top)
                        .apply_if(code_action_vline.is_none(), |s| s.hide())
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .active(|s| {
                            s.background(
                                config.color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ),
                            )
                        })
                }),
            ))
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

fn editor_breadcrumbs(
    workspace: Arc<LapceWorkspace>,
    editor: Rc<EditorData>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let doc = editor.view.doc;
    let doc_path = create_memo(move |_| {
        let doc = doc.get();
        let content = doc.content.get();
        if let DocContent::History(history) = &content {
            Some(history.path.clone())
        } else {
            content.path().cloned()
        }
    });
    container(
        scroll(
            stack((
                {
                    let workspace = workspace.clone();
                    dyn_stack(
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
                                .filter_map(|path| {
                                    Some(
                                        path.file_name()?
                                            .to_string_lossy()
                                            .into_owned(),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .enumerate()
                        },
                        |(i, section)| (*i, section.to_string()),
                        move |(i, section)| {
                            stack((
                                svg(move || {
                                    config
                                        .get()
                                        .ui_svg(LapceIcons::BREADCRUMB_SEPARATOR)
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    s.apply_if(i == 0, |s| s.hide())
                                        .size(size, size)
                                        .color(
                                            config.color(
                                                LapceColor::LAPCE_ICON_ACTIVE,
                                            ),
                                        )
                                }),
                                label(move || section.clone()),
                            ))
                            .style(|s| s.items_center())
                        },
                    )
                    .style(|s| s.padding_horiz(10.0))
                },
                label(move || {
                    let doc = doc.get();
                    if let DocContent::History(history) = doc.content.get() {
                        format!("({})", history.version)
                    } else {
                        "".to_string()
                    }
                })
                .style(move |s| {
                    let doc = doc.get();
                    let is_history = doc.content.with_untracked(|content| {
                        matches!(content, DocContent::History(_))
                    });

                    s.padding_right(10.0).apply_if(!is_history, |s| s.hide())
                }),
            ))
            .style(|s| s.items_center()),
        )
        .on_scroll_to(move || {
            doc.track();
            Some(Point::new(3000.0, 0.0))
        })
        .hide_bar(|| true)
        .style(move |s| {
            s.absolute()
                .size_pct(100.0, 100.0)
                .border_bottom(1.0)
                .border_color(config.get().color(LapceColor::LAPCE_BORDER))
                .items_center()
        }),
    )
    .style(move |s| {
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        s.items_center()
            .width_pct(100.0)
            .height(line_height as f32)
            .apply_if(doc_path.get().is_none(), |s| s.hide())
    })
}

fn editor_content(
    editor: RwSignal<Rc<EditorData>>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
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

    scroll({
        let editor_content_view =
            editor_view(editor.get_untracked(), debug_breakline, is_active).style(
                move |s| {
                    let config = config.get();
                    let padding_bottom = if config.editor.scroll_beyond_last_line {
                        viewport.get().height() as f32
                            - config.editor.line_height() as f32
                    } else {
                        0.0
                    };
                    s.absolute()
                        .padding_bottom(padding_bottom)
                        .cursor(CursorStyle::Text)
                        .min_size_pct(100.0, 100.0)
                },
            );
        let id = editor_content_view.id();
        editor_content_view
            .on_event_cont(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
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
            .on_event_stop(EventListener::PointerLeave, move |event| {
                if let Event::PointerLeave = event {
                    editor.get_untracked().pointer_leave();
                }
            })
    })
    .on_move(move |point| {
        window_origin.set(point);
    })
    .on_scroll_to(move || scroll_to.get().map(|s| s.to_point()))
    .on_scroll_delta(move || scroll_delta.get())
    .on_ensure_visible(move || {
        let editor = editor.get_untracked();
        let cursor = cursor.get();
        let offset = cursor.offset();
        editor.view.doc.track();
        editor.view.kind.track();

        let LineRegion { x, width, rvline } =
            cursor_caret(&editor.view, offset, !cursor.is_insert(), cursor.affinity);
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        // TODO: is there a good way to avoid the calculation of the vline here?
        let vline = editor.view.vline_of_rvline(rvline);
        let rect = Rect::from_origin_size(
            (x, (vline.get() * line_height) as f64),
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
            rect.y0 -= (config.editor.cursor_surrounding_lines * line_height) as f64
                + sticky_header_height.get_untracked();
            rect.y1 += (config.editor.cursor_surrounding_lines * line_height) as f64;
            rect
        }
    })
    .style(|s| s.absolute().size_pct(100.0, 100.0))
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

    stack((
        text_input(find_editor, move || {
            is_active(true)
                && visual.get()
                && find_focus.get()
                && !replace_focus.get()
        })
        .on_event_cont(EventListener::PointerDown, move |_| {
            find_focus.set(true);
            replace_focus.set(false);
        })
        .style(|s| s.width_pct(100.0)),
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
        .style(|s| s.padding_vert(4.0)),
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
        .style(|s| s.padding_left(6.0)),
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
        .style(|s| s.padding_horiz(6.0)),
    ))
    .style(move |s| {
        let config = config.get();
        s.width(200.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
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

    stack((
        text_input(replace_editor, move || {
            is_active(true)
                && visual.get()
                && find_focus.get()
                && replace_active.get()
                && replace_focus.get()
        })
        .on_event_cont(EventListener::PointerDown, move |_| {
            find_focus.set(true);
            replace_focus.set(true);
        })
        .style(|s| s.width_pct(100.0)),
        empty().style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32 + 10.0;
            s.size(0.0, size).padding_vert(4.0)
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.width(200.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}

fn find_view(
    editor: RwSignal<Rc<EditorData>>,
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let config = find_editor.common.config;
    let find_visual = find_editor.common.find.visual;
    let replace_doc = replace_editor.view.doc;
    let focus = find_editor.common.focus;

    let find_pos = create_memo(move |_| {
        let visual = find_visual.get();
        if !visual {
            return (0, 0);
        }
        let editor = editor.get_untracked();
        let cursor = editor.cursor;
        let offset = cursor.with(|cursor| cursor.offset());
        let occurrences = editor.view.doc.get().backend.find_result.occurrences;
        occurrences.with(|occurrences| {
            for (i, region) in occurrences.regions().iter().enumerate() {
                if offset <= region.max() {
                    return (i + 1, occurrences.regions().len());
                }
            }
            (occurrences.regions().len(), occurrences.regions().len())
        })
    });

    container(
        stack((
            stack((
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
                .style(|s| s.padding_horiz(6.0)),
                search_editor_view(
                    find_editor,
                    find_focus,
                    is_active,
                    replace_focus,
                ),
                label(move || {
                    let (current, all) = find_pos.get();
                    if all == 0 {
                        "No Results".to_string()
                    } else {
                        format!("{current} of {all}")
                    }
                })
                .style(|s| s.margin_left(6.0).min_width(70.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_BACKWARD,
                    move || {
                        editor
                            .get_untracked()
                            .search_backward(ModifiersState::empty());
                    },
                    move || false,
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_FORWARD,
                    move || {
                        editor
                            .get_untracked()
                            .search_forward(ModifiersState::empty());
                    },
                    move || false,
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        editor.get_untracked().clear_search();
                    },
                    move || false,
                    || false,
                    config,
                )
                .style(|s| s.padding_horiz(6.0)),
            ))
            .style(|s| s.items_center()),
            stack((
                empty().style(move |s| {
                    let config = config.get();
                    let width = config.ui.icon_size() as f32 + 10.0 + 6.0 * 2.0;
                    s.width(width)
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
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_next(&text);
                    },
                    move || false,
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_REPLACE_ALL,
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_all(&text);
                    },
                    move || false,
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
            ))
            .style(move |s| {
                s.items_center()
                    .margin_top(4.0)
                    .apply_if(!replace_active.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.margin_right(50.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border_radius(6.0)
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .padding_vert(4.0)
                .cursor(CursorStyle::Default)
                .flex_col()
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            let editor = editor.get_untracked();
            if let Some(editor_tab_id) = editor.editor_tab_id.get_untracked() {
                editor
                    .common
                    .internal_command
                    .send(InternalCommand::FocusEditorTab { editor_tab_id });
            }
            focus.set(Focus::Workbench);
        }),
    )
    .style(move |s| {
        s.absolute()
            .margin_top(-1.0)
            .width_pct(100.0)
            .justify_end()
            .apply_if(!find_visual.get(), |s| s.hide())
    })
}

/// Iterator over (len, color, modified) for each change in the diff
fn changes_color_iter<'a>(
    changes: &'a im::Vector<DiffLines>,
    config: &'a LapceConfig,
) -> impl Iterator<Item = (usize, Option<Color>, bool)> + 'a {
    let mut last_change = None;
    changes.iter().map(move |change| {
        let len = match change {
            DiffLines::Left(_range) => 0,
            DiffLines::Both(info) => info.right.len(),
            DiffLines::Right(range) => range.len(),
        };
        let mut modified = false;
        let color = match change {
            DiffLines::Left(_range) => {
                Some(config.color(LapceColor::SOURCE_CONTROL_REMOVED))
            }
            DiffLines::Right(_range) => {
                if let Some(DiffLines::Left(_)) = last_change.as_ref() {
                    modified = true;
                }
                if modified {
                    Some(config.color(LapceColor::SOURCE_CONTROL_MODIFIED))
                } else {
                    Some(config.color(LapceColor::SOURCE_CONTROL_ADDED))
                }
            }
            _ => None,
        };

        last_change = Some(change.clone());

        (len, color, modified)
    })
}

// TODO: both of the changes color functions could easily return iterators

/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_screen(
    view: &EditorViewData,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let screen_lines = view.screen_lines.get_untracked();
    let config = view.config.get_untracked();

    let Some((min, max)) = screen_lines.rvline_range() else {
        return Vec::new();
    };

    let mut line = 0;
    let mut colors = Vec::new();

    for (len, color, modified) in changes_color_iter(&changes, &config) {
        let pre_line = line;

        line += len;
        if line < min.line {
            continue;
        }

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            let Some(info) = screen_lines.info_for_line(pre_line) else {
                continue;
            };

            let y = info.vline_y;
            let height = {
                // Accumulate the number of line indices each potentially wrapped line spans
                let rvline = info.vline_info.rvline;
                let end_line = rvline.line + len;

                view.iter_rvlines_over(false, rvline, end_line).count()
            };
            let removed = len == 0;

            colors.push((y, height, removed, color));
        }

        if line > max.line {
            break;
        }
    }

    colors
}

// TODO: limit the visual line that changes are considered past to some reasonable number
// TODO(minor): This could be a `changes_colors_range` with some minor changes, but it isn't needed
/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_all(
    view: &EditorViewData,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let config = view.config.get_untracked();
    let line_height = config.editor.line_height();

    let mut line = 0;
    let mut colors = Vec::new();

    let mut vline_iter = view.iter_vlines(false, VLine(0)).peekable();

    for (len, color, modified) in changes_color_iter(&changes, &config) {
        let pre_line = line;

        line += len;

        // Skip over all vlines that are before the current line
        vline_iter
            .by_ref()
            .peeking_take_while(|info| info.rvline.line < pre_line)
            .count();

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            // Find the info with a line == pre_line
            let Some(info) = vline_iter.peek() else {
                continue;
            };

            let y = info.vline.get() * line_height;
            let end_line = info.rvline.line + len;
            let height = vline_iter
                .by_ref()
                .peeking_take_while(|info| info.rvline.line < end_line)
                .count();
            let removed = len == 0;

            colors.push((y as f64, height, removed, color));
        }

        if vline_iter.peek().is_none() {
            break;
        }
    }

    colors
}
