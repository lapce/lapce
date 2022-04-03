use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    Color, Command, Event, EventCtx, FontFamily, MouseEvent, Point, RenderContext,
    Size, Target, Widget,
};
use lapce_data::{
    command::{
        CommandTarget, LapceCommandNew, LapceWorkbenchCommand, LAPCE_NEW_COMMAND,
    },
    config::LapceTheme,
    data::{FocusArea, LapceTabData, PanelKind},
    panel::PanelPosition,
    state::Mode,
    svg::get_svg, 
};

use crate::tab::LapceIcon;

pub struct LapceStatusNew {
    height: f64,
    panel_icons: Vec<LapceIcon>,
    mouse_pos: Point,
    icon_size: f64,
}

impl LapceStatusNew {
    pub fn new() -> Self {
        Self {
            height: 25.0,
            panel_icons: Vec::new(),
            mouse_pos: Point::ZERO,
            icon_size: 13.0,
        }
    }

    fn panel_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let left_panels = data
            .panels
            .get(&PanelPosition::BottomLeft)
            .map(|p| p.widgets.clone())
            .unwrap_or_default();
        let mut right_panels = data
            .panels
            .get(&PanelPosition::BottomRight)
            .map(|p| p.widgets.clone())
            .unwrap_or_default();
        let mut panels = left_panels;
        panels.append(&mut right_panels);

        let panel_icons_size = self_size.height * panels.len() as f64;
        let offset = (self_size.width - panel_icons_size) / 2.0;

        let icons: Vec<LapceIcon> = panels
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = match p {
                    PanelKind::FileExplorer => {
                        LapceWorkbenchCommand::ToggleFileExplorerVisual
                    }
                    PanelKind::SourceControl => {
                        LapceWorkbenchCommand::ToggleSourceControlVisual
                    }
                    PanelKind::Plugin => LapceWorkbenchCommand::TogglePluginVisual,
                    PanelKind::Terminal => {
                        LapceWorkbenchCommand::ToggleTerminalVisual
                    }
                    PanelKind::Search => LapceWorkbenchCommand::ToggleSearchVisual,
                    PanelKind::Problem => LapceWorkbenchCommand::ToggleProblemVisual,
                };

                LapceIcon {
                    icon: p.svg_name().to_string(),
                    rect: Size::new(self_size.height, self_size.height)
                        .to_rect()
                        .with_origin(Point::new(
                            offset + self_size.height * i as f64,
                            0.0,
                        )),
                    command: Command::new(
                        LAPCE_NEW_COMMAND,
                        LapceCommandNew {
                            cmd: cmd.to_string(),
                            data: None,
                            palette_desc: None,
                            target: CommandTarget::Workbench,
                        },
                        Target::Widget(data.id),
                    ),
                }
            })
            .collect();
        icons
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.panel_icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.panel_icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }
}

impl Default for LapceStatusNew {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for LapceStatusNew {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &druid::Env,
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
        _ctx: &mut druid::LifeCycleCtx,
        _event: &druid::LifeCycle,
        _data: &LapceTabData,
        _env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &druid::Env,
    ) {
        match (
            old_data.main_split.active_editor(),
            data.main_split.active_editor(),
        ) {
            (Some(old_data), Some(data)) => {
                if old_data.cursor.get_mode() != data.cursor.get_mode() {
                    ctx.request_paint();
                }
            }
            (None, None) => (),
            _ => ctx.request_paint(),
        }

        if old_data.main_split.warning_count != data.main_split.warning_count
            || old_data.main_split.error_count != data.main_split.error_count
        {
            ctx.request_paint();
            return;
        }

        if !old_data.progresses.ptr_eq(&data.progresses) {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceTabData,
        _env: &druid::Env,
    ) -> Size {
        let self_size = Size::new(bc.max().width, self.height);
        self.panel_icons = self.panel_icons(self_size, data);
        self_size
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceTabData,
        _env: &druid::Env,
    ) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.blurred_rect(
            rect,
            5.0,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::STATUS_BACKGROUND),
        );

        let mut left = 0.0;

        if data.config.lapce.modal {
            let (mode, color) = {
                let mode =
                    if data.focus_area == FocusArea::Panel(PanelKind::Terminal) {
                        data.terminal
                            .terminals
                            .get(&data.terminal.active_term_id)
                            .unwrap()
                            .mode
                    } else {
                        data.main_split
                            .active_editor()
                            .map(|e| e.cursor.get_mode())
                            .unwrap_or(Mode::Normal)
                    };
                match mode {
                    Mode::Normal => ("Normal", Color::rgb8(64, 120, 242)),
                    Mode::Insert => ("Insert", Color::rgb8(228, 86, 73)),
                    Mode::Visual => ("Visual", Color::rgb8(193, 132, 1)),
                    Mode::Terminal => ("Terminal", Color::rgb8(228, 86, 73)),
                }
            };

            let text_layout = ctx
                .text()
                .new_text_layout(mode)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let fill_size = Size::new(text_size.width + 10.0, size.height);
            ctx.fill(fill_size.to_rect(), &color);
            ctx.draw_text(&text_layout, Point::new(5.0, 4.0));
            left += text_size.width + 10.0;
        }

        let text_layout = ctx
            .text()
            .new_text_layout(format!(
                "{}  {}",
                data.main_split.error_count, data.main_split.warning_count
            ))
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(&text_layout, Point::new(left + 10.0, 4.0));
        left += 10.0 + text_layout.size().width;

        for progress in data.progresses.iter() {
            let mut text = progress.title.clone();
            let message = progress.message.clone().unwrap_or_else(|| "".to_string());
            if !message.is_empty() {
                text += ": ";
                text += &message;
            }
            let text_layout = ctx
                .text()
                .new_text_layout(text)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(&text_layout, Point::new(left + 10.0, 4.0));
            left += 10.0 + text_layout.size().width;
        }

        let icon_padding = (self.height - self.icon_size) / 2.0;
        for icon in self.panel_icons.iter() {
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
    }
}
