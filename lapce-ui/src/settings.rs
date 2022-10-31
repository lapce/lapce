use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use druid::{
    kurbo::{BezPath, Line},
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    widget::Padding,
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, ExtEventSink, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx, Point, Rect,
    RenderContext, Size, Target, TimerToken, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod,
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
    config::{CoreConfig, EditorConfig, LapceTheme, TerminalConfig, UIConfig},
    data::{FocusArea, LapceEditorData, LapceTabData},
    document::{BufferContent, Document},
    keypress::KeyPressFocus,
    settings::{LapceSettingsFocusData, LapceSettingsKind, SettingsValueKind},
};
use serde::Serialize;
use xi_rope::Rope;

use crate::{
    editor::view::LapceEditorView,
    keymap::LapceKeymap,
    scroll::{LapcePadding, LapceScroll},
    split::LapceSplit,
};

pub struct LapceSettingsPanel {
    widget_id: WidgetId,
    editor_tab_id: WidgetId,
    active: LapceSettingsKind,
    content_rect: Rect,
    switcher_rect: Rect,
    switcher: WidgetPod<LapceTabData, LapceScroll<LapceTabData, SettingsSwitcher>>,
    children: HashMap<
        LapceSettingsKind,
        WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    >,
}

impl LapceSettingsPanel {
    pub fn new(
        data: &LapceTabData,
        widget_id: WidgetId,
        editor_tab_id: WidgetId,
        keymap_input_view_id: WidgetId,
    ) -> Self {
        let mut children = HashMap::new();
        children.insert(
            LapceSettingsKind::Core,
            WidgetPod::new(
                LapceSettings::new_scroll(LapceSettingsKind::Core).boxed(),
            ),
        );
        children.insert(
            LapceSettingsKind::UI,
            WidgetPod::new(LapceSettings::new_scroll(LapceSettingsKind::UI).boxed()),
        );
        children.insert(
            LapceSettingsKind::Editor,
            WidgetPod::new(
                LapceSettings::new_scroll(LapceSettingsKind::Editor).boxed(),
            ),
        );
        children.insert(
            LapceSettingsKind::Terminal,
            WidgetPod::new(
                LapceSettings::new_scroll(LapceSettingsKind::Terminal).boxed(),
            ),
        );
        children.insert(
            LapceSettingsKind::Theme,
            WidgetPod::new(ThemeSettings::new_boxed()),
        );
        children.insert(
            LapceSettingsKind::Keymap,
            WidgetPod::new(LapceKeymap::new_split(keymap_input_view_id).boxed()),
        );
        for (volt_id, volt) in data.plugin.installed.iter() {
            if volt.config.is_some() {
                children.insert(
                    LapceSettingsKind::Plugin(volt_id.to_string()),
                    WidgetPod::new(
                        LapceSettings::new_scroll(LapceSettingsKind::Plugin(
                            volt_id.to_string(),
                        ))
                        .boxed(),
                    ),
                );
            }
        }

        let switcher = LapceScroll::new(SettingsSwitcher::new(widget_id));

        Self {
            widget_id,
            editor_tab_id,
            content_rect: Rect::ZERO,
            switcher_rect: Rect::ZERO,
            switcher: WidgetPod::new(switcher),
            children,
            active: LapceSettingsKind::Core,
        }
    }

    fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        let editor_tab = data
            .main_split
            .editor_tabs
            .get_mut(&self.editor_tab_id)
            .unwrap();
        let editor_tab = Arc::make_mut(editor_tab);
        if let Some(index) = editor_tab
            .children
            .iter()
            .position(|child| child.widget_id() == self.widget_id)
        {
            editor_tab.active = index;
        }

        data.main_split.active_tab = Arc::new(Some(self.editor_tab_id));
        data.focus = Arc::new(self.widget_id);
        data.focus_area = FocusArea::Editor;
        ctx.request_focus();
    }

    fn update_plugins(&mut self, ctx: &mut EventCtx, data: &LapceTabData) {
        let current_keys: Vec<LapceSettingsKind> =
            self.children.keys().cloned().collect();
        for kind in current_keys {
            if let LapceSettingsKind::Plugin(volt_id) = &kind {
                if !data.plugin.installed.keys().contains(&volt_id) {
                    self.children.remove(&kind);
                    ctx.children_changed();
                    if self.active == kind {
                        self.active = LapceSettingsKind::Core;
                        self.switcher
                            .widget_mut()
                            .child_mut()
                            .set_active(self.active.clone(), data);
                    }
                }
            }
        }
        for (_, volt) in data.plugin.installed.iter() {
            if volt.config.is_some() {
                let kind = LapceSettingsKind::Plugin(volt.id());
                if !self.children.keys().contains(&kind) {
                    self.children.insert(
                        kind.clone(),
                        WidgetPod::new(LapceSettings::new_scroll(kind).boxed()),
                    );
                    ctx.children_changed();
                }
            }
        }
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
                        config: data.config.clone(),
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
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                let mut focus = LapceSettingsFocusData {
                    widget_id: self.widget_id,
                    editor_tab_id: self.editor_tab_id,
                    main_split: data.main_split.clone(),
                    config: data.config.clone(),
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
                        let kind = LapceSettingsKind::Core;
                        self.active = kind.clone();
                        self.switcher
                            .widget_mut()
                            .child_mut()
                            .set_active(kind, data);
                        ctx.request_focus();
                    }
                    LapceUICommand::ShowKeybindings => {
                        let kind = LapceSettingsKind::Core;
                        self.active = kind.clone();
                        self.switcher
                            .widget_mut()
                            .child_mut()
                            .set_active(kind, data);
                        ctx.request_focus();
                    }
                    LapceUICommand::ShowSettingsKind(kind) => {
                        self.active = kind.clone();
                        self.switcher
                            .widget_mut()
                            .child_mut()
                            .set_active(kind.clone(), data);
                        ctx.request_layout();
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
                    LapceUICommand::VoltInstalled(_, _)
                    | LapceUICommand::VoltRemoved(_, _) => {
                        ctx.set_handled();
                        self.update_plugins(ctx, data);
                    }
                    _ => (),
                }
            }
            _ => {}
        }

        if ctx.is_handled() {
            return;
        }

        self.switcher.event(ctx, event, data, env);
        if event.should_propagate_to_hidden() {
            for child in self.children.values_mut() {
                child.event(ctx, event, data, env);
            }
        } else if let Some(child) = self.children.get_mut(&self.active) {
            child.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.switcher.lifecycle(ctx, event, data, env);
        for child in self.children.values_mut() {
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
        self.switcher.update(ctx, data, env);
        for child in self.children.values_mut() {
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

        let switcher_size = self.switcher.layout(
            ctx,
            &BoxConstraints::new(Size::ZERO, bc.max()),
            data,
            env,
        );
        self.switcher.set_origin(ctx, data, env, Point::ZERO);

        self.switcher_rect = Size::new(150.0, self_size.height)
            .to_rect()
            .with_origin(Point::ZERO)
            .round();

        let content_size = Size::new(
            self_size.width - switcher_size.width - 20.0,
            self_size.height,
        );
        let content_origin = Point::new(switcher_size.width + 20.0, 0.0);
        let content_bc = BoxConstraints::tight(content_size);
        if let Some(child) = self.children.get_mut(&self.active) {
            child.layout(ctx, &content_bc, data, env);
            child.set_origin(ctx, data, env, content_origin);
        }

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.switcher.paint(ctx, data, env);
        if let Some(child) = self.children.get_mut(&self.active) {
            child.paint(ctx, data, env);
        }

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
    pub fn new_scroll(
        kind: LapceSettingsKind,
    ) -> LapceScroll<LapceTabData, LapceSettings> {
        LapceScroll::new(Self {
            widget_id: WidgetId::next(),
            kind,
            children: Vec::new(),
        })
    }

    fn update_children(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        fn into_settings_map(
            data: &impl Serialize,
        ) -> HashMap<String, serde_json::Value> {
            serde_json::to_value(data)
                .and_then(serde_json::from_value)
                .unwrap()
        }

        self.children.clear();

        let (kind, fields, descs, mut settings) = match &self.kind {
            LapceSettingsKind::Core => (
                "core",
                &CoreConfig::FIELDS[..],
                &CoreConfig::DESCS[..],
                into_settings_map(&data.config.core),
            ),
            LapceSettingsKind::UI => (
                "ui",
                &UIConfig::FIELDS[..],
                &UIConfig::DESCS[..],
                into_settings_map(&data.config.ui),
            ),
            LapceSettingsKind::Editor => (
                "editor",
                &EditorConfig::FIELDS[..],
                &EditorConfig::DESCS[..],
                into_settings_map(&data.config.editor),
            ),
            LapceSettingsKind::Terminal => (
                "terminal",
                &TerminalConfig::FIELDS[..],
                &TerminalConfig::DESCS[..],
                into_settings_map(&data.config.terminal),
            ),
            LapceSettingsKind::Theme | LapceSettingsKind::Keymap => {
                return;
            }
            LapceSettingsKind::Plugin(volt_id) => {
                if let Some(volt) = data.plugin.installed.get(volt_id).cloned() {
                    if let Some(config) = volt.config.as_ref() {
                        for (key, config) in
                            config.iter().sorted_by_key(|(key, _)| *key)
                        {
                            let mut value = config.default.clone();
                            if let Some(plugin_config) =
                                data.config.plugins.get(&volt.name)
                            {
                                if let Some(v) = plugin_config.get(key) {
                                    value = v.clone();
                                }
                            }
                            self.children.push(create_settings_item(
                                data,
                                volt.name.clone(),
                                key.to_string(),
                                config.description.clone(),
                                value,
                                ctx.get_external_handle(),
                            ));
                        }
                    }
                }
                return;
            }
        };

        for (field, desc) in fields.iter().zip(descs.iter()) {
            // TODO(dbuga): we should generate kebab-case field names
            let field = field.replace('_', "-");
            let value = settings.remove(&field).unwrap();
            self.children.push(create_settings_item(
                data,
                kind.to_string(),
                field,
                desc.to_string(),
                value,
                ctx.get_external_handle(),
            ));
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

/// Create a settings item widget  
/// Includes padding.
fn create_settings_item(
    data: &mut LapceTabData,
    kind: String,
    key: String,
    desc: String,
    value: serde_json::Value,
    event_sink: ExtEventSink,
) -> WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>> {
    let insets = (10.0, 10.0);
    match value {
        serde_json::Value::Number(n) => {
            let value_kind = if n.is_f64() {
                SettingsValueKind::Float
            } else {
                SettingsValueKind::Integer
            };
            WidgetPod::new(
                LapcePadding::new(
                    insets,
                    InputSettingsItem::new(
                        data,
                        kind,
                        key,
                        desc,
                        event_sink,
                        n.to_string(),
                        value_kind,
                    ),
                )
                .boxed(),
            )
        }
        serde_json::Value::String(s) => WidgetPod::new(
            LapcePadding::new(
                insets,
                InputSettingsItem::new(
                    data,
                    kind,
                    key,
                    desc,
                    event_sink,
                    s,
                    SettingsValueKind::String,
                ),
            )
            .boxed(),
        ),
        serde_json::Value::Bool(checked) => WidgetPod::new(
            LapcePadding::new(
                insets,
                CheckBoxSettingsItem::new(key, kind, desc, checked),
            )
            .boxed(),
        ),
        serde_json::Value::Array(_)
        | serde_json::Value::Object(_)
        | serde_json::Value::Null => WidgetPod::new(
            LapcePadding::new(insets, EmptySettingsItem::new(key, kind, desc))
                .boxed(),
        ),
    }
}

/// Shared information between each setting item
struct SettingsItemInfo {
    width: f64,
    padding: f64,

    /// Key of the field
    key: String,
    kind: String,
    desc: String,

    name_text: Option<PietTextLayout>,
    desc_text: Option<PietTextLayout>,

    /// Timer which keeps track of when it was last edited  
    /// So that it can update
    last_idle_timer: TimerToken,
}
impl SettingsItemInfo {
    /// The amount of time to wait for the next key press before storing settings.
    const SAVE_DELAY: Duration = Duration::from_millis(500);

    fn new(key: String, kind: String, desc: String) -> Self {
        SettingsItemInfo {
            width: 0.0,
            padding: 10.0,
            key,
            kind,
            desc,
            name_text: None,
            desc_text: None,
            last_idle_timer: TimerToken::INVALID,
        }
    }

    /// Check if the last-idle-timer has been triggered, and thus it should probably update
    fn idle_timer_triggered(&self, token: TimerToken) -> bool {
        token == self.last_idle_timer
    }

    fn clear_text_layout_cache(&mut self) {
        self.name_text = None;
        self.desc_text = None;
    }

    /// Get the text layout for the name of the setting item, creating it if it has changed
    /// or if it is not already initialize.
    pub fn name(
        &mut self,
        text: &mut PietText,
        data: &LapceTabData,
    ) -> &PietTextLayout {
        // TODO: This could likely use smallvec, or even skip the allocs completely for
        // the *common* case of the name text not changing..
        let splits: Vec<&str> = self.key.rsplitn(2, '.').collect();
        let mut name_text = "".to_string();
        if let Some(title) = splits.get(1) {
            for (i, part) in title.split('.').enumerate() {
                if i > 0 {
                    name_text.push_str(" > ");
                }
                name_text.push_str(&part.to_title_case());
            }
            name_text.push_str(": ");
        }
        if let Some(name) = splits.first() {
            name_text.push_str(&name.to_title_case());
        }

        if self.name_text.is_none() {
            let text_layout = text
                .new_text_layout(name_text)
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

    /// Get the text layout for the description of the setting item, creating it if it doesn't exist.  
    /// `extra_width` is used for when there are other rendered elements on the same line as the description,  
    /// such as the checkbox.
    pub fn desc(
        &mut self,
        text: &mut PietText,
        data: &LapceTabData,
        extra_width: f64,
    ) -> &PietTextLayout {
        if self.desc_text.is_none() {
            let max_width = self.width - extra_width;
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

    fn update_settings(
        &self,
        data: &LapceTabData,
        ctx: &mut EventCtx,
        value: serde_json::Value,
    ) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateSettingsFile(
                self.kind.clone(),
                self.key.clone(),
                value,
            ),
            Target::Widget(data.id),
        ));
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        _env: &Env,
    ) {
        if data.config.id != old_data.config.id {
            self.clear_text_layout_cache();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
        extra_width: f64,
    ) -> Size {
        let width = bc.max().width;
        if width != self.width {
            self.width = width;
            self.clear_text_layout_cache();
        }
        let text = ctx.text();
        let name = self.name(text, data).size();
        let desc = self.desc(text, data, extra_width).size();
        let mut height = name.height + desc.height + (self.padding * 3.0);
        height = height.round();

        Size::new(self.width, height)
    }

    /// Paint the name of the setting and the description  
    /// `extra_width` decides how the description should be shifted to the right  
    /// Returns the y position of the description, so that you can relative to it.
    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &LapceTabData,
        _env: &Env,
        extra_width: f64,
    ) -> f64 {
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

        let desc_y = y;

        let text = ctx.text();
        let desc = self.desc(text, data, extra_width);
        ctx.draw_text(desc, Point::new(extra_width, y));

        desc_y
    }
}

/// An uneditable settings item. Typically this is because it must
/// be edited directly in the `settings.toml` file.
struct EmptySettingsItem {
    info: SettingsItemInfo,
}
impl EmptySettingsItem {
    fn new(key: String, kind: String, desc: String) -> Self {
        EmptySettingsItem {
            info: SettingsItemInfo::new(key, kind, desc),
        }
    }
}
impl Widget<LapceTabData> for EmptySettingsItem {
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
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.info.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.info.layout(ctx, bc, data, env, 0.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.info.paint(ctx, data, env, 0.0);
    }
}

struct CheckBoxSettingsItem {
    checked: bool,
    checkbox_width: f64,

    info: SettingsItemInfo,
}
impl CheckBoxSettingsItem {
    fn new(key: String, kind: String, desc: String, checked: bool) -> Self {
        Self {
            checked,
            checkbox_width: 20.0,

            info: SettingsItemInfo::new(key, kind, desc),
        }
    }
}
impl Widget<LapceTabData> for CheckBoxSettingsItem {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(_) => ctx.set_handled(),
            Event::MouseDown(mouse_event) => {
                let rect = Size::new(self.checkbox_width, self.checkbox_width)
                    .to_rect()
                    .with_origin(Point::new(
                        0.0,
                        self.info.name(ctx.text(), data).size().height
                            + self.info.padding * 2.0
                            + 4.0,
                    ));
                if rect.contains(mouse_event.pos) {
                    self.checked = !self.checked;
                    self.info.last_idle_timer =
                        ctx.request_timer(SettingsItemInfo::SAVE_DELAY, None);
                }
            }
            Event::Timer(token) if self.info.idle_timer_triggered(*token) => {
                ctx.set_handled();
                self.info.update_settings(
                    data,
                    ctx,
                    serde_json::Value::Bool(self.checked),
                );
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.info.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.info.layout(ctx, bc, data, env, self.checkbox_width)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let y = self.info.paint(ctx, data, env, self.checkbox_width);

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
        if self.checked {
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
    }
}

// TODO: Split input into separate kinds for for int/f64/string?
struct InputSettingsItem {
    /// ID of the editor so that it can be easily accessed
    view_id: WidgetId,

    input: WidgetPod<LapceTabData, Padding<LapceTabData, LapceEditorView>>,

    /// The value kind of the setting item, so that we know what to parse it as.
    value_kind: SettingsValueKind,

    info: SettingsItemInfo,
}
impl InputSettingsItem {
    fn new(
        data: &mut LapceTabData,
        kind: String,
        key: String,
        desc: String,
        event_sink: ExtEventSink,
        input: String,
        value_kind: SettingsValueKind,
    ) -> Self {
        let name = format!("{kind}.{key}");
        let content = BufferContent::SettingsValue(name.clone());

        let mut doc =
            Document::new(content.clone(), data.id, event_sink, data.proxy.clone());
        doc.reload(Rope::from(&input), true);
        data.main_split.value_docs.insert(name, Arc::new(doc));
        let editor = LapceEditorData::new(None, None, None, content, &data.config);
        let view_id = editor.view_id;
        let input = LapceEditorView::new(editor.view_id, editor.editor_id, None)
            .hide_header()
            .hide_gutter()
            .padding((5.0, 0.0, 50.0, 0.0));
        data.main_split.editors.insert(view_id, Arc::new(editor));

        Self {
            view_id,
            input: WidgetPod::new(input),
            value_kind,
            info: SettingsItemInfo::new(key, kind, desc),
        }
    }
}
impl Widget<LapceTabData> for InputSettingsItem {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        // Don't alert the input to mouse-wheel events
        if !matches!(event, Event::Wheel(_)) {
            self.input.event(ctx, event, data, env);
        }

        match event {
            Event::MouseMove(_) => ctx.set_handled(),
            // Save settings when it has been some time since the last edit
            Event::Timer(token) if self.info.idle_timer_triggered(*token) => {
                ctx.set_handled();
                let editor_data = data.editor_view_content(self.view_id);

                if let BufferContent::SettingsValue(_) = &editor_data.editor.content
                {
                    let content = editor_data.doc.buffer().to_string();
                    let value = match self.value_kind {
                        SettingsValueKind::String => {
                            Some(serde_json::json!(content))
                        }
                        SettingsValueKind::Integer => {
                            content.parse::<i64>().ok().map(|n| serde_json::json!(n))
                        }
                        SettingsValueKind::Float => {
                            content.parse::<f64>().ok().map(|n| serde_json::json!(n))
                        }
                        // Should be unreachable
                        SettingsValueKind::Bool => None,
                    };

                    if let Some(value) = value {
                        self.info.update_settings(data, ctx, value);
                    }
                } else {
                    log::warn!("Setting Input editor view id referred to editor with non-settings-value BufferContent");
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
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }

        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.info.update(ctx, old_data, data, env);

        let old_editor_data = old_data.editor_view_content(self.view_id);
        let editor_data = data.editor_view_content(self.view_id);

        // If there's been changes, then report that the last time we were idle is right now
        // TODO: minor. These usages of slice_to_cow are fine, since all settings are short values and thus
        // it can probably just return a `Cow::Borrowed(_)`, but there's probably a better way to compare
        if !editor_data.doc.buffer().is_pristine()
            && (editor_data.doc.buffer().len() != old_editor_data.doc.buffer().len()
                || editor_data.doc.buffer().text().slice_to_cow(..)
                    != old_editor_data.doc.buffer().text().slice_to_cow(..))
        {
            self.info.last_idle_timer =
                ctx.request_timer(SettingsItemInfo::SAVE_DELAY, None);
        }

        self.input.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = self.info.layout(ctx, bc, data, env, 0.0);

        let input_size = self.input.layout(ctx, bc, data, env);
        self.input
            .set_origin(ctx, data, env, Point::new(0.0, size.height));

        Size::new(size.width, (size.height + input_size.height).ceil())
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.info.paint(ctx, data, env, 0.0);
        self.input.paint(ctx, data, env);
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

#[derive(Clone, Copy)]
pub enum ThemeKind {
    Base,
    UI,
    Syntax,
}

impl Display for ThemeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ThemeKind::Base => "color-theme.base",
            ThemeKind::UI => "color-theme.ui",
            ThemeKind::Syntax => "color-theme.syntax",
        })
    }
}

pub struct ThemeSettings {
    widget_id: WidgetId,
    kind: ThemeKind,
    inputs: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    keys: Vec<String>,
    text_layouts: Option<Vec<PietTextLayout>>,
    changed_rects: Vec<(String, String, Rect)>,
    mouse_down_rect: Option<(String, String, Rect)>,
}

impl ThemeSettings {
    fn new_boxed() -> Box<dyn Widget<LapceTabData>> {
        LapceScroll::new(
            LapceSplit::new(WidgetId::next())
                .horizontal()
                .hide_border()
                .with_child(
                    Self {
                        kind: ThemeKind::Base,
                        widget_id: WidgetId::next(),
                        inputs: Vec::new(),
                        keys: Vec::new(),
                        text_layouts: None,
                        changed_rects: Vec::new(),
                        mouse_down_rect: None,
                    }
                    .boxed(),
                    None,
                    1.0,
                )
                .with_child(
                    Self {
                        kind: ThemeKind::Syntax,
                        widget_id: WidgetId::next(),
                        inputs: Vec::new(),
                        keys: Vec::new(),
                        text_layouts: None,
                        changed_rects: Vec::new(),
                        mouse_down_rect: None,
                    }
                    .boxed(),
                    None,
                    1.0,
                )
                .with_child(
                    Self {
                        kind: ThemeKind::UI,
                        widget_id: WidgetId::next(),
                        inputs: Vec::new(),
                        keys: Vec::new(),
                        text_layouts: None,
                        changed_rects: Vec::new(),
                        mouse_down_rect: None,
                    }
                    .boxed(),
                    None,
                    1.0,
                )
                .boxed(),
        )
        .boxed()
    }

    fn update_inputs(&mut self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        self.keys.clear();
        self.inputs.clear();
        self.text_layouts = None;

        let colors: Vec<&str> = match self.kind {
            ThemeKind::Base => {
                data.config.color.base.keys().into_iter().sorted().collect()
            }
            ThemeKind::UI => data
                .config
                .color
                .ui
                .keys()
                .map(|s| s.as_str())
                .sorted()
                .collect(),
            ThemeKind::Syntax => data
                .config
                .color
                .syntax
                .keys()
                .map(|s| s.as_str())
                .sorted()
                .collect(),
        };

        for color in colors {
            let name = format!("{}.{color}", self.kind);
            let content = BufferContent::SettingsValue(name.clone());
            let mut doc = Document::new(
                content.clone(),
                data.id,
                ctx.get_external_handle(),
                data.proxy.clone(),
            );
            doc.reload(
                Rope::from(match self.kind {
                    ThemeKind::Base => {
                        data.config.color_theme.base.get(color).unwrap()
                    }
                    ThemeKind::UI => data.config.color_theme.ui.get(color).unwrap(),
                    ThemeKind::Syntax => {
                        data.config.color_theme.syntax.get(color).unwrap()
                    }
                }),
                true,
            );
            data.main_split.value_docs.insert(name, Arc::new(doc));
            let editor =
                LapceEditorData::new(None, None, None, content, &data.config);
            let view_id = editor.view_id;
            let input = LapceEditorView::new(editor.view_id, editor.editor_id, None)
                .hide_header()
                .hide_gutter()
                .padding((5.0, 0.0, 5.0, 0.0));
            data.main_split.editors.insert(view_id, Arc::new(editor));
            self.keys.push(color.to_string());
            self.inputs.push(WidgetPod::new(input.boxed()));
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
        match event {
            Event::MouseDown(mouse_event) => {
                self.mouse_down_rect = None;
                for (key, default, change) in self.changed_rects.iter() {
                    if change.contains(mouse_event.pos) {
                        self.mouse_down_rect =
                            Some((key.to_string(), default.to_string(), *change));
                    }
                }
            }
            Event::MouseUp(mouse_event) => {
                if let Some((key, default, rect)) = self.mouse_down_rect.as_ref() {
                    if rect.contains(mouse_event.pos) {
                        let name = format!("{}.{key}", self.kind);
                        let doc = data.main_split.value_docs.get_mut(&name).unwrap();
                        let doc = Arc::make_mut(doc);
                        doc.reload(Rope::from(default), true);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ResetSettingsFile(
                                self.kind.to_string(),
                                key.clone(),
                            ),
                            Target::Widget(data.id),
                        ));
                    }
                }
                self.mouse_down_rect = None;
            }
            _ => {}
        }
        for input in self.inputs.iter_mut() {
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
        for input in self.inputs.iter_mut() {
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
            self.text_layouts = None;
        }
        for input in self.inputs.iter_mut() {
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

        if self.text_layouts.is_none() {
            let mut text_layouts = Vec::new();
            for key in self.keys.iter() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(key.to_string())
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
                text_layouts.push(text_layout);
            }
            self.text_layouts = Some(text_layouts);
        }

        let text_width = self
            .text_layouts
            .as_ref()
            .unwrap()
            .iter()
            .map(|text_layout| text_layout.size().width.ceil() as usize)
            .max()
            .unwrap_or(0) as f64;

        let mut y = 30.0;
        let input_bc = BoxConstraints::tight(Size::new(
            (bc.max().width - text_width - 10.0).min(150.0),
            100.0,
        ));

        let reset_text = ctx
            .text()
            .new_text_layout("reset")
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
        let reset_size = reset_text.size();
        self.changed_rects.clear();

        for (i, input) in self.inputs.iter_mut().enumerate() {
            let size = input.layout(ctx, &input_bc, data, env);
            let padding = (size.height * 0.2).round();
            y += padding;
            input.set_origin(ctx, data, env, Point::new(text_width + 10.0, y));
            y += size.height + padding;

            let (changed, default) = match self.kind {
                ThemeKind::Base => {
                    let default = data
                        .config
                        .default_color_theme
                        .base
                        .get(&self.keys[i])
                        .unwrap()
                        .to_string();
                    (
                        data.config.color_theme.base.get(&self.keys[i])
                            != Some(&default),
                        default,
                    )
                }
                ThemeKind::UI => {
                    if let Some(default) =
                        data.config.default_color_theme.ui.get(&self.keys[i])
                    {
                        (
                            data.config.color_theme.ui.get(&self.keys[i])
                                != Some(default),
                            default.to_string(),
                        )
                    } else {
                        continue;
                    }
                }
                ThemeKind::Syntax => {
                    let default = data
                        .config
                        .default_color_theme
                        .syntax
                        .get(&self.keys[i])
                        .cloned()
                        .unwrap_or_else(|| "".to_string());
                    (
                        data.config.color_theme.syntax.get(&self.keys[i])
                            != Some(&default),
                        default,
                    )
                }
            };
            if changed {
                let x = input.layout_rect().x1 + input.layout_rect().height() + 15.0;
                let y0 = input.layout_rect().y0;
                let y1 = input.layout_rect().y1;
                let rect = Rect::new(x, y0, x + reset_size.width + 20.0, y1);
                self.changed_rects
                    .push((self.keys[i].clone(), default, rect));
            }
        }

        Size::new(bc.max().width, y + 10.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let header_text = ctx
            .text()
            .new_text_layout(match self.kind {
                ThemeKind::Base => "Base Colors",
                ThemeKind::UI => "UI Colors",
                ThemeKind::Syntax => "Syntax Colors",
            })
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(&header_text, Point::new(0.0, header_text.y_offset(30.0)));

        for (i, input) in self.inputs.iter_mut().enumerate() {
            let text_layout = &self.text_layouts.as_ref().unwrap()[i];
            let layout_rect = input.layout_rect();
            ctx.draw_text(
                text_layout,
                Point::new(
                    0.0,
                    layout_rect.y0 + text_layout.y_offset(layout_rect.height()),
                ),
            );
            input.paint(ctx, data, env);
            let preview_color_text = text_layout.text();
            let preview_color = match self.kind {
                ThemeKind::Base => data
                    .config
                    .color
                    .base
                    .get(preview_color_text)
                    .unwrap_or_else(|| {
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    }),
                ThemeKind::UI => {
                    data.config.color.ui.get(preview_color_text).unwrap_or_else(
                        || {
                            data.config
                                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                        },
                    )
                }
                ThemeKind::Syntax => data
                    .config
                    .color
                    .syntax
                    .get(preview_color_text)
                    .unwrap_or_else(|| {
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    }),
            };
            let color_rect = Rect::new(
                layout_rect.x1 + 5.0,
                layout_rect.y0,
                layout_rect.x1 + 5.0 + layout_rect.height(),
                layout_rect.y1,
            )
            .inflate(-0.5, -0.5);
            ctx.stroke(
                color_rect,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            ctx.fill(color_rect.inflate(-0.5, -0.5), preview_color);
        }

        let reset_text = ctx
            .text()
            .new_text_layout("reset")
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
        for (_, _, rect) in self.changed_rects.iter() {
            ctx.stroke(
                rect.inflate(-0.5, -0.5),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            ctx.draw_text(
                &reset_text,
                Point::new(
                    rect.x0 + 10.0,
                    rect.y0 + reset_text.y_offset(rect.height()),
                ),
            )
        }
    }
}

struct SettingsSwitcher {
    settings_widget_id: WidgetId,
    plugin_settings_expanded: bool,
    line_height: f64,
    last_mouse_down: Option<usize>,
    active: LapceSettingsKind,
    active_index: Option<usize>,
}

impl SettingsSwitcher {
    fn new(settings_widget_id: WidgetId) -> Self {
        Self {
            settings_widget_id,
            plugin_settings_expanded: true,
            line_height: 40.0,
            last_mouse_down: None,
            active: LapceSettingsKind::Core,
            active_index: Some(0),
        }
    }

    fn num_items(&self, data: &LapceTabData) -> usize {
        let mut n = 7;
        if self.plugin_settings_expanded {
            n += data
                .plugin
                .installed
                .iter()
                .filter(|(_, v)| v.config.is_some())
                .count();
        }
        n
    }

    pub fn set_active(&mut self, active: LapceSettingsKind, data: &LapceTabData) {
        self.active = active;

        if let LapceSettingsKind::Plugin(active_volt_id) = &self.active {
            for (i, (volt_id, _)) in data
                .plugin
                .installed
                .iter()
                .filter(|(_, v)| v.config.is_some())
                .sorted_by_key(|(_, v)| &v.display_name)
                .enumerate()
            {
                if active_volt_id == volt_id {
                    self.active_index = Some(i + 6);
                    return;
                }
            }
        }

        let kinds = [
            LapceSettingsKind::Core,
            LapceSettingsKind::UI,
            LapceSettingsKind::Editor,
            LapceSettingsKind::Terminal,
            LapceSettingsKind::Theme,
            LapceSettingsKind::Keymap,
        ];

        for (i, kind) in kinds.iter().enumerate() {
            if kind == &self.active {
                self.active_index = Some(i);
                return;
            }
        }
    }
}

impl Widget<LapceTabData> for SettingsSwitcher {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse_event) => {
                self.last_mouse_down = None;
                let index = (mouse_event.pos.y / self.line_height) as usize;
                if index < self.num_items(data) {
                    self.last_mouse_down = Some(index);
                }
            }
            Event::MouseUp(mouse_event) => {
                if let Some(last_index) = self.last_mouse_down.take() {
                    let index = (mouse_event.pos.y / self.line_height) as usize;
                    if index < self.num_items(data) && index == last_index {
                        match index {
                            6 => {
                                self.plugin_settings_expanded =
                                    !self.plugin_settings_expanded;
                                ctx.request_layout();
                            }
                            _ if index > 6 => {
                                if let Some((volt_id, _)) = data
                                    .plugin
                                    .installed
                                    .iter()
                                    .filter(|(_, v)| v.config.is_some())
                                    .sorted_by_key(|(_, v)| &v.display_name)
                                    .nth(index - 7)
                                {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::ShowSettingsKind(
                                            LapceSettingsKind::Plugin(
                                                volt_id.to_string(),
                                            ),
                                        ),
                                        Target::Widget(self.settings_widget_id),
                                    ));
                                }
                            }
                            _ => {
                                if let Some(kind) = [
                                    LapceSettingsKind::Core,
                                    LapceSettingsKind::UI,
                                    LapceSettingsKind::Editor,
                                    LapceSettingsKind::Terminal,
                                    LapceSettingsKind::Theme,
                                    LapceSettingsKind::Keymap,
                                ]
                                .get(index)
                                {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::ShowSettingsKind(
                                            kind.clone(),
                                        ),
                                        Target::Widget(self.settings_widget_id),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Event::MouseMove(mouse_event) => {
                if mouse_event.pos.y
                    <= self.num_items(data) as f64 * self.line_height
                {
                    ctx.set_cursor(&Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
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
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let width = 150.0;
        let mut n = 7;
        if self.plugin_settings_expanded {
            n += data
                .plugin
                .installed
                .iter()
                .filter(|(_, v)| v.config.is_some())
                .count();
        }
        Size::new(width, bc.max().height.max(n as f64 * self.line_height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let mut settings_sections: Vec<&str> = vec![
            "Core Settings",
            "UI Settings",
            "Editor Settings",
            "Terminal Settings",
            "Theme Settings",
            "Keybindings",
            "Plugin Settings",
        ];

        if self.plugin_settings_expanded {
            for (_, volt) in data
                .plugin
                .installed
                .iter()
                .sorted_by_key(|(_, v)| &v.display_name)
            {
                if volt.config.is_some() {
                    settings_sections.push(volt.display_name.as_str());
                }
            }
        }

        for (i, text) in settings_sections.iter().enumerate() {
            let font_size = if i <= 6 {
                data.config.ui.font_size() + 1
            } else {
                data.config.ui.font_size()
            };
            let text_layout = ctx
                .text()
                .new_text_layout(text.to_string())
                .font(data.config.ui.font_family(), font_size as f64)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();

            let x = if i <= 6 { 20.0 } else { 40.0 };
            ctx.draw_text(
                &text_layout,
                Point::new(
                    x,
                    i as f64 * self.line_height
                        + text_layout.y_offset(self.line_height),
                ),
            );
        }

        let x = 2.0;
        let active = self.active_index.unwrap_or(0);
        let active = if active > 5 { active + 1 } else { active };
        if active <= 6 || self.plugin_settings_expanded {
            let y0 = self.line_height * active as f64;
            let y1 = y0 + self.line_height;
            ctx.stroke(
                Line::new(Point::new(x, y0 + 5.0), Point::new(x, y1 - 5.0)),
                data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                2.0,
            );
        }
    }
}
