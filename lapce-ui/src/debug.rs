use std::sync::Arc;

use druid::{
    piet::{Text, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx,
    Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
    debug::{
        DapData, DebugAction, RunAction, RunDebugAction, RunDebugData, RunDebugMode,
        RunDebugProcess,
    },
    panel::PanelKind,
};
use lapce_rpc::terminal::TermId;

use crate::{
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::LapceScroll,
};

pub struct DebugProcessList {
    line_height: f64,
    mouse_down: Option<Point>,
    mouse_pos: Point,
    mouse_process: Option<TermId>,
}

#[derive(Clone, Debug)]
struct ProcessIcon {
    svg: &'static str,
    color: &'static str,
    action: RunDebugAction,
    active: bool,
}

pub fn new_debug_panel(data: &RunDebugData) -> LapcePanel {
    LapcePanel::new(
        PanelKind::Debug,
        data.widget_id,
        data.split_id,
        vec![(
            WidgetId::next(),
            PanelHeaderKind::Simple("Processes".into()),
            LapceScroll::new(DebugProcessList::new()).boxed(),
            PanelSizing::Size(200.0),
        )],
    )
}

impl DebugProcessList {
    fn new() -> Self {
        DebugProcessList {
            line_height: 25.0,
            mouse_down: None,
            mouse_pos: Point::ZERO,
            mouse_process: None,
        }
    }

    fn process_icons(
        process: &RunDebugProcess,
        dap: Option<&DapData>,
    ) -> Vec<ProcessIcon> {
        match process.mode {
            RunDebugMode::Run => vec![
                ProcessIcon {
                    svg: LapceIcons::DEBUG_RESTART,
                    color: LapceTheme::LAPCE_ICON_ACTIVE,
                    action: RunDebugAction::Run(RunAction::Restart),
                    active: true,
                },
                ProcessIcon {
                    svg: LapceIcons::DEBUG_STOP,
                    color: if process.stopped {
                        LapceTheme::LAPCE_ICON_INACTIVE
                    } else {
                        LapceTheme::LAPCE_ICON_ACTIVE
                    },
                    action: RunDebugAction::Run(RunAction::Stop),
                    active: !process.stopped,
                },
                ProcessIcon {
                    svg: LapceIcons::CLOSE,
                    color: LapceTheme::LAPCE_ICON_ACTIVE,
                    action: RunDebugAction::Run(RunAction::Close),
                    active: true,
                },
            ],
            RunDebugMode::Debug => {
                let paused = dap.map(|dap| dap.stopped).unwrap_or(false);
                let stopped = process.stopped;
                vec![
                    ProcessIcon {
                        svg: LapceIcons::DEBUG_CONTINUE,
                        color: if paused && !stopped {
                            LapceTheme::LAPCE_ICON_ACTIVE
                        } else {
                            LapceTheme::LAPCE_ICON_INACTIVE
                        },
                        action: RunDebugAction::Debug(DebugAction::Continue),
                        active: paused && !stopped,
                    },
                    ProcessIcon {
                        svg: LapceIcons::DEBUG_STOP,
                        color: if stopped {
                            LapceTheme::LAPCE_ICON_INACTIVE
                        } else {
                            LapceTheme::LAPCE_ICON_ACTIVE
                        },
                        action: RunDebugAction::Debug(DebugAction::Stop),
                        active: !stopped,
                    },
                ]
            }
        }
    }

    fn clicked_icon(
        &self,
        data: &LapceTabData,
        width: f64,
        mouse_down: Point,
        mouse_up: Point,
    ) -> Option<(TermId, Option<ProcessIcon>)> {
        let down_line = (mouse_down.y / self.line_height).floor() as usize;
        let up_line = (mouse_up.y / self.line_height).floor() as usize;
        if down_line != up_line {
            return None;
        }

        let processes = data.terminal.run_debug_process();
        let (term_id, process) = processes.get(up_line)?;
        let dap = data.terminal.debug.daps.get(&process.config.dap_id);

        let mut icons = Self::process_icons(process, dap);
        icons.reverse();

        let down_icon = ((width - mouse_down.x) / self.line_height).floor() as usize;
        let up_icon = ((width - mouse_up.x) / self.line_height).floor() as usize;
        if up_icon > icons.len() && down_icon > icons.len() {
            return Some((*term_id, None));
        }

        if down_icon != up_icon {
            return None;
        }

        Some((*term_id, icons.get(up_icon).cloned()))
    }
}

impl Widget<LapceTabData> for DebugProcessList {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                ctx.clear_cursor();
                let process = if let Some((term_id, icon)) = self.clicked_icon(
                    data,
                    ctx.size().width,
                    mouse_event.pos,
                    mouse_event.pos,
                ) {
                    if let Some(icon) = icon {
                        if icon.active {
                            ctx.set_cursor(&Cursor::Pointer);
                        }
                    } else {
                        ctx.set_cursor(&Cursor::Pointer);
                    }
                    Some(term_id)
                } else {
                    None
                };
                if process != self.mouse_process {
                    ctx.request_paint();
                    self.mouse_process = process;
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down = Some(mouse_event.pos);
            }
            Event::MouseUp(mouse_event) => {
                if let Some(mouse_down) = self.mouse_down {
                    if let Some((term_id, icon)) = self.clicked_icon(
                        data,
                        ctx.size().width,
                        mouse_down,
                        mouse_event.pos,
                    ) {
                        if let Some(icon) = icon {
                            if icon.active {
                                match icon.action {
                                    RunDebugAction::Run(RunAction::Close)
                                    | RunDebugAction::Debug(DebugAction::Close) => {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::CloseTerminal(term_id),
                                            Target::Widget(data.id),
                                        ));
                                        return;
                                    }
                                    RunDebugAction::Run(RunAction::Restart)
                                    | RunDebugAction::Debug(DebugAction::Restart) => {
                                        if let Some(terminal) =
                                            Arc::make_mut(&mut data.terminal)
                                                .get_terminal_mut(&term_id)
                                        {
                                            Arc::make_mut(terminal)
                                                .restart_run_debug(&data.config);
                                        }
                                    }
                                    RunDebugAction::Run(RunAction::Stop) => {
                                        if let Some(terminal) =
                                            Arc::make_mut(&mut data.terminal)
                                                .get_terminal_mut(&term_id)
                                        {
                                            Arc::make_mut(terminal).stop_run_debug();
                                        }
                                    }
                                    RunDebugAction::Debug(DebugAction::Stop) => {
                                        data.terminal.dap_stop(term_id);
                                    }
                                    RunDebugAction::Debug(DebugAction::Continue) => {
                                        data.terminal.dap_continue(term_id);
                                    }
                                }
                            }
                        } else {
                            let terminal = Arc::make_mut(&mut data.terminal);
                            let debug = Arc::make_mut(&mut terminal.debug);
                            debug.active_term = Some(term_id);
                        }
                        if let Some(terminal) = data.terminal.get_terminal(&term_id)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(terminal.widget_id),
                            ));
                        }
                    }
                }
                self.mouse_down = None;
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let mut n = 0;
        for (_, tab) in &data.terminal.tabs {
            for (_, terminal) in &tab.terminals {
                if terminal.run_debug.is_some() {
                    n += 1;
                }
            }
        }
        Size::new(
            bc.max().width,
            bc.max().height.max(self.line_height * n as f64),
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();

        let processes = data.terminal.run_debug_process();

        let mouse_at_line = (self.mouse_pos.y / self.line_height).floor() as usize;
        for (i, (term_id, process)) in processes.into_iter().enumerate() {
            if data.terminal.debug.active_term == Some(term_id) {
                ctx.fill(
                    Size::new(size.width, self.line_height)
                        .to_rect()
                        .with_origin(Point::new(0.0, i as f64 * self.line_height)),
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_CURRENT_BACKGROUND),
                );
            }

            let icon_size = data.config.ui.icon_size() as f64;
            let icon_rect = Rect::ZERO
                .with_origin(Point::new(
                    self.line_height / 2.0,
                    i as f64 * self.line_height + self.line_height / 2.0,
                ))
                .inflate(icon_size / 2.0, icon_size / 2.0);

            let color = if !process.stopped {
                LapceTheme::LAPCE_ICON_ACTIVE
            } else {
                LapceTheme::LAPCE_ICON_INACTIVE
            };
            let svg = match process.mode {
                RunDebugMode::Run => LapceIcons::START,
                RunDebugMode::Debug => LapceIcons::DEBUG,
            };

            let icons = if data.terminal.debug.active_term == Some(term_id)
                || (ctx.is_hot() && mouse_at_line == i)
            {
                let dap = data.terminal.debug.daps.get(&process.config.dap_id);
                Self::process_icons(process, dap)
            } else {
                Vec::new()
            };
            let icon_start_x = size.width - self.line_height * icons.len() as f64;

            ctx.with_save(|ctx| {
                ctx.clip(
                    Rect::ZERO
                        .with_size(Size::new(icon_start_x, size.height))
                        .with_origin(Point::new(0.0, self.line_height * i as f64)),
                );
                ctx.draw_svg(
                    &data.config.ui_svg(svg),
                    icon_rect,
                    Some(data.config.get_color_unchecked(color)),
                );
                let text_layout = ctx
                    .text()
                    .new_text_layout(process.config.name.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::PANEL_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        self.line_height,
                        (i as f64 * self.line_height)
                            + text_layout.y_offset(self.line_height),
                    ),
                );
            });

            let mut x = icon_start_x;
            for icon in icons {
                let rect = Rect::ZERO
                    .with_origin(Point::new(
                        x + self.line_height / 2.0,
                        i as f64 * self.line_height + self.line_height / 2.0,
                    ))
                    .inflate(icon_size / 2.0, icon_size / 2.0);

                ctx.draw_svg(
                    &data.config.ui_svg(icon.svg),
                    rect,
                    Some(data.config.get_color_unchecked(icon.color)),
                );

                x += self.line_height;
            }
        }
    }
}
