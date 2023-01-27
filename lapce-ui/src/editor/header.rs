use std::{iter::Iterator, path::PathBuf};

use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetId,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{CommandKind, LapceCommand, LAPCE_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::{LapceTabData, LapceWorkspace},
    document::BufferContent,
    editor::LapceEditorBufferData,
};

use crate::tab::LapceIcon;

pub struct LapceEditorHeader {
    view_id: WidgetId,
    pub display: bool,
    cross_rect: Rect,
    mouse_pos: Point,
    pub view_is_hot: bool,
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
            icon_size: 24.0,
            svg_padding: 4.0,
            icons: Vec::new(),
        }
    }

    pub fn get_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let _data = data.editor_view_content(self.view_id);
        let gap = (data.config.ui.header_height() as f64 - self.icon_size) / 2.0;

        let mut icons = Vec::new();
        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::CLOSE,
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::SplitClose),
                    data: None,
                },
                Target::Widget(self.view_id),
            ),
        };
        icons.push(icon);

        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::SPLIT_HORIZONTAL,
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::SplitVertical),
                    data: None,
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
        }
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
        if let BufferContent::File(_) | BufferContent::Scratch(..) =
            data.doc.content()
        {
            let mut path = match data.doc.content() {
                BufferContent::File(path) => path.to_path_buf(),
                BufferContent::Scratch(_, scratch_doc_name) => {
                    scratch_doc_name.into()
                }
                _ => PathBuf::from(""),
            };

            ctx.with_save(|ctx| {
                ctx.clip(clip_rect);
                let (svg, svg_color) = data.config.file_svg(&path);

                let font_size = data.config.ui.font_size() as f64;

                let svg_size = data.config.ui.icon_size() as f64;
                let rect =
                    Size::new(svg_size, svg_size)
                        .to_rect()
                        .with_origin(Point::new(
                            (size.height - svg_size) / 2.0,
                            (size.height - svg_size) / 2.0,
                        ));
                ctx.draw_svg(&svg, rect, svg_color);

                let mut file_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if !data.doc.buffer().is_pristine() {
                    file_name = "*".to_string() + &file_name;
                }
                if let Some(_compare) = data.editor.compare.as_ref() {
                    file_name += " (Working tree)";
                }
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
                    file_name = format!("{file_name} {folder}");
                }
                let total_len = file_name.len();
                let mut text_layout = ctx
                    .text()
                    .new_text_layout(file_name)
                    .font(data.config.ui.font_family(), font_size)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    );
                if !folder.is_empty() {
                    text_layout = text_layout.range_attribute(
                        total_len - folder.len()..total_len,
                        TextAttribute::TextColor(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                .clone(),
                        ),
                    );
                }
                let text_layout = text_layout.build().unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(size.height, text_layout.y_offset(size.height)),
                );
            });
        }

        if self.view_is_hot {
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                {
                    let svg = data.config.ui_svg(icon.icon);
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
                } else {
                    ctx.clear_cursor();
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
        if self.display && self.view_id == data.palette.preview_editor {
            let size =
                Size::new(bc.max().width, data.config.ui.header_height() as f64);
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
