use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, ExtEventSink,
    Key, KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, Modifiers, PaintCtx,
    Point, RenderContext, Selector, Size, Target, TextLayout, UpdateCtx,
    Widget, WidgetPod,
};
use lazy_static::lazy_static;
use std::time::Duration;
use std::{any::Any, thread};
use std::{collections::HashMap, sync::Arc, sync::Mutex};

use crate::container::CraneContainer;

pub struct CraneUI {
    container: CraneContainer<u32>,
}

pub struct Editor {
    text_layout: TextLayout,
}

impl Editor {
    pub fn new() -> Self {
        let text_layout = TextLayout::new("");
        Editor { text_layout }
    }
}

impl<T: Data> Widget<T> for Editor {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        Size::new(500.0, 1000.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let mut layout = TextLayout::new("abc sldkjfdslkjf");
        layout.rebuild_if_needed(&mut ctx.text(), env);
        layout.draw(ctx, Point::new(10.0, 10.0));
    }
}
