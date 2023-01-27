use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{CommandKind, LapceCommand, LAPCE_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
};

use crate::{editor::view::LapceEditorView, tab::LapceIcon};

// Local search widget
pub struct FindBox {
    parent_view_id: WidgetId,
    input_width: f64,
    result_width: f64,
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
}

impl FindBox {
    pub fn new(
        view_id: WidgetId,
        editor_id: WidgetId,
        parent_view_id: WidgetId,
    ) -> Self {
        let input = LapceEditorView::new(view_id, editor_id, None)
            .hide_header()
            .hide_gutter()
            .padding((10.0, 5.0));
        let icons = vec![
            LapceIcon {
                icon: LapceIcons::SEARCH_BACKWARD,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::SearchBackward),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: LapceIcons::SEARCH_FORWARD,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::SearchForward),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: LapceIcons::SEARCH_CASE_SENSITIVE,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::ToggleCaseSensitive),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: LapceIcons::CLOSE,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::ClearSearch),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
        ];
        Self {
            parent_view_id,
            input_width: 200.0,
            result_width: 75.0,
            input: WidgetPod::new(input.boxed()),
            icons,
            mouse_pos: Point::ZERO,
        }
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

impl Widget<LapceTabData> for FindBox {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.input.event(ctx, event, data, env);
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                self.mouse_pos = mouse_event.pos;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let input_bc =
            BoxConstraints::tight(Size::new(self.input_width, bc.max().height));
        let mut input_size = self.input.layout(ctx, &input_bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);
        let icons_len = self.icons.len() as f64;
        let height = input_size.height;
        let mut width = input_size.width + self.result_width + height * icons_len;

        if width - 20.0 > bc.max().width {
            let input_bc = BoxConstraints::tight(Size::new(
                bc.max().width - height * icons_len - 20.0 - self.result_width,
                bc.max().height,
            ));
            input_size = self.input.layout(ctx, &input_bc, data, env);
            self.input.set_origin(ctx, data, env, Point::ZERO);
            width = input_size.width + self.result_width + height * icons_len;
        }

        for (i, icon) in self.icons.iter_mut().enumerate() {
            icon.rect = Size::new(height, height)
                .to_rect()
                .with_origin(Point::new(
                    input_size.width + self.result_width + i as f64 * height,
                    0.0,
                ))
                .inflate(-5.0, -5.0);
        }

        Size::new(width, height)
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.update(ctx, data, env);
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !data.find.visual {
            return;
        }

        let buffer = data.editor_view_content(self.parent_view_id);

        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            ctx.clip(rect.inset((100.0, 0.0, 100.0, 100.0)));
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
                    rect.inflate(0.5, 0.5),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
        });
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );
        self.input.paint(ctx, data, env);

        let mut index = None;
        let cursor_offset = buffer.editor.cursor.offset();

        for i in 0..buffer.doc.find.borrow().occurrences().regions().len() {
            let region = buffer.doc.find.borrow().occurrences().regions()[i];
            if region.min() <= cursor_offset && cursor_offset <= region.max() {
                index = Some(i);
            }
        }

        let text_layout = ctx
            .text()
            .new_text_layout(if !buffer.doc.find.borrow().occurrences().is_empty() {
                match index {
                    Some(index) => format!(
                        "{}/{}",
                        index + 1,
                        buffer.doc.find.borrow().occurrences().len()
                    ),
                    None => format!(
                        "{} results",
                        buffer.doc.find.borrow().occurrences().len()
                    ),
                }
            } else {
                "No results".to_string()
            })
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .max_width(self.result_width)
            .build()
            .unwrap();

        let input_size = self.input.layout_rect().size();
        ctx.draw_text(
            &text_layout,
            Point::new(input_size.width, text_layout.y_offset(input_size.height)),
        );

        let case_sensitive = data
            .main_split
            .active_editor()
            .map(|editor| {
                let editor_data = data.editor_view_content(editor.view_id);
                editor_data.find.case_sensitive()
            })
            .unwrap_or_default();

        for icon in self.icons.iter() {
            if icon.icon == LapceIcons::SEARCH_CASE_SENSITIVE && case_sensitive {
                ctx.fill(
                    icon.rect,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_TAB_ACTIVE_UNDERLINE),
                );
            } else if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    icon.rect,
                    &data.config.get_hover_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                    ),
                );
            }

            let svg = data.config.ui_svg(icon.icon);
            ctx.draw_svg(
                &svg,
                icon.rect.inflate(-7.0, -7.0),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }
    }
}
