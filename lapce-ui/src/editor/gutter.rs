use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetId,
};
use lapce_data::{
    buffer::DiffLines,
    config::LapceTheme,
    data::LapceTabData,
    editor::{LapceEditorBufferData, Syntax},
};
use crate::svg::get_svg;

pub struct LapceEditorGutter {
    view_id: WidgetId,
    width: f64,
}

impl LapceEditorGutter {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            width: 0.0,
        }
    }
}

impl Widget<LapceTabData> for LapceEditorGutter {
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
        // let old_last_line = old_data.buffer.last_line() + 1;
        // let last_line = data.buffer.last_line() + 1;
        // if old_last_line.to_string().len() != last_line.to_string().len() {
        //     ctx.request_layout();
        //     return;
        // }

        // if (*old_data.main_split.active == self.view_id
        //     && *data.main_split.active != self.view_id)
        //     || (*old_data.main_split.active != self.view_id
        //         && *data.main_split.active == self.view_id)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.editor.cursor.current_line(&old_data.buffer)
        //     != data.editor.cursor.current_line(&data.buffer)
        // {
        //     ctx.request_paint();
        // }

        // if old_data.current_code_actions().is_some()
        //     != data.current_code_actions().is_some()
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let data = data.editor_view_content(self.view_id);
        let last_line = data.buffer.last_line() + 1;
        let char_width = data.config.editor_text_width(ctx.text(), "W");
        self.width = (char_width * last_line.to_string().len() as f64).ceil();
        let mut width = self.width + 16.0 + char_width * 2.0;
        if data.editor.compare.is_some() {
            width += self.width + char_width * 2.0;
        }
        Size::new(width.ceil(), bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let data = data.editor_view_content(self.view_id);
        self.paint_gutter(&data, ctx);
    }
}

impl LapceEditorGutter {
    fn paint_gutter_inline_diff(
        &self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        compare: &str,
    ) {
        if data.buffer.history_changes.get(compare).is_none() {
            return;
        }
        let self_size = ctx.size();
        let rect = self_size.to_rect();
        let changes = data.buffer.history_changes.get(compare).unwrap();
        let line_height = data.config.editor.line_height as f64;
        let scroll_offset = data.editor.scroll_offset;
        let start_line = (scroll_offset.y / line_height).floor() as usize;
        let end_line =
            (scroll_offset.y + rect.height() / line_height).ceil() as usize;
        let current_line = data.editor.cursor.current_line(&data.buffer);
        let last_line = data.buffer.last_line();
        let width = data.config.editor_text_width(ctx.text(), "W");

        let mut line = 0;
        for change in changes.iter() {
            match change {
                DiffLines::Left(r) => {
                    let len = r.len();
                    line += len;

                    if line < start_line {
                        continue;
                    }
                    ctx.fill(
                        Size::new(self_size.width, line_height * len as f64)
                            .to_rect()
                            .with_origin(Point::new(
                                0.0,
                                line_height * (line - len) as f64 - scroll_offset.y,
                            )),
                        data.config
                            .get_color_unchecked(LapceTheme::SOURCE_CONTROL_REMOVED),
                    );
                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let actual_line = l - (line - len) + r.start;

                        let content = actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(
                                content.to_string()
                                    + &vec![
                                        " ";
                                        (last_line + 1).to_string().len() + 2
                                    ]
                                    .join("")
                                    + " -",
                            )
                            .font(
                                data.config.editor.font_family(),
                                data.config.editor.font_size as f64,
                            )
                            .text_color(
                                data.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
                DiffLines::Both(left, r) => {
                    let len = r.len();
                    line += len;
                    if line < start_line {
                        continue;
                    }

                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let left_actual_line = l - (line - len) + left.start;
                        let right_actual_line = l - (line - len) + r.start;

                        let left_content = left_actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - left_content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(left_content.to_string())
                            .font(
                                data.config.editor.font_family(),
                                data.config.editor.font_size as f64,
                            )
                            .text_color(
                                data.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone(),
                            )
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        let right_content = right_actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - right_content.to_string().len())
                            as f64
                            * width
                            + self.width
                            + 2.0 * width;
                        let pos = Point::new(x, y);
                        let text_layout = ctx
                            .text()
                            .new_text_layout(right_content.to_string())
                            .font(
                                data.config.editor.font_family(),
                                data.config.editor.font_size as f64,
                            )
                            .text_color(if right_actual_line == current_line {
                                data.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone()
                            } else {
                                data.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone()
                            })
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
                DiffLines::Skip(_l, _r) => {
                    let rect = Size::new(self_size.width, line_height)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            line_height * line as f64 - scroll_offset.y,
                        ));
                    ctx.fill(
                        rect,
                        data.config
                            .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                    );
                    ctx.stroke(
                        rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        1.0,
                    );
                    line += 1;
                }
                DiffLines::Right(r) => {
                    let len = r.len();
                    line += len;
                    if line < start_line {
                        continue;
                    }

                    ctx.fill(
                        Size::new(self_size.width, line_height * len as f64)
                            .to_rect()
                            .with_origin(Point::new(
                                0.0,
                                line_height * (line - len) as f64 - scroll_offset.y,
                            )),
                        data.config
                            .get_color_unchecked(LapceTheme::SOURCE_CONTROL_ADDED),
                    );

                    for l in line - len..line {
                        if l < start_line {
                            continue;
                        }
                        let actual_line = l - (line - len) + r.start;

                        let content = actual_line + 1;
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width
                            + self.width
                            + 2.0 * width;
                        let y = line_height * l as f64 + 5.0 - scroll_offset.y;
                        let pos = Point::new(x, y);

                        let text_layout = ctx
                            .text()
                            .new_text_layout(content.to_string() + " +")
                            .font(
                                data.config.editor.font_family(),
                                data.config.editor.font_size as f64,
                            )
                            .text_color(if actual_line == current_line {
                                data.config
                                    .get_color_unchecked(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )
                                    .clone()
                            } else {
                                data.config
                                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                                    .clone()
                            })
                            .build()
                            .unwrap();
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
            }
        }
    }

    fn paint_gutter_code_lens(
        &self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
    ) {
        let rect = ctx.size().to_rect();
        let scroll_offset = data.editor.scroll_offset;
        let empty_lens = Syntax::lens_from_normal_lines(
            data.buffer.len(),
            data.config.editor.line_height,
            data.config.editor.code_lens_font_size,
            &[],
        );
        let lens = if let Some(syntax) = data.buffer.syntax.as_ref() {
            &syntax.lens
        } else {
            &empty_lens
        };

        let cursor_line = data
            .buffer
            .line_of_offset(data.editor.cursor.offset().min(data.buffer.len()));
        let last_line = data.buffer.line_of_offset(data.buffer.len());
        let start_line = lens
            .line_of_height(scroll_offset.y.floor() as usize)
            .min(last_line);
        let end_line = lens
            .line_of_height(
                (scroll_offset.y + rect.height()).ceil() as usize
                    + data.config.editor.line_height,
            )
            .min(last_line);
        let char_width = data
            .config
            .char_width(ctx.text(), data.config.editor.font_size as f64);
        let max_line_width = (last_line + 1).to_string().len() as f64 * char_width;

        let mut y = lens.height_of_line(start_line) as f64;
        for (line, line_height) in lens.iter_chunks(start_line..end_line + 1) {
            let content = if *data.main_split.active != Some(self.view_id)
                || data.editor.cursor.is_insert()
                || line == cursor_line
            {
                line + 1
            } else if line > cursor_line {
                line - cursor_line
            } else {
                cursor_line - line
            };
            let content = content.to_string();
            let is_small = line_height < data.config.editor.line_height;
            let text_layout = ctx
                .text()
                .new_text_layout(content.clone())
                .font(
                    data.config.editor.font_family(),
                    if is_small {
                        data.config.editor.code_lens_font_size as f64
                    } else {
                        data.config.editor.font_size as f64
                    },
                )
                .text_color(if line == cursor_line {
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone()
                } else {
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone()
                })
                .build()
                .unwrap();
            let x = max_line_width - text_layout.size().width;
            let pos = Point::new(
                x,
                y - scroll_offset.y
                    + if is_small {
                        0.0
                    } else {
                        (line_height as f64 - text_layout.size().height) / 2.0
                    },
            );
            ctx.draw_text(&text_layout, pos);

            y += line_height as f64;
        }
    }

    fn paint_code_actions_hint(
        &self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
    ) {
        if let Some(actions) = data.current_code_actions() {
            if !actions.is_empty() {
                let line_height = data.config.editor.line_height as f64;
                let offset = data.editor.cursor.offset();
                let (line, _) = data
                    .buffer
                    .offset_to_line_col(offset, data.config.editor.tab_width);
                let svg = get_svg("lightbulb.svg").unwrap();
                let width = 16.0;
                let height = 16.0;
                let char_width = data.config.editor_text_width(ctx.text(), "W");
                let rect =
                    Size::new(width, height).to_rect().with_origin(Point::new(
                        self.width + char_width + 3.0,
                        (line_height - height) / 2.0 + line_height * line as f64
                            - data.editor.scroll_offset.y,
                    ));
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)),
                );
            }
        }
    }

    fn paint_gutter(&self, data: &LapceEditorBufferData, ctx: &mut PaintCtx) {
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            let clip_rect = rect;
            ctx.clip(clip_rect);
            if let Some(compare) = data.editor.compare.as_ref() {
                self.paint_gutter_inline_diff(data, ctx, compare);
                return;
            }
            if data.editor.code_lens {
                self.paint_gutter_code_lens(data, ctx);
                return;
            }
            let line_height = data.config.editor.line_height as f64;
            let scroll_offset = data.editor.scroll_offset;
            let start_line = (scroll_offset.y / line_height).floor() as usize;
            let num_lines = (ctx.size().height / line_height).floor() as usize;
            let last_line = data.buffer.last_line();
            let current_line = data.editor.cursor.current_line(&data.buffer);
            let char_width = data.config.editor_text_width(ctx.text(), "W");

            let line_label_length =
                (last_line + 1).to_string().len() as f64 * char_width;
            let last_displayed_line = (start_line + num_lines + 1).min(last_line);

            let sequential_line_numbers = *data.main_split.active
                != Some(data.view_id)
                || data.editor.cursor.is_insert();

            let font_family = data.config.editor.font_family();

            for line in start_line..last_displayed_line {
                let line_no = if sequential_line_numbers || line == current_line {
                    line + 1
                } else {
                    // TODO: after Rust 1.60, this can be replaced with `line.abs_diff(current_line)`
                    if line > current_line {
                        line - current_line
                    } else {
                        current_line - line
                    }
                };

                let content = line_no.to_string();

                let text_layout = ctx
                    .text()
                    .new_text_layout(content)
                    .font(font_family.clone(), data.config.editor.font_size as f64)
                    .text_color(
                        data.config
                            .get_color_unchecked(if line == current_line {
                                LapceTheme::EDITOR_FOREGROUND
                            } else {
                                LapceTheme::EDITOR_DIM
                            })
                            .clone(),
                    )
                    .build()
                    .unwrap();

                // Horizontally right aligned
                let x = line_label_length as f64 - text_layout.size().width;

                // Vertically centered
                let y = line_height * line as f64 - scroll_offset.y
                    + (line_height - text_layout.size().height) / 2.0;

                ctx.draw_text(&text_layout, Point::new(x, y));
            }

            if let Some(changes) = data.buffer.history_changes.get("head") {
                let end_line =
                    (scroll_offset.y + rect.height() / line_height).ceil() as usize;

                let mut line = 0;
                let mut last_change = None;
                for change in changes.iter() {
                    let len = match change {
                        DiffLines::Left(_range) => 0,
                        DiffLines::Skip(_left, right) => right.len(),
                        DiffLines::Both(_left, right) => right.len(),
                        DiffLines::Right(range) => range.len(),
                    };
                    line += len;
                    if line < start_line {
                        last_change = Some(change);
                        continue;
                    }

                    let mut modified = false;
                    let color = match change {
                        DiffLines::Left(_range) => {
                            Some(data.config.get_color_unchecked(
                                LapceTheme::SOURCE_CONTROL_REMOVED,
                            ))
                        }
                        DiffLines::Right(_range) => {
                            if let Some(DiffLines::Left(_)) = last_change.as_ref() {
                                modified = true;
                            }
                            if modified {
                                Some(data.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_MODIFIED,
                                ))
                            } else {
                                Some(data.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_ADDED,
                                ))
                            }
                        }
                        _ => None,
                    };

                    if let Some(color) = color.cloned() {
                        let removed_height = 10.0;
                        let x = self.width + char_width;
                        let mut y =
                            (line - len) as f64 * line_height - scroll_offset.y;
                        if len == 0 {
                            y -= removed_height / 2.0;
                        }
                        if modified {
                            let rect = Rect::from_origin_size(
                                Point::new(x, y - removed_height / 2.0),
                                Size::new(3.0, removed_height),
                            );
                            ctx.fill(
                                rect,
                                data.config.get_color_unchecked(
                                    LapceTheme::EDITOR_BACKGROUND,
                                ),
                            );
                        }
                        let rect = Rect::from_origin_size(
                            Point::new(x, y),
                            Size::new(
                                3.0,
                                if len == 0 {
                                    removed_height
                                } else {
                                    line_height * len as f64
                                },
                            ),
                        );
                        ctx.fill(rect, &color.with_alpha(0.8));
                    }

                    if line > end_line {
                        break;
                    }
                    last_change = Some(change);
                }
            }

            if *data.main_split.active == Some(self.view_id) {
                self.paint_code_actions_hint(data, ctx);
            }
        });
    }
}
