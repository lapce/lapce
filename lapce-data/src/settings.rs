use std::{collections::HashMap, sync::Arc};

use druid::{
    kurbo::BezPath,
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Vec2, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use inflector::Inflector;

use crate::{
    command::{CommandExecuted, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    config::{EditorConfig, LapceConfig, LapceTheme},
    data::LapceTabData,
    keypress::KeyPressFocus,
    scroll::{LapcePadding, LapceScrollNew},
    state::Mode,
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

pub enum SettingsValue {
    Bool(bool),
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

    name_text: Option<PietTextLayout>,
    desc_text: Option<PietTextLayout>,
    value_text: Option<Option<PietTextLayout>>,
    input_rect: Rect,
}

impl LapceSettingsItem {
    pub fn new(
        kind: String,
        name: String,
        desc: String,
        value: serde_json::Value,
    ) -> Self {
        Self {
            kind,
            name,
            desc,
            value,
            padding: 10.0,
            width: 0.0,
            checkbox_width: 20.0,
            input_max_width: 500.0,
            input: "".to_string(),
            cursor: 0,
            name_text: None,
            desc_text: None,
            value_text: None,
            input_rect: Rect::ZERO,
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
