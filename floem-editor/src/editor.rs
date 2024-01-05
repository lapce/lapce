use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use floem::{
    cosmic_text::{Attrs, AttrsList, LineHeightValue, TextLayout, Wrap},
    kurbo::{Point, Rect, Vec2},
    peniko::Color,
    pointer::{PointerButton, PointerInputEvent, PointerMoveEvent},
    reactive::{batch, untrack, ReadSignal, RwSignal, Scope},
};
use lapce_core::{
    buffer::rope_text::{RopeText, RopeTextVal},
    cursor::{ColPosition, Cursor, CursorAffinity, CursorMode},
    mode::Mode,
    register::Register,
    selection::Selection,
    soft_tab::{snap_to_soft_tab_line_col, SnapDirection},
};
use lapce_xi_rope::Rope;

use crate::{
    id::EditorId,
    layout::TextLayoutLine,
    phantom_text::PhantomTextLine,
    text::Preedit,
    view::{LineInfo, ScreenLinesBase},
    visual_line::{
        hit_position_aff, FontSizeCacheId, LayoutEvent, LineFontSizeProvider, Lines,
        RVLine, ResolvedWrap, TextLayoutProvider, VLine, VLineInfo,
    },
};

use super::{
    color::EditorColor,
    text::{Document, PreeditData, Styling, WrapMethod},
    view::ScreenLines,
};

pub(crate) const CHAR_WIDTH: f64 = 7.5;

/// The data for a specific editor view
#[derive(Clone)]
pub struct Editor {
    cx: Cell<Scope>,
    effects_cx: Cell<Scope>,

    id: EditorId,

    pub active: RwSignal<bool>,

    /// Whether you can edit within this editor.
    pub read_only: RwSignal<bool>,
    /// Whether you can scroll beyond the last line of the document.
    pub scroll_beyond_last_line: RwSignal<bool>,
    pub cursor_surrounding_lines: RwSignal<usize>,

    /// Whether modal mode is enabled
    pub modal: RwSignal<bool>,
    /// Whether line numbers are relative in modal mode
    pub modal_relative_line_numbers: RwSignal<bool>,

    /// Whether to insert the indent that is detected for the file when a tab character
    /// is inputted.
    pub smart_tab: RwSignal<bool>,

    pub(crate) doc: RwSignal<Rc<dyn Document>>,
    pub(crate) style: RwSignal<Rc<dyn Styling>>,

    pub cursor: RwSignal<Cursor>,

    pub viewport: RwSignal<Rect>,

    /// The current scroll position.
    pub scroll_delta: RwSignal<Vec2>,
    pub scroll_to: RwSignal<Option<Vec2>>,

    /// Holds the cache of the lines and provides many utility functions for them.
    lines: Rc<Lines>,
    pub screen_lines: RwSignal<ScreenLines>,

    /// Modal mode register
    pub register: RwSignal<Register>,
    // TODO: this could have the Lapce snippet support built-in
}
impl Editor {
    // TODO: shouldn't this accept an `RwSignal<Rc<dyn Document>>` so that it can listen for
    // changes in other editors?
    // TODO: should we really allow callers to arbitrarily specify the Id? That could open up
    // confusing behavior.
    /// `id` should typically be constructed by [`EditorId::next`]  
    /// `doc`: The backing [`Document`], such as [`TextDocument`]  
    /// `style`: How the editor should be styled, such as [`SimpleStyling`]  
    /// `register` is the modal mode register, which will be created if `None`. You can pass in an
    /// existing signal for it if you wish to share the state between editors.
    pub fn new(
        cx: Scope,
        id: EditorId,
        doc: Rc<dyn Document>,
        style: Rc<dyn Styling>,
        register: Option<RwSignal<Register>>,
    ) -> Rc<Editor> {
        let cx = cx.create_child();

        let viewport = cx.create_rw_signal(Rect::ZERO);
        let modal = false;
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

        let ed = Editor {
            cx: Cell::new(cx),
            effects_cx: Cell::new(cx.create_child()),
            id,
            active: cx.create_rw_signal(false),
            read_only: cx.create_rw_signal(false),
            scroll_beyond_last_line: cx.create_rw_signal(false),
            cursor_surrounding_lines: cx.create_rw_signal(1),
            modal: cx.create_rw_signal(modal),
            modal_relative_line_numbers: cx.create_rw_signal(true),
            smart_tab: cx.create_rw_signal(true),
            doc,
            style,
            cursor,
            viewport,
            scroll_delta: cx.create_rw_signal(Vec2::ZERO),
            scroll_to: cx.create_rw_signal(None),
            lines,
            screen_lines,
            register: register
                .unwrap_or_else(|| cx.create_rw_signal(Register::default())),
        };
        let ed = Rc::new(ed);

        create_view_effects(ed.effects_cx.get(), ed.clone());

        ed
    }

    pub fn id(&self) -> EditorId {
        self.id
    }

    /// Get the document untracked
    pub fn doc(&self) -> Rc<dyn Document> {
        self.doc.get_untracked()
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

    pub fn text_prov(&self) -> EditorTextProv {
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

    fn preedit(&self) -> PreeditData {
        self.doc.with_untracked(|doc| doc.preedit())
    }

    pub fn set_preedit(
        &self,
        text: String,
        cursor: Option<(usize, usize)>,
        offset: usize,
    ) {
        self.preedit().preedit.set(Some(Preedit {
            text,
            cursor,
            offset,
        }));
        // TODO: clear text cache
    }

    pub fn clear_preedit(&self) {
        let preedit = self.preedit();
        if preedit.preedit.with_untracked(|preedit| preedit.is_some()) {
            preedit.preedit.set(None);
            // TODO: clear text cache
        }
    }

    pub fn receive_char(&self, c: &str) {
        self.doc().receive_char(self, c)
    }

    fn compute_screen_lines(&self, base: RwSignal<ScreenLinesBase>) -> ScreenLines {
        // This function *cannot* access `ScreenLines` with how it is currently implemented.
        // This is being called from within an update to screen lines.

        self.doc().compute_screen_lines(self, base)
    }

    /// Default handler for `PointerDown` event
    pub fn pointer_down(&self, pointer_event: &PointerInputEvent) {
        match pointer_event.button {
            PointerButton::Primary => {
                self.active.set(true);
                self.left_click(pointer_event);
            }
            PointerButton::Secondary => {
                self.right_click(pointer_event);
            }
            _ => {}
        }
    }

    fn left_click(&self, pointer_event: &PointerInputEvent) {
        match pointer_event.count {
            1 => {
                self.single_click(pointer_event);
            }
            2 => {
                self.double_click(pointer_event);
            }
            3 => {
                self.triple_click(pointer_event);
            }
            _ => {}
        }
    }

    fn single_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (new_offset, _) = self.offset_of_point(mode, pointer_event.pos);
        self.cursor.update(|cursor| {
            cursor.set_offset(
                new_offset,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    fn double_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (mouse_offset, _) = self.offset_of_point(mode, pointer_event.pos);
        let (start, end) = self.select_word(mouse_offset);

        self.cursor.update(|cursor| {
            cursor.add_region(
                start,
                end,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    fn triple_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (mouse_offset, _) = self.offset_of_point(mode, pointer_event.pos);
        let line = self.line_of_offset(mouse_offset);
        let start = self.offset_of_line(line);
        let end = self.offset_of_line(line + 1);

        self.cursor.update(|cursor| {
            cursor.add_region(
                start,
                end,
                pointer_event.modifiers.shift_key(),
                pointer_event.modifiers.alt_key(),
            )
        });
    }

    pub fn pointer_move(&self, pointer_event: &PointerMoveEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (offset, is_inside) = self.offset_of_point(mode, pointer_event.pos);
        if self.active.get_untracked()
            && self.cursor.with_untracked(|c| c.offset()) != offset
        {
            self.cursor.update(|cursor| {
                cursor.set_offset(offset, true, pointer_event.modifiers.alt_key())
            });
        }
    }

    pub fn pointer_up(&self, _pointer_event: &PointerInputEvent) {
        self.active.set(false);
    }

    fn right_click(&self, pointer_event: &PointerInputEvent) {
        let mode = self.cursor.with_untracked(|c| c.get_mode());
        let (offset, _) = self.offset_of_point(mode, pointer_event.pos);
        let doc = self.doc();
        let pointer_inside_selection = self
            .cursor
            .with_untracked(|c| c.edit_selection(&doc.rope_text()).contains(offset));
        if !pointer_inside_selection {
            // move cursor to pointer position if outside current selection
            self.single_click(pointer_event);
        }

        // TODO(floem-editor): should we have a default right click context menu?
        // let is_file = doc.content.with_untracked(|content| content.is_file());
        // let mut menu = Menu::new("");
        // let cmds = if is_file {
        //     vec![
        //         Some(CommandKind::Focus(FocusCommand::GotoDefinition)),
        //         Some(CommandKind::Focus(FocusCommand::GotoTypeDefinition)),
        //         None,
        //         Some(CommandKind::Focus(FocusCommand::Rename)),
        //         None,
        //         Some(CommandKind::Edit(EditCommand::ClipboardCut)),
        //         Some(CommandKind::Edit(EditCommand::ClipboardCopy)),
        //         Some(CommandKind::Edit(EditCommand::ClipboardPaste)),
        //         None,
        //         Some(CommandKind::Workbench(
        //             LapceWorkbenchCommand::PaletteCommand,
        //         )),
        //     ]
        // } else {
        //     vec![
        //         Some(CommandKind::Edit(EditCommand::ClipboardCut)),
        //         Some(CommandKind::Edit(EditCommand::ClipboardCopy)),
        //         Some(CommandKind::Edit(EditCommand::ClipboardPaste)),
        //         None,
        //         Some(CommandKind::Workbench(
        //             LapceWorkbenchCommand::PaletteCommand,
        //         )),
        //     ]
        // };
        // let lapce_command = self.common.lapce_command;
        // for cmd in cmds {
        //     if let Some(cmd) = cmd {
        //         menu = menu.entry(
        //             MenuItem::new(cmd.desc().unwrap_or_else(|| cmd.str())).action(
        //                 move || {
        //                     lapce_command.send(LapceCommand {
        //                         kind: cmd.clone(),
        //                         data: None,
        //                     })
        //                 },
        //             ),
        //         );
        //     } else {
        //         menu = menu.separator();
        //     }
        // }
        // show_context_menu(menu, None);
    }

    // === Information ===

    pub fn phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc().phantom_text(line)
    }

    pub fn line_height(&self, line: usize) -> f32 {
        self.style().line_height(line)
    }

    pub fn color(&self, color: EditorColor) -> Color {
        self.style().color(color)
    }

    // === Line Information ===

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

#[derive(Clone)]
pub struct EditorTextProv {
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

        match self.style.wrap() {
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

/// Minimum width that we'll allow the view to be wrapped at.
const MIN_WRAPPED_WIDTH: f32 = 100.0;

/// Create various reactive effects to update the screen lines whenever relevant parts of the view,
/// doc, text layouts, viewport, etc. change.
/// This tries to be smart to a degree.
fn create_view_effects(cx: Scope, ed: Rc<Editor>) {
    // Cloning is fun.
    let ed2 = ed.clone();
    let ed3 = ed.clone();
    let ed4 = ed.clone();

    let update_screen_lines = |ed: &Editor| {
        // This function should not depend on the viewport signal directly.

        // This is wrapped in an update to make any updates-while-updating very obvious
        // which they wouldn't be if we computed and then `set`.
        ed.screen_lines.update(|screen_lines| {
            let new_screen_lines = ed.compute_screen_lines(screen_lines.base);

            *screen_lines = new_screen_lines;
        });
    };

    // Listen for cache revision changes (essentially edits to the file or requiring
    // changes on text layouts, like if diagnostics load in)
    cx.create_effect(move |_| {
        // We can't put this with the other effects because we only want to update screen lines if
        // the cache rev actually changed
        let cache_rev = ed.doc.with(|doc| doc.cache_rev()).get();
        ed.lines.check_cache_rev(cache_rev);
    });

    // Listen for layout events, currently only when a layout is created, and update screen
    // lines based on that
    ed3.lines.layout_event.listen_with(cx, move |val| {
        let view = &ed2;
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
        let ed = &ed3;

        let viewport = ed.viewport.get();

        let wrap = match ed.style.get().wrap() {
            WrapMethod::None => ResolvedWrap::None,
            WrapMethod::EditorWidth => {
                ResolvedWrap::Width((viewport.width() as f32).max(MIN_WRAPPED_WIDTH))
            }
            WrapMethod::WrapColumn { .. } => todo!(),
            WrapMethod::WrapWidth { width } => ResolvedWrap::Width(width),
        };

        ed.lines.set_wrap(wrap);

        // Update the base
        let base = ed.screen_lines.with_untracked(|sl| sl.base);

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

        update_screen_lines(&ed4);
    });
}

pub fn normal_compute_screen_lines(
    editor: &Editor,
    base: RwSignal<ScreenLinesBase>,
) -> ScreenLines {
    let lines = &editor.lines;
    let style = editor.style.get();
    // TODO: don't assume universal line height!
    let line_height = style.line_height(0);

    let (y0, y1) = base
        .with_untracked(|base| (base.active_viewport.y0, base.active_viewport.y1));
    // Get the start and end (visual) lines that are visible in the viewport
    let min_vline = VLine((y0 / line_height as f64).floor() as usize);
    let max_vline = VLine((y1 / line_height as f64).ceil() as usize);

    editor.doc.get().cache_rev().track();
    // TODO(floem-editor): somehow let us track some relevant information like 'loaded' or 'content'?

    let min_info = editor.iter_vlines(false, min_vline).next();
    // TODO: if you need the max vline you probably need the min vline too and so you could grab
    // both in one iter call, which would be more efficient than two iterations
    // let max_info = editor.iter_vlines(false, max_vline).next();

    let mut rvlines = Vec::new();
    let mut info = HashMap::new();

    let Some(min_info) = min_info else {
        return ScreenLines {
            lines: Rc::new(rvlines),
            info: Rc::new(info),
            diff_sections: None,
            base,
        };
    };

    // TODO: the original was min_line..max_line + 1, are we iterating too little now?
    // the iterator is from min_vline..max_vline
    let count = max_vline.get() - min_vline.get();
    // TODO(floem-editor): config id
    let iter = lines
        .iter_rvlines_init(editor.text_prov(), 0, min_info.rvline, false)
        .take(count);

    for (i, vline_info) in iter.enumerate() {
        rvlines.push(vline_info.rvline);

        let line_height = f64::from(style.line_height(vline_info.rvline.line));

        let y_idx = min_vline.get() + i;
        let vline_y = y_idx as f64 * line_height;
        let line_y = vline_y - vline_info.rvline.line_index as f64 * line_height;

        // Add the information to make it cheap to get in the future.
        // This y positions are shifted by the baseline y0
        info.insert(
            vline_info.rvline,
            LineInfo {
                y: line_y - y0,
                vline_y: vline_y - y0,
                vline_info,
            },
        );
    }

    ScreenLines {
        lines: Rc::new(rvlines),
        info: Rc::new(info),
        diff_sections: None,
        base,
    }
}
