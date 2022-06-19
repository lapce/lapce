use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{CommandKind, LapceCommand, LAPCE_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
};

use crate::{editor::view::LapceEditorView, svg::get_svg, tab::LapceIcon};

pub struct FindBox {
    input_width: f64,
    result_width: f64,
    result_pos: Point,
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
                icon: "arrow-up.svg",
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
                icon: "arrow-down.svg",
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
                icon: "close.svg",
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
            input_width: 200.0,
            result_width: 75.0,
            result_pos: Point::ZERO,
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
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
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
        let height = input_size.height;
        let mut width = input_size.width + self.result_width + height * 3.0;

        if width - 20.0 > bc.max().width {
            let input_bc = BoxConstraints::tight(Size::new(
                bc.max().width - height * 3.0 - 20.0 - self.result_width,
                bc.max().height,
            ));
            input_size = self.input.layout(ctx, &input_bc, data, env);
            width = input_size.width + self.result_width + height * 3.0;
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

        self.result_pos = Point::new(
            input_size.width,
            (height - data.config.ui.font_size() as f64) / 2.0,
        );

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

        let buffer = match data
            .main_split
            .editor_tabs
            .get(&data.main_split.active_tab.unwrap())
            .unwrap()
            .active_child()
        {
            lapce_data::data::EditorTabChild::Editor(view_id, _, _) => {
                data.editor_view_content(*view_id)
            }
            lapce_data::data::EditorTabChild::Settings(view_id, _) => {
                data.editor_view_content(*view_id)
            }
        };

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

        ctx.draw_text(&text_layout, self.result_pos);

        for icon in self.icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    &icon.rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }

            let svg = get_svg(icon.icon).unwrap();
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
