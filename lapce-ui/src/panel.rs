use std::{collections::HashMap, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{Text, TextLayout, TextLayoutBuilder, TextStorage},
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx,
    Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{DragContent, LapceTabData},
    panel::{PanelContainerPosition, PanelKind, PanelPosition},
};

use crate::{scroll::LapceScroll, split::LapceSplit, tab::LapceIcon};

pub enum PanelSizing {
    Size(f64),
    /// Flex-sized. Bool decides whether it is resizable in the UI or not.
    Flex(bool),
}

pub struct LapcePanel {
    split: WidgetPod<LapceTabData, LapceSplit>,
}

impl Widget<LapceTabData> for LapcePanel {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
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

        self.split.layout(ctx, bc, data, env);
        self.split.set_origin(ctx, data, env, Point::ZERO);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.split.paint(ctx, data, env);
    }
}

impl LapcePanel {
    #[allow(clippy::type_complexity)]
    pub fn new(
        kind: PanelKind,
        _widget_id: WidgetId,
        split_id: WidgetId,
        sections: Vec<(
            WidgetId,
            PanelHeaderKind,
            Box<dyn Widget<LapceTabData>>,
            PanelSizing,
        )>,
    ) -> Self {
        let mut split = LapceSplit::new(split_id).panel(kind);
        for (section_id, header, content, size) in sections {
            let header = match header {
                PanelHeaderKind::None => None,
                PanelHeaderKind::Simple(s) => {
                    Some(PanelSectionHeader::new(s, kind).boxed())
                }
                PanelHeaderKind::Widget(w) => Some(w),
            };
            let section = PanelSection::new(kind, header, content).boxed();

            split = match size {
                PanelSizing::Size(size) => {
                    split.with_child(section, Some(section_id), size)
                }
                PanelSizing::Flex(resizable) => {
                    split.with_flex_child(section, Some(section_id), 1.0, resizable)
                }
            };
        }
        Self {
            split: WidgetPod::new(split),
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
            ReadOnlyString::Static(str) => str,
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
    kind: PanelKind,
    display_content: bool,
    mouse_down: bool,
    header: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    content: WidgetPod<
        LapceTabData,
        LapceScroll<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    >,
}

impl PanelSection {
    pub fn new(
        kind: PanelKind,
        header: Option<Box<dyn Widget<LapceTabData>>>,
        content: Box<dyn Widget<LapceTabData>>,
    ) -> Self {
        let content = LapceScroll::new(content).vertical();
        Self {
            kind,
            display_content: true,
            mouse_down: true,
            header: header.map(WidgetPod::new),
            content: WidgetPod::new(content),
        }
    }
}

const HEADER_HEIGHT: f64 = 30.0f64;

impl Widget<LapceTabData> for PanelSection {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.clear_cursor();
                if let Some(header) = self.header.as_ref() {
                    let rect = header.layout_rect();
                    if rect.contains(mouse_event.pos) {
                        ctx.set_cursor(&Cursor::Pointer);
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down = false;
                if let Some(header) = self.header.as_ref() {
                    let rect = header.layout_rect();
                    if rect.contains(mouse_event.pos) {
                        self.mouse_down = true
                    }
                }
            }
            Event::MouseUp(mouse_event) => {
                if let Some(header) = self.header.as_ref() {
                    let rect = header.layout_rect();
                    if self.mouse_down && rect.contains(mouse_event.pos) {
                        self.mouse_down = false;
                        self.display_content = !self.display_content;
                        ctx.request_layout();
                    }
                }
            }
            _ => {}
        }

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

        let header_size = if let Some(header) = self.header.as_mut() {
            let size = if !self.display_content
                && data
                    .panel
                    .panel_position(&self.kind)
                    .map(|(_, pos)| pos.is_bottom())
                    .unwrap_or(false)
            {
                header.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(
                        HEADER_HEIGHT,
                        self_size.height,
                    )),
                    data,
                    env,
                )
            } else {
                header.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(
                        self_size.width,
                        HEADER_HEIGHT,
                    )),
                    data,
                    env,
                )
            };
            header.set_origin(ctx, data, env, Point::ZERO);
            size
        } else {
            Size::ZERO
        };

        let content_size = if self.display_content {
            let s = self.content.layout(
                ctx,
                &BoxConstraints::new(
                    Size::ZERO,
                    Size::new(self_size.width, self_size.height - HEADER_HEIGHT),
                ),
                data,
                env,
            );
            self.content.set_origin(
                ctx,
                data,
                env,
                Point::new(0.0, header_size.height),
            );
            s
        } else {
            Size::ZERO
        };

        Size::new(
            header_size.width.max(content_size.width),
            header_size.height + content_size.height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if let Some(header) = self.header.as_mut() {
            header.paint(ctx, data, env);

            let icon_name = if self.display_content {
                LapceIcons::PANEL_RESTORE
            } else {
                LapceIcons::PANEL_MAXIMISE
            };

            let header_rect = header.layout_rect();

            let icon_size = data.config.ui.icon_size();
            let icon_rect = Size::ZERO
                .to_rect()
                .with_origin(Point::new(
                    header_rect.width() - HEADER_HEIGHT / 2.0,
                    HEADER_HEIGHT / 2.0,
                ))
                .inflate(icon_size as f64 / 2.0, icon_size as f64 / 2.0);
            ctx.draw_svg(
                &data.config.ui_svg(icon_name),
                icon_rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }
        if self.display_content {
            self.content.paint(ctx, data, env);
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

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.with_save(|ctx| {
                let (_, pos) = data.panel.panel_position(&self.kind).unwrap();
                if pos.is_bottom() {
                    ctx.clip(rect.inset((0.0, 0.0, 0.0, 50.0)));
                } else {
                    ctx.clip(rect.inset((0.0, 50.0, 0.0, 50.0)));
                }

                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });
        }

        ctx.with_save(|ctx| {
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
            let y = text_layout.y_offset(height);
            ctx.draw_text(&text_layout, Point::new(10.0, y));
        });
    }
}

pub struct PanelContainer {
    pub widget_id: WidgetId,
    switcher0: WidgetPod<LapceTabData, PanelSwitcher>,
    switcher1: WidgetPod<LapceTabData, PanelSwitcher>,
    position: PanelContainerPosition,
    pub panels:
        HashMap<PanelKind, WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl PanelContainer {
    pub fn new(position: PanelContainerPosition) -> Self {
        let (switcher0, switcher1) = match position {
            PanelContainerPosition::Left => (
                PanelSwitcher::new(PanelPosition::LeftTop),
                PanelSwitcher::new(PanelPosition::LeftBottom),
            ),
            PanelContainerPosition::Right => (
                PanelSwitcher::new(PanelPosition::RightTop),
                PanelSwitcher::new(PanelPosition::RightBottom),
            ),
            PanelContainerPosition::Bottom => (
                PanelSwitcher::new(PanelPosition::BottomLeft),
                PanelSwitcher::new(PanelPosition::BottomRight),
            ),
        };
        Self {
            widget_id: WidgetId::next(),
            switcher0: WidgetPod::new(switcher0),
            switcher1: WidgetPod::new(switcher1),
            position,
            panels: HashMap::new(),
        }
    }

    pub fn insert_panel(
        &mut self,
        kind: PanelKind,
        panel: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    ) {
        self.panels.insert(kind, panel);
    }
}

impl Widget<LapceTabData> for PanelContainer {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                ctx.set_handled();
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::ChildrenChanged = &command {
                    ctx.children_changed();
                }
            }
            _ => {}
        }

        self.switcher0.event(ctx, event, data, env);
        self.switcher1.event(ctx, event, data, env);
        if event.should_propagate_to_hidden() {
            for (_, panel) in self.panels.iter_mut() {
                panel.event(ctx, event, data, env);
            }
        } else {
            if let Some((panel, shown)) =
                data.panel.active_panel_at_position(&self.position.first())
            {
                if shown {
                    if let Some(panel) = self.panels.get_mut(&panel) {
                        panel.event(ctx, event, data, env);
                    }
                }
            }
            if let Some((panel, shown)) =
                data.panel.active_panel_at_position(&self.position.second())
            {
                if shown {
                    if let Some(panel) = self.panels.get_mut(&panel) {
                        panel.event(ctx, event, data, env);
                    }
                }
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.switcher0.lifecycle(ctx, event, data, env);
        self.switcher1.lifecycle(ctx, event, data, env);
        for (_, panel) in self.panels.iter_mut() {
            panel.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.switcher0.update(ctx, data, env);
        self.switcher1.update(ctx, data, env);
        if let Some((panel, shown)) =
            data.panel.active_panel_at_position(&self.position.first())
        {
            if shown {
                if let Some(panel) = self.panels.get_mut(&panel) {
                    panel.update(ctx, data, env);
                }
            }
        }
        if let Some((panel, shown)) =
            data.panel.active_panel_at_position(&self.position.second())
        {
            if shown {
                if let Some(panel) = self.panels.get_mut(&panel) {
                    panel.update(ctx, data, env);
                }
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let is_bottom = self.position.is_bottom();

        let (should_shown0, should_shown1) = match self.position {
            PanelContainerPosition::Left => (
                data.panel.position_has_panels(&PanelPosition::LeftTop),
                data.panel.position_has_panels(&PanelPosition::LeftBottom),
            ),
            PanelContainerPosition::Right => (
                data.panel.position_has_panels(&PanelPosition::RightTop),
                data.panel.position_has_panels(&PanelPosition::RightBottom),
            ),
            PanelContainerPosition::Bottom => (
                data.panel.position_has_panels(&PanelPosition::BottomLeft),
                data.panel.position_has_panels(&PanelPosition::BottomRight),
            ),
        };

        let switcher0_size = if should_shown0 {
            data.config.ui.header_height() as f64
        } else {
            0.0
        };
        let switcher1_size = if should_shown1 {
            data.config.ui.header_height() as f64
        } else {
            0.0
        };

        self.switcher0.layout(ctx, bc, data, env);
        self.switcher0.set_origin(ctx, data, env, Point::ZERO);

        self.switcher1.layout(ctx, bc, data, env);
        if self.position.is_bottom() {
            self.switcher1.set_origin(
                ctx,
                data,
                env,
                Point::new(self_size.width - switcher1_size, 0.0),
            );
        } else {
            self.switcher1.set_origin(
                ctx,
                data,
                env,
                Point::new(0.0, self_size.height - switcher1_size),
            );
        }

        let panel_first = data
            .panel
            .active_panel_at_position(&self.position.first())
            .and_then(|(panel, shown)| if shown { Some(panel) } else { None });
        let panel_second = data
            .panel
            .active_panel_at_position(&self.position.second())
            .and_then(|(panel, shown)| if shown { Some(panel) } else { None });

        match (panel_first, panel_second) {
            (Some(panel_first), Some(panel_second)) => {
                let split = match self.position {
                    PanelContainerPosition::Left => data.panel.size.left_split,
                    PanelContainerPosition::Bottom => data.panel.size.bottom_split,
                    PanelContainerPosition::Right => data.panel.size.right_split,
                };
                let separator = 4.0;
                if is_bottom {
                    let size_fist = ((self_size.width
                        - switcher0_size
                        - switcher1_size
                        - separator)
                        * split)
                        .round();
                    let size_second = self_size.width
                        - separator
                        - switcher0_size
                        - switcher1_size
                        - size_fist;
                    let panel_first = self.panels.get_mut(&panel_first).unwrap();
                    panel_first.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            size_fist,
                            self_size.height,
                        )),
                        data,
                        env,
                    );
                    panel_first.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(switcher0_size, 0.0),
                    );
                    let panel_second = self.panels.get_mut(&panel_second).unwrap();
                    panel_second.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            size_second,
                            self_size.height,
                        )),
                        data,
                        env,
                    );
                    panel_second.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(size_fist + switcher0_size + separator, 0.0),
                    );
                } else {
                    let size_fist = ((self_size.height
                        - switcher0_size
                        - switcher1_size
                        - separator)
                        * split)
                        .round();
                    let size_second = self_size.height
                        - separator
                        - switcher0_size
                        - switcher1_size
                        - size_fist;
                    let panel_first = self.panels.get_mut(&panel_first).unwrap();
                    panel_first.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            self_size.width,
                            size_fist,
                        )),
                        data,
                        env,
                    );
                    panel_first.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(0.0, switcher0_size),
                    );

                    let panel_second = self.panels.get_mut(&panel_second).unwrap();
                    panel_second.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            self_size.width,
                            size_second,
                        )),
                        data,
                        env,
                    );
                    panel_second.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(0.0, size_fist + switcher0_size + separator),
                    );
                }
            }
            (Some(panel), None) | (None, Some(panel)) => {
                let panel = self.panels.get_mut(&panel).unwrap();
                if is_bottom {
                    panel.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            self_size.width - switcher0_size - switcher1_size,
                            self_size.height,
                        )),
                        data,
                        env,
                    );
                    panel.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(switcher0_size, 0.0),
                    );
                } else {
                    panel.layout(
                        ctx,
                        &BoxConstraints::tight(Size::new(
                            self_size.width,
                            self_size.height - switcher0_size - switcher1_size,
                        )),
                        data,
                        env,
                    );
                    panel.set_origin(
                        ctx,
                        data,
                        env,
                        Point::new(0.0, switcher0_size),
                    );
                }
            }
            (None, None) => {}
        }

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        let rect = ctx.size().to_rect();
        match self.position {
            PanelContainerPosition::Left => {
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                );
                if shadow_width > 0.0 {
                    ctx.blurred_rect(
                        rect,
                        shadow_width,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x1 + 0.5, rect.y0),
                            Point::new(rect.x1 + 0.5, rect.y1),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            PanelContainerPosition::Right => {
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                );
                if shadow_width > 0.0 {
                    ctx.blurred_rect(
                        rect,
                        shadow_width,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0 - 0.5, rect.y0),
                            Point::new(rect.x0 - 0.5, rect.y1),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            PanelContainerPosition::Bottom => {
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                );
                if shadow_width > 0.0 {
                    ctx.blurred_rect(
                        rect,
                        shadow_width,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0, rect.y0 - 0.5),
                            Point::new(rect.x1, rect.y0 - 0.5),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
        }

        if let Some((panel, shown)) =
            data.panel.active_panel_at_position(&self.position.first())
        {
            if shown {
                let panel = self.panels.get_mut(&panel).unwrap();
                panel.paint(ctx, data, env);
            }
        }
        if let Some((panel, shown)) =
            data.panel.active_panel_at_position(&self.position.second())
        {
            if shown {
                let panel = self.panels.get_mut(&panel).unwrap();
                panel.paint(ctx, data, env);
            }
        }

        let is_bottom = self.position.is_bottom();
        if let Some((panel0, shown0)) =
            data.panel.active_panel_at_position(&self.position.first())
        {
            if shown0 {
                if let Some((panel1, shown1)) =
                    data.panel.active_panel_at_position(&self.position.second())
                {
                    if shown1 {
                        let panel0 = self.panels.get_mut(&panel0).unwrap();
                        let panel0_rect = panel0.layout_rect();
                        let panel1 = self.panels.get_mut(&panel1).unwrap();
                        let panel1_rect = panel1.layout_rect();
                        if is_bottom {
                            ctx.stroke(
                                Line::new(
                                    Point::new(panel0_rect.x1 + 0.5, panel0_rect.y0),
                                    Point::new(panel0_rect.x1 + 0.5, panel0_rect.y1),
                                ),
                                data.config
                                    .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                                1.0,
                            );
                            ctx.stroke(
                                Line::new(
                                    Point::new(panel1_rect.x0 - 0.5, panel1_rect.y0),
                                    Point::new(panel1_rect.x0 - 0.5, panel1_rect.y1),
                                ),
                                data.config
                                    .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                                1.0,
                            );
                        } else {
                            ctx.stroke(
                                Line::new(
                                    Point::new(panel0_rect.x0, panel0_rect.y1 + 0.5),
                                    Point::new(panel0_rect.x1, panel0_rect.y1 + 0.5),
                                ),
                                data.config
                                    .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                                1.0,
                            );
                            ctx.stroke(
                                Line::new(
                                    Point::new(panel1_rect.x0, panel1_rect.y0 - 0.5),
                                    Point::new(panel1_rect.x1, panel1_rect.y0 - 0.5),
                                ),
                                data.config
                                    .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                                1.0,
                            );
                        }
                    }
                }
            }
        }

        self.switcher0.paint(ctx, data, env);
        self.switcher1.paint(ctx, data, env);

        if self.panels.is_empty() {
            let text_layout = ctx
                .text()
                .new_text_layout("You can drag panel icon here")
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
            let x = (rect.width() - text_layout.size().width) / 2.0;
            ctx.draw_text(
                &text_layout,
                Point::new(x, text_layout.y_offset(rect.height())),
            );
        }
    }
}

pub struct PanelSwitcher {
    position: PanelPosition,
    icons: Vec<(PanelKind, LapceIcon)>,
    mouse_pos: Point,
    on_icon: bool,
    clicked_icon: Option<usize>,
    maximise_toggle: Option<Rect>,
    clicked_maximise: bool,
}

impl PanelSwitcher {
    pub fn new(position: PanelPosition) -> Self {
        Self {
            position,
            icons: Vec::new(),
            mouse_pos: Point::ZERO,
            on_icon: false,
            clicked_icon: None,
            maximise_toggle: None,
            clicked_maximise: false,
        }
    }

    fn icon_padding(data: &LapceTabData) -> f64 {
        ((data.config.ui.header_height() - data.config.ui.font_size()) as f64 * 0.5
            / 2.0)
            .round()
    }

    fn update_icons(&mut self, self_size: Size, data: &LapceTabData) {
        let mut icons = Vec::new();
        if let Some(order) = data.panel.order.get(&self.position) {
            for kind in order.iter() {
                icons.push(Self::panel_icon(kind, data));
            }
        }
        let switcher_size = data.config.ui.header_height() as f64;
        let icon_size = data.config.ui.font_size() as f64;
        self.maximise_toggle = None;
        if self.position.is_bottom() {
            for (i, (_, icon)) in icons.iter_mut().enumerate() {
                icon.rect = Rect::ZERO
                    .with_origin(Point::new(
                        self_size.width / 2.0,
                        (i as f64 + 0.5) * switcher_size,
                    ))
                    .inflate(icon_size / 2.0, icon_size / 2.0);
            }
            self.maximise_toggle = Some(
                Rect::ZERO
                    .with_origin(Point::new(
                        self_size.width / 2.0,
                        self_size.height - switcher_size / 2.0,
                    ))
                    .inflate(icon_size / 2.0, icon_size / 2.0),
            );
        } else {
            for (i, (_, icon)) in icons.iter_mut().enumerate() {
                icon.rect = Rect::ZERO
                    .with_origin(Point::new(
                        (i as f64 + 0.5) * switcher_size,
                        self_size.height / 2.0,
                    ))
                    .inflate(icon_size / 2.0, icon_size / 2.0);
            }
        }
        self.icons = icons;
    }

    fn panel_icon(kind: &PanelKind, data: &LapceTabData) -> (PanelKind, LapceIcon) {
        let cmd = match kind {
            PanelKind::FileExplorer => {
                LapceWorkbenchCommand::ToggleFileExplorerVisual
            }
            PanelKind::SourceControl => {
                LapceWorkbenchCommand::ToggleSourceControlVisual
            }
            PanelKind::Plugin => LapceWorkbenchCommand::TogglePluginVisual,
            PanelKind::Terminal => LapceWorkbenchCommand::ToggleTerminalVisual,
            PanelKind::Search => LapceWorkbenchCommand::ToggleSearchVisual,
            PanelKind::Problem => LapceWorkbenchCommand::ToggleProblemVisual,
            PanelKind::Debug => LapceWorkbenchCommand::ToggleDebugVisual,
        };
        (
            *kind,
            LapceIcon {
                icon: kind.svg_name(),
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Workbench(cmd),
                        data: None,
                    },
                    Target::Widget(data.id),
                ),
            },
        )
    }
}

impl Widget<LapceTabData> for PanelSwitcher {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if ctx.is_active() {
                    if let Some(i) = self.clicked_icon {
                        ctx.set_active(false);
                        let (kind, icon) = &self.icons[i];
                        let offset =
                            mouse_event.pos.to_vec2() - icon.rect.origin().to_vec2();
                        *Arc::make_mut(&mut data.drag) = Some((
                            offset,
                            mouse_event.window_pos.to_vec2(),
                            DragContent::Panel(*kind, icon.rect),
                        ));
                    }
                }
                self.mouse_pos = mouse_event.pos;
                let icon_padding = Self::icon_padding(data);
                for (_, icon) in self.icons.iter() {
                    let rect = icon.rect.inflate(icon_padding, icon_padding);
                    if rect.contains(self.mouse_pos) {
                        if !self.on_icon {
                            ctx.set_cursor(&Cursor::Pointer);
                            self.on_icon = true;
                            ctx.request_paint();
                        }
                        return;
                    }
                }
                if self
                    .maximise_toggle
                    .map(|r| {
                        r.inflate(icon_padding, icon_padding)
                            .contains(mouse_event.pos)
                    })
                    .unwrap_or(false)
                {
                    if !self.on_icon {
                        ctx.set_cursor(&Cursor::Pointer);
                        self.on_icon = true;
                        ctx.request_paint();
                    }
                    return;
                }
                if self.on_icon {
                    self.on_icon = false;
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                let icon_padding = Self::icon_padding(data);
                for (i, (_, icon)) in self.icons.iter().enumerate() {
                    let rect = icon.rect.inflate(icon_padding, icon_padding);
                    if rect.contains(mouse_event.pos) {
                        self.clicked_icon = Some(i);
                        ctx.set_active(true);
                        break;
                    }
                }
                self.clicked_maximise = false;
                if self
                    .maximise_toggle
                    .map(|r| {
                        r.inflate(icon_padding, icon_padding)
                            .contains(mouse_event.pos)
                    })
                    .unwrap_or(false)
                {
                    self.clicked_maximise = true;
                }
            }
            Event::MouseUp(mouse_event) => {
                ctx.set_active(false);
                let icon_padding = Self::icon_padding(data);
                for (i, (_, icon)) in self.icons.iter().enumerate() {
                    let rect = icon.rect.inflate(icon_padding, icon_padding);
                    if rect.contains(mouse_event.pos) {
                        if self.clicked_icon == Some(i) {
                            ctx.submit_command(icon.command.clone());
                        }
                        break;
                    }
                }
                if self.clicked_maximise
                    && self
                        .maximise_toggle
                        .map(|r| {
                            r.inflate(icon_padding, icon_padding)
                                .contains(mouse_event.pos)
                        })
                        .unwrap_or(false)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::ToggleMaximizedPanel,
                            ),
                            data: None,
                        },
                        Target::Widget(data.id),
                    ));
                }
                self.clicked_icon = None;
                self.clicked_maximise = false;
            }
            _ => (),
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
        let self_size = if self.position.is_bottom() {
            Size::new(data.config.ui.header_height() as f64, bc.max().height)
        } else {
            Size::new(bc.max().width, data.config.ui.header_height() as f64)
        };
        self.update_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let should_show = data
            .panel
            .order
            .get(&self.position)
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !should_show {
            return;
        }

        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(if self.position.is_bottom() {
                    LapceTheme::EDITOR_BACKGROUND
                } else {
                    LapceTheme::PANEL_BACKGROUND
                }),
        );
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        match self.position {
            PanelPosition::LeftTop | PanelPosition::RightTop => {
                if shadow_width > 0.0 {
                    ctx.with_save(|ctx| {
                        ctx.clip(rect.inset((0.0, 0.0, 0.0, 50.0)));
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    });
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0, rect.y1 + 0.5),
                            Point::new(rect.x1, rect.y1 + 0.5),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            PanelPosition::LeftBottom | PanelPosition::RightBottom => {
                if shadow_width > 0.0 {
                    ctx.with_save(|ctx| {
                        ctx.clip(rect.inset((0.0, 50.0, 0.0, 0.0)));
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    });
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0, rect.y0 - 0.5),
                            Point::new(rect.x1, rect.y0 - 0.5),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            PanelPosition::BottomLeft => {
                if shadow_width > 0.0 {
                    ctx.with_save(|ctx| {
                        ctx.clip(rect.inset((0.0, 0.0, 50.0, 0.0)));
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    });
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x1 + 0.5, rect.y0),
                            Point::new(rect.x1 + 0.5, rect.y1),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            PanelPosition::BottomRight => {
                if shadow_width > 0.0 {
                    ctx.with_save(|ctx| {
                        ctx.clip(rect.inset((50.0, 0.0, 0.0, 0.0)));
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    });
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0 - 0.5, rect.y0),
                            Point::new(rect.x0 - 0.5, rect.y1),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
        }

        let icon_padding = Self::icon_padding(data);
        let is_bottom = self.position.is_bottom();
        let mut active_kinds = Vec::new();
        if let Some((panel, shown)) =
            data.panel.active_panel_at_position(&self.position)
        {
            if shown {
                active_kinds.push(panel);
            }
        }
        for (kind, icon) in self.icons.iter() {
            let mouse_rect = icon.rect.inflate(icon_padding, icon_padding);
            if mouse_rect.contains(self.mouse_pos) {
                ctx.fill(
                    mouse_rect,
                    &data.config.get_hover_color(if is_bottom {
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    } else {
                        data.config
                            .get_color_unchecked(LapceTheme::PANEL_BACKGROUND)
                    }),
                );
            }
            if active_kinds.contains(kind) {
                match self.position {
                    PanelPosition::LeftTop | PanelPosition::RightTop => {
                        ctx.stroke(
                            Line::new(
                                Point::new(mouse_rect.x0, mouse_rect.y1 + 1.0),
                                Point::new(mouse_rect.x1, mouse_rect.y1 + 1.0),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        );
                    }
                    PanelPosition::LeftBottom | PanelPosition::RightBottom => {
                        ctx.stroke(
                            Line::new(
                                Point::new(mouse_rect.x0, mouse_rect.y0 - 1.0),
                                Point::new(mouse_rect.x1, mouse_rect.y0 - 1.0),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        );
                    }
                    PanelPosition::BottomLeft => {
                        ctx.stroke(
                            Line::new(
                                Point::new(mouse_rect.x0 - 1.0, mouse_rect.y0),
                                Point::new(mouse_rect.x0 - 1.0, mouse_rect.y1),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        );
                    }
                    PanelPosition::BottomRight => {
                        ctx.stroke(
                            Line::new(
                                Point::new(mouse_rect.x1 + 1.0, mouse_rect.y0),
                                Point::new(mouse_rect.x1 + 1.0, mouse_rect.y1),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        );
                    }
                }
            }
            let svg = data.config.ui_svg(icon.icon);
            ctx.draw_svg(
                &svg,
                icon.rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                ),
            );
        }

        if let Some(rect) = self.maximise_toggle {
            let maximized = data
                .panel
                .style
                .get(&PanelPosition::BottomLeft)
                .map(|s| s.maximized)
                .unwrap_or(false)
                || data
                    .panel
                    .style
                    .get(&PanelPosition::BottomRight)
                    .map(|s| s.maximized)
                    .unwrap_or(false);
            let mouse_rect = rect.inflate(icon_padding, icon_padding);
            if mouse_rect.contains(self.mouse_pos) {
                ctx.fill(
                    mouse_rect,
                    &data.config.get_hover_color(if is_bottom {
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    } else {
                        data.config
                            .get_color_unchecked(LapceTheme::PANEL_BACKGROUND)
                    }),
                );
            }
            let svg = if maximized {
                data.config.ui_svg(LapceIcons::PANEL_RESTORE)
            } else {
                data.config.ui_svg(LapceIcons::PANEL_MAXIMISE)
            };
            ctx.draw_svg(
                &svg,
                rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                ),
            );
        }
    }
}
