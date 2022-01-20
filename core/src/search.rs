use std::{path::PathBuf, sync::Arc};

use crate::{
    buffer::{BufferNew, UpdateEvent},
    config::Config,
    data::EditorContent,
    editor::LapceEditorView,
    split::SplitDirection,
};
use crossbeam_channel::Sender;
use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll, SvgData},
    Affine, BoxConstraints, Color, Command, Cursor, Data, Env, Event, EventCtx,
    FontFamily, FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent,
    PaintCtx, Point, Rect, RenderContext, Size, Target, TextLayout, UpdateCtx, Vec2,
    Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};

use crate::{
    data::{LapceEditorData, LapceTabData},
    panel::{LapcePanel, PanelHeaderKind},
    split::LapceSplitNew,
};

pub struct SearchData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub editor_view_id: WidgetId,
}

impl SearchData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            editor_view_id: WidgetId::next(),
        }
    }

    pub fn new_panel(&self, data: &LapceTabData) -> LapcePanel {
        let editor_data = data
            .main_split
            .editors
            .get(&data.search.editor_view_id)
            .unwrap();
        let input = LapceEditorView::new(editor_data)
            .hide_header()
            .hide_gutter()
            .padding(10.0);
        let split = LapceSplitNew::new(self.split_id)
            .horizontal()
            .with_child(input.boxed(), None, 45.0)
            .with_flex_child(SearchContent::new().boxed(), None, 1.0);
        LapcePanel::new(
            self.widget_id,
            self.split_id,
            SplitDirection::Vertical,
            PanelHeaderKind::Simple("Search".to_string()),
            vec![(self.split_id, PanelHeaderKind::None, split.boxed(), None)],
        )
    }
}

pub struct SearchContent {}

impl SearchContent {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<LapceTabData> for SearchContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {}
}
