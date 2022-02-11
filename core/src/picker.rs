use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Env, Event, EventCtx, FontFamily, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
    WidgetExt, WidgetId, WidgetPod,
};
use lapce_proxy::dispatch::FileNodeItem;

use crate::{
    config::LapceTheme, data::LapceTabData, explorer::paint_file_node_item,
    scroll::LapceScrollNew,
};

#[derive(Clone)]
pub struct FilePickerData {
    pub widget_id: WidgetId,
    pub active: bool,
    root: FileNodeItem,
    pub home: PathBuf,
    pub pwd: PathBuf,
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
                println!("update node count {path:?} {}", node.children_open_count);
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
        self.pwd.event(ctx, event, data, env);
        self.explorer.event(ctx, event, data, env);
        self.control.event(ctx, event, data, env);
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

pub struct FilePickerPwd {}

impl Widget<LapceTabData> for FilePickerPwd {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
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
        Size::new(bc.max().width, 40.0)
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
    }
}

impl FilePickerPwd {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct FilePickerExplorer {}

impl FilePickerExplorer {
    pub fn new() -> Self {
        Self {}
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
        let index = 0;
        let min = (rect.y0 / line_height).floor() as usize;
        let max = (rect.y1 / line_height) as usize + 2;
        let level = 0;

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
                );
                if i > max {
                    return;
                }
            }
        }
    }
}

pub struct FilePickerControl {}

impl FilePickerControl {
    pub fn new() -> Self {
        Self {}
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
        Size::new(bc.max().width, 40.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {}
}
