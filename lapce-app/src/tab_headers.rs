use crate::config::LapceConfig;
use crate::config::color::LapceColor;
use floem::event::{Event, EventListener, EventPropagation};
use floem::kurbo::Size;
use floem::prelude::*;
use floem::reactive::{ReadSignal, Scope};
use floem::style::CursorStyle;
use floem::taffy::AbsoluteAxis;
use floem::views::{Decorators, dyn_stack, empty};
use floem::{IntoView, View};
use im::HashMap;
use std::cell::{Cell, RefCell};
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

type Order<K> = Rc<RefCell<HashMap<K, (RwSignal<usize>, Cell<bool>)>>>;

fn enumerate_each_fn<I, T, K>(
    order: &Order<K>,
    each_fn: Rc<dyn Fn() -> I>,
    key_fn: Rc<dyn Fn(&T) -> K + 'static>,
) -> impl IntoIterator<Item = (RwSignal<usize>, T)> + use<I, T, K>
where
    I: IntoIterator<Item = T>,
    T: 'static,
    K: Eq + Hash + Clone + 'static,
{
    let order = order.clone();
    let scope = Scope::new();

    each_fn()
        .into_iter()
        .enumerate()
        .map({
            let order = order.clone();

            move |(i, v)| {
                let mut order = order.borrow_mut();
                let key = key_fn(&v);

                let i_signal = if let Some((i_signal, hit)) = order.get_mut(&key) {
                    if i_signal.get_untracked() != i {
                        i_signal.set(i);
                    }

                    // mark as seen to avoid clean up
                    hit.set(true);

                    *i_signal
                } else {
                    let i_signal = scope.create_rw_signal(i);

                    // remember for later
                    order.insert(key, (i_signal, Cell::new(true)));

                    i_signal
                };

                (i_signal, v)
            }
        })
        .on_drop(move || {
            let mut order = order.borrow_mut();

            order.retain(|_, (_, seen)| {
                // only keep keys that were seen in the previous loop
                let keep = seen.get();

                // reset for the next cycle
                seen.set(false);

                keep
            });
        })
}

#[allow(clippy::too_many_arguments)]
pub fn dyn_reorderable<TabGroup, I, T, K, V>(
    axis: AbsoluteAxis,
    tab_group: TabGroup,
    dragging: RwSignal<Option<(TabGroup, usize)>>,
    config: ReadSignal<Arc<LapceConfig>>,
    each_fn: impl Fn() -> I + 'static,
    key_fn: impl Fn(&T) -> K + 'static,
    swap_fn: impl Fn((TabGroup, usize), (TabGroup, usize)) + 'static,
    view_fn: impl Fn(T) -> V + 'static,
) -> impl View
where
    TabGroup: Clone + Copy + 'static,
    I: IntoIterator<Item = T>,
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
{
    let key_fn = Rc::new(key_fn);
    let each_fn = {
        let order = Order::default();
        let each_fn = Rc::new(each_fn);
        let key_fn = key_fn.clone();

        move || enumerate_each_fn(&order.clone(), each_fn.clone(), key_fn.clone())
    };
    let key_fn = move |(_, value): &(RwSignal<usize>, T)| key_fn(value);

    let swap_fn = Rc::new(swap_fn);

    let view_fn = move |(i, value): (RwSignal<usize>, T)| {
        let drag_over_prev: RwSignal<Option<bool>> = create_rw_signal(None);
        let tab_size = create_rw_signal(Size::ZERO);
        let swap_fn = swap_fn.clone();

        stack((
            view_fn(value)
                .draggable()
                .on_event_stop(EventListener::DragStart, move |_| {
                    dragging.set(Some((tab_group, i.get_untracked())));
                })
                .on_event_stop(EventListener::DragEnd, move |_| {
                    dragging.set(None);
                }),
            drop_indicator(
                axis,
                i.read_only(),
                tab_size.read_only(),
                drag_over_prev.read_only(),
                config,
            ),
        ))
        .style(|s| s.cursor(CursorStyle::Pointer))
        .on_resize(move |rect| {
            tab_size.set(rect.size());
        })
        .on_event_stop(EventListener::DragOver, move |event| {
            let Event::PointerMove(pointer_event) = event else {
                return;
            };

            if dragging.with_untracked(|dragging| dragging.is_none()) {
                return;
            }

            let new_prev = if axis == AbsoluteAxis::Horizontal {
                pointer_event.pos.x < tab_size.get_untracked().width / 2.0
            } else {
                pointer_event.pos.y < tab_size.get_untracked().height / 2.0
            };

            if drag_over_prev.get_untracked() != Some(new_prev) {
                drag_over_prev.set(Some(new_prev));
            }
        })
        .on_event(EventListener::Drop, move |event| {
            let Some(from) = dragging.get_untracked() else {
                return EventPropagation::Continue;
            };

            drag_over_prev.set(None);

            if let Event::PointerUp(pointer_event) = event {
                let left = if axis == AbsoluteAxis::Horizontal {
                    pointer_event.pos.x < tab_size.get_untracked().width / 2.0
                } else {
                    pointer_event.pos.y < tab_size.get_untracked().height / 2.0
                };

                let index = i.get_untracked();
                let new_index = if left { index } else { index + 1 };
                swap_fn(from, (tab_group, new_index));
            }

            EventPropagation::Stop
        })
        .on_event_stop(EventListener::DragLeave, move |_| {
            drag_over_prev.set(None);
        })
    };

    dyn_stack(each_fn, key_fn, view_fn).debug_name("Tab Stack")
}

fn drop_indicator(
    axis: AbsoluteAxis,
    i: ReadSignal<usize>,
    tab_size: ReadSignal<Size>,
    drag_over_prev: ReadSignal<Option<bool>>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    if axis == AbsoluteAxis::Horizontal {
        empty()
            .style(move |s| {
                let i = i.get();
                let drag_over_prev = drag_over_prev.get();

                s.absolute()
                    .margin_left(if i == 0 { 0.0 } else { -2.0 })
                    .height_full()
                    .width(
                        tab_size.get().width as f32 + if i == 0 { 1.0 } else { 3.0 },
                    )
                    .apply_if(drag_over_prev.is_none(), |s| s.hide())
                    .apply_if(drag_over_prev.is_some(), |s| {
                        if let Some(drag_over_prev) = drag_over_prev {
                            if drag_over_prev {
                                s.border_left(3.0)
                            } else {
                                s.border_right(3.0)
                            }
                        } else {
                            s
                        }
                    })
                    .border_color(
                        config
                            .get()
                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                            .multiply_alpha(0.5),
                    )
            })
            .debug_name("Drop Indicator")
    } else {
        empty()
            .style(move |s| {
                let i = i.get();
                let drag_over_prev = drag_over_prev.get();

                s.absolute()
                    .margin_top(if i == 0 { 0.0 } else { -2.0 })
                    .width_full()
                    .height(
                        tab_size.get().height as f32
                            + if i == 0 { 1.0 } else { 3.0 },
                    )
                    .apply_if(drag_over_prev.is_none(), |s| s.hide())
                    .apply_if(drag_over_prev.is_some(), |s| {
                        if let Some(drag_over_prev) = drag_over_prev {
                            if drag_over_prev {
                                s.border_top(3.0)
                            } else {
                                s.border_bottom(3.0)
                            }
                        } else {
                            s
                        }
                    })
                    .border_color(
                        config
                            .get()
                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                            .multiply_alpha(0.5),
                    )
            })
            .debug_name("Drop Indicator")
    }
}

pub fn dyn_h_reorderable<TabGroup, I, T, K, V>(
    tab_group: TabGroup,
    dragging: RwSignal<Option<(TabGroup, usize)>>,
    config: ReadSignal<Arc<LapceConfig>>,
    each_fn: impl Fn() -> I + 'static,
    key_fn: impl Fn(&T) -> K + 'static,
    swap_fn: impl Fn((TabGroup, usize), (TabGroup, usize)) + 'static,
    view_fn: impl Fn(T) -> V + 'static,
) -> impl View
where
    TabGroup: Clone + Copy + 'static,
    I: IntoIterator<Item = T>,
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
{
    dyn_reorderable(
        AbsoluteAxis::Horizontal,
        tab_group,
        dragging,
        config,
        each_fn,
        key_fn,
        swap_fn,
        view_fn,
    )
}

struct OnIterDrop<I, F: FnOnce()> {
    iter: I,
    on_drop: Option<F>,
}

impl<I: Iterator, F: FnOnce()> Iterator for OnIterDrop<I, F> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<I, F: FnOnce()> Drop for OnIterDrop<I, F> {
    fn drop(&mut self) {
        if let Some(on_drop) = self.on_drop.take() {
            on_drop()
        }
    }
}

trait OnIterDropTrait: Iterator {
    fn on_drop<F: FnOnce()>(self, f: F) -> OnIterDrop<Self, F>
    where
        Self: std::marker::Sized,
    {
        OnIterDrop {
            iter: self,
            on_drop: Some(f),
        }
    }
}

impl<T: Iterator> OnIterDropTrait for T {}
