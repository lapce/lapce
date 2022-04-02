use std::{collections::HashMap, sync::Arc, time::Duration};

use druid::{
    kurbo::BezPath,
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    BoxConstraints, Command, Env, Event, EventCtx, ExtEventSink, FontFamily,
    FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, MouseEvent, PaintCtx,
    Point, Rect, RenderContext, Size, Target, TimerToken, UpdateCtx, Vec2, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use inflector::Inflector;
use lapce_data::{
    buffer::{Buffer, BufferContent},
    command::{
        CommandExecuted, LapceCommand, LapceUICommand, LAPCE_NEW_COMMAND,
        LAPCE_UI_COMMAND,
    },
    config::{EditorConfig, LapceConfig, LapceTheme},
    data::{LapceEditorData, LapceTabData},
    keypress::KeyPressFocus,
    proxy::VERSION,
    state::Mode,
};

use crate::{
    editor::view::LapceEditorView,
    keymap::LapceKeymap,
    scroll::{LapcePadding, LapceScrollNew},
    split::LapceSplitNew,
    svg::get_svg,
};

pub enum LapceSettingsKind {
    Core,
    Editor,
}

#[derive(Clone)]
pub struct LapceSettingsPanelData {
    pub shown: bool,
    pub panel_widget_id: WidgetId,

    pub keymap_widget_id: WidgetId,
    pub keymap_view_id: WidgetId,
    pub keymap_split_id: WidgetId,

    pub settings_widget_id: WidgetId,
    pub settings_view_id: WidgetId,
    pub settings_split_id: WidgetId,
}

impl LapceSettingsPanelData {
    pub fn new() -> Self {
        Self {
            shown: false,
            panel_widget_id: WidgetId::next(),
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
            settings_widget_id: WidgetId::next(),
            settings_view_id: WidgetId::next(),
            settings_split_id: WidgetId::next(),
        }
    }
}

impl Default for LapceSettingsPanelData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LapceSettingsPanel {
    widget_id: WidgetId,
    active: usize,
    content_rect: Rect,
    header_rect: Rect,
    switcher_rect: Rect,
    switcher_line_height: f64,
    close_rect: Rect,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettingsPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let children = vec![
            WidgetPod::new(Box::new(LapceSettings::new_split(
                LapceSettingsKind::Core,
                data,
            )) as Box<dyn Widget<_>>),
            WidgetPod::new(Box::new(LapceSettings::new_split(
                LapceSettingsKind::Editor,
                data,
            ))),
            WidgetPod::new(Box::new(LapceKeymap::new_split(data))),
        ];
        Self {
            widget_id: data.settings.panel_widget_id,
            active: 0,
            header_rect: Rect::ZERO,
            content_rect: Rect::ZERO,
            close_rect: Rect::ZERO,
            switcher_rect: Rect::ZERO,
            switcher_line_height: 40.0,
            children,
        }
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &mut LapceTabData,
    ) {
        if self.close_rect.contains(mouse_event.pos)
            || !self.content_rect.contains(mouse_event.pos)
        {
            let settings = Arc::make_mut(&mut data.settings);
            settings.shown = false;
            return;
        }

        ctx.set_handled();
        ctx.request_focus();
        if self.switcher_rect.contains(mouse_event.pos) {
            let index = ((mouse_event.pos.y - self.switcher_rect.y0)
                / self.switcher_line_height)
                .floor() as usize;
            if index < self.children.len() {
                self.active = index;
                ctx.request_layout();
            }
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        self.close_rect.contains(mouse_event.pos)
    }
}

impl Widget<LapceTabData> for LapceSettingsPanel {
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
        if !data.settings.shown {
            return;
        }
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {}
            Event::Command(cmd) if cmd.is(LAPCE_NEW_COMMAND) => {}
            _ => {
                self.children[self.active].event(ctx, event, data, env);
            }
        }
        if ctx.is_handled() {
            return;
        }
        match event {
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                let performed_action = mut_keypress.key_down(
                    ctx,
                    key_event,
                    Arc::make_mut(&mut data.settings),
                    env,
                );
                data.keypress = keypress;
                if performed_action {
                    ctx.set_handled();
                }
            }
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event, data);
            }
            Event::MouseUp(_mouse_event) => {
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::ShowSettings => {
                        ctx.request_focus();
                        self.active = 0;
                    }
                    LapceUICommand::ShowKeybindings => {
                        ctx.request_focus();
                        self.active = 2;
                    }
                    LapceUICommand::Hide => {
                        Arc::make_mut(&mut data.settings).shown = false;
                        if let Some(active) = *data.main_split.active {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(active),
                            ));
                        }
                    }
                    _ => (),
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.settings.shown {
            self.children[self.active].update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let tab_size = bc.max();

        let self_size = Size::new(
            (tab_size.width * 0.8).min(900.0),
            (tab_size.height * 0.8).min(700.0),
        );
        let origin = Point::new(
            tab_size.width / 2.0 - self_size.width / 2.0,
            (tab_size.height / 2.0 - self_size.height / 2.0) / 2.0,
        )
        .round();
        self.content_rect = self_size.to_rect().with_origin(origin).round();
        self.header_rect = Size::new(self_size.width, 50.0)
            .to_rect()
            .with_origin(origin)
            .round();

        let close_size = 26.0;
        self.close_rect = Size::new(close_size, close_size)
            .to_rect()
            .with_origin(
                origin
                    + (
                        self.header_rect.width()
                            - (self.header_rect.height() / 2.0 - close_size / 2.0)
                            - close_size,
                        self.header_rect.height() / 2.0 - close_size / 2.0,
                    ),
            )
            .round();

        self.switcher_rect =
            Size::new(150.0, self_size.height - self.header_rect.height())
                .to_rect()
                .with_origin(origin + (0.0, self.header_rect.height()))
                .round();

        let content_size = Size::new(
            self_size.width - self.switcher_rect.width() - 40.0,
            self_size.height - self.header_rect.height(),
        );
        let content_origin = origin
            + (
                self_size.width - content_size.width - 20.0,
                self_size.height - content_size.height,
            );
        let content_bc = BoxConstraints::tight(content_size);
        let child = &mut self.children[self.active];
        child.layout(ctx, &content_bc, data, env);
        child.set_origin(ctx, data, env, content_origin);

        tab_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.settings.shown {
            let rect = ctx.size().to_rect();
            ctx.fill(
                rect,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW)
                    .clone()
                    .with_alpha(0.5),
            );

            let shadow_width = 5.0;
            ctx.blurred_rect(
                self.content_rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            ctx.fill(
                self.content_rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );

            ctx.with_save(|ctx| {
                ctx.clip(
                    self.switcher_rect.inflate(50.0, 0.0) + Vec2::new(50.0, 0.0),
                );
                ctx.blurred_rect(
                    self.switcher_rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });

            ctx.fill(
                Size::new(self.switcher_rect.width(), self.switcher_line_height)
                    .to_rect()
                    .with_origin(
                        self.switcher_rect.origin()
                            + (0.0, self.active as f64 * self.switcher_line_height),
                    ),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );

            for (i, text) in ["Core Settings", "Editor Settings", "Keybindings"]
                .iter()
                .enumerate()
            {
                let text_layout = ctx
                    .text()
                    .new_text_layout(text.to_string())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                ctx.draw_text(
                    &text_layout,
                    self.switcher_rect.origin()
                        + (
                            20.0,
                            i as f64 * self.switcher_line_height
                                + (self.switcher_line_height / 2.0
                                    - text_size.height / 2.0),
                        ),
                );
            }

            ctx.blurred_rect(
                self.header_rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            let text_layout = ctx
                .text()
                .new_text_layout("Settings".to_string())
                .font(FontFamily::SYSTEM_UI, 16.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            let x = self.header_rect.height() / 2.0 - text_size.height / 2.0;
            let y = self.header_rect.height() / 2.0 - text_size.height / 2.0;
            ctx.draw_text(&text_layout, self.header_rect.origin() + (x, y));

            let version_text_layout = ctx
                .text()
                .new_text_layout(format!("v{VERSION}"))
                .font(FontFamily::SYSTEM_UI, 10.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &version_text_layout,
                self.header_rect.origin() + (x + text_size.width + 10.0, y + 5.0),
            );

            let svg = get_svg("close.svg").unwrap();
            let icon_padding = 4.0;
            ctx.draw_svg(
                &svg,
                self.close_rect.inflate(-icon_padding, -icon_padding),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );

            self.children[self.active].paint(ctx, data, env);
        }
    }
}

pub enum SettingsValue {
    Bool(bool),
}

pub struct LapceSettings {
    widget_id: WidgetId,
    kind: LapceSettingsKind,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettings {
    pub fn new_split(kind: LapceSettingsKind, data: &LapceTabData) -> LapceSplitNew {
        let settings = LapceScrollNew::new(
            Self {
                widget_id: WidgetId::next(),
                kind,
                children: Vec::new(),
            }
            .boxed(),
        );

        let _input = LapceEditorView::new(data.settings.settings_view_id, None)
            .hide_header()
            .hide_gutter()
            .padding((15.0, 15.0));

        let split = LapceSplitNew::new(data.settings.settings_split_id)
            .horizontal()
            //.with_child(input.boxed(), None, 55.0)
            .with_flex_child(settings.boxed(), None, 1.0);

        split
    }

    fn update_children(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        self.children.clear();

        let (kind, fileds, descs, settings) = match self.kind {
            LapceSettingsKind::Core => {
                let settings: HashMap<String, serde_json::Value> =
                    serde_json::from_value(
                        serde_json::to_value(&data.config.lapce).unwrap(),
                    )
                    .unwrap();
                (
                    "lapce".to_string(),
                    LapceConfig::FIELDS.to_vec(),
                    LapceConfig::DESCS.to_vec(),
                    settings,
                )
            }
            LapceSettingsKind::Editor => {
                let settings: HashMap<String, serde_json::Value> =
                    serde_json::from_value(
                        serde_json::to_value(&data.config.editor).unwrap(),
                    )
                    .unwrap();
                (
                    "editor".to_string(),
                    EditorConfig::FIELDS.to_vec(),
                    EditorConfig::DESCS.to_vec(),
                    settings,
                )
            }
        };

        for (i, field) in fileds.into_iter().enumerate() {
            self.children.push(WidgetPod::new(
                LapcePadding::new(
                    (10.0, 10.0),
                    LapceSettingsItem::new(
                        data,
                        kind.clone(),
                        field.to_string(),
                        descs[i].to_string(),
                        settings.get(&field.replace('_', "-")).unwrap().clone(),
                        ctx.get_external_handle(),
                    ),
                )
                .boxed(),
            ))
        }
    }
}

impl Widget<LapceTabData> for LapceSettings {
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
        for child in self.children.iter_mut() {
            child.event(ctx, event, data, env);
        }
        if self.children.is_empty() {
            self.update_children(ctx, data);
            ctx.children_changed();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        if self.children.is_empty() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::InitChildren,
                Target::Widget(self.widget_id),
            ));
        }

        let mut y = 0.0;
        for child in self.children.iter_mut() {
            let size = child.layout(ctx, bc, data, env);
            child.set_origin(ctx, data, env, Point::new(0.0, y));
            y += size.height;
        }

        Size::new(bc.max().width, bc.max().height.max(y))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for child in self.children.iter_mut() {
            child.paint(ctx, data, env);
        }
    }
}

pub struct LapceSettingsItemKeypress {
    input: String,
    cursor: usize,
}

pub struct LapceSettingsItem {
    kind: String,
    name: String,
    desc: String,
    value: serde_json::Value,
    padding: f64,
    checkbox_width: f64,
    input_max_width: f64,
    width: f64,
    cursor: usize,
    input: String,
    value_changed: bool,
    last_idle_timer: TimerToken,

    name_text: Option<PietTextLayout>,
    desc_text: Option<PietTextLayout>,
    value_text: Option<Option<PietTextLayout>>,
    input_rect: Rect,
    input_view_id: Option<WidgetId>,
    input_widget: Option<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettingsItem {
    /// The amount of time to wait for the next key press before storing settings.
    const SAVE_DELAY: Duration = Duration::from_millis(300);

    pub fn new(
        data: &mut LapceTabData,
        kind: String,
        name: String,
        desc: String,
        value: serde_json::Value,
        event_sink: ExtEventSink,
    ) -> Self {
        let input = match &value {
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::String(s) => Some(s.to_string()),
            serde_json::Value::Array(_)
            | serde_json::Value::Object(_)
            | serde_json::Value::Bool(_)
            | serde_json::Value::Null => None,
        };
        let input = input.map(|input| {
            let name = format!("{kind}.{name}");
            let content = BufferContent::Value(name.clone());
            let mut buffer =
                Buffer::new(content.clone(), data.id, event_sink).set_local();
            buffer.load_content(&input);
            data.main_split.value_buffers.insert(name, Arc::new(buffer));
            let editor = LapceEditorData::new(None, None, content, &data.config);
            let view_id = editor.view_id;
            let input = LapceEditorView::new(editor.view_id, None)
                .hide_header()
                .hide_gutter()
                .padding((5.0, 0.0, 50.0, 0.0));
            data.main_split.editors.insert(view_id, Arc::new(editor));
            (view_id, WidgetPod::new(input.boxed()))
        });
        let input_view_id = input.as_ref().map(|i| i.0);
        let input_widget = input.map(|i| i.1);
        Self {
            kind,
            name,
            desc,
            value,
            padding: 10.0,
            width: 0.0,
            checkbox_width: 20.0,
            input_max_width: 500.0,
            cursor: 0,
            input: "".to_string(),
            value_changed: false,
            last_idle_timer: TimerToken::INVALID,

            name_text: None,
            desc_text: None,
            value_text: None,
            input_rect: Rect::ZERO,
            input_view_id,
            input_widget,
        }
    }

    pub fn name(
        &mut self,
        text: &mut PietText,
        data: &LapceTabData,
    ) -> &PietTextLayout {
        if self.name_text.is_none() {
            let text_layout = text
                .new_text_layout(self.name.to_title_case())
                .font(FontFamily::SYSTEM_UI, 14.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
                .max_width(self.width)
                .build()
                .unwrap();
            self.name_text = Some(text_layout);
        }

        self.name_text.as_ref().unwrap()
    }

    pub fn desc(
        &mut self,
        text: &mut PietText,
        data: &LapceTabData,
    ) -> &PietTextLayout {
        if self.desc_text.is_none() {
            let max_width = if self.value.is_boolean() {
                self.width - self.checkbox_width
            } else {
                self.width
            };
            let text_layout = text
                .new_text_layout(self.desc.clone())
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .max_width(max_width)
                .build()
                .unwrap();
            self.desc_text = Some(text_layout);
        }

        self.desc_text.as_ref().unwrap()
    }

    pub fn value(
        &mut self,
        text: &mut PietText,
        data: &LapceTabData,
    ) -> Option<&PietTextLayout> {
        if self.value_text.is_none() {
            let value = match &self.value {
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::String(s) => Some(s.to_string()),
                serde_json::Value::Array(_)
                | serde_json::Value::Object(_)
                | serde_json::Value::Bool(_)
                | serde_json::Value::Null => None,
            };
            let text_layout = value.map(|value| {
                self.input = value.to_string();
                text.new_text_layout(value)
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap()
            });
            self.value_text = Some(text_layout);
        }

        self.value_text.as_ref().unwrap().as_ref()
    }

    fn get_key(&self) -> String {
        format!("{}.{}", self.kind, self.name.to_kebab_case())
    }
}

impl KeyPressFocus for LapceSettingsItemKeypress {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, _condition: &str) -> bool {
        false
    }

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        match command {
            LapceCommand::Right => {
                self.cursor += 1;
                if self.cursor > self.input.len() {
                    self.cursor = self.input.len();
                }
            }
            LapceCommand::Left => {
                if self.cursor == 0 {
                    return CommandExecuted::Yes;
                }
                self.cursor -= 1;
            }
            LapceCommand::DeleteBackward => {
                if self.cursor == 0 {
                    return CommandExecuted::Yes;
                }
                self.input.remove(self.cursor - 1);
                self.cursor -= 1;
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, c: &str) {
        self.input.insert_str(self.cursor, c);
        self.cursor += c.len();
    }
}

impl Widget<LapceTabData> for LapceSettingsItem {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if let Some(input) = self.input_widget.as_mut() {
            match event {
                Event::Wheel(_) => {}
                _ => {
                    input.event(ctx, event, data, env);
                }
            }
        }
        match event {
            Event::MouseDown(mouse_event) => {
                // ctx.request_focus();
                let input = self.input.clone();
                if let Some(_text) = self.value(ctx.text(), data) {
                    let text = ctx
                        .text()
                        .new_text_layout(input)
                        .font(FontFamily::SYSTEM_UI, 13.0)
                        .text_color(
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                                .clone(),
                        )
                        .build()
                        .unwrap();
                    let mut height = self.name(ctx.text(), data).size().height;
                    height += self.desc(ctx.text(), data).size().height;
                    height += self.padding * 2.0 * 2.0 + self.padding;

                    let rect = Size::new(
                        ctx.size().width.min(self.input_max_width),
                        text.size().height,
                    )
                    .to_rect()
                    .with_origin(Point::new(0.0, height))
                    .inflate(0.0, 8.0);
                    if rect.contains(mouse_event.pos) {
                        let pos = mouse_event.pos - (8.0, 0.0);
                        let hit = text.hit_test_point(pos);
                        self.cursor = hit.idx;
                    }
                } else if let serde_json::Value::Bool(checked) = self.value {
                    let rect = Size::new(self.checkbox_width, self.checkbox_width)
                        .to_rect()
                        .with_origin(Point::new(
                            0.0,
                            self.name(ctx.text(), data).size().height
                                + self.padding * 3.0,
                        ));
                    if rect.contains(mouse_event.pos) {
                        self.value = serde_json::json!(!checked);
                        self.value_changed = true;
                        self.last_idle_timer = ctx.request_timer(Self::SAVE_DELAY);
                    }
                }
            }
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                if self.input_rect.contains(mouse_event.pos) {
                    ctx.set_cursor(&druid::Cursor::IBeam);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::Timer(token)
                if self.value_changed && *token == self.last_idle_timer =>
            {
                self.value_changed = false;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSettingsFile(
                        self.get_key(),
                        self.value.clone(),
                    ),
                    Target::Widget(data.id),
                ));
            }

            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        match event {
            LifeCycle::FocusChanged(false) if self.value_changed => {
                self.value_changed = false;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSettingsFile(
                        self.get_key(),
                        self.value.clone(),
                    ),
                    Target::Widget(data.id),
                ));
            }
            _ => (),
        };

        if let Some(input) = self.input_widget.as_mut() {
            input.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if let Some(view_id) = self.input_view_id.as_ref() {
            let editor = data.main_split.editors.get(view_id).unwrap();
            if let BufferContent::Value(name) = &editor.content {
                let buffer = data.main_split.value_buffers.get(name).unwrap();
                let old_buffer =
                    old_data.main_split.value_buffers.get(name).unwrap();
                if buffer.len() != old_buffer.len()
                    || buffer.rope().slice_to_cow(..)
                        != old_buffer.rope().slice_to_cow(..)
                {
                    let new_value = match &self.value {
                        serde_json::Value::Number(_n) => {
                            if let Ok(new_n) =
                                buffer.rope().slice_to_cow(..).parse::<i64>()
                            {
                                serde_json::json!(new_n)
                            } else {
                                return;
                            }
                        }
                        serde_json::Value::String(_s) => {
                            serde_json::json!(&buffer.rope().slice_to_cow(..))
                        }
                        _ => return,
                    };

                    self.value = new_value;
                    self.value_changed = true;
                    self.last_idle_timer = ctx.request_timer(Self::SAVE_DELAY);
                }
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.width = bc.max().width;
        let text = ctx.text();
        let name = self.name(text, data).size();
        let desc = self.desc(text, data).size();
        let mut height = name.height + desc.height + (self.padding * 2.0 * 2.0);
        height = height.round();

        if let Some(input) = self.input_widget.as_mut() {
            input.layout(ctx, bc, data, env);
            input.set_origin(ctx, data, env, Point::new(0.0, height));
        }

        let text = ctx.text();
        let value = self
            .value(text, data)
            .map(|v| v.size().height)
            .unwrap_or(0.0);
        if value > 0.0 {
            height += value + self.padding * 2.0;
        }
        Size::new(self.width, height.ceil())
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let mut y = 0.0;
        let padding = self.padding;

        let text = ctx.text();
        let text = self.name(text, data);
        text.set_color(
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
        );
        y += padding;
        ctx.draw_text(text, Point::new(0.0, y));
        y += text.size().height + padding;

        y += padding;
        let x = if let serde_json::Value::Bool(checked) = self.value {
            let width = 13.0;
            let height = 13.0;
            let origin = Point::new(0.0, y);
            let rect = Size::new(width, height).to_rect().with_origin(origin);
            ctx.stroke(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                1.0,
            );
            if checked {
                let mut path = BezPath::new();
                path.move_to((origin.x + 3.0, origin.y + 7.0));
                path.line_to((origin.x + 6.0, origin.y + 9.5));
                path.line_to((origin.x + 10.0, origin.y + 3.0));
                ctx.stroke(
                    path,
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                    2.0,
                );
            }

            self.checkbox_width
        } else {
            0.0
        };
        let text = ctx.text();
        let text = self.desc(text, data);
        text.set_color(
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
        );
        ctx.draw_text(text, Point::new(x, y));

        if let Some(input) = self.input_widget.as_mut() {
            input.paint(ctx, data, env);
        }
    }
}
