use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand, LAPCE_COMMAND},
    config::LapceTheme,
    data::{LapceTabData, PanelKind},
    panel::PanelPosition,
    split::SplitDirection,
};
use serde_json::json;

use crate::{
    scroll::LapceScrollNew, split::LapceSplitNew, svg::get_svg, tab::LapceIcon,
};

pub struct LapcePanel {
    #[allow(dead_code)]
    widget_id: WidgetId,
    header: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    split: WidgetPod<LapceTabData, LapceSplitNew>,
}

impl Widget<LapceTabData> for LapcePanel {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.header.event(ctx, event, data, env);
        self.split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.lifecycle(ctx, event, data, env);
        self.split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.update(ctx, data, env);
        self.split.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();

        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);

        let bc = BoxConstraints::tight(Size::new(
            self_size.width,
            self_size.height - header_size.height,
        ));
        self.split.layout(ctx, &bc, data, env);
        self.split
            .set_origin(ctx, data, env, Point::new(0.0, header_size.height));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.split.paint(ctx, data, env);
        self.header.paint(ctx, data, env);
    }
}

impl LapcePanel {
    #[allow(clippy::type_complexity)]
    pub fn new(
        kind: PanelKind,
        widget_id: WidgetId,
        split_id: WidgetId,
        split_direction: SplitDirection,
        header: PanelHeaderKind,
        sections: Vec<(
            WidgetId,
            PanelHeaderKind,
            Box<dyn Widget<LapceTabData>>,
            Option<f64>,
        )>,
    ) -> Self {
        let mut split = LapceSplitNew::new(split_id).direction(split_direction);
        for (section_widget_id, header, content, size) in sections {
            let header = match header {
                PanelHeaderKind::None => None,
                PanelHeaderKind::Simple(s) => {
                    Some(PanelSectionHeader::new(s).boxed())
                }
                PanelHeaderKind::Widget(w) => Some(w),
            };
            let section =
                PanelSection::new(section_widget_id, header, content).boxed();

            if let Some(size) = size {
                split = split.with_child(section, Some(section_widget_id), size);
            } else {
                split = split.with_flex_child(section, Some(section_widget_id), 1.0);
            }
        }
        let header = match header {
            PanelHeaderKind::None => {
                PanelMainHeader::new(widget_id, kind, "".to_string()).boxed()
            }
            PanelHeaderKind::Simple(s) => {
                PanelMainHeader::new(widget_id, kind, s).boxed()
            }
            PanelHeaderKind::Widget(w) => w,
        };
        Self {
            widget_id,
            split: WidgetPod::new(split),
            header: WidgetPod::new(header.boxed()),
        }
    }
}

pub enum PanelHeaderKind {
    None,
    Simple(String),
    Widget(Box<dyn Widget<LapceTabData>>),
}

pub struct PanelSection {
    #[allow(dead_code)]
    widget_id: WidgetId,
    header: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    content: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl PanelSection {
    pub fn new(
        widget_id: WidgetId,
        header: Option<Box<dyn Widget<LapceTabData>>>,
        content: Box<dyn Widget<LapceTabData>>,
    ) -> Self {
        let content = LapceScrollNew::new(content).vertical().boxed();
        Self {
            widget_id,
            header: header.map(WidgetPod::new),
            content: WidgetPod::new(content),
        }
    }

    pub fn new_simple(
        widget_id: WidgetId,
        header: String,
        content: Box<dyn Widget<LapceTabData>>,
    ) -> Self {
        let header = PanelSectionHeader::new(header).boxed();
        Self::new(widget_id, Some(header), content)
    }
}

impl Widget<LapceTabData> for PanelSection {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if let Some(header) = self.header.as_mut() {
            header.event(ctx, event, data, env);
        }
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let Some(header) = self.header.as_mut() {
            header.lifecycle(ctx, event, data, env);
        }
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let Some(header) = self.header.as_mut() {
            header.update(ctx, data, env);
        }
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let header_height = if let Some(header) = self.header.as_mut() {
            let header_height = 30.0;
            header.layout(
                ctx,
                &BoxConstraints::tight(Size::new(self_size.width, header_height)),
                data,
                env,
            );
            header.set_origin(ctx, data, env, Point::ZERO);
            header_height
        } else {
            0.0
        };

        let content_size = self.content.layout(
            ctx,
            &BoxConstraints::new(
                Size::ZERO,
                Size::new(self_size.width, self_size.height - header_height),
            ),
            data,
            env,
        );
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, header_height));

        Size::new(
            self_size.width,
            (header_height + content_size.height).max(self_size.height),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.content.paint(ctx, data, env);
        if let Some(header) = self.header.as_mut() {
            header.paint(ctx, data, env);
        }
    }
}

pub struct PanelSectionHeader {
    text: String,
}

impl PanelSectionHeader {
    pub fn new(text: String) -> Self {
        Self { text }
    }
}

impl Widget<LapceTabData> for PanelSectionHeader {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
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
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let shadow_width = 5.0;
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );

            let text_layout = ctx
                .text()
                .new_text_layout(self.text.clone())
                .font(FontFamily::SYSTEM_UI, data.config.editor.font_size as f64)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let height = ctx.size().height;
            let y = (height - text_layout.size().height) / 2.0;
            ctx.draw_text(&text_layout, Point::new(10.0, y));
        });
    }
}

/// This struct is used as the outer container for a panel,
/// it contains the heading such as "Terminal" or "File Explorer".
pub struct PanelMainHeader {
    text: String,
    icons: Vec<LapceIcon>,

    #[allow(dead_code)]
    panel_widget_id: WidgetId,
    kind: PanelKind,
    mouse_pos: Point,
}

impl PanelMainHeader {
    pub fn new(panel_widget_id: WidgetId, kind: PanelKind, text: String) -> Self {
        Self {
            panel_widget_id,
            kind,
            text,
            icons: Vec::new(),
            mouse_pos: Point::ZERO,
        }
    }

    fn update_icons(&mut self, self_size: Size, data: &LapceTabData) {
        let icon_size = 24.0;
        let gap = (self_size.height - icon_size) / 2.0;

        let mut icons = Vec::new();
        let x = self_size.width - ((icons.len() + 1) as f64) * (gap + icon_size);
        let icon = LapceIcon {
            icon: "close.svg".to_string(),
            rect: Size::new(icon_size, icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Workbench(LapceWorkbenchCommand::HidePanel),
                    data: Some(json!(self.kind)),
                },
                Target::Widget(data.id),
            ),
        };
        icons.push(icon);

        let position = data.panel_position(self.kind);
        if let Some(position) = position {
            if position == PanelPosition::BottomLeft
                || position == PanelPosition::BottomRight
            {
                let mut icon_svg = "chevron-up.svg";
                for (_, panel) in data.panels.iter() {
                    if panel.widgets.contains(&self.kind) {
                        if panel.maximized {
                            icon_svg = "chevron-down.svg";
                        }
                        break;
                    }
                }

                let x =
                    self_size.width - ((icons.len() + 1) as f64) * (gap + icon_size);
                let icon = LapceIcon {
                    icon: icon_svg.to_string(),
                    rect: Size::new(icon_size, icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::ToggleMaximizedPanel,
                            ),
                            data: Some(json!(self.kind)),
                        },
                        Target::Widget(data.id),
                    ),
                };
                icons.push(icon);
            }
        }

        self.icons = icons;
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }
}

impl Widget<LapceTabData> for PanelMainHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
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
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let height = 30.0;
        let self_size = Size::new(bc.max().width, height);
        self.update_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let shadow_width = 5.0;
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );

            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );

            let text_layout = ctx
                .text()
                .new_text_layout(self.text.clone())
                .font(FontFamily::SYSTEM_UI, data.config.editor.font_size as f64)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let height = ctx.size().height;
            let y = (height - text_layout.size().height) / 2.0;
            ctx.draw_text(&text_layout, Point::new(10.0, y));

            let icon_padding = 4.0;
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        &icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                if let Some(svg) = get_svg(&icon.icon) {
                    ctx.draw_svg(
                        &svg,
                        icon.rect.inflate(-icon_padding, -icon_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        });
    }
}
