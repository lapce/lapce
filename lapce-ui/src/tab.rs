use std::{process, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, InternalLifeCycle,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext,
    Selector, SingleUse, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
    WidgetPod,
};
use itertools::Itertools;
use lapce_core::{
    command::FocusCommand,
    cursor::{Cursor, CursorMode},
    language::LapceLanguage,
    selection::Selection,
};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_OPEN_FILE, LAPCE_OPEN_FOLDER, LAPCE_SAVE_FILE_AS,
        LAPCE_UI_COMMAND,
    },
    completion::CompletionStatus,
    config::{LapceConfig, LapceIcons, LapceTheme},
    data::{
        DragContent, EditorDiagnostic, EditorTabChild, FocusArea, LapceData,
        LapceTabData, LapceWindowData, LapceWorkspace, LapceWorkspaceType,
        WorkProgress,
    },
    document::{BufferContent, LocalBufferKind},
    editor::EditorLocation,
    hover::HoverStatus,
    keypress::{DefaultKeyPressHandler, KeyMap, KeyPressData},
    menu::MenuKind,
    palette::PaletteStatus,
    panel::{
        PanelContainerPosition, PanelKind, PanelPosition, PanelResizePosition,
        PanelStyle,
    },
    plugin::plugin_install_status::{PluginInstallStatus, PluginInstallType},
    proxy::path_from_url,
    signature::SignatureStatus,
};
use lapce_rpc::proxy::ProxyResponse;
use lapce_xi_rope::Rope;
use lsp_types::DiagnosticSeverity;

use crate::{
    about::AboutBox, alert::AlertBox, completion::CompletionContainer,
    editor::view::LapceEditorView, explorer::FileExplorer, hover::HoverContainer,
    message::LapceMessage, panel::PanelContainer, picker::FilePicker,
    plugin::Plugin, problem::new_problem_panel, scroll::LapceScroll,
    search::new_search_panel, signature::SignatureContainer,
    source_control::new_source_control_panel, split::split_data_widget,
    status::LapceStatus, terminal::TerminalPanel, title::Title,
};

pub const LAPCE_TAB_META: Selector<SingleUse<LapceTabMeta>> =
    Selector::new("lapce.tab_meta");

pub struct LapceTabMeta {
    pub data: LapceTabData,
    pub widget: WidgetPod<LapceWindowData, Box<dyn Widget<LapceWindowData>>>,
}

pub struct LapceIcon {
    pub rect: Rect,
    pub command: Command,
    pub icon: &'static str,
}

pub struct LapceButton {
    pub rect: Rect,
    pub command: Command,
    pub text_layout: PietTextLayout,
}

pub struct LapceTab {
    id: WidgetId,
    pub title: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    main_split: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    completion: WidgetPod<LapceTabData, CompletionContainer>,
    signature: WidgetPod<LapceTabData, SignatureContainer>,
    hover: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    rename: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    status: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    picker: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    about: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    alert: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    message: WidgetPod<LapceTabData, LapceScroll<LapceTabData, LapceMessage>>,
    panel_left: WidgetPod<LapceTabData, PanelContainer>,
    panel_bottom: WidgetPod<LapceTabData, PanelContainer>,
    panel_right: WidgetPod<LapceTabData, PanelContainer>,
    current_bar_hover: Option<PanelResizePosition>,
    width: f64,
    height: f64,
    title_height: f64,
    status_height: f64,
    mouse_pos: Point,
}

fn workspace_title(workspace: &LapceWorkspace) -> Option<String> {
    let p = workspace.path.as_ref()?;
    let dir = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
    Some(match &workspace.kind {
        LapceWorkspaceType::Local => format!("{dir}"),
        LapceWorkspaceType::RemoteSSH(ssh) => format!("{dir} [{ssh}]"),
        #[cfg(windows)]
        LapceWorkspaceType::RemoteWSL => format!("{dir} [wsl]"),
    })
}

impl LapceTab {
    pub fn new(data: &mut LapceTabData) -> Self {
        let title = WidgetPod::new(Title::new(data).boxed());
        let split_data = data
            .main_split
            .splits
            .get(&*data.main_split.split_id)
            .unwrap();
        let main_split = split_data_widget(split_data, data);

        let completion = CompletionContainer::new(&data.completion);
        let signature = SignatureContainer::new(&data.signature);
        let hover = HoverContainer::new(&data.hover);
        let rename =
            LapceEditorView::new(data.rename.view_id, data.rename.editor_id, None)
                .hide_header()
                .hide_gutter()
                .padding((10.0, 5.0, 10.0, 5.0));
        let status = LapceStatus::new();
        let picker = FilePicker::new(data);

        let about = AboutBox::new(data);
        let alert = AlertBox::new(data);
        let message = LapceScroll::new(LapceMessage::new(*data.message_widget_id));

        let mut panel_left = PanelContainer::new(PanelContainerPosition::Left);
        let mut panel_bottom = PanelContainer::new(PanelContainerPosition::Bottom);
        let mut panel_right = PanelContainer::new(PanelContainerPosition::Right);

        for (position, order) in data.panel.order.clone().iter() {
            let panel = match position {
                PanelPosition::LeftTop | PanelPosition::LeftBottom => {
                    &mut panel_left
                }
                PanelPosition::RightTop | PanelPosition::RightBottom => {
                    &mut panel_right
                }
                PanelPosition::BottomLeft | PanelPosition::BottomRight => {
                    &mut panel_bottom
                }
            };
            for kind in order.iter() {
                match kind {
                    PanelKind::FileExplorer => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(FileExplorer::new_panel(data).boxed()),
                        );
                    }
                    PanelKind::SourceControl => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(new_source_control_panel(data).boxed()),
                        );
                    }
                    PanelKind::Plugin => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(Plugin::new_panel(data).boxed()),
                        );
                    }
                    PanelKind::Terminal => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(TerminalPanel::new_panel(data).boxed()),
                        );
                    }
                    PanelKind::Search => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(new_search_panel(data).boxed()),
                        );
                    }
                    PanelKind::Problem => {
                        panel.insert_panel(
                            *kind,
                            WidgetPod::new(new_problem_panel(&data.problem).boxed()),
                        );
                    }
                }
            }
        }

        Self {
            id: data.id,
            title,
            main_split: WidgetPod::new(main_split.boxed()),
            completion: WidgetPod::new(completion),
            signature: WidgetPod::new(signature),
            hover: WidgetPod::new(hover.boxed()),
            rename: WidgetPod::new(rename.boxed()),
            picker: WidgetPod::new(picker.boxed()),
            status: WidgetPod::new(status.boxed()),
            about: WidgetPod::new(about.boxed()),
            alert: WidgetPod::new(alert.boxed()),
            message: WidgetPod::new(message),
            panel_left: WidgetPod::new(panel_left),
            panel_right: WidgetPod::new(panel_right),
            panel_bottom: WidgetPod::new(panel_bottom),
            current_bar_hover: None,
            width: 0.0,
            height: 0.0,
            status_height: 0.0,
            title_height: 0.0,
            mouse_pos: Point::ZERO,
        }
    }

    fn update_split_point(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_pos: Point,
    ) {
        if let Some(position) = self.current_bar_hover.as_ref() {
            match position {
                PanelResizePosition::Left => {
                    let maximum = self.width - 100.0 - data.panel.size.right;
                    Arc::make_mut(&mut data.panel).size.left =
                        mouse_pos.x.round().max(180.0).min(maximum);
                    if mouse_pos.x < 90.0 {
                        if data
                            .panel
                            .is_container_shown(&PanelContainerPosition::Left)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Workbench(
                                        LapceWorkbenchCommand::TogglePanelLeftVisual,
                                    ),
                                    data: None,
                                },
                                Target::Widget(data.id),
                            ));
                        }
                    } else if !data
                        .panel
                        .is_container_shown(&PanelContainerPosition::Left)
                    {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Workbench(
                                    LapceWorkbenchCommand::TogglePanelLeftVisual,
                                ),
                                data: None,
                            },
                            Target::Widget(data.id),
                        ));
                    }
                }
                PanelResizePosition::Right => {
                    let maximum = self.width - 100.0 - data.panel.size.left;
                    let right = self.width - mouse_pos.x.round();
                    Arc::make_mut(&mut data.panel).size.right =
                        right.max(180.0).min(maximum);
                    if right < 90.0 {
                        if data
                            .panel
                            .is_container_shown(&PanelContainerPosition::Right)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Workbench(
                                        LapceWorkbenchCommand::TogglePanelRightVisual,
                                    ),
                                    data: None,
                                },
                                Target::Widget(data.id),
                            ));
                        }
                    } else if !data
                        .panel
                        .is_container_shown(&PanelContainerPosition::Right)
                    {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Workbench(
                                    LapceWorkbenchCommand::TogglePanelRightVisual,
                                ),
                                data: None,
                            },
                            Target::Widget(data.id),
                        ));
                    }
                }
                PanelResizePosition::LeftSplit => (),
                PanelResizePosition::Bottom => {
                    let bottom =
                        self.height - mouse_pos.y.round() - self.status_height;

                    let header_height = data.config.ui.header_height() as f64;
                    // The maximum position (from the bottom) that the bottom split is allowed to reach
                    let minimum = self.height
                        - self.title_height
                        - self.status_height
                        - header_height
                        - 1.0;

                    Arc::make_mut(&mut data.panel).size.bottom =
                        bottom.max(180.0).min(minimum);

                    // Check if it should snap the bottom panel away, if you are too low
                    if bottom < 90.0 {
                        if data
                            .panel
                            .is_container_shown(&PanelContainerPosition::Bottom)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_COMMAND,
                                LapceCommand {
                                    kind: CommandKind::Workbench(
                                        LapceWorkbenchCommand::TogglePanelBottomVisual,
                                    ),
                                    data: None,
                                },
                                Target::Widget(data.id),
                            ));
                        }
                    } else if !data
                        .panel
                        .is_container_shown(&PanelContainerPosition::Bottom)
                    {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Workbench(
                                    LapceWorkbenchCommand::TogglePanelBottomVisual,
                                ),
                                data: None,
                            },
                            Target::Widget(data.id),
                        ));
                    }
                }
            }
        }
    }

    fn bar_hit_test(&self, mouse_pos: Point) -> Option<PanelResizePosition> {
        let rect = self.main_split.layout_rect();
        let left = rect.x0;
        let right = rect.x1;
        let bottom = rect.y1;

        if mouse_pos.x >= left - 2.0 && mouse_pos.x <= left + 2.0 {
            return Some(PanelResizePosition::Left);
        }

        if mouse_pos.x >= right - 2.0 && mouse_pos.x <= right + 2.0 {
            return Some(PanelResizePosition::Right);
        }

        if mouse_pos.x > left
            && mouse_pos.x < right
            && mouse_pos.y >= bottom - 2.0
            && mouse_pos.y <= bottom + 2.0
        {
            return Some(PanelResizePosition::Bottom);
        }

        None
    }

    fn panel_rects(&self) -> Vec<(PanelPosition, Rect)> {
        let mut rects = Vec::new();

        let left_rect = self.panel_left.layout_rect();
        let left_size = left_rect.size();
        let new_size = Size::new(left_size.width, (left_size.height / 2.0).round());
        rects.push((PanelPosition::LeftTop, left_rect.with_size(new_size)));
        rects.push((
            PanelPosition::LeftBottom,
            left_rect.with_size(new_size).with_origin(Point::new(
                left_rect.x0,
                left_rect.y0 + new_size.height,
            )),
        ));

        let right_rect = self.panel_right.layout_rect();
        let right_size = right_rect.size();
        let new_size =
            Size::new(right_size.width, (right_size.height / 2.0).round());
        rects.push((PanelPosition::RightTop, right_rect.with_size(new_size)));
        rects.push((
            PanelPosition::RightBottom,
            right_rect.with_size(new_size).with_origin(Point::new(
                right_rect.x0,
                right_rect.y0 + new_size.height,
            )),
        ));

        let bottom_rect = self.panel_bottom.layout_rect();
        let bottom_size = bottom_rect.size();
        let new_size =
            Size::new((bottom_size.width / 2.0).round(), bottom_size.height);
        rects.push((PanelPosition::BottomLeft, bottom_rect.with_size(new_size)));
        rects.push((
            PanelPosition::BottomRight,
            bottom_rect.with_size(new_size).with_origin(Point::new(
                bottom_rect.x0 + new_size.width,
                bottom_rect.y0,
            )),
        ));

        rects
    }

    fn move_panel(
        &mut self,
        ctx: &mut UpdateCtx,
        kind: PanelKind,
        from: PanelPosition,
        to: PanelPosition,
    ) {
        let (panel_widget_id, panel) = match from {
            PanelPosition::LeftTop | PanelPosition::LeftBottom => (
                self.panel_left.widget().widget_id,
                self.panel_left.widget_mut().panels.remove(&kind).unwrap(),
            ),
            PanelPosition::RightTop | PanelPosition::RightBottom => (
                self.panel_right.widget().widget_id,
                self.panel_right.widget_mut().panels.remove(&kind).unwrap(),
            ),
            PanelPosition::BottomLeft | PanelPosition::BottomRight => (
                self.panel_bottom.widget().widget_id,
                self.panel_bottom.widget_mut().panels.remove(&kind).unwrap(),
            ),
        };

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ChildrenChanged,
            Target::Widget(panel_widget_id),
        ));

        let new_panel_widget_id = match to {
            PanelPosition::LeftTop | PanelPosition::LeftBottom => {
                self.panel_left.widget_mut().panels.insert(kind, panel);
                self.panel_left.widget().widget_id
            }
            PanelPosition::RightTop | PanelPosition::RightBottom => {
                self.panel_right.widget_mut().panels.insert(kind, panel);
                self.panel_right.widget().widget_id
            }
            PanelPosition::BottomLeft | PanelPosition::BottomRight => {
                self.panel_bottom.widget_mut().panels.insert(kind, panel);
                self.panel_bottom.widget().widget_id
            }
        };

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ChildrenChanged,
            Target::Widget(new_panel_widget_id),
        ));
    }

    fn handle_panel_drop(&mut self, _ctx: &mut EventCtx, data: &mut LapceTabData) {
        if let Some((_, _, DragContent::Panel(kind, _))) = data.drag.as_ref() {
            let rects = self.panel_rects();
            for (p, rect) in rects.iter() {
                if !rect.contains(self.mouse_pos) {
                    continue;
                }

                let (_, from_position) = data.panel.panel_position(kind).unwrap();
                if from_position == *p {
                    return;
                }

                let panel = Arc::make_mut(&mut data.panel);
                if let Some(order) = panel.order.get_mut(&from_position) {
                    order.retain(|k| k != kind);
                }

                let order = panel.order.entry(*p).or_insert_with(im::Vector::new);

                order.push_back(*kind);

                let style = panel.style.entry(*p).or_insert(PanelStyle {
                    active: 0,
                    shown: true,
                    maximized: false,
                });

                style.active = order.len() - 1;
                style.shown = true;
                let _ = data.db.save_panel_orders(&panel.order);

                return;
            }
        }
    }

    fn paint_drag_on_panel(&self, ctx: &mut PaintCtx, data: &LapceTabData) {
        if let Some((_, _, DragContent::Panel(_, _))) = data.drag.as_ref() {
            let rects = self.panel_rects();
            for (_, rect) in rects.iter() {
                if !rect.contains(self.mouse_pos) {
                    continue;
                }

                ctx.fill(
                    rect,
                    data.config.get_color_unchecked(
                        LapceTheme::EDITOR_DRAG_DROP_TAB_BACKGROUND,
                    ),
                );
                break;
            }
        }
    }

    fn paint_drag(&self, ctx: &mut PaintCtx, data: &LapceTabData) {
        if let Some((offset, start, drag_content)) = data.drag.as_ref() {
            if (self.mouse_pos.x - start.x).abs() < 5.
                && (self.mouse_pos.y - start.y).abs() < 5.
            {
                // debounce accidental drags
                return;
            }
            match drag_content {
                DragContent::EditorTab(_, _, _, tab_rect) => {
                    let rect = tab_rect.rect.with_origin(self.mouse_pos - *offset);
                    let size = rect.size();
                    let shadow_width = data.config.ui.drop_shadow_width() as f64;
                    if shadow_width > 0.0 {
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    } else {
                        ctx.stroke(
                            rect.inflate(0.5, 0.5),
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                            1.0,
                        );
                    }
                    ctx.fill(
                        rect,
                        data.config.get_color_unchecked(
                            LapceTheme::EDITOR_DRAG_DROP_BACKGROUND,
                        ),
                    );

                    let width = 13.0;
                    let height = 13.0;
                    let svg_rect =
                        Size::new(width, height).to_rect().with_origin(Point::new(
                            rect.x0 + (size.height - width) / 2.0,
                            rect.y0 + (size.height - height) / 2.0,
                        ));
                    ctx.draw_svg(&tab_rect.svg, svg_rect, None);
                    ctx.draw_text(
                        &tab_rect.text_layout,
                        Point::new(
                            rect.x0 + size.height,
                            rect.y0 + tab_rect.text_layout.y_offset(size.height),
                        ),
                    );
                }
                DragContent::Panel(kind, rect) => {
                    let inflate = (rect.width() / 2.0).round();
                    let icon_rect = rect
                        .with_origin(self.mouse_pos - *offset)
                        .inflate(inflate, inflate);
                    let padding = (icon_rect.width() * 0.3).round();
                    let rect = icon_rect.inflate(padding, padding);
                    let shadow_width = data.config.ui.drop_shadow_width() as f64;
                    if shadow_width > 0.0 {
                        ctx.blurred_rect(
                            rect,
                            shadow_width,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                    } else {
                        ctx.stroke(
                            rect.inflate(0.5, 0.5),
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_BORDER),
                            1.0,
                        );
                    }
                    ctx.fill(
                        rect,
                        data.config.get_color_unchecked(
                            LapceTheme::EDITOR_DRAG_DROP_BACKGROUND,
                        ),
                    );
                    let svg = data.config.ui_svg(kind.svg_name());
                    ctx.draw_svg(
                        &svg,
                        icon_rect,
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                        ),
                    );
                }
            }
        }
    }

    fn handle_mouse_event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse) => {
                Arc::make_mut(&mut data.rename).mouse_within = data.rename.active
                    && self.rename.layout_rect().contains(mouse.pos);
            }
            Event::MouseDown(mouse) => {
                if !ctx.is_handled() && mouse.button.is_left() {
                    if let Some(position) = self.bar_hit_test(mouse.pos) {
                        self.current_bar_hover = Some(position);
                        ctx.set_active(true);
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if mouse.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                }
                if let Some((_, _, DragContent::Panel(_, _))) = data.drag.as_ref() {
                    self.handle_panel_drop(ctx, data);
                    *Arc::make_mut(&mut data.drag) = None;
                }
            }
            _ => {}
        }
    }

    fn handle_command_event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_COMMAND);
                data.run_command(ctx, command, None, env);
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_SAVE_FILE_AS) => {
                ctx.set_handled();
                let file = cmd.get_unchecked(LAPCE_SAVE_FILE_AS);
                if let Some(info) = data.main_split.current_save_as.as_ref() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SaveAs {
                            content: info.0.clone(),
                            path: file.path.clone(),
                            view_id: info.1,
                            exit: info.2,
                        },
                        Target::Widget(data.id),
                    ));
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_OPEN_FOLDER) => {
                ctx.set_handled();
                let file = cmd.get_unchecked(LAPCE_OPEN_FOLDER);
                let workspace = LapceWorkspace {
                    kind: LapceWorkspaceType::Local,
                    path: Some(file.path.clone()),
                    last_open: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetWorkspace(workspace),
                    Target::Window(*data.window_id),
                ));
            }
            Event::Command(cmd) if cmd.is(LAPCE_OPEN_FILE) => {
                ctx.set_handled();
                let file = cmd.get_unchecked(LAPCE_OPEN_FILE);
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenFile(file.path.clone(), false),
                    Target::Widget(data.id),
                ));
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdateStarted => {
                        ctx.set_handled();
                        ctx.request_layout();
                    }
                    LapceUICommand::UpdateFailed => {
                        ctx.set_handled();
                        ctx.request_layout();
                    }
                    LapceUICommand::RequestPaint => {
                        ctx.request_paint();
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowMenu(point, items) => {
                        ctx.set_handled();

                        let mut menu = druid::Menu::new("");
                        for i in items.iter() {
                            match i {
                                MenuKind::Item(i) => {
                                    let mut item = druid::MenuItem::new(i.desc());
                                    if let Some(key) = data
                                        .keypress
                                        .command_keymaps
                                        .get(i.command.kind.str())
                                        .and_then(|m| {
                                            m.iter().find_map(KeyMap::hotkey)
                                        })
                                    {
                                        item = item.dynamic_hotkey(move |_, _| {
                                            Some(key.clone())
                                        })
                                    }
                                    item = item
                                        .command(Command::new(
                                            LAPCE_COMMAND,
                                            i.command.clone(),
                                            Target::Widget(data.id),
                                        ))
                                        .enabled(i.enabled);
                                    menu = menu.entry(item);
                                }
                                MenuKind::Separator => {
                                    menu = menu.separator();
                                }
                            }
                        }
                        ctx.show_context_menu::<LapceData>(menu, *point);
                    }
                    LapceUICommand::InitBufferContent(init) => {
                        init.execute(ctx, data)
                    }
                    LapceUICommand::InitBufferContentLine(init) => {
                        init.execute(ctx, data)
                    }
                    LapceUICommand::InitBufferContentLineCol(init) => {
                        init.execute(ctx, data)
                    }
                    LapceUICommand::InitBufferContentLsp(init) => {
                        init.execute(ctx, data)
                    }
                    LapceUICommand::InitPaletteInput(pattern) => {
                        let doc = data
                            .main_split
                            .local_docs
                            .get_mut(&LocalBufferKind::Palette)
                            .unwrap();
                        Arc::make_mut(doc).reload(Rope::from(pattern), true);
                        let editor = data
                            .main_split
                            .editors
                            .get_mut(&data.palette.input_editor)
                            .unwrap();
                        let offset = doc.buffer().line_end_offset(0, true);
                        Arc::make_mut(editor).cursor.mode =
                            lapce_core::cursor::CursorMode::Insert(
                                lapce_core::selection::Selection::caret(offset),
                            );
                    }
                    LapceUICommand::UpdatePaletteInput(pattern) => {
                        let mut palette_data = data.palette_view_data();
                        palette_data.update_input(ctx, pattern.to_owned());
                        data.palette = palette_data.palette.clone();
                    }
                    LapceUICommand::UpdateSearchInput(pattern) => {
                        let doc = data
                            .main_split
                            .local_docs
                            .get_mut(&LocalBufferKind::Search)
                            .unwrap();
                        if &doc.buffer().to_string() != pattern {
                            Arc::make_mut(doc).reload(Rope::from(pattern), true);
                        }
                    }
                    LapceUICommand::UpdateSearch(pattern, new_cs) => {
                        if pattern.is_empty() {
                            Arc::make_mut(&mut data.find).unset();
                            Arc::make_mut(&mut data.search).matches =
                                Arc::new(Default::default());
                        } else {
                            let find = Arc::make_mut(&mut data.find);
                            if let Some(cs) = new_cs {
                                find.set_case_sensitive(*cs);
                            }
                            find.set_find(pattern, false, false);
                            find.visual = true;
                            if data.focus_area == FocusArea::Panel(PanelKind::Search)
                                && data.config.editor.move_focus_while_search
                            {
                                if let Some(widget_id) = *data.main_split.active {
                                    ctx.submit_command(Command::new(
                                        LAPCE_COMMAND,
                                        LapceCommand {
                                            kind: CommandKind::Focus(
                                                FocusCommand::SearchInView,
                                            ),
                                            data: None,
                                        },
                                        Target::Widget(widget_id),
                                    ));
                                }
                            }
                            let pattern = pattern.to_string();
                            let event_sink = ctx.get_external_handle();
                            let tab_id = data.id;
                            data.proxy.proxy_rpc.global_search(
                                pattern.clone(),
                                find.case_sensitive(),
                                Box::new(move |result| {
                                    if let Ok(
                                        ProxyResponse::GlobalSearchResponse {
                                            matches,
                                        },
                                    ) = result
                                    {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GlobalSearchResult(
                                                pattern,
                                                Arc::new(matches),
                                            ),
                                            Target::Widget(tab_id),
                                        );
                                    }
                                }),
                            )
                        }
                    }
                    LapceUICommand::OpenPluginInfo(volt) => {
                        data.main_split.open_plugin_info(ctx, volt);
                    }
                    LapceUICommand::GlobalSearchResult(pattern, matches) => {
                        let doc = data
                            .main_split
                            .local_docs
                            .get(&LocalBufferKind::Search)
                            .unwrap();
                        if &doc.buffer().text().slice_to_cow(..) == pattern {
                            Arc::make_mut(&mut data.search).matches =
                                matches.clone();
                        }
                    }
                    LapceUICommand::LoadBufferHead {
                        path,
                        version,
                        content,
                    } => {
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        let doc = Arc::make_mut(doc);
                        doc.load_history(
                            version,
                            content.clone(),
                            data.config.editor.diff_context_lines,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::PrepareRename {
                        path,
                        rev,
                        offset,
                        start,
                        end,
                        placeholder,
                    } => {
                        ctx.set_handled();
                        Arc::make_mut(&mut data.rename).handle_prepare_rename(
                            ctx,
                            &mut data.main_split,
                            path.to_path_buf(),
                            *rev,
                            *offset,
                            *start,
                            *end,
                            placeholder.clone(),
                        );
                    }
                    LapceUICommand::UpdateTerminalTitle(term_id, title) => {
                        for (_, split) in
                            Arc::make_mut(&mut data.terminal).tabs.iter_mut()
                        {
                            if let Some(terminal) = split.terminals.get_mut(term_id)
                            {
                                Arc::make_mut(terminal).title = title.to_string();
                            }
                        }
                    }
                    LapceUICommand::CancelFilePicker => {
                        Arc::make_mut(&mut data.picker).active = false;
                        ctx.set_handled();
                    }
                    LapceUICommand::ProxyUpdateStatus(status) => {
                        data.proxy_status = Arc::new(*status);
                        ctx.set_handled();
                    }
                    LapceUICommand::HomeDir(path) => {
                        Arc::make_mut(&mut data.picker).init_home(path);
                        data.set_picker_pwd(path.clone());
                        ctx.set_handled();
                    }
                    LapceUICommand::WorkspaceFileChange => {
                        data.handle_workspace_file_change(ctx);
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateCompletion {
                        request_id,
                        input,
                        resp,
                        plugin_id,
                    } => {
                        let completion = Arc::make_mut(&mut data.completion);
                        completion.receive(
                            *request_id,
                            input.to_owned(),
                            resp.to_owned(),
                            *plugin_id,
                        );
                    }
                    LapceUICommand::CancelCompletion { request_id } => {
                        if data.completion.request_id == *request_id {
                            let completion = Arc::make_mut(&mut data.completion);
                            completion.cancel();
                        }
                    }
                    LapceUICommand::UpdateSignature {
                        request_id,
                        resp,
                        plugin_id,
                    } => {
                        let signature = Arc::make_mut(&mut data.signature);
                        signature.receive(*request_id, resp.to_owned(), *plugin_id);
                    }
                    LapceUICommand::CloseTerminal(id) => {
                        let terminal_panel = Arc::make_mut(&mut data.terminal);
                        if let Some(terminal) = terminal_panel
                            .active_terminal_split_mut()
                            .unwrap()
                            .terminals
                            .get_mut(id)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::SplitTerminalClose(
                                    terminal.term_id,
                                    terminal.widget_id,
                                ),
                                Target::Widget(terminal.split_id),
                            ));
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::LoadPluginLatest(info) => {
                        ctx.set_handled();
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.installed_latest.insert(info.id(), info.clone());
                    }
                    LapceUICommand::LoadPlugins(info) => {
                        ctx.set_handled();
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.volts.update_volts(info);
                    }
                    LapceUICommand::LoadPluginsFailed => {
                        ctx.set_handled();
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.volts.failed();
                    }
                    LapceUICommand::LoadPluginIcon(id, icon) => {
                        ctx.set_handled();
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.volts.icons.insert(id.clone(), icon.clone());
                    }
                    LapceUICommand::VoltInstalled(volt, icon) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.volt_installed(
                            data.id,
                            volt,
                            icon,
                            ctx.get_external_handle(),
                        );

                        for (_, tabs) in data.main_split.editor_tabs.iter() {
                            for child in tabs.children.iter() {
                                if let EditorTabChild::Settings {
                                    settings_widget_id,
                                    ..
                                } = child
                                {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::VoltInstalled(
                                            volt.clone(),
                                            icon.clone(),
                                        ),
                                        Target::Widget(*settings_widget_id),
                                    ));
                                }
                            }
                        }
                    }
                    LapceUICommand::VoltInstalling(volt, error) => {
                        let plugin = Arc::make_mut(&mut data.plugin);

                        let event_sink = ctx.get_external_handle();
                        let id = data.id;
                        let volt_id = volt.id();
                        if !error.is_empty() {
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_secs(
                                    3,
                                ));
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::VoltInstallStatusClear(volt_id),
                                    Target::Widget(id),
                                );
                            });
                        }

                        if let Some(elem) = plugin.installing.get_mut(&volt.id()) {
                            if !error.is_empty() {
                                elem.set_error(error);
                            }
                        } else {
                            plugin.installing.insert(
                                volt.id(),
                                PluginInstallStatus::new(
                                    PluginInstallType::Installation,
                                    &volt.display_name,
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                    LapceUICommand::VoltRemoving(volt, error) => {
                        let plugin = Arc::make_mut(&mut data.plugin);

                        let event_sink = ctx.get_external_handle();
                        let id = data.id;
                        let volt_id = volt.id();
                        if !error.is_empty() {
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_secs(
                                    3,
                                ));
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::VoltInstallStatusClear(volt_id),
                                    Target::Widget(id),
                                );
                            });
                        }

                        if let Some(elem) = plugin.installing.get_mut(&volt.id()) {
                            if !error.is_empty() {
                                elem.set_error(error);
                            }
                        } else {
                            plugin.installing.insert(
                                volt.id(),
                                PluginInstallStatus::new(
                                    PluginInstallType::Uninstallation,
                                    &volt.display_name,
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                    LapceUICommand::VoltInstallStatusClear(volt_id) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.installing.remove(volt_id);
                    }
                    LapceUICommand::VoltRemoved(volt, only_installing) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        let id = volt.id();

                        // if there is a value inside the installing map, remove it from there as soon as it is installed.
                        plugin.installing.remove(&volt.id());

                        if !(*only_installing) {
                            plugin.installed.remove(&id);

                            if plugin.disabled.remove(&id) {
                                let _ = data.db.save_disabled_volts(
                                    plugin.disabled.iter().collect(),
                                );
                            }
                            if plugin.workspace_disabled.remove(&id) {
                                let _ = data.db.save_disabled_volts(
                                    plugin.workspace_disabled.iter().collect(),
                                );
                            }

                            for (_, tabs) in data.main_split.editor_tabs.iter() {
                                for child in tabs.children.iter() {
                                    if let EditorTabChild::Settings {
                                        settings_widget_id,
                                        ..
                                    } = child
                                    {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::VoltRemoved(
                                                volt.clone(),
                                                false,
                                            ),
                                            Target::Widget(*settings_widget_id),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    LapceUICommand::DisableVoltWorkspace(volt) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.workspace_disabled.insert(volt.id());
                        data.proxy.proxy_rpc.disable_volt(volt.clone());
                        let _ = data.db.save_workspace_disabled_volts(
                            &data.workspace,
                            plugin.workspace_disabled.iter().collect(),
                        );
                    }
                    LapceUICommand::EnableVoltWorkspace(volt) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        let id = volt.id();
                        plugin.workspace_disabled.remove(&id);
                        if !plugin.plugin_disabled(&id) {
                            data.proxy.proxy_rpc.enable_volt(volt.clone());
                        }
                        let _ = data.db.save_workspace_disabled_volts(
                            &data.workspace,
                            plugin.workspace_disabled.iter().collect(),
                        );
                    }
                    LapceUICommand::DisableVolt(volt) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        plugin.disabled.insert(volt.id());
                        data.proxy.proxy_rpc.disable_volt(volt.clone());
                        let _ = data
                            .db
                            .save_disabled_volts(plugin.disabled.iter().collect());
                    }
                    LapceUICommand::EnableVolt(volt) => {
                        let plugin = Arc::make_mut(&mut data.plugin);
                        let id = volt.id();
                        plugin.disabled.remove(&id);
                        if !plugin.plugin_disabled(&id) {
                            data.proxy.proxy_rpc.enable_volt(volt.clone());
                        }
                        let _ = data
                            .db
                            .save_disabled_volts(plugin.disabled.iter().collect());
                    }
                    LapceUICommand::UpdateDiffInfo(diff) => {
                        let source_control = Arc::make_mut(&mut data.source_control);
                        source_control.branch = diff.head.to_string();
                        source_control.branches =
                            diff.branches.iter().cloned().collect();
                        source_control.file_diffs = diff
                            .diffs
                            .iter()
                            .cloned()
                            .map(|diff| {
                                let checked = source_control
                                    .file_diffs
                                    .get(diff.path())
                                    .map_or(true, |(_, c)| *c);
                                (diff.path().clone(), (diff, checked))
                            })
                            .collect();

                        for (_path, doc) in data.main_split.open_docs.iter() {
                            doc.reload_history("head");
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::WorkDoneProgress(params) => {
                        match &params.value {
                            lsp_types::ProgressParamsValue::WorkDone(progress) => {
                                match progress {
                                    lsp_types::WorkDoneProgress::Begin(begin) => {
                                        Arc::make_mut(&mut data.progresses).push(
                                            WorkProgress {
                                                token: params.token.clone(),
                                                title: begin.title.clone(),
                                                message: begin.message.clone(),
                                                percentage: begin.percentage,
                                            },
                                        );
                                    }
                                    lsp_types::WorkDoneProgress::Report(report) => {
                                        for p in Arc::make_mut(&mut data.progresses)
                                            .iter_mut()
                                        {
                                            if p.token == params.token {
                                                p.message = report.message.clone();
                                                p.percentage = report.percentage;
                                            }
                                        }
                                    }
                                    lsp_types::WorkDoneProgress::End(_end) => {
                                        for view_id in data.main_split.editors.keys()
                                        {
                                            let editor_data =
                                                data.editor_view_content(*view_id);
                                            editor_data.doc.get_inlay_hints();
                                        }
                                        for i in data
                                            .progresses
                                            .iter()
                                            .positions(|p| p.token == params.token)
                                            .sorted()
                                            .rev()
                                        {
                                            Arc::make_mut(&mut data.progresses)
                                                .remove(i);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    LapceUICommand::PublishDiagnostics(diagnostics) => {
                        let path = path_from_url(&diagnostics.uri);
                        let diagnostics = diagnostics
                            .diagnostics
                            .iter()
                            .map(|d| EditorDiagnostic {
                                range: (0, 0),
                                diagnostic: d.clone(),
                                lines: d
                                    .related_information
                                    .as_ref()
                                    .map(|r| {
                                        r.iter()
                                            .map(|r| {
                                                r.message.matches('\n').count()
                                                    + 1
                                                    + 1
                                            })
                                            .sum()
                                    })
                                    .unwrap_or(0)
                                    + d.message.matches('\n').count()
                                    + 1,
                            })
                            .sorted_by_key(|d| d.diagnostic.range.start)
                            .collect();
                        let diagnostics: Arc<Vec<EditorDiagnostic>> =
                            Arc::new(diagnostics);

                        // inform the document about the diagnostics
                        if let Some(document) =
                            data.main_split.open_docs.get_mut(&path)
                        {
                            let document = Arc::make_mut(document);
                            document.set_diagnostics(&diagnostics);
                        }

                        data.main_split.diagnostics.insert(path, diagnostics);

                        let mut errors = 0;
                        let mut warnings = 0;
                        for (_, diagnostics) in data.main_split.diagnostics.iter() {
                            for diagnostic in diagnostics.iter() {
                                if let Some(severity) =
                                    diagnostic.diagnostic.severity
                                {
                                    match severity {
                                        DiagnosticSeverity::ERROR => errors += 1,
                                        DiagnosticSeverity::WARNING => warnings += 1,
                                        _ => (),
                                    }
                                }
                            }
                        }
                        data.main_split.error_count = errors;
                        data.main_split.warning_count = warnings;

                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentSave { path, exit } => {
                        data.main_split.document_save(ctx, path, *exit);
                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentFormatAndSave {
                        path,
                        rev,
                        result,
                        exit,
                    } => {
                        data.main_split.document_format_and_save(
                            ctx, path, *rev, result, *exit,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentFormat { path, rev, result } => {
                        data.main_split.document_format(path, *rev, result);
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowAbout => {
                        let about = Arc::make_mut(&mut data.about);
                        about.active = true;
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(about.widget_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowAlert(content) => {
                        let alert = Arc::make_mut(&mut data.alert);
                        alert.active = true;
                        alert.content = content.to_owned();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(alert.widget_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::BufferSave {
                        path,
                        rev,
                        exit: exit_widget_id,
                    } => {
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        if doc.rev() == *rev {
                            Arc::make_mut(doc).buffer_mut().set_pristine();
                            if let Some(widget_id) = exit_widget_id {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Focus(
                                            FocusCommand::SplitClose,
                                        ),
                                        data: None,
                                    },
                                    Target::Widget(*widget_id),
                                ));
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSettingsFile { kind, key, value } => {
                        ctx.set_handled();
                        if let Some(value) = toml_edit::ser::to_item(value)
                            .ok()
                            .and_then(|i| i.into_value().ok())
                        {
                            let update_result =
                                LapceConfig::update_file(kind, key, value);
                            debug_assert!(update_result.is_some());
                        }
                    }
                    LapceUICommand::ResetSettingsFile { kind, key } => {
                        LapceConfig::reset_setting(kind, key);
                    }
                    LapceUICommand::OpenFileDiff { path, history } => {
                        let editor_view_id = data.main_split.jump_to_location(
                            ctx,
                            None,
                            false,
                            EditorLocation {
                                path: path.clone(),
                                position: None::<usize>,
                                scroll_offset: None,
                                history: Some(history.to_string()),
                            },
                            &data.config,
                        );
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(editor_view_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateKeymapsFilter(pattern) => {
                        ctx.set_handled();
                        let keypress = Arc::make_mut(&mut data.keypress);
                        keypress.filter_commands(pattern);
                    }
                    LapceUICommand::FilterKeymaps {
                        pattern,
                        keymaps,
                        commands,
                    } => {
                        ctx.set_handled();
                        let keypress = Arc::make_mut(&mut data.keypress);
                        if &keypress.filter_pattern == pattern {
                            keypress.filtered_commands_with_keymap = keymaps.clone();
                            keypress.filtered_commands_without_keymap =
                                commands.clone();
                        }
                    }
                    LapceUICommand::UpdateKeymap(keymap, keys) => {
                        KeyPressData::update_file(keymap, keys);
                    }
                    LapceUICommand::OpenURI(uri) => {
                        ctx.set_handled();
                        if !uri.is_empty() {
                            log::debug!(target: "lapce_ui::tab::handle_event::open_uri", "uri: {uri}");
                            match open::that(uri) {
                                Ok(_) => {
                                    log::debug!(target: "lapce_ui::tab::handle_event::open_uri", "successfully opened URI: {uri}")
                                }
                                Err(e) => {
                                    log::error!(target: "lapce_ui::tab::handle_event::open_uri", "{e}")
                                }
                            }
                        }
                    }
                    LapceUICommand::OpenFile(path, same_tab) => {
                        data.main_split.jump_to_location(
                            ctx,
                            None,
                            *same_tab,
                            EditorLocation {
                                path: path.clone(),
                                position: None::<usize>,
                                scroll_offset: None,
                                history: None,
                            },
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::RevealInFileExplorer(path) => {
                        // TODO: replace with proper implementation from druid that
                        // highlights items in file explorer
                        let path = match path.parent() {
                            Some(p) => p,
                            None => path,
                        };
                        if path.exists() {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::OpenURI(
                                    path.to_str().unwrap().to_string(),
                                ),
                                Target::Auto,
                            ));
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::GoToLocation(
                        editor_view_id,
                        location,
                        same_tab,
                    ) => {
                        data.main_split.go_to_location(
                            ctx,
                            *editor_view_id,
                            *same_tab,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToPosition(
                        editor_view_id,
                        position,
                        same_tab,
                    ) => {
                        data.main_split.jump_to_position(
                            ctx,
                            *editor_view_id,
                            *same_tab,
                            *position,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLocation(
                        editor_view_id,
                        location,
                        same_tab,
                    ) => {
                        data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            *same_tab,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLspLocation(
                        editor_view_id,
                        location,
                        same_tab,
                    ) => {
                        data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            *same_tab,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::ToggleProblem(path) => {
                        let problem = Arc::make_mut(&mut data.problem);
                        let state = problem
                            .collapsed
                            .entry(path.to_owned())
                            .or_insert(false);
                        *state = !*state;
                    }
                    LapceUICommand::JumpToLineLocation(editor_view_id, location) => {
                        data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            true,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLineColLocation(
                        editor_view_id,
                        location,
                        same_tab,
                    ) => {
                        data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            *same_tab,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLine(editor_view_id, line) => {
                        data.main_split.jump_to_line(
                            ctx,
                            *editor_view_id,
                            *line,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::TerminalJumpToLine(line) => {
                        if let Some(terminal) = data.terminal.active_terminal() {
                            terminal.raw.lock().term.vi_goto_point(
                                alacritty_terminal::index::Point::new(
                                    alacritty_terminal::index::Line(*line),
                                    alacritty_terminal::index::Column(0),
                                ),
                            );
                            ctx.request_paint();
                        }
                        // data.term_tx.send((
                        //     data.terminal.active_term_id,
                        //     TerminalEvent::JumpToLine(*line),
                        // ));
                        ctx.set_handled();
                    }
                    LapceUICommand::GotoDefinition {
                        editor_view_id,
                        offset,
                        location,
                    } => {
                        if let Some(editor) = data.main_split.active_editor() {
                            if *editor_view_id == editor.view_id
                                && *offset == editor.cursor.offset()
                            {
                                data.main_split.jump_to_location(
                                    ctx,
                                    None,
                                    true,
                                    location.clone(),
                                    &data.config,
                                );
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateInlayHints { path, rev, hints } => {
                        if let Some(doc) = data.main_split.open_docs.get_mut(path) {
                            if doc.rev() == *rev {
                                Arc::make_mut(doc).set_inlay_hints(hints.clone());
                            }
                        }
                    }
                    LapceUICommand::CodeActionsError { path, rev, offset } => {
                        if let Some(doc) = data.main_split.open_docs.get_mut(path) {
                            if doc.rev() == *rev {
                                Arc::make_mut(doc).code_actions.remove(offset);
                            }
                        }
                    }
                    LapceUICommand::UpdateCodeActions {
                        path,
                        plugin_id,
                        rev,
                        offset,
                        resp,
                    } => {
                        if let Some(doc) = data.main_split.open_docs.get_mut(path) {
                            if doc.rev() == *rev {
                                Arc::make_mut(doc)
                                    .code_actions
                                    .insert(*offset, (*plugin_id, resp.clone()));
                            }
                        }
                    }
                    LapceUICommand::PaletteReferences(offset, locations) => {
                        if let Some(editor) = data.main_split.active_editor() {
                            if *offset == editor.cursor.offset() {
                                let locations = locations
                                    .iter()
                                    .map(|l| EditorLocation {
                                        path: path_from_url(&l.uri),
                                        position: Some(l.range.start),
                                        scroll_offset: None,
                                        history: None,
                                    })
                                    .collect();
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RunPaletteReferences(locations),
                                    Target::Widget(data.palette.widget_id),
                                ));
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::SaveAs {
                        content,
                        path,
                        view_id,
                        exit,
                    } => {
                        data.main_split.save_as(ctx, content, path, *view_id, *exit);
                        ctx.set_handled();
                    }
                    LapceUICommand::SaveAsSuccess {
                        content,
                        rev,
                        path,
                        view_id,
                        exit,
                    } => {
                        data.main_split.save_as_success(
                            ctx, content, *rev, path, *view_id, *exit,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::OpenFileChanged { path, content } => {
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        let doc = Arc::make_mut(doc);
                        doc.handle_file_changed(content.to_owned());
                    }
                    LapceUICommand::ReloadBuffer { path, rev, content } => {
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        if doc.rev() + 1 == *rev {
                            let doc = Arc::make_mut(doc);
                            doc.reload(content.to_owned(), true);

                            for (_, editor) in data.main_split.editors.iter_mut() {
                                if &editor.content == doc.content()
                                    && editor.cursor.offset() >= doc.buffer().len()
                                {
                                    let editor = Arc::make_mut(editor);
                                    if data.config.core.modal {
                                        editor.cursor = Cursor::new(
                                            CursorMode::Normal(
                                                doc.buffer().offset_line_end(
                                                    doc.buffer().len(),
                                                    false,
                                                ),
                                            ),
                                            None,
                                            None,
                                        );
                                    } else {
                                        editor.cursor = Cursor::new(
                                            CursorMode::Insert(Selection::caret(
                                                doc.buffer().offset_line_end(
                                                    doc.buffer().len(),
                                                    true,
                                                ),
                                            )),
                                            None,
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSemanticStyles {
                        path,
                        rev,
                        styles,
                        ..
                    } => {
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        if doc.rev() == *rev {
                            let doc = Arc::make_mut(doc);
                            doc.set_semantic_styles(Some(styles.clone()));
                        }

                        ctx.set_handled();
                    }
                    LapceUICommand::Focus => {
                        ctx.window().set_title(
                            &workspace_title(&data.workspace)
                                .map(|x| format!("{x} - Lapce"))
                                .unwrap_or_else(|| String::from("Lapce")),
                        );
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(*data.focus),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSyntax { content, syntax } => {
                        ctx.set_handled();
                        let doc = match content {
                            BufferContent::File(path) => {
                                data.main_split.open_docs.get_mut(path).unwrap()
                            }
                            BufferContent::Local(kind) => {
                                data.main_split.local_docs.get_mut(kind).unwrap()
                            }
                            BufferContent::SettingsValue(name) => {
                                data.main_split.value_docs.get_mut(name).unwrap()
                            }
                            BufferContent::Scratch(id, _) => {
                                data.main_split.scratch_docs.get_mut(id).unwrap()
                            }
                        };
                        let doc = Arc::make_mut(doc);
                        if let Some(syntax) = syntax.take() {
                            if doc.rev() == syntax.rev {
                                doc.set_syntax(Some(syntax));
                            }
                        }
                    }
                    LapceUICommand::SetLanguage(name) => {
                        ctx.set_handled();
                        let editor = if let Some(editor) =
                            data.main_split.active_editor().cloned()
                        {
                            editor
                        } else {
                            return;
                        };

                        let doc = data.main_split.content_doc_mut(&editor.content);
                        let doc = Arc::make_mut(doc);

                        if name.is_empty() || name.to_lowercase().eq("plain text") {
                            doc.set_syntax(None);
                        } else {
                            let lang = match LapceLanguage::from_name(name) {
                                Some(v) => v,
                                None => return,
                            };

                            doc.set_language(lang);
                        }
                        doc.trigger_syntax_change(None);
                    }
                    LapceUICommand::UpdateHistoryChanges {
                        path,
                        rev,
                        history,
                        changes,
                        diff_context_lines,
                        ..
                    } => {
                        ctx.set_handled();
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        Arc::make_mut(doc).update_history_changes(
                            *rev,
                            history,
                            changes.clone(),
                            *diff_context_lines,
                        );
                    }
                    LapceUICommand::UpdateHistoryStyle {
                        path,
                        history,
                        highlights,
                        ..
                    } => {
                        ctx.set_handled();
                        let doc = data.main_split.open_docs.get_mut(path).unwrap();
                        Arc::make_mut(doc)
                            .update_history_styles(history, highlights.to_owned());
                    }
                    LapceUICommand::UpdatePickerPwd(path) => {
                        Arc::make_mut(&mut data.picker).pwd = path.clone();
                        data.read_picker_pwd(ctx);
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdatePickerItems(path, items) => {
                        Arc::make_mut(&mut data.picker)
                            .root
                            .set_item_children(path, items.clone());
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateExplorerItems {
                        path,
                        items,
                        expand,
                    } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        file_explorer.update_children(
                            path,
                            items.to_owned(),
                            *expand,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::CreateFileOpen { path } => {
                        let path_c = path.clone();
                        let event_sink = ctx.get_external_handle();
                        let tab_id = data.id;
                        let explorer = data.file_explorer.clone();
                        data.proxy.proxy_rpc.create_file(
                            path.clone(),
                            Box::new(move |res| {
                                match res {
                                    Ok(_) => {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::OpenFile(path_c, false),
                                            Target::Widget(tab_id),
                                        );
                                    }
                                    Err(err) => {
                                        // TODO: Inform the user through a corner-notif
                                        log::warn!(
                                            "Failed to create file: {:?}",
                                            err,
                                        );
                                    }
                                }
                                explorer.reload();
                            }),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::CreateDirectory { path } => {
                        let explorer = data.file_explorer.clone();
                        data.proxy.proxy_rpc.create_directory(
                            path.clone(),
                            Box::new(move |res| {
                                if let Err(err) = res {
                                    // TODO: Inform the user through a corner-notif
                                    log::warn!(
                                        "Failed to create directory: {:?}",
                                        err
                                    );
                                }
                                explorer.reload();
                            }),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::DuplicateFileOpen {
                        existing_path,
                        new_path,
                    } => {
                        let new_path_clone = new_path.clone();
                        let event_sink = ctx.get_external_handle();
                        let tab_id = data.id;
                        let explorer = data.file_explorer.clone();
                        data.proxy.proxy_rpc.duplicate_path(
                            existing_path.clone(),
                            new_path.clone(),
                            Box::new(move |res| {
                                match res {
                                    Ok(_) => {
                                        let _ = event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::OpenFile(
                                                new_path_clone,
                                                false,
                                            ),
                                            Target::Widget(tab_id),
                                        );
                                    }
                                    Err(err) => {
                                        // TODO: Inform the user through a corner-notif
                                        log::warn!(
                                            "Failed to duplicate file: {:?}",
                                            err,
                                        );
                                    }
                                }
                                explorer.reload();
                            }),
                        );
                    }
                    LapceUICommand::RenamePath { from, to } => {
                        let explorer = data.file_explorer.clone();
                        data.proxy.proxy_rpc.rename_path(
                            from.clone(),
                            to.clone(),
                            Box::new(move |res| {
                                if let Err(err) = res {
                                    // TODO: inform the user through a corner-notif
                                    log::warn!("Failed to rename path: {:?}", err);
                                }
                                explorer.reload();
                            }),
                        );
                    }
                    LapceUICommand::TrashPath { path } => {
                        let explorer = data.file_explorer.clone();
                        data.proxy.proxy_rpc.trash_path(
                            path.clone(),
                            Box::new(move |res| {
                                if let Err(err) = res {
                                    // TODO: inform the user through a corner-notif
                                    log::warn!("Failed to trash path: {:?}", err);
                                }
                                explorer.reload();
                            }),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::ExplorerNew {
                        list_index,
                        indent_level,
                        is_dir,
                        base_path,
                    } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        file_explorer.start_naming(
                            ctx,
                            &mut data.main_split,
                            *list_index,
                            *indent_level,
                            *is_dir,
                            base_path.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::ExplorerStartDuplicate {
                        list_index,
                        indent_level,
                        base_path,
                        name,
                    } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        file_explorer.start_duplicating(
                            ctx,
                            &mut data.main_split,
                            *list_index,
                            *indent_level,
                            base_path.clone(),
                            name.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::ExplorerStartRename {
                        list_index,
                        indent_level,
                        text,
                    } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        file_explorer.start_renaming(
                            ctx,
                            &mut data.main_split,
                            *list_index,
                            *indent_level,
                            text.clone(),
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::ExplorerEndNaming { apply_naming } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        if *apply_naming {
                            file_explorer.apply_naming(ctx, &data.main_split);
                        } else {
                            file_explorer.cancel_naming();
                        }
                    }
                    LapceUICommand::PutToClipboard(target_string) => {
                        let mut clipboard = druid::Application::global().clipboard();
                        clipboard.put_string(target_string);
                    }
                    LapceUICommand::NewMessage {
                        kind,
                        title,
                        message,
                    } => {
                        ctx.set_handled();
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::NewMessage {
                                kind: *kind,
                                title: title.clone(),
                                message: message.clone(),
                            },
                            Target::Widget(*data.message_widget_id),
                        ));
                    }
                    LapceUICommand::RunCommand(cmd, args) => {
                        ctx.set_handled();
                        let _ = process::Command::new(cmd).args(args).spawn();
                    }
                    LapceUICommand::ImageLoaded { url, image } => {
                        ctx.set_handled();
                        let images = Arc::make_mut(&mut data.images);
                        images.load_finished(url, image)
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

impl Widget<LapceTabData> for LapceTab {
    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.handle_command_event(ctx, event, data, env);

        if data.about.active || event.should_propagate_to_hidden() {
            self.about.event(ctx, event, data, env);
        }
        if data.alert.active || event.should_propagate_to_hidden() {
            self.alert.event(ctx, event, data, env);
        }
        if data.picker.active || event.should_propagate_to_hidden() {
            self.picker.event(ctx, event, data, env);
        }
        self.title.event(ctx, event, data, env);
        self.message.event(ctx, event, data, env);
        if data.completion.status == CompletionStatus::Started
            || event.should_propagate_to_hidden()
        {
            self.completion.event(ctx, event, data, env);
        }
        if data.signature.status == SignatureStatus::Started
            || event.should_propagate_to_hidden()
        {
            self.signature.event(ctx, event, data, env);
        }
        if data.hover.status == HoverStatus::Done
            || event.should_propagate_to_hidden()
        {
            self.hover.event(ctx, event, data, env);
        }
        if data.rename.active || event.should_propagate_to_hidden() {
            self.rename.event(ctx, event, data, env);
        }

        self.handle_mouse_event(ctx, event, data, env);

        self.main_split.event(ctx, event, data, env);

        self.status.event(ctx, event, data, env);
        if data.panel.is_container_shown(&PanelContainerPosition::Left)
            || event.should_propagate_to_hidden()
        {
            self.panel_left.event(ctx, event, data, env);
        }
        if data
            .panel
            .is_container_shown(&PanelContainerPosition::Right)
            || event.should_propagate_to_hidden()
        {
            self.panel_right.event(ctx, event, data, env);
        }
        if data
            .panel
            .is_container_shown(&PanelContainerPosition::Bottom)
            || event.should_propagate_to_hidden()
        {
            self.panel_bottom.event(ctx, event, data, env);
        }

        if data.hover.status != HoverStatus::Inactive {
            if let Event::MouseMove(mouse_event) = &event {
                if !self.hover.layout_rect().contains(mouse_event.pos)
                    && !self.main_split.layout_rect().contains(mouse_event.pos)
                {
                    Arc::make_mut(&mut data.hover).cancel();
                }
            }
            if !data.main_split.editor_tabs.iter().any(|(_, tab)| {
                tab.active_child().map(|c| c.widget_id())
                    == Some(data.hover.editor_view_id)
            }) {
                Arc::make_mut(&mut data.hover).cancel();
            }
        }

        if ctx.is_handled() {
            return;
        }

        match event {
            Event::MouseMove(mouse) => {
                self.mouse_pos = mouse.pos;
                if ctx.is_active() {
                    self.update_split_point(ctx, data, mouse.pos);
                    ctx.request_layout();
                    ctx.set_handled();
                } else if data.drag.is_some() {
                    ctx.request_paint();
                } else if ctx.has_active() {
                    ctx.clear_cursor();
                } else {
                    match self.bar_hit_test(mouse.pos) {
                        Some(position) => {
                            if self.current_bar_hover.as_ref() != Some(&position) {
                                self.current_bar_hover = Some(position.clone());
                                ctx.request_paint();
                            }
                            match position {
                                PanelResizePosition::Left => {
                                    ctx.set_cursor(&druid::Cursor::ResizeLeftRight);
                                }
                                PanelResizePosition::Right => {
                                    ctx.set_cursor(&druid::Cursor::ResizeLeftRight);
                                }
                                PanelResizePosition::LeftSplit => {
                                    ctx.set_cursor(&druid::Cursor::ResizeUpDown);
                                }
                                PanelResizePosition::Bottom => {
                                    ctx.set_cursor(&druid::Cursor::ResizeUpDown)
                                }
                            }
                        }
                        None => {
                            if self.current_bar_hover.is_some() {
                                self.current_bar_hover = None;
                                ctx.request_paint();
                            }
                            ctx.clear_cursor();
                        }
                    }
                }
            }
            Event::MouseDown(_) => {
                if !ctx.is_handled() {
                    if data.palette.status != PaletteStatus::Inactive {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(FocusCommand::ModalClose),
                                data: None,
                            },
                            Target::Widget(data.palette.widget_id),
                        ));
                    }
                    if data.title.branches.active {
                        Arc::make_mut(&mut data.title).branches.active = false;
                    }
                    if data.focus_area == FocusArea::Rename {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(FocusCommand::ModalClose),
                                data: None,
                            },
                            Target::Widget(data.rename.view_id),
                        ));
                    }
                }
            }
            Event::KeyDown(key_event) if !ctx.is_handled() => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                mut_keypress.key_down(
                    ctx,
                    key_event,
                    &mut DefaultKeyPressHandler {},
                    env,
                );
                data.keypress = keypress;
                ctx.set_handled();
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
        if let LifeCycle::Internal(InternalLifeCycle::ParentWindowOrigin) = event {
            let current_window_origin = ctx.window_origin();
            if current_window_origin != *data.window_origin.borrow() {
                *data.window_origin.borrow_mut() = current_window_origin;
                ctx.request_layout();
            }
        }
        self.title.lifecycle(ctx, event, data, env);
        self.main_split.lifecycle(ctx, event, data, env);
        self.status.lifecycle(ctx, event, data, env);
        self.completion.lifecycle(ctx, event, data, env);
        self.signature.lifecycle(ctx, event, data, env);
        self.hover.lifecycle(ctx, event, data, env);
        self.rename.lifecycle(ctx, event, data, env);
        self.picker.lifecycle(ctx, event, data, env);
        self.about.lifecycle(ctx, event, data, env);
        self.alert.lifecycle(ctx, event, data, env);
        self.message.lifecycle(ctx, event, data, env);
        self.panel_left.lifecycle(ctx, event, data, env);
        self.panel_right.lifecycle(ctx, event, data, env);
        self.panel_bottom.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if old_data.config.id != data.config.id {
            ctx.request_layout();
        }

        if old_data.focus != data.focus {
            ctx.request_paint();
        }

        if !old_data.drag.same(&data.drag) {
            ctx.request_paint();
        }

        if !old_data.plugin.same(&data.plugin) {
            if old_data.plugin.installed.len() != data.plugin.installed.len()
                || old_data.plugin.volts.volts.len() != data.plugin.volts.volts.len()
            {
                ctx.request_layout();
            } else {
                ctx.request_paint();
            }
        }

        if old_data.about.active != data.about.active {
            ctx.request_layout();
        }
        if old_data.alert.active != data.alert.active {
            ctx.request_layout();
        }

        if !old_data
            .main_split
            .diagnostics
            .same(&data.main_split.diagnostics)
        {
            ctx.request_paint();
        }

        if !old_data.panel.order.same(&data.panel.order) {
            for (pos, order) in old_data.panel.order.iter() {
                for kind in order.iter() {
                    if let Some((_, new_pos)) = data.panel.panel_position(kind) {
                        if pos != &new_pos {
                            self.move_panel(ctx, *kind, *pos, new_pos);
                        }
                    }
                }
            }
        }

        if !old_data.panel.same(&data.panel) {
            ctx.request_layout();
        }

        if !old_data.config.same(&data.config) {
            ctx.request_layout();
        }

        if old_data.rename.active != data.rename.active {
            ctx.request_layout();
        }

        if old_data.picker.active != data.picker.active {
            ctx.request_layout();
        }

        self.title.update(ctx, data, env);
        self.main_split.update(ctx, data, env);
        self.completion.update(ctx, data, env);
        self.signature.update(ctx, data, env);
        self.hover.update(ctx, data, env);
        self.rename.update(ctx, data, env);
        self.status.update(ctx, data, env);
        self.picker.update(ctx, data, env);
        self.about.update(ctx, data, env);
        self.alert.update(ctx, data, env);
        self.message.update(ctx, data, env);
        self.panel_left.update(ctx, data, env);
        self.panel_right.update(ctx, data, env);
        self.panel_bottom.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        self.height = self_size.height;
        self.width = self_size.width;

        self.title.layout(ctx, bc, data, env);
        self.title.set_origin(ctx, data, env, Point::ZERO);
        self.title_height = 36.0;
        let title_height = self.title_height;

        let status_size = self.status.layout(ctx, bc, data, env);
        self.status.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, self_size.height - status_size.height),
        );
        self.status_height = status_size.height;

        let left_width = data.panel.size.left;
        self.panel_left.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                left_width,
                self_size.height - status_size.height - title_height,
            )),
            data,
            env,
        );
        self.panel_left
            .set_origin(ctx, data, env, Point::new(0.0, title_height));
        let panel_left_width =
            if data.panel.is_container_shown(&PanelContainerPosition::Left) {
                left_width
            } else {
                0.0
            };

        let right_width = data.panel.size.right;
        self.panel_right.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                right_width,
                self_size.height - status_size.height - title_height,
            )),
            data,
            env,
        );
        self.panel_right.set_origin(
            ctx,
            data,
            env,
            Point::new(self_size.width - right_width, title_height),
        );
        let panel_right_width = if data
            .panel
            .is_container_shown(&PanelContainerPosition::Right)
        {
            right_width
        } else {
            0.0
        };

        let bottom_height = if data.panel.panel_bottom_maximized() {
            self_size.height - status_size.height - title_height
        } else {
            data.panel.size.bottom
        };
        self.panel_bottom.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                self_size.width - panel_left_width - panel_right_width,
                bottom_height,
            )),
            data,
            env,
        );
        self.panel_bottom.set_origin(
            ctx,
            data,
            env,
            Point::new(
                panel_left_width,
                self_size.height - status_size.height - bottom_height,
            ),
        );
        let panel_bottom_height = if data
            .panel
            .is_container_shown(&PanelContainerPosition::Bottom)
        {
            bottom_height
        } else {
            0.0
        };

        let main_split_size = Size::new(
            self_size.width - panel_left_width - panel_right_width,
            self_size.height
                - status_size.height
                - panel_bottom_height
                - title_height,
        );
        let main_split_bc = BoxConstraints::tight(main_split_size);
        let main_split_origin = Point::new(panel_left_width, title_height);
        data.main_split.update_split_layout_rect(
            *data.main_split.split_id,
            main_split_size.to_rect().with_origin(main_split_origin),
        );
        self.main_split.layout(ctx, &main_split_bc, data, env);
        self.main_split
            .set_origin(ctx, data, env, main_split_origin);

        if data.completion.status != CompletionStatus::Inactive {
            let completion_size = self.completion.layout(ctx, bc, data, env);
            let completion_origin = data.completion_origin(
                ctx.text(),
                self_size,
                completion_size,
                &data.config,
            );
            self.completion
                .set_origin(ctx, data, env, completion_origin);
        }

        if data.signature.status != SignatureStatus::Inactive {
            let signature_size = self.signature.layout(ctx, bc, data, env);
            let label_offset = self.signature.widget().label_offset;
            let signature_origin = data.signature_origin(
                ctx.text(),
                self_size,
                signature_size,
                label_offset,
                &data.config,
            );
            self.signature.set_origin(ctx, data, env, signature_origin);
        }

        if data.hover.status == HoverStatus::Done {
            self.hover.layout(ctx, bc, data, env);
            let hover_origin =
                data.hover_origin(ctx.text(), self_size, &data.config);
            self.hover.set_origin(ctx, data, env, hover_origin);
        }

        if data.rename.active {
            let rename_size = self.rename.layout(
                ctx,
                &BoxConstraints::tight(Size::new(200.0, 200.0)),
                data,
                env,
            );
            let rename_origin =
                data.rename_origin(ctx.text(), self_size, rename_size, &data.config);
            self.rename.set_origin(ctx, data, env, rename_origin);
        }

        if data.picker.active {
            let picker_size = self.picker.layout(ctx, bc, data, env);
            self.picker.set_origin(
                ctx,
                data,
                env,
                Point::new(
                    (self_size.width - picker_size.width) / 2.0,
                    (self_size.height - picker_size.height) / 3.0,
                ),
            );
        }

        if data.about.active {
            self.about.layout(ctx, bc, data, env);
            self.about.set_origin(ctx, data, env, Point::ZERO);
        }
        if data.alert.active {
            self.alert.layout(ctx, bc, data, env);
            self.alert.set_origin(ctx, data, env, Point::ZERO);
        }

        let message_size = self.message.layout(
            ctx,
            &BoxConstraints::new(
                Size::ZERO,
                Size::new(
                    self_size.width,
                    self_size.height - title_height - status_size.height - 20.0,
                ),
            ),
            data,
            env,
        );
        self.message.set_origin(
            ctx,
            data,
            env,
            Point::new(
                (self_size.width - message_size.width - 10.0).max(0.0),
                title_height + 10.0,
            ),
        );

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.main_split.paint(ctx, data, env);
        ctx.incr_alpha_depth();
        if data
            .panel
            .is_container_shown(&PanelContainerPosition::Bottom)
        {
            self.panel_bottom.paint(ctx, data, env);
        }
        if data.panel.is_container_shown(&PanelContainerPosition::Left) {
            self.panel_left.paint(ctx, data, env);
        }
        if data
            .panel
            .is_container_shown(&PanelContainerPosition::Right)
        {
            self.panel_right.paint(ctx, data, env);
        }
        if let Some(position) = self.current_bar_hover.as_ref() {
            let (p0, p1) = match position {
                PanelResizePosition::Left => {
                    let rect = self.panel_left.layout_rect();
                    if !data.panel.is_container_shown(&PanelContainerPosition::Left)
                    {
                        (Point::new(1.0, rect.y0), Point::new(1.0, rect.y1))
                    } else {
                        (
                            Point::new(rect.x1.round(), rect.y0),
                            Point::new(rect.x1.round(), rect.y1),
                        )
                    }
                }
                PanelResizePosition::Right => {
                    let rect = self.panel_right.layout_rect();
                    if !data
                        .panel
                        .is_container_shown(&PanelContainerPosition::Right)
                    {
                        (
                            Point::new(rect.x1 - 1.0, rect.y0),
                            Point::new(rect.x1 - 1.0, rect.y1),
                        )
                    } else {
                        (
                            Point::new(rect.x0.round(), rect.y0),
                            Point::new(rect.x0.round(), rect.y1),
                        )
                    }
                }
                PanelResizePosition::LeftSplit => {
                    let rect = self.panel_left.layout_rect();
                    (
                        Point::new(rect.x1.round(), rect.y0),
                        Point::new(rect.x1.round(), rect.y1),
                    )
                }
                PanelResizePosition::Bottom => {
                    let rect = self.panel_bottom.layout_rect();
                    if !data
                        .panel
                        .is_container_shown(&PanelContainerPosition::Bottom)
                    {
                        let status_rect = self.status.layout_rect();
                        (
                            Point::new(rect.x0, status_rect.y0 - 1.0),
                            Point::new(rect.x1, status_rect.y0 - 1.0),
                        )
                    } else {
                        (
                            Point::new(rect.x0, rect.y0.round()),
                            Point::new(rect.x1, rect.y0.round()),
                        )
                    }
                }
            };
            ctx.stroke(
                Line::new(p0, p1),
                data.config.get_color_unchecked(LapceTheme::EDITOR_CARET),
                2.0,
            );
        }
        self.title.paint(ctx, data, env);
        self.status.paint(ctx, data, env);
        if data.rename.active {
            let rect = self.rename.layout_rect();
            let shadow_width = data.config.ui.drop_shadow_width() as f64;
            if shadow_width > 0.0 {
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            } else {
                ctx.stroke(
                    rect.inflate(0.5, 0.5),
                    data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );
            self.rename.paint(ctx, data, env);
        }
        self.completion.paint(ctx, data, env);
        self.signature.paint(ctx, data, env);
        self.hover.paint(ctx, data, env);
        self.picker.paint(ctx, data, env);
        ctx.incr_alpha_depth();
        self.paint_drag_on_panel(ctx, data);
        self.paint_drag(ctx, data);
        ctx.incr_alpha_depth();
        self.about.paint(ctx, data, env);
        self.alert.paint(ctx, data, env);
        if self.message.widget().child().has_items() {
            self.message.paint(ctx, data, env);
        }
    }
}

/// The tab header of window tabs where you can click to focus and
/// drag to re order them
///
/// Each window tab hosts a separate workspace, which gives you an alternative
/// way to work with multiple workspaces.
pub struct LapceTabHeader {
    pub drag_start: Option<(Point, Point)>,
    pub mouse_pos: Point,
    close_icon_rect: Rect,
    holding_click_rect: Option<Rect>,
}

impl LapceTabHeader {
    pub fn new() -> Self {
        Self {
            close_icon_rect: Rect::ZERO,
            holding_click_rect: None,
            drag_start: None,
            mouse_pos: Point::ZERO,
        }
    }

    pub fn origin(&self) -> Option<Point> {
        self.drag_start
            .map(|(drag, origin)| origin + (self.mouse_pos - drag))
    }
}

impl Default for LapceTabHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for LapceTabHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if ctx.is_active() {
                    if let Some(_pos) = self.drag_start {
                        self.mouse_pos = ctx.to_window(mouse_event.pos);
                        ctx.request_layout();
                    }
                    return;
                }
                if self.close_icon_rect.contains(mouse_event.pos) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            Event::MouseDown(mouse_event) => {
                if mouse_event.button.is_left() {
                    if self.close_icon_rect.contains(mouse_event.pos) {
                        self.holding_click_rect = Some(self.close_icon_rect);
                    } else {
                        self.drag_start = Some((
                            ctx.to_window(mouse_event.pos),
                            ctx.window_origin(),
                        ));
                        self.mouse_pos = ctx.to_window(mouse_event.pos);
                        ctx.set_active(true);
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::FocusTabId(data.id),
                            Target::Auto,
                        ));
                    }
                }
            }
            Event::MouseUp(mouse_event) => {
                if mouse_event.button.is_right() {
                    let tab_id = data.id;
                    let window_id = *data.window_id;

                    let mut menu = druid::Menu::<LapceData>::new("Tab");
                    let item = druid::MenuItem::new("Move Tab To a New Window")
                        .on_activate(move |ctx, _data, _env| {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::TabToWindow(window_id, tab_id),
                                Target::Window(window_id),
                            ));
                        });
                    menu = menu.entry(item);

                    let item = druid::MenuItem::new("Close Tab").on_activate(
                        move |ctx, _data, _env| {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::CloseTabId(tab_id),
                                Target::Auto,
                            ));
                        },
                    );
                    menu = menu.entry(item);
                    ctx.show_context_menu::<LapceData>(
                        menu,
                        ctx.to_window(mouse_event.pos),
                    )
                } else {
                    if self.close_icon_rect.contains(mouse_event.pos)
                        && self.holding_click_rect.eq(&Some(self.close_icon_rect))
                    {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::CloseTabId(data.id),
                            Target::Auto,
                        ));
                    }
                    self.holding_click_rect = None;
                    ctx.set_active(false);
                    self.drag_start = None;
                    ctx.request_layout();
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::HotChanged(_is_hot) = event {
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        _ctx: &mut druid::UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let size = bc.max();

        let close_icon_width = size.height;
        let padding = 9.0;
        let origin = Point::new(size.width - 25.0, padding);
        self.close_icon_rect = Size::new(close_icon_width, close_icon_width)
            .to_rect()
            .inflate(-padding, -padding)
            .with_origin(origin);

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let tab_rect = ctx.size().to_rect();

        if ctx.is_hot() || self.drag_start.is_some() {
            // Currenlty, we only paint background for the hot tab to prevent showing
            // overlapped content on drag. In the future, we might want to:
            // - introduce a tab background color
            // - introduce a hover color
            ctx.fill(
                tab_rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );
        }

        const BORDER_PADDING: f64 = 8.0;

        ctx.stroke(
            Line::new(
                Point::new(tab_rect.x0, tab_rect.y0 + BORDER_PADDING),
                Point::new(tab_rect.x0, tab_rect.y1 - BORDER_PADDING),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        ctx.stroke(
            Line::new(
                Point::new(tab_rect.x1, tab_rect.y0 + BORDER_PADDING),
                Point::new(tab_rect.x1, tab_rect.y1 - BORDER_PADDING),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        let text_layout = ctx
            .text()
            .new_text_layout(
                workspace_title(&data.workspace)
                    .unwrap_or_else(|| String::from("Lapce")),
            )
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();

        let size = ctx.size();
        let text_size = text_layout.size();
        let x = (size.width - text_size.width) / 2.0;
        let y = text_layout.y_offset(size.height);
        ctx.draw_text(&text_layout, Point::new(x, y));

        if ctx.is_hot() || self.drag_start.is_some() {
            let svg = data.config.ui_svg(LapceIcons::CLOSE);
            ctx.draw_svg(
                &svg,
                self.close_icon_rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }
    }
}
