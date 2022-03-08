use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Cursor, Env, Event, EventCtx, FontFamily, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext,
    Size, UpdateCtx, Widget, WidgetId,
};
use lapce_proxy::plugin::PluginDescription;
use strum_macros::Display;

use crate::{config::LapceTheme, data::LapceTabData};

pub struct PluginData {
    pub widget_id: WidgetId,
}

impl PluginData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
        }
    }
}

impl Default for PluginData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Display, PartialEq)]
pub enum PluginStatus {
    Installed,
    Install,
    Upgrade,
}

pub struct Plugin {
    line_height: f64,
}

impl Plugin {
    pub fn new() -> Self {
        Self { line_height: 25.0 }
    }

    fn hit_test<'a>(
        &self,
        ctx: &mut EventCtx,
        data: &'a LapceTabData,
        mouse_event: &MouseEvent,
    ) -> Option<(&'a PluginDescription, PluginStatus)> {
        let index = (mouse_event.pos.y / (self.line_height * 3.0)) as usize;
        let plugin = data.plugins.get(index)?;
        let status = match data
            .installed_plugins
            .get(&plugin.name)
            .map(|installed| plugin.version.clone() == installed.version.clone())
        {
            Some(true) => PluginStatus::Installed,
            Some(false) => PluginStatus::Upgrade,
            None => PluginStatus::Install,
        };

        if status == PluginStatus::Installed {
            return None;
        }

        let padding = 10.0;
        let text_padding = 5.0;

        let text_layout = ctx
            .text()
            .new_text_layout(status.to_string())
            .font(FontFamily::SYSTEM_UI, 13.0)
            .build()
            .unwrap();

        let text_size = text_layout.size();
        let x = ctx.size().width - text_size.width - text_padding * 2.0 - padding;
        let y = 3.0 * self.line_height * index as f64 + self.line_height * 2.0;
        let rect = Size::new(text_size.width + text_padding * 2.0, self.line_height)
            .to_rect()
            .with_origin(Point::new(x, y));
        if rect.contains(mouse_event.pos) {
            Some((plugin, status))
        } else {
            None
        }
    }
}

impl Default for Plugin {
    fn default() -> Self {
        Self::new()
    }
}
