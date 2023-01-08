use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, Target, UpdateCtx, Widget,
    WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceIcons, LapceTheme},
    dropdown::DropdownData,
    list::ListData,
};

use crate::list::{List, ListPaint};

pub const DROPDOWN_SIZE: Size = Size::new(300.0, 30.0);

pub struct DropdownSelector<
    T: Clone + DropdownPaint<D> + PartialEq + 'static,
    D: Data,
> {
    widget_id: WidgetId,

    dropdown_list: WidgetPod<DropdownData<T, D>, DropdownList<T, D>>,
}
impl<T: Clone + DropdownPaint<D> + PartialEq + 'static, D: Data> Default
    for DropdownSelector<T, D>
{
    fn default() -> Self {
        let dropdown_list = WidgetPod::new(DropdownList::default());

        Self {
            widget_id: WidgetId::next(),

            dropdown_list,
        }
    }
}
impl<T: Clone + DropdownPaint<D> + PartialEq + 'static, D: Data>
    Widget<DropdownData<T, D>> for DropdownSelector<T, D>
{
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut DropdownData<T, D>,
        env: &Env,
    ) {
        if event.should_propagate_to_hidden() || data.list_active {
            self.dropdown_list.event(ctx, event, data, env);
        }

        // TODO: Hover event should highlight it and change the cursor
        if let Event::MouseUp(_) = event {
            if data.list_active {
                data.hide();
            } else {
                data.show();
            }

            ctx.request_focus();
            ctx.request_layout();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &DropdownData<T, D>,
        env: &Env,
    ) {
        self.dropdown_list.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &DropdownData<T, D>,
        data: &DropdownData<T, D>,
        env: &Env,
    ) {
        if data.list_active != old_data.list_active {
            ctx.request_layout();
        }

        if data.list_active {
            self.dropdown_list.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &DropdownData<T, D>,
        env: &Env,
    ) -> Size {
        let list_size = if data.list_active {
            let height = data.list.config.editor.line_height() as f64 * 10.0;
            let bc = BoxConstraints::tight(Size::new(DROPDOWN_SIZE.width, height));
            let list_size = self.dropdown_list.layout(ctx, &bc, data, env);
            let list_origin = Point::new(0.0, DROPDOWN_SIZE.height);
            self.dropdown_list.set_origin(ctx, data, env, list_origin);

            list_size
        } else {
            Size::ZERO
        };

        Size::new(
            DROPDOWN_SIZE.width.max(list_size.width),
            DROPDOWN_SIZE.height + list_size.height,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &DropdownData<T, D>, env: &Env) {
        let svg_size = data.list.config.ui.icon_size() as f64;
        let svg_y = (DROPDOWN_SIZE.height - svg_size) / 2.0;
        ctx.draw_svg(
            &data.list.config.ui_svg(LapceIcons::DROPDOWN_ARROW),
            Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(1.0, svg_y)),
            Some(
                data.list
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
            ),
        );

        ctx.stroke(
            Size::new(DROPDOWN_SIZE.width - 1.0, DROPDOWN_SIZE.height)
                .to_rect()
                .with_origin(Point::new(1.0, 0.0)),
            data.list
                .config
                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        if let Some(item) = data.get_active_item() {
            let origin = Point::new(svg_size * 1.5, 0.0);
            item.paint_active(ctx, data, env, origin);
        }

        if data.list_active {
            self.dropdown_list.paint(ctx, data, env);
        }
    }
}

pub struct DropdownList<T: Clone + DropdownPaint<D> + PartialEq + 'static, D: Data> {
    widget_id: WidgetId,
    list: WidgetPod<ListData<T, D>, List<T, D>>,
}
impl<T: Clone + DropdownPaint<D> + PartialEq + 'static, D: Data> Default
    for DropdownList<T, D>
{
    fn default() -> Self {
        let widget_id = WidgetId::next();
        let scroll_id = WidgetId::next();
        let list = WidgetPod::new(List::new(scroll_id));
        Self { widget_id, list }
    }
}
impl<T: Clone + DropdownPaint<D> + PartialEq + 'static, D: Data>
    Widget<DropdownData<T, D>> for DropdownList<T, D>
{
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut DropdownData<T, D>,
        env: &Env,
    ) {
        // The caller should call `update_data` on the DropdownData before passing it to us
        // and the DropdownData updating also updates the list
        self.list.event(ctx, event, &mut data.list, env);

        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                let pre_active_idx = data.active_item_index;
                match command {
                    LapceUICommand::Hide => {
                        // TODO: Should we have an option on the dropdown for whether we should
                        // update the current active item when the dropdown is hidden (rather
                        // than typical selection)?
                        data.update_active_item();
                        data.hide();
                        ctx.request_layout();
                    }
                    LapceUICommand::ListItemSelected => {
                        data.update_active_item();
                        data.hide();
                        ctx.request_layout();
                    }
                    _ => {}
                }

                if pre_active_idx != data.active_item_index {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::DropdownItemSelected,
                        Target::Widget(data.list.parent),
                    ));
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &DropdownData<T, D>,
        env: &Env,
    ) {
        if let LifeCycle::FocusChanged(focus) = event {
            if !focus {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Hide,
                    Target::Widget(self.widget_id),
                ));
            }
        }

        self.list.lifecycle(ctx, event, &data.list, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &DropdownData<T, D>,
        data: &DropdownData<T, D>,
        env: &Env,
    ) {
        self.list.update(ctx, &data.list, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &DropdownData<T, D>,
        env: &Env,
    ) -> Size {
        let list_size = if data.list_active {
            self.list.layout(ctx, bc, &data.list, env)
        } else {
            Size::ZERO
        };

        self.list.set_origin(ctx, &data.list, env, Point::ZERO);

        ctx.set_paint_insets(4000.0);

        list_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &DropdownData<T, D>, env: &Env) {
        if data.list_active {
            let rect = ctx.size().to_rect();
            ctx.fill(
                rect,
                data.list
                    .config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );
            self.list.paint(ctx, &data.list, env);
        }
    }
}

pub trait DropdownPaint<D: Data>: ListPaint<D> {
    /// Paint the active item (aka the item displayed within the button itself)
    fn paint_active(
        &self,
        ctx: &mut PaintCtx,
        data: &DropdownData<Self, D>,
        env: &Env,
        origin: Point,
    );
}

impl<D: Data> DropdownPaint<D> for String {
    fn paint_active(
        &self,
        ctx: &mut PaintCtx,
        data: &DropdownData<Self, D>,
        _env: &Env,
        origin: Point,
    ) {
        let line_height = data.list.config.ui.list_line_height() as f64;
        let text_layout = ctx
            .text()
            .new_text_layout(self.clone())
            .font(
                data.list.config.ui.font_family(),
                data.list.config.ui.font_size() as f64,
            )
            .text_color(
                data.list
                    .config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();

        let point =
            Point::new(1.0 + origin.x, text_layout.y_offset(line_height) + origin.y);
        ctx.draw_text(&text_layout, point);
    }
}
