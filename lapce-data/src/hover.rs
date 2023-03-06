use std::{cell::RefCell, rc::Rc, sync::Arc};

use druid::{ExtEventSink, Size, Target, WidgetId};
use lapce_rpc::{buffer::BufferId, proxy::ProxyResponse};
use lsp_types::{HoverContents, MarkedString, MarkupKind, Position};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceTheme},
    data::EditorDiagnostic,
    document::{BufferContent, Document},
    markdown::{from_marked_string, parse_markdown, Content},
    proxy::LapceProxy,
    rich_text::{RichText, RichTextBuilder},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone)]
pub struct HoverData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    /// The editor view id that the hover is displayed for
    pub editor_view_id: WidgetId,
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
    /// Stores the actual size of the hover content
    pub content_size: Rc<RefCell<Size>>,
    /// The hover items that are currently loaded
    pub items: Arc<Vec<Content>>,
    /// The text for the diagnostic(s) at the position
    pub diagnostic_content: Option<RichText>,
}

impl HoverData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            editor_view_id: WidgetId::next(),
            status: HoverStatus::Inactive,
            offset: 0,
            buffer_id: BufferId(0),
            request_id: 0,
            // TODO: make this configurable by themes
            size: Size::new(600.0, 300.0),
            content_size: Rc::new(RefCell::new(Size::ZERO)),

            items: Arc::new(Vec::new()),
            diagnostic_content: None,
        }
    }

    pub fn get_current_items(&self) -> Option<&[Content]> {
        if self.items.is_empty() {
            None
        } else {
            Some(&self.items)
        }
    }

    /// The length of the current hover items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Cancel the current hover information, clearing out held data
    pub fn cancel(&mut self) {
        if self.status == HoverStatus::Inactive {
            return;
        }

        self.status = HoverStatus::Inactive;
        Arc::make_mut(&mut self.items).clear();
    }

    /// Send a request to update the hover at the given position and file
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &mut self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        doc: Arc<Document>,
        diagnostics: Option<Arc<Vec<EditorDiagnostic>>>,
        position: Position,
        hover_widget_id: WidgetId,
        event_sink: ExtEventSink,
        config: Arc<LapceConfig>,
    ) {
        if let BufferContent::File(path) = doc.content() {
            // Clone config for use inside the proxy callback
            let p_config = config.clone();
            // Get the information/documentation that should be shown on hover
            proxy.proxy_rpc.get_hover(
                request_id,
                path.clone(),
                position,
                Box::new(move |result| {
                    if let Ok(ProxyResponse::HoverResponse { request_id, hover }) =
                        result
                    {
                        let items = parse_hover_resp(hover, &p_config);
                        let items = Arc::new(items);

                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHover { request_id, items },
                            Target::Widget(hover_widget_id),
                        );
                    }
                }),
            );
            self.collect_diagnostics(position, diagnostics, config);
        }
    }

    /// Receive the result of a hover request
    pub fn receive(&mut self, request_id: usize, items: Arc<Vec<Content>>) {
        // If we've moved to inactive between the time the request started and now
        // or we've moved to a different request
        // then don't bother processing the received data
        if self.status == HoverStatus::Inactive || self.request_id != request_id {
            return;
        }

        self.status = HoverStatus::Done;
        self.items = items;
    }

    fn collect_diagnostics(
        &mut self,
        position: Position,
        diagnostics: Option<Arc<Vec<EditorDiagnostic>>>,
        config: Arc<LapceConfig>,
    ) {
        if let Some(diagnostics) = diagnostics {
            let diagnostics = diagnostics
                .iter()
                .map(|diag| &diag.diagnostic)
                .filter(|diag| {
                    position >= diag.range.start && position < diag.range.end
                });

            // Get the dim foreground color for extra information about the error that is typically
            // not significant
            let dim_color =
                config.get_color_unchecked(LapceTheme::EDITOR_DIM).clone();

            // Build up the text for all the diagnostics
            let mut content = RichTextBuilder::new();
            content.set_line_height(1.5);
            for diagnostic in diagnostics {
                content.push(&diagnostic.message);

                // If there's a source of the message (ex: it came from rustc or rust-analyzer)
                // then include that
                if let Some(source) = &diagnostic.source {
                    content.push(" ");
                    content.push(source).text_color(dim_color.clone());

                    // If there's an available error code then include that
                    if let Some(code) = &diagnostic.code {
                        // TODO: code description field has information like documentation on the
                        // error code which could be useful to provide as a link

                        // formatted as  diagsource(code)
                        content.push("(").text_color(dim_color.clone());
                        match code {
                            lsp_types::NumberOrString::Number(v) => {
                                content
                                    .push(&v.to_string())
                                    .text_color(dim_color.clone());
                            }
                            lsp_types::NumberOrString::String(v) => {
                                content
                                    .push(v.as_str())
                                    .text_color(dim_color.clone());
                            }
                        }
                        content.push(")").text_color(dim_color.clone());
                    }
                }

                // TODO: The Related information field has data that can give better insight into
                // the causes of the error
                // (ex: The place where a variable was moved into when the 'main' error is at where
                // you tried using it. This would work the best with some way to link to files)

                content.push("\n");
            }

            self.diagnostic_content = Some(content.build());
        } else {
            self.diagnostic_content = None;
        }
    }
}

impl Default for HoverData {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_hover_resp(hover: lsp_types::Hover, config: &LapceConfig) -> Vec<Content> {
    match hover.contents {
        HoverContents::Scalar(text) => match text {
            MarkedString::String(text) => parse_markdown(&text, 1.5, config),
            MarkedString::LanguageString(code) => parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                1.5,
                config,
            ),
        },
        HoverContents::Array(array) => {
            let entries = array
                .into_iter()
                .map(|t| from_marked_string(t, config))
                .rev();

            // TODO: It'd be nice to avoid this vec
            itertools::Itertools::intersperse(entries, vec![Content::Separator])
                .flatten()
                .collect()
        }
        HoverContents::Markup(content) => match content.kind {
            MarkupKind::PlainText => {
                let mut builder = RichTextBuilder::new();
                builder.set_line_height(1.5);
                builder.push(&content.value);
                vec![Content::Text(builder.build())]
            }
            MarkupKind::Markdown => parse_markdown(&content.value, 1.5, config),
        },
    }
}
