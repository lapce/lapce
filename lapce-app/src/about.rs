use std::rc::Rc;

use floem::{
    event::EventListener,
    keyboard::ModifiersState,
    reactive::{RwSignal, Scope},
    style::{CursorStyle, Display, Position},
    view::View,
    views::{container, label, stack, svg, Decorators},
};
use lapce_core::{command::FocusCommand, meta::VERSION, mode::Mode};

use crate::{
    command::{CommandExecuted, CommandKind},
    config::color::LapceColor,
    keypress::KeyPressFocus,
    web_link::web_link,
    window_tab::{Focus, WindowTabData},
};

struct AboutUri {}

impl AboutUri {
    const LAPCE: &'static str = "https://lapce.dev";
    const GITHUB: &'static str = "https://github.com/lapce/lapce";
    const MATRIX: &'static str = "https://matrix.to/#/#lapce-editor:matrix.org";
    const DISCORD: &'static str = "https://discord.gg/n8tGJ6Rn6D";
    const CODICONS: &'static str = "https://github.com/microsoft/vscode-codicons";
}

#[derive(Clone)]
pub struct AboutData {
    pub visible: RwSignal<bool>,
    pub focus: RwSignal<Focus>,
}

impl AboutData {
    pub fn new(cx: Scope, focus: RwSignal<Focus>) -> Self {
        let visible = cx.create_rw_signal(false);

        Self { visible, focus }
    }

    pub fn open(&self) {
        self.visible.set(true);
        self.focus.set(Focus::AboutPopup);
    }

    pub fn close(&self) {
        self.visible.set(false);
        self.focus.set(Focus::Workbench);
    }
}

impl KeyPressFocus for AboutData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(
        &self,
        _condition: crate::keypress::condition::Condition,
    ) -> bool {
        self.visible.get_untracked()
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: ModifiersState,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Edit(_) => {}
            CommandKind::Move(_) => {}
            CommandKind::Focus(cmd) => {
                if cmd == &FocusCommand::ModalClose {
                    self.close();
                }
            }
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, _c: &str) {}

    fn focus_only(&self) -> bool {
        true
    }
}

pub fn about_popup(window_tab_data: Rc<WindowTabData>) -> impl View {
    let about_data = window_tab_data.about_data.clone();
    let config = window_tab_data.common.config;
    let internal_command = window_tab_data.common.internal_command;
    let logo_size = 100.0;

    exclusive_popup(window_tab_data, about_data.visible, move || {
        stack((
            svg(move || (config.get()).logo_svg()).style(move |s| {
                s.size(logo_size, logo_size)
                    .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
            }),
            label(|| "Lapce".to_string()).style(move |s| {
                s.font_bold()
                    .margin_top(10.0)
                    .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
            }),
            label(|| format!("Version: {}", VERSION)).style(move |s| {
                s.margin_top(10.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
            }),
            web_link(
                || "Website".to_string(),
                || AboutUri::LAPCE.to_string(),
                move || config.get().color(LapceColor::EDITOR_LINK),
                internal_command,
            )
            .style(|s| s.margin_top(20.0)),
            web_link(
                || "GitHub".to_string(),
                || AboutUri::GITHUB.to_string(),
                move || config.get().color(LapceColor::EDITOR_LINK),
                internal_command,
            )
            .style(|s| s.margin_top(10.0)),
            web_link(
                || "Discord".to_string(),
                || AboutUri::DISCORD.to_string(),
                move || config.get().color(LapceColor::EDITOR_LINK),
                internal_command,
            )
            .style(|s| s.margin_top(10.0)),
            web_link(
                || "Matrix".to_string(),
                || AboutUri::MATRIX.to_string(),
                move || config.get().color(LapceColor::EDITOR_LINK),
                internal_command,
            )
            .style(|s| s.margin_top(10.0)),
            label(|| "Attributions".to_string()).style(move |s| {
                s.font_bold()
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .margin_top(40.0)
            }),
            web_link(
                || "Codicons (CC-BY-4.0)".to_string(),
                || AboutUri::CODICONS.to_string(),
                move || config.get().color(LapceColor::EDITOR_LINK),
                internal_command,
            )
            .style(|s| s.margin_top(10.0)),
        ))
        .style(|s| s.flex_col().items_center())
    })
}

fn exclusive_popup<V: View + 'static>(
    window_tab_data: Rc<WindowTabData>,
    visibility: RwSignal<bool>,
    content: impl FnOnce() -> V,
) -> impl View {
    let config = window_tab_data.common.config;

    container(
        container(
            container(content())
                .style(move |s| {
                    let config = config.get();
                    s.padding_vert(25.0)
                        .padding_horiz(100.0)
                        .border(1.0)
                        .border_radius(6.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                        .background(config.color(LapceColor::PANEL_BACKGROUND))
                })
                .on_event_stop(EventListener::PointerDown, move |_| {}),
        )
        .style(move |s| {
            s.flex_grow(1.0)
                .flex_row()
                .items_center()
                .hover(move |s| s.cursor(CursorStyle::Default))
        }),
    )
    .on_event_stop(EventListener::PointerDown, move |_| {
        window_tab_data.about_data.close();
    })
    // Prevent things behind the grayed out area from being hovered.
    .on_event_stop(EventListener::PointerMove, move |_| {})
    .style(move |s| {
        s.display(if visibility.get() {
            Display::Flex
        } else {
            Display::None
        })
        .position(Position::Absolute)
        .size_pct(100.0, 100.0)
        .flex_col()
        .items_center()
        .background(
            config
                .get()
                .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                .with_alpha_factor(0.5),
        )
    })
}
