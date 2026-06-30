use std::{borrow::Cow, ops::Range, path::PathBuf, str::FromStr};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, batch};
use lapce_core::{
    buffer::{
        Buffer,
        rope_text::{RopeText, RopeTextRef},
    },
    rope_text_pos::RopeTextPosition,
    selection::Selection,
};
use lsp_types::InsertTextFormat;

use crate::{config::LapceConfig, doc::Doc, editor::EditorData, snippet::Snippet};

// TODO: we could integrate completion lens with this, so it is considered at the same time

/// Redefinition of lsp types inline completion item with offset range
#[derive(Debug, Clone)]
pub struct InlineCompletionItem {
    /// The text to replace the range with.
    pub insert_text: String,
    /// Text used to decide if this inline completion should be shown.
    pub filter_text: Option<String>,
    /// The range (of offsets) to replace  
    pub range: Option<Range<usize>>,
    pub command: Option<lsp_types::Command>,
    pub insert_text_format: Option<InsertTextFormat>,
}
impl InlineCompletionItem {
    pub fn from_lsp(buffer: &Buffer, item: lsp_types::InlineCompletionItem) -> Self {
        let range = item.range.map(|r| {
            let start = buffer.offset_of_position(&r.start);
            let end = buffer.offset_of_position(&r.end);
            start..end
        });
        Self {
            insert_text: item.insert_text,
            filter_text: item.filter_text,
            range,
            command: item.command,
            insert_text_format: item.insert_text_format,
        }
    }

    pub fn apply(
        &self,
        editor: &EditorData,
        start_offset: usize,
    ) -> anyhow::Result<()> {
        let text_format = self
            .insert_text_format
            .unwrap_or(InsertTextFormat::PLAIN_TEXT);

        let selection = if let Some(range) = &self.range {
            Selection::region(range.start, range.end)
        } else {
            Selection::caret(start_offset)
        };

        match text_format {
            InsertTextFormat::PLAIN_TEXT => editor.do_edit(
                &selection,
                &[(selection.clone(), self.insert_text.as_str())],
            ),
            InsertTextFormat::SNIPPET => {
                editor.completion_apply_snippet(
                    &self.insert_text,
                    &selection,
                    Vec::new(),
                    start_offset,
                )?;
            }
            _ => {
                // We don't know how to support this text format
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineCompletionStatus {
    /// The inline completion is not active.
    Inactive,
    /// The inline completion is active and is waiting for the server to respond.
    Started,
    /// The inline completion is active and has received a response from the server.
    Active,
}

#[derive(Clone)]
pub struct InlineCompletionData {
    pub status: InlineCompletionStatus,
    /// The active inline completion index in the list of completions.
    pub active: RwSignal<usize>,
    pub items: im::Vector<InlineCompletionItem>,
    pub start_offset: usize,
    pub path: PathBuf,
}
impl InlineCompletionData {
    pub fn new(cx: Scope) -> Self {
        Self {
            status: InlineCompletionStatus::Inactive,
            active: cx.create_rw_signal(0),
            items: im::vector![],
            start_offset: 0,
            path: PathBuf::new(),
        }
    }

    pub fn current_item(&self) -> Option<&InlineCompletionItem> {
        let active = self.active.get_untracked();
        self.items.get(active)
    }

    pub fn next(&mut self) {
        if !self.items.is_empty() {
            let next_index = (self.active.get_untracked() + 1) % self.items.len();
            self.active.set(next_index);
        }
    }

    pub fn previous(&mut self) {
        if !self.items.is_empty() {
            let prev_index = if self.active.get_untracked() == 0 {
                self.items.len() - 1
            } else {
                self.active.get_untracked() - 1
            };
            self.active.set(prev_index);
        }
    }

    pub fn cancel(&mut self) {
        if self.status == InlineCompletionStatus::Inactive {
            return;
        }

        self.items.clear();
        self.status = InlineCompletionStatus::Inactive;
    }

    /// Set the items for the inline completion.  
    /// Sets `active` to `0` and `status` to `InlineCompletionStatus::Active`.
    pub fn set_items(
        &mut self,
        items: im::Vector<InlineCompletionItem>,
        start_offset: usize,
        path: PathBuf,
    ) {
        batch(|| {
            self.items = items;
            self.active.set(0);
            self.status = InlineCompletionStatus::Active;
            self.start_offset = start_offset;
            self.path = path;
        });
    }

    pub fn update_doc(&self, doc: &Doc, offset: usize) {
        if self.status != InlineCompletionStatus::Active {
            doc.clear_inline_completion();
            return;
        }

        if self.items.is_empty() {
            doc.clear_inline_completion();
            return;
        }

        let active = self.active.get_untracked();
        let active = if active >= self.items.len() {
            self.active.set(0);
            0
        } else {
            active
        };

        let item = &self.items[active];
        let text = doc.buffer.with_untracked(|buffer| buffer.text().clone());
        let text = RopeTextRef::new(&text);
        let completion = inline_completion_text(text, offset, offset, item, None);

        match completion {
            ICompletionRes::Set(text, offset) => {
                let (line, col) = doc
                    .buffer
                    .with_untracked(|buffer| buffer.offset_to_line_col(offset));
                doc.set_inline_completion(text, line, col);
            }
            ICompletionRes::Hide | ICompletionRes::Unchanged => {
                doc.clear_inline_completion();
            }
        }
    }

    pub fn update_inline_completion(
        &self,
        config: &LapceConfig,
        doc: &Doc,
        cursor_offset: usize,
    ) {
        if !config.editor.enable_inline_completion {
            doc.clear_inline_completion();
            return;
        }

        let text = doc.buffer.with_untracked(|buffer| buffer.text().clone());
        let text = RopeTextRef::new(&text);
        let Some(item) = self.current_item() else {
            // TODO(minor): should we cancel completion
            return;
        };

        let completion = doc.inline_completion.with_untracked(|cur| {
            let cur = cur.as_deref();
            inline_completion_text(text, self.start_offset, cursor_offset, item, cur)
        });

        match completion {
            ICompletionRes::Hide => {
                doc.clear_inline_completion();
            }
            ICompletionRes::Unchanged => {}
            ICompletionRes::Set(new, offset) => {
                let (line, col) = text.offset_to_line_col(offset);
                doc.set_inline_completion(new, line, col);
            }
        }
    }
}

enum ICompletionRes {
    Hide,
    Unchanged,
    /// The ghost text to display, paired with the absolute buffer offset it
    /// should render at (the cursor position).
    Set(String, usize),
}

/// Get the text of the inline completion item  
fn inline_completion_text(
    rope_text: impl RopeText,
    start_offset: usize,
    cursor_offset: usize,
    item: &InlineCompletionItem,
    current_completion: Option<&str>,
) -> ICompletionRes {
    let text_format = item
        .insert_text_format
        .unwrap_or(InsertTextFormat::PLAIN_TEXT);

    // The suggestion replaces `item.range` (or, when the server gives no range,
    // starts where the completion was requested). Copilot returns a range whose
    // start is before the cursor (its example starts at character 0 of the line),
    // so the already typed text forms a prefix of the suggestion. We can only
    // render ghost text when that start is at or before the cursor; the
    // `strip_prefix` below is what actually decides whether the suggestion still
    // applies to the current input.
    let edit_start = item.range.as_ref().map(|r| r.start).unwrap_or(start_offset);
    if edit_start > cursor_offset {
        return ICompletionRes::Hide;
    }

    let text = match text_format {
        InsertTextFormat::PLAIN_TEXT => Cow::Borrowed(&item.insert_text),
        InsertTextFormat::SNIPPET => {
            let Ok(snippet) = Snippet::from_str(&item.insert_text) else {
                return ICompletionRes::Hide;
            };
            let text = snippet.text();

            Cow::Owned(text)
        }
        _ => {
            // We don't know how to support this text format
            return ICompletionRes::Hide;
        }
    };

    let range = edit_start..cursor_offset;
    let prefix = rope_text.slice_to_cow(range);
    // We strip the prefix of the current input from the label.
    // So that, for example `p` with a completion of `println` will show `rintln`.
    let Some(text) = text.strip_prefix(prefix.as_ref()) else {
        return ICompletionRes::Hide;
    };

    if Some(text) == current_completion {
        ICompletionRes::Unchanged
    } else {
        // Ghost text renders at the cursor, i.e. immediately after the prefix we
        // just stripped, so report that absolute offset.
        ICompletionRes::Set(text.to_string(), cursor_offset)
    }
}
