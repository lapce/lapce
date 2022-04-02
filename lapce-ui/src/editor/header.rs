use std::iter::Iterator;

use druid::{
    piet::{Text, TextLayout as TextLayoutTrait, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetId,
};
use lapce_data::{
    buffer::BufferContent,
    command::{CommandTarget, LapceCommand, LapceCommandNew, LAPCE_NEW_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    editor::LapceEditorBufferData,
    state::LapceWorkspace,
    
};

use crate::{
    svg::{file_svg_new, get_svg},
    tab::LapceIcon
};

pub struct LapceEditorHeader {
    view_id: WidgetId,
    pub display: bool,
    cross_rect: Rect,
    mouse_pos: Point,
    pub view_is_hot: bool,
    height: f64,
    icon_size: f64,
    icons: Vec<LapceIcon>,
    svg_padding: f64,
}

impl LapceEditorHeader {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            display: true,
            view_id,
            cross_rect: Rect::ZERO,
            mouse_pos: Point::ZERO,
            view_is_hot: false,
            height: 30.0,
            icon_size: 24.0,
            svg_padding: 4.0,
            icons: Vec::new(),
        }
    }

    pub fn get_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let _data = data.editor_view_content(self.view_id);
        let gap = (self.height - self.icon_size) / 2.0;

        let mut icons = Vec::new();
        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "close.svg".to_string(),
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_NEW_COMMAND,
                LapceCommandNew {
                    cmd: LapceCommand::SplitClose.to_string(),
                    data: None,
                    palette_desc: None,
                    target: CommandTarget::Focus,
                },
                Target::Widget(self.view_id),
            ),
        };
        icons.push(icon);

        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: "split-horizontal.svg".to_string(),
            rect: Size::new(self.icon_size, self.icon_size)
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
                Target::Widget(self.view_id),
            ),
        };
        icons.push(icon);

        icons
    }

    pub fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    pub fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    pub fn paint_buffer(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceEditorBufferData,
        workspace: &LapceWorkspace,
    ) {
        let shadow_width = 5.0;
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        let mut clip_rect = ctx.size().to_rect();
        if self.view_is_hot {
            if let Some(icon) = self.icons.iter().rev().next().as_ref() {
                clip_rect.x1 = icon.rect.x0;
            }
        }
        if let BufferContent::File(path) = data.buffer.content() {
            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                let mut path = path.clone();
                let svg = file_svg_new(&path);

                let width = 13.0;
                let height = 13.0;
                let rect = Size::new(width, height).to_rect().with_origin(
                    Point::new((30.0 - width) / 2.0, (30.0 - height) / 2.0),
                );
                ctx.draw_svg(&svg, rect, None);

                let mut file_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if data.buffer.dirty() {
                    file_name = "*".to_string() + &file_name;
                }
                if let Some(_compare) = data.editor.compare.as_ref() {
                    file_name += " (Working tree)";
                }
                let text_layout = ctx
                    .text()
                    .new_text_layout(file_name)
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(30.0, 7.0));

                if let Some(workspace_path) = workspace.path.as_ref() {
                    path = path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&path)
                        .to_path_buf();
                }
                let folder = path
                    .parent()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if !folder.is_empty() {
                    let x = text_layout.size().width;

                    let text_layout = ctx
                        .text()
                        .new_text_layout(folder)
                        .font(FontFamily::SYSTEM_UI, 13.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    ctx.draw_text(&text_layout, Point::new(30.0 + x + 5.0, 7.0));
                }
            });
        }

        if self.view_is_hot {
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
                        icon.rect.inflate(-self.svg_padding, -self.svg_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        ),
                    );
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceEditorHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
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
        // ctx.set_paint_insets((0.0, 0.0, 0.0, 10.0));
        if self.display
            && (!data.config.editor.show_tab
                || self.view_id == data.palette.preview_editor)
        {
            let size = Size::new(bc.max().width, self.height);
            self.icons = self.get_icons(size, data);
            let cross_size = 20.0;
            let padding = (size.height - cross_size) / 2.0;
            let origin = Point::new(size.width - padding - cross_size, padding);
            self.cross_rect = Size::new(cross_size, cross_size)
                .to_rect()
                .with_origin(origin);
            size
        } else {
            Size::new(bc.max().width, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if !self.display {
            return;
        }
        self.paint_buffer(
            ctx,
            &data.editor_view_content(self.view_id),
            &data.workspace,
        );
    }
}
