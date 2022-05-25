use std::{collections::HashMap, sync::Arc, time::Duration};

use druid::{
    kurbo::{BezPath, Line},
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    BoxConstraints, Command, Env, Event, EventCtx, ExtEventSink, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, MouseEvent, PaintCtx, Point,
    Rect, RenderContext, Size, Target, TimerToken, UpdateCtx, Widget, WidgetExt,
    WidgetId, WidgetPod,
};
use inflector::Inflector;
use itertools::Itertools;
use lapce_core::{
    command::{EditCommand, MoveCommand},
    mode::Mode,
};
use lapce_data::{
    command::{
        CommandExecuted, CommandKind, LapceUICommand, LAPCE_COMMAND,
        LAPCE_UI_COMMAND,
    },
    config::{EditorConfig, LapceConfig, LapceTheme, TerminalConfig, UIConfig},
    data::{LapceEditorData, LapceTabData},
    document::{BufferContent, Document},
    keypress::KeyPressFocus,
    settings::LapceSettingsFocusData,
};
use xi_rope::Rope;

use crate::{
    editor::view::LapceEditorView,
    keymap::LapceKeymap,
    scroll::{LapcePadding, LapceScrollNew},
    split::LapceSplitNew,
};

enum LapceSettingsKind {
    Core,
    UI,
    Editor,
    Terminal,
}

pub struct LapceSettingsPanel {
    widget_id: WidgetId,
    editor_tab_id: WidgetId,
    active: usize,
    content_rect: Rect,
    switcher_rect: Rect,
    switcher_line_height: f64,
    children: Vec<WidgetPod<LapceTabData, LapceSplitNew>>,
}

impl LapceSettingsPanel {
    pub fn new(
        data: &LapceTabData,
        widget_id: WidgetId,
        editor_tab_id: WidgetId,
    ) -> Self {
        let children = vec![
            WidgetPod::new(LapceSettings::new_split(LapceSettingsKind::Core, data)),
            WidgetPod::new(LapceSettings::new_split(LapceSettingsKind::UI, data)),
            WidgetPod::new(LapceSettings::new_split(
                LapceSettingsKind::Editor,
                data,
            )),
            WidgetPod::new(LapceSettings::new_split(
                LapceSettingsKind::Terminal,
                data,
            )),
            WidgetPod::new(ThemeSettings::new()),
            WidgetPod::new(LapceKeymap::new_split(data)),
        ];
        Self {
            widget_id,
            editor_tab_id,
            active: 0,
            content_rect: Rect::ZERO,
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
        if self.switcher_rect.contains(mouse_event.pos) {
            let index = ((mouse_event.pos.y - self.switcher_rect.y0)
                / self.switcher_line_height)
                .floor() as usize;
            if index < self.children.len() {
                self.active = index;
                ctx.request_layout();
            }
            ctx.set_handled();
            self.request_focus(ctx, data);
        }
    }

    fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        data.main_split.active_tab = Arc::new(Some(self.editor_tab_id));
        ctx.request_focus();
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
        match event {
            Event::KeyDown(key_event) => {
                if ctx.is_focused() {
                    let mut keypress = data.keypress.clone();
                    let mut focus = LapceSettingsFocusData {
                        widget_id: self.widget_id,
                        editor_tab_id: self.editor_tab_id,
                        main_split: data.main_split.clone(),
                    };
                    let mut_keypress = Arc::make_mut(&mut keypress);
                    let performed_action =
                        mut_keypress.key_down(ctx, key_event, &mut focus, env);
                    data.keypress = keypress;
                    data.main_split = focus.main_split;
                    if performed_action {
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event, data);
            }
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                let mut focus = LapceSettingsFocusData {
                    widget_id: self.widget_id,
                    editor_tab_id: self.editor_tab_id,
                    main_split: data.main_split.clone(),
                };
                if focus.run_command(ctx, cmd, None, Modifiers::empty(), env)
                    == CommandExecuted::Yes
                {
                    ctx.set_handled();
                }
                data.main_split = focus.main_split;
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::Focus => {
                        ctx.set_handled();
                        self.request_focus(ctx, data);
                    }
                    LapceUICommand::ShowSettings => {
                        ctx.request_focus();
                        self.active = 0;
                    }
                    LapceUICommand::ShowKeybindings => {
                        ctx.request_focus();
                        self.active = 5;
                    }
                    LapceUICommand::Hide => {
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

        if ctx.is_handled() {
            return;
        }

        if event.should_propagate_to_hidden() {
            for child in self.children.iter_mut() {
                child.event(ctx, event, data, env);
            }
        } else {
            self.children[self.active].event(ctx, event, data, env);
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
        let self_size = bc.max();
        let origin = Point::ZERO;
        self.content_rect = self_size.to_rect().with_origin(origin).round();

        self.switcher_rect = Size::new(150.0, self_size.height)
            .to_rect()
            .with_origin(Point::ZERO)
            .round();

        let content_size = Size::new(
            self_size.width - self.switcher_rect.width() - 20.0,
            self_size.height,
        );
        let content_origin = Point::new(self.switcher_rect.width() + 20.0, 0.0);
        let content_bc = BoxConstraints::tight(content_size);
        let child = &mut self.children[self.active];
        child.layout(ctx, &content_bc, data, env);
        child.set_origin(ctx, data, env, content_origin);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        ctx.fill(
            self.content_rect,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        ctx.fill(
            Size::new(self.switcher_rect.width(), self.switcher_line_height)
                .to_rect()
                .with_origin(
                    self.switcher_rect.origin()
                        + (0.0, self.active as f64 * self.switcher_line_height),
                ),
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
        );

        const SETTINGS_SECTIONS: [&str; 6] = [
            "Core Settings",
            "UI Settings",
            "Editor Settings",
            "Terminal Settings",
            "Theme Settings",
            "Keybindings",
        ];

        for (i, text) in SETTINGS_SECTIONS.into_iter().enumerate() {
            let text_layout = ctx
                .text()
                .new_text_layout(text)
                .font(
                    data.config.ui.font_family(),
                    (data.config.ui.font_size() + 1) as f64,
                )
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

        self.children[self.active].paint(ctx, data, env);

        ctx.stroke(
            Line::new(
                Point::new(self.switcher_rect.x1 + 0.5, self.switcher_rect.y0),
                Point::new(self.switcher_rect.x1 + 0.5, self.switcher_rect.y1),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
    }
}

struct LapceSettings {
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

        let _input = LapceEditorView::new(
            data.settings.settings_view_id,
            WidgetId::next(),
            None,
        )
        .hide_header()
        .hide_gutter()
        .padding((15.0, 15.0, 0.0, 15.0));

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
            LapceSettingsKind::UI => {
                let settings: HashMap<String, serde_json::Value> =
                    serde_json::from_value(
                        serde_json::to_value(&data.config.ui).unwrap(),
                    )
                    .unwrap();
                (
                    "ui".to_string(),
                    UIConfig::FIELDS.to_vec(),
                    UIConfig::DESCS.to_vec(),
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
            LapceSettingsKind::Terminal => {
                let settings: HashMap<String, serde_json::Value> =
                    serde_json::from_value(
                        serde_json::to_value(&data.config.terminal).unwrap(),
                    )
                    .unwrap();
                (
                    "terminal".to_string(),
                    TerminalConfig::FIELDS.to_vec(),
                    TerminalConfig::DESCS.to_vec(),
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

struct LapceSettingsItemKeypress {
    input: String,
    cursor: usize,
}

struct LapceSettingsItem {
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

            let mut doc = Document::new(
                content.clone(),
                data.id,
                event_sink,
                data.proxy.clone(),
            );
            doc.reload(Rope::from(&input), true);
            data.main_split.value_docs.insert(name, Arc::new(doc));
            let editor =
                LapceEditorData::new(None, None, None, content, &data.config);
            let view_id = editor.view_id;
            let input = LapceEditorView::new(editor.view_id, editor.editor_id, None)
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
                .font(
                    data.config.ui.font_family(),
                    (data.config.ui.font_size() + 1) as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
                .max_width(self.width - 30.0)
                .set_line_height(1.5)
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
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .max_width(max_width - 30.0)
                .set_line_height(1.5)
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
            });
            self.value_text = Some(text_layout);
        }

        self.value_text.as_ref().unwrap().as_ref()
    }

    fn get_key(&self) -> String {
        format!("{}.{}", self.kind, self.name.to_kebab_case())
    }

    fn clear_text_layout_cache(&mut self) {
        self.name_text = None;
        self.desc_text = None;
        self.value_text = None;
    }
}

impl KeyPressFocus for LapceSettingsItemKeypress {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, _condition: &str) -> bool {
        false
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, c: &str) {
        self.input.insert_str(self.cursor, c);
        self.cursor += c.len();
    }

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        command: &lapce_data::command::LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Move(cmd) => match cmd {
                MoveCommand::Right => {
                    self.cursor += 1;
                    if self.cursor > self.input.len() {
                        self.cursor = self.input.len();
                    }
                }
                MoveCommand::Left => {
                    if self.cursor == 0 {
                        return CommandExecuted::Yes;
                    }
                    self.cursor -= 1;
                }
                _ => return CommandExecuted::No,
            },
            CommandKind::Edit(EditCommand::DeleteForward) => {
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
                    let mut height = self.name(ctx.text(), data).size().height;
                    height += self.desc(ctx.text(), data).size().height;
                    height += self.padding * 2.0 + self.padding;

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
                                + self.padding * 2.0
                                + 4.0,
                        ));
                    if rect.contains(mouse_event.pos) {
                        self.value = serde_json::json!(!checked);
                        self.value_changed = true;
                        self.last_idle_timer =
                            ctx.request_timer(Self::SAVE_DELAY, None);
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
        env: &Env,
    ) {
        if data.config.id != old_data.config.id {
            self.clear_text_layout_cache();
        }
        if let Some(view_id) = self.input_view_id.as_ref() {
            let editor = data.main_split.editors.get(view_id).unwrap();
            if let BufferContent::Value(name) = &editor.content {
                let doc = data.main_split.value_docs.get(name).unwrap();
                let old_doc = old_data.main_split.value_docs.get(name).unwrap();
                if doc.buffer().len() != old_doc.buffer().len()
                    || doc.buffer().text().slice_to_cow(..)
                        != old_doc.buffer().text().slice_to_cow(..)
                {
                    let new_value = match &self.value {
                        serde_json::Value::Number(_n) => {
                            if let Ok(new_n) =
                                doc.buffer().text().slice_to_cow(..).parse::<i64>()
                            {
                                serde_json::json!(new_n)
                            } else {
                                return;
                            }
                        }
                        serde_json::Value::String(_s) => {
                            serde_json::json!(&doc.buffer().text().slice_to_cow(..))
                        }
                        _ => return,
                    };

                    self.value = new_value;
                    self.value_changed = true;
                    self.last_idle_timer = ctx.request_timer(Self::SAVE_DELAY, None);
                }
            }
        }
        if let Some(input) = self.input_widget.as_mut() {
            input.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = bc.max().width;
        if width != self.width {
            self.width = width;
            self.clear_text_layout_cache();
        }
        let text = ctx.text();
        let name = self.name(text, data).size();
        let desc = self.desc(text, data).size();
        let mut height = name.height + desc.height + (self.padding * 3.0);
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

        let rect = ctx
            .size()
            .to_rect()
            .inflate(0.0, padding)
            .inset((padding, 0.0, -30.0, 0.0));
        if ctx.is_hot() {
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
            );
        }

        let text = ctx.text();
        let text = self.name(text, data);
        y += padding;
        ctx.draw_text(text, Point::new(0.0, y));
        y += text.size().height;

        y += padding;
        let x = if let serde_json::Value::Bool(checked) = self.value {
            let width = 13.0;
            let height = 13.0;
            let origin = Point::new(0.0, y + 4.0);
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
        ctx.draw_text(text, Point::new(x, y));

        if let Some(input) = self.input_widget.as_mut() {
            input.paint(ctx, data, env);
        }
    }
}

pub struct ThemeSettings {
    widget_id: WidgetId,
    inputs: Vec<(
        PietTextLayout,
        WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    )>,
}

impl ThemeSettings {
    fn new() -> LapceSplitNew {
        let settings = LapceScrollNew::new(
            Self {
                widget_id: WidgetId::next(),
                inputs: Vec::new(),
            }
            .boxed(),
        );

        let split = LapceSplitNew::new(WidgetId::next())
            .horizontal()
            .with_flex_child(settings.boxed(), None, 1.0);

        split
    }

    fn update_inputs(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        self.inputs.clear();

        let colors: Vec<&String> = data
            .config
            .themes
            .default_theme()
            .color_keys()
            .sorted()
            .collect();
        let style_colors: Vec<&String> =
            data.config.themes.default_theme().style_keys().collect();

        for color in colors {
            let name = format!("lapce.color.{color}");
            let content = BufferContent::Value(name.clone());
            let mut doc = Document::new(
                content.clone(),
                data.id,
                ctx.get_external_handle(),
                data.proxy.clone(),
            );
            doc.reload(
                Rope::from(format!(
                    "{:?}",
                    data.config.themes.color(color).unwrap()
                )),
                true,
            );
            data.main_split.value_docs.insert(name, Arc::new(doc));
            let editor =
                LapceEditorData::new(None, None, None, content, &data.config);
            let view_id = editor.view_id;
            let input = LapceEditorView::new(editor.view_id, editor.editor_id, None)
                .hide_header()
                .hide_gutter()
                .padding((5.0, 0.0, 50.0, 0.0));
            data.main_split.editors.insert(view_id, Arc::new(editor));
            let text_layout = ctx
                .text()
                .new_text_layout(color.clone())
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
            self.inputs
                .push((text_layout, WidgetPod::new(input.boxed())));
        }
    }
}

impl Widget<LapceTabData> for ThemeSettings {
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
        for (_, input) in self.inputs.iter_mut() {
            match event {
                Event::Wheel(_) => {}
                _ => {
                    input.event(ctx, event, data, env);
                }
            }
        }

        if self.inputs.is_empty() {
            self.update_inputs(ctx, data);
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
        for (_, input) in self.inputs.iter_mut() {
            input.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        for (_, input) in self.inputs.iter_mut() {
            input.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        if self.inputs.is_empty() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::InitChildren,
                Target::Widget(self.widget_id),
            ));
        }

        let text_width = self
            .inputs
            .iter()
            .map(|(text_layout, _)| text_layout.size().width.ceil() as usize)
            .max()
            .unwrap_or(0) as f64;

        let mut y = 0.0;
        let input_bc = BoxConstraints::tight(Size::new(
            bc.max().width - text_width,
            bc.max().height,
        ));
        for (_, input) in self.inputs.iter_mut() {
            let size = input.layout(ctx, &input_bc, data, env);
            input.set_origin(ctx, data, env, Point::new(text_width, y));
            y += size.height;
        }

        Size::new(bc.max().width, bc.max().height.max(y))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        for (text_layout, input) in self.inputs.iter_mut() {
            ctx.draw_text(&text_layout, Point::new(0.0, input.layout_rect().y0));
            input.paint(ctx, data, env);
        }
    }
}
