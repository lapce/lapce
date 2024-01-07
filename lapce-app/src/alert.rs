use std::{
    fmt,
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
};

use floem::{
    event::EventListener,
    reactive::{ReadSignal, RwSignal, Scope},
    style::CursorStyle,
    view::View,
    views::{container, dyn_stack, label, stack, svg, Decorators},
};

use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct AlertButton {
    pub text: String,
    pub action: Rc<dyn Fn()>,
}

impl fmt::Debug for AlertButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("AlertButton");
        s.field("text", &self.text);
        s.finish()
    }
}

#[derive(Clone)]
pub struct AlertBoxData {
    pub active: RwSignal<bool>,
    pub title: RwSignal<String>,
    pub msg: RwSignal<String>,
    pub buttons: RwSignal<Vec<AlertButton>>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl AlertBoxData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            title: cx.create_rw_signal("".to_string()),
            msg: cx.create_rw_signal("".to_string()),
            buttons: cx.create_rw_signal(Vec::new()),
            config: common.config,
        }
    }
}

pub fn alert_box(alert_data: AlertBoxData) -> impl View {
    let config = alert_data.config;
    let active = alert_data.active;
    let title = alert_data.title;
    let msg = alert_data.msg;
    let buttons = alert_data.buttons;
    let button_id = AtomicU64::new(0);

    container({
        container({
            stack((
                svg(move || config.get().ui_svg(LapceIcons::WARNING)).style(
                    move |s| {
                        s.size(50.0, 50.0)
                            .color(config.get().color(LapceColor::LAPCE_WARN))
                    },
                ),
                label(move || title.get()).style(move |s| {
                    s.margin_top(20.0)
                        .width_pct(100.0)
                        .font_bold()
                        .font_size((config.get().ui.font_size() + 1) as f32)
                }),
                label(move || msg.get())
                    .style(move |s| s.width_pct(100.0).margin_top(10.0)),
                dyn_stack(
                    move || buttons.get(),
                    move |_button| {
                        button_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    },
                    move |button| {
                        label(move || button.text.clone())
                            .on_click_stop(move |_| {
                                (button.action)();
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.margin_top(10.0)
                                    .width_pct(100.0)
                                    .justify_center()
                                    .font_size((config.ui.font_size() + 1) as f32)
                                    .line_height(1.6)
                                    .border(1.0)
                                    .border_radius(6.0)
                                    .border_color(
                                        config.color(LapceColor::LAPCE_BORDER),
                                    )
                                    .hover(|s| {
                                        s.cursor(CursorStyle::Pointer).background(
                                            config.color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            ),
                                        )
                                    })
                                    .active(|s| {
                                        s.background(config.color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                                    })
                            })
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0).margin_top(10.0)),
                label(|| "Cancel".to_string())
                    .on_click_stop(move |_| {
                        active.set(false);
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.margin_top(20.0)
                            .width_pct(100.0)
                            .justify_center()
                            .font_size((config.ui.font_size() + 1) as f32)
                            .line_height(1.5)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                )
                            })
                            .active(|s| {
                                s.background(config.color(
                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            })
                    }),
            ))
            .style(|s| s.flex_col().items_center().width_pct(100.0))
        })
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            let config = config.get();
            s.padding(20.0)
                .width(250.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        })
    })
    .on_event_stop(EventListener::PointerDown, move |_| {})
    .style(move |s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(!active.get(), |s| s.hide())
            .background(
                config
                    .get()
                    .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                    .with_alpha_factor(0.5),
            )
    })
}
