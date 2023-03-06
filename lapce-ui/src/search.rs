use std::path::PathBuf;

use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
    editor::{EditorLocation, LineCol},
    panel::PanelKind,
};

use crate::{
    editor::view::LapceEditorView,
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::LapceScroll,
    split::LapceSplit,
    tab::LapceIcon,
};

// Global search widget
pub struct SearchInput {
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    icons: Vec<LapceIcon>,
    parent_view_id: WidgetId,
    result_width: f64,
    search_input_padding: f64,
    mouse_pos: Point,
    background_color: Option<&'static str>,
}

impl SearchInput {
    fn new(view_id: WidgetId) -> Self {
        let id = WidgetId::next();

        let search_input_padding = 15.0;
        let input = LapceEditorView::new(view_id, id, None)
            .hide_header()
            .hide_gutter()
            .padding((search_input_padding, search_input_padding));

        let icons = vec![LapceIcon {
            icon: LapceIcons::SEARCH_CASE_SENSITIVE,
            rect: Rect::ZERO,
            command: Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::ToggleCaseSensitive),
                    data: None,
                },
                Target::Widget(view_id),
            ),
        }];

        Self {
            parent_view_id: view_id,
            result_width: 75.0,
            input: WidgetPod::new(input.boxed()),
            icons,
            mouse_pos: Point::ZERO,
            search_input_padding,
            background_color: Some(LapceTheme::EDITOR_BACKGROUND),
        }
    }

    pub fn clear_background_color(mut self) -> Self {
        self.background_color = None;
        self
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

impl Widget<LapceTabData> for SearchInput {
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
        let input_bc = BoxConstraints::tight(bc.max());
        let mut input_size = self.input.layout(ctx, &input_bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);
        let icon_len = self.icons.len() as f64;
        let height = input_size.height;
        let icon_height = height - self.search_input_padding;
        let mut width = input_size.width + self.result_width + height * icon_len;

        if width > bc.max().width {
            let input_bc = BoxConstraints::tight(Size::new(
                bc.max().width - height * icon_len - self.result_width,
                bc.max().height,
            ));
            input_size = self.input.layout(ctx, &input_bc, data, env);
            self.input.set_origin(ctx, data, env, Point::ZERO);
            width = input_size.width + self.result_width + height * icon_len;
        }

        for (i, icon) in self.icons.iter_mut().enumerate() {
            icon.rect = Size::new(icon_height, icon_height)
                .to_rect()
                .with_origin(Point::new(
                    input_size.width + self.result_width + i as f64 * icon_height,
                    self.search_input_padding / 2.0,
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
        let buffer = data.editor_view_content(self.parent_view_id);

        if let Some(background_color) = self.background_color {
            let rect = ctx.size().to_rect();
            ctx.fill(rect, data.config.get_color_unchecked(background_color));
        }
        self.input.paint(ctx, data, env);

        let mut index = None;
        let cursor_offset = buffer.editor.cursor.offset();

        for i in 0..buffer.doc.find.borrow().occurrences().regions().len() {
            let region = buffer.doc.find.borrow().occurrences().regions()[i];
            if region.min() <= cursor_offset && cursor_offset <= region.max() {
                index = Some(i);
            }
        }

        let match_count = data
            .search
            .matches
            .iter()
            .map(|(_, matches)| matches.len())
            .sum::<usize>();

        let text_layout = ctx
            .text()
            .new_text_layout(if match_count > 0 {
                match index {
                    Some(index) => format!("{}/{}", index + 1, match_count),
                    None => format!("{match_count} results"),
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

pub fn new_search_panel(data: &LapceTabData) -> LapcePanel {
    let editor_data = data
        .main_split
        .editors
        .get(&data.search.editor_view_id)
        .unwrap();

    let search_bar = SearchInput::new(editor_data.view_id).clear_background_color();

    let split = LapceSplit::new(data.search.split_id)
        .horizontal()
        .with_child(search_bar.boxed(), None, 100.0)
        .with_flex_child(
            LapceScroll::new(SearchContent::new().boxed())
                .vertical()
                .boxed(),
            None,
            1.0,
            false,
        )
        .hide_border();

    LapcePanel::new(
        PanelKind::Search,
        data.search.widget_id,
        data.search.split_id,
        vec![(
            data.search.split_id,
            PanelHeaderKind::None,
            split.boxed(),
            PanelSizing::Flex(false),
        )],
    )
}

struct SearchContent {
    mouse_pos: Point,
    line_height: f64,
}

impl SearchContent {
    pub fn new() -> Self {
        Self {
            mouse_pos: Point::ZERO,
            line_height: 25.0,
        }
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &LapceTabData,
    ) {
        let n = (mouse_event.pos.y / self.line_height).floor() as usize;

        let mut i = 0;
        for (path, matches) in data.search.matches.iter() {
            if matches.len() + 1 + i < n {
                i += matches.len() + 1;
                continue;
            }

            for (line_number, (start, _end), _line) in matches {
                i += 1;
                if i == n {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::JumpToLineColLocation(
                            None,
                            EditorLocation {
                                path: path.clone(),
                                position: Some(LineCol {
                                    line: line_number.saturating_sub(1),
                                    column: *start,
                                }),
                                scroll_offset: None,
                                history: None,
                            },
                            false,
                        ),
                        Target::Widget(data.id),
                    ));
                    return;
                }
            }
            i += 1;
        }
    }
}

impl Default for SearchContent {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for SearchContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                ctx.set_cursor(&Cursor::Pointer);
                ctx.request_paint();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event, data);
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
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if !old_data.search.matches.same(&data.search.matches) {
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
        let n = data
            .search
            .matches
            .iter()
            .map(|(_, matches)| matches.len() + 1)
            .sum::<usize>();

        let height = self.line_height * n as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if ctx.is_hot() {
            let size = ctx.size();
            let n = (self.mouse_pos.y / self.line_height).floor() as usize;
            ctx.fill(
                Size::new(size.width, self.line_height)
                    .to_rect()
                    .with_origin(Point::new(0.0, self.line_height * n as f64)),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
            );
        }

        let rect = ctx.region().bounding_box();
        let min = (rect.y0 / self.line_height).floor() as usize;
        let max = (rect.y1 / self.line_height) as usize + 2;

        let focus_color = data.config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);
        let mut i = 0;
        for (path, matches) in data.search.matches.iter() {
            if matches.len() + 1 + i < min {
                i += matches.len() + 1;
                continue;
            }

            let svg_size = data.config.ui.icon_size() as f64;
            let (svg, svg_color) = data.config.file_svg(path);
            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        (self.line_height - svg_size) / 2.0,
                        self.line_height * i as f64
                            + (self.line_height - svg_size) / 2.0,
                    ));
            ctx.draw_svg(&svg, rect, svg_color);

            let text_layout = ctx
                .text()
                .new_text_layout(
                    path.file_name().unwrap().to_str().unwrap().to_string(),
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
            ctx.draw_text(
                &text_layout,
                Point::new(
                    self.line_height,
                    self.line_height * i as f64
                        + text_layout.y_offset(self.line_height),
                ),
            );

            let mut path: PathBuf = path.clone();
            if let Some(workspace_path) = data.workspace.path.as_ref() {
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
                let x = text_layout.size().width + self.line_height + 5.0;

                let text_layout = ctx
                    .text()
                    .new_text_layout(folder)
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
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        x,
                        self.line_height * i as f64
                            + text_layout.y_offset(self.line_height),
                    ),
                );
            }

            for (line_number, (start, end), line) in matches {
                i += 1;
                if i > max {
                    return;
                }

                let whitespace_count: usize =
                    if data.config.ui.trim_search_results_whitespace() {
                        line.chars()
                            .take_while(|ch| ch.is_whitespace() && *ch != '\n')
                            .map(|ch| ch.len_utf8())
                            .sum()
                    } else {
                        0
                    };

                if i >= min {
                    let mut text_layout = ctx
                        .text()
                        .new_text_layout(format!(
                            "{}: {}",
                            line_number,
                            &line[whitespace_count..]
                        ))
                        .font(
                            data.config.ui.font_family(),
                            data.config.ui.font_size() as f64,
                        )
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                                .clone(),
                        );
                    let prefix = line_number.to_string().len() + 2;
                    text_layout = text_layout.range_attribute(
                        *start + prefix - whitespace_count
                            ..*end + prefix - whitespace_count,
                        TextAttribute::TextColor(focus_color.clone()),
                    );
                    text_layout = text_layout.range_attribute(
                        *start + prefix - whitespace_count
                            ..*end + prefix - whitespace_count,
                        TextAttribute::Weight(FontWeight::BOLD),
                    );
                    let text_layout = text_layout.build().unwrap();
                    ctx.draw_text(
                        &text_layout,
                        Point::new(
                            self.line_height,
                            self.line_height * i as f64
                                + text_layout.y_offset(self.line_height),
                        ),
                    );
                }
            }
            i += 1;
        }
    }
}
