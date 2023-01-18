use std::{collections::HashMap, sync::Arc};

use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    index::{Column, Direction, Line, Side},
    selection::{Selection, SelectionType},
    term::{cell::Flags, search::RegexSearch, Term},
};
use druid::{
    piet::{PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder},
    widget::{Click, ControllerHost},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_core::{mode::Mode, register::Clipboard};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LapceWorkbenchCommand,
        LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::{FocusArea, LapceTabData},
    debug::RunDebugMode,
    document::SystemClipboard,
    panel::PanelKind,
    proxy::LapceProxy,
    terminal::{EventProxy, LapceTerminalData, LapceTerminalViewData},
};
use lapce_rpc::terminal::TermId;
use smallvec::SmallVec;
use unicode_width::UnicodeWidthChar;

use crate::{
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::{LapcePadding, LapceScroll},
    split::LapceSplit,
    svg::LapceIconSvg,
    tab::LapceIcon,
};

pub type TermConfig = alacritty_terminal::config::Config;

/// This struct represents the main body of the terminal, i.e. the part
/// where the shell is presented.
pub struct TerminalPanel {
    widget_id: WidgetId,
    tabs: HashMap<WidgetId, WidgetPod<LapceTabData, LapceSplit>>,
    header: WidgetPod<LapceTabData, LapceTerminalPanelHeader>,
}

impl TerminalPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let tabs = data
            .terminal
            .tabs
            .iter()
            .map(|(term_tab_id, tab)| {
                let mut split = LapceSplit::new(tab.split_id);
                for (_, term_data) in tab.terminals.iter() {
                    let term = LapceTerminalView::new(term_data);
                    split = split.with_flex_child(
                        term.boxed(),
                        Some(term_data.widget_id),
                        1.0,
                        true,
                    );
                }
                (*term_tab_id, WidgetPod::new(split))
            })
            .collect();
        let header = WidgetPod::new(LapceTerminalPanelHeader::new());
        Self {
            widget_id: data.terminal.widget_id,
            tabs,
            header,
        }
    }

    pub fn new_panel(data: &LapceTabData) -> LapcePanel {
        let split_id = WidgetId::next();
        LapcePanel::new(
            PanelKind::Terminal,
            data.terminal.widget_id,
            split_id,
            vec![(
                split_id,
                PanelHeaderKind::None,
                Self::new(data).boxed(),
                PanelSizing::Flex(true),
            )],
        )
    }

    fn handle_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        if let Some(term) = data.terminal.active_terminal() {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(term.widget_id),
            ));
        } else {
            let terminal_panel = Arc::make_mut(&mut data.terminal);
            terminal_panel.new_tab(
                data.workspace.clone(),
                data.proxy.clone(),
                &data.config,
                ctx.get_external_handle(),
                None,
            );
        }
    }
}

impl Widget<LapceTabData> for TerminalPanel {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.handle_focus(ctx, data);
                }
            }
            _ => (),
        }
        self.header.event(ctx, event, data, env);
        for (tab_id, tab) in self.tabs.iter_mut() {
            let active_id =
                data.terminal.active_terminal_split().map(|s| &s.split_id);
            if event.should_propagate_to_hidden() || Some(tab_id) == active_id {
                tab.event(ctx, event, data, env);
            }
        }

        // We're using SmallVec here because, strictly speaking, it's impossible to have more
        // than one empty TerminalSplitData at the same time. Thus `SmallVec<[WidgetId; 1]>`
        // will allow how to avoid unnecessary allocation and at the same time will not take
        // up more space on the stack than necessary.
        //
        // Note: Here we are using a SmallVec instead of iterating directly with deletion, since
        // that would require `Arc::make_mut` to be called every time, even if no changes were made.
        let empty_tabs = data
            .terminal
            .tabs
            .iter()
            .filter(|(_, t)| t.terminals.is_empty())
            .map(|(tab_id, _)| *tab_id)
            .collect::<SmallVec<[WidgetId; 1]>>();

        // We remove all empty entries, but at the same time it is necessary to synchronize
        // changes in `data.terminal.tabs` with `data.terminal.tabs.tabs_order`.
        // The `data.terminal.tabstabs_order` is a vector, so `removing` elements from it in
        // a loop is inefficient due to shifting all elements to the left after each removal.
        //
        // Therefore, we first store the first `WidgetId` to be removed in the `id_to_remove`
        // variable, and in the loop itself, instead of deleting, we will equate all the values
        // to be removed to `id_to_remove`. After the loop, we simply rebuild the vector again,
        // excluding all elements equal to `id_to_remove`.
        //
        // This will be equally effective with one removed element, and with more of them, as
        // it will allow us to allocate a vector with the desired capacity only once.
        if let Some(&id_to_remove) = empty_tabs.get(0) {
            let removed_len = empty_tabs.len();

            let terminal = Arc::make_mut(&mut data.terminal);
            let tabs_order = Arc::make_mut(&mut terminal.tabs_order);
            for tab_id in empty_tabs {
                self.tabs.remove(&tab_id);
                terminal.tabs.remove(&tab_id);

                if let Some(id) = tabs_order.iter_mut().find(|id| *id == &tab_id) {
                    *id = id_to_remove;
                }
            }
            let mut new_order: Vec<WidgetId> =
                Vec::with_capacity(tabs_order.len() - removed_len);
            new_order.extend(tabs_order.iter().filter(|id| *id != &id_to_remove));
            *tabs_order = new_order;

            ctx.children_changed();

            if tabs_order.is_empty() {
                if data.panel.is_panel_visible(&PanelKind::Terminal) {
                    Arc::make_mut(&mut data.panel).hide_panel(&PanelKind::Terminal);
                }
                if let Some(active) = *data.main_split.active_tab {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(active),
                    ));
                }
            } else {
                self.handle_focus(ctx, data);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.lifecycle(ctx, event, data, env);
        for (_, tab) in self.tabs.iter_mut() {
            tab.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.update(ctx, data, env);
        for (_, tab) in self.tabs.iter_mut() {
            tab.update(ctx, data, env);
        }
        if !data.terminal.same(&old_data.terminal) {
            if data.terminal.active_terminal_split().map(|s| &s.split_id)
                != old_data
                    .terminal
                    .active_terminal_split()
                    .map(|s| &s.split_id)
            {
                ctx.request_layout();
            }
            if !data.terminal.tabs_order.same(&old_data.terminal.tabs_order) {
                let mut changed = false;
                for (tab_id, tab) in data.terminal.tabs.iter() {
                    if !self.tabs.contains_key(tab_id) {
                        changed = true;
                        ctx.children_changed();
                        let mut split = LapceSplit::new(tab.split_id);
                        for (_, term_data) in tab.terminals.iter() {
                            let term = LapceTerminalView::new(term_data);
                            split = split.with_flex_child(
                                term.boxed(),
                                Some(term_data.widget_id),
                                1.0,
                                true,
                            );
                        }
                        self.tabs.insert(*tab_id, WidgetPod::new(split));
                    }
                }
                self.tabs.retain(|tab_id, _| {
                    if !data.terminal.tabs.contains_key(tab_id) {
                        changed = true;
                        ctx.children_changed();
                        return false;
                    }
                    true
                });
                if changed && !self.tabs.is_empty() {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(self.widget_id),
                    ));
                }
            }
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = bc.max();

        let header_size = self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);
        if let Some(tab) = data
            .terminal
            .active_terminal_split()
            .and_then(|s| self.tabs.get_mut(&s.split_id))
        {
            tab.layout(
                ctx,
                &BoxConstraints::tight(Size::new(
                    size.width,
                    size.height - header_size.height,
                )),
                data,
                env,
            );
            tab.set_origin(ctx, data, env, Point::new(0.0, header_size.height));
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND),
        );
        if let Some(tab) = data
            .terminal
            .active_terminal_split()
            .and_then(|s| self.tabs.get_mut(&s.split_id))
        {
            tab.paint(ctx, data, env);
        }
        self.header.paint(ctx, data, env);
    }
}

struct LapceTerminalPanelHeader {
    content: WidgetPod<
        LapceTabData,
        LapceScroll<LapceTabData, LapceTerminalPanelHeaderContent>,
    >,
    icon: WidgetPod<
        LapceTabData,
        ControllerHost<
            LapcePadding<LapceTabData, LapceIconSvg>,
            Click<LapceTabData>,
        >,
    >,
    icon_padding: f64,
    mouse_pos: Point,
}

impl LapceTerminalPanelHeader {
    fn new() -> Self {
        let content = WidgetPod::new(
            LapceScroll::new(LapceTerminalPanelHeaderContent::new())
                .vertical_scroll_for_horizontal(),
        );
        let icon_padding = 4.0;
        let icon = LapcePadding::new(4.0, LapceIconSvg::new(LapceIcons::ADD))
            .controller(Click::new(|ctx, _data, _env| {
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Workbench(
                            LapceWorkbenchCommand::NewTerminalTab,
                        ),
                        data: None,
                    },
                    Target::Auto,
                ));
            }));
        Self {
            content,
            icon: WidgetPod::new(icon),
            mouse_pos: Point::ZERO,
            icon_padding,
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalPanelHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if let Event::MouseMove(mouse_event) = event {
            self.mouse_pos = mouse_event.pos;
            if self.icon.layout_rect().contains(mouse_event.pos) {
                ctx.set_cursor(&druid::Cursor::Pointer);
            } else {
                ctx.clear_cursor();
            }
        }
        self.content.event(ctx, event, data, env);
        self.icon.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
        self.icon.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.update(ctx, data, env);
        self.icon.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let size = Size::new(bc.max().width, data.config.ui.header_height() as f64);

        self.content.layout(
            ctx,
            &BoxConstraints::tight(Size::new(size.width - size.height, size.height)),
            data,
            env,
        );
        self.content.set_origin(ctx, data, env, Point::ZERO);

        let icon_size = data.config.ui.icon_size() as f64;
        self.icon.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                icon_size + self.icon_padding * 2.0,
                icon_size + self.icon_padding * 2.0,
            )),
            data,
            env,
        );
        self.icon.set_origin(
            ctx,
            data,
            env,
            Point::new(
                size.width - size.height
                    + ((size.height - icon_size) / 2.0 - self.icon_padding),
                (size.height - icon_size) / 2.0 - self.icon_padding,
            ),
        );

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.content.paint(ctx, data, env);

        {
            let scroll_offset = self.content.widget().offset().x;
            let content_rect = self.content.layout_rect();
            let child_size = self.content.widget().child_size();
            if scroll_offset > 0.0 {
                ctx.with_save(|ctx| {
                    ctx.clip(content_rect);
                    let rect = Rect::new(
                        content_rect.x0 - 10.0,
                        content_rect.y0 - 10.0,
                        content_rect.x0,
                        content_rect.y1 + 10.0,
                    );
                    ctx.blurred_rect(
                        rect,
                        4.0,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                });
            }
            if scroll_offset < child_size.width - content_rect.width() {
                ctx.with_save(|ctx| {
                    ctx.clip(content_rect);
                    let rect = Rect::new(
                        content_rect.x1,
                        content_rect.y0 - 10.0,
                        content_rect.x1 + 10.0,
                        content_rect.y1 + 10.0,
                    );
                    ctx.blurred_rect(
                        rect,
                        4.0,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                });
            }
        }

        let icon_rect = self.icon.layout_rect();
        if icon_rect.contains(self.mouse_pos) {
            ctx.fill(
                icon_rect,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)
                    .clone()
                    .with_alpha(0.1),
            );
        }
        self.icon.paint(ctx, data, env);

        let size = ctx.size();
        let rect = size.to_rect();
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.with_save(|ctx| {
                ctx.clip(rect.inset((0.0, 0.0, 0.0, 50.0)));
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });
        } else {
            ctx.stroke(
                druid::kurbo::Line::new(
                    Point::new(rect.x0, rect.y1 + 0.5),
                    Point::new(rect.x1, rect.y1 + 0.5),
                ),
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }
    }
}

struct LapceTerminalPanelHeaderContent {
    items: HashMap<
        WidgetId,
        WidgetPod<LapceTabData, LapceTerminalPanelHeaderContentItem>,
    >,
}

impl LapceTerminalPanelHeaderContent {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalPanelHeaderContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        for (_, item) in self.items.iter_mut() {
            item.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for (_, item) in self.items.iter_mut() {
            item.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if !data.terminal.same(&old_data.terminal) || self.items.is_empty() {
            if !data.terminal.tabs.ptr_eq(&old_data.terminal.tabs) {
                for (_, item) in self.items.iter_mut() {
                    item.update(ctx, data, env);
                }
            }
            if !data.terminal.tabs_order.same(&old_data.terminal.tabs_order)
                || self.items.is_empty()
            {
                for (tab_id, tab) in data.terminal.tabs.iter() {
                    if !self.items.contains_key(tab_id) {
                        ctx.children_changed();
                        self.items.insert(
                            *tab_id,
                            WidgetPod::new(
                                LapceTerminalPanelHeaderContentItem::new(
                                    tab.split_id,
                                ),
                            ),
                        );
                    }
                }
                self.items.retain(|tab_id, _| {
                    if !data.terminal.tabs.contains_key(tab_id) {
                        ctx.children_changed();
                        return false;
                    }
                    true
                });
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let mut total_width = 0.0;
        for id in data.terminal.tabs_order.iter() {
            if let Some(item) = self.items.get_mut(id) {
                let size = item.layout(ctx, bc, data, env);
                item.set_origin(ctx, data, env, Point::new(total_width, 0.0));
                total_width += size.width;
            }
        }
        Size::new(total_width, bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let active_index = data
            .terminal
            .active
            .min(data.terminal.tabs_order.len().saturating_sub(1));
        for (i, id) in data.terminal.tabs_order.iter().enumerate() {
            if let Some(item) = self.items.get_mut(id) {
                item.paint(ctx, data, env);
                let rect = item.layout_rect();
                let x = rect.x1;
                let size = rect.size();
                ctx.stroke(
                    druid::kurbo::Line::new(
                        Point::new(x - 0.5, (size.height * 0.8).round()),
                        Point::new(
                            x - 0.5,
                            size.height - (size.height * 0.8).round(),
                        ),
                    ),
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_TAB_SEPARATOR),
                    1.0,
                );
                if i == active_index {
                    let stroke = if data.focus_area
                        == FocusArea::Panel(PanelKind::Terminal)
                    {
                        data.config.get_color_unchecked(
                            LapceTheme::LAPCE_TAB_ACTIVE_UNDERLINE,
                        )
                    } else {
                        data.config.get_color_unchecked(
                            LapceTheme::LAPCE_TAB_INACTIVE_UNDERLINE,
                        )
                    };
                    ctx.stroke(
                        druid::kurbo::Line::new(
                            Point::new(rect.x0 + 2.0, rect.y1 - 1.0),
                            Point::new(rect.x1 - 2.0, rect.y1 - 1.0),
                        ),
                        stroke,
                        2.0,
                    );
                }
            }
        }
    }
}

struct LapceTerminalPanelHeaderContentItem {
    text_layout: Option<PietTextLayout>,
    split_id: WidgetId,
    padding: f64,
    icon_padding: f64,
    title_width: f64,
    mouse_pos: Point,
    close: WidgetPod<
        LapceTabData,
        ControllerHost<
            LapcePadding<LapceTabData, LapceIconSvg>,
            Click<LapceTabData>,
        >,
    >,
}

impl LapceTerminalPanelHeaderContentItem {
    fn new(split_id: WidgetId) -> Self {
        let padding = 10.0;
        let icon_padding = 4.0;
        let icon = LapcePadding::new(4.0, LapceIconSvg::new(LapceIcons::CLOSE))
            .controller(Click::new(move |ctx, _data, _env| {
                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Workbench(
                            LapceWorkbenchCommand::CloseTerminalTab,
                        ),
                        data: Some(serde_json::json!(split_id.to_usize())),
                    },
                    Target::Auto,
                ));
            }));
        Self {
            text_layout: None,
            split_id,
            mouse_pos: Point::ZERO,
            padding,
            icon_padding,
            title_width: 120.0,
            close: WidgetPod::new(icon),
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalPanelHeaderContentItem {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                ctx.set_cursor(&druid::Cursor::Pointer);
            }
            Event::MouseDown(mouse_event) => {
                if !self.close.layout_rect().contains(mouse_event.pos) {
                    if let Some(i) = data
                        .terminal
                        .tabs_order
                        .iter()
                        .position(|t| t == &self.split_id)
                    {
                        let terminal = Arc::make_mut(&mut data.terminal);
                        terminal.active = i;
                    }
                }
            }
            _ => (),
        }
        self.close.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.close.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.close.update(ctx, data, env);
        let old_title = old_data
            .terminal
            .tabs
            .get(&self.split_id)
            .and_then(|t| t.active_terminal())
            .map(|t| &t.title);
        let new_title = data
            .terminal
            .tabs
            .get(&self.split_id)
            .and_then(|t| t.active_terminal())
            .map(|t| &t.title);
        if old_title != new_title {
            ctx.request_layout();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let title = data
            .terminal
            .tabs
            .get(&self.split_id)
            .and_then(|t| t.active_terminal())
            .map(|t| {
                let title = if let Some(run_debug) = t.run_debug.as_ref() {
                    run_debug.config.name.clone()
                } else if !t.title.is_empty() {
                    t.title.clone()
                } else {
                    "Terminal".to_string()
                };
                title
            });
        let text = match title {
            Some(title) => title,
            None => return Size::new(0.0, bc.max().height),
        };

        let text_color = data
            .config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone();

        self.text_layout = Some({
            let text_layout = ctx
                .text()
                .new_text_layout(text.clone())
                .font(
                    data.config.ui.font_family(),
                    data.config.ui.font_size() as f64,
                )
                .text_color(text_color.clone())
                .build()
                .unwrap();

            if text_layout.layout.width() > self.title_width as f32 {
                let ending = ctx
                    .text()
                    .new_text_layout("...")
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .build()
                    .unwrap();
                let ending_width = ending.size().width;

                let hit_point = text_layout.hit_test_point(Point::new(
                    self.title_width - ending_width,
                    0.0,
                ));

                ctx.text()
                    .new_text_layout(format!("{}...", &text[..hit_point.idx]))
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(text_color)
                    .build()
                    .unwrap()
            } else {
                text_layout
            }
        });

        let height = bc.max().height;

        let icon_size = data.config.ui.icon_size() as f64;
        self.close.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                icon_size + self.icon_padding * 2.0,
                icon_size + self.icon_padding * 2.0,
            )),
            data,
            env,
        );
        self.close.set_origin(
            ctx,
            data,
            env,
            Point::new(
                self.padding
                    + icon_size
                    + self.padding
                    + self.title_width
                    + self.padding
                    - self.icon_padding,
                (height - icon_size) / 2.0 - self.icon_padding,
            ),
        );

        let width = self.padding
            + icon_size
            + self.padding
            + self.title_width
            + icon_size
            + self.padding * 2.0;

        Size::new(width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();

        let icon_size = data.config.ui.icon_size() as f64;

        let icon = data
            .terminal
            .tabs
            .get(&self.split_id)
            .and_then(|t| t.active_terminal())
            .map(|t| {
                let icon = if let Some(run_debug) = t.run_debug.as_ref() {
                    match run_debug.mode {
                        RunDebugMode::Run => {
                            if run_debug.stopped {
                                LapceIcons::RUN_ERRORS
                            } else {
                                LapceIcons::START
                            }
                        }
                        RunDebugMode::Debug => LapceIcons::DEBUG,
                    }
                } else {
                    LapceIcons::TERMINAL
                };
                icon
            })
            .unwrap_or(LapceIcons::TERMINAL);

        let icon_rect = Rect::ZERO
            .with_origin(Point::new(
                self.padding + icon_size / 2.0,
                size.height / 2.0,
            ))
            .inflate(icon_size / 2.0, icon_size / 2.0);
        ctx.draw_svg(
            &data.config.ui_svg(icon),
            icon_rect,
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
            ),
        );

        let text_layout = self.text_layout.as_ref().unwrap();
        ctx.draw_text(
            text_layout,
            Point::new(
                self.padding + icon_size + self.padding,
                text_layout.y_offset(size.height),
            ),
        );

        let icon_rect = self.close.layout_rect();
        if icon_rect.contains(self.mouse_pos) {
            ctx.fill(
                icon_rect,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)
                    .clone()
                    .with_alpha(0.1),
            );
        }
        self.close.paint(ctx, data, env);
    }
}

pub struct LapceTerminalView {
    header: WidgetPod<LapceTabData, LapceTerminalHeader>,
    terminal: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl LapceTerminalView {
    pub fn new(data: &LapceTerminalData) -> Self {
        let header = LapceTerminalHeader::new(data);
        let terminal = LapcePadding::new(10.0, LapceTerminal::new(data));
        Self {
            header: WidgetPod::new(header),
            terminal: WidgetPod::new(terminal.boxed()),
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalView {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.header.event(ctx, event, data, env);
        self.terminal.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::HotChanged(is_hot) = event {
            self.header.widget_mut().view_is_hot = *is_hot;
            ctx.request_paint();
        }
        self.header.lifecycle(ctx, event, data, env);
        self.terminal.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.header.update(ctx, data, env);
        self.terminal.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        self.header.layout(ctx, bc, data, env);
        self.header.set_origin(ctx, data, env, Point::ZERO);

        self.terminal.layout(ctx, bc, data, env);
        self.terminal.set_origin(ctx, data, env, Point::ZERO);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.terminal.paint(ctx, data, env);
        self.header.paint(ctx, data, env);
    }
}

struct LapceTerminalHeader {
    term_id: TermId,
    split_id: WidgetId,
    height: f64,
    icon_size: f64,
    icon_padding: f64,
    icons: Vec<LapceIcon>,
    mouse_pos: Point,
    view_is_hot: bool,
    hover_rect: Option<Rect>,
}

impl LapceTerminalHeader {
    pub fn new(data: &LapceTerminalData) -> Self {
        Self {
            term_id: data.term_id,
            split_id: data.split_id,
            height: 30.0,
            icon_size: 24.0,
            mouse_pos: Point::ZERO,
            icon_padding: 4.0,
            icons: Vec::new(),
            view_is_hot: false,
            hover_rect: None,
        }
    }

    fn get_icons(&self, self_size: Size, data: &LapceTabData) -> Vec<LapceIcon> {
        let gap = (self.height - self.icon_size) / 2.0;

        let terminal_data = data
            .terminal
            .tabs
            .get(&self.split_id)
            .unwrap()
            .terminals
            .get(&self.term_id)
            .unwrap();

        let mut icons = Vec::new();
        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::CLOSE,
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::CloseTerminal(self.term_id),
                Target::Widget(data.id),
            ),
        };
        icons.push(icon);

        let x =
            self_size.width - ((icons.len() + 1) as f64) * (gap + self.icon_size);
        let icon = LapceIcon {
            icon: LapceIcons::SPLIT_HORIZONTAL,
            rect: Size::new(self.icon_size, self.icon_size)
                .to_rect()
                .with_origin(Point::new(x, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitTerminal(true, terminal_data.widget_id),
                Target::Widget(terminal_data.split_id),
            ),
        };
        icons.push(icon);

        icons
    }

    fn icon_hit_test(&mut self, mouse_event: &MouseEvent) -> bool {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                self.hover_rect = Some(icon.rect);
                return true;
            }
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for icon in self.icons.iter() {
            if icon.rect.contains(mouse_event.pos) {
                ctx.submit_command(icon.command.clone());
            }
        }
    }
}

impl Widget<LapceTabData> for LapceTerminalHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = mouse_event.pos;
                let hover_rect = self.hover_rect;
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    self.hover_rect = None;
                    ctx.clear_cursor();
                }
                if hover_rect != self.hover_rect {
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, mouse_event);
            }
            _ => {}
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
        let self_size = Size::new(bc.max().width, self.height);
        self.icons = self.get_icons(self_size, data);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if self.view_is_hot {
            for icon in self.icons.iter() {
                if icon.rect.contains(self.mouse_pos) {
                    ctx.fill(
                        icon.rect,
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
                    );
                }
                {
                    let svg = data.config.ui_svg(icon.icon);
                    ctx.draw_svg(
                        &svg,
                        icon.rect.inflate(-self.icon_padding, -self.icon_padding),
                        Some(
                            data.config
                                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                        ),
                    );
                }
            }
        }
    }
}

struct LapceTerminal {
    term_id: TermId,
    widget_id: WidgetId,
    split_id: WidgetId,
    width: f64,
    height: f64,
    proxy: Arc<LapceProxy>,
}

impl Drop for LapceTerminal {
    fn drop(&mut self) {
        self.proxy.proxy_rpc.terminal_close(self.term_id);
    }
}

impl LapceTerminal {
    pub fn new(data: &LapceTerminalData) -> Self {
        Self {
            term_id: data.term_id,
            widget_id: data.widget_id,
            split_id: data.split_id,
            proxy: data.proxy.clone(),
            width: 0.0,
            height: 0.0,
        }
    }

    pub fn request_focus(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        ctx.request_focus();
        let terminal_panel = Arc::make_mut(&mut data.terminal);
        terminal_panel.active = terminal_panel
            .tabs_order
            .iter()
            .position(|id| id == &self.split_id)
            .unwrap_or(terminal_panel.active);
        let terminal_split = terminal_panel.tabs.get_mut(&self.split_id).unwrap();
        terminal_split.active = self.widget_id;
        terminal_split.active_term_id = self.term_id;
        data.focus = Arc::new(self.widget_id);
        data.focus_area = FocusArea::Panel(PanelKind::Terminal);
        if let Some((index, position)) =
            data.panel.panel_position(&PanelKind::Terminal)
        {
            let panel = Arc::make_mut(&mut data.panel);
            if let Some(style) = panel.style.get_mut(&position) {
                style.active = index;
            }
            panel.active = position;
        }
        if let Some(terminal) = terminal_panel.get_terminal_mut(&self.term_id) {
            if terminal.run_debug.is_some() {
                Arc::make_mut(&mut data.debug).active_term = Some(self.term_id);
            }
        }
    }

    fn select(
        &self,
        term: &mut Term<EventProxy>,
        mouse_event: &MouseEvent,
        ty: SelectionType,
    ) {
        let row_size = self.height / term.screen_lines() as f64;
        let col_size = self.width / term.columns() as f64;
        let offset = term.grid().display_offset();
        let column = Column((mouse_event.pos.x / col_size) as usize);
        let line = Line((mouse_event.pos.y / row_size) as i32 - offset as i32);
        match &mut term.selection {
            Some(selection) => selection.update(
                alacritty_terminal::index::Point { line, column },
                Direction::Left,
            ),
            None => {
                term.selection = Some(Selection::new(
                    ty,
                    alacritty_terminal::index::Point { line, column },
                    Direction::Left,
                ));
            }
        }
    }
}

impl Widget<LapceTabData> for LapceTerminal {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        let old_terminal_data = data
            .terminal
            .tabs
            .get(&self.split_id)
            .and_then(|split| split.terminals.get(&self.term_id).cloned());
        let old_terminal_data = match old_terminal_data {
            Some(t) => t,
            None => return,
        };

        let mut term_data = LapceTerminalViewData {
            terminal: old_terminal_data.clone(),
            config: data.config.clone(),
            find: data.find.clone(),
        };
        ctx.set_cursor(&Cursor::IBeam);
        match event {
            Event::MouseDown(mouse_event) => {
                self.request_focus(ctx, data);
                let terminal = old_terminal_data.clone();
                let term = &mut terminal.raw.lock().term;
                if mouse_event.button.is_right() {
                    let mut clipboard = SystemClipboard {};
                    match term.selection_to_string() {
                        Some(selection) => {
                            clipboard.put_string(selection);
                            term.selection = None;
                        }
                        None => {
                            if let Some(string) = clipboard.get_string() {
                                terminal.proxy.proxy_rpc.terminal_write(
                                    terminal.term_id,
                                    string.as_str(),
                                );
                                term.scroll_display(Scroll::Bottom);
                            }
                        }
                    }
                } else if mouse_event.button.is_left() {
                    match mouse_event.count {
                        2 => self.select(term, mouse_event, SelectionType::Semantic),
                        _ => {
                            term.selection = None;
                            if mouse_event.count == 3 {
                                self.select(term, mouse_event, SelectionType::Lines);
                            }
                        }
                    }
                }
            }
            Event::MouseMove(mouse_event) => {
                if mouse_event.buttons.has_left() {
                    let terminal = old_terminal_data.clone();
                    let term = &mut terminal.raw.lock().term;
                    self.select(term, mouse_event, SelectionType::Simple);
                    ctx.request_paint();
                }
            }
            Event::Wheel(wheel_event) => {
                old_terminal_data.wheel_scroll(wheel_event.wheel_delta.y);
                ctx.request_paint();
            }
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                if !Arc::make_mut(&mut keypress).key_down(
                    ctx,
                    key_event,
                    &mut term_data,
                    env,
                ) && term_data.terminal.mode == Mode::Terminal
                {
                    term_data.send_keypress(key_event);
                }
                ctx.set_handled();
                data.keypress = keypress.clone();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx, data);
                }
            }
            _ => (),
        }
        if !term_data.terminal.same(&old_terminal_data) {
            Arc::make_mut(&mut data.terminal)
                .tabs
                .get_mut(&self.split_id)
                .unwrap()
                .terminals
                .insert(term_data.terminal.term_id, term_data.terminal.clone());
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        if let LifeCycle::FocusChanged(_) = event {
            ctx.request_paint();
        }
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let size = bc.max();
        if self.width != size.width || self.height != size.height {
            self.width = size.width;
            self.height = size.height;
            let width = data.config.terminal_char_width(ctx.text());
            let line_height = data.config.terminal_line_height() as f64;
            let width = if width > 0.0 {
                (self.width / width).floor() as usize
            } else {
                0
            };
            let height = (self.height / line_height).floor() as usize;
            data.terminal
                .tabs
                .get(&self.split_id)
                .unwrap()
                .terminals
                .get(&self.term_id)
                .unwrap()
                .resize(width, height);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let char_size = data.config.terminal_text_size(ctx.text(), "W");
        let char_width = char_size.width;
        let line_height = data.config.terminal_line_height() as f64;

        let terminal = data
            .terminal
            .tabs
            .get(&self.split_id)
            .unwrap()
            .terminals
            .get(&self.term_id)
            .unwrap();
        let raw = terminal.raw.lock();
        let term = &raw.term;
        let content = term.renderable_content();

        if let Some(selection) = content.selection.as_ref() {
            let start_line = selection.start.line.0 + content.display_offset as i32;
            let start_line = if start_line < 0 {
                0
            } else {
                start_line as usize
            };
            let start_col = selection.start.column.0;

            let end_line = selection.end.line.0 + content.display_offset as i32;
            let end_line = if end_line < 0 { 0 } else { end_line as usize };
            let end_col = selection.end.column.0;

            for line in start_line..end_line + 1 {
                let left_col = if selection.is_block || line == start_line {
                    start_col
                } else {
                    0
                };
                let right_col = if selection.is_block || line == end_line {
                    end_col + 1
                } else {
                    term.last_column().0
                };
                let x0 = left_col as f64 * char_width;
                let x1 = right_col as f64 * char_width;
                let y0 = line as f64 * line_height;
                let y1 = y0 + line_height;
                ctx.fill(
                    Rect::new(x0, y0, x1, y1),
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_SELECTION),
                );
            }
        } else if terminal.mode != Mode::Terminal {
            let y = (content.cursor.point.line.0 as f64
                + content.display_offset as f64)
                * line_height;
            let size = ctx.size();
            ctx.fill(
                Rect::new(0.0, y, size.width, y + line_height),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_CURRENT_LINE),
            );
        }

        let cursor_point = &content.cursor.point;

        let term_bg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_BACKGROUND)
            .clone();
        let _term_fg = data
            .config
            .get_color_unchecked(LapceTheme::TERMINAL_FOREGROUND)
            .clone();
        for item in content.display_iter {
            let point = item.point;
            let cell = item.cell;
            let inverse = cell.flags.contains(Flags::INVERSE);

            let x = point.column.0 as f64 * char_width;
            let y =
                (point.line.0 as f64 + content.display_offset as f64) * line_height;

            let mut bg = data.terminal.tabs.get(&self.split_id).unwrap().get_color(
                &cell.bg,
                content.colors,
                &data.config,
            );
            let mut fg = data.terminal.tabs.get(&self.split_id).unwrap().get_color(
                &cell.fg,
                content.colors,
                &data.config,
            );
            if cell.flags.contains(Flags::DIM)
                || cell.flags.contains(Flags::DIM_BOLD)
            {
                fg = fg.with_alpha(0.66);
            }

            if inverse {
                let fg_clone = fg.clone();
                fg = bg;
                bg = fg_clone;
            }

            if term_bg != bg {
                let rect = Size::new(char_width, line_height)
                    .to_rect()
                    .with_origin(Point::new(x, y));
                ctx.fill(rect, &bg);
            }

            if cursor_point == &point {
                let rect = Size::new(
                    char_width * cell.c.width().unwrap_or(1) as f64,
                    line_height,
                )
                .to_rect()
                .with_origin(Point::new(
                    cursor_point.column.0 as f64 * char_width,
                    (cursor_point.line.0 as f64 + content.display_offset as f64)
                        * line_height,
                ));
                let cursor_color = if terminal.mode == Mode::Terminal {
                    data.config.get_color_unchecked(LapceTheme::TERMINAL_CURSOR)
                } else {
                    data.config.get_color_unchecked(LapceTheme::EDITOR_CARET)
                };
                if ctx.is_focused() {
                    ctx.fill(rect, cursor_color);
                } else {
                    ctx.stroke(rect, cursor_color, 1.0);
                }
            }

            let bold = cell.flags.contains(Flags::BOLD)
                || cell.flags.contains(Flags::DIM_BOLD);

            if &point == cursor_point && ctx.is_focused() {
                fg = term_bg.clone();
            }

            if cell.c != ' ' && cell.c != '\t' {
                let mut builder = ctx
                    .text()
                    .new_text_layout(cell.c.to_string())
                    .font(
                        data.config.terminal_font_family(),
                        data.config.terminal_font_size() as f64,
                    )
                    .text_color(fg);
                if bold {
                    builder = builder
                        .default_attribute(TextAttribute::Weight(FontWeight::BOLD));
                }
                let text_layout = builder.build().unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(x, y + text_layout.y_offset(line_height)),
                );
            }
        }
        if data.find.visual {
            if let Some(search_string) = data.find.search_string.as_ref() {
                if let Ok(dfas) = RegexSearch::new(&regex::escape(search_string)) {
                    let mut start = alacritty_terminal::index::Point::new(
                        alacritty_terminal::index::Line(
                            -(content.display_offset as i32),
                        ),
                        alacritty_terminal::index::Column(0),
                    );
                    let end_line = (start.line + term.screen_lines())
                        .min(term.bottommost_line());
                    let mut max_lines = (end_line.0 - start.line.0) as usize;

                    while let Some(m) = term.search_next(
                        &dfas,
                        start,
                        Direction::Right,
                        Side::Left,
                        Some(max_lines),
                    ) {
                        let match_start = m.start();
                        if match_start.line.0 < start.line.0
                            || (match_start.line.0 == start.line.0
                                && match_start.column.0 < start.column.0)
                        {
                            break;
                        }
                        let x = match_start.column.0 as f64 * char_width;
                        let y = (match_start.line.0 as f64
                            + content.display_offset as f64)
                            * line_height;
                        let rect = Rect::ZERO
                            .with_origin(Point::new(x, y))
                            .with_size(Size::new(
                                (m.end().column.0 - m.start().column.0
                                    + term.grid()[*m.end()].c.width().unwrap_or(1))
                                    as f64
                                    * char_width,
                                line_height,
                            ));
                        ctx.stroke(
                            rect,
                            data.config.get_color_unchecked(
                                LapceTheme::TERMINAL_FOREGROUND,
                            ),
                            1.0,
                        );
                        start = *m.end();
                        if start.column.0 < term.last_column() {
                            start.column.0 += 1;
                        } else if start.line.0 < term.bottommost_line() {
                            start.column.0 = 0;
                            start.line.0 += 1;
                        }
                        max_lines = (end_line.0 - start.line.0) as usize;
                    }
                }
            }
        }
    }
}
