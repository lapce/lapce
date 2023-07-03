use crate::tab::LapceIcon;
use druid::{
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetId,
};
use lapce_core::{buffer::diff::DiffLines, command::FocusCommand};
use lapce_data::{
    command::{CommandKind, LapceCommand, LAPCE_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::EditorView,
    data::LapceTabData,
};

// Diff tool box
pub struct DiffBox {
    parent_view_id: WidgetId,
    result_width: f64,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
}

impl DiffBox {
    pub fn new(parent_view_id: WidgetId) -> Self {
        let icons = vec![
            LapceIcon {
                icon: LapceIcons::FILE,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::OpenSourceFile),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
            LapceIcon {
                icon: LapceIcons::SEARCH_BACKWARD,
                rect: Rect::ZERO,
                command: Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Focus(FocusCommand::PreviousDiff),
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
                        kind: CommandKind::Focus(FocusCommand::NextDiff),
                        data: None,
                    },
                    Target::Widget(parent_view_id),
                ),
            },
        ];
        Self {
            parent_view_id,
            result_width: 75.0,
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

impl Widget<LapceTabData> for DiffBox {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        let editor_data = data.editor_view_content(self.parent_view_id);
        match &editor_data.editor.view {
            EditorView::Diff(_) => {}
            _ => {
                return;
            }
        }
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
        _ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let icons_len = self.icons.len() as f64;
        let height = 35.0;
        let width = self.result_width + height * icons_len;

        for (i, icon) in self.icons.iter_mut().enumerate() {
            icon.rect = Size::new(height, height)
                .to_rect()
                .with_origin(Point::new(i as f64 * height, 0.0))
                .inflate(-5.0, -5.0);
        }

        Size::new(width, height)
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let editor_data = data.editor_view_content(self.parent_view_id);
        match &editor_data.editor.view {
            EditorView::Diff(_) => {}
            _ => {
                return;
            }
        }

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

        // find all diff blocks, ignore Both and Skip
        if let Some(history) = editor_data.doc.get_history("head") {
            for (i, change) in history.changes().iter().enumerate() {
                match change {
                    DiffLines::Left(_) => {
                        if let Some(next) = history.changes().get(i + 1) {
                            match next {
                                DiffLines::Right(_) => {}
                                DiffLines::Left(_) => {}
                                _ => {} // DiffLines::Both(_, r) => {
                                        //     diff_blocks.push(Range {
                                        //         start: r.start,
                                        //         end: r.start,
                                        //     });
                                        // }
                                        // DiffLines::Skip(_, r) => {
                                        //     diff_blocks.push(Range {
                                        //         start: r.start,
                                        //         end: r.start,
                                        //     });
                                        // }
                            }
                        }
                    }
                    DiffLines::Both(_) => {}
                    DiffLines::Right(_) => {}
                }
            }
        }

        for icon in self.icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
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
