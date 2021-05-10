use std::sync::Arc;

use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Size, Widget, WidgetExt, WidgetId, WidgetPod,
};

use crate::{
    buffer::{BufferId, BufferNew, BufferState},
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceEditorLens, LapceMainSplitData, LapceTabData},
    editor::LapceEditorView,
    split::LapceSplitNew,
};

pub struct LapceTabNew {
    id: WidgetId,
    main_split: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl LapceTabNew {
    pub fn new(data: &LapceTabData) -> Self {
        let editor_widget_id = data.main_split.editors.iter().next().unwrap().0;
        let main_split = LapceSplitNew::new().with_flex_child(
            LapceEditorView::new()
                .lens(LapceEditorLens(*editor_widget_id))
                .boxed(),
            1.0,
        );
        Self {
            id: data.id,
            main_split: WidgetPod::new(main_split.boxed()),
        }
    }
}

impl Widget<LapceTabData> for LapceTabNew {
    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::WindowConnected => {
                for (_, editor) in data.main_split.editors.iter() {
                    if let Some(path) = editor.buffer.as_ref() {
                        if !data.main_split.open_files.contains_key(path) {
                            let buffer_id = BufferId::next();
                            data.main_split
                                .open_files
                                .insert(path.clone(), buffer_id);
                            data.main_split
                                .buffers
                                .insert(buffer_id, BufferState::Loading);
                            BufferNew::load_file(
                                buffer_id,
                                path.clone(),
                                data.proxy.clone(),
                                data.id,
                                ctx.get_external_handle(),
                            );
                        }
                    }
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::LoadFile { id, path, content } => {
                        let buffer =
                            BufferNew::new(*id, path.to_owned(), content.to_owned());
                        data.main_split
                            .buffers
                            .insert(*id, BufferState::Open(Arc::new(buffer)));
                        ctx.set_handled();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.main_split.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.main_split.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.main_split.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        self.main_split.layout(ctx, bc, data, env);
        self.main_split.set_origin(ctx, data, env, Point::ZERO);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.main_split.paint(ctx, data, env);
    }
}
