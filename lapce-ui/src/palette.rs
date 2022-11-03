use std::{path::Path, sync::Arc};

use druid::{
    kurbo::{Line, Rect},
    piet::{Svg, Text, TextAttribute, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Data, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, RenderContext,
    Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceTheme},
    data::{LapceTabData, LapceWorkspaceType},
    keypress::KeyPressFocus,
    list::ListData,
    palette::{
        PaletteItem, PaletteItemContent, PaletteListData, PaletteStatus,
        PaletteType, PaletteViewData,
    },
};
use lsp_types::SymbolKind;

use crate::{
    editor::view::LapceEditorView,
    list::{List, ListPaint},
};

pub struct Palette {
    widget_id: WidgetId,
    container: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl Palette {
    pub fn new(data: &LapceTabData) -> Self {
        let container = PaletteContainer::new(data);
        Self {
            widget_id: data.palette.widget_id,
            container: WidgetPod::new(container).boxed(),
        }
    }
}

impl Widget<LapceTabData> for Palette {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        // match event {
        //     Event::MouseDown(_)
        //     | Event::MouseMove(_)
        //     | Event::Wheel(_)
        //     | Event::MouseUp(_) => {
        //         if data.palette.status == PaletteStatus::Inactive {
        //             return;
        //         }
        //     }
        //     _ => (),
        // }

        match event {
            // Event::KeyDown(key_event) => {
            //     let mut keypress = data.keypress.clone();
            //     let mut_keypress = Arc::make_mut(&mut keypress);
            //     let mut palette_data = data.palette_view_data();
            //     mut_keypress.key_down(ctx, key_event, &mut palette_data, env);
            //     data.palette = palette_data.palette.clone();
            //     data.keypress = keypress;
            //     data.workspace = palette_data.workspace.clone();
            //     data.main_split = palette_data.main_split.clone();
            //     data.find = palette_data.find.clone();
            //     ctx.set_handled();
            // }
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_COMMAND);
                let mut palette_data = data.palette_view_data();
                palette_data.run_command(
                    ctx,
                    command,
                    None,
                    Modifiers::default(),
                    env,
                );
                data.palette = palette_data.palette.clone();
                data.workspace = palette_data.workspace.clone();
                data.main_split = palette_data.main_split.clone();
                data.find = palette_data.find.clone();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::RunPalette(palette_type) => {
                        ctx.set_handled();
                        let mut palette_data = data.palette_view_data();
                        palette_data.run(ctx, palette_type.to_owned(), None, true);
                        data.palette = palette_data.palette.clone();
                        data.keypress = palette_data.keypress.clone();
                        data.workspace = palette_data.workspace.clone();
                        data.main_split = palette_data.main_split.clone();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.palette.input_editor),
                        ));
                    }
                    LapceUICommand::RunPaletteReferences(locations) => {
                        let mut palette_data = data.palette_view_data();
                        palette_data.run_references(ctx, locations);
                        data.palette = palette_data.palette.clone();
                        data.keypress = palette_data.keypress.clone();
                        data.workspace = palette_data.workspace.clone();
                        data.main_split = palette_data.main_split.clone();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.palette.input_editor),
                        ));
                    }
                    LapceUICommand::CancelPalette => {
                        let mut palette_data = data.palette_view_data();
                        palette_data.cancel(ctx);
                        data.palette = palette_data.palette.clone();
                    }
                    LapceUICommand::UpdatePaletteItems(run_id, items) => {
                        let palette = Arc::make_mut(&mut data.palette);
                        if &palette.run_id == run_id {
                            palette.total_items = items.clone();
                            palette.preview(ctx);
                            if palette.get_input() == "" {
                                palette.list_data.items =
                                    palette.total_items.clone();
                            } else {
                                let _ = palette.sender.send((
                                    palette.run_id.clone(),
                                    palette.get_input().to_string(),
                                    palette.total_items.clone(),
                                ));
                            }
                        }
                    }
                    LapceUICommand::FilterPaletteItems(
                        run_id,
                        input,
                        filtered_items,
                    ) => {
                        let palette = Arc::make_mut(&mut data.palette);
                        if &palette.run_id == run_id && palette.get_input() == input
                        {
                            palette.list_data.items = filtered_items.clone();
                            palette.preview(ctx);
                        }
                    }
                    LapceUICommand::ListItemSelected => {
                        data.palette_view_data().select(ctx);
                    }
                    _ => {}
                }
            }
            _ => {
                self.container.event(ctx, event, data, env);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.container.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if !old_data.palette.same(&data.palette) {
            ctx.request_layout();
        }

        self.container.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();

        self.container.layout(ctx, bc, data, env);
        self.container.set_origin(ctx, data, env, Point::ZERO);

        ctx.set_paint_insets(4000.0);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.container.paint(ctx, data, env);
    }
}

struct PaletteContainer {
    content_rect: Rect,
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    #[allow(clippy::type_complexity)]
    content: WidgetPod<
        ListData<PaletteItem, PaletteListData>,
        List<PaletteItem, PaletteListData>,
    >,
    preview: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl PaletteContainer {
    pub fn new(data: &LapceTabData) -> Self {
        let preview_editor = data
            .main_split
            .editors
            .get(&data.palette.preview_editor)
            .unwrap();
        let input =
            LapceEditorView::new(data.palette.input_editor, WidgetId::next(), None)
                .hide_header()
                .hide_gutter()
                .padding((10.0, 5.0, 10.0, 5.0));
        let content = List::new(data.palette.scroll_id);
        let preview =
            LapceEditorView::new(preview_editor.view_id, WidgetId::next(), None);
        Self {
            content_rect: Rect::ZERO,
            input: WidgetPod::new(input.boxed()),
            content: WidgetPod::new(content),
            preview: WidgetPod::new(preview.boxed()),
        }
    }

    fn ensure_item_visible(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.widget_mut().ensure_item_visible(
            ctx,
            &data.palette.list_data.clone_with(data.config.clone()),
            env,
        );
    }
}

impl Widget<LapceTabData> for PaletteContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.input.event(ctx, event, data, env);

        let palette = Arc::make_mut(&mut data.palette);
        palette.list_data.update_data(data.config.clone());
        self.content.event(ctx, event, &mut palette.list_data, env);

        self.preview.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.lifecycle(ctx, event, data, env);
        self.content.lifecycle(
            ctx,
            event,
            &data.palette.list_data.clone_with(data.config.clone()),
            env,
        );
        self.preview.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if old_data.palette.input != data.palette.input
            || old_data.palette.run_id != data.palette.run_id
        {
            self.ensure_item_visible(ctx, data, env);
        }
        self.input.update(ctx, data, env);
        self.content.update(
            ctx,
            &data.palette.list_data.clone_with(data.config.clone()),
            env,
        );
        self.preview.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = bc.max().width;
        let max_height = bc.max().height;

        let bc = BoxConstraints::tight(Size::new(width, max_height));

        let input_size = self.input.layout(ctx, &bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);

        let height = max_height - input_size.height;
        let bc = BoxConstraints::tight(Size::new(width, height));
        let mut content_size =
            self.content.layout(ctx, &bc, &data.palette.list_data, env);
        if content_size.height > 0.0 {
            content_size.height += 5.0;
        }
        self.content.set_origin(
            ctx,
            &data.palette.list_data.clone_with(data.config.clone()),
            env,
            Point::new(0.0, input_size.height),
        );

        let max_preview_height = max_height
            - input_size.height
            - data.palette.list_data.max_displayed_items as f64
                * data.palette.list_data.line_height() as f64
            - 5.0;
        let preview_height = if data.palette.palette_type.has_preview() {
            if content_size.height > 0.0 {
                max_preview_height
            } else {
                0.0
            }
        } else {
            0.0
        };
        let bc = BoxConstraints::tight(Size::new(
            f64::max(width, data.config.ui.preview_editor_width() as f64),
            max_preview_height,
        ));
        let preview_size = self.preview.layout(ctx, &bc, data, env);
        let preview_width = if preview_size.width > width {
            width - preview_size.width as f64
        } else {
            0.00
        };
        self.preview.set_origin(
            ctx,
            data,
            env,
            Point::new(preview_width / 2.0, input_size.height + content_size.height),
        );

        ctx.set_paint_insets(4000.0);

        let self_size = Size::new(
            width,
            input_size.height + content_size.height + preview_height,
        );
        self.content_rect = Size::new(width, self_size.height)
            .to_rect()
            .with_origin(Point::new(0.0, 0.0));
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.palette.status != PaletteStatus::Inactive {
            let rect = self.content_rect;
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
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::PALETTE_BACKGROUND),
            );
        }

        self.input.paint(ctx, data, env);

        if data.palette.status == PaletteStatus::Inactive {
            return;
        }

        self.content.paint(
            ctx,
            &data.palette.list_data.clone_with(data.config.clone()),
            env,
        );

        if !data.palette.is_empty() && data.palette.palette_type.has_preview() {
            let rect = self.preview.layout_rect();
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
            self.preview.paint(ctx, data, env);
        }
    }
}

pub struct PaletteInput {}

impl PaletteInput {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for PaletteInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<PaletteViewData> for PaletteInput {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut PaletteViewData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &PaletteViewData,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &PaletteViewData,
        _env: &Env,
    ) -> Size {
        Size::new(bc.max().width, 14.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, _env: &Env) {
        let text = data.palette.input.clone();
        let cursor = data.palette.cursor;

        let text_layout = if text.is_empty()
            && data.palette.palette_type == PaletteType::SshHost
        {
            ctx.text()
                .new_text_layout("Enter your SSH details, like user@host")
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
                .unwrap()
        } else {
            ctx.text()
                .new_text_layout(text)
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
                .unwrap()
        };

        let pos = text_layout.hit_test_text_position(cursor);
        let line_metric = text_layout.line_metric(0).unwrap();
        let p0 = (pos.point.x, line_metric.y_offset);
        let p1 = (pos.point.x, line_metric.y_offset + line_metric.height);
        let line = Line::new(p0, p1);

        ctx.stroke(
            line,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            1.0,
        );
        ctx.draw_text(&text_layout, Point::new(0.0, 0.0));
    }
}

pub struct PalettePreview {}

impl PalettePreview {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for PalettePreview {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<PaletteViewData> for PalettePreview {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut PaletteViewData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &PaletteViewData,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        _env: &Env,
    ) -> Size {
        if data.palette.palette_type.has_preview() {
            bc.max()
        } else {
            Size::ZERO
        }
    }

    fn paint(&mut self, _ctx: &mut PaintCtx, _data: &PaletteViewData, _env: &Env) {}
}

struct PaletteItemPaintInfo {
    svg: Option<Svg>,
    svg_color: Option<Color>,
    text: String,
    text_indices: Vec<usize>,
    hint: String,
    hint_indices: Vec<usize>,
}

impl PaletteItemPaintInfo {
    /// Construct paint info when there is only known text and text indices
    fn new_text(text: String, text_indices: Vec<usize>) -> PaletteItemPaintInfo {
        PaletteItemPaintInfo {
            svg: None,
            svg_color: None,
            text,
            text_indices,
            hint: String::new(),
            hint_indices: Vec::new(),
        }
    }
}

impl ListPaint<PaletteListData> for PaletteItem {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &ListData<Self, PaletteListData>,
        _env: &Env,
        line: usize,
    ) {
        let PaletteItemPaintInfo {
            svg,
            svg_color,
            text,
            text_indices,
            hint,
            hint_indices,
        } = match &self.content {
            PaletteItemContent::File(path, _) => {
                file_paint_items(path, &self.indices, data)
            }
            PaletteItemContent::DocumentSymbol {
                kind,
                name,
                container_name,
                ..
            } => {
                let text = name.to_string();
                let hint = container_name.clone().unwrap_or_default();
                let text_indices = self
                    .indices
                    .iter()
                    .filter_map(|i| {
                        let i = *i;
                        if i < text.len() {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                let hint_indices = self
                    .indices
                    .iter()
                    .filter_map(|i| {
                        let i = *i;
                        if i >= text.len() {
                            Some(i - text.len())
                        } else {
                            None
                        }
                    })
                    .collect();
                PaletteItemPaintInfo {
                    svg: data.config.symbol_svg(kind),
                    svg_color: Some(
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)
                            .clone(),
                    ),
                    text,
                    text_indices,
                    hint,
                    hint_indices,
                }
            }
            PaletteItemContent::WorkspaceSymbol {
                kind,
                name,
                location,
                ..
            } => file_paint_symbols(
                &location.path,
                &self.indices,
                data.data
                    .workspace
                    .as_ref()
                    .and_then(|workspace| workspace.path.as_deref()),
                name.as_str(),
                *kind,
                &data.config,
            ),
            PaletteItemContent::Line(_, text) => {
                PaletteItemPaintInfo::new_text(text.clone(), self.indices.to_vec())
            }
            PaletteItemContent::ReferenceLocation(rel_path, _location) => {
                file_paint_items(rel_path, &self.indices, data)
            }
            PaletteItemContent::Workspace(w) => {
                let text = w.path.as_ref().unwrap().to_str().unwrap();
                let text = match &w.kind {
                    LapceWorkspaceType::Local => text.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("[{user}@{host}] {text}")
                    }
                    LapceWorkspaceType::RemoteWSL => {
                        format!("[wsl] {text}")
                    }
                };
                PaletteItemPaintInfo::new_text(text, self.indices.to_vec())
            }
            PaletteItemContent::Command(command) => {
                let text = command
                    .kind
                    .desc()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "".to_string());
                PaletteItemPaintInfo::new_text(text, self.indices.to_vec())
            }
            PaletteItemContent::ColorTheme(theme) => PaletteItemPaintInfo::new_text(
                theme.to_string(),
                self.indices.to_vec(),
            ),
            PaletteItemContent::IconTheme(theme) => PaletteItemPaintInfo::new_text(
                theme.to_string(),
                self.indices.to_vec(),
            ),
            PaletteItemContent::Language(name) => PaletteItemPaintInfo::new_text(
                name.to_string(),
                self.indices.to_vec(),
            ),
            PaletteItemContent::TerminalLine(_line, content) => {
                PaletteItemPaintInfo::new_text(
                    content.clone(),
                    self.indices.to_vec(),
                )
            }
            PaletteItemContent::SshHost(user, host) => {
                PaletteItemPaintInfo::new_text(
                    format!("{user}@{host}"),
                    self.indices.to_vec(),
                )
            }
        };

        let line_height = data.line_height() as f64;

        if let Some(svg) = svg.as_ref() {
            let svg_size = data.config.ui.icon_size() as f64;
            let rect =
                Size::new(svg_size, svg_size)
                    .to_rect()
                    .with_origin(Point::new(
                        (line_height - svg_size) / 2.0 + 5.0,
                        (line_height - svg_size) / 2.0 + line_height * line as f64,
                    ));
            ctx.draw_svg(svg, rect, svg_color.as_ref());
        }

        let svg_x = match &self.content {
            &PaletteItemContent::Line(_, _) | &PaletteItemContent::Workspace(_) => {
                0.0
            }
            _ => line_height,
        };

        let focus_color = data.config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);

        let full_text = if hint.is_empty() {
            text.clone()
        } else {
            text.clone() + " " + &hint
        };
        let mut text_layout = ctx
            .text()
            .new_text_layout(full_text.clone())
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );
        for &i_start in &text_indices {
            let i_end = full_text
                .char_indices()
                .find(|(i, _)| *i == i_start)
                .map(|(_, c)| c.len_utf8() + i_start);
            let i_end = if let Some(i_end) = i_end {
                i_end
            } else {
                // Log a warning, but continue as we don't want to crash on a bug
                log::warn!(
                    "Invalid text indices in palette: text: '{}', i_start: {}",
                    text,
                    i_start
                );
                continue;
            };

            text_layout = text_layout.range_attribute(
                i_start..i_end,
                TextAttribute::TextColor(focus_color.clone()),
            );
            text_layout = text_layout.range_attribute(
                i_start..i_end,
                TextAttribute::Weight(FontWeight::BOLD),
            );
        }

        if !hint.is_empty() {
            text_layout = text_layout
                .range_attribute(
                    text.len() + 1..full_text.len(),
                    TextAttribute::FontSize(13.0),
                )
                .range_attribute(
                    text.len() + 1..full_text.len(),
                    TextAttribute::TextColor(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    ),
                );
            for i in &hint_indices {
                let i = *i + text.len() + 1;
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::TextColor(focus_color.clone()),
                );
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::Weight(FontWeight::BOLD),
                );
            }
        }

        let text_layout = text_layout.build().unwrap();
        let x = svg_x + 5.0;
        let y = line_height * line as f64 + text_layout.y_offset(line_height);
        let point = Point::new(x, y);
        ctx.draw_text(&text_layout, point);
    }
}

fn file_paint_symbols(
    path: &Path,
    indices: &[usize],
    workspace_path: Option<&Path>,
    name: &str,
    kind: SymbolKind,
    config: &LapceConfig,
) -> PaletteItemPaintInfo {
    let text = name.to_string();
    let hint = path.to_string_lossy();
    // Remove the workspace prefix from the path
    let hint = workspace_path
        .and_then(Path::to_str)
        .and_then(|x| hint.strip_prefix(x))
        .map(|x| x.strip_prefix('/').unwrap_or(x))
        .map(ToString::to_string)
        .unwrap_or_else(|| hint.to_string());
    let text_indices = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if i < text.len() {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    let hint_indices = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if i >= text.len() {
                Some(i - text.len())
            } else {
                None
            }
        })
        .collect();
    PaletteItemPaintInfo {
        svg: config.symbol_svg(&kind),
        svg_color: Some(
            config
                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)
                .clone(),
        ),
        text,
        text_indices,
        hint,
        hint_indices,
    }
}

fn file_paint_items(
    path: &Path,
    indices: &[usize],
    data: &ListData<PaletteItem, PaletteListData>,
) -> PaletteItemPaintInfo {
    let (svg, svg_color) = data.config.file_svg(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let folder = path
        .parent()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let folder_len = folder.len();
    let text_indices: Vec<usize> = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if folder_len > 0 {
                if i > folder_len {
                    Some(i - folder_len - 1)
                } else {
                    None
                }
            } else {
                Some(i)
            }
        })
        .collect();
    let hint_indices: Vec<usize> = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if i < folder_len {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    PaletteItemPaintInfo {
        svg: Some(svg),
        svg_color: svg_color.cloned(),
        text: file_name,
        text_indices,
        hint: folder,
        hint_indices,
    }
}
