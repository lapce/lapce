use druid::{
    piet::{PietText, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx,
    Widget, WidgetId,
};
use lapce_core::buffer::DiffLines;
use lapce_data::document::BufferContent;
use lapce_data::history::DocumentHistory;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::{EditorView, LapceTabData},
    editor::{LapceEditorBufferData, Syntax},
};

pub struct LapceEditorGutter {
    view_id: WidgetId,
    width: f64,
    mouse_down_pos: Point,
}

impl LapceEditorGutter {
    pub fn new(view_id: WidgetId) -> Self {
        Self {
            view_id,
            width: 0.0,
            mouse_down_pos: Point::ZERO,
        }
    }
}

impl Widget<LapceTabData> for LapceEditorGutter {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse_event) => {
                self.mouse_down_pos = mouse_event.pos;
            }
            Event::MouseUp(mouse_event) => {
                let data = data.editor_view_content(self.view_id);
                if let Some((_plugin_id, actions)) = data.current_code_actions() {
                    if !actions.is_empty() {
                        let rect = self.code_actions_rect(ctx.text(), &data);
                        if rect.contains(self.mouse_down_pos)
                            && rect.contains(mouse_event.pos)
                        {
                            let line_height =
                                data.config.editor.line_height() as f64;
                            let offset = data.editor.cursor.offset();
                            let (line, _) =
                                data.doc.buffer().offset_to_line_col(offset);
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ShowCodeActions(Some(
                                    ctx.to_window(Point::new(
                                        rect.x0,
                                        (line + 1) as f64 * line_height
                                            - data.editor.scroll_offset.y,
                                    )),
                                )),
                                Target::Widget(data.editor.editor_id),
                            ))
                        }
                    }
                }
                let editor = data.main_split.editors.get(&self.view_id).unwrap();
                if let BufferContent::File(_) = &editor.content {
                    if let EditorView::Diff(version) = &data.editor.view {
                        if let Some(history) = data.doc.get_history(version) {
                            let diff_skip = self
                                .check_and_get_diff_skip_mouse_within(
                                    ctx,
                                    &data,
                                    history,
                                    mouse_event.pos,
                                );
                            if let Some(diff_skip) = diff_skip {
                                history.trigger_increase_diff_extend_lines(
                                    &data.doc, diff_skip,
                                )
                            }
                        }
                    }
                };
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
        let last_line = data.doc.buffer().last_line() + 1;
        let char_width = data.config.editor_char_width(ctx.text());
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
        version: &str,
    ) {
        if data.doc.get_history(version).is_none() {
            return;
        }
        let history = data.doc.get_history(version).unwrap();
        let self_size = ctx.size();
        let rect = self_size.to_rect();
        let line_height = data.config.editor.line_height() as f64;
        let scroll_offset = data.editor.scroll_offset;
        let start_line = (scroll_offset.y / line_height).floor() as usize;
        let end_line =
            (scroll_offset.y + rect.height() / line_height).ceil() as usize;
        let current_line = data
            .doc
            .buffer()
            .line_of_offset(data.editor.cursor.offset());
        let last_line = data.doc.buffer().last_line();
        let width = data.config.editor_char_width(ctx.text());

        let mut line = 0;
        for change in history.changes().iter() {
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
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64
                            + text_layout.y_offset(line_height)
                            - scroll_offset.y;
                        let pos = Point::new(x, y);
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
                        let x = ((last_line + 1).to_string().len()
                            - left_content.to_string().len())
                            as f64
                            * width;
                        let y = line_height * l as f64
                            + text_layout.y_offset(line_height)
                            - scroll_offset.y;
                        let pos = Point::new(x, y);
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
                    let pos = Point::new(
                        (self_size.width - width * 3.0) / 2.0,
                        line_height * line as f64 - scroll_offset.y,
                    );
                    let text_layout = ctx
                        .text()
                        .new_text_layout("...")
                        .font(
                            data.config.editor.font_family(),
                            data.config.editor.font_size as f64,
                        )
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    ctx.draw_text(&text_layout, pos);
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
                        let x = ((last_line + 1).to_string().len()
                            - content.to_string().len())
                            as f64
                            * width
                            + self.width
                            + 2.0 * width;
                        let y = line_height * l as f64
                            + text_layout.y_offset(line_height)
                            - scroll_offset.y;
                        let pos = Point::new(x, y);
                        ctx.draw_text(&text_layout, pos);

                        if l > end_line {
                            break;
                        }
                    }
                }
            }
        }
    }

    fn check_and_get_diff_skip_mouse_within(
        &self,
        ctx: &mut EventCtx,
        data: &LapceEditorBufferData,
        history: &DocumentHistory,
        mouse_pos: Point,
    ) -> Option<DiffLines> {
        let line_height = data.config.editor.line_height() as f64;
        let self_size = ctx.size();
        let rect = self_size.to_rect();
        let scroll_offset = data.editor.scroll_offset;
        let end_line =
            (scroll_offset.y + rect.height() / line_height).ceil() as usize;

        let mut line = 0;
        for change in history.changes().iter() {
            match change {
                DiffLines::Left(r) => {
                    let len = r.len();
                    line += len;
                }
                DiffLines::Both(_, r) => {
                    let len = r.len();
                    line += len;
                }
                DiffLines::Skip(l, r) => {
                    let rect = Size::new(self_size.width, line_height)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            line_height * line as f64 - scroll_offset.y,
                        ));
                    if rect.contains(mouse_pos) {
                        return Some(DiffLines::Skip(l.clone(), r.clone()));
                    }
                    line += 1;
                }
                DiffLines::Right(r) => {
                    let len = r.len();
                    line += len;
                }
            }
            if line > end_line {
                break;
            }
        }
        None
    }

    fn paint_gutter_code_lens(
        &self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
    ) {
        let rect = ctx.size().to_rect();
        let scroll_offset = data.editor.scroll_offset;
        let empty_lens = Syntax::lens_from_normal_lines(
            data.doc.buffer().len(),
            data.config.editor.line_height(),
            data.config.editor.code_lens_font_size,
            &[],
        );
        let lens = if let Some(syntax) = data.doc.syntax() {
            &syntax.lens
        } else {
            &empty_lens
        };

        let cursor_line = data.doc.buffer().line_of_offset(
            data.editor.cursor.offset().min(data.doc.buffer().len()),
        );
        let last_line = data.doc.buffer().line_of_offset(data.doc.buffer().len());
        let start_line = lens
            .line_of_height(scroll_offset.y.floor() as usize)
            .min(last_line);
        let end_line = lens
            .line_of_height(
                (scroll_offset.y + rect.height()).ceil() as usize
                    + data.config.editor.line_height(),
            )
            .min(last_line);
        let char_width = data.config.editor_char_width(ctx.text());
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
            let is_small = line_height < data.config.editor.line_height();
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
                        text_layout.y_offset(line_height as f64)
                    },
            );
            ctx.draw_text(&text_layout, pos);

            y += line_height as f64;
        }
    }

    fn code_actions_rect(
        &self,
        text: &mut PietText,
        data: &LapceEditorBufferData,
    ) -> Rect {
        let line_height = data.config.editor.line_height() as f64;
        let offset = data.editor.cursor.offset();
        let (line, _) = data.doc.buffer().offset_to_line_col(offset);

        let width = 16.0;
        let height = 16.0;
        let char_width = data.config.editor_char_width(text);
        Size::new(width, height).to_rect().with_origin(Point::new(
            self.width + char_width + 3.0,
            (line_height - height) / 2.0 + line_height * line as f64
                - data.editor.scroll_offset.y,
        ))
    }

    fn paint_code_actions_hint(
        &self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
    ) {
        if let Some((_plugin_id, actions)) = data.current_code_actions() {
            if !actions.is_empty() {
                let svg = data.config.ui_svg(LapceIcons::LIGHTBULB);
                let rect = self.code_actions_rect(ctx.text(), data);
                ctx.draw_svg(
                    &svg,
                    rect,
                    Some(data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)),
                );
            }
        }
    }

    fn paint_sticky_header(
        &self,
        ctx: &mut PaintCtx,
        data: &LapceEditorBufferData,
        line_label_length: f64,
    ) {
        if !data.config.editor.sticky_header {
            return;
        }

        let size = ctx.size();
        let line_height = data.config.editor.line_height() as f64;

        let info = data.editor.sticky_header.borrow();

        let sticky_area_rect = Size::new(size.width, info.height)
            .to_rect()
            .with_origin(Point::new(0.0, 0.0));

        ctx.fill(
            sticky_area_rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        let total_lines = info.lines.len();
        for (i, line) in info.lines.iter().enumerate() {
            let y_diff = if i == total_lines - 1 {
                info.last_y_diff
            } else {
                0.0
            };

            let rect = Size::new(size.width, line_height - y_diff)
                .to_rect()
                .with_origin(Point::new(0.0, line_height * i as f64));
            ctx.with_save(|ctx| {
                ctx.clip(rect);
                let text_layout = ctx
                    .text()
                    .new_text_layout((line + 1).to_string())
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
                let x = line_label_length - text_layout.size().width;
                let y = line_height * i as f64 + text_layout.y_offset(line_height)
                    - y_diff;
                ctx.draw_text(&text_layout, Point::new(x, y));
            });
        }
    }

    fn paint_gutter(&self, data: &LapceEditorBufferData, ctx: &mut PaintCtx) {
        let rect = ctx.size().to_rect();
        ctx.with_save(|ctx| {
            let clip_rect = rect;
            ctx.clip(clip_rect);
            if let EditorView::Diff(version) = &data.editor.view {
                self.paint_gutter_inline_diff(data, ctx, version);
                return;
            }
            if data.editor.is_code_lens() {
                self.paint_gutter_code_lens(data, ctx);
                return;
            }
            let line_height = data.config.editor.line_height() as f64;
            let scroll_offset = data.editor.scroll_offset;
            let start_line = (scroll_offset.y / line_height).floor() as usize;
            let num_lines = (ctx.size().height / line_height).floor() as usize;
            let last_line = data.doc.buffer().last_line();
            let current_line = data
                .doc
                .buffer()
                .line_of_offset(data.editor.cursor.offset());
            let char_width = data.config.editor_char_width(ctx.text());

            let line_label_length =
                (last_line + 1).to_string().len() as f64 * char_width;
            let last_displayed_line = (start_line + num_lines + 1).min(last_line);

            let sequential_line_numbers = *data.main_split.active
                != Some(data.view_id)
                || data.editor.cursor.is_insert()
                || !data.config.editor.modal_mode_relative_line_numbers;

            let font_family = data.config.editor.font_family();

            for line in start_line..last_displayed_line + 1 {
                let line_no = if sequential_line_numbers || line == current_line {
                    line + 1
                } else {
                    line.abs_diff(current_line)
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
                let x = line_label_length - text_layout.size().width;

                // Vertically centered
                let y = line_height * line as f64 - scroll_offset.y
                    + text_layout.y_offset(line_height);

                ctx.draw_text(&text_layout, Point::new(x, y));
            }

            if let Some(history) = data.doc.get_history("head") {
                let end_line =
                    (scroll_offset.y + rect.height() / line_height).ceil() as usize;

                let mut line = 0;
                let mut last_change = None;
                for change in history.changes().iter() {
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

            self.paint_sticky_header(ctx, data, line_label_length);
        });
    }
}
