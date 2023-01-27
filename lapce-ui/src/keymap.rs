use std::sync::Arc;

use druid::{
    kurbo::Line,
    piet::{Text, TextAttribute, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontWeight, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_core::mode::Modes;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    keypress::{
        paint_key, Alignment, DefaultKeyPressHandler, KeyMap, KeyPress, KeyPressData,
    },
};

use crate::{editor::view::LapceEditorView, scroll::LapceScroll, split::LapceSplit};

pub struct LapceKeymap {
    widget_id: WidgetId,
    active_keymap: Option<(KeyMap, Vec<KeyPress>)>,
    keymap_confirm: Rect,
    keymap_cancel: Rect,
    line_height: f64,
}

impl LapceKeymap {
    pub fn new_split(keymap_input_view_id: WidgetId) -> LapceSplit {
        let keymap = Self {
            widget_id: WidgetId::next(),
            active_keymap: None,
            line_height: 35.0,
            keymap_confirm: Rect::ZERO,
            keymap_cancel: Rect::ZERO,
        };
        let keymap = LapceScroll::new(keymap);

        let input =
            LapceEditorView::new(keymap_input_view_id, WidgetId::next(), None)
                .hide_header()
                .hide_gutter()
                .padding((15.0, 15.0));
        let header = LapceKeymapHeader::new();
        let split = LapceSplit::new(WidgetId::next())
            .horizontal()
            .with_child(input.boxed(), None, 100.0)
            .with_child(header.boxed(), None, 100.0)
            .with_flex_child(keymap.boxed(), None, 1.0, false);

        split
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        ev: &druid::MouseEvent,
        data: &LapceTabData,
    ) {
        use druid::MouseButton as Btn;

        if let Some((keymap, keys)) = self.active_keymap.as_mut() {
            match ev.button {
                Btn::Left if self.keymap_confirm.contains(ev.pos) => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateKeymap(keymap.clone(), keys.clone()),
                        Target::Widget(data.id),
                    ));
                    self.active_keymap = None;
                }
                Btn::Left if self.keymap_cancel.contains(ev.pos) => {
                    self.active_keymap = None;
                }
                _other => {
                    if keys.len() == 2 {
                        keys.clear();
                    }
                    keys.push(KeyPress::mouse(ev));
                    ctx.request_paint();
                    ctx.set_handled();
                }
            }
            return;
        }
        let commands_with_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_with_keymap
        } else {
            &data.keypress.filtered_commands_with_keymap
        };

        let commands_without_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_without_keymap
        } else {
            &data.keypress.filtered_commands_without_keymap
        };

        let i = (ev.pos.y / self.line_height).floor() as usize;
        if i < commands_with_keymap.len() {
            let keymap = commands_with_keymap[i].clone();
            self.active_keymap = Some((keymap, Vec::new()));
        } else {
            let j = i - commands_with_keymap.len();
            if let Some(command) = commands_without_keymap.get(j) {
                self.active_keymap = Some((
                    KeyMap {
                        command: command.kind.str().to_string(),
                        key: Vec::new(),
                        modes: Modes::empty(),
                        when: None,
                    },
                    Vec::new(),
                ));
            }
        }
    }

    fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        data.focus = Arc::new(self.widget_id);
        ctx.request_focus();
    }
}

impl Widget<LapceTabData> for LapceKeymap {
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
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx, data);
                }
            }
            Event::MouseMove(_mouse_event) => {
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.request_focus(ctx, data);
                self.mouse_down(ctx, mouse_event, data);
                ctx.request_paint();
            }
            Event::KeyDown(key_event) => {
                if let Some((_keymap, keys)) = self.active_keymap.as_mut() {
                    if let Some(keypress) = KeyPressData::keypress(key_event) {
                        if keys.len() == 2 {
                            keys.clear();
                        }
                        keys.push(keypress);
                        ctx.request_paint();
                        ctx.set_handled();
                    }
                } else {
                    let mut keypress = data.keypress.clone();
                    Arc::make_mut(&mut keypress).key_down(
                        ctx,
                        key_event,
                        &mut DefaultKeyPressHandler {},
                        env,
                    );
                }
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
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if !data
            .keypress
            .commands_with_keymap
            .same(&old_data.keypress.commands_with_keymap)
            || !data
                .keypress
                .commands_without_keymap
                .same(&old_data.keypress.commands_without_keymap)
            || data.keypress.filter_pattern != old_data.keypress.filter_pattern
            || !data
                .keypress
                .filtered_commands_with_keymap
                .same(&old_data.keypress.filtered_commands_with_keymap)
            || !data
                .keypress
                .filtered_commands_without_keymap
                .same(&old_data.keypress.filtered_commands_without_keymap)
        {
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let commands_with_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_with_keymap
        } else {
            &data.keypress.filtered_commands_with_keymap
        };

        let commands_without_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_without_keymap
        } else {
            &data.keypress.filtered_commands_without_keymap
        };

        Size::new(
            bc.max().width,
            (self.line_height
                * (commands_with_keymap.len() + commands_without_keymap.len())
                    as f64)
                .max(bc.max().height),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();
        let rect = ctx.region().bounding_box();
        let start = (rect.y0 / self.line_height).floor() as usize;
        let end = (rect.y1 / self.line_height).ceil() as usize;
        let keypress_width = 200.0;

        let commands_with_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_with_keymap
        } else {
            &data.keypress.filtered_commands_with_keymap
        };

        let commands_without_keymap = if data.keypress.filter_pattern.is_empty() {
            &data.keypress.commands_without_keymap
        } else {
            &data.keypress.filtered_commands_without_keymap
        };

        let commands_with_keymap_len = commands_with_keymap.len();
        for i in start..end + 1 {
            if i % 2 == 0 {
                ctx.fill(
                    Size::new(rect.width(), self.line_height)
                        .to_rect()
                        .with_origin(Point::new(
                            rect.x0,
                            self.line_height * i as f64,
                        )),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            if i < commands_with_keymap_len {
                let keymap = &commands_with_keymap[i];
                if let Some(cmd) = data.keypress.commands.get(&keymap.command) {
                    ctx.with_save(|ctx| {
                        ctx.clip(Rect::new(
                            0.0,
                            i as f64 * self.line_height,
                            size.width / 2.0 - keypress_width,
                            (i + 1) as f64 * self.line_height,
                        ));
                        let text_layout = ctx
                            .text()
                            .new_text_layout(match cmd.kind.desc() {
                                Some(desc) => desc.to_string(),
                                None => {
                                    let mut formatted =
                                        cmd.kind.str().replace('_', " ");
                                    format!(
                                        "{}{formatted}",
                                        formatted.remove(0).to_uppercase()
                                    )
                                }
                            })
                            .font(
                                data.config.ui.font_family(),
                                data.config.ui.font_size() as f64,
                            )
                            .text_color(
                                data.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        ctx.draw_text(
                            &text_layout,
                            Point::new(
                                10.0,
                                i as f64 * self.line_height
                                    + text_layout.y_offset(self.line_height),
                            ),
                        );
                    });
                }

                let origin = Point::new(
                    size.width / 2.0 - keypress_width + 10.0,
                    i as f64 * self.line_height + self.line_height / 2.0,
                );
                keymap.paint(ctx, origin, Alignment::Left, &data.config);

                if let Some(condition) = keymap.when.as_ref() {
                    let text_layout = ctx
                        .text()
                        .new_text_layout(condition.to_string())
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
                            size.width / 2.0
                                + 10.0
                                + if data.config.core.modal {
                                    keypress_width
                                } else {
                                    0.0
                                },
                            i as f64 * self.line_height
                                + text_layout.y_offset(self.line_height),
                        ),
                    )
                }

                if data.config.core.modal && !keymap.modes.is_empty() {
                    let mut origin = Point::new(
                        size.width / 2.0 + 10.0,
                        i as f64 * self.line_height + self.line_height / 2.0,
                    );
                    let bits = [
                        (Modes::INSERT, "Insert"),
                        (Modes::NORMAL, "Normal"),
                        (Modes::VISUAL, "Visual"),
                        (Modes::TERMINAL, "Terminal"),
                    ];
                    for (bit, mode) in bits {
                        if keymap.modes.contains(bit) {
                            let (rect, text_layout, text_layout_pos) =
                                paint_key(ctx, mode, origin, &data.config);
                            ctx.draw_text(&text_layout, text_layout_pos);
                            ctx.stroke(
                                rect,
                                data.config
                                    .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                                1.0,
                            );
                            origin += (rect.width() + 5.0, 0.0);
                        }
                    }
                }
            } else {
                let j = i - commands_with_keymap_len;
                if let Some(command) = commands_without_keymap.get(j) {
                    ctx.with_save(|ctx| {
                        ctx.clip(Rect::new(
                            0.0,
                            i as f64 * self.line_height,
                            size.width / 2.0 - keypress_width,
                            (i + 1) as f64 * self.line_height,
                        ));
                        let text_layout = ctx
                            .text()
                            .new_text_layout(match command.kind.desc() {
                                Some(desc) => desc.to_string(),
                                None => {
                                    let mut formatted =
                                        command.kind.str().replace('_', " ");
                                    format!(
                                        "{}{formatted}",
                                        formatted.remove(0).to_uppercase()
                                    )
                                }
                            })
                            .font(
                                data.config.ui.font_family(),
                                data.config.ui.font_size() as f64,
                            )
                            .text_color(
                                data.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        let text_size = text_layout.size();
                        ctx.draw_text(
                            &text_layout,
                            Point::new(
                                10.0,
                                i as f64 * self.line_height
                                    + (self.line_height - text_size.height) / 2.0,
                            ),
                        );
                    });
                }
            }
        }

        let x = size.width / 2.0 - keypress_width;
        ctx.stroke(
            Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        let x = size.width / 2.0;
        ctx.stroke(
            Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        if data.config.core.modal {
            let x = size.width / 2.0 + keypress_width;
            ctx.stroke(
                Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }

        if let Some((keymap, keys)) = self.active_keymap.as_ref() {
            let paint_rect = rect;
            let size = paint_rect.size();
            let active_width = 450.0;
            let active_height = 150.0;
            let active_rect = Size::new(active_width, active_height)
                .to_rect()
                .with_origin(Point::new(
                    size.width / 2.0 - active_width / 2.0,
                    size.height / 2.0 - active_height / 2.0 + paint_rect.y0,
                ));
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.blurred_rect(
                    active_rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            }
            ctx.fill(
                active_rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );

            let input_height = 35.0;
            let rect = Size::new(0.0, 0.0)
                .to_rect()
                .with_origin(rect.center())
                .inflate(active_width / 2.0 - 10.0, input_height / 2.0);
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
            ctx.stroke(
                rect,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            KeyMap {
                key: keys.clone(),
                modes: keymap.modes,
                when: keymap.when.clone(),
                command: keymap.command.clone(),
            }
            .paint(ctx, rect.center(), Alignment::Center, &data.config);

            if let Some(cmd) = data.keypress.commands.get(&keymap.command) {
                let text = ctx
                    .text()
                    .new_text_layout(
                        cmd.kind.desc().unwrap_or_else(|| cmd.kind.str()),
                    )
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
                let text_size = text.size();
                let rect_center = active_rect.center();
                let text_center = Point::new(
                    rect_center.x,
                    active_rect.y0
                        + (active_rect.height() / 2.0 - input_height / 2.0) / 2.0,
                );
                ctx.draw_text(
                    &text,
                    Point::new(
                        text_center.x - text_size.width / 2.0,
                        text_center.y - text_size.height / 2.0,
                    ),
                );
            }

            let center = active_rect.center()
                + (
                    active_width / 4.0,
                    input_height / 2.0
                        + (active_height / 2.0 - input_height / 2.0) / 2.0,
                );
            let text = ctx
                .text()
                .new_text_layout("Save")
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
            let text_size = text.size();
            ctx.draw_text(
                &text,
                Point::new(
                    center.x - text_size.width / 2.0,
                    center.y - text_size.height / 2.0,
                ),
            );

            self.keymap_confirm = Size::new(0.0, 0.0)
                .to_rect()
                .with_origin(center)
                .inflate(50.0, 15.0);
            ctx.stroke(
                self.keymap_confirm,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );

            let center = active_rect.center()
                + (
                    -active_width / 4.0,
                    input_height / 2.0
                        + (active_height / 2.0 - input_height / 2.0) / 2.0,
                );
            let text = ctx
                .text()
                .new_text_layout("Cancel")
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
            let text_size = text.size();
            ctx.draw_text(
                &text,
                Point::new(
                    center.x - text_size.width / 2.0,
                    center.y - text_size.height / 2.0,
                ),
            );
            self.keymap_cancel = Size::new(0.0, 0.0)
                .to_rect()
                .with_origin(center)
                .inflate(50.0, 15.0);
            ctx.stroke(
                self.keymap_cancel,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }
}

struct LapceKeymapHeader {}

impl LapceKeymapHeader {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LapceKeymapHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for LapceKeymapHeader {
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
        Size::new(bc.max().width, 40.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();
        let keypress_width = 200.0;

        let text_layout = ctx
            .text()
            .new_text_layout("Command")
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        ctx.draw_text(
            &text_layout,
            Point::new(10.0, (size.height - text_size.height) / 2.0),
        );

        let text_layout = ctx
            .text()
            .new_text_layout("Key Binding")
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        ctx.draw_text(
            &text_layout,
            Point::new(
                size.width / 2.0 - keypress_width + 10.0,
                (size.height - text_size.height) / 2.0,
            ),
        );

        let text_layout = ctx
            .text()
            .new_text_layout("When")
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        ctx.draw_text(
            &text_layout,
            Point::new(
                size.width / 2.0
                    + 10.0
                    + if data.config.core.modal {
                        keypress_width
                    } else {
                        0.0
                    },
                (size.height - text_size.height) / 2.0,
            ),
        );

        if data.config.core.modal {
            let text_layout = ctx
                .text()
                .new_text_layout("Modes")
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            ctx.draw_text(
                &text_layout,
                Point::new(
                    size.width / 2.0 + 10.0,
                    (size.height - text_size.height) / 2.0,
                ),
            );
        }

        let x = size.width / 2.0 - keypress_width;
        ctx.stroke(
            Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        let x = size.width / 2.0;
        ctx.stroke(
            Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        if data.config.core.modal {
            let x = size.width / 2.0 + keypress_width;
            ctx.stroke(
                Line::new(Point::new(x, 0.0), Point::new(x, size.height)),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }
}
