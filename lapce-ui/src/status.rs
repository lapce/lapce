use druid::{
    kurbo::Line,
    piet::{Text, TextLayout, TextLayoutBuilder},
    Command, Event, EventCtx, MouseEvent, PaintCtx, Point, RenderContext, Size,
    Target, Widget,
};
use lapce_core::mode::Mode;
use lapce_data::{
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand, LAPCE_COMMAND},
    config::{Config, LapceTheme},
    data::{FocusArea, LapceTabData, PanelKind},
    panel::PanelPosition,
};

use crate::{svg::get_svg, tab::LapceIcon};

pub struct LapceStatusNew {
    panel_icons: Vec<LapceIcon>,
    mouse_pos: Point,
    icon_size: f64,
}

impl LapceStatusNew {
    pub fn new() -> Self {
        Self {
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
                    icon: p.svg_name(),
                    rect: Size::new(self_size.height, self_size.height)
                        .to_rect()
                        .with_origin(Point::new(
                            offset + self_size.height * i as f64,
                            0.0,
                        )),
                    command: Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(cmd),
                            data: None,
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

    fn paint_icon_with_label(
        &self,
        left: f64,
        height: f64,
        icon: &'static str,
        label: String,
        ctx: &mut PaintCtx,
        config: &Config,
    ) -> f64 {
        let fg_color = config.get_color_unchecked(LapceTheme::EDITOR_FOREGROUND);

        let text_layout = ctx
            .text()
            .new_text_layout(label)
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(fg_color.clone())
            .build()
            .unwrap();

        let icon_padding = (height - self.icon_size) / 2.0;

        let mut left = left;

        if let Some(warnings_icon) = get_svg(icon) {
            let rect = Size::new(height, height)
                .to_rect()
                .inflate(-icon_padding, -icon_padding)
                .with_origin(Point::new(left + 2.0 * icon_padding, icon_padding));
            ctx.draw_svg(&warnings_icon, rect, Some(fg_color));

            left += icon_padding + height;
        }

        ctx.draw_text(
            &text_layout,
            Point::new(left, (height - text_layout.size().height) / 2.0),
        );
        left + text_layout.size().width
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
        let self_size =
            Size::new(bc.max().width, data.config.ui.status_height() as f64);
        self.panel_icons = self.panel_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &druid::Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
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
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::STATUS_BACKGROUND),
        );

        let mut left = 0.0;

        if data.config.lapce.modal {
            let mode = if data.focus_area == FocusArea::Panel(PanelKind::Terminal) {
                data.terminal
                    .terminals
                    .get(&data.terminal.active_term_id)
                    .map(|terminal| terminal.mode)
            } else {
                data.main_split.active_editor().map(|e| e.cursor.get_mode())
            };

            let (mode, color) = match mode.unwrap_or(Mode::Normal) {
                Mode::Normal => ("Normal", LapceTheme::STATUS_MODAL_NORMAL),
                Mode::Insert => ("Insert", LapceTheme::STATUS_MODAL_INSERT),
                Mode::Visual => ("Visual", LapceTheme::STATUS_MODAL_VISUAL),
                Mode::Terminal => ("Terminal", LapceTheme::STATUS_MODAL_TERMINAL),
            };

            let text_layout = ctx
                .text()
                .new_text_layout(mode)
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let fill_size = Size::new(text_size.width + 10.0, size.height);
            ctx.fill(fill_size.to_rect(), data.config.get_color_unchecked(color));
            ctx.draw_text(
                &text_layout,
                Point::new(5.0, (size.height - text_layout.size().height) / 2.0),
            );
            left += text_size.width + 10.0;
        }

        left = self.paint_icon_with_label(
            left,
            size.height,
            "error.svg",
            data.main_split.error_count.to_string(),
            ctx,
            &data.config,
        );
        left = self.paint_icon_with_label(
            left - 5.0,
            size.height,
            "warning.svg",
            data.main_split.warning_count.to_string(),
            ctx,
            &data.config,
        );

        for progress in data.progresses.iter() {
            let mut text = progress.title.clone();
            if let Some(message) = progress.message.as_ref() {
                text += ": ";
                text += message;
            }
            let text_layout = ctx
                .text()
                .new_text_layout(text)
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
            ctx.draw_text(
                &text_layout,
                Point::new(
                    left + 10.0,
                    (size.height - text_layout.size().height) / 2.0,
                ),
            );
            left += 10.0 + text_layout.size().width;
        }

        let icon_padding = (size.height - self.icon_size) / 2.0;
        for icon in self.panel_icons.iter() {
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
    }
}
