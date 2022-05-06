use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Size, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_data::data::LapceTabData;

use crate::{
    editor::{gutter::LapceEditorGutter, LapceEditor},
    scroll::{LapceIdentityWrapper, LapcePadding, LapceScrollNew},
};

pub struct LapceEditorContainer {
    pub view_id: WidgetId,
    pub scroll_id: WidgetId,
    pub display_gutter: bool,
    pub gutter:
        WidgetPod<LapceTabData, LapcePadding<LapceTabData, LapceEditorGutter>>,
    pub editor: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScrollNew<LapceTabData, LapceEditor>>,
    >,
}

impl LapceEditorContainer {
    pub fn new(view_id: WidgetId) -> Self {
        let scroll_id = WidgetId::next();
        let gutter = LapceEditorGutter::new(view_id);
        let gutter = LapcePadding::new((10.0, 0.0, 0.0, 0.0), gutter);
        let editor = LapceEditor::new(view_id);
        let editor = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(editor).vertical().horizontal(),
            scroll_id,
        );
        Self {
            view_id,
            scroll_id,
            display_gutter: true,
            gutter: WidgetPod::new(gutter),
            editor: WidgetPod::new(editor),
        }
    }
}

impl Widget<LapceTabData> for LapceEditorContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.gutter.event(ctx, event, data, env);
        self.editor.event(ctx, event, data, env);
        match event {
            Event::MouseDown(_) | Event::MouseUp(_) => {
                let editor =
                    data.main_split.editors.get(&self.view_id).unwrap().clone();
                let mut editor_data = data.editor_view_content(self.view_id);
                let doc = editor_data.doc.clone();
                editor_data
                    .sync_buffer_position(self.editor.widget().inner().offset());
                data.update_from_editor_buffer_data(editor_data, &editor, &doc);
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.gutter.lifecycle(ctx, event, data, env);
        self.editor.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.gutter.update(ctx, data, env);
        self.editor.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let gutter_size = self.gutter.layout(ctx, bc, data, env);
        self.gutter.set_origin(ctx, data, env, Point::ZERO);
        let editor_size = Size::new(
            self_size.width
                - if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
            self_size.height,
        );
        let editor_bc = BoxConstraints::new(Size::ZERO, editor_size);
        let editor_size = self.editor.layout(ctx, &editor_bc, data, env);
        self.editor.set_origin(
            ctx,
            data,
            env,
            Point::new(
                if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
                0.0,
            ),
        );
        *data
            .main_split
            .editors
            .get(&self.view_id)
            .unwrap()
            .size
            .borrow_mut() = editor_size;
        Size::new(
            if self.display_gutter {
                gutter_size.width
            } else {
                0.0
            } + editor_size.width,
            editor_size.height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.editor.paint(ctx, data, env);
        if self.display_gutter {
            self.gutter.paint(ctx, data, env);
        }
    }
}
