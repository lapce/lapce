use std::path::Path;
use std::sync::Arc;

use druid::kurbo::Line;
use druid::piet::{Svg, TextAttribute, TextLayout};
use druid::{
    kurbo::Rect,
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target, UpdateCtx, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use druid::{FontWeight, Modifiers};
use lapce_data::command::LAPCE_COMMAND;
use lapce_data::config::Config;
use lapce_data::data::LapceWorkspaceType;
use lapce_data::palette::PaletteItemContent;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    keypress::KeyPressFocus,
    palette::{PaletteStatus, PaletteType, PaletteViewData, PaletteViewLens},
};

use crate::{
    editor::view::LapceEditorView,
    scroll::{LapceIdentityWrapper, LapceScroll},
    svg::{file_svg, symbol_svg},
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
        match event {
            Event::MouseDown(_)
            | Event::MouseMove(_)
            | Event::Wheel(_)
            | Event::MouseUp(_) => {
                if data.palette.status == PaletteStatus::Inactive {
                    return;
                }
            }
            _ => (),
        }

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
                        palette_data.run(ctx, palette_type.to_owned());
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
                            palette.items = items.to_owned();
                            palette.preview(ctx);
                            if palette.get_input() != "" {
                                let _ = palette.sender.send((
                                    palette.run_id.clone(),
                                    palette.get_input().to_string(),
                                    palette.items.clone(),
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
                            palette.filtered_items = filtered_items.to_owned();
                            palette.preview(ctx);
                        }
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
            ctx.request_paint();
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
        let width = 600.0;
        let self_size = Size::new(width, bc.max().height);

        let bc = BoxConstraints::tight(self_size);
        self.container.layout(ctx, &bc, data, env);
        self.container.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.palette.status == PaletteStatus::Inactive {
            return;
        }

        self.container.paint(ctx, data, env);
    }
}

struct PaletteContainer {
    content_size: Size,
    line_height: f64,
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    #[allow(clippy::type_complexity)]
    content: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<
            LapceScroll<LapceTabData, Box<dyn Widget<LapceTabData>>>,
        >,
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
                .padding(10.0);
        let content = LapceIdentityWrapper::wrap(
            LapceScroll::new(PaletteContent::new().lens(PaletteViewLens).boxed())
                .vertical(),
            data.palette.scroll_id,
        );
        let preview =
            LapceEditorView::new(preview_editor.view_id, WidgetId::next(), None);
        Self {
            content_size: Size::ZERO,
            input: WidgetPod::new(input.boxed()),
            content: WidgetPod::new(content),
            preview: WidgetPod::new(preview.boxed()),
            line_height: 25.0,
        }
    }

    fn ensure_item_visible(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let rect =
            Size::new(width, self.line_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    data.palette.index as f64 * self.line_height,
                ));
        if self
            .content
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(data.palette.scroll_id),
            ));
        }
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
        self.content.event(ctx, event, data, env);
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
        self.content.lifecycle(ctx, event, data, env);
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
            || old_data.palette.index != data.palette.index
        {
            self.ensure_item_visible(ctx, data, env);
        }
        self.input.update(ctx, data, env);
        self.content.update(ctx, data, env);
        self.preview.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = 600.0;
        let max_height = bc.max().height;

        let bc = BoxConstraints::tight(Size::new(width, bc.max().height));
        let input_size = self.input.layout(ctx, &bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);

        let max_items = 15;
        let height = max_items.min(data.palette.len());
        let height = self.line_height * height as f64;
        let bc = BoxConstraints::tight(Size::new(width, height));
        let content_size = self.content.layout(ctx, &bc, data, env);
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, input_size.height));
        let mut content_height = content_size.height;
        if content_height > 0.0 {
            content_height += 6.0;
        }

        let max_preview_height = max_height
            - input_size.height
            - max_items as f64 * self.line_height
            - 6.0;
        let preview_height = if data.palette.palette_type.has_preview() {
            if content_height > 0.0 {
                max_preview_height
            } else {
                0.0
            }
        } else {
            0.0
        };
        let bc = BoxConstraints::tight(Size::new(width, max_preview_height));
        let _preview_size = self.preview.layout(ctx, &bc, data, env);
        self.preview.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, input_size.height + content_height),
        );

        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        let self_size =
            Size::new(width, input_size.height + content_height + preview_height);
        self.content_size = self_size;
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = self.content_size.to_rect();
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

        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);

        if !data.palette.current_items().is_empty()
            && data.palette.palette_type.has_preview()
        {
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

pub struct PaletteContent {
    mouse_down: usize,
    line_height: f64,
}

impl PaletteContent {
    pub fn new() -> Self {
        Self {
            mouse_down: 0,
            line_height: 25.0,
        }
    }

    fn paint_palette_item(
        palette_item_content: &PaletteItemContent,
        ctx: &mut PaintCtx,
        line: usize,
        indices: &[usize],
        line_height: f64,
        config: &Config,
    ) {
        let (svg, text, text_indices, hint, hint_indices) =
            match palette_item_content {
                PaletteItemContent::File(path, _) => {
                    Self::file_paint_items(path, indices)
                }
                PaletteItemContent::DocumentSymbol {
                    kind,
                    name,
                    container_name,
                    ..
                } => {
                    let text = name.to_string();
                    let hint =
                        container_name.clone().unwrap_or_else(|| "".to_string());
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
                    (symbol_svg(kind), text, text_indices, hint, hint_indices)
                }
                PaletteItemContent::Line(_, text) => {
                    (None, text.clone(), indices.to_vec(), "".to_string(), vec![])
                }
                PaletteItemContent::ReferenceLocation(rel_path, _location) => {
                    Self::file_paint_items(rel_path, indices)
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
                    (None, text, indices.to_vec(), "".to_string(), vec![])
                }
                PaletteItemContent::Command(command) => (
                    None,
                    command
                        .kind
                        .desc()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| "".to_string()),
                    indices.to_vec(),
                    "".to_string(),
                    vec![],
                ),
                PaletteItemContent::Theme(theme) => (
                    None,
                    theme.to_string(),
                    indices.to_vec(),
                    "".to_string(),
                    vec![],
                ),
                PaletteItemContent::TerminalLine(_line, content) => (
                    None,
                    content.clone(),
                    indices.to_vec(),
                    "".to_string(),
                    vec![],
                ),
                PaletteItemContent::SshHost(user, host) => (
                    None,
                    format!("{user}@{host}"),
                    indices.to_vec(),
                    "".to_string(),
                    vec![],
                ),
            };

        if let Some(svg) = svg.as_ref() {
            let width = 14.0;
            let height = 14.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (line_height - width) / 2.0 + 5.0,
                (line_height - height) / 2.0 + line_height * line as f64,
            ));
            ctx.draw_svg(svg, rect, None);
        }

        let svg_x = match palette_item_content {
            &PaletteItemContent::Line(_, _) | &PaletteItemContent::Workspace(_) => {
                0.0
            }
            _ => line_height,
        };

        let focus_color = config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);

        let full_text = if hint.is_empty() {
            text.clone()
        } else {
            text.clone() + " " + &hint
        };
        let mut text_layout = ctx
            .text()
            .new_text_layout(full_text.clone())
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(
                config
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
                        config.get_color_unchecked(LapceTheme::EDITOR_DIM).clone(),
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
        let y = line_height * line as f64
            + (line_height - text_layout.size().height) / 2.0;
        let point = Point::new(x, y);
        ctx.draw_text(&text_layout, point);
    }

    fn file_paint_items(
        path: &Path,
        indices: &[usize],
    ) -> (Option<Svg>, String, Vec<usize>, String, Vec<usize>) {
        let svg = file_svg(path);
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
        (Some(svg), file_name, text_indices, folder, hint_indices)
    }
}

impl Default for PaletteContent {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<PaletteViewData> for PaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(_mouse_event) => {
                ctx.set_cursor(&druid::Cursor::Pointer);
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                let line = (mouse_event.pos.y / self.line_height).floor() as usize;
                self.mouse_down = line;
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                let line = (mouse_event.pos.y / self.line_height).floor() as usize;
                if line == self.mouse_down {
                    let palette = Arc::make_mut(&mut data.palette);
                    palette.index = line;
                    data.select(ctx);
                }
                ctx.set_handled();
            }
            _ => (),
        }
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
        let height = self.line_height * data.palette.len() as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, _env: &Env) {
        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let items = data.palette.current_items();

        let start_line = (rect.y0 / self.line_height).floor() as usize;
        let end_line = (rect.y1 / self.line_height).ceil() as usize;

        for line in start_line..end_line {
            if line >= items.len() {
                break;
            }
            if line == data.palette.index {
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, line as f64 * self.line_height))
                        .with_size(Size::new(size.width, self.line_height)),
                    data.config.get_color_unchecked(LapceTheme::PALETTE_CURRENT),
                );
            }

            let item = &items[line];

            Self::paint_palette_item(
                &item.content,
                ctx,
                line,
                &item.indices,
                self.line_height,
                &data.config,
            );
        }
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
