use std::path::Path;

use druid::{
    piet::{PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetId,
};
use lapce_data::{
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
    document::BufferContent,
};

pub struct LapceEditorBreadCrumb {
    pub view_id: WidgetId,
    text_layouts: Vec<(Point, PietTextLayout)>,
    svgs: Vec<(Rect, Svg)>,
}

impl LapceEditorBreadCrumb {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            text_layouts: Vec::new(),
            svgs: Vec::new(),
        }
    }
}

impl Widget<LapceTabData> for LapceEditorBreadCrumb {
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let editor_buffer = data.editor_view_content(self.view_id);
        self.text_layouts.clear();
        self.svgs.clear();

        let line_height = data.config.editor.line_height() as f64;

        let font_size = data.config.ui.font_size() as f64;

        let mut x = font_size;
        if let BufferContent::File(path) = &editor_buffer.editor.content {
            let mut path = path.to_path_buf();
            if let Some(workspace_path) = data.workspace.path.as_ref() {
                path = path
                    .strip_prefix(workspace_path)
                    .unwrap_or(&path)
                    .to_path_buf();
            }

            if let Some(path) = path.parent() {
                for p in path.ancestors().collect::<Vec<&Path>>().iter().rev() {
                    if let Some(file_name) = p.file_name().and_then(|s| s.to_str()) {
                        if !file_name.is_empty() {
                            let text_layout = ctx
                                .text()
                                .new_text_layout(file_name.to_string())
                                .font(data.config.ui.font_family(), font_size)
                                .text_color(
                                    data.config
                                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                        .clone(),
                                )
                                .build()
                                .unwrap();
                            let size = text_layout.size();
                            self.text_layouts.push((
                                Point::new(x, text_layout.y_offset(line_height)),
                                text_layout,
                            ));

                            x += size.width;
                            self.svgs.push((
                                Rect::ZERO
                                    .with_origin(Point::new(
                                        x + font_size / 2.0,
                                        line_height / 2.0,
                                    ))
                                    .inflate(font_size / 2.0, font_size / 2.0),
                                data.config.ui_svg(LapceIcons::BREADCRUMB_SEPARATOR),
                            ));
                            x += font_size;
                        }
                    }
                }
            }
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                let text_layout = ctx
                    .text()
                    .new_text_layout(file_name.to_string())
                    .font(data.config.ui.font_family(), font_size)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let size = text_layout.size();
                self.text_layouts.push((
                    Point::new(x, text_layout.y_offset(line_height)),
                    text_layout,
                ));

                x += size.width;
                x += font_size;
            }
        }

        Size::new(bc.max().width.max(x), line_height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        for (point, text_layout) in self.text_layouts.iter() {
            ctx.draw_text(text_layout, *point);
        }

        for (rect, svg) in self.svgs.iter() {
            ctx.draw_svg(
                svg,
                *rect,
                Some(data.config.get_color_unchecked(LapceTheme::EDITOR_DIM)),
            );
        }
    }
}
