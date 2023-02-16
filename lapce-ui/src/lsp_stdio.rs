use druid::{
    ArcStr, BoxConstraints, Cursor, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, TextLayout, UpdateCtx,
    Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    config::LapceTheme, data::LapceTabData, panel::PanelKind, rich_text::RichText,
};

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

enum LspStdioKind {
    Request,
    Response,
}

struct LspStdioContent {
    kind: LspStdioKind,
    mouse_index: Option<usize>,
    lines: Vec<(bool, TextLayout<RichText>, TextLayout<RichText>)>,
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
                for (index, (collapsed, mini_text, pretty_text)) in
                    self.lines.iter().enumerate()
                {
                    let text = if *collapsed { mini_text } else { pretty_text };

                    let size = text.size();
                    if y_offset < mouse_event.pos.y
                        && mouse_event.pos.y < (y_offset + size.height)
                        && 0.0 < mouse_event.pos.x
                        && mouse_event.pos.x < self.width
                    {
                        self.mouse_index = Some(index);
                        ctx.set_cursor(&Cursor::Pointer);
                        ctx.request_layout();
                        break;
                    }

                    y_offset += size.height + PADDING;
                }
            }
            Event::MouseDown(_) => {
                if let Some(mouse_index) = self.mouse_index {
                    if let Some((collapsed, _, _)) = self.lines.get_mut(mouse_index)
                    {
                        *collapsed = !*collapsed;
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
        let vec = match self.kind {
            LspStdioKind::Request => &data.lsp_stdio.lsp_request,
            LspStdioKind::Response => &data.lsp_stdio.lsp_response,
        };

        if vec.len() > self.lines.len() {
            self.lines
                .extend(vec.iter().skip(self.lines.len()).map(|line| {
                    let mut mini_text = TextLayout::new();
                    mini_text.set_text(RichText::new(ArcStr::from(
                        serde_json::to_string(line).unwrap(),
                    )));
                    let mut pretty_text = TextLayout::new();
                    pretty_text.set_text(RichText::new(ArcStr::from(
                        serde_json::to_string_pretty(line).unwrap(),
                    )));
                    (true, mini_text, pretty_text)
                }));
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let mut height = 0.0;
        let width = bc.max().width;

        for (collapsed, mini_text, pretty_text) in self.lines.iter_mut() {
            let text = if *collapsed { mini_text } else { pretty_text };
            text.set_wrap_width(width - PADDING * 2.0);
            height += text.size().height + PADDING;
            text.rebuild_if_needed(ctx.text(), env);
        }

        self.width = width;
        Size::new(
            width,
            height + 10.0, // Add some padding to the bottom
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let mut y_offset = 0.0;
        for (index, (collapsed, mini_text, pretty_text)) in
            self.lines.iter().enumerate()
        {
            let text = if *collapsed { mini_text } else { pretty_text };
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
            y_offset += text.size().height + PADDING;
        }
    }
}
