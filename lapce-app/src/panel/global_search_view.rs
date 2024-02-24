use std::{path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    event::EventListener,
    reactive::ReadSignal,
    style::{CursorStyle, Style},
    view::View,
    views::{
        container, label, scroll, stack, svg, virtual_stack, Decorators,
        VirtualDirection, VirtualItemSize,
    },
};
use lapce_xi_rope::find::CaseMatching;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    editor::location::{EditorLocation, EditorPosition},
    focus_text::focus_text,
    global_search::{GlobalSearchData, SearchMatchData},
    listener::Listener,
    text_input::text_input,
    window_tab::{Focus, WindowTabData},
    workspace::LapceWorkspace,
};

pub fn global_search_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let global_search = window_tab_data.global_search.clone();
    let editor = global_search.editor.clone();
    let config = global_search.common.config;
    let workspace = global_search.common.workspace.clone();
    let internal_command = global_search.common.internal_command;
    let case_matching = global_search.common.find.case_matching;
    let whole_word = global_search.common.find.whole_words;
    let is_regex = global_search.common.find.is_regex;

    let focus = global_search.common.focus;
    let is_focused = move || focus.get() == Focus::Panel(PanelKind::Search);

    stack((
        container(
            stack((
                text_input(editor, is_focused).style(|s| s.width_pct(100.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_CASE_SENSITIVE,
                    move || {
                        let new = match case_matching.get_untracked() {
                            CaseMatching::Exact => CaseMatching::CaseInsensitive,
                            CaseMatching::CaseInsensitive => CaseMatching::Exact,
                        };
                        case_matching.set(new);
                    },
                    move || case_matching.get() == CaseMatching::Exact,
                    || false,
                    config,
                )
                .style(|s| s.padding_vert(4.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_WHOLE_WORD,
                    move || {
                        whole_word.update(|whole_word| {
                            *whole_word = !*whole_word;
                        });
                    },
                    move || whole_word.get(),
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || LapceIcons::SEARCH_REGEX,
                    move || {
                        is_regex.update(|is_regex| {
                            *is_regex = !*is_regex;
                        });
                    },
                    move || is_regex.get(),
                    || false,
                    config,
                )
                .style(|s| s.padding_left(6.0)),
            ))
            .on_event_cont(EventListener::PointerDown, move |_| {
                focus.set(Focus::Panel(PanelKind::Search));
            })
            .style(move |s| {
                s.width_pct(100.0)
                    .padding_right(6.0)
                    .items_center()
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.get().color(LapceColor::LAPCE_BORDER))
            }),
        )
        .style(|s| s.width_pct(100.0).padding(10.0)),
        search_result(workspace, global_search, internal_command, config),
    ))
    .style(|s| s.absolute().size_pct(100.0, 100.0).flex_col())
}

fn search_result(
    workspace: Arc<LapceWorkspace>,
    global_search_data: GlobalSearchData,
    internal_command: Listener<InternalCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let ui_line_height = global_search_data.common.ui_line_height;
    container({
        scroll({
            virtual_stack(
                VirtualDirection::Vertical,
                VirtualItemSize::Fn(Box::new(
                    |(_, match_data): &(PathBuf, SearchMatchData)| {
                        match_data.height()
                    },
                )),
                move || global_search_data.clone(),
                move |(path, _)| path.to_owned(),
                move |(path, match_data)| {
                    let full_path = path.clone();
                    let path = if let Some(workspace_path) = workspace.path.as_ref()
                    {
                        path.strip_prefix(workspace_path)
                            .unwrap_or(&full_path)
                            .to_path_buf()
                    } else {
                        path
                    };
                    let style_path = path.clone();

                    let file_name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();

                    let folder = path
                        .parent()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();

                    let expanded = match_data.expanded;

                    stack((
                        stack((
                            svg(move || {
                                config.get().ui_svg(if expanded.get() {
                                    LapceIcons::ITEM_OPENED
                                } else {
                                    LapceIcons::ITEM_CLOSED
                                })
                            })
                            .style(move |s| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;
                                s.margin_left(10.0)
                                    .margin_right(6.0)
                                    .size(size, size)
                                    .min_size(size, size)
                                    .color(
                                        config.color(LapceColor::LAPCE_ICON_ACTIVE),
                                    )
                            }),
                            svg(move || config.get().file_svg(&path).0).style(
                                move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    let color = config.file_svg(&style_path).1;
                                    s.margin_right(6.0)
                                        .size(size, size)
                                        .min_size(size, size)
                                        .apply_opt(color, Style::color)
                                },
                            ),
                            stack((
                                label(move || file_name.clone()).style(|s| {
                                    s.margin_right(6.0)
                                        .max_width_pct(100.0)
                                        .text_ellipsis()
                                }),
                                label(move || folder.clone()).style(move |s| {
                                    s.color(
                                        config.get().color(LapceColor::EDITOR_DIM),
                                    )
                                    .min_width(0.0)
                                    .text_ellipsis()
                                }),
                            ))
                            .style(move |s| s.min_width(0.0).items_center()),
                        ))
                        .on_click_stop(move |_| {
                            expanded.update(|expanded| *expanded = !*expanded);
                        })
                        .style(move |s| {
                            s.width_pct(100.0)
                                .min_width_pct(100.0)
                                .items_center()
                                .hover(|s| {
                                    s.cursor(CursorStyle::Pointer).background(
                                        config.get().color(
                                            LapceColor::PANEL_HOVERED_BACKGROUND,
                                        ),
                                    )
                                })
                        }),
                        virtual_stack(
                            VirtualDirection::Vertical,
                            VirtualItemSize::Fixed(Box::new(move || {
                                ui_line_height.get()
                            })),
                            move || {
                                if expanded.get() {
                                    match_data.matches.get()
                                } else {
                                    im::Vector::new()
                                }
                            },
                            |m| (m.line, m.start, m.end),
                            move |m| {
                                let path = full_path.clone();
                                let line_number = m.line;
                                let start = m.start;
                                let end = m.end;
                                let line_content = m.line_content.clone();

                                focus_text(
                                    move || {
                                        let config = config.get();
                                        let content = if config
                                            .ui
                                            .trim_search_results_whitespace
                                        {
                                            m.line_content.trim()
                                        } else {
                                            &m.line_content
                                        };
                                        format!("{}: {content}", m.line,)
                                    },
                                    move || {
                                        let config = config.get();
                                        let mut offset = if config
                                            .ui
                                            .trim_search_results_whitespace
                                        {
                                            line_content.trim_start().len() as i32
                                                - line_content.len() as i32
                                        } else {
                                            0
                                        };
                                        offset +=
                                            line_number.to_string().len() as i32 + 2;

                                        ((start as i32 + offset) as usize
                                            ..(end as i32 + offset) as usize)
                                            .collect()
                                    },
                                    move || {
                                        config.get().color(LapceColor::EDITOR_FOCUS)
                                    },
                                )
                                .style(move |s| {
                                    let config = config.get();
                                    let icon_size = config.ui.icon_size() as f32;
                                    s.margin_left(10.0 + icon_size + 6.0).hover(
                                        |s| {
                                            s.cursor(CursorStyle::Pointer)
                                                .background(config.color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            ))
                                        },
                                    )
                                })
                                .on_click_stop(
                                    move |_| {
                                        internal_command.send(
                                            InternalCommand::JumpToLocation {
                                                location: EditorLocation {
                                                    path: path.clone(),
                                                    position: Some(
                                                        EditorPosition::Line(
                                                            line_number
                                                                .saturating_sub(1),
                                                        ),
                                                    ),
                                                    scroll_offset: None,
                                                    ignore_unconfirmed: false,
                                                    same_editor_tab: false,
                                                },
                                            },
                                        );
                                    },
                                )
                            },
                        )
                        .style(|s| s.flex_col()),
                    ))
                    .style(|s| s.flex_col())
                },
            )
            .style(|s| s.flex_col().min_width_pct(100.0).line_height(1.6))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .style(|s| s.size_pct(100.0, 100.0))
}
