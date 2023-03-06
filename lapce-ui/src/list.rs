use std::marker::PhantomData;

use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx,
    Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    list::ListData,
};

use crate::scroll::{LapceIdentityWrapper, LapceScroll};

// TODO: Support optional multi-select

/// Contains a list of choices of type `T`  
/// The type must be cloneable, list-paintable, and able to be compared.  
/// `D` is associated data that users of `List` may need, since the painting is only given
/// `&ListData<T, D>` for use.  
///  
/// We let the `KeyPressFocus` be handled by the containing widget, since widgets like palette
/// want focus for themselves. You can call `ListData::run_command` to use its sensible
/// defaults for movement and the like.  
pub struct List<T: Clone + ListPaint<D> + PartialEq + 'static, D: Data> {
    content_rect: Rect,
    scroll_id: WidgetId,
    // I don't see a way to break this apart that doesn't make it less clear
    #[allow(clippy::type_complexity)]
    content: WidgetPod<
        ListData<T, D>,
        LapceIdentityWrapper<LapceScroll<ListData<T, D>, ListContent<T, D>>>,
    >,
}
impl<T: Clone + ListPaint<D> + PartialEq + 'static, D: Data> List<T, D> {
    pub fn new(scroll_id: WidgetId) -> List<T, D> {
        let content = LapceIdentityWrapper::wrap(
            LapceScroll::new(ListContent::new()).vertical(),
            scroll_id,
        );
        List {
            content_rect: Rect::ZERO,
            scroll_id,
            content: WidgetPod::new(content),
        }
    }

    pub fn scroll_to(&mut self, point: Point) -> bool {
        self.content.widget_mut().inner_mut().scroll_to(point)
    }

    pub fn scroll_to_visible(&mut self, rect: Rect, env: &Env) -> bool {
        self.content
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
    }

    pub fn ensure_item_visible(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &ListData<T, D>,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let line_height = data.line_height() as f64;

        let rect = Size::new(width, line_height)
            .to_rect()
            .with_origin(Point::new(0.0, data.selected_index as f64 * line_height));
        if self.scroll_to_visible(rect, env) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(self.scroll_id),
            ));
        }
    }
}
impl<T: Clone + ListPaint<D> + PartialEq + 'static, D: Data> Widget<ListData<T, D>>
    for List<T, D>
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut ListData<T, D>,
        env: &Env,
    ) {
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &ListData<T, D>,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &ListData<T, D>,
        data: &ListData<T, D>,
        env: &Env,
    ) {
        if !data.same(old_data) {
            self.ensure_item_visible(ctx, data, env);
            ctx.request_paint();
        }

        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ListData<T, D>,
        env: &Env,
    ) -> Size {
        // TODO: Allow restricting the max height? Technically that can just be done by restricting the
        // maximum number of displayed items

        // The width are given by whatever widget contains the list
        let width = bc.max().width;

        let line_height = data.line_height() as f64;

        let count = data.max_display_count();
        // The height of the rendered entries
        let height = count as f64 * line_height;

        // TODO: Have an option to let us fill the rest of the space with empty background
        // Since some lists may want to take up all the space they're given

        // Create a bc which only contains the list we're actually rendering
        let bc = BoxConstraints::tight(Size::new(width, height));

        let content_size = self.content.layout(ctx, &bc, data, env);
        self.content.set_origin(ctx, data, env, Point::ZERO);
        let content_height = content_size.height;

        let self_size = Size::new(content_size.width, content_height);

        self.content_rect = self_size.to_rect().with_origin(Point::ZERO);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ListData<T, D>, env: &Env) {
        // TODO: We could have the user of this provide a custom color
        // Or we could say that they have to fill it? Since something like
        // palette also wants to draw their background behind the other bits
        // and so, if we painted here, we'd double-paint
        // which would be annoying for transparent colors.
        // ctx.fill(
        //     rect,
        //     data.config
        //         .get_color_unchecked(LapceTheme::PALETTE_BACKGROUND),
        // );

        self.content.paint(ctx, data, env);
    }
}

/// The actual list of entries
struct ListContent<T: Clone + ListPaint<D> + 'static, D: Data> {
    /// The line the mouse was last down upon
    mouse_down: usize,
    _marker: PhantomData<(*const T, *const D)>,
}
impl<T: Clone + ListPaint<D> + 'static, D: Data> ListContent<T, D> {
    pub fn new() -> ListContent<T, D> {
        ListContent {
            mouse_down: 0,
            _marker: PhantomData,
        }
    }
}
impl<T: Clone + ListPaint<D> + 'static, D: Data> Widget<ListData<T, D>>
    for ListContent<T, D>
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut ListData<T, D>,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(_) => {
                ctx.set_cursor(&druid::Cursor::Pointer);
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                let line =
                    (mouse_event.pos.y / data.line_height() as f64).floor() as usize;
                self.mouse_down = line;
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                // TODO: function for translating mouse pos to the line, so that we don't repeat
                // this calculation; which makes it harder to change later
                let line =
                    (mouse_event.pos.y / data.line_height() as f64).floor() as usize;
                if line == self.mouse_down {
                    data.selected_index = line;
                    data.select(ctx);
                    ctx.set_handled();
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &ListData<T, D>,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &ListData<T, D>,
        _data: &ListData<T, D>,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ListData<T, D>,
        _env: &Env,
    ) -> Size {
        let line_height = data.line_height() as f64;
        // We include the total number of items because we should be in a scroll widget
        // and that needs the overall height, not just the rendered height.
        let height = line_height * data.items.len() as f64;

        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ListData<T, D>, env: &Env) {
        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let line_height = data.line_height() as f64;

        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;
        let count = end_line - start_line;

        // Get the items, skip over all items before the start line, and
        // ignore all items after the end line
        for (line, item) in
            data.items.iter().enumerate().skip(start_line).take(count)
        {
            if line == data.selected_index {
                // Create a rect covering the entry at the selected index
                let bg_rect = Rect::ZERO
                    .with_origin(Point::new(0.0, line as f64 * line_height))
                    .with_size(Size::new(size.width, line_height));

                // TODO: Give this its own theme name entry
                ctx.fill(
                    bg_rect,
                    data.config
                        .get_color_unchecked(LapceTheme::PALETTE_CURRENT_BACKGROUND),
                );
            }

            item.paint(ctx, data, env, line);
        }
    }
}

/// A trait for painting relatively simple elements that are put in a list  
/// They don't get a say in their layout or custom handling of events  
///  
/// Takes an immutable reference, due to `data` containing this entry
pub trait ListPaint<D: Data>: Sized + Clone {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &ListData<Self, D>,
        env: &Env,
        line: usize,
    );
}

// A simple implementation of ListPaint for entries which are just strings
impl<D: Data> ListPaint<D> for String {
    fn paint(
        &self,
        ctx: &mut PaintCtx,
        data: &ListData<Self, D>,
        _env: &Env,
        line: usize,
    ) {
        let line_height = data.line_height() as f64;
        let text_layout = ctx
            .text()
            .new_text_layout(self.clone())
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(if line == data.selected_index {
                        LapceTheme::PALETTE_CURRENT_FOREGROUND
                    } else {
                        LapceTheme::PALETTE_FOREGROUND
                    })
                    .clone(),
            )
            .build()
            .unwrap();

        // The point for the baseline of the text
        // This is shifted to the right a bit to provide some minor padding
        let point = Point::new(
            5.0,
            line_height * line as f64 + text_layout.y_offset(line_height),
        );
        ctx.draw_text(&text_layout, point);
    }
}
