use druid::WidgetId;

pub struct OutlineState {
    #[allow(dead_code)]
    widget_id: WidgetId,
}

impl OutlineState {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
        }
    }
}
