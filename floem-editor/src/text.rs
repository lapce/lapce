use std::{borrow::Cow, fmt::Debug, rc::Rc};

use downcast_rs::{impl_downcast, Downcast};
use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, Stretch, Weight},
    keyboard::ModifiersState,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, Scope},
};
use lapce_core::{
    buffer::{
        rope_text::{RopeText, RopeTextVal},
        Buffer,
    },
    cursor::Cursor,
    editor::Action,
    mode::MotionMode,
    register::Register,
    word::WordCursor,
};
use lapce_xi_rope::Rope;
use smallvec::smallvec;

use crate::{
    command::{Command, CommandExecuted},
    editor::{normal_compute_screen_lines, Editor},
    layout::TextLayoutLine,
    phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine},
    view::{ScreenLines, ScreenLinesBase},
};

use super::color::EditorColor;

#[derive(Clone)]
pub struct Preedit {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
    pub offset: usize,
}

/// IME Preedit  
/// This is used for IME input, and must be owned by the `Document`.  
#[derive(Debug, Clone)]
pub struct PreeditData {
    pub preedit: RwSignal<Option<Preedit>>,
}
impl PreeditData {
    pub fn new(cx: Scope) -> PreeditData {
        PreeditData {
            preedit: cx.create_rw_signal(None),
        }
    }
}

/// A document. This holds text.  
pub trait Document: DocumentPhantom + Downcast {
    /// Get the text of the document
    fn text(&self) -> Rope;

    fn rope_text(&self) -> RopeTextVal {
        RopeTextVal::new(self.text())
    }

    fn cache_rev(&self) -> RwSignal<u64>;

    /// Find the next/previous offset of the match of the given character.  
    /// This is intended for use by the [`Movement::NextUnmatched`] and
    /// [`Movement::PreviousUnmatched`] commands.
    fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        let text = self.text();
        let mut cursor = WordCursor::new(&text, offset);
        let new_offset = if previous {
            cursor.previous_unmatched(ch)
        } else {
            cursor.next_unmatched(ch)
        };

        new_offset.unwrap_or(offset)
    }

    /// Find the offset of the matching pair character.  
    /// This is intended for use by the [`Movement::MatchPairs`] command.
    fn find_matching_pair(&self, offset: usize) -> usize {
        WordCursor::new(&self.text(), offset)
            .match_pairs()
            .unwrap_or(offset)
    }

    fn preedit(&self) -> PreeditData;

    // TODO: I don't like passing `under_line` as a parameter but `Document` doesn't have styling
    // should we just move preedit + phantom text into `Styling`?
    fn preedit_phantom(
        &self,
        under_line: Option<Color>,
        line: usize,
    ) -> Option<PhantomText> {
        let preedit = self.preedit().preedit.get_untracked()?;

        let rope_text = self.rope_text();

        let (ime_line, col) = rope_text.offset_to_line_col(preedit.offset);

        if line != ime_line {
            return None;
        }

        Some(PhantomText {
            kind: PhantomTextKind::Ime,
            text: preedit.text,
            col,
            font_size: None,
            fg: None,
            bg: None,
            under_line,
        })
    }

    /// Compute the visible screen lines.  
    /// Note: you should typically *not* need to implement this, unless you have some custom
    /// behavior. Unfortunately this needs an `&self` to be a trait object. So don't call `.update`
    /// on `Self`
    fn compute_screen_lines(
        &self,
        editor: &Editor,
        base: RwSignal<ScreenLinesBase>,
    ) -> ScreenLines {
        normal_compute_screen_lines(editor, base)
    }

    // TODO: pass in the editorid this is from?
    fn run_command(
        &self,
        cmd: &Command,
        count: Option<usize>,
        modifiers: ModifiersState,
    );

    // TODO: We only require this because the standard movement logic wants to call it
    // It would be nice to not require this if you're not using the baseline movement
    // but it would require passing not `&Editor` to movement but either being called with `&Self`
    // or some trait object - but we need some of the editor features for movement. So possibly
    // boxed trait object, or we pass the `&Editor` containing the document to a `Document` callback
    // For now we'll leave it, as it is easy to stub if you aren't using it.
    fn exec_motion_mode(
        &self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    );
}
impl_downcast!(Document);

pub trait DocumentPhantom {
    fn phantom_text(&self, line: usize) -> PhantomTextLine;

    /// Translate a column position into the position it would be before combining with
    /// the phantom text.
    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        let phantom = self.phantom_text(line);
        phantom.before_col(col)
    }

    fn has_multiline_phantom(&self) -> bool {
        true
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum WrapMethod {
    None,
    #[default]
    EditorWidth,
    WrapColumn {
        col: usize,
    },
    WrapWidth {
        width: f32,
    },
}
impl WrapMethod {
    pub fn is_none(&self) -> bool {
        matches!(self, WrapMethod::None)
    }

    pub fn is_constant(&self) -> bool {
        matches!(
            self,
            WrapMethod::None
                | WrapMethod::WrapColumn { .. }
                | WrapMethod::WrapWidth { .. }
        )
    }
}

/// There's currently three stages of styling text:  
/// - `Attrs`: This sets the default values for the text
///   - Default font size, font family, etc.
/// - `AttrsList`: This lets you set spans of text to have different styling
///   - Syntax highlighting, bolding specific words, etc.
/// Then once the text layout for the line is created from that, we have:
/// - `Layout Styles`: Where it may depend on the position of text in the line (after wrapping)
///   - Outline boxes
///
/// TODO: We could unify the first two steps if we expose a `.defaults_mut()` on `AttrsList`, and
/// then `Styling` mostly just applies whatever attributes it wants and defaults at the same time?
/// but that would complicate pieces of code that need the font size or line height independently.
pub trait Styling {
    fn font_size(&self, _line: usize) -> usize {
        16
    }

    fn line_height(&self, line: usize) -> f32 {
        let font_size = self.font_size(line) as f32;
        (1.5 * font_size).round().max(font_size)
    }

    fn font_family(&self, _line: usize) -> Cow<[FamilyOwned]> {
        Cow::Borrowed(&[FamilyOwned::SansSerif])
    }

    fn weight(&self, _line: usize) -> Weight {
        Weight::NORMAL
    }

    // TODO(minor): better name?
    fn italic_style(&self, _line: usize) -> floem::cosmic_text::Style {
        floem::cosmic_text::Style::Normal
    }

    fn stretch(&self, _line: usize) -> Stretch {
        Stretch::Normal
    }

    fn tab_width(&self, _line: usize) -> usize {
        4
    }

    /// Whether the cursor should treat leading soft tabs as if they are hard tabs
    fn atomic_soft_tabs(&self, _line: usize) -> bool {
        false
    }

    // TODO: get other style information based on EditorColor enum?
    // TODO: line_style equivalent?

    /// Apply custom attribute styles to the line  
    fn apply_attr_styles(
        &self,
        _line: usize,
        _default: Attrs,
        _attrs: &mut AttrsList,
    ) {
    }

    // TODO: we could have line-specific wrapping, but that would need some extra functions for
    // questions that visual lines' [`Lines`] uses
    fn wrap(&self) -> WrapMethod {
        WrapMethod::EditorWidth
    }

    fn apply_layout_styles(&self, _line: usize, _layout_line: &mut TextLayoutLine) {}

    // TODO: should we replace `foreground` with using `editor.foreground` here?
    fn color(&self, color: EditorColor) -> Color {
        default_light_color(color)
    }
}

pub fn default_light_color(color: EditorColor) -> Color {
    let fg = Color::rgb8(0x38, 0x3A, 0x42);
    let bg = Color::rgb8(0xFA, 0xFA, 0xFA);
    let blue = Color::rgb8(0x40, 0x78, 0xF2);
    let grey = Color::rgb8(0xE5, 0xE5, 0xE6);
    match color {
        EditorColor::Background => bg,
        EditorColor::Scrollbar => Color::rgba8(0xB4, 0xB4, 0xB4, 0xBB),
        EditorColor::DropdownShadow => Color::rgb8(0xB4, 0xB4, 0xB4),
        EditorColor::Foreground => fg,
        EditorColor::Dim => Color::rgb8(0xA0, 0xA1, 0xA7),
        EditorColor::Focus => Color::BLACK,
        EditorColor::Caret => Color::rgb8(0x52, 0x6F, 0xFF),
        EditorColor::Selection => grey,
        EditorColor::CurrentLine => Color::rgb8(0xF2, 0xF2, 0xF2),
        EditorColor::Link => blue,
        EditorColor::VisibleWhitespace => grey,
        EditorColor::IndentGuide => grey,
        EditorColor::StickyHeaderBackground => bg,
        EditorColor::PreeditUnderline => fg,
    }
}

pub fn default_dark_color(color: EditorColor) -> Color {
    let fg = Color::rgb8(0xAB, 0xB2, 0xBF);
    let bg = Color::rgb8(0x28, 0x2C, 0x34);
    let blue = Color::rgb8(0x61, 0xAF, 0xEF);
    let grey = Color::rgb8(0x3E, 0x44, 0x51);
    match color {
        EditorColor::Background => bg,
        EditorColor::Scrollbar => Color::rgba8(0x3E, 0x44, 0x51, 0xBB),
        EditorColor::DropdownShadow => Color::BLACK,
        EditorColor::Foreground => fg,
        EditorColor::Dim => Color::rgb8(0x5C, 0x63, 0x70),
        EditorColor::Focus => Color::rgb8(0xCC, 0xCC, 0xCC),
        EditorColor::Caret => Color::rgb8(0x52, 0x8B, 0xFF),
        EditorColor::Selection => grey,
        EditorColor::CurrentLine => Color::rgb8(0x2C, 0x31, 0x3c),
        EditorColor::Link => blue,
        EditorColor::VisibleWhitespace => grey,
        EditorColor::IndentGuide => grey,
        EditorColor::StickyHeaderBackground => bg,
        EditorColor::PreeditUnderline => fg,
    }
}

pub type DocumentRef = Rc<dyn Document>;

/// A simple text document that holds content in a rope.  
/// This can be used as a base structure for common operations.
#[derive(Clone)]
pub struct TextDocument {
    buffer: RwSignal<Buffer>,
    cache_rev: RwSignal<u64>,
    preedit: PreeditData,
}
impl TextDocument {
    pub fn new(cx: Scope, text: impl Into<Rope>) -> TextDocument {
        let text = text.into();
        let buffer = Buffer::new(text);
        let preedit = PreeditData {
            preedit: cx.create_rw_signal(None),
        };

        TextDocument {
            buffer: cx.create_rw_signal(buffer),
            cache_rev: cx.create_rw_signal(0),
            preedit,
        }
    }
}
impl Document for TextDocument {
    fn text(&self) -> Rope {
        self.buffer.with_untracked(|buffer| buffer.text().clone())
    }

    fn cache_rev(&self) -> RwSignal<u64> {
        self.cache_rev
    }

    fn preedit(&self) -> PreeditData {
        self.preedit.clone()
    }

    fn run_command(
        &self,
        cmd: &Command,
        count: Option<usize>,
        modifiers: ModifiersState,
    ) {
        todo!()
    }

    fn exec_motion_mode(
        &self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    ) {
        // TODO(floem-editor): Action::execute_motion_mode returns with the buffer's syntax edits
        // but we don't want treesitter to be included in base floem-editor
        self.buffer.try_update(move |buffer| {
            Action::execute_motion_mode(
                cursor,
                buffer,
                motion_mode,
                start,
                end,
                is_vertical,
                register,
            )
        });
    }
}
impl DocumentPhantom for TextDocument {
    fn phantom_text(&self, _line: usize) -> PhantomTextLine {
        PhantomTextLine::default()
    }

    fn has_multiline_phantom(&self) -> bool {
        false
    }
}

impl Debug for TextDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("TextDocument");
        s.field("text", &self.text());
        s.finish()
    }
}

// TODO: move this to tests or examples
/// Example document for phantom text that simply puts the line length at the end of the line
#[derive(Clone)]
pub struct PhantomTextDocument {
    // We use a text document as the base to easily 'inherit' all of its functionality
    doc: TextDocument,
    style: ReadSignal<Rc<dyn Styling>>,
}
impl PhantomTextDocument {
    /// Create a new phantom text document
    pub fn new(
        doc: TextDocument,
        style: ReadSignal<Rc<dyn Styling>>,
    ) -> PhantomTextDocument {
        PhantomTextDocument { doc, style }
    }
}
impl Document for PhantomTextDocument {
    fn text(&self) -> Rope {
        self.doc.text()
    }

    fn cache_rev(&self) -> RwSignal<u64> {
        self.doc.cache_rev()
    }

    fn preedit(&self) -> PreeditData {
        self.doc.preedit()
    }

    fn run_command(
        &self,
        cmd: &Command,
        count: Option<usize>,
        modifiers: ModifiersState,
    ) {
        self.doc.run_command(cmd, count, modifiers)
    }

    fn exec_motion_mode(
        &self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    ) {
        self.doc.exec_motion_mode(
            cursor,
            motion_mode,
            start,
            end,
            is_vertical,
            register,
        )
    }
}
impl DocumentPhantom for PhantomTextDocument {
    fn phantom_text(&self, line: usize) -> PhantomTextLine {
        let rope_text = self.rope_text();
        let line_end = rope_text.line_end_col(line, true);

        let phantom = PhantomText {
            kind: PhantomTextKind::Diagnostic,
            col: line_end,
            text: line_end.to_string(),
            font_size: None,
            fg: None,
            bg: None,
            under_line: None,
        };

        let mut text = smallvec![phantom];

        let preedit_underline = self
            .style
            .get_untracked()
            .color(EditorColor::PreeditUnderline);
        if let Some(preedit) = self.preedit_phantom(Some(preedit_underline), line) {
            text.push(preedit);
        }

        return PhantomTextLine { text };
    }

    fn has_multiline_phantom(&self) -> bool {
        false
    }
}

/// A document-wrapper for handling commands.  
pub struct ExtCmdDocument<D, F> {
    pub doc: D,
    /// Called whenever [`Document::run_command`] is called.  
    /// If `handler` returns [`CommandExecuted::Yes`] then the default handler on `doc: D` will not
    /// be called.
    pub handler: F,
}
impl<
        D: Document,
        F: Fn(&Command, Option<usize>, ModifiersState) -> CommandExecuted + 'static,
    > ExtCmdDocument<D, F>
{
    pub fn new(doc: D, handler: F) -> ExtCmdDocument<D, F> {
        ExtCmdDocument { doc, handler }
    }
}
// TODO: it'd be nice if there was some macro to wrap all of the `Document` methods
// but replace specific ones
impl<D, F> Document for ExtCmdDocument<D, F>
where
    D: Document,
    F: Fn(&Command, Option<usize>, ModifiersState) -> CommandExecuted + 'static,
{
    fn text(&self) -> Rope {
        self.doc.text()
    }

    fn rope_text(&self) -> RopeTextVal {
        self.doc.rope_text()
    }

    fn cache_rev(&self) -> RwSignal<u64> {
        self.doc.cache_rev()
    }

    fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        self.doc.find_unmatched(offset, previous, ch)
    }

    fn find_matching_pair(&self, offset: usize) -> usize {
        self.doc.find_matching_pair(offset)
    }

    fn preedit(&self) -> PreeditData {
        self.doc.preedit()
    }

    fn preedit_phantom(
        &self,
        under_line: Option<Color>,
        line: usize,
    ) -> Option<PhantomText> {
        self.doc.preedit_phantom(under_line, line)
    }

    fn compute_screen_lines(
        &self,
        editor: &Editor,
        base: RwSignal<ScreenLinesBase>,
    ) -> ScreenLines {
        self.doc.compute_screen_lines(editor, base)
    }

    fn run_command(
        &self,
        cmd: &Command,
        count: Option<usize>,
        modifiers: ModifiersState,
    ) {
        if (self.handler)(cmd, count, modifiers) == CommandExecuted::Yes {
            return;
        }

        self.doc.run_command(cmd, count, modifiers)
    }

    fn exec_motion_mode(
        &self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    ) {
        self.doc.exec_motion_mode(
            cursor,
            motion_mode,
            start,
            end,
            is_vertical,
            register,
        )
    }
}
impl<D, F> DocumentPhantom for ExtCmdDocument<D, F>
where
    D: Document,
    F: Fn(&Command, Option<usize>, ModifiersState) -> CommandExecuted,
{
    fn phantom_text(&self, line: usize) -> PhantomTextLine {
        self.doc.phantom_text(line)
    }

    fn has_multiline_phantom(&self) -> bool {
        self.doc.has_multiline_phantom()
    }

    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        self.doc.before_phantom_col(line, col)
    }
}

pub const SCALE_OR_SIZE_LIMIT: f32 = 5.0;

#[derive(Debug, Clone)]
pub struct SimpleStyling<C> {
    pub font_size: usize,
    // TODO: should we really have this be a float? Shouldn't it just be a LineHeightValue?
    /// If less than 5.0, line height will be a multiple of the font size
    pub line_height: f32,
    pub font_family: Vec<FamilyOwned>,
    pub weight: Weight,
    pub italic_style: floem::cosmic_text::Style,
    pub stretch: Stretch,
    pub tab_width: usize,
    pub atomic_soft_tabs: bool,
    pub wrap: WrapMethod,
    pub color: C,
}
impl<C: Fn(EditorColor) -> Color> SimpleStyling<C> {
    pub fn new(color: C) -> SimpleStyling<C> {
        SimpleStyling {
            font_size: 16,
            line_height: 1.5,
            font_family: vec![FamilyOwned::SansSerif],
            weight: Weight::NORMAL,
            italic_style: floem::cosmic_text::Style::Normal,
            stretch: Stretch::Normal,
            tab_width: 4,
            atomic_soft_tabs: false,
            wrap: WrapMethod::EditorWidth,
            color,
        }
    }
}
impl Default for SimpleStyling<fn(EditorColor) -> Color> {
    fn default() -> Self {
        SimpleStyling::new(default_light_color)
    }
}
impl<C: Fn(EditorColor) -> Color> Styling for SimpleStyling<C> {
    fn font_size(&self, _line: usize) -> usize {
        self.font_size
    }

    fn line_height(&self, _line: usize) -> f32 {
        let line_height = if self.line_height < SCALE_OR_SIZE_LIMIT {
            self.line_height * self.font_size as f32
        } else {
            self.line_height
        };

        // Prevent overlapping lines
        (line_height.round() as usize).max(self.font_size) as f32
    }

    fn font_family(&self, _line: usize) -> Cow<[FamilyOwned]> {
        Cow::Borrowed(&self.font_family)
    }

    fn weight(&self, _line: usize) -> Weight {
        self.weight
    }

    fn italic_style(&self, _line: usize) -> floem::cosmic_text::Style {
        self.italic_style
    }

    fn stretch(&self, _line: usize) -> Stretch {
        self.stretch
    }

    fn tab_width(&self, _line: usize) -> usize {
        self.tab_width
    }

    fn atomic_soft_tabs(&self, _line: usize) -> bool {
        self.atomic_soft_tabs
    }

    fn apply_attr_styles(
        &self,
        _line: usize,
        _default: Attrs,
        _attrs: &mut AttrsList,
    ) {
    }

    fn wrap(&self) -> WrapMethod {
        self.wrap
    }

    fn apply_layout_styles(&self, _line: usize, _layout_line: &mut TextLayoutLine) {}

    fn color(&self, color: EditorColor) -> Color {
        (self.color)(color)
    }
}
