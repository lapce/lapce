use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontFamily,
    FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::{LapceTabData, PanelKind},
    editor::EditorLocationNew,
    search::Match,
    split::SplitDirection,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use crate::{
    editor::view::LapceEditorView,
    panel::{LapcePanel, PanelHeaderKind},
    scroll::LapceScrollNew,
    split::LapceSplitNew,
    svg::file_svg_new,
};

#[derive(Clone)]
pub struct SearchData {
    pub active: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub editor_view_id: WidgetId,
    pub matches: Arc<HashMap<PathBuf, Vec<Match>>>,
}

pub fn new_search_panel(data: &LapceTabData) -> LapcePanel {
    let editor_data = data
        .main_split
        .editors
        .get(&data.search.editor_view_id)
        .unwrap();
    let input = LapceEditorView::new(editor_data.view_id, None)
        .hide_header()
        .hide_gutter()
        .padding((15.0, 15.0));
    let split = LapceSplitNew::new(data.search.split_id)
        .horizontal()
        .with_child(input.boxed(), None, 100.0)
        .with_flex_child(
            LapceScrollNew::new(SearchContent::new().boxed())
                .vertical()
                .boxed(),
            None,
            1.0,
        );
    LapcePanel::new(
        PanelKind::Search,
        data.search.widget_id,
        data.search.split_id,
        SplitDirection::Vertical,
        PanelHeaderKind::Simple("Search".to_string()),
        vec![(
            data.search.split_id,
            PanelHeaderKind::None,
            split.boxed(),
            None,
        )],
    )
}

impl SearchData {
    pub fn new() -> Self {
        let editor_view_id = WidgetId::next();
        Self {
            active: editor_view_id,
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            editor_view_id,
            matches: Arc::new(HashMap::new()),
        }
    }

    pub fn new_panel(&self, data: &LapceTabData) -> LapcePanel {
        let editor_data = data
            .main_split
            .editors
            .get(&data.search.editor_view_id)
            .unwrap();
        let input = LapceEditorView::new(editor_data.view_id, None)
            .hide_header()
            .hide_gutter()
            .padding((15.0, 15.0));
        let split = LapceSplitNew::new(self.split_id)
            .horizontal()
            .with_child(input.boxed(), None, 55.0)
            .with_flex_child(
                LapceScrollNew::new(SearchContent::new().boxed())
                    .vertical()
                    .boxed(),
                None,
                1.0,
            );
        LapcePanel::new(
            PanelKind::Search,
            self.widget_id,
            self.split_id,
            SplitDirection::Vertical,
            PanelHeaderKind::Simple("Search".to_string()),
            vec![(self.split_id, PanelHeaderKind::None, split.boxed(), None)],
        )
    }
}

impl Default for SearchData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SearchContent {
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
                        LapceUICommand::JumpToLocation(
                            None,
                            EditorLocationNew {
                                path: path.clone(),
                                position: Some(lsp_types::Position {
                                    line: *line_number as u32 - 1,
                                    character: *start as u32,
                                }),
                                scroll_offset: None,
                                hisotry: None,
                            },
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
        let padding = (self.line_height - 14.0) / 2.0;
        let mut i = 0;
        for (path, matches) in data.search.matches.iter() {
            if matches.len() + 1 + i < min {
                i += matches.len() + 1;
                continue;
            }

            let svg = file_svg_new(path);
            let rect = Size::new(self.line_height, self.line_height)
                .to_rect()
                .with_origin(Point::new(0.0, self.line_height * i as f64))
                .inflate(-padding, -padding);
            ctx.draw_svg(&svg, rect, None);

            let text_layout = ctx
                .text()
                .new_text_layout(
                    path.file_name().unwrap().to_str().unwrap().to_string(),
                )
                .font(FontFamily::SYSTEM_UI, 13.0)
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
                        + (self.line_height - text_layout.size().height) / 2.0,
                ),
            );

            let mut path = path.clone();
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
                    .font(FontFamily::SYSTEM_UI, 13.0)
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
                            + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );
            }

            for (line_number, (start, end), line) in matches {
                i += 1;
                if i > max {
                    return;
                }

                if i >= min {
                    let mut text_layout = ctx
                        .text()
                        .new_text_layout(format!("{line_number}: {line}"))
                        .font(FontFamily::SYSTEM_UI, 13.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                                .clone(),
                        );
                    let prefix = line_number.to_string().len() + 2;
                    text_layout = text_layout.range_attribute(
                        *start + prefix..*end + prefix,
                        TextAttribute::TextColor(focus_color.clone()),
                    );
                    text_layout = text_layout.range_attribute(
                        *start + prefix..*end + prefix,
                        TextAttribute::Weight(FontWeight::BOLD),
                    );
                    let text_layout = text_layout.build().unwrap();
                    ctx.draw_text(
                        &text_layout,
                        Point::new(
                            self.line_height,
                            self.line_height * i as f64
                                + (self.line_height - text_layout.size().height)
                                    / 2.0,
                        ),
                    );
                }
            }
            i += 1;
        }
    }
}
