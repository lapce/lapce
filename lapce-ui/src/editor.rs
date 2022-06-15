use std::time::Duration;
use std::{iter::Iterator, sync::Arc};

use druid::TimerToken;
use druid::{
    kurbo::{BezPath, Line},
    piet::{PietText, PietTextLayout, Text, TextLayout as _, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, InternalLifeCycle,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseButton, MouseEvent, PaintCtx, Point,
    Rect, RenderContext, Size, Target, UpdateCtx, Widget, WidgetId,
};
use lapce_core::buffer::DiffLines;
use lapce_core::command::EditCommand;
use lapce_core::{
    command::FocusCommand,
    cursor::{ColPosition, CursorMode},
    mode::{Mode, VisualMode},
    movement::Movement,
};
use lapce_data::command::CommandKind;
use lapce_data::data::{EditorView, LapceData};
use lapce_data::document::{BufferContent, LocalBufferKind};
use lapce_data::keypress::KeyPressFocus;
use lapce_data::menu::MenuKind;
use lapce_data::{
    command::{
        LapceCommand, LapceUICommand, LapceWorkbenchCommand, LAPCE_UI_COMMAND,
    },
    config::{Config, LapceTheme},
    data::{LapceTabData, PanelData, PanelKind},
    editor::{LapceEditorBufferData, Syntax},
    menu::MenuItem,
    panel::PanelPosition,
};
use lsp_types::{CodeActionOrCommand, DiagnosticSeverity};

pub mod container;
pub mod gutter;
pub mod header;
pub mod tab;
pub mod tab_header;
pub mod tab_header_content;
pub mod view;

pub struct LapceEditor {
    view_id: WidgetId,
    editor_id: WidgetId,
    placeholder: Option<String>,

    mouse_pos: Point,
    /// A timer for listening for when the user has hovered for long enough to trigger showing
    /// of hover info (if there is any)
    mouse_hover_timer: TimerToken,
    drag_timer: TimerToken,
}

impl LapceEditor {
    pub fn new(view_id: WidgetId, editor_id: WidgetId) -> Self {
        Self {
            view_id,
            editor_id,
            placeholder: None,
            mouse_pos: Point::ZERO,
            mouse_hover_timer: TimerToken::INVALID,
            drag_timer: TimerToken::INVALID,
        }
    }

    fn mouse_within_scroll(
        &self,
        editor_data: &LapceEditorBufferData,
        point: Point,
    ) -> bool {
        let scroll_offset = editor_data.editor.scroll_offset;
        let size = *editor_data.editor.size.borrow();

        scroll_offset.x <= point.x
            && point.x <= scroll_offset.x + size.width
            && scroll_offset.y <= point.y
            && point.y <= scroll_offset.y + size.height
    }

    fn mouse_drag(
        &mut self,
        ctx: &mut EventCtx,
        editor_data: &LapceEditorBufferData,
        config: &Config,
    ) -> bool {
        if !ctx.is_active() {
            return false;
        }

        let line_height = config.editor.line_height as f64;
        let scroll_offset = editor_data.editor.scroll_offset;
        let size = *editor_data.editor.size.borrow();

        let y_distance_1 = self.mouse_pos.y - scroll_offset.y;
        let y_distance_2 = scroll_offset.y + size.height - self.mouse_pos.y;

        let y_diff = if y_distance_1 < line_height {
            let shift = if y_distance_1 > 0.0 {
                y_distance_1
            } else {
                0.0
            };
            -line_height + shift
        } else if y_distance_2 < line_height {
            let shift = if y_distance_2 > 0.0 {
                y_distance_2
            } else {
                0.0
            };
            line_height - shift
        } else {
            0.0
        };

        let x_diff = if self.mouse_pos.x < editor_data.editor.scroll_offset.x {
            -5.0
        } else if self.mouse_pos.x
            > editor_data.editor.scroll_offset.x
                + editor_data.editor.size.borrow().width
        {
            5.0
        } else {
            0.0
        };

        if x_diff != 0.0 || y_diff != 0.0 {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Scroll((x_diff, y_diff)),
                Target::Widget(editor_data.view_id),
            ));

            self.drag_timer = ctx.request_timer(Duration::from_millis(16), None);
            return true;
        }

        false
    }

    fn mouse_move(
        &mut self,
        ctx: &mut EventCtx,
        mouse_pos: Point,
        editor_data: &mut LapceEditorBufferData,
        config: &Config,
    ) {
        let mouse_actually_moved = self.mouse_pos != mouse_pos;
        self.mouse_pos = mouse_pos;
        self.mouse_hover_timer = TimerToken::INVALID;

        let dragged = self.mouse_drag(ctx, editor_data, config);

        if !mouse_actually_moved && !dragged {
            return;
        }

        if ctx.is_active() {
            let (new_offset, _) = editor_data.doc.offset_of_point(
                ctx.text(),
                editor_data.get_mode(),
                mouse_pos,
                config.editor.font_size,
                config,
            );
            let editor = Arc::make_mut(&mut editor_data.editor);
            editor.cursor.set_offset(new_offset, true, false);
            return;
        }

        let (offset, is_inside) = editor_data.doc.offset_of_point(
            ctx.text(),
            Mode::Insert,
            mouse_pos,
            config.editor.font_size,
            config,
        );
        let within_scroll = self.mouse_within_scroll(editor_data, mouse_pos);
        if !editor_data.check_hover(ctx, offset, is_inside, within_scroll)
            && is_inside
            && within_scroll
        {
            self.mouse_hover_timer = ctx.request_timer(
                Duration::from_millis(config.editor.hover_delay),
                None,
            );
        }
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        editor_data: &mut LapceEditorBufferData,
        config: &Config,
    ) {
        ctx.set_handled();
        match mouse_event.button {
            MouseButton::Left => {
                self.left_click(ctx, mouse_event, editor_data, config);
                editor_data.get_code_actions(ctx);
                editor_data.cancel_completion();
                // TODO: Don't cancel over here, because it would good to allow the user to
                // select text inside the hover data
                editor_data.cancel_hover();
            }
            MouseButton::Right => {
                self.mouse_hover_timer = TimerToken::INVALID;
                self.right_click(ctx, editor_data, mouse_event, config);
                editor_data.get_code_actions(ctx);
                editor_data.cancel_completion();
                editor_data.cancel_hover();
            }
            MouseButton::Middle => {}
            _ => (),
        }
    }

    fn left_click(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        editor_data: &mut LapceEditorBufferData,
        config: &Config,
    ) {
        match mouse_event.count {
            1 => {
                editor_data.single_click(ctx, mouse_event, config);
            }
            2 => {
                editor_data.double_click(ctx, mouse_event, config);
            }
            3 => {
                editor_data.triple_click(ctx, mouse_event, config);
            }
            _ => {}
        }
    }

    fn right_click(
        &mut self,
        ctx: &mut EventCtx,
        editor_data: &mut LapceEditorBufferData,
        mouse_event: &MouseEvent,
        config: &Config,
    ) {
        let (offset, _) = editor_data.doc.offset_of_point(
            ctx.text(),
            editor_data.get_mode(),
            mouse_event.pos,
            config.editor.font_size,
            config,
        );

        if !editor_data
            .editor
            .cursor
            .edit_selection(editor_data.doc.buffer())
            .contains(offset)
        {
            editor_data.single_click(ctx, mouse_event, config);
        }

        let menu_items = vec![
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::GotoDefinition),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::PaletteCommand,
                    ),
                    data: None,
                },
            }),
            MenuKind::Separator,
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardCut),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardCopy),
                    data: None,
                },
            }),
            MenuKind::Item(MenuItem {
                desc: None,
                command: LapceCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardPaste),
                    data: None,
                },
            }),
        ];

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ShowMenu(
                ctx.to_window(mouse_event.pos),
                Arc::new(menu_items),
            ),
            Target::Widget(*editor_data.main_split.tab_id),
        ));
    }

    pub fn get_size(
        data: &LapceEditorBufferData,
        text: &mut PietText,
        editor_size: Size,
        panels: &im::HashMap<PanelPosition, Arc<PanelData>>,
        env: &Env,
    ) -> Size {
        let line_height = data.config.editor.line_height as f64;
        let width = data.config.editor_char_width(text);
        match &data.editor.content {
            BufferContent::File(_)
            | BufferContent::Scratch(..)
            | BufferContent::Local(LocalBufferKind::Empty) => {
                if data.editor.code_lens {
                    if let Some(syntax) = data.doc.syntax() {
                        let height =
                            syntax.lens.height_of_line(syntax.lens.len() + 1);
                        Size::new(
                            (width * data.doc.buffer().max_len() as f64)
                                .max(editor_size.width),
                            if data.config.editor.scroll_beyond_last_line {
                                (height as f64 - line_height).max(0.0)
                                    + editor_size.height
                            } else {
                                (height as f64).max(editor_size.height)
                            },
                        )
                    } else {
                        let height = data.doc.buffer().num_lines()
                            * data.config.editor.code_lens_font_size;
                        Size::new(
                            (width * data.doc.buffer().max_len() as f64)
                                .max(editor_size.width),
                            if data.config.editor.scroll_beyond_last_line {
                                (height as f64 - line_height).max(0.0)
                                    + editor_size.height
                            } else {
                                (height as f64).max(editor_size.height)
                            },
                        )
                    }
                } else if let Some(compare) = data.editor.compare.as_ref() {
                    let mut lines = 0;
                    if let Some(history) = data.doc.get_history(compare) {
                        for change in history.changes().iter() {
                            match change {
                                DiffLines::Left(l) => lines += l.len(),
                                DiffLines::Both(_l, r) => lines += r.len(),
                                DiffLines::Skip(_l, _r) => lines += 1,
                                DiffLines::Right(r) => lines += r.len(),
                            }
                        }
                    }
                    Size::new(
                        (width * data.doc.buffer().max_len() as f64)
                            .max(editor_size.width),
                        if data.config.editor.scroll_beyond_last_line {
                            (line_height * lines as f64 - line_height).max(0.0)
                                + editor_size.height
                        } else {
                            (line_height * lines as f64).max(editor_size.height)
                        },
                    )
                } else {
                    Size::new(
                        (width * data.doc.buffer().max_len() as f64)
                            .max(editor_size.width),
                        if data.config.editor.scroll_beyond_last_line {
                            (line_height * data.doc.buffer().num_lines() as f64
                                - line_height)
                                .max(0.0)
                                + editor_size.height
                        } else {
                            (line_height * data.doc.buffer().num_lines() as f64)
                                .max(editor_size.height)
                        },
                    )
                }
            }
            BufferContent::Local(LocalBufferKind::SourceControl) => {
                for (pos, panels) in panels.iter() {
                    for panel_kind in panels.widgets.iter() {
                        if panel_kind == &PanelKind::SourceControl {
                            return match pos {
                                PanelPosition::BottomLeft
                                | PanelPosition::BottomRight => {
                                    let width = 200.0;
                                    Size::new(width, editor_size.height)
                                }
                                _ => {
                                    let height = 100.0f64;
                                    let height = height.max(
                                        line_height
                                            * data.doc.buffer().num_lines() as f64,
                                    );
                                    Size::new(
                                        (width * data.doc.buffer().max_len() as f64)
                                            .max(editor_size.width),
                                        height,
                                    )
                                }
                            };
                        }
                    }
                }
                Size::ZERO
            }
            _ => Size::new(
                editor_size.width.max(
                    data.doc
                        .get_text_layout(
                            text,
                            0,
                            data.config.editor.font_size,
                            &data.config,
                        )
                        .size()
                        .width,
                ),
                env.get(LapceTheme::INPUT_LINE_HEIGHT)
                    + env.get(LapceTheme::INPUT_LINE_PADDING) * 2.0,
            ),
        }
    }

    pub fn paint_code_lens_content(
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        is_focused: bool,
    ) {
        let rect = ctx.region().bounding_box();
        let ref_text_layout = ctx
            .text()
            .new_text_layout("W")
            .font(
                data.config.editor.font_family(),
                data.config.editor.font_size as f64,
            )
            .build()
            .unwrap();
        let char_width = ref_text_layout.size().width;
        let y_shift = (data.config.editor.line_height as f64
            - ref_text_layout.size().height)
            / 2.0;
        let small_char_width = data
            .config
            .char_width(ctx.text(), data.config.editor.code_lens_font_size as f64);

        let empty_lens = Syntax::lens_from_normal_lines(
            data.doc.buffer().len(),
            data.config.editor.line_height,
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
        let start_line =
            lens.line_of_height(rect.y0.floor() as usize).min(last_line);
        let end_line = lens
            .line_of_height(rect.y1.ceil() as usize + data.config.editor.line_height)
            .min(last_line);
        let start_offset = data.doc.buffer().offset_of_line(start_line);
        let end_offset = data.doc.buffer().offset_of_line(end_line + 1);
        let mut lines_iter =
            data.doc.buffer().text().lines(start_offset..end_offset);

        let mut y = lens.height_of_line(start_line) as f64;
        for (line, line_height) in lens.iter_chunks(start_line..end_line + 1) {
            if let Some(line_content) = lines_iter.next() {
                let is_small = line_height < data.config.editor.line_height;

                let mut x = 0.0;
                if is_small {
                    for ch in line_content.chars() {
                        if ch == ' ' {
                            x += char_width - small_char_width;
                        } else if ch == '\t' {
                            x += (char_width - small_char_width)
                                * data.config.editor.tab_width as f64;
                        } else {
                            break;
                        }
                    }
                }

                Self::paint_cursor_on_line(
                    data,
                    ctx,
                    is_focused,
                    cursor_line,
                    line,
                    x,
                    y,
                    if is_small {
                        small_char_width
                    } else {
                        char_width
                    },
                    line_height as f64,
                );
                let text_layout = data.doc.get_text_layout(
                    ctx.text(),
                    line,
                    if is_small {
                        data.config.editor.code_lens_font_size
                    } else {
                        data.config.editor.font_size
                    },
                    &data.config,
                );
                ctx.draw_text(
                    &text_layout,
                    Point::new(x, if is_small { y } else { y + y_shift }),
                );
                y += line_height as f64;
            }
        }
    }

    fn paint_content(
        &mut self,
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        is_focused: bool,
        env: &Env,
    ) {
        let line_height = Self::line_height(data, env);
        let line_padding = Self::line_padding(data, env);

        let font_size = if data.editor.content.is_input() {
            env.get(LapceTheme::INPUT_FONT_SIZE) as usize
        } else {
            data.config.editor.font_size
        };

        let text_layout = ctx
            .text()
            .new_text_layout("W")
            .font(data.config.editor.font_family(), font_size as f64)
            .build()
            .unwrap();
        let char_width = text_layout.size().width;
        let y_shift = (line_height - text_layout.size().height) / 2.0;

        //if data.editor.content.is_input()
        //    || (data.editor.compare.is_none() && !data.editor.code_lens)
        //{
        // Self::paint_cursor(
        //     data,
        //     ctx,
        //     is_focused,
        //     self.placeholder.as_ref(),
        //     char_width,
        //     env,
        // );
        // Self::paint_find(data, ctx, char_width, env);
        //}
        let self_size = ctx.size();
        let rect = ctx.region().bounding_box();
        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;

        if !data.editor.content.is_input() && data.editor.code_lens {
            Self::paint_code_lens_content(data, ctx, is_focused);
        } else if let EditorView::Diff(version) = &data.editor.view {
            if let Some(history) = data.doc.get_history(version) {
                let cursor_line = data
                    .doc
                    .buffer()
                    .line_of_offset(data.editor.cursor.offset());
                let mut line = 0;
                for change in history.changes().iter() {
                    match change {
                        DiffLines::Left(range) => {
                            let len = range.len();
                            line += len;

                            if line < start_line {
                                continue;
                            }
                            ctx.fill(
                                Size::new(self_size.width, line_height * len as f64)
                                    .to_rect()
                                    .with_origin(Point::new(
                                        0.0,
                                        line_height * (line - len) as f64,
                                    )),
                                data.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_REMOVED,
                                ),
                            );
                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let actual_line = l - (line - len) + range.start;
                                let text_layout = history.get_text_layout(
                                    ctx.text(),
                                    actual_line,
                                    &data.config,
                                );
                                ctx.draw_text(
                                    &text_layout,
                                    Point::new(
                                        0.0,
                                        line_height * l as f64 + y_shift,
                                    ),
                                );

                                if l > end_line {
                                    break;
                                }
                            }
                        }
                        DiffLines::Skip(left, right) => {
                            let rect = Size::new(self_size.width, line_height)
                                .to_rect()
                                .with_origin(Point::new(
                                    0.0,
                                    line_height * line as f64,
                                ));
                            ctx.fill(
                                rect,
                                data.config.get_color_unchecked(
                                    LapceTheme::PANEL_BACKGROUND,
                                ),
                            );
                            ctx.stroke(
                                rect,
                                data.config.get_color_unchecked(
                                    LapceTheme::EDITOR_FOREGROUND,
                                ),
                                1.0,
                            );
                            let text_layout = ctx
                                .text()
                                .new_text_layout(format!(
                                    " -{}, +{}",
                                    left.end + 1,
                                    right.end + 1
                                ))
                                .font(
                                    data.config.editor.font_family(),
                                    font_size as f64,
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
                                Point::new(0.0, line_height * line as f64 + y_shift),
                            );
                            line += 1;
                        }
                        DiffLines::Both(_left, right) => {
                            let len = right.len();
                            line += len;
                            if line < start_line {
                                continue;
                            }
                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let rope_line = l - (line - len) + right.start;
                                Self::paint_cursor_on_line(
                                    data,
                                    ctx,
                                    is_focused,
                                    cursor_line,
                                    rope_line,
                                    0.0,
                                    l as f64 * line_height,
                                    char_width,
                                    line_height,
                                );
                                let text_layout = data.doc.get_text_layout(
                                    ctx.text(),
                                    rope_line,
                                    font_size,
                                    &data.config,
                                );
                                ctx.draw_text(
                                    &text_layout,
                                    Point::new(
                                        0.0,
                                        line_height * l as f64 + y_shift,
                                    ),
                                );
                                if l > end_line {
                                    break;
                                }
                            }
                        }
                        DiffLines::Right(range) => {
                            let len = range.len();
                            line += len;

                            if line < start_line {
                                continue;
                            }

                            ctx.fill(
                                Size::new(
                                    self_size.width,
                                    line_height * range.len() as f64,
                                )
                                .to_rect()
                                .with_origin(
                                    Point::new(
                                        0.0,
                                        line_height * (line - range.len()) as f64,
                                    ),
                                ),
                                data.config.get_color_unchecked(
                                    LapceTheme::SOURCE_CONTROL_ADDED,
                                ),
                            );

                            for l in line - len..line {
                                if l < start_line {
                                    continue;
                                }
                                let rope_line = l - (line - len) + range.start;
                                Self::paint_cursor_on_line(
                                    data,
                                    ctx,
                                    is_focused,
                                    cursor_line,
                                    rope_line,
                                    0.0,
                                    l as f64 * line_height,
                                    char_width,
                                    line_height,
                                );
                                let text_layout = data.doc.get_text_layout(
                                    ctx.text(),
                                    rope_line,
                                    font_size,
                                    &data.config,
                                );
                                ctx.draw_text(
                                    &text_layout,
                                    Point::new(
                                        0.0,
                                        line_height * l as f64 + y_shift,
                                    ),
                                );
                                if l > end_line {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            return;
        } else {
            let last_line = data.doc.buffer().last_line();

            Self::paint_cursor(
                data,
                ctx,
                is_focused,
                self.placeholder.as_ref(),
                char_width,
                font_size,
                env,
            );
            Self::paint_find(data, ctx, char_width, env);

            for line in start_line..end_line + 1 {
                if line > last_line {
                    break;
                }

                let text_layout = data.doc.get_text_layout(
                    ctx.text(),
                    line,
                    font_size,
                    &data.config,
                );
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        0.0,
                        line_height * line as f64
                            + (line_height - text_layout.size().height) / 2.0
                            + line_padding,
                    ),
                );
            }
        }

        Self::paint_snippet(data, ctx);
        Self::paint_diagnostics(data, ctx);
        if data.doc.buffer().is_empty() {
            if let Some(placeholder) = self.placeholder.as_ref() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(placeholder.to_string())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&text_layout, Point::new(0.0, y_shift));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_cursor_on_line(
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        is_focused: bool,
        cursor_line: usize,
        actual_line: usize,
        x_shift: f64,
        y: f64,
        char_width: f64,
        line_height: f64,
    ) {
        match &data.editor.cursor.mode {
            CursorMode::Normal(_) => {}
            CursorMode::Visual { start, end, mode } => {
                let (start_line, start_col) =
                    data.doc.buffer().offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    data.doc.buffer().offset_to_line_col(*start.max(end));
                if actual_line < start_line || actual_line > end_line {
                    return;
                }

                let left_col = match mode {
                    VisualMode::Normal => {
                        if start_line == actual_line {
                            start_col
                        } else {
                            0
                        }
                    }
                    VisualMode::Linewise => 0,
                    VisualMode::Blockwise => {
                        let max_col =
                            data.doc.buffer().line_end_col(actual_line, false);
                        let left = start_col.min(end_col);
                        if left > max_col {
                            return;
                        }
                        left
                    }
                };

                let right_col = match mode {
                    VisualMode::Normal => {
                        if actual_line == end_line {
                            let max_col =
                                data.doc.buffer().line_end_col(actual_line, true);
                            (end_col + 1).min(max_col)
                        } else {
                            data.doc.buffer().line_end_col(actual_line, true) + 1
                        }
                    }
                    VisualMode::Linewise => {
                        data.doc.buffer().line_end_col(actual_line, true) + 1
                    }
                    VisualMode::Blockwise => {
                        let max_col =
                            data.doc.buffer().line_end_col(actual_line, true);
                        let right = match data.editor.cursor.horiz.as_ref() {
                            Some(&ColPosition::End) => max_col,
                            _ => (end_col.max(start_col) + 1).min(max_col),
                        };
                        right
                    }
                };

                let x0 = left_col as f64 * char_width + x_shift;
                let x1 = right_col as f64 * char_width + x_shift;
                let y0 = y;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );
            }
            CursorMode::Insert(selection) => {
                let start_offset = data.doc.buffer().offset_of_line(actual_line);
                let end_offset = data.doc.buffer().offset_of_line(actual_line + 1);
                let regions = selection.regions_in_range(start_offset, end_offset);
                for region in regions {
                    if region.is_caret() {
                        let caret_actual_line =
                            data.doc.buffer().line_of_offset(region.end());
                        if caret_actual_line == actual_line {
                            let size = ctx.size();
                            ctx.fill(
                                Rect::ZERO
                                    .with_origin(Point::new(0.0, y))
                                    .with_size(Size::new(size.width, line_height)),
                                data.config.get_color_unchecked(
                                    LapceTheme::EDITOR_CURRENT_LINE,
                                ),
                            );
                        }
                    } else {
                        let start = region.start();
                        let end = region.end();
                        let (start_line, start_col) =
                            data.doc.buffer().offset_to_line_col(start.min(end));
                        let (end_line, end_col) =
                            data.doc.buffer().offset_to_line_col(start.max(end));
                        let left_col = match actual_line {
                            _ if actual_line == start_line => start_col,
                            _ => 0,
                        };
                        let right_col = match actual_line {
                            _ if actual_line == end_line => {
                                let max_col = data
                                    .doc
                                    .buffer()
                                    .line_end_col(actual_line, true);
                                end_col.min(max_col)
                            }
                            _ => data.doc.buffer().line_end_col(actual_line, true),
                        };
                        let x0 = left_col as f64 * char_width + x_shift;
                        let x1 = right_col as f64 * char_width + x_shift;
                        let y0 = y;
                        let y1 = y0 + line_height;
                        ctx.fill(
                            Rect::new(x0, y0, x1, y1),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                        );
                    }
                }
                for region in regions {
                    if is_focused {
                        let (caret_actual_line, col) =
                            data.doc.buffer().offset_to_line_col(region.end());
                        if caret_actual_line == actual_line {
                            let x = col as f64 * char_width + x_shift;
                            ctx.stroke(
                                Line::new(
                                    Point::new(x, y),
                                    Point::new(x, y + line_height),
                                ),
                                data.config
                                    .get_color_unchecked(LapceTheme::EDITOR_CARET),
                                2.0,
                            )
                        }
                    }
                }
            }
        }
        if cursor_line == actual_line {
            if let CursorMode::Normal(_) = &data.editor.cursor.mode {
                let size = ctx.size();
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, y))
                        .with_size(Size::new(size.width, line_height)),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                );
            }
            match &data.editor.cursor.mode {
                CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                    if is_focused {
                        let x0 = data
                            .doc
                            .point_of_offset(
                                ctx.text(),
                                data.editor.cursor.offset(),
                                data.config.editor.font_size,
                                &data.config,
                            )
                            .x;
                        let (right_offset, _) = data.doc.move_offset(
                            ctx.text(),
                            data.editor.cursor.offset(),
                            None,
                            1,
                            &Movement::Right,
                            Mode::Insert,
                            data.config.editor.font_size,
                            &data.config,
                        );
                        let x1 = data
                            .doc
                            .point_of_offset(
                                ctx.text(),
                                right_offset,
                                data.config.editor.font_size,
                                &data.config,
                            )
                            .x;
                        let cursor_width =
                            if x1 > x0 { x1 - x0 } else { char_width };
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(x0 + x_shift, y))
                                .with_size(Size::new(cursor_width, line_height)),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                        );
                    }
                }
                CursorMode::Insert(_) => {}
            }
        }
    }

    fn paint_cursor(
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        is_focused: bool,
        placeholder: Option<&String>,
        width: f64,
        font_size: usize,
        env: &Env,
    ) {
        let line_height = Self::line_height(data, env);
        let line_padding = Self::line_padding(data, env);
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.borrow().height
            + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        match &data.editor.cursor.mode {
            CursorMode::Normal(offset) => {
                let line = data.doc.buffer().line_of_offset(*offset);
                Self::paint_cursor_line(data, ctx, line, is_focused, placeholder);

                if is_focused {
                    let x0 = data
                        .doc
                        .point_of_offset(
                            ctx.text(),
                            *offset,
                            font_size,
                            &data.config,
                        )
                        .x;
                    let (right_offset, _) = data.doc.move_offset(
                        ctx.text(),
                        *offset,
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                        data.config.editor.font_size,
                        &data.config,
                    );
                    let x1 = data
                        .doc
                        .point_of_offset(
                            ctx.text(),
                            right_offset,
                            font_size,
                            &data.config,
                        )
                        .x;
                    let char_width = if x1 > x0 { x1 - x0 } else { width };
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(
                                x0,
                                line as f64 * line_height + line_padding,
                            ))
                            .with_size(Size::new(char_width, line_height)),
                        data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                    );
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    data.doc.buffer().offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    data.doc.buffer().offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let left_col = match mode {
                        VisualMode::Normal => match line {
                            _ if line == start_line => start_col,
                            _ => 0,
                        },
                        VisualMode::Linewise => 0,
                        VisualMode::Blockwise => {
                            let max_col =
                                data.doc.buffer().line_end_col(line, false);
                            let left = start_col.min(end_col);
                            if left > max_col {
                                continue;
                            }
                            left
                        }
                    };

                    let (right_col, line_end) = match mode {
                        VisualMode::Normal => match line {
                            _ if line == end_line => {
                                let max_col =
                                    data.doc.buffer().line_end_col(line, true);
                                ((end_col + 1).min(max_col), false)
                            }
                            _ => (data.doc.buffer().line_end_col(line, true), true),
                        },
                        VisualMode::Linewise => {
                            (data.doc.buffer().line_end_col(line, true), true)
                        }
                        VisualMode::Blockwise => {
                            let max_col = data.doc.buffer().line_end_col(line, true);
                            let right = match data.editor.cursor.horiz.as_ref() {
                                Some(&ColPosition::End) => max_col,
                                _ => (end_col.max(start_col) + 1).min(max_col),
                            };
                            (right, false)
                        }
                    };

                    let x0 = data
                        .doc
                        .point_of_line_col(
                            ctx.text(),
                            line,
                            left_col,
                            font_size,
                            &data.config,
                        )
                        .x;
                    let mut x1 = data
                        .doc
                        .point_of_line_col(
                            ctx.text(),
                            line,
                            right_col,
                            font_size,
                            &data.config,
                        )
                        .x;
                    if line_end {
                        x1 += width;
                    }

                    let y0 = line as f64 * line_height + line_padding;
                    let y1 = y0 + line_height;
                    ctx.fill(
                        Rect::new(x0, y0, x1, y1),
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                    );

                    if is_focused {
                        let line = data.doc.buffer().line_of_offset(*end);

                        let x0 = data
                            .doc
                            .point_of_offset(
                                ctx.text(),
                                *end,
                                font_size,
                                &data.config,
                            )
                            .x;
                        let (right_offset, _) = data.doc.move_offset(
                            ctx.text(),
                            *end,
                            None,
                            1,
                            &Movement::Right,
                            Mode::Insert,
                            data.config.editor.font_size,
                            &data.config,
                        );
                        let x1 = data
                            .doc
                            .point_of_offset(
                                ctx.text(),
                                right_offset,
                                font_size,
                                &data.config,
                            )
                            .x;
                        let char_width = if x1 > x0 { x1 - x0 } else { width };
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(
                                    x0,
                                    line as f64 * line_height + line_padding,
                                ))
                                .with_size(Size::new(char_width, line_height)),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                        );
                    }
                }
            }
            CursorMode::Insert(selection) => {
                let last_line = data.doc.buffer().last_line();
                let end_line = if end_line > last_line {
                    last_line
                } else {
                    end_line
                };
                let start = data.doc.buffer().offset_of_line(start_line);
                let end = data.doc.buffer().offset_of_line(end_line + 1);
                let regions = selection.regions_in_range(start, end);
                for region in regions {
                    if region.start() == region.end() {
                        let line = data.doc.buffer().line_of_offset(region.start());
                        Self::paint_cursor_line(
                            data,
                            ctx,
                            line,
                            is_focused,
                            placeholder,
                        );
                    } else {
                        let start = region.start();
                        let end = region.end();
                        let paint_start_line = start_line;
                        let paint_end_line = end_line;
                        let (start_line, start_col) =
                            data.doc.buffer().offset_to_line_col(start.min(end));
                        let (end_line, end_col) =
                            data.doc.buffer().offset_to_line_col(start.max(end));
                        for line in paint_start_line..paint_end_line + 1 {
                            if line < start_line || line > end_line {
                                continue;
                            }

                            let line_content = data.doc.buffer().line_content(line);
                            let left_col = match line {
                                _ if line == start_line => start_col,
                                _ => 0,
                            };
                            let right_col = match line {
                                _ if line == end_line => {
                                    let max_col =
                                        data.doc.buffer().line_end_col(line, true);
                                    end_col.min(max_col)
                                }
                                _ => data.doc.buffer().line_end_col(line, true),
                            };

                            if !line_content.is_empty() {
                                let x0 = data
                                    .doc
                                    .point_of_line_col(
                                        ctx.text(),
                                        line,
                                        left_col,
                                        font_size,
                                        &data.config,
                                    )
                                    .x;
                                let x1 = data
                                    .doc
                                    .point_of_line_col(
                                        ctx.text(),
                                        line,
                                        right_col,
                                        font_size,
                                        &data.config,
                                    )
                                    .x;
                                let y0 = line as f64 * line_height + line_padding;
                                let y1 = y0 + line_height;
                                ctx.fill(
                                    Rect::new(x0, y0, x1, y1),
                                    data.config.get_color_unchecked(
                                        LapceTheme::EDITOR_SELECTION,
                                    ),
                                );
                            }
                        }
                    }
                }

                for region in regions {
                    if is_focused {
                        let (line, col) =
                            data.doc.buffer().offset_to_line_col(region.end());
                        let x = data
                            .doc
                            .point_of_line_col(
                                ctx.text(),
                                line,
                                col,
                                font_size,
                                &data.config,
                            )
                            .x;
                        let y = line as f64 * line_height + line_padding;
                        ctx.stroke(
                            Line::new(
                                Point::new(x, y),
                                Point::new(x, y + line_height),
                            ),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET),
                            2.0,
                        )
                    }
                }
            }
        }
    }

    fn paint_cursor_line(
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        line: usize,
        is_focused: bool,
        placeholder: Option<&String>,
    ) {
        if !is_focused && data.doc.buffer().is_empty() && placeholder.is_some() {
            return;
        }
        if data.editor.content.is_input() {
            return;
        }
        let line_height = data.config.editor.line_height as f64;
        let size = ctx.size();
        ctx.fill(
            Rect::ZERO
                .with_origin(Point::new(0.0, line as f64 * line_height))
                .with_size(Size::new(size.width, line_height)),
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
        );
    }

    fn paint_find(
        data: &LapceEditorBufferData,
        ctx: &mut PaintCtx,
        _char_width: f64,
        env: &Env,
    ) {
        if data.editor.content.is_search() {
            return;
        }
        if !data.find.visual {
            return;
        }
        let line_height = Self::line_height(data, env);
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.borrow().height
            + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;

        let start_offset = data.doc.buffer().offset_of_line(start_line);
        let end_offset = data.doc.buffer().offset_of_line(end_line + 1);
        let cursor_offset = data.editor.cursor.offset();

        // Update the find with the whole document, so the count will be accurate in the widget
        data.doc
            .update_find(&data.find, 0, data.doc.buffer().last_line());
        if data.find.search_string.is_some() {
            for region in data
                .doc
                .find
                .borrow()
                .occurrences()
                .regions_in_range(start_offset, end_offset)
            {
                let start = region.min();
                let end = region.max();
                let active = start <= cursor_offset && cursor_offset <= end;
                let (start_line, start_col) =
                    data.doc.buffer().offset_to_line_col(start);
                let (end_line, end_col) = data.doc.buffer().offset_to_line_col(end);
                for line in start_line..end_line + 1 {
                    let left_col = if line == start_line { start_col } else { 0 };
                    let right_col = if line == end_line {
                        end_col
                    } else {
                        data.doc.buffer().line_end_col(line, true) + 1
                    };
                    let text_layout = data.doc.get_text_layout(
                        ctx.text(),
                        line,
                        data.config.editor.font_size,
                        &data.config,
                    );
                    let x0 = text_layout.hit_test_text_position(left_col).point.x;
                    let x1 = text_layout.hit_test_text_position(right_col).point.x;
                    let y0 = line as f64 * line_height;
                    let y1 = y0 + line_height;
                    let rect = Rect::new(x0, y0, x1, y1);
                    if active {
                        ctx.fill(
                            rect,
                            &data
                                .config
                                .get_color_unchecked(LapceTheme::EDITOR_CARET)
                                .clone()
                                .with_alpha(0.5),
                        );
                    }
                    ctx.stroke(
                        rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                        1.0,
                    );
                }
            }
        }
    }

    fn paint_snippet(data: &LapceEditorBufferData, ctx: &mut PaintCtx) {
        let line_height = data.config.editor.line_height as f64;
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.borrow().height
            + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;
        let width = data.config.editor_char_width(ctx.text());
        if let Some(snippet) = data.editor.snippet.as_ref() {
            for (_, (start, end)) in snippet {
                let paint_start_line = start_line;
                let paint_end_line = end_line;
                let (start_line, start_col) =
                    data.doc.buffer().offset_to_line_col(*start.min(end));
                let (end_line, end_col) =
                    data.doc.buffer().offset_to_line_col(*start.max(end));
                for line in paint_start_line..paint_end_line {
                    if line < start_line || line > end_line {
                        continue;
                    }
                    let line_content = data.doc.buffer().line_content(line);
                    let left_col = match line {
                        _ if line == start_line => start_col,
                        _ => 0,
                    };
                    let x0 = left_col as f64 * width;

                    let right_col = match line {
                        _ if line == end_line => {
                            let max_col = data.doc.buffer().line_end_col(line, true);
                            end_col.min(max_col)
                        }
                        _ => data.doc.buffer().line_end_col(line, true),
                    };
                    if !line_content.is_empty() {
                        let x1 = right_col as f64 * width;
                        let y0 = line as f64 * line_height;
                        let y1 = y0 + line_height;
                        ctx.stroke(
                            Rect::new(x0, y0, x1, y1).inflate(1.0, -0.5),
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                            1.0,
                        );
                    }
                }
            }
        }
    }

    fn paint_diagnostics(data: &LapceEditorBufferData, ctx: &mut PaintCtx) {
        let line_height = data.config.editor.line_height as f64;
        let start_line =
            (data.editor.scroll_offset.y / line_height).floor() as usize;
        let end_line = ((data.editor.size.borrow().height
            + data.editor.scroll_offset.y)
            / line_height)
            .ceil() as usize;

        let mut current = None;
        let cursor_offset = data.editor.cursor.offset();
        if let Some(diagnostics) = data.diagnostics() {
            for diagnostic in diagnostics.iter() {
                let start = diagnostic.diagnostic.range.start;
                let end = diagnostic.diagnostic.range.end;
                if (start.line as usize) <= end_line
                    && (end.line as usize) >= start_line
                {
                    let start_offset = if let Some(range) = diagnostic.range {
                        range.0
                    } else {
                        data.doc.buffer().offset_of_position(&start)
                    };
                    if start_offset == cursor_offset {
                        current = Some(diagnostic.clone());
                    }
                    for line in start.line as usize..end.line as usize + 1 {
                        if line < start_line {
                            continue;
                        }
                        if line > end_line {
                            break;
                        }

                        let text_layout = data.doc.get_text_layout(
                            ctx.text(),
                            line,
                            data.config.editor.font_size,
                            &data.config,
                        );
                        let x0 = if line == start.line as usize {
                            text_layout
                                .hit_test_text_position(start.character as usize)
                                .point
                                .x
                        } else {
                            let (_, col) = data.doc.buffer().offset_to_line_col(
                                data.doc
                                    .buffer()
                                    .first_non_blank_character_on_line(line),
                            );
                            text_layout.hit_test_text_position(col).point.x
                        };
                        let x1 = if line == end.line as usize {
                            text_layout
                                .hit_test_text_position(end.character as usize)
                                .point
                                .x
                        } else {
                            let col =
                                data.doc.buffer().line_end_col(line, false) + 1;
                            text_layout.hit_test_text_position(col).point.x
                        };
                        let _y1 = (line + 1) as f64 * line_height;
                        let y0 = (line + 1) as f64 * line_height - 4.0;

                        let severity = diagnostic
                            .diagnostic
                            .severity
                            .as_ref()
                            .unwrap_or(&DiagnosticSeverity::Information);
                        let color = match severity {
                            DiagnosticSeverity::Error => data
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_ERROR),
                            DiagnosticSeverity::Warning => data
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_WARN),
                            _ => data
                                .config
                                .get_color_unchecked(LapceTheme::LAPCE_WARN),
                        };
                        Self::paint_wave_line(
                            ctx,
                            Point::new(x0, y0),
                            x1 - x0,
                            color,
                        );
                    }
                }
            }
        }

        if let Some(diagnostic) = current {
            if data.editor.cursor.is_normal() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(diagnostic.diagnostic.message.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .max_width(data.editor.size.borrow().width - 20.0)
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let mut text_height = text_size.height;

                let related = diagnostic
                    .diagnostic
                    .related_information
                    .map(|related| {
                        related
                            .iter()
                            .map(|i| {
                                let text_layout = ctx
                                    .text()
                                    .new_text_layout(i.message.clone())
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
                                    .max_width(
                                        data.editor.size.borrow().width - 20.0,
                                    )
                                    .build()
                                    .unwrap();
                                text_height += 10.0 + text_layout.size().height;
                                text_layout
                            })
                            .collect::<Vec<PietTextLayout>>()
                    })
                    .unwrap_or_else(Vec::new);

                let start = diagnostic.diagnostic.range.start;
                let rect = Rect::ZERO
                    .with_origin(Point::new(
                        0.0,
                        (start.line + 1) as f64 * line_height,
                    ))
                    .with_size(Size::new(
                        data.editor.size.borrow().width,
                        text_height + 20.0,
                    ));
                ctx.fill(
                    rect,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );

                let severity = diagnostic
                    .diagnostic
                    .severity
                    .as_ref()
                    .unwrap_or(&DiagnosticSeverity::Information);
                let color = match severity {
                    DiagnosticSeverity::Error => {
                        data.config.get_color_unchecked(LapceTheme::LAPCE_ERROR)
                    }
                    DiagnosticSeverity::Warning => {
                        data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)
                    }
                    _ => data.config.get_color_unchecked(LapceTheme::LAPCE_WARN),
                };
                ctx.stroke(rect, color, 1.0);
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        10.0 + data.editor.scroll_offset.x,
                        (start.line + 1) as f64 * line_height + 10.0,
                    ),
                );
                let mut text_height = text_size.height;

                for text in related {
                    text_height += 10.0;
                    ctx.draw_text(
                        &text,
                        Point::new(
                            10.0 + data.editor.scroll_offset.x,
                            (start.line + 1) as f64 * line_height
                                + 10.0
                                + text_height,
                        ),
                    );
                    text_height += text.size().height;
                }
            }
        }
    }

    fn line_height(data: &LapceEditorBufferData, env: &Env) -> f64 {
        if data.editor.content.is_input() {
            env.get(LapceTheme::INPUT_LINE_HEIGHT)
        } else {
            data.config.editor.line_height as f64
        }
    }

    fn line_padding(data: &LapceEditorBufferData, env: &Env) -> f64 {
        if data.editor.content.is_input() {
            env.get(LapceTheme::INPUT_LINE_PADDING)
        } else {
            0.0
        }
    }

    fn paint_wave_line(
        ctx: &mut PaintCtx,
        origin: Point,
        max_width: f64,
        color: &Color,
    ) {
        let mut path = BezPath::new();
        let mut x = 0.0;
        let width = 3.5;
        let height = 4.0;
        path.move_to(origin + (0.0, height / 2.0));
        let mut direction = 1.0;
        while x < max_width {
            let point = origin + (x, height / 2.0);
            let p1 = point + (width / 2.0, -height / 2.0 * direction);
            let p2 = point + (width, 0.0);
            path.quad_to(p1, p2);
            x += width;
            direction *= -1.0;
        }
        ctx.stroke(path, color, 1.4);
    }
}

impl Widget<LapceTabData> for LapceEditor {
    fn id(&self) -> Option<WidgetId> {
        Some(self.editor_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                ctx.set_cursor(&druid::Cursor::IBeam);
                let doc = data.main_split.editor_doc(self.view_id);
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                let mut editor_data = data.editor_view_content(self.view_id);
                self.mouse_move(
                    ctx,
                    mouse_event.pos,
                    &mut editor_data,
                    &data.config,
                );
                data.update_from_editor_buffer_data(editor_data, &editor, &doc);
            }
            Event::MouseUp(_mouse_event) => {
                ctx.set_active(false);
            }
            Event::MouseDown(mouse_event) => {
                let doc = data.main_split.editor_doc(self.view_id);
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                let mut editor_data = data.editor_view_content(self.view_id);
                self.mouse_down(ctx, mouse_event, &mut editor_data, &data.config);
                data.update_from_editor_buffer_data(editor_data, &editor, &doc);
            }
            Event::Timer(id) => {
                if self.mouse_hover_timer == *id {
                    let editor =
                        data.main_split.editors.get(&self.view_id).unwrap().clone();
                    let mut editor_data = data.editor_view_content(self.view_id);
                    let doc = editor_data.doc.clone();
                    let (offset, _) = doc.offset_of_point(
                        ctx.text(),
                        editor.cursor.get_mode(),
                        self.mouse_pos,
                        data.config.editor.font_size,
                        &data.config,
                    );
                    editor_data.update_hover(ctx, offset);
                    data.update_from_editor_buffer_data(editor_data, &editor, &doc);
                } else if self.drag_timer == *id {
                    let doc = data.main_split.editor_doc(self.view_id);
                    let editor =
                        data.main_split.editors.get(&self.view_id).unwrap().clone();
                    let mut editor_data = data.editor_view_content(self.view_id);
                    self.mouse_move(
                        ctx,
                        self.mouse_pos,
                        &mut editor_data,
                        &data.config,
                    );
                    data.update_from_editor_buffer_data(editor_data, &editor, &doc);
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let cmd = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::ShowCodeActions(point) = cmd {
                    let editor_data = data.editor_view_content(self.view_id);
                    if let Some(actions) = editor_data.current_code_actions() {
                        if !actions.is_empty() {
                            let mut menu = druid::Menu::new("");

                            for action in actions.iter() {
                                let title = match action {
                                    CodeActionOrCommand::Command(c) => {
                                        c.title.clone()
                                    }
                                    CodeActionOrCommand::CodeAction(a) => {
                                        a.title.clone()
                                    }
                                };
                                let mut item = druid::MenuItem::new(title);
                                item = item.command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RunCodeAction(action.clone()),
                                    Target::Widget(editor_data.view_id),
                                ));
                                menu = menu.entry(item);
                            }

                            let point = point.unwrap_or_else(|| {
                                let offset = editor_data.editor.cursor.offset();
                                let (line, col) = editor_data
                                    .doc
                                    .buffer()
                                    .offset_to_line_col(offset);
                                let x = editor_data
                                    .doc
                                    .point_of_line_col(
                                        ctx.text(),
                                        line,
                                        col,
                                        editor_data.config.editor.font_size,
                                        &editor_data.config,
                                    )
                                    .x;
                                let y = editor_data.config.editor.line_height as f64
                                    * (line + 1) as f64;
                                ctx.to_window(Point::new(x, y))
                            });
                            ctx.show_context_menu::<LapceData>(menu, point);
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::Internal(InternalLifeCycle::ParentWindowOrigin) = event {
            let editor = data.main_split.editors.get(&self.view_id).unwrap();
            let current_window_origin = ctx.window_origin();
            if current_window_origin != *editor.window_origin.borrow() {
                *editor.window_origin.borrow_mut() = current_window_origin;
                ctx.request_layout();
            }
        }
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
        env: &Env,
    ) -> Size {
        let editor_data = data.editor_view_content(self.view_id);
        Self::get_size(&editor_data, ctx.text(), bc.max(), &data.panels, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let is_focused = data.focus == self.view_id;
        let data = data.editor_view_content(self.view_id);
        let is_focused = is_focused
            && (data
                .editor
                .last_cursor_instant
                .borrow()
                .elapsed()
                .as_millis()
                / 500)
                % 2
                == 0;
        self.paint_content(&data, ctx, is_focused, env);
    }
}

#[derive(Clone)]
pub struct RegisterContent {}

#[derive(Clone)]
pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}
