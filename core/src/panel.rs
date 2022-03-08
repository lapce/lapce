use std::{collections::HashMap, sync::Arc};

use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};
use parking_lot::Mutex;
use serde_json::json;

use crate::{
    command::LapceCommandNew,
    command::{CommandTarget, LapceWorkbenchCommand, LAPCE_NEW_COMMAND},
    config::LapceTheme,
    data::{LapceTabData, PanelKind},
    scroll::LapceScrollNew,
    split::{LapceSplitNew, SplitDirection},
    svg::get_svg,
    tab::LapceIcon,
};

pub enum PanelResizePosition {
    Left,
    LeftSplit,
    Bottom,
}

#[derive(Eq, PartialEq, Hash, Clone)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

pub trait PanelProperty: Send {
    fn widget_id(&self) -> WidgetId;
    fn position(&self) -> &PanelPosition;
    fn active(&self) -> usize;
    fn size(&self) -> (f64, f64);
}

pub struct PanelState {
    #[allow(dead_code)]
    window_id: WindowId,

    #[allow(dead_code)]
    tab_id: WidgetId,
    pub panels: HashMap<WidgetId, Arc<Mutex<dyn PanelProperty>>>,
    pub shown: HashMap<PanelPosition, bool>,
    pub widgets: HashMap<PanelPosition, WidgetId>,
}

impl PanelState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> Self {
        let panels = HashMap::new();
        let mut shown = HashMap::new();
        shown.insert(PanelPosition::LeftTop, true);
        shown.insert(PanelPosition::LeftBottom, true);
        shown.insert(PanelPosition::BottomLeft, true);
        shown.insert(PanelPosition::RightTop, true);

        let mut widgets = HashMap::new();
        widgets.insert(PanelPosition::LeftTop, WidgetId::next());
        widgets.insert(PanelPosition::LeftBottom, WidgetId::next());
        widgets.insert(PanelPosition::BottomLeft, WidgetId::next());
        widgets.insert(PanelPosition::BottomRight, WidgetId::next());
        widgets.insert(PanelPosition::RightTop, WidgetId::next());
        widgets.insert(PanelPosition::RightBottom, WidgetId::next());
        Self {
            window_id,
            tab_id,
            panels,
            shown,
            widgets,
        }
    }

    pub fn is_shown(&self, position: &PanelPosition) -> bool {
        *self.shown.get(position).unwrap_or(&false)
    }

    pub fn add(
        &mut self,
        widget_id: WidgetId,
        panel: Arc<Mutex<dyn PanelProperty>>,
    ) {
        self.panels.insert(widget_id, panel);
    }

    pub fn widget_id(&self, position: &PanelPosition) -> WidgetId {
        *self.widgets.get(position).unwrap()
    }

    pub fn size(&self, position: &PanelPosition) -> Option<(f64, f64)> {
        let mut active_panel = None;
        let mut active = 0;
        for (_, panel) in self.panels.iter() {
            let local_panel = panel.clone();
            let panel = panel.lock();
            if panel.position() == position {
                let panel_active = panel.active();
                if panel_active > active {
                    active_panel = Some(local_panel);
                    active = panel_active;
                }
            }
        }
        active_panel.map(|p| p.lock().size())
    }

    pub fn get(
        &self,
        position: &PanelPosition,
    ) -> Option<Arc<Mutex<dyn PanelProperty>>> {
        let mut active_panel = None;
        for (_, panel) in self.panels.iter() {
            let local_panel = panel.clone();
            let (current_position, active) = {
                let panel = panel.lock();
                let position = panel.position().clone();
                let active = panel.active();
                (position, active)
            };
            if &current_position == position {
                if active_panel.is_none() {
                    active_panel = Some(local_panel);
                } else if active > active_panel.as_ref().unwrap().lock().active() {
                    active_panel = Some(local_panel)
                }
            }
        }
        active_panel
    }

    pub fn shown_panels(
        &self,
    ) -> HashMap<PanelPosition, Arc<Mutex<dyn PanelProperty>>> {
        let mut shown_panels = HashMap::new();
        for (postion, shown) in self.shown.iter() {
            if *shown {
                shown_panels.insert(postion.clone(), None);
            }
        }

        for (_, panel) in self.panels.iter() {
            let local_panel = panel.clone();
            let (position, active) = {
                let panel = panel.lock();
                let position = panel.position().clone();
                let active = panel.active();
                (position, active)
            };
            if let Some(p) = shown_panels.get_mut(&position) {
                if p.is_none() {
                    *p = Some(local_panel);
                } else if active > p.as_ref().unwrap().lock().active() {
                    *p = Some(local_panel)
                }
            }
        }
        shown_panels
            .iter()
            .filter_map(|(p, panel)| Some((p.clone(), panel.clone()?)))
            .collect()
    }
}

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
        self.header.paint(ctx, data, env);
        self.split.paint(ctx, data, env);
    }
}

impl LapcePanel {
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
            ctx.clip(rect.inflate(0.0, 100.0));
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
                LAPCE_NEW_COMMAND,
                LapceCommandNew {
                    cmd: LapceWorkbenchCommand::HidePanel.to_string(),
                    data: Some(json!(self.kind)),
                    palette_desc: None,
                    target: CommandTarget::Workbench,
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
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceWorkbenchCommand::ToggleMaximizedPanel
                                .to_string(),
                            data: Some(json!(self.kind)),
                            palette_desc: None,
                            target: CommandTarget::Workbench,
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
            ctx.clip(rect.inflate(0.0, 100.0));
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
