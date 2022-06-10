use std::sync::Arc;

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout, TextLayoutBuilder, TextStorage},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
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
        match split_direction {
            SplitDirection::Vertical => {}
            SplitDirection::Horizontal => split = split.hide_border(),
        };
        for (section_widget_id, header, content, size) in sections {
            let header = match header {
                PanelHeaderKind::None => None,
                PanelHeaderKind::Simple(s) => {
                    Some(PanelSectionHeader::new(s, kind).boxed())
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
                PanelMainHeader::new(widget_id, kind, "".into()).boxed()
            }
            PanelHeaderKind::Simple(s) => {
                PanelMainHeader::new(widget_id, kind, s).boxed()
            }
            PanelHeaderKind::Widget(w) => w,
        };
        Self {
            split: WidgetPod::new(split),
            header: WidgetPod::new(header),
        }
    }
}

/// An immutable piece of string that is cheap to clone.
#[derive(Clone)]
pub enum ReadOnlyString {
    Static(&'static str),
    String(Arc<str>),
}

impl From<&'static str> for ReadOnlyString {
    fn from(str: &'static str) -> Self {
        Self::Static(str)
    }
}

impl From<String> for ReadOnlyString {
    fn from(str: String) -> Self {
        Self::String(Arc::from(str))
    }
}

impl TextStorage for ReadOnlyString {
    fn as_str(&self) -> &str {
        match self {
            ReadOnlyString::Static(str) => *str,
            ReadOnlyString::String(str) => str.as_ref(),
        }
    }
}

pub enum PanelHeaderKind {
    None,
    Simple(ReadOnlyString),
    Widget(Box<dyn Widget<LapceTabData>>),
}

struct PanelSection {
    header: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    content: WidgetPod<
        LapceTabData,
        LapceScrollNew<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    >,
}

impl PanelSection {
    pub fn new(
        _widget_id: WidgetId,
        header: Option<Box<dyn Widget<LapceTabData>>>,
        content: Box<dyn Widget<LapceTabData>>,
    ) -> Self {
        let content = LapceScrollNew::new(content).vertical();
        Self {
            header: header.map(WidgetPod::new),
            content: WidgetPod::new(content),
        }
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

        Size::new(content_size.width, header_height + content_size.height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.content.paint(ctx, data, env);
        if let Some(header) = self.header.as_mut() {
            header.paint(ctx, data, env);
        }
    }
}

pub struct PanelSectionHeader {
    text: ReadOnlyString,
    kind: PanelKind,
}

impl PanelSectionHeader {
    pub fn new(text: ReadOnlyString, kind: PanelKind) -> Self {
        Self { text, kind }
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
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            } else if let Some(position) = data.panel_position(self.kind) {
                match position {
                    PanelPosition::BottomLeft | PanelPosition::BottomRight => {
                        ctx.stroke(
                            Line::new(
                                Point::new(rect.x0, rect.y0 + 0.5),
                                Point::new(rect.x1, rect.y0 + 0.5),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                            1.0,
                        );
                    }
                    _ => {}
                }
            }

            ctx.clip(rect);
            let text_layout = ctx
                .text()
                .new_text_layout(self.text.clone())
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
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
    text: ReadOnlyString,
    icons: Vec<LapceIcon>,

    kind: PanelKind,
    mouse_pos: Point,
}

impl PanelMainHeader {
    pub fn new(
        _panel_widget_id: WidgetId,
        kind: PanelKind,
        text: ReadOnlyString,
    ) -> Self {
        Self {
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
            icon: "close.svg",
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

        if let Some(position) = data.panel_position(self.kind) {
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
                    icon: icon_svg,
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
        let self_size =
            Size::new(bc.max().width, data.config.ui.header_height() as f64);
        self.update_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let rect = ctx.size().to_rect();
        let position = data.panel_position(self.kind);
        ctx.with_save(|ctx| {
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.clip(rect.inset((0.0, 0.0, 0.0, 50.0)));
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            } else if let Some(position) = position {
                ctx.stroke(
                    Line::new(
                        Point::new(rect.x0, rect.y1 + 0.5),
                        Point::new(rect.x1, rect.y1 + 0.5),
                    ),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
                match position {
                    PanelPosition::BottomLeft | PanelPosition::BottomRight => {
                        ctx.stroke(
                            Line::new(
                                Point::new(rect.x0, rect.y0 - 0.5),
                                Point::new(rect.x1, rect.y0 - 0.5),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                            1.0,
                        );
                    }
                    _ => {}
                }
            }

            ctx.clip(rect);
            let background = match position {
                Some(PanelPosition::BottomLeft)
                | Some(PanelPosition::BottomRight) => LapceTheme::EDITOR_BACKGROUND,
                _ => LapceTheme::PANEL_BACKGROUND,
            };
            ctx.fill(rect, data.config.get_color_unchecked(background));

            let text_layout = ctx
                .text()
                .new_text_layout(self.text.clone())
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
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
                if let Some(svg) = get_svg(icon.icon) {
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
