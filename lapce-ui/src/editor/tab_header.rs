use druid::{
    kurbo::Line, BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{
        CommandTarget, LapceCommand, LapceCommandNew, LapceUICommand,
        LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::LapceTabData,
};

use crate::{
    editor::tab_header_content::LapceEditorTabHeaderContent, scroll::LapceScrollNew,
    svg::get_svg, tab::LapceIcon,
};

pub struct LapceEditorTabHeader {
    pub widget_id: WidgetId,
    pub content: WidgetPod<
        LapceTabData,
        LapceScrollNew<LapceTabData, LapceEditorTabHeaderContent>,
    >,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
    is_hot: bool,
}

impl LapceEditorTabHeader {
    pub fn new(widget_id: WidgetId) -> Self {
        let content =
            LapceScrollNew::new(LapceEditorTabHeaderContent::new(widget_id))
                .horizontal();
        Self {
            widget_id,
            content: WidgetPod::new(content),
            icons: Vec::new(),
            mouse_pos: Point::ZERO,
            is_hot: false,
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

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorTabHeader {
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
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::EnsureEditorTabActiveVisble = command {
                    let editor_tab =
                        data.main_split.editor_tabs.get(&self.widget_id).unwrap();
                    let active = editor_tab.active;
                    if active < self.content.widget().child().rects.len() {
                        let rect = self.content.widget().child().rects[active].rect;
                        if self.content.widget_mut().scroll_to_visible(rect, env) {
                            self.content
                                .widget_mut()
                                .scroll_component
                                .reset_scrollbar_fade(|d| ctx.request_timer(d), env);
                        }
                    }
                }
            }
            _ => (),
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
        if let LifeCycle::HotChanged(is_hot) = event {
            self.is_hot = *is_hot;
            ctx.request_layout();
        }
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.icons.clear();

        let size = if data.config.editor.show_tab {
            let height = 30.0;
            let size = Size::new(bc.max().width, height);

            let editor_tab =
                data.main_split.editor_tabs.get(&self.widget_id).unwrap();
            if self.is_hot || *editor_tab.content_is_hot.borrow() {
                let icon_size = 24.0;
                let gap = (height - icon_size) / 2.0;
                let x =
                    size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
                let icon = LapceIcon {
                    icon: "close.svg".to_string(),
                    rect: Size::new(icon_size, icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitClose,
                        Target::Widget(self.widget_id),
                    ),
                };
                self.icons.push(icon);

                let x =
                    size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
                let icon = LapceIcon {
                    icon: "split-horizontal.svg".to_string(),
                    rect: Size::new(icon_size, icon_size)
                        .to_rect()
                        .with_origin(Point::new(x, gap)),
                    command: Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: LapceCommand::SplitVertical.to_string(),
                            data: None,
                            palette_desc: None,
                            target: CommandTarget::Focus,
                        },
                        Target::Widget(self.widget_id),
                    ),
                };
                self.icons.push(icon);
            }

            size
        } else {
            Size::new(bc.max().width, 0.0)
        };
        self.content.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                size.width - self.icons.len() as f64 * size.height,
                size.height,
            )),
            data,
            env,
        );
        self.content.set_origin(ctx, data, env, Point::ZERO);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        ctx.stroke(
            Line::new(
                Point::new(0.0, size.height - 0.5),
                Point::new(size.width, size.height - 0.5),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        self.content.paint(ctx, data, env);

        let svg_padding = 4.0;
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
                    icon.rect.inflate(-svg_padding, -svg_padding),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                );
            }
        }
        if !self.icons.is_empty() {
            let x = size.width - self.icons.len() as f64 * size.height - 0.5;
            ctx.stroke(
                Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }
}
