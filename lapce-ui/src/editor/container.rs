use druid::{
    kurbo::Line, BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget, WidgetId,
    WidgetPod,
};
use lapce_data::{config::LapceTheme, data::LapceTabData};

use super::bread_crumb::LapceEditorBreadCrumb;
use crate::{
    editor::{gutter::LapceEditorGutter, LapceEditor},
    scroll::{LapceIdentityWrapper, LapcePadding, LapceScroll},
};

pub struct LapceEditorContainer {
    pub view_id: WidgetId,
    pub scroll_id: WidgetId,
    pub display_gutter: bool,
    pub bread_crumb:
        WidgetPod<LapceTabData, LapceScroll<LapceTabData, LapceEditorBreadCrumb>>,
    pub gutter:
        WidgetPod<LapceTabData, LapcePadding<LapceTabData, LapceEditorGutter>>,
    pub editor: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<LapceScroll<LapceTabData, LapceEditor>>,
    >,
}

impl LapceEditorContainer {
    pub fn new(view_id: WidgetId, editor_id: WidgetId) -> Self {
        let scroll_id = WidgetId::next();
        let bread_crumb = LapceScroll::new(LapceEditorBreadCrumb::new(view_id))
            .horizontal()
            .vertical_scroll_for_horizontal();
        let gutter = LapceEditorGutter::new(view_id);
        let gutter = LapcePadding::new((0.0, 0.0, 0.0, 0.0), gutter);
        let editor = LapceEditor::new(view_id, editor_id);
        let editor = LapceIdentityWrapper::wrap(
            LapceScroll::new(editor).vertical().horizontal(),
            scroll_id,
        );
        Self {
            view_id,
            scroll_id,
            display_gutter: true,
            bread_crumb: WidgetPod::new(bread_crumb),
            gutter: WidgetPod::new(gutter),
            editor: WidgetPod::new(editor),
        }
    }

    fn show_bread_crumbs(&self, data: &LapceTabData) -> bool {
        if !data.config.editor.show_bread_crumbs {
            return false;
        }

        let editor = data.main_split.editors.get(&self.view_id).unwrap();
        if editor.content.is_file() {
            return true;
        }

        false
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
        if self.show_bread_crumbs(data) {
            self.bread_crumb.event(ctx, event, data, env);
        }
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
        self.bread_crumb.lifecycle(ctx, event, data, env);
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
        self.bread_crumb.update(ctx, data, env);
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
        let show_bread_crumbs = self.show_bread_crumbs(data);

        let mut bread_crumbs_size = Size::ZERO;
        if show_bread_crumbs {
            bread_crumbs_size = self.bread_crumb.layout(ctx, bc, data, env);
            self.bread_crumb.set_origin(ctx, data, env, Point::ZERO);
        }

        let bc = BoxConstraints::tight(Size::new(
            self_size.width,
            self_size.height - bread_crumbs_size.height,
        ));
        let gutter_size = self.gutter.layout(ctx, &bc, data, env);
        self.gutter.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, bread_crumbs_size.height),
        );
        let editor_size = Size::new(
            self_size.width
                - if self.display_gutter {
                    gutter_size.width
                } else {
                    0.0
                },
            self_size.height - bread_crumbs_size.height,
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
                bread_crumbs_size.height,
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
            bread_crumbs_size.height + editor_size.height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let show_bread_crumbs = self.show_bread_crumbs(data);

        self.editor.paint(ctx, data, env);
        if self.display_gutter {
            self.gutter.paint(ctx, data, env);
        }

        let mut bread_crumbs_height = 0.0;
        if data.config.editor.sticky_header {
            let data = data.editor_view_content(self.view_id);
            let info = data.editor.sticky_header.borrow();
            let size = ctx.size();
            if info.height > 0.0 {
                bread_crumbs_height = if show_bread_crumbs {
                    self.bread_crumb.layout_rect().height()
                } else {
                    0.0
                };

                ctx.with_save(|ctx| {
                    let rect = Size::new(size.width, info.height)
                        .to_rect()
                        .with_origin(Point::new(0.0, bread_crumbs_height));
                    ctx.clip(rect.inset((0.0, 0.0, 0.0, info.height)));
                    ctx.blurred_rect(
                        rect.inflate(50.0, 0.0),
                        3.0,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                });
            }
        }

        if show_bread_crumbs {
            if bread_crumbs_height == 0.0 {
                let rect = self.bread_crumb.layout_rect();
                let shadow_width = data.config.ui.drop_shadow_width() as f64;
                if shadow_width > 0.0 {
                    ctx.with_save(|ctx| {
                        ctx.clip(rect.inset((0.0, 0.0, 0.0, rect.height())));
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    });
                } else {
                    ctx.stroke(
                        Line::new(
                            Point::new(rect.x0, rect.y1 - 0.5),
                            Point::new(rect.x1, rect.y1 - 0.5),
                        ),
                        data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                        1.0,
                    );
                }
            }
            self.bread_crumb.paint(ctx, data, env);
        }
    }
}
