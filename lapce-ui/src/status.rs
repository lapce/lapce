use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    Command, Data, Event, EventCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, Widget,
};
use lapce_core::mode::Mode;
use lapce_data::{
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand, LAPCE_COMMAND},
    config::{LapceConfig, LapceIcons, LapceTheme},
    data::LapceTabData,
    panel::PanelContainerPosition,
};

use crate::tab::LapceIcon;

pub struct LapceStatus {
    panel_icons: Vec<LapceIcon>,
    clickable_items: Vec<(Rect, Command)>,
    mouse_pos: Point,
    icon_size: f64,
    active_icon: Option<Rect>,
}

impl LapceStatus {
    pub fn new() -> Self {
        Self {
            panel_icons: Vec::new(),
            clickable_items: Vec::new(),
            mouse_pos: Point::ZERO,
            icon_size: 13.0,
            active_icon: None,
        }
    }

    fn panel_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let icons = [
            (
                if data.panel.is_container_shown(&PanelContainerPosition::Left) {
                    LapceIcons::SIDEBAR_LEFT
                } else {
                    LapceIcons::SIDEBAR_LEFT_OFF
                },
                LapceWorkbenchCommand::TogglePanelLeftVisual,
            ),
            (
                if data
                    .panel
                    .is_container_shown(&PanelContainerPosition::Bottom)
                {
                    LapceIcons::LAYOUT_PANEL
                } else {
                    LapceIcons::LAYOUT_PANEL_OFF
                },
                LapceWorkbenchCommand::TogglePanelBottomVisual,
            ),
            (
                if data
                    .panel
                    .is_container_shown(&PanelContainerPosition::Right)
                {
                    LapceIcons::SIDEBAR_RIGHT
                } else {
                    LapceIcons::SIDEBAR_RIGHT_OFF
                },
                LapceWorkbenchCommand::TogglePanelRightVisual,
            ),
        ];

        let panel_icons_size = self_size.height * icons.len() as f64;
        let offset = (self_size.width - panel_icons_size) / 2.0;

        let icons: Vec<LapceIcon> = icons
            .into_iter()
            .enumerate()
            .map(|(i, (svg, cmd))| LapceIcon {
                icon: svg,
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
            })
            .collect();
        icons
    }

    fn icon_hit_test(&mut self, mouse_event: &MouseEvent) -> bool {
        for icon in self.panel_icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                self.active_icon = Some(icon.rect);
                return true;
            }
        }
        for (rect, _) in self.clickable_items.iter() {
            if rect.contains(mouse_event.pos) {
                self.active_icon = Some(*rect);
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.panel_icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
                return;
            }
        }
        for (rect, cmd) in self.clickable_items.iter() {
            if rect.contains(mouse_event.pos) {
                ctx.submit_command(cmd.clone());
                return;
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
        config: &LapceConfig,
    ) -> (f64, Option<(Rect, Svg)>, (Point, PietTextLayout)) {
        let fg_color = config.get_color_unchecked(LapceTheme::STATUS_FOREGROUND);

        let text_layout = ctx
            .text()
            .new_text_layout(label)
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(fg_color.clone())
            .build()
            .unwrap();

        let icon_padding = (height - self.icon_size) / 2.0;

        let mut left = left;

        let svg = {
            let warnings_icon = config.ui_svg(icon);
            let rect = Size::new(height, height)
                .to_rect()
                .inflate(-icon_padding, -icon_padding)
                .with_origin(Point::new(left + 2.0 * icon_padding, icon_padding));

            left += icon_padding + height;
            Some((rect, warnings_icon))
        };

        let point = Point::new(left, text_layout.y_offset(height));
        (left + text_layout.size().width, svg, (point, text_layout))
    }

    fn paint_icon_with_label_from_right(
        &self,
        right: f64,
        height: f64,
        icon: Option<&'static str>,
        label: String,
        ctx: &mut PaintCtx,
        config: &LapceConfig,
    ) -> (f64, Option<(Rect, Svg)>, (Point, PietTextLayout)) {
        let fg_color = config.get_color_unchecked(LapceTheme::STATUS_FOREGROUND);

        let text_layout = ctx
            .text()
            .new_text_layout(label)
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(fg_color.clone())
            .build()
            .unwrap();

        let icon_padding = (height - self.icon_size) / 2.0;

        let mut right = right;

        let svg = if let Some(icon) = icon {
            let warnings_icon = config.ui_svg(icon);
            let rect = Size::new(height, height)
                .to_rect()
                .inflate(-icon_padding, -icon_padding)
                .with_origin(Point::new(right - 2.0 * icon_padding, icon_padding));

            right -= icon_padding + height;
            Some((rect, warnings_icon))
        } else {
            None
        };

        let point = Point::new(
            right - text_layout.size().width,
            text_layout.y_offset(height),
        );
        (right - text_layout.size().width, svg, (point, text_layout))
    }
}

impl Default for LapceStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for LapceStatus {
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
                let active_icon = self.active_icon;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    self.active_icon = None;
                    ctx.clear_cursor();
                }
                if active_icon != self.active_icon {
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
                if old_data.cursor.get_mode() != data.cursor.get_mode()
                    || old_data.editor_id != data.editor_id
                {
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

        if !old_data.progresses.same(&data.progresses) {
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
        self.clickable_items.clear();
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
        let mut _right = 0.0;

        if data.config.core.modal {
            let (mode, color) = match data.mode() {
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
                Point::new(5.0, text_layout.y_offset(size.height)),
            );
            left += text_size.width + 10.0;
        }

        let x = left + 5.0;
        let (new_left, error_svg, (error_point, error_text_layout)) = self
            .paint_icon_with_label(
                left,
                size.height,
                LapceIcons::ERROR,
                data.main_split.error_count.to_string(),
                ctx,
                &data.config,
            );
        left = new_left;
        let (new_left, warning_svg, (warning_point, warning_text_layout)) = self
            .paint_icon_with_label(
                left - 5.0,
                size.height,
                LapceIcons::WARNING,
                data.main_split.warning_count.to_string(),
                ctx,
                &data.config,
            );
        left = new_left;

        let problem_rect = Rect::ZERO
            .with_origin(Point::new(x, 0.0))
            .with_size(Size::new(left + 5.0 - x, size.height));
        if problem_rect.contains(self.mouse_pos) {
            ctx.fill(
                problem_rect,
                data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
            );
        }
        if let Some((rect, svg)) = error_svg {
            ctx.draw_svg(
                &svg,
                rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::STATUS_FOREGROUND),
                ),
            );
        }
        ctx.draw_text(&error_text_layout, error_point);
        if let Some((rect, svg)) = warning_svg {
            ctx.draw_svg(
                &svg,
                rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::STATUS_FOREGROUND),
                ),
            );
        }
        ctx.draw_text(&warning_text_layout, warning_point);
        self.clickable_items.push((
            problem_rect,
            Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::ToggleProblemVisual,
                    ),
                    data: None,
                },
                Target::Widget(data.id),
            ),
        ));

        for progress in data.progresses.iter() {
            let mut text = progress.title.clone();
            if let Some(message) = progress.message.as_ref() {
                if text.len() + message.len() < 48 {
                    text += ": ";
                    text += message;
                }
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
                Point::new(left + 10.0, text_layout.y_offset(size.height)),
            );
            left += 10.0 + text_layout.size().width;
        }

        let icon_padding = (size.height - self.icon_size) / 2.0;
        for icon in self.panel_icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    icon.rect,
                    data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
                );
            }
            {
                let svg = data.config.ui_svg(icon.icon);
                ctx.draw_svg(
                    &svg,
                    icon.rect.inflate(-icon_padding, -icon_padding),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::STATUS_FOREGROUND),
                    ),
                );
            }
        }

        let mut right = size.width - 5.0;
        if let Some(editor) = &data.main_split.active_editor() {
            let lang = match data.main_split.content_doc(&editor.content).syntax() {
                Some(v) => v.language.to_string(),
                None => String::from("Plain Text"),
            };
            let x1 = right;
            let (new_right, svg, (point, text_layout)) = self
                .paint_icon_with_label_from_right(
                    right - 5.0,
                    size.height,
                    None,
                    lang,
                    ctx,
                    &data.config,
                );
            right = new_right;
            let x0 = right - 5.0;
            let rect = Rect::ZERO
                .with_origin(Point::new(x0, 0.0))
                .with_size(Size::new(x1 - x0, size.height));
            if rect.contains(self.mouse_pos) {
                ctx.fill(
                    rect,
                    data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
                );
            }
            if let Some((rect, svg)) = svg {
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    ),
                );
            }
            ctx.draw_text(&text_layout, point);
            self.clickable_items.push((
                rect,
                Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Workbench(
                            LapceWorkbenchCommand::ChangeFileLanguage,
                        ),
                        data: None,
                    },
                    Target::Widget(data.id),
                ),
            ));

            let mut string = "".to_string();
            let editor_content = data.editor_view_content(editor.view_id);
            if let Some(cursor_pos) =
                editor.cursor.get_line_col_char(editor_content.doc.buffer())
            {
                string += &format!(
                    "Ln {}, Col {}, Char {}",
                    cursor_pos.0 + 1,
                    cursor_pos.1 + 1,
                    cursor_pos.2
                );
            }

            if let Some(selection) = editor.cursor.get_selection() {
                let selection_range = selection.0.abs_diff(selection.1);

                if selection.0 != selection.1 {
                    string += &format!(" ({} selected)", selection_range);
                }
            }
            let selection_count = editor.cursor.get_selection_count();
            if selection_count > 1 {
                string += &format!(" {} selections", selection_count);
            }

            if !string.is_empty() {
                let (_, _, (point, text_layout)) = self
                    .paint_icon_with_label_from_right(
                        right - text_layout.size().width,
                        size.height,
                        None,
                        string,
                        ctx,
                        &data.config,
                    );
                ctx.draw_text(&text_layout, point);
            }
        }
    }
}
