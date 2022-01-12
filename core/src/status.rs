use druid::piet::Text;
use druid::piet::TextLayout;
use druid::piet::TextLayoutBuilder;
use druid::theme;
use druid::Color;
use druid::Command;
use druid::EventCtx;
use druid::MouseEvent;
use druid::Target;
use druid::Vec2;
use druid::{
    kurbo::Line, Event, FontDescriptor, FontFamily, Point, RenderContext, Size,
    Widget, WidgetId, WindowId,
};
use lsp_types::DiagnosticSeverity;

use crate::command::CommandTarget;
use crate::command::LapceCommandNew;
use crate::command::LapceWorkbenchCommand;
use crate::command::LAPCE_NEW_COMMAND;
use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};
use crate::config::LapceTheme;
use crate::data::FocusArea;
use crate::data::LapceTabData;
use crate::data::PanelKind;
use crate::panel::PanelPosition;
use crate::state::Mode;
use crate::svg::get_svg;
use crate::tab::LapceIcon;
use crate::theme::OldLapceTheme;

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
            .map(|p| {
                p.widgets
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect::<Vec<PanelKind>>()
            })
            .unwrap_or(Vec::new());
        let mut right_panels = data
            .panels
            .get(&PanelPosition::BottomRight)
            .map(|p| {
                p.widgets
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect::<Vec<PanelKind>>()
            })
            .unwrap_or(Vec::new());
        let mut panels = left_panels;
        panels.append(&mut right_panels);

        let panel_icons_size = self_size.height * panels.len() as f64;
        let offset = (self_size.width - panel_icons_size) / 2.0;

        let icons: Vec<LapceIcon> = panels
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = match p {
                    PanelKind::FileExplorer => LapceWorkbenchCommand::ToggleTerminal,
                    PanelKind::SourceControl => {
                        LapceWorkbenchCommand::ToggleSourceControl
                    }
                    PanelKind::Plugin => LapceWorkbenchCommand::TogglePlugin,
                    PanelKind::Terminal => LapceWorkbenchCommand::ToggleTerminal,
                    PanelKind::Problem => LapceWorkbenchCommand::ToggleProblem,
                };
                LapceIcon {
                    icon: p.svg_name(),
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

impl Widget<LapceTabData> for LapceStatusNew {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &druid::Env,
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
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &LapceTabData,
        env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &druid::Env,
    ) {
        if old_data.main_split.active_editor().cursor.get_mode()
            != data.main_split.active_editor().cursor.get_mode()
        {
            ctx.request_paint();
            return;
        }

        if old_data.main_split.warning_count != data.main_split.warning_count
            || old_data.main_split.error_count != data.main_split.error_count
        {
            ctx.request_paint();
            return;
        }

        if !old_data.progresses.ptr_eq(&data.progresses) {
            ctx.request_paint();
            return;
        }
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &LapceTabData,
        env: &druid::Env,
    ) -> Size {
        let self_size = Size::new(bc.max().width, self.height);
        self.panel_icons = self.panel_icons(self_size, data);
        self_size
    }

    fn paint(
        &mut self,
        ctx: &mut druid::PaintCtx,
        data: &LapceTabData,
        env: &druid::Env,
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
                        data.main_split.active_editor().cursor.get_mode()
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
            let message = progress.message.clone().unwrap_or("".to_string());
            if message != "" {
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
