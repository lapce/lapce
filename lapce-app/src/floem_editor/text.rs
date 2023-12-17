use std::{borrow::Cow, fmt::Debug, rc::Rc};

use crate::doc::phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine};
use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, Stretch, Weight},
    peniko::Color,
};
use lapce_core::buffer::{
    rope_text::{RopeText, RopeTextVal},
    Buffer,
};
use lapce_xi_rope::Rope;
use smallvec::smallvec;

/// A document. This holds text.  
pub trait Document: DocumentStyle {
    /// Get the text of the document
    fn text(&self) -> Rope;

    fn rope_text(&self) -> RopeTextVal {
        RopeTextVal::new(self.text())
    }
}

pub trait DocumentStyle {
    fn phantom_text(&self, line: usize) -> PhantomTextLine;
}

/// There's currently three stages of styling text:  
/// - `Attrs`: This sets the default values for the text
///   - Default font size, font family, etc.
/// - `AttrsList`: This lets you set spans of text to have different styling
///   - Syntax highlighting, bolding specific words, etc.
/// Then once the text layout for the line is created from that, we have:
/// - `Extra Styles`: Where it may depend on the position of text in the line (after wrapping)
///   - Outline boxes
///
/// TODO: We could unify the first two steps if we expose a `.defaults_mut()` on `AttrsList`, and
/// then `Styling` mostly just applies whatever attributes it wants and defaults at the same time?
/// but that would complicate pieces of code that need the font size or line height independently.
pub trait Styling {
    /// Default foreground color of text
    fn foreground(&self, _line: usize) -> Color {
        Color::BLACK
    }

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
}

pub type DocumentRef = Rc<dyn Document>;

/// A simple text document that holds content in a rope.  
/// This can be used as a base structure for common operations.
#[derive(Clone)]
pub struct TextDocument {
    buffer: Buffer,
}
impl Document for TextDocument {
    fn text(&self) -> Rope {
        self.buffer.text().clone()
    }
}
impl DocumentStyle for TextDocument {
    fn phantom_text(&self, _line: usize) -> PhantomTextLine {
        PhantomTextLine::default()
    }
}

impl Debug for TextDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("TextDocument");
        s.field("text", self.buffer.text());
        s.finish()
    }
}

// TODO: move this to tests or examples
/// Example document for phantom text that simply puts the line length at the end of the line
#[derive(Clone)]
pub struct PhantomTextDocument {
    // We use a text document as the base to easily 'inherit' all of its functionality
    doc: TextDocument,
}
impl PhantomTextDocument {
    /// Create a new phantom text document
    pub fn new(doc: TextDocument) -> PhantomTextDocument {
        PhantomTextDocument { doc }
    }
}
impl Document for PhantomTextDocument {
    fn text(&self) -> Rope {
        self.doc.text()
    }
}
impl DocumentStyle for PhantomTextDocument {
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

        return PhantomTextLine {
            text: smallvec![phantom],
            max_severity: None,
        };
    }
}
