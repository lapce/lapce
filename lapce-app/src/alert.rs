use std::{
    fmt,
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
};

use floem::{
    event::EventListener,
    reactive::{ReadSignal, RwSignal, Scope},
    style::{CursorStyle, Style},
    view::View,
    views::{container, label, list, stack, svg, Decorators},
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
    pub fn new(cx: Scope, common: CommonData) -> Self {
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

    container(|| {
        container(|| {
            stack(|| {
                (
                    svg(move || config.get().ui_svg(LapceIcons::WARNING)).style(
                        move || {
                            Style::BASE.size_px(50.0, 50.0).color(
                                *config.get().get_color(LapceColor::LAPCE_WARN),
                            )
                        },
                    ),
                    label(move || title.get()).style(move || {
                        Style::BASE
                            .margin_top_px(20.0)
                            .width_pct(100.0)
                            .font_bold()
                            .font_size((config.get().ui.font_size() + 1) as f32)
                    }),
                    label(move || msg.get()).style(move || {
                        Style::BASE.width_pct(100.0).margin_top_px(10.0)
                    }),
                    list(
                        move || buttons.get(),
                        move |_button| {
                            button_id
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        },
                        move |button| {
                            label(move || button.text.clone())
                                .on_click(move |_| {
                                    (button.action)();
                                    true
                                })
                                .style(move || {
                                    let config = config.get();
                                    Style::BASE
                                        .margin_top_px(10.0)
                                        .width_pct(100.0)
                                        .justify_center()
                                        .font_size(
                                            (config.ui.font_size() + 1) as f32,
                                        )
                                        .line_height(1.6)
                                        .border(1.0)
                                        .border_radius(6.0)
                                        .border_color(
                                            *config
                                                .get_color(LapceColor::LAPCE_BORDER),
                                        )
                                })
                                .hover_style(move || {
                                    Style::BASE
                                        .cursor(CursorStyle::Pointer)
                                        .background(*config.get().get_color(
                                            LapceColor::PANEL_HOVERED_BACKGROUND,
                                        ))
                                })
                                .active_style(move || {
                                    Style::BASE.background(*config.get().get_color(
                                        LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                    ))
                                })
                        },
                    )
                    .style(|| {
                        Style::BASE.flex_col().width_pct(100.0).margin_top_px(10.0)
                    }),
                    label(|| "Cancel".to_string())
                        .on_click(move |_| {
                            active.set(false);
                            true
                        })
                        .style(move || {
                            let config = config.get();
                            Style::BASE
                                .margin_top_px(20.0)
                                .width_pct(100.0)
                                .justify_center()
                                .font_size((config.ui.font_size() + 1) as f32)
                                .line_height(1.5)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    *config.get_color(LapceColor::LAPCE_BORDER),
                                )
                        })
                        .hover_style(move || {
                            Style::BASE.cursor(CursorStyle::Pointer).background(
                                *config
                                    .get()
                                    .get_color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .active_style(move || {
                            Style::BASE.background(*config.get().get_color(
                                LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                            ))
                        }),
                )
            })
            .style(|| Style::BASE.flex_col().items_center().width_pct(100.0))
        })
        .on_event(EventListener::PointerDown, |_| true)
        .style(move || {
            let config = config.get();
            Style::BASE
                .padding_px(20.0)
                .width_px(250.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
                .color(*config.get_color(LapceColor::EDITOR_FOREGROUND))
                .background(*config.get_color(LapceColor::PANEL_BACKGROUND))
        })
    })
    .on_event(EventListener::PointerDown, move |_| true)
    .style(move || {
        Style::BASE
            .absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(!active.get(), |s| s.hide())
            .background(
                config
                    .get()
                    .get_color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                    .with_alpha_factor(0.5),
            )
    })
}
