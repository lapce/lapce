use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use druid::{
    kurbo::Line,
    piet::{Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};
use lapce_proxy::dispatch::FileNodeItem;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceTabData,
    explorer::{get_item_children, get_item_children_mut, paint_file_node_item},
    scroll::LapceScrollNew,
    state::LapceWorkspace,
    svg::get_svg,
    tab::LapceButton,
};

#[derive(Clone)]
pub struct FilePickerData {
    pub widget_id: WidgetId,
    pub active: bool,
    root: FileNodeItem,
    pub home: PathBuf,
    pub pwd: PathBuf,
    index: usize,
}

impl FilePickerData {
    pub fn new() -> Self {
        let root = FileNodeItem {
            path_buf: PathBuf::from("/"),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        };
        let home = PathBuf::from("/");
        let pwd = PathBuf::from("/");
        Self {
            widget_id: WidgetId::next(),
            active: false,
            root,
            home,
            pwd,
            index: 0,
        }
    }

    pub fn set_item_children(
        &mut self,
        path: &PathBuf,
        children: HashMap<PathBuf, FileNodeItem>,
    ) {
        if let Some(node) = self.get_file_node_mut(path) {
            node.open = true;
            node.read = true;
            node.children = children;
        }

        for p in path.ancestors() {
            self.update_node_count(&PathBuf::from(p));
        }
    }

    pub fn init_home(&mut self, home: &PathBuf) {
        self.home = home.clone();
        let mut current_file_node = FileNodeItem {
            path_buf: home.clone(),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        };
        let mut current_path = home.clone();

        let mut ancestors = home.ancestors();
        ancestors.next();

        for p in ancestors {
            let mut file_node = FileNodeItem {
                path_buf: PathBuf::from(p),
                is_dir: true,
                read: false,
                open: true,
                children: HashMap::new(),
                children_open_count: 0,
            };
            file_node
                .children
                .insert(current_path.clone(), current_file_node.clone());
            current_file_node = file_node;
            current_path = PathBuf::from(p);
        }
        self.root = current_file_node;
        self.pwd = home.clone();

        println!("init home {:?}", self.root);
    }

    pub fn get_file_node_mut(
        &mut self,
        path: &PathBuf,
    ) -> Option<&mut FileNodeItem> {
        let mut node = Some(&mut self.root);

        let ancestors = path.ancestors().collect::<Vec<&Path>>();
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get_mut(&PathBuf::from(p))?);
        }
        node
    }

    pub fn get_file_node(&self, path: &PathBuf) -> Option<&FileNodeItem> {
        let mut node = Some(&self.root);

        let ancestors = path.ancestors().collect::<Vec<&Path>>();
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get(&PathBuf::from(p))?);
        }
        node
    }

    pub fn update_node_count(&mut self, path: &PathBuf) -> Option<()> {
        let node = self.get_file_node_mut(path)?;
        if node.is_dir {
            if node.open {
                node.children_open_count = node
                    .children
                    .iter()
                    .map(|(_, item)| item.children_open_count + 1)
                    .sum::<usize>();
            } else {
                node.children_open_count = 0;
            }
        }
        None
    }
}

pub struct FilePicker {
    widget_id: WidgetId,
    pwd: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    explorer: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    control: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl FilePicker {
    pub fn new(widget_id: WidgetId) -> Self {
        let pwd = FilePickerPwd::new();
        let explorer = LapceScrollNew::new(FilePickerExplorer::new());
        let control = FilePickerControl::new();
        Self {
            widget_id,
            pwd: WidgetPod::new(pwd.boxed()),
            explorer: WidgetPod::new(explorer.boxed()),
            control: WidgetPod::new(control.boxed()),
        }
    }
}

impl Widget<LapceTabData> for FilePicker {
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
        if data.picker.active {
            self.pwd.event(ctx, event, data, env);
            self.explorer.event(ctx, event, data, env);
            self.control.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.pwd.lifecycle(ctx, event, data, env);
        self.explorer.lifecycle(ctx, event, data, env);
        self.control.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.pwd.update(ctx, data, env);
        self.explorer.update(ctx, data, env);
        self.control.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = Size::new(500.0, 400.0);

        let pwd_size =
            self.pwd
                .layout(ctx, &BoxConstraints::tight(self_size), data, env);
        self.pwd.set_origin(ctx, data, env, Point::ZERO);

        let control_size =
            self.control
                .layout(ctx, &BoxConstraints::tight(self_size), data, env);
        self.control.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, self_size.height - control_size.height),
        );

        self.explorer.layout(
            ctx,
            &BoxConstraints::tight(Size::new(
                self_size.width,
                self_size.height - pwd_size.height - control_size.height,
            )),
            data,
            env,
        );
        self.explorer
            .set_origin(ctx, data, env, Point::new(0.0, pwd_size.height));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !data.picker.active {
            return;
        }

        let rect = ctx.size().to_rect();

        let shadow_width = 5.0;
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );

        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );

        self.pwd.paint(ctx, data, env);
        self.explorer.paint(ctx, data, env);
        self.control.paint(ctx, data, env);
    }
}

pub struct FilePickerPwd {
    icons: Vec<(Rect, Svg)>,
}

impl Widget<LapceTabData> for FilePickerPwd {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.mouse_down(ctx, data, mouse_event);
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
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = Size::new(bc.max().width, 40.0);
        let line_height = data.config.editor.line_height as f64;

        let icon_size = line_height;
        let gap = (self_size.height - icon_size) / 2.0;

        self.icons.clear();

        let x =
            self_size.width - ((self.icons.len() + 1) as f64) * (gap + icon_size);
        let rect = Size::new(icon_size, icon_size)
            .to_rect()
            .with_origin(Point::new(x, gap));
        let svg = get_svg("arrow-up.svg").unwrap();
        self.icons.push((rect, svg));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let line_height = data.config.editor.line_height as f64;

        if let Some(path) = data.picker.pwd.to_str() {
            let text_layout = ctx
                .text()
                .new_text_layout(path.to_string())
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(20.0, (size.height - text_layout.size().height) / 2.0),
            );
        }

        for (rect, svg) in self.icons.iter() {
            ctx.stroke(
                rect,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            ctx.draw_svg(
                svg,
                rect.inflate(-5.0, -5.0),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }

        ctx.stroke(
            Line::new(
                Point::new(0.0, size.height - 0.5),
                Point::new(size.width, size.height - 0.5),
            ),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
    }
}

impl FilePickerPwd {
    pub fn new() -> Self {
        Self { icons: Vec::new() }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for (rect, _) in self.icons.iter() {
            if rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        for (i, (rect, _)) in self.icons.iter().enumerate() {
            if rect.contains(mouse_event.pos) {
                match i {
                    0 => {
                        let picker = Arc::make_mut(&mut data.picker);
                        if let Some(parent) = picker.pwd.parent() {
                            let path = PathBuf::from(parent);
                            let tab_id = data.id;
                            let event_sink = ctx.get_external_handle();
                            picker.pwd = path.clone();
                            data.proxy.read_dir(
                                &path.clone(),
                                Box::new(move |result| {
                                    if let Ok(res) = result {
                                        let resp: Result<
                                            Vec<FileNodeItem>,
                                            serde_json::Error,
                                        > = serde_json::from_value(res);
                                        if let Ok(items) = resp {
                                            event_sink.submit_command(
                                                LAPCE_UI_COMMAND,
                                                LapceUICommand::UpdatePickerItems(
                                                    path,
                                                    items
                                                        .iter()
                                                        .map(|item| {
                                                            (
                                                                item.path_buf
                                                                    .clone(),
                                                                item.clone(),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                Target::Widget(tab_id),
                                            );
                                        }
                                    }
                                }),
                            );
                        }
                    }
                    _ => (),
                }
            }
        }
    }
}

pub struct FilePickerExplorer {
    toggle_rects: HashMap<usize, Rect>,
    last_left_click: Option<(usize, std::time::Instant)>,
}

impl FilePickerExplorer {
    pub fn new() -> Self {
        Self {
            toggle_rects: HashMap::new(),
            last_left_click: None,
        }
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        ctx.set_handled();
        let line_height = data.config.editor.line_height as f64;
        let picker = Arc::make_mut(&mut data.picker);
        let pwd = picker.pwd.clone();
        let index = ((mouse_event.pos.y + line_height) / line_height) as usize;
        if let Some(item) = picker.get_file_node_mut(&pwd) {
            let (_, node) = get_item_children_mut(0, index, item);
            if let Some(node) = node {
                if node.is_dir {
                    let mut clicked_toogle = false;
                    if let Some(rect) = self.toggle_rects.get(&index) {
                        if rect.contains(mouse_event.pos) {
                            clicked_toogle = true;
                            if node.read {
                                node.open = !node.open;
                            } else {
                                let tab_id = data.id;
                                let path = node.path_buf.clone();
                                let event_sink = ctx.get_external_handle();
                                data.proxy.read_dir(
                                    &node.path_buf,
                                    Box::new(move |result| {
                                        if let Ok(res) = result {
                                            let resp: Result<
                                                Vec<FileNodeItem>,
                                                serde_json::Error,
                                            > = serde_json::from_value(res);
                                            if let Ok(items) = resp {
                                                event_sink.submit_command(
                                                LAPCE_UI_COMMAND,
                                                LapceUICommand::UpdatePickerItems(
                                                    path,
                                                    items
                                                        .iter()
                                                        .map(|item| {
                                                            (
                                                                item.path_buf
                                                                    .clone(),
                                                                item.clone(),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                Target::Widget(tab_id),
                                            );
                                            }
                                        }
                                    }),
                                );
                            }
                        }
                    }
                    let mut last_left_click =
                        Some((index, std::time::Instant::now()));
                    if !clicked_toogle {
                        if let Some((i, t)) = self.last_left_click.as_ref() {
                            if *i == index {
                                if t.elapsed().as_millis() < 500 {
                                    // double click
                                    self.last_left_click = None;
                                    let tab_id = data.id;
                                    let path = node.path_buf.clone();
                                    let event_sink = ctx.get_external_handle();
                                    data.proxy.read_dir(
                                        &node.path_buf,
                                        Box::new(move |result| {
                                            if let Ok(res) = result {
                                                let resp: Result<
                                                    Vec<FileNodeItem>,
                                                    serde_json::Error,
                                                > = serde_json::from_value(res);
                                                if let Ok(items) = resp {
                                                    event_sink.submit_command(
                                                LAPCE_UI_COMMAND,
                                                LapceUICommand::UpdatePickerItems(
                                                    path,
                                                    items
                                                        .iter()
                                                        .map(|item| {
                                                            (
                                                                item.path_buf
                                                                    .clone(),
                                                                item.clone(),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                Target::Widget(tab_id),
                                            );
                                                }
                                            }
                                        }),
                                    );
                                    picker.pwd = node.path_buf.clone();
                                    picker.index = 0;
                                    return;
                                }
                            }
                        }
                    } else {
                        last_left_click = None;
                    }
                    self.last_left_click = last_left_click;
                } else {
                    if let Some((i, t)) = self.last_left_click.as_ref() {
                        if *i == index {
                            if t.elapsed().as_millis() < 500 {
                                self.last_left_click = None;
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::OpenFile(node.path_buf.clone()),
                                    Target::Widget(data.id),
                                ));
                                picker.active = false;
                                return;
                            }
                        }
                    }
                    self.last_left_click = Some((index, std::time::Instant::now()));
                }
                let path = node.path_buf.clone();
                for p in path.ancestors() {
                    picker.update_node_count(&PathBuf::from(p));
                }
                picker.index = index;
            }
        }
    }
}

impl Widget<LapceTabData> for FilePickerExplorer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse_event) => {
                self.mouse_down(ctx, data, mouse_event);
            }
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                let line_height = data.config.editor.line_height as f64;
                let picker = Arc::make_mut(&mut data.picker);
                let pwd = picker.pwd.clone();
                let index =
                    ((mouse_event.pos.y + line_height) / line_height) as usize;
                ctx.request_paint();
                if let Some(item) = picker.get_file_node_mut(&pwd) {
                    let (_, node) = get_item_children(0, index, item);
                    if let Some(node) = node {
                        ctx.set_cursor(&druid::Cursor::Pointer);
                    } else {
                        ctx.clear_cursor();
                    }
                } else {
                    ctx.clear_cursor();
                }
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
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data.picker.root.children_open_count
            != old_data.picker.root.children_open_count
        {
            ctx.request_layout();
        }

        if data.picker.pwd != old_data.picker.pwd {
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
        let height = if let Some(item) = data.picker.get_file_node(&data.picker.pwd)
        {
            (item.children_open_count * data.config.editor.line_height) as f64
        } else {
            bc.max().height
        };
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let line_height = data.config.editor.line_height as f64;

        let size = ctx.size();
        let rect = ctx.region().bounding_box();
        let width = size.width;
        let index = data.picker.index;
        let min = (rect.y0 / line_height).floor() as usize;
        let max = (rect.y1 / line_height) as usize + 2;
        let level = 0;

        self.toggle_rects.clear();

        if let Some(item) = data.picker.get_file_node(&data.picker.pwd) {
            let mut i = 0;
            for item in item.sorted_children() {
                i = paint_file_node_item(
                    ctx,
                    item,
                    min,
                    max,
                    line_height,
                    width,
                    level + 1,
                    i + 1,
                    index,
                    &data.config,
                    &mut self.toggle_rects,
                );
                if i > max {
                    return;
                }
            }
        }
    }
}

pub struct FilePickerControl {
    buttons: Vec<LapceButton>,
}

impl FilePickerControl {
    pub fn new() -> Self {
        Self {
            buttons: Vec::new(),
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for btn in self.buttons.iter() {
            if btn.rect.contains(mouse_event.pos) {
                return true;
            }
        }
        false
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        data: &mut LapceTabData,
        mouse_event: &MouseEvent,
    ) {
        for btn in self.buttons.iter() {
            if btn.rect.contains(mouse_event.pos) {
                if btn.command.is(LAPCE_UI_COMMAND) {
                    let command = btn.command.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::SetWorkspace(workspace) => {
                            if let Some(item) =
                                data.picker.get_file_node(&data.picker.pwd)
                            {
                                let (_, node) =
                                    get_item_children(0, data.picker.index, item);
                                if let Some(node) = node {
                                    if node.is_dir {
                                        let mut workspace = workspace.clone();
                                        workspace.path = Some(node.path_buf.clone());
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::SetWorkspace(workspace),
                                            Target::Auto,
                                        ));
                                    } else {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::OpenFile(
                                                node.path_buf.clone(),
                                            ),
                                            Target::Widget(data.id),
                                        ));
                                        let picker = Arc::make_mut(&mut data.picker);
                                        picker.active = false;
                                    }
                                }
                            }
                        }
                        _ => {
                            ctx.submit_command(btn.command.clone());
                        }
                    }
                }
            }
        }
    }
}

impl Widget<LapceTabData> for FilePickerControl {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.mouse_down(ctx, data, mouse_event);
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
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = Size::new(bc.max().width, 50.0);

        let button_height = 25.0;
        let gap = (self_size.height - button_height) / 2.0;

        self.buttons.clear();
        let mut x = self_size.width - gap;
        let text_layout = ctx
            .text()
            .new_text_layout("Open")
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        let btn_width = text_size.width + gap * 2.0;
        let btn = LapceButton {
            rect: Size::new(text_size.width + gap * 2.0, button_height)
                .to_rect()
                .with_origin(Point::new(x - btn_width, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SetWorkspace((*data.workspace).clone()),
                Target::Auto,
            ),
            text_layout,
        };
        self.buttons.push(btn);

        x -= btn_width + gap;
        let text_layout = ctx
            .text()
            .new_text_layout("Cancel")
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        let btn_width = text_size.width + gap * 2.0;
        let btn = LapceButton {
            rect: Size::new(text_size.width + gap * 2.0, button_height)
                .to_rect()
                .with_origin(Point::new(x - btn_width, gap)),
            command: Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::CancelFilePicker,
                Target::Widget(data.id),
            ),
            text_layout,
        };
        self.buttons.push(btn);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        ctx.stroke(
            Line::new(Point::new(0.0, 0.5), Point::new(size.width, 0.5)),
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );

        for btn in self.buttons.iter() {
            ctx.stroke(
                &btn.rect,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            let text_size = btn.text_layout.size();
            let btn_size = btn.rect.size();
            let x = btn.rect.x0 + (btn_size.width - text_size.width) / 2.0;
            let y = btn.rect.y0 + (btn_size.height - text_size.height) / 2.0;
            ctx.draw_text(&btn.text_layout, Point::new(x, y));
        }
    }
}
