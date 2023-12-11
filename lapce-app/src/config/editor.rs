use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

use crate::doc::RenderWhitespace;

pub const SCALE_OR_SIZE_LIMIT: f64 = 5.0;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ClickMode {
    #[default]
    #[serde(rename = "single")]
    SingleClick,
    #[serde(rename = "file")]
    DoubleClickFile,
    #[serde(rename = "all")]
    DoubleClickAll,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WrapStyle {
    /// No wrapping
    None,
    /// Wrap at the editor width
    #[default]
    EditorWidth,
    // /// Wrap at the wrap-column
    // WrapColumn,
    /// Wrap at a specific width
    WrapWidth,
}
impl WrapStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            WrapStyle::None => "none",
            WrapStyle::EditorWidth => "editor-width",
            // WrapStyle::WrapColumn => "wrap-column",
            WrapStyle::WrapWidth => "wrap-width",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(WrapStyle::None),
            "editor-width" => Some(WrapStyle::EditorWidth),
            // "wrap-column" => Some(WrapStyle::WrapColumn),
            "wrap-width" => Some(WrapStyle::WrapWidth),
            _ => None,
        }
    }
}
impl ToString for WrapStyle {
    fn to_string(&self) -> String {
        self.as_str().to_string()
    }
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct EditorConfig {
    #[field_names(desc = "Set the editor font family")]
    pub font_family: String,
    #[field_names(desc = "Set the editor font size")]
    font_size: usize,
    #[field_names(desc = "Set the font size in the code lens")]
    pub code_lens_font_size: usize,
    #[field_names(
        desc = "Set the editor line height. If less than 5.0, line height will be a multiple of the font size."
    )]
    line_height: f64,
    #[field_names(
        desc = "If enabled, when you input a tab character, it will insert indent that's detected based on your files."
    )]
    pub smart_tab: bool,
    #[field_names(desc = "Set the tab width")]
    pub tab_width: usize,
    #[field_names(desc = "If opened editors are shown in a tab")]
    pub show_tab: bool,
    #[field_names(desc = "If navigation breadcrumbs are shown for the file")]
    pub show_bread_crumbs: bool,
    #[field_names(desc = "If the editor can scroll beyond the last line")]
    pub scroll_beyond_last_line: bool,
    #[field_names(
        desc = "Set the minimum number of visible lines above and below the cursor"
    )]
    pub cursor_surrounding_lines: usize,
    #[field_names(desc = "The kind of wrapping to perform")]
    pub wrap_style: WrapStyle,
    // #[field_names(desc = "The number of columns to wrap at")]
    // pub wrap_column: usize,
    #[field_names(desc = "The number of pixels to wrap at")]
    pub wrap_width: usize,
    #[field_names(
        desc = "Show code context like functions and classes at the top of editor when scroll"
    )]
    pub sticky_header: bool,
    #[field_names(
        desc = "If the editor should show the documentation of the current completion item"
    )]
    pub completion_show_documentation: bool,
    #[field_names(
        desc = "If the editor should show the signature of the function as the parameters are being typed"
    )]
    pub show_signature: bool,
    #[field_names(
        desc = "If the signature view should put the codeblock into a label. This might not work nicely for LSPs which provide invalid code for their labels."
    )]
    pub signature_label_code_block: bool,
    #[field_names(
        desc = "Whether the editor should enable automatic closing of matching pairs"
    )]
    pub auto_closing_matching_pairs: bool,
    #[field_names(
        desc = "Whether the editor should automatically surround selected text when typing quotes or brackets"
    )]
    pub auto_surround: bool,
    #[field_names(
        desc = "How long (in ms) it should take before the hover information appears"
    )]
    pub hover_delay: u64,
    #[field_names(
        desc = "If modal mode should have relative line numbers (though, not in insert mode)"
    )]
    pub modal_mode_relative_line_numbers: bool,
    #[field_names(
        desc = "Whether it should format the document on save (if there is an available formatter)"
    )]
    pub format_on_save: bool,

    #[field_names(desc = "If matching brackets are highlighted")]
    pub highlight_matching_brackets: bool,

    #[field_names(desc = "If scope lines are highlighted")]
    pub highlight_scope_lines: bool,

    #[field_names(desc = "If inlay hints should be displayed")]
    pub enable_inlay_hints: bool,

    #[field_names(
        desc = "Set the inlay hint font family. If empty, it uses the editor font family."
    )]
    pub inlay_hint_font_family: String,
    #[field_names(
        desc = "Set the inlay hint font size. If less than 5 or greater than editor font size, it uses the editor font size."
    )]
    pub inlay_hint_font_size: usize,
    #[field_names(desc = "If diagnostics should be displayed inline")]
    pub enable_error_lens: bool,
    #[field_names(
        desc = "Whether error lens should go to the end of view line, or only to the end of the diagnostic"
    )]
    pub error_lens_end_of_line: bool,
    #[field_names(
        desc = "Whether error lens should extend over multiple lines. If false, it will have newlines stripped."
    )]
    pub error_lens_multiline: bool,
    // TODO: Error lens but put entirely on the next line
    // TODO: error lens with indentation matching.
    #[field_names(
        desc = "Set error lens font family. If empty, it uses the inlay hint font family."
    )]
    pub error_lens_font_family: String,
    #[field_names(
        desc = "Set the error lens font size. If 0 it uses the inlay hint font size."
    )]
    pub error_lens_font_size: usize,
    #[field_names(
        desc = "If the editor should display the completion item as phantom text"
    )]
    pub enable_completion_lens: bool,
    #[field_names(desc = "If the editor should display inline completions")]
    pub enable_inline_completion: bool,
    #[field_names(
        desc = "Set completion lens font family. If empty, it uses the inlay hint font family."
    )]
    pub completion_lens_font_family: String,
    #[field_names(
        desc = "Set the completion lens font size. If 0 it uses the inlay hint font size."
    )]
    pub completion_lens_font_size: usize,
    #[field_names(
        desc = "Set the cursor blink interval (in milliseconds). Set to 0 to completely disable."
    )]
    blink_interval: u64,
    #[field_names(
        desc = "Whether the multiple cursor selection is case sensitive."
    )]
    pub multicursor_case_sensitive: bool,
    #[field_names(
        desc = "Whether the multiple cursor selection only selects whole words."
    )]
    pub multicursor_whole_words: bool,
    #[field_names(
        desc = "How the editor should render whitespace characters.\nOptions: none, all, boundary, trailing."
    )]
    pub render_whitespace: RenderWhitespace,
    #[field_names(desc = "Whether the editor show indent guide.")]
    pub show_indent_guide: bool,
    #[field_names(
        desc = "Set the auto save delay (in milliseconds), Set to 0 to completely disable"
    )]
    pub autosave_interval: u64,
    #[field_names(
        desc = "Whether the document should be formatted when an autosave is triggered (required Format on Save)"
    )]
    pub format_on_autosave: bool,
    #[field_names(
        desc = "If enabled the cursor treats leading soft tabs as if they are hard tabs."
    )]
    pub atomic_soft_tabs: bool,
    #[field_names(
        desc = "Use a double click to interact with the file explorer.\nOptions: single (default), file or all."
    )]
    pub double_click: ClickMode,
    #[field_names(desc = "Move the focus as you type in the global search box")]
    pub move_focus_while_search: bool,
    #[field_names(
        desc = "Set the default number of visible lines above and below the diff block (-1 for infinite)"
    )]
    pub diff_context_lines: i32,
}

impl EditorConfig {
    pub fn font_size(&self) -> usize {
        self.font_size.max(6).min(32)
    }

    pub fn line_height(&self) -> usize {
        let line_height = if self.line_height < SCALE_OR_SIZE_LIMIT {
            self.line_height * self.font_size as f64
        } else {
            self.line_height
        };

        // Prevent overlapping lines
        (line_height.round() as usize).max(self.font_size)
    }

    pub fn inlay_hint_font_size(&self) -> usize {
        if self.inlay_hint_font_size < 5
            || self.inlay_hint_font_size > self.font_size
        {
            self.font_size()
        } else {
            self.inlay_hint_font_size
        }
    }

    pub fn error_lens_font_size(&self) -> usize {
        if self.error_lens_font_size == 0 {
            self.inlay_hint_font_size()
        } else {
            self.error_lens_font_size
        }
    }

    pub fn completion_lens_font_size(&self) -> usize {
        if self.completion_lens_font_size == 0 {
            self.inlay_hint_font_size()
        } else {
            self.completion_lens_font_size
        }
    }

    /// Returns the tab width if atomic soft tabs are enabled.
    pub fn atomic_soft_tab_width(&self) -> Option<usize> {
        if self.atomic_soft_tabs {
            Some(self.tab_width)
        } else {
            None
        }
    }

    pub fn blink_interval(&self) -> u64 {
        if self.blink_interval == 0 {
            return 0;
        }
        self.blink_interval.max(200)
    }
}
