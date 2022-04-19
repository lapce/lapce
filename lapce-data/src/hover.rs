use std::sync::Arc;

use druid::{ExtEventSink, Size, Target, WidgetId};
use lapce_rpc::buffer::BufferId;
use lsp_types::{
    Hover, HoverContents, MarkedString, MarkupContent, MarkupKind, Position,
};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    proxy::LapceProxy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverStatus {
    Inactive,
    Started,
}

#[derive(Clone)]
pub struct HoverData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    /// The current request status
    pub status: HoverStatus,
    /// The offset that is currently being handled, if there is one
    pub offset: usize,
    /// The buffer that this hover is for
    pub buffer_id: BufferId,
    /// A counter to keep track of the active requests
    pub request_id: usize,
    /// Stores the size of the hover box
    pub size: Size,

    /// The current hover string that is active, because there can be multiple for a single entry
    /// (such as if there is uncertainty over the exact version, such as in overloading or
    /// scripting languages where the declaration is uncertain)
    pub active_item_index: usize,
    /// The hover items that are currently loaded
    pub items: Arc<Vec<HoverItem>>,
}

impl HoverData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            status: HoverStatus::Inactive,
            offset: 0,
            buffer_id: BufferId(0),
            request_id: 0,
            size: Size::new(400.0, 300.0),

            active_item_index: 0,
            items: Arc::new(Vec::new()),
        }
    }

    pub fn get_current_item(&self) -> Option<&HoverItem> {
        self.items.get(self.active_item_index)
    }

    /// The length of the current hover items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Move to the next hover item
    pub fn next(&mut self) {
        // If there is an item after the current one, then actually change
        if self.active_item_index + 1 < self.len() {
            self.active_item_index += 1;
        }
    }

    /// Move to the previous hover item
    pub fn previous(&mut self) {
        if self.active_item_index > 0 {
            self.active_item_index -= 1;
        }
    }

    /// Cancel the current hover information, clearing out held data
    pub fn cancel(&mut self) {
        if self.status == HoverStatus::Inactive {
            return;
        }

        self.status = HoverStatus::Inactive;
        Arc::make_mut(&mut self.items).clear();
        self.active_item_index = 0;
    }

    /// Send a request to update the hover at the given position anad file
    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        hover_widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        proxy.get_hover(
            request_id,
            buffer_id,
            position,
            Box::new(move |result| {
                if let Ok(resp) = result {
                    if let Ok(resp) = serde_json::from_value::<Hover>(resp) {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHover(request_id, resp),
                            Target::Widget(hover_widget_id),
                        );
                    }
                }
            }),
        );
    }

    /// Receive the result of a hover request
    pub fn receive(&mut self, request_id: usize, resp: lsp_types::Hover) {
        // If we've moved to inactive between the time the request started and now
        // or we've moved to a different request
        // then don't bother processing the received data
        if self.status == HoverStatus::Inactive || self.request_id != request_id {
            return;
        }

        let items = Arc::make_mut(&mut self.items);
        // Extract the items in the format that we want them to be in
        *items = match resp.contents {
            HoverContents::Scalar(text) => vec![HoverItem::from(text)],
            HoverContents::Array(entries) => {
                entries.into_iter().map(HoverItem::from).collect()
            }
            HoverContents::Markup(content) => {
                vec![HoverItem::from(content)]
            }
        };
    }
}

impl Default for HoverData {
    fn default() -> Self {
        Self::new()
    }
}

/// A hover entry
/// This is separate from the lsp-types version to make using it more direct
#[derive(Clone)]
pub enum HoverItem {
    // TODO: This could hold the data needed to render the markdown output
    Markdown(String),
    PlainText(String),
}
impl HoverItem {
    pub fn as_str(&self) -> &str {
        match self {
            HoverItem::Markdown(x) | HoverItem::PlainText(x) => x.as_str(),
        }
    }
}
impl From<MarkedString> for HoverItem {
    fn from(text: MarkedString) -> Self {
        match text {
            MarkedString::String(text) => HoverItem::Markdown(text),
            // This is a short version of a code block
            MarkedString::LanguageString(code) => {
                // Simply construct the string as if it was written directly
                HoverItem::Markdown(format!(
                    "```{}\n{}\n```",
                    code.language, code.value
                ))
            }
        }
    }
}
impl From<MarkupContent> for HoverItem {
    fn from(content: MarkupContent) -> Self {
        match content.kind {
            MarkupKind::Markdown => HoverItem::Markdown(content.value),
            MarkupKind::PlainText => HoverItem::PlainText(content.value),
        }
    }
}
