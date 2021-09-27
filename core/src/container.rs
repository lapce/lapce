use std::{collections::HashMap, sync::Arc};

use crate::command::{
    LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
};
use crate::state::Mode;
use crate::theme::OldLapceTheme;
use crate::{scroll::LapceScroll, state::LapceFocus};
use druid::piet::TextAttribute;
use druid::FontWeight;
use druid::{
    kurbo::{Line, Rect},
    piet::Text,
    piet::TextLayoutBuilder,
    Color, Vec2, WidgetId,
};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
    WidgetExt, WidgetPod, WindowId,
};

pub struct ChildState {
    pub origin: Option<Point>,
    pub size: Option<Size>,
    pub hidden: bool,
}
