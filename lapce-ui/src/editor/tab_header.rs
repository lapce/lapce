use std::time::Duration;

use druid::{
    kurbo::Line,
    piet::{Text, TextAttribute, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    TimerToken, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_core::{command::FocusCommand, meta};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{EditorTabChild, LapceTabData},
    document::BufferContent,
};

use crate::{
    editor::tab_header_content::LapceEditorTabHeaderContent, scroll::LapceScroll,
    tab::LapceIcon,
};

pub struct LapceEditorTabHeader {
    pub widget_id: WidgetId,
    pub content: WidgetPod<
        LapceTabData,
        LapceScroll<LapceTabData, LapceEditorTabHeaderContent>,
    >,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
    hover_rect: Option<Rect>,
    is_hot: bool,
}

impl LapceEditorTabHeader {
    pub fn new(widget_id: WidgetId) -> Self {
        let content = LapceScroll::new(LapceEditorTabHeaderContent::new(widget_id))
            .horizontal()
            .hide_bar()
            .vertical_scroll_for_horizontal();
        Self {
            widget_id,
            content: WidgetPod::new(content),
            icons: Vec::new(),
            mouse_pos: Point::ZERO,
            is_hot: false,
            hover_rect: None,
        }
    }

    fn icon_hit_test(&mut self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                self.hover_rect = Some(icon.rect);
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }

    fn ensure_active_visible<F>(
        &mut self,
        data: &LapceTabData,
        request_timer: F,
        env: &Env,
    ) where
        F: FnOnce(Duration) -> TimerToken,
    {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let active = editor_tab.active;
        if active < self.content.widget().child().rects.len() {
            let rect = self.content.widget().child().rects[active].rect;
            if self.content.widget_mut().scroll_to_visible(rect, env) {
                self.content
                    .widget_mut()
                    .reset_scrollbar_fade(request_timer, env);
            }
        }
    }

    fn paint_header(&self, ctx: &mut PaintCtx, data: &LapceTabData) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let child = editor_tab.active_child();
        let mut text = "".to_string();
        let mut hint = "".to_string();
        let mut svg = data.config.ui_svg(LapceIcons::FILE);
        let mut svg_color = Some(
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
        );
        if let Some(child) = child {
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor_buffer = data.editor_view_content(*view_id);

                    if let BufferContent::File(path) = &editor_buffer.editor.content
                    {
                        (svg, svg_color) = data.config.file_svg(path);
                        if let Some(file_name) = path.file_name() {
                            if let Some(s) = file_name.to_str() {
                                text = s.to_string();
                            }
                        }
                        let mut path = path.to_path_buf();
                        if let Some(workspace_path) = data.workspace.path.as_ref() {
                            path = path
                                .strip_prefix(workspace_path)
                                .unwrap_or(&path)
                                .to_path_buf();
                        }
                        hint = path
                            .parent()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                    } else if let BufferContent::Scratch(..) =
                        &editor_buffer.editor.content
                    {
                        text = editor_buffer.editor.content.file_name().to_string();
                    }
                    if !editor_buffer.doc.buffer().is_pristine() {
                        text = format!("*{text}");
                    }
                    if let Some(_compare) = editor_buffer.editor.compare.as_ref() {
                        text = format!("{text} (Working tree)");
                    }
                }
                EditorTabChild::Settings { .. } => {
                    text = "Settings".to_string();
                    hint = format!("ver. {}", *meta::VERSION);
                    svg = data.config.ui_svg(LapceIcons::SETTINGS);
                }
                EditorTabChild::Plugin { volt_name, .. } => {
                    text = format!("Plugin: {volt_name}");
                    svg = data.config.ui_svg(LapceIcons::EXTENSIONS);
                }
            }
        }
        let font_size = data.config.ui.font_size() as f64;

        let svg_size = data.config.ui.icon_size() as f64;
        let size = ctx.size();
        let svg_rect =
            Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(
                    (size.height - svg_size) / 2.0,
                    (size.height - svg_size) / 2.0,
                ));
        ctx.draw_svg(&svg, svg_rect, svg_color);

        if !hint.is_empty() {
            text = format!("{text} {hint}");
        }
        let total_len = text.len();
        let mut text_layout = ctx
            .text()
            .new_text_layout(text)
            .font(data.config.ui.font_family(), font_size)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );
        if !hint.is_empty() {
            text_layout = text_layout.range_attribute(
                total_len - hint.len()..total_len,
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
            Point::new(svg_rect.x1 + 5.0, text_layout.y_offset(size.height)),
        );
    }
}

impl Widget<LapceTabData> for LapceEditorTabHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                let hover_rect = self.hover_rect;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    self.hover_rect = None;
                    ctx.clear_cursor();
                }
                if hover_rect != self.hover_rect {
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            _ => (),
        }
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::HotChanged(is_hot) = event {
            self.is_hot = *is_hot;
            ctx.request_layout();
        }
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        let editor_tab = data.main_split.editor_tabs.get(&self.widget_id).unwrap();
        let old_editor_tab = old_data
            .main_split
            .editor_tabs
            .get(&self.widget_id)
            .unwrap();
        if editor_tab.active != old_editor_tab.active {
            let scroll_id = self.content.id();
            self.ensure_active_visible(
                data,
                |d| ctx.request_timer(d, Some(scroll_id)),
                env,
            );
        }
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.icons.clear();

        let header_height = data.config.ui.header_height() as f64;
        let size = Size::new(bc.max().width, data.config.ui.header_height() as f64);

        let icon_size = 24.0;
        let gap = (header_height - icon_size) / 2.0;
        let x = size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::CLOSE,
            rect: Size::new(icon_size, icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitClose,
                Target::Widget(self.widget_id),
            ),
        };
        self.icons.push(icon);

        let x = size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::SPLIT_HORIZONTAL,
            rect: Size::new(icon_size, icon_size)
                .to_rect()
                .with_origin(Point::new(x + gap, gap)),
            command: Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::SplitVertical),
                    data: None,
                },
                Target::Widget(self.widget_id),
            ),
        };
        self.icons.push(icon);

        if data.config.editor.show_tab {
            let icon = LapceIcon {
                icon: LapceIcons::TAB_PREVIOUS,
                rect: Size::new(icon_size, icon_size)
                    .to_rect()
                    .with_origin(Point::new(gap, gap)),
                command: Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PreviousEditorTab,
                    Target::Widget(self.widget_id),
                ),
            };
            self.icons.push(icon);

            let icon = LapceIcon {
                icon: LapceIcons::TAB_NEXT,
                rect: Size::new(icon_size, icon_size)
                    .to_rect()
                    .with_origin(Point::new(gap + icon_size, gap)),
                command: Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NextEditorTab,
                    Target::Widget(self.widget_id),
                ),
            };
            self.icons.push(icon);
        }

        self.content.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                size.width - self.icons.len() as f64 * (gap + icon_size),
                size.height,
            )),
            data,
            env,
        );
        self.content.set_origin(
            ctx,
            data,
            env,
            Point::new(
                if data.config.editor.show_tab {
                    2.0 * (gap + icon_size)
                } else {
                    0.0
                },
                0.0,
            ),
        );
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        ctx.fill(
            size.to_rect(),
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_TAB_INACTIVE_BACKGROUND),
        );

        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.with_save(|ctx| {
                ctx.clip(rect.inset((0.0, 0.0, 0.0, 50.0)));
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });
        } else {
            ctx.stroke(
                Line::new(
                    Point::new(rect.x0, rect.y1 + 0.5),
                    Point::new(rect.x1, rect.y1 + 0.5),
                ),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
        if data.config.editor.show_tab {
            self.content.paint(ctx, data, env);
            let scroll_offset = self.content.widget().offset().x;
            let content_rect = self.content.layout_rect();
            let child_size = self.content.widget().child_size();
            if scroll_offset > 0.0 {
                ctx.with_save(|ctx| {
                    ctx.clip(content_rect);
                    let rect = Rect::new(
                        content_rect.x0 - 10.0,
                        content_rect.y0 - 10.0,
                        content_rect.x0,
                        content_rect.y1 + 10.0,
                    );
                    ctx.blurred_rect(
                        rect,
                        4.0,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                });
            }
            if scroll_offset < child_size.width - content_rect.width() {
                ctx.with_save(|ctx| {
                    ctx.clip(content_rect);
                    let rect = Rect::new(
                        content_rect.x1,
                        content_rect.y0 - 10.0,
                        content_rect.x1 + 10.0,
                        content_rect.y1 + 10.0,
                    );
                    ctx.blurred_rect(
                        rect,
                        4.0,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                });
            }
        } else {
            self.paint_header(ctx, data);
        }

        let svg_padding = 4.0;
        for icon in self.icons.iter() {
            if icon.rect.contains(self.mouse_pos) {
                ctx.fill(
                    icon.rect,
                    &data.config.get_hover_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                    ),
                );
            }
            {
                let svg = data.config.ui_svg(icon.icon);
                ctx.draw_svg(
                    &svg,
                    icon.rect.inflate(-svg_padding, -svg_padding),
                    Some(
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                    ),
                );
            }
        }
    }
}
