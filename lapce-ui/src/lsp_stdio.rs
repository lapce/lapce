use druid::{
    BoxConstraints, Cursor, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, TextLayout, UpdateCtx,
    Widget, WidgetExt, WidgetId,
};
use lapce_data::{config::LapceTheme, data::LapceTabData, panel::PanelKind};
use serde_json::Value;

use crate::{
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::LapceScroll,
};

const PADDING: f64 = 5.0;

pub fn new_output_panel(data: &LapceTabData) -> LapcePanel {
    LapcePanel::new(
        PanelKind::LspStdio,
        data.lsp_stdio.widget_id,
        WidgetId::next(),
        vec![
            (
                WidgetId::next(),
                PanelHeaderKind::Simple("LSP stdin".into()),
                LapceScroll::new(
                    LspStdioContent::new(LspStdioKind::Request).boxed(),
                )
                .vertical()
                .boxed(),
                PanelSizing::Flex(true),
            ),
            (
                WidgetId::next(),
                PanelHeaderKind::Simple("LSP stdout".into()),
                LapceScroll::new(
                    LspStdioContent::new(LspStdioKind::Response).boxed(),
                )
                .vertical()
                .boxed(),
                PanelSizing::Flex(true),
            ),
        ],
    )
}

#[derive(Clone, Copy)]
enum LspStdioKind {
    Request,
    Response,
}

struct RowState {
    collapsed: bool,
    needs_rebuild: bool,
    text: TextLayout<String>,
}

struct LspStdioContent {
    kind: LspStdioKind,
    mouse_index: Option<usize>,
    lines: Vec<RowState>,
    width: f64,
}

impl LspStdioContent {
    pub fn new(kind: LspStdioKind) -> LspStdioContent {
        LspStdioContent {
            kind,
            mouse_index: None,
            lines: Vec::new(),
            width: 0.0f64,
        }
    }
}

fn get_data<'a>(
    kind: LspStdioKind,
    data: &'a LapceTabData,
) -> impl Iterator<Item = &'a Value> {
    match kind {
        LspStdioKind::Request => data.lsp_stdio.lsp_request.iter(),
        LspStdioKind::Response => data.lsp_stdio.lsp_response.iter(),
    }
}

impl Widget<LapceTabData> for LspStdioContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_index = None;

                let mut y_offset = 0.0;
                for (index, RowState { text, .. }) in self.lines.iter().enumerate() {
                    let text_height = text.size().height;
                    if y_offset < mouse_event.pos.y
                        && mouse_event.pos.y < (y_offset + text_height)
                        && 0.0 < mouse_event.pos.x
                        && mouse_event.pos.x < self.width
                    {
                        self.mouse_index = Some(index);
                        ctx.set_cursor(&Cursor::Pointer);
                        ctx.request_layout();
                        break;
                    }

                    y_offset += text_height + PADDING;
                }
            }
            Event::MouseDown(_) => {
                if let Some(mouse_index) = self.mouse_index {
                    if let Some(RowState {
                        collapsed,
                        needs_rebuild,
                        ..
                    }) = self.lines.get_mut(mouse_index)
                    {
                        *collapsed = !*collapsed;
                        *needs_rebuild = true;
                        ctx.request_layout();
                    }
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        let old_length = self.lines.len();

        self.lines
            .extend(
                get_data(self.kind, data)
                    .skip(self.lines.len())
                    .map(|line| {
                        let mut text = TextLayout::new();
                        text.set_text(serde_json::to_string(line).unwrap());
                        text.set_wrap_width(self.width - PADDING * 2.0);
                        RowState {
                            collapsed: true,
                            needs_rebuild: true,
                            text,
                        }
                    }),
            );

        let new_length = self.lines.len();

        if old_length != new_length {
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let mut total_height = 0.0;
        self.width = bc.max().width;

        for (
            RowState {
                collapsed,
                needs_rebuild,
                text,
            },
            value,
        ) in self.lines.iter_mut().zip(get_data(self.kind, data))
        {
            if *needs_rebuild {
                let text_str = if *collapsed {
                    serde_json::to_string(value).unwrap()
                } else {
                    serde_json::to_string_pretty(value).unwrap()
                };
                text.set_text(text_str);
                text.set_wrap_width(self.width - PADDING * 2.0);

                let text_height = text.size().height;
                total_height += text_height + PADDING;
                *needs_rebuild = false;
            } else {
                total_height += text.size().height;
            }

            text.rebuild_if_needed(ctx.text(), env);
        }

        Size::new(
            self.width,
            total_height + 10.0, // Add some padding to the bottom
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let rect = ctx.region().bounding_box();
        let mut y_offset = 0.0;
        for (index, RowState { text, .. }) in self.lines.iter().enumerate() {
            let text_height = text.size().height;

            // Checks if:
            // 1. Bottom half is within bounding box
            // 2. The whole thing within the bounding box
            // 3. Top half is within the bounding box
            // 4. Bounding box is within the thing
            if (y_offset <= rect.y0 && y_offset + text_height >= rect.y0)
                || (y_offset >= rect.y0 && y_offset + text_height <= rect.y1)
                || (y_offset <= rect.y1 && y_offset + text_height >= rect.y1)
                || (y_offset <= rect.y1 && y_offset + text_height >= rect.y1)
            {
                if Some(index) == self.mouse_index {
                    let width = ctx.size().width;
                    ctx.fill(
                        Size::new(width - PADDING, text.size().height)
                            .to_rect()
                            .with_origin(Point::new(PADDING, y_offset)),
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                    );
                }
                text.draw(ctx, Point::new(0.0, y_offset));
            }
            y_offset += text_height + PADDING;
        }
    }
}
