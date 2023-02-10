use std::{collections::HashMap, path::Path, sync::Arc};

use druid::{
    menu::MenuEventCtx,
    piet::{Text, TextAttribute, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Env, Event, EventCtx, ExtEventSink, KbKey,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod, WindowId,
};
use itertools::Itertools;
use lapce_core::{command::FocusCommand, meta};
use lapce_data::{
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{ClickMode, LapceConfig, LapceIcons, LapceTheme},
    data::{EditorTabChild, LapceData, LapceEditorData, LapceTabData},
    document::{BufferContent, LocalBufferKind},
    explorer::{FileExplorerData, Naming},
    panel::PanelKind,
    proxy::LapceProxy,
};
use lapce_rpc::{file::FileNodeItem, source_control::FileDiff};

use crate::{
    editor::view::LapceEditorView,
    panel::{LapcePanel, PanelHeaderKind, PanelSizing},
    scroll::LapceScroll,
};

#[allow(clippy::too_many_arguments)]
/// Paint the file node item at its position
fn paint_single_file_node_item(
    ctx: &mut PaintCtx,
    item: &FileNodeItem,
    line_height: f64,
    width: f64,
    level: usize,
    current: usize,
    active: Option<&Path>,
    hovered: Option<usize>,
    config: &LapceConfig,
    toggle_rects: &mut HashMap<usize, Rect>,
    file_diff: Option<FileDiff>,
) {
    let background = if Some(item.path_buf.as_ref()) == active {
        Some(LapceTheme::PANEL_CURRENT_BACKGROUND)
    } else if Some(current) == hovered {
        Some(LapceTheme::PANEL_HOVERED_BACKGROUND)
    } else {
        None
    };

    if let Some(background) = background {
        ctx.fill(
            Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    current as f64 * line_height - line_height,
                ))
                .with_size(Size::new(width, line_height)),
            config.get_color_unchecked(background),
        );
    }

    let text_color = if let Some(diff) = file_diff {
        match diff {
            FileDiff::Modified(_) | FileDiff::Renamed(_, _) => {
                LapceTheme::SOURCE_CONTROL_MODIFIED
            }
            FileDiff::Added(_) => LapceTheme::SOURCE_CONTROL_ADDED,
            FileDiff::Deleted(_) => LapceTheme::SOURCE_CONTROL_REMOVED,
        }
    } else {
        LapceTheme::PANEL_FOREGROUND
    };

    let font_size = config.ui.font_size() as f64;

    let y = current as f64 * line_height - line_height;
    let svg_size = config.ui.icon_size() as f64;
    let svg_y = y + (line_height - svg_size) / 2.0;
    let padding = 15.0 * level as f64;

    if item.is_dir {
        let icon_name = if item.open {
            LapceIcons::ITEM_OPENED
        } else {
            LapceIcons::ITEM_CLOSED
        };
        let svg = config.ui_svg(icon_name);

        let rect = Size::new(svg_size, svg_size)
            .to_rect()
            .with_origin(Point::new(1.0 + padding, svg_y));
        ctx.draw_svg(
            &svg,
            rect,
            Some(config.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
        );
        toggle_rects.insert(current, rect);

        let (svg, svg_color) =
            if let Some((svg, svg_color)) = config.folder_svg(&item.path_buf) {
                (svg, svg_color)
            } else {
                let icon_name = if item.open {
                    LapceIcons::DIRECTORY_OPENED
                } else {
                    LapceIcons::DIRECTORY_CLOSED
                };
                let svg = config.ui_svg(icon_name);
                (
                    svg,
                    Some(config.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
                )
            };
        let rect = Size::new(svg_size, svg_size)
            .to_rect()
            .with_origin(Point::new(1.0 + 16.0 + padding, svg_y));
        ctx.draw_svg(&svg, rect, svg_color);
    } else {
        let (svg, svg_color) = config.file_svg(&item.path_buf);
        let rect = Size::new(svg_size, svg_size)
            .to_rect()
            .with_origin(Point::new(1.0 + 16.0 + padding, svg_y));
        ctx.draw_svg(&svg, rect, svg_color);
    }

    let text_layout = ctx
        .text()
        .new_text_layout(
            item.path_buf
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
        )
        .font(config.ui.font_family(), font_size)
        .text_color(config.get_color_unchecked(text_color).clone())
        .build()
        .unwrap();
    ctx.draw_text(
        &text_layout,
        Point::new(38.0 + padding, y + text_layout.y_offset(line_height)),
    );
}

/// Paint the file node item, if it is in view, and its children
#[allow(clippy::too_many_arguments)]
pub fn paint_file_node_item(
    ctx: &mut PaintCtx,
    env: &Env,
    item: &FileNodeItem,
    min: usize,
    max: usize,
    line_height: f64,
    width: f64,
    level: usize,
    current: usize,
    active: Option<&Path>,
    hovered: Option<usize>,
    naming: Option<&Naming>,
    name_edit_input: &mut NameEditInput,
    drawn_name_input: &mut bool,
    data: &LapceTabData,
    config: &LapceConfig,
    toggle_rects: &mut HashMap<usize, Rect>,
) -> usize {
    if current > max {
        return current;
    }
    if current + item.children_open_count < min {
        return current + item.children_open_count;
    }

    let mut i = current;

    if current >= min {
        let mut should_paint_file_node = true;
        if !*drawn_name_input {
            if let Some(naming) = naming {
                if current == naming.list_index() {
                    draw_name_input(ctx, data, env, &mut i, naming, name_edit_input);
                    *drawn_name_input = true;
                    // If it is renaming then don't draw the underlying file node
                    should_paint_file_node =
                        !matches!(naming, Naming::Renaming { .. })
                }
            }
        }

        if should_paint_file_node {
            paint_single_file_node_item(
                ctx,
                item,
                line_height,
                width,
                level,
                i,
                active,
                hovered,
                config,
                toggle_rects,
                get_item_diff(item, data),
            );
        }
    }

    if item.open {
        for item in item.sorted_children() {
            i = paint_file_node_item(
                ctx,
                env,
                item,
                min,
                max,
                line_height,
                width,
                level + 1,
                i + 1,
                active,
                hovered,
                naming,
                name_edit_input,
                drawn_name_input,
                data,
                config,
                toggle_rects,
            );
            if i > max {
                return i;
            }
        }
    }
    i
}

fn draw_name_input(
    ctx: &mut PaintCtx,
    data: &LapceTabData,
    env: &Env,
    i: &mut usize,
    naming: &Naming,
    name_edit_input: &mut NameEditInput,
) {
    match naming {
        Naming::Renaming { .. } => {
            name_edit_input.paint(ctx, data, env);
        }
        Naming::Naming { .. } | Naming::Duplicating { .. } => {
            name_edit_input.paint(ctx, data, env);
            // Skip forward by an entry
            // This is fine since we aren't using i as an index, but as an offset-multiple in painting
            *i += 1;
        }
    }
}

pub fn get_item_children(
    i: usize,
    index: usize,
    item: &FileNodeItem,
) -> (usize, Option<&FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children(i + 1, index, child);
                if new_index == index {
                    return (new_index, node);
                }
            }
            i += count + 1;
        }
    }
    (i, None)
}

/// Get a FileDiff for the given FileNodeItem. If the given item is a folder
/// that contains changes, returns a "fake" FileDiff that can be used to style
/// the item accordingly.
fn get_item_diff<'data>(
    item: &'data FileNodeItem,
    data: &'data LapceTabData,
) -> Option<FileDiff> {
    if item.is_dir {
        data.source_control
            .file_diffs
            .keys()
            .find(|path| path.as_path().starts_with(&item.path_buf))
            .map(|path| FileDiff::Modified(path.clone()))
    } else {
        data.source_control
            .file_diffs
            .get(&item.path_buf)
            .map(|d| d.0.clone())
    }
}

pub fn get_item_children_mut(
    i: usize,
    index: usize,
    item: &mut FileNodeItem,
) -> (usize, Option<&mut FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children_mut() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children_mut(i + 1, index, child);
                if new_index == index {
                    return (new_index, node);
                }
            }
            i += count + 1;
        }
    }
    (i, None)
}

pub struct FileExplorer {
    widget_id: WidgetId,
    file_list:
        WidgetPod<LapceTabData, LapceScroll<LapceTabData, FileExplorerFileList>>,
    pending_scroll: Option<(f64, f64)>,
    pending_layout: bool,
}

impl FileExplorer {
    pub fn new(data: &mut LapceTabData) -> Self {
        // Create the input editor for renaming/naming files/directories
        let editor = LapceEditorData::new(
            Some(data.file_explorer.renaming_editor_view_id),
            None,
            None,
            BufferContent::Local(LocalBufferKind::PathName),
            &data.config,
        );

        let input = LapceEditorView::new(editor.view_id, editor.editor_id, None)
            .hide_header()
            .hide_gutter()
            .hide_border()
            .set_background_color(LapceTheme::PANEL_HOVERED_BACKGROUND);
        let view_id = editor.view_id;
        data.main_split.editors.insert(view_id, Arc::new(editor));
        // Create the file listing
        let file_list = LapceScroll::new(FileExplorerFileList::new(WidgetPod::new(
            input.boxed(),
        )));

        Self {
            widget_id: data.file_explorer.widget_id,
            file_list: WidgetPod::new(file_list),
            pending_scroll: None,
            pending_layout: true,
        }
    }

    pub fn new_panel(data: &mut LapceTabData) -> LapcePanel {
        let split_id = WidgetId::next();
        LapcePanel::new(
            PanelKind::FileExplorer,
            data.file_explorer.widget_id,
            split_id,
            vec![
                (
                    WidgetId::next(),
                    PanelHeaderKind::Simple("Open Editors".into()),
                    LapceScroll::new(OpenEditorList::new()).boxed(),
                    PanelSizing::Size(200.0),
                ),
                (
                    split_id,
                    PanelHeaderKind::Simple(
                        data.workspace
                            .path
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "No Folder Open".to_string())
                            .into(),
                    ),
                    Self::new(data).boxed(),
                    PanelSizing::Flex(true),
                ),
            ],
        )
    }
}

impl Widget<LapceTabData> for FileExplorer {
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

                if let LapceUICommand::ScrollTo(point) = command {
                    self.pending_scroll = Some(point.to_owned());
                    self.pending_layout = true;
                    ctx.request_anim_frame();
                    return;
                }
            }

            Event::AnimFrame(_) => {
                if self.pending_layout {
                    ctx.request_anim_frame();
                } else {
                    // make sure layout is updated before we scroll
                    if let Some(scroll) = self.pending_scroll.take() {
                        let target = Point::from(scroll);

                        self.file_list.widget_mut().scroll_to(target);
                    }
                }
            }

            _ => (),
        }

        self.file_list.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.file_list.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.file_list.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        self.file_list.layout(ctx, bc, data, env);
        self.file_list
            .set_origin(ctx, data, env, Point::new(0.0, 0.0));
        self.pending_layout = false;
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.file_list.paint(ctx, data, env);
    }
}

type NameEditInput = WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>;

struct FileExplorerFileList {
    line_height: f64,
    hovered: Option<usize>,
    name_edit_input: NameEditInput,
}

impl FileExplorerFileList {
    pub fn new(input: NameEditInput) -> Self {
        Self {
            line_height: 25.0,
            hovered: None,
            name_edit_input: input,
        }
    }

    pub fn reveal_path(
        &self,
        path: &Path,
        file_explorer: &mut FileExplorerData,
        tab_id: WidgetId,
        proxy: &LapceProxy,
        target: Target,
        ctx: &mut EventCtx,
    ) {
        let paths = match file_explorer.node_tree(path) {
            Some(paths) => paths,
            None => return,
        };

        for node_path in paths.iter().rev() {
            log::debug!("visiting node: {}", node_path.display());

            let node = file_explorer.get_node_mut(node_path).unwrap();

            if !node.is_dir {
                continue;
            }

            if !node.read {
                let event_sink = ctx.get_external_handle();
                let event_sink_read = ctx.get_external_handle();
                let path = path.to_path_buf();

                FileExplorerData::read_dir_cb(
                    &node.path_buf,
                    true,
                    tab_id,
                    proxy,
                    event_sink_read,
                    Some(move || {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ExplorerRevealPath { path },
                            target,
                        );
                    }),
                );

                return;
            }

            if !node.open {
                node.open = true;

                let path = node.path_buf.clone();

                for current_path in path.ancestors() {
                    file_explorer.update_node_count(current_path);
                }
            }
        }

        let index = file_explorer.get_node_index(path);

        if let Some(index) = index {
            let point = Point::new(0f64, (index - 3) as f64 * self.line_height);

            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ScrollTo(point.into()),
                target,
            ));
        }
    }
}

impl Widget<LapceTabData> for FileExplorerFileList {
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

                match command {
                    LapceUICommand::ActiveFileChanged { path } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        file_explorer.active_selected = path.clone();
                        ctx.request_paint();
                    }
                    LapceUICommand::FileExplorerRefresh => {
                        data.file_explorer.reload();
                    }

                    LapceUICommand::ExplorerRevealPath { path } => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);

                        self.reveal_path(
                            path,
                            file_explorer,
                            data.id,
                            data.proxy.as_ref(),
                            cmd.target(),
                            ctx,
                        );
                    }
                    _ => (),
                }
            }
            _ => {}
        }

        // Finish any renaming if the user presses enter
        if let Event::KeyDown(key_ev) = event {
            if self.name_edit_input.has_focus() {
                if key_ev.key == KbKey::Enter {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ExplorerEndNaming { apply_naming: true },
                        Target::Auto,
                    ));
                } else if key_ev.key == KbKey::Escape {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ExplorerEndNaming {
                            apply_naming: false,
                        },
                        Target::Auto,
                    ));
                }
            }
        }

        if data.file_explorer.naming.is_some() {
            self.name_edit_input.event(ctx, event, data, env);
            // If the input handled the event, then we just ignore it.
            if ctx.is_handled() {
                return;
            }
        }

        // We can catch these here because they'd be consumed by name edit input if they were for/on it
        if matches!(
            event,
            Event::MouseDown(_) | Event::KeyUp(_) | Event::KeyDown(_)
        ) && data.file_explorer.naming.is_some()
            && !self.name_edit_input.has_focus()
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ExplorerEndNaming { apply_naming: true },
                Target::Auto,
            ));
            return;
        }

        match event {
            Event::MouseMove(mouse_event) => {
                if !ctx.is_hot() {
                    return;
                }

                if let Some(workspace) = data.file_explorer.workspace.as_ref() {
                    let y = mouse_event.pos.y;
                    if y <= self.line_height
                        * (workspace.children_open_count + 1 + 1) as f64
                    {
                        ctx.set_cursor(&Cursor::Pointer);
                        let hovered = Some(
                            ((mouse_event.pos.y + self.line_height)
                                / self.line_height)
                                as usize,
                        );

                        if hovered != self.hovered {
                            ctx.request_paint();
                            self.hovered = hovered;
                        }
                    } else {
                        ctx.clear_cursor();
                        self.hovered = None;
                    }
                }
            }
            Event::MouseDown(mouse_event) => {
                if !ctx.is_hot() {
                    return;
                }

                let double_click_mode = data.config.editor.double_click.clone();
                let file_explorer = Arc::make_mut(&mut data.file_explorer);
                let index = ((mouse_event.pos.y + self.line_height)
                    / self.line_height) as usize;

                if mouse_event.button.is_left() {
                    if let Some((_, node)) =
                        file_explorer.get_node_by_index_mut(index)
                    {
                        if node.is_dir {
                            let cont_open = !(matches!(
                                double_click_mode,
                                ClickMode::DoubleClickAll
                            ) && mouse_event.count < 2);
                            if cont_open {
                                if node.read {
                                    node.open = !node.open;
                                } else {
                                    let tab_id = data.id;
                                    let event_sink = ctx.get_external_handle();
                                    FileExplorerData::read_dir(
                                        &node.path_buf,
                                        true,
                                        tab_id,
                                        &data.proxy,
                                        event_sink,
                                    );
                                }
                                let path = node.path_buf.clone();
                                if let Some(paths) = file_explorer.node_tree(&path) {
                                    for path in paths.iter() {
                                        file_explorer.update_node_count(path);
                                    }
                                }
                            }
                        } else {
                            let cont_open =
                                matches!(double_click_mode, ClickMode::SingleClick)
                                    || mouse_event.count > 1;
                            if cont_open {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::OpenFile(
                                        node.path_buf.clone(),
                                        false,
                                    ),
                                    Target::Widget(data.id),
                                ));
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::ActiveFileChanged {
                                        path: Some(node.path_buf.clone()),
                                    },
                                    Target::Widget(file_explorer.widget_id),
                                ));
                            }
                        }
                    }
                }

                if mouse_event.button.is_right() {
                    if let Some((indent_level, node)) = file_explorer
                        .get_node_by_index(index)
                        .or_else(|| file_explorer.workspace.as_ref().map(|x| (0, x)))
                    {
                        let workspace_path = file_explorer
                            .workspace
                            .as_ref()
                            .map(|x| &x.path_buf)
                            .unwrap();
                        let is_workspace = &node.path_buf == workspace_path;

                        // The folder that it is, or is within
                        let base = if node.is_dir {
                            Some(node.path_buf.clone())
                        } else {
                            node.path_buf.parent().map(ToOwned::to_owned)
                        };

                        // If there's no reasonable path at the point, then ignore it
                        let base = if let Some(base) = base {
                            base
                        } else {
                            return;
                        };

                        // Create a context menu with different actions that can be performed on a file/dir
                        // or in the directory
                        let mut menu = druid::Menu::<LapceData>::new("Explorer");

                        // The ids are so that the correct LapceTabData can be acquired inside the menu event cb
                        // since the context menu only gets access to LapceData
                        let window_id = *data.window_id;
                        let tab_id = data.id;
                        let item = druid::MenuItem::new("New File").on_activate(
                            make_new_file_cb(
                                ctx,
                                &base,
                                window_id,
                                tab_id,
                                is_workspace,
                                index,
                                indent_level,
                                false,
                            ),
                        );

                        menu = menu.entry(item);

                        let item = druid::MenuItem::new("New Directory")
                            .on_activate(make_new_file_cb(
                                ctx,
                                &base,
                                window_id,
                                tab_id,
                                is_workspace,
                                index,
                                indent_level,
                                true,
                            ));
                        menu = menu.entry(item);

                        // Separator between non destructive and destructive actions
                        menu = menu.separator();

                        if !data.workspace.kind.is_remote() {
                            let item =
                                druid::MenuItem::new("Reveal in file explorer")
                                    .command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::RevealInFileExplorer(
                                            node.path_buf.clone(),
                                        ),
                                        Target::Auto,
                                    ));
                            menu = menu.entry(item);
                        }

                        // Don't allow us to rename or delete the current workspace
                        if !is_workspace {
                            let item = druid::MenuItem::new("Rename").command(
                                Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::ExplorerStartRename {
                                        list_index: index,
                                        indent_level,
                                        text: node
                                            .path_buf
                                            .file_name()
                                            .map(|x| x.to_string_lossy().to_string())
                                            .unwrap_or_else(String::new),
                                    },
                                    Target::Auto,
                                ),
                            );
                            menu = menu.entry(item);

                            if !node.is_dir {
                                let item = druid::MenuItem::new("Duplicate")
                                    .command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::ExplorerStartDuplicate {
                                            list_index: index,
                                            indent_level,
                                            base_path: node
                                                .path_buf
                                                .parent()
                                                .expect("file without parent")
                                                .to_owned(),
                                            name: node
                                                .path_buf
                                                .file_name()
                                                .expect("file without name")
                                                .to_string_lossy()
                                                .into_owned(),
                                        },
                                        Target::Auto,
                                    ));
                                menu = menu.entry(item);
                            }

                            let trash_text = if node.is_dir {
                                "Move Directory to Trash"
                            } else {
                                "Move File to Trash"
                            };
                            let item = druid::MenuItem::new(trash_text).command(
                                Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::TrashPath {
                                        path: node.path_buf.clone(),
                                    },
                                    Target::Auto,
                                ),
                            );
                            menu = menu.entry(item);
                        }

                        menu = menu.separator();
                        let path_to_file = node.path_buf.clone();
                        let item =
                            druid::MenuItem::new("Copy Path").command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::PutToClipboard(
                                    path_to_file.to_str().unwrap().to_string(),
                                ),
                                Target::Auto,
                            ));
                        menu = menu.entry(item);

                        let relative_path = node
                            .path_buf
                            .strip_prefix(workspace_path)
                            .unwrap()
                            .to_path_buf();
                        let item = druid::MenuItem::new("Copy Relative Path")
                            .command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::PutToClipboard(
                                    relative_path.to_str().unwrap().to_string(),
                                ),
                                Target::Auto,
                            ));
                        menu = menu.entry(item);

                        menu = menu.separator();
                        let item =
                            druid::MenuItem::new("Refresh").command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::FileExplorerRefresh,
                                Target::Auto,
                            ));
                        menu = menu.entry(item);

                        ctx.show_context_menu::<LapceData>(
                            menu,
                            ctx.to_window(mouse_event.pos),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::HotChanged(false) = event {
            self.hovered = None;
        }

        self.name_edit_input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if data
            .file_explorer
            .workspace
            .as_ref()
            .map(|w| w.children_open_count)
            != old_data
                .file_explorer
                .workspace
                .as_ref()
                .map(|w| w.children_open_count)
        {
            ctx.request_layout();
        }

        if data.file_explorer.naming.is_some() {
            self.name_edit_input.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        if let Some(naming) = &data.file_explorer.naming {
            let (&index, &level) = match naming {
                Naming::Renaming {
                    list_index,
                    indent_level,
                }
                | Naming::Naming {
                    list_index,
                    indent_level,
                    ..
                }
                | Naming::Duplicating {
                    list_index,
                    indent_level,
                    ..
                } => (list_index, indent_level),
            };

            let max = bc.max();
            let input_bc = bc.shrink(Size::new(max.width / 2.0, 0.0));
            self.name_edit_input.layout(ctx, &input_bc, data, env);

            let y_pos = (index as f64 * self.line_height) - self.line_height;
            let x_pos = 38.0 + (15.0 * level as f64);
            self.name_edit_input.set_origin(
                ctx,
                data,
                env,
                Point::new(x_pos, y_pos),
            );
        }

        let mut height = data
            .file_explorer
            .workspace
            .as_ref()
            .map(|w| w.children_open_count)
            .unwrap_or(0);
        if matches!(data.file_explorer.naming, Some(Naming::Naming { .. })) {
            height += 1;
        }
        let height = height as f64 * self.line_height;
        // Choose whichever one is larger
        // We want to use bc.max().height when the number of entries is smaller than the window
        // height, because receiving right click events requires reporting that we fill the panel
        let height = height.max(bc.max().height);

        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.region().bounding_box();
        let size = ctx.size();
        let width = size.width;
        let active = data.file_explorer.active_selected.as_deref();
        let min = (rect.y0 / self.line_height).floor() as usize;
        let max = (rect.y1 / self.line_height) as usize + 2;
        let level = 0;
        let mut drawn_name_input = false;

        if let Some(item) = data.file_explorer.workspace.as_ref() {
            let mut i = 0;
            for item in item.sorted_children() {
                i = paint_file_node_item(
                    ctx,
                    env,
                    item,
                    min,
                    max,
                    self.line_height,
                    width,
                    level + 1,
                    i + 1,
                    active,
                    self.hovered,
                    data.file_explorer.naming.as_ref(),
                    &mut self.name_edit_input,
                    &mut drawn_name_input,
                    data,
                    &data.config,
                    &mut HashMap::new(),
                );
                if i > max {
                    return;
                }
            }

            // If we didn't draw the name input then we'll have to draw it here
            if let Some(naming) = &data.file_explorer.naming {
                if i == 0
                    || (naming.list_index() >= min && naming.list_index() < max)
                {
                    draw_name_input(
                        ctx,
                        data,
                        env,
                        // This value does not matter here
                        &mut 0,
                        naming,
                        &mut self.name_edit_input,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Create a callback for the context menu when creating a file/directory
/// This is the same function for both, besides one change in parameter
fn make_new_file_cb(
    ctx: &mut EventCtx,
    base: &Path,
    window_id: WindowId,
    tab_id: WidgetId,
    is_workspace: bool,
    index: usize,
    indent_level: usize,
    is_dir: bool,
) -> impl FnMut(&mut MenuEventCtx, &mut LapceData, &Env) + 'static {
    // If the node we're on is the workspace then we'll appear at the very start
    let display_index = if is_workspace { 1 } else { index + 1 };

    let event_sink = ctx.get_external_handle();
    let base_path = base.to_owned();
    move |_ctx, data: &mut LapceData, _env| {
        // Clone the handle within, since on_active is an FnMut, so we can't move it into the second
        // closure
        let event_sink = event_sink.clone();
        let base_path = base_path.clone();

        // Acquire the LapceTabData instance we were within
        let tab_data = data
            .windows
            .get_mut(&window_id)
            .unwrap()
            .tabs
            .get_mut(&tab_id)
            .unwrap();

        // Expand the directory, if it is one and if it needs to
        expand_dir(
            event_sink.clone(),
            &tab_data.proxy,
            tab_id,
            Arc::make_mut(&mut tab_data.file_explorer),
            index,
            move || {
                // After we send the command to update the directory, we submit the command to display the new file
                // input box
                // We ignore any error coming from submit command as failing here shouldn't crash lapce
                let res = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ExplorerNew {
                        list_index: display_index,
                        indent_level,
                        is_dir,
                        base_path,
                    },
                    Target::Auto,
                );

                if let Err(err) = res {
                    log::warn!(
                        "Failed to start constructing new/file directory: {:?}",
                        err
                    );
                }
            },
        );
    }
}

/// Expand the directory in the view
/// `on_finished` is called when its done, but the files in the list are not yet updated
/// but the command has been sent. This lets the user queue commands to occur right after it.
/// Note: `on_finished` is also called when there is no dir it didn't need reading
fn expand_dir(
    event_sink: ExtEventSink,
    proxy: &LapceProxy,
    tab_id: WidgetId,
    file_explorer: &mut FileExplorerData,
    index: usize,
    on_finished: impl FnOnce() + Send + 'static,
) {
    if let Some((_, node)) = file_explorer.get_node_by_index_mut(index) {
        if node.is_dir {
            if node.read {
                node.open = true;
                on_finished();
            } else {
                FileExplorerData::read_dir_cb(
                    &node.path_buf,
                    true,
                    tab_id,
                    proxy,
                    event_sink,
                    Some(on_finished),
                );
            }
            let path = node.path_buf.clone();
            if let Some(paths) = file_explorer.node_tree(&path) {
                for path in paths.iter() {
                    file_explorer.update_node_count(path);
                }
            }
        } else {
            on_finished();
        }
    } else {
        on_finished();
    }
}

struct OpenEditorList {
    line_height: f64,
    mouse_pos: Point,
    in_view_tab_children: HashMap<usize, (Rect, WidgetId)>,
    mouse_down: Option<(usize, Option<Rect>)>,
    hover_index: Option<usize>,
}

impl OpenEditorList {
    fn new() -> Self {
        Self {
            line_height: 25.0,
            mouse_pos: Point::ZERO,
            in_view_tab_children: HashMap::new(),
            mouse_down: None,
            hover_index: None,
        }
    }

    fn paint_editor(
        &mut self,
        ctx: &mut PaintCtx,
        i: usize,
        data: &LapceTabData,
        child: &EditorTabChild,
        active: bool,
    ) {
        let size = ctx.size();
        let mut text = "".to_string();
        let mut hint = "".to_string();
        let mut svg = data.config.ui_svg(LapceIcons::FILE);
        let mut svg_color = Some(
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
        );
        let mut pristine = true;
        match child {
            EditorTabChild::Editor(view_id, _, _) => {
                let editor_buffer = data.editor_view_content(*view_id);
                pristine = editor_buffer.doc.buffer().is_pristine();

                if let BufferContent::File(path) = &editor_buffer.editor.content {
                    (svg, svg_color) = data.config.file_svg(path);
                    if let Some(file_name) = path.file_name() {
                        if let Some(s) = file_name.to_str() {
                            text = s.to_string();
                        }
                    }
                    let mut path = path.to_path_buf();
                    if let Some(workspace_path) = data.workspace.path.as_ref() {
                        path = path
                            .strip_prefix(workspace_path)
                            .unwrap_or(&path)
                            .to_path_buf();
                    }
                    // TODO: Can be updated to use a disambiguation algorithm.
                    // For example when opening multiple lib.rs which usually sits
                    // under src/
                    hint = path
                        .parent()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                } else if let BufferContent::Scratch(..) =
                    &editor_buffer.editor.content
                {
                    text = editor_buffer.editor.content.file_name().to_string();
                }
                if let Some(_compare) = editor_buffer.editor.compare.as_ref() {
                    text = format!("{text} (Working tree)");
                }
            }
            EditorTabChild::Settings { .. } => {
                text = "Settings".to_string();
                hint = format!("ver. {}", *meta::VERSION);
                svg = data.config.ui_svg(LapceIcons::SETTINGS);
            }
            EditorTabChild::Plugin { volt_name, .. } => {
                text = format!("Plugin: {volt_name}");
                svg = data.config.ui_svg(LapceIcons::EXTENSIONS);
            }
        }

        let font_size = data.config.ui.font_size() as f64;

        let current_item = Size::new(size.width, self.line_height)
            .to_rect()
            .with_origin(Point::new(0.0, i as f64 * self.line_height));

        if ctx.is_hot()
            && i == (self.mouse_pos.y / self.line_height).floor() as usize
        {
            ctx.fill(
                current_item,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_HOVERED_BACKGROUND),
            );
        } else if active {
            ctx.fill(
                current_item,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_CURRENT_BACKGROUND),
            );
        }

        let svg_size = data.config.ui.icon_size() as f64;
        let close_rect =
            Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(
                    10.0 + (self.line_height - svg_size) / 2.0,
                    i as f64 * self.line_height
                        + (self.line_height - svg_size) / 2.0,
                ));

        self.in_view_tab_children
            .insert(i, (close_rect.inflate(2.0, 2.0), child.widget_id()));

        let close_svg = if ctx.is_hot() && current_item.contains(self.mouse_pos) {
            if close_rect.inflate(2.0, 2.0).contains(self.mouse_pos) {
                ctx.fill(
                    close_rect.inflate(2.0, 2.0),
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_CURRENT_BACKGROUND),
                );
            }
            Some(data.config.ui_svg(LapceIcons::CLOSE))
        } else if pristine {
            None
        } else {
            Some(data.config.ui_svg(LapceIcons::UNSAVED))
        };
        if let Some(close_svg) = close_svg {
            ctx.draw_svg(
                &close_svg,
                close_rect,
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
                ),
            );
        }

        let svg_rect =
            Size::new(svg_size, svg_size)
                .to_rect()
                .with_origin(Point::new(
                    10.0 + self.line_height,
                    i as f64 * self.line_height
                        + (self.line_height - svg_size) / 2.0,
                ));
        ctx.draw_svg(&svg, svg_rect, svg_color);

        if !hint.is_empty() {
            text = format!("{text} {hint}");
        }
        let total_len = text.len();
        let mut text_layout = ctx
            .text()
            .new_text_layout(text)
            .font(data.config.ui.font_family(), font_size)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_FOREGROUND)
                    .clone(),
            );
        if !hint.is_empty() {
            text_layout = text_layout.range_attribute(
                total_len - hint.len()..total_len,
                TextAttribute::TextColor(
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_FOREGROUND_DIM)
                        .clone(),
                ),
            );
        }
        let text_layout = text_layout.build().unwrap();
        ctx.draw_text(
            &text_layout,
            Point::new(
                svg_rect.x1 + 5.0,
                i as f64 * self.line_height + text_layout.y_offset(self.line_height),
            ),
        );
    }
}

impl Widget<LapceTabData> for OpenEditorList {
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
                let index = (mouse_event.pos.y / self.line_height).floor() as usize;
                let hover_index = self.hover_index;
                if self.in_view_tab_children.contains_key(&index) {
                    self.hover_index = Some(index);
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    self.hover_index = None;
                    ctx.clear_cursor();
                }
                if hover_index != self.hover_index {
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down = None;
                let index = (mouse_event.pos.y / self.line_height).floor() as usize;
                if let Some((close_rect, _)) = self.in_view_tab_children.get(&index)
                {
                    self.mouse_down = Some((
                        index,
                        if close_rect.contains(mouse_event.pos) {
                            Some(*close_rect)
                        } else {
                            None
                        },
                    ));
                }
            }
            Event::MouseUp(mouse_event) => {
                if let Some((down_index, down_close_rect)) = self.mouse_down {
                    let index =
                        (mouse_event.pos.y / self.line_height).floor() as usize;
                    if index == down_index {
                        if let Some((close_rect, widget_id)) =
                            self.in_view_tab_children.get(&index)
                        {
                            if down_close_rect.is_some()
                                && close_rect.contains(mouse_event.pos)
                            {
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
                            } else {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(*widget_id),
                                ));
                            }
                        }
                    }
                }
                self.mouse_down = None;
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
        let n = data
            .main_split
            .editor_tabs
            .iter()
            .map(|(_, tab)| tab.children.len())
            .sum::<usize>();

        // If there are split editor tabs, then we need to consider the group headers
        let n = if data.main_split.editor_tabs.len() > 1 {
            n + data.main_split.editor_tabs.len()
        } else {
            n
        };
        Size::new(bc.max().width, self.line_height * n as f64)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        self.in_view_tab_children.clear();
        let rect = ctx.region().bounding_box();
        let mut i = 0;
        let mut g = 0;

        for (_, tab) in
            data.main_split
                .editor_tabs
                .iter()
                .sorted_by(|(_, a), (_, b)| {
                    let a_rect = a.layout_rect.borrow();
                    let b_rect = b.layout_rect.borrow();

                    if a_rect.y0 == b_rect.y0 {
                        a_rect.x0.total_cmp(&b_rect.x0)
                    } else {
                        a_rect.y0.total_cmp(&b_rect.y0)
                    }
                })
        {
            if data.main_split.editor_tabs.len() > 1 {
                if self.line_height * (i as f64) > rect.y1 {
                    return;
                }
                g += 1;
                if self.line_height * ((i + 1) as f64) >= rect.y0 {
                    let text_layout = ctx
                        .text()
                        .new_text_layout(format!("Group {g}"))
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
                            10.0,
                            (i as f64 * self.line_height)
                                + text_layout.y_offset(self.line_height),
                        ),
                    );
                }
                i += 1;
            }
            for (child_index, child) in tab.children.iter().enumerate() {
                if self.line_height * ((i + 1) as f64) < rect.y0 {
                    i += 1;
                    continue;
                }
                if self.line_height * (i as f64) > rect.y1 {
                    return;
                }
                let active = *data.main_split.active_tab == Some(tab.widget_id)
                    && tab.active == child_index;
                self.paint_editor(ctx, i, data, child, active);
                i += 1;
            }
        }
    }
}
